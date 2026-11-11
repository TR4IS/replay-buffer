//! The settings page: a tiny HTTP server on 127.0.0.1 that serves one page
//! (ui.html) and a small JSON API. Only this PC can reach it, and every API
//! call must carry a random per-run token that only the served page knows, so
//! websites can't poke it.

use crate::app::App;
use crate::config::{self, Config};
use serde_json::{Value, json};
use std::hash::{BuildHasher, Hasher};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::Ordering;

const PAGE: &str = include_str!("ui.html");

fn random_token() -> String {
    let mut s = String::new();
    for _ in 0..2 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(crate::capture::qpc_now() as u64);
        s.push_str(&format!("{:016x}", h.finish()));
    }
    s
}

/// Starts the server; returns the port it listens on.
pub fn start(app: Arc<App>, want_port: u16) -> Option<u16> {
    let mut listener = None;
    for port in want_port..want_port + 10 {
        if let Ok(l) = TcpListener::bind(("127.0.0.1", port)) {
            listener = Some((l, port));
            break;
        }
    }
    let Some((listener, port)) = listener else {
        crate::log!("settings page: no free port near {want_port}");
        return None;
    };
    let token = Arc::new(random_token());
    crate::log!("settings page: http://127.0.0.1:{port}/");
    std::thread::Builder::new()
        .name("ui".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let (app, token) = (app.clone(), token.clone());
                std::thread::spawn(move || {
                    let _ = handle(stream, &app, &token, port);
                });
            }
        })
        .ok()?;
    Some(port)
}

fn handle(mut s: TcpStream, app: &Arc<App>, token: &str, port: u16) -> std::io::Result<()> {
    s.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
    let mut r = BufReader::new(s.try_clone()?);
    let mut line = String::new();
    r.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("/").to_string();
    let (mut len, mut host, mut tok) = (0usize, String::new(), String::new());
    loop {
        let mut h = String::new();
        if r.read_line(&mut h)? == 0 || h == "\r\n" || h == "\n" {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            let v = v.trim().to_string();
            match k.trim().to_ascii_lowercase().as_str() {
                "content-length" => len = v.parse().unwrap_or(0).min(1 << 20),
                "host" => host = v,
                "x-token" => tok = v,
                _ => {}
            }
        }
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;

    // Refuse anything not addressed to us by name (blocks DNS-rebinding tricks).
    if host != format!("127.0.0.1:{port}") && host != format!("localhost:{port}") {
        return reply(&mut s, 403, "text/plain", b"forbidden");
    }
    if method == "GET" && (path == "/" || path.starts_with("/?")) {
        let page = PAGE.replace("__TOKEN__", token);
        return reply(&mut s, 200, "text/html; charset=utf-8", page.as_bytes());
    }
    if !path.starts_with("/api/") {
        return reply(&mut s, 404, "text/plain", b"not found");
    }
    if tok != token {
        return reply(&mut s, 403, "text/plain", b"bad token");
    }
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let out = match (method.as_str(), path.as_str()) {
        ("GET", "/api/state") => state(app),
        ("POST", "/api/save") => {
            crate::app::save(app);
            json!({"ok": true})
        }
        ("POST", "/api/config") => match serde_json::from_value::<Config>(body) {
            Ok(c) => {
                let c = config::sanitize(c);
                match config::save(&c) {
                    Ok(()) => {
                        app.request_restart();
                        json!({"ok": true})
                    }
                    Err(e) => json!({"ok": false, "error": format!("Couldn't write settings: {e}")}),
                }
            }
            Err(e) => json!({"ok": false, "error": format!("Invalid settings: {e}")}),
        },
        ("POST", "/api/synctest") => {
            crate::app::sync_test(app);
            json!({"ok": true})
        }
        ("POST", "/api/open") => open(app, &body),
        _ => json!({"ok": false, "error": "unknown request"}),
    };
    reply(&mut s, 200, "application/json", out.to_string().as_bytes())
}

fn reply(s: &mut TcpStream, code: u16, ctype: &str, body: &[u8]) -> std::io::Result<()> {
    let status = match code {
        200 => "OK",
        403 => "Forbidden",
        _ => "Not Found",
    };
    write!(
        s,
        "HTTP/1.1 {code} {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    s.write_all(body)?;
    s.flush()
}

fn state(app: &Arc<App>) -> Value {
    let rec = app.rec.lock().unwrap();
    let st = &rec.stats;
    let (held, bytes) = {
        let s = rec.store.lock().unwrap();
        (s.held_seconds(), s.held_bytes())
    };
    let audio: Vec<Value> = rec
        .tracks
        .iter()
        .map(|t| {
            json!({
                "source": format!("{:?}", t.source).to_lowercase(),
                "device": *t.device.lock().unwrap(),
                "active": t.active.load(Ordering::Relaxed),
                "peak": t.take_peak(),
                "glitches": t.glitches.load(Ordering::Relaxed),
            })
        })
        .collect();
    let monitors: Vec<Value> = crate::capture::list_monitors()
        .into_iter()
        .map(|(n, w, h, p)| json!({"n": n, "w": w, "h": h, "primary": p}))
        .collect();
    let error = rec.last_error.lock().unwrap().clone();
    json!({
        "now": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0),
        "recording": rec.timeline.lock().unwrap().is_some() && error.is_none(),
        "error": error,
        "held": held,
        "target": rec.cfg.replay_minutes as f64 * 60.0,
        "bytes": bytes,
        "frames": st.frames.load(Ordering::Relaxed),
        "updates": st.new_frames.load(Ordering::Relaxed),
        "dropped": st.dropped.load(Ordering::Relaxed),
        "encoded_bytes": st.bytes.load(Ordering::Relaxed),
        "width": st.width.load(Ordering::Relaxed),
        "height": st.height.load(Ordering::Relaxed),
        "saving": rec.saving.load(Ordering::Relaxed),
        "hotkey_ok": app.hotkey_ok.load(Ordering::Relaxed),
        "audio": audio,
        "config": rec.cfg,
        "desktop_delay": rec.desktop_delay.load(Ordering::Relaxed),
        "monitors": monitors,
        "last_save": app.last_save.lock().unwrap().clone(),
        "sync": app.sync.lock().unwrap().clone(),
        "clips": clips(&rec.cfg.save_dir),
    })
}

fn clips(dir: &std::path::Path) -> Vec<Value> {
    let mut v: Vec<(u64, String, u64)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.to_ascii_lowercase().ends_with(".mp4") {
                return None;
            }
            let m = e.metadata().ok()?;
            let t = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
            Some((t, name, m.len()))
        })
        .collect();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    v.into_iter().take(6).map(|(t, name, size)| json!({"name": name, "size": size, "time": t})).collect()
}

fn open(app: &Arc<App>, body: &Value) -> Value {
    let dir = app.rec.lock().unwrap().cfg.save_dir.clone();
    match body.get("what").and_then(|v| v.as_str()) {
        Some("folder") => {
            let _ = std::fs::create_dir_all(&dir);
            crate::app::open_path(&dir);
            json!({"ok": true})
        }
        Some("clip") => {
            let name = body.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let p = dir.join(name);
            if name.contains(['/', '\\']) || name.contains("..") || !p.is_file() {
                return json!({"ok": false, "error": "That clip isn't in the replays folder anymore."});
            }
            crate::app::open_path(&p);
            json!({"ok": true})
        }
        _ => json!({"ok": false, "error": "unknown target"}),
    }
}

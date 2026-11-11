#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod buffer;
mod capture;
mod config;
mod log;
mod recorder;
mod selftest;
mod tsmux;
mod ui;

use app::App;
use recorder::Recorder;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIconBuilder};
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
use windows::Win32::Media::{timeBeginPeriod, timeEndPeriod};
use windows::Win32::System::Threading::{
    BELOW_NORMAL_PRIORITY_CLASS, CreateMutexW, GetCurrentProcess, GetCurrentThreadId, SetPriorityClass,
};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

const HOTKEY_ID: i32 = 1;

fn main() {
    log::init();
    let args: Vec<String> = std::env::args().collect();

    // Only one copy at a time.
    let _mutex = unsafe { CreateMutexW(None, true, w!("ReplayBuffer.SingleInstance")) };
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        log!("already running");
        return;
    }

    unsafe {
        // Desktop Duplication's newer API (no hidden format conversion) needs this.
        let _ = windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        );
        // Games always win the CPU; our capture thread raises itself back to normal.
        let _ = SetPriorityClass(GetCurrentProcess(), BELOW_NORMAL_PRIORITY_CLASS);
        // 1 ms timer resolution for precise frame pacing.
        timeBeginPeriod(1);
    }

    if args.iter().any(|a| a == "--synctest") {
        let rec = Recorder::start(config::load());
        std::thread::sleep(std::time::Duration::from_secs(2));
        match rec.tracks.iter().find(|t| t.source == audio::Source::Desktop) {
            Some(t) => log!("sync test result: {:?}", selftest::measure_desktop_delay(t)),
            None => log!("desktop audio is off"),
        }
    } else if let Some(i) = args.iter().position(|a| a == "--test") {
        let secs: u64 = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(15);
        test_run(secs, args.iter().any(|a| a == "--flash"));
    } else {
        tray_app();
    }
    unsafe {
        timeEndPeriod(1);
    }
}

/// Records for `secs`, saves a replay, prints stats, exits. For testing.
fn test_run(secs: u64, flash: bool) {
    let cfg = config::load();
    log!("test run: {secs}s, config {:?}", cfg);
    let rec = Recorder::start(cfg);
    let start = std::time::Instant::now();
    let flash_stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flasher = flash.then(|| selftest::spawn(flash_stop.clone()));
    std::thread::sleep(std::time::Duration::from_secs(secs));
    flash_stop.store(true, Ordering::Relaxed);
    if let Some(f) = flasher {
        let _ = f.join();
    }
    let st = &rec.stats;
    let el = start.elapsed().as_secs_f64();
    let frames = st.frames.load(Ordering::Relaxed);
    log!(
        "stats: {frames} frames in {el:.1}s ({:.1} fps), {} desktop updates, {} repeats, {} dropped slots, {} merged game frames, {:.1} Mbps",
        frames as f64 / el,
        st.new_frames.load(Ordering::Relaxed),
        st.repeats.load(Ordering::Relaxed),
        st.dropped.load(Ordering::Relaxed),
        st.merged.load(Ordering::Relaxed),
        st.bytes.load(Ordering::Relaxed) as f64 * 8.0 / el / 1e6
    );
    for t in &rec.tracks {
        log!("{:?} audio: {} discontinuities", t.source, t.glitches.load(Ordering::Relaxed));
    }
    let (tx, rx) = std::sync::mpsc::channel();
    rec.save(move |r| {
        let _ = tx.send(r);
    });
    match rx.recv() {
        Ok(Ok((p, s))) => log!("saved {} ({s:.0}s buffered)", p.display()),
        Ok(Err(e)) => log!("save failed: {e}"),
        Err(_) => log!("save thread died"),
    }
    if let Some(e) = rec.last_error.lock().unwrap().as_ref() {
        log!("last error: {e}");
    }
}

fn tray_app() {
    let cfg = config::load();
    let hotkey_text = cfg.hotkey.clone();
    let want_port = cfg.ui_port;
    let app = Arc::new(App {
        rec: Mutex::new(Recorder::start(cfg)),
        last_save: Mutex::new(None),
        sync: Mutex::new(app::SyncState::Idle),
        hotkey_ok: AtomicBool::new(false),
        main_thread: unsafe { GetCurrentThreadId() },
    });
    let hotkey_ok = register_hotkey(&hotkey_text);
    app::hotkey_ok_flag(&app, hotkey_ok);
    let port = ui::start(app.clone(), want_port);
    let ui_url = port.map(|p| format!("http://127.0.0.1:{p}/"));

    let save_item = MenuItem::new(format!("Save replay	{hotkey_text}"), true, None);
    let settings_item = MenuItem::new("Settings", ui_url.is_some(), None);
    let folder_item = MenuItem::new("Open replays folder", true, None);
    let quit_item = MenuItem::new("Quit", true, None);
    let menu = Menu::new();
    let _ = menu.append_items(&[
        &save_item,
        &settings_item,
        &folder_item,
        &PredefinedMenuItem::separator(),
        &quit_item,
    ]);
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(tooltip(&app))
        .with_icon(make_icon())
        .build()
        .expect("tray icon");

    if !hotkey_ok {
        app::notify("Hotkey unavailable", &format!("{hotkey_text} is taken by another app. Change it in Settings."));
    }

    let mut msg = MSG::default();
    let mut last_tip = std::time::Instant::now();
    loop {
        // Wake at least every 2 s to refresh the tooltip.
        let got = unsafe {
            if PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                true
            } else {
                MsgWaitForMultipleObjects(None, false, 2000, QS_ALLINPUT);
                false
            }
        };
        if got {
            if msg.message == WM_QUIT {
                break;
            }
            if msg.message == WM_HOTKEY && msg.wParam.0 as i32 == HOTKEY_ID {
                app::save(&app);
            }
            if msg.message == app::WM_APP_RESTART {
                // Settings changed: reload and restart with the new ones.
                let mut rec = app.rec.lock().unwrap();
                rec.stop();
                unsafe {
                    let _ = UnregisterHotKey(None, HOTKEY_ID);
                }
                *rec = Recorder::start(config::load());
                let ok = register_hotkey(&rec.cfg.hotkey);
                app::hotkey_ok_flag(&app, ok);
                save_item.set_text(format!("Save replay	{}", rec.cfg.hotkey));
                log!("settings applied");
            }
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            if ev.id == save_item.id() {
                app::save(&app);
            } else if ev.id == settings_item.id() {
                if let Some(u) = &ui_url {
                    app::open_url(u);
                }
            } else if ev.id == folder_item.id() {
                let dir = app.rec.lock().unwrap().cfg.save_dir.clone();
                let _ = std::fs::create_dir_all(&dir);
                app::open_path(&dir);
            } else if ev.id == quit_item.id() {
                unsafe { PostQuitMessage(0) };
            }
        }

        if last_tip.elapsed().as_secs() >= 2 {
            let _ = tray.set_tooltip(Some(tooltip(&app)));
            last_tip = std::time::Instant::now();
        }
    }
    app.rec.lock().unwrap().stop();
}

fn tooltip(app: &App) -> String {
    let rec = app.rec.lock().unwrap();
    let held = rec.store.lock().map(|s| s.held_seconds()).unwrap_or(0.0);
    let state = match rec.last_error.lock().unwrap().as_ref() {
        Some(e) if held < 1.0 => format!("Problem: {}", e.chars().take(60).collect::<String>()),
        _ => format!("Recording · {} buffered", app::fmt_len(held)),
    };
    format!("Replay Buffer
{state}
{} to save", rec.cfg.hotkey)
}

/// Parses things like "Alt+F10" or "Ctrl+Shift+S" and registers it globally.
fn register_hotkey(text: &str) -> bool {
    let mut mods = MOD_NOREPEAT;
    let mut vk: Option<u32> = None;
    for part in text.split('+').map(|s| s.trim().to_ascii_uppercase()) {
        match part.as_str() {
            "ALT" => mods |= MOD_ALT,
            "CTRL" | "CONTROL" => mods |= MOD_CONTROL,
            "SHIFT" => mods |= MOD_SHIFT,
            "WIN" => mods |= MOD_WIN,
            k if k.len() > 1 && k.starts_with('F') && k[1..].parse::<u32>().is_ok() => {
                let n: u32 = k[1..].parse().unwrap();
                if (1..=24).contains(&n) {
                    vk = Some(VK_F1.0 as u32 + n - 1);
                }
            }
            k if k.len() == 1 && k.chars().all(|c| c.is_ascii_alphanumeric()) => {
                vk = Some(k.as_bytes()[0] as u32);
            }
            "HOME" => vk = Some(VK_HOME.0 as u32),
            "END" => vk = Some(VK_END.0 as u32),
            "INSERT" => vk = Some(VK_INSERT.0 as u32),
            "PAUSE" => vk = Some(VK_PAUSE.0 as u32),
            "PRINTSCREEN" => vk = Some(VK_SNAPSHOT.0 as u32),
            _ => {}
        }
    }
    let Some(vk) = vk else {
        log!("could not understand hotkey {text:?}");
        return false;
    };
    let ok = unsafe { RegisterHotKey(None, HOTKEY_ID, mods, vk).is_ok() };
    log!("hotkey {text}: {}", if ok { "registered" } else { "FAILED" });
    ok
}

/// A simple red "record" dot.
fn make_icon() -> Icon {
    let n = 32u32;
    let mut px = Vec::with_capacity((n * n * 4) as usize);
    let c = (n as f32 - 1.0) / 2.0;
    for y in 0..n {
        for x in 0..n {
            let d = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt();
            let a = (12.5 - d).clamp(0.0, 1.0); // anti-aliased edge
            px.extend_from_slice(&[232, 52, 52, (a * 255.0) as u8]);
        }
    }
    Icon::from_rgba(px, n, n).expect("icon")
}

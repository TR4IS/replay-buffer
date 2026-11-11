//! State shared by the tray, the hotkey and the settings page.

use crate::recorder::Recorder;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub const WM_APP_RESTART: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 2;

#[derive(Clone, Serialize)]
pub struct SaveEvent {
    pub ok: bool,
    pub text: String,
    pub name: Option<String>,
    pub at: u64,
}

#[derive(Clone, Serialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum SyncState {
    Idle,
    Running,
    Done { ms: i32 },
    Failed { error: String },
}

pub struct App {
    pub rec: Mutex<Recorder>,
    pub last_save: Mutex<Option<SaveEvent>>,
    pub sync: Mutex<SyncState>,
    pub hotkey_ok: AtomicBool,
    pub main_thread: u32,
}

impl App {
    /// Asks the main thread to reload settings and restart the recorder.
    pub fn request_restart(&self) {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::PostThreadMessageW(
                self.main_thread,
                WM_APP_RESTART,
                Default::default(),
                Default::default(),
            );
        }
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn fmt_len(secs: f64) -> String {
    let s = secs.round() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

pub fn save(app: &Arc<App>) {
    crate::log!("saving replay");
    let a = app.clone();
    app.rec.lock().unwrap().save(move |r| {
        let ev = match r {
            Ok((path, secs)) => {
                crate::log!("saved {}", path.display());
                let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                notify("Replay saved", &format!("{} · {}", fmt_len(secs), name));
                SaveEvent { ok: true, text: format!("Saved {}", fmt_len(secs)), name: Some(name), at: unix_now() }
            }
            Err(e) => {
                crate::log!("save failed: {e}");
                notify("Couldn't save replay", &e);
                SaveEvent { ok: false, text: e, name: None, at: unix_now() }
            }
        };
        *a.last_save.lock().unwrap() = Some(ev);
    });
}

/// Runs the flash-and-beep sync test in the background and stores the result.
pub fn sync_test(app: &Arc<App>) {
    {
        let mut s = app.sync.lock().unwrap();
        if matches!(*s, SyncState::Running) {
            return;
        }
        *s = SyncState::Running;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let (track, delay) = {
            let rec = app.rec.lock().unwrap();
            (
                rec.tracks.iter().find(|t| t.source == crate::audio::Source::Desktop).cloned(),
                rec.desktop_delay.clone(),
            )
        };
        let result = match track {
            None => Err("Turn on desktop audio first.".to_string()),
            Some(t) => crate::selftest::measure_desktop_delay(&t),
        };
        let state = match result {
            Ok(ms) => {
                let ms = ms.round() as i32;
                delay.store(ms, Ordering::Relaxed);
                let mut cfg = crate::config::load();
                cfg.desktop_delay_ms = ms;
                let _ = crate::config::save(&cfg);
                app.rec.lock().unwrap().cfg.desktop_delay_ms = ms;
                crate::log!("sync test: desktop delay set to {ms} ms");
                SyncState::Done { ms }
            }
            Err(e) => SyncState::Failed { error: e },
        };
        *app.sync.lock().unwrap() = state;
    });
}

pub fn notify(title: &str, text: &str) {
    let (title, text) = (title.to_string(), text.to_string());
    std::thread::spawn(move || {
        let _ = tauri_winrt_notification::Toast::new(tauri_winrt_notification::Toast::POWERSHELL_APP_ID)
            .title(&title)
            .text1(&text)
            .sound(None)
            .show();
    });
}

pub fn open_path(p: &std::path::Path) {
    let _ = std::process::Command::new("explorer").arg(p).spawn();
}

pub fn open_url(url: &str) {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{HSTRING, w};
    unsafe {
        ShellExecuteW(None, w!("open"), &HSTRING::from(url), None, None, SW_SHOWNORMAL);
    }
}

pub fn hotkey_ok_flag(app: &App, ok: bool) {
    app.hotkey_ok.store(ok, Ordering::Relaxed);
}

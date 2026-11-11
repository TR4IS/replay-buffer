use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

static START: OnceLock<Instant> = OnceLock::new();
static FILE: OnceLock<Mutex<Option<std::fs::File>>> = OnceLock::new();

pub fn init() {
    START.get_or_init(Instant::now);
    let dir = crate::config::app_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("log.txt");
    // Keep the log small: start fresh if it grew past 1 MB.
    if std::fs::metadata(&path).map(|m| m.len() > 1_000_000).unwrap_or(false) {
        let _ = std::fs::remove_file(&path);
    }
    let f = std::fs::OpenOptions::new().create(true).append(true).open(path).ok();
    FILE.get_or_init(|| Mutex::new(f));
}

pub fn write(msg: &str) {
    let t = START.get_or_init(Instant::now).elapsed().as_secs_f64();
    let line = format!("[{t:9.3}] {msg}\n");
    eprint!("{line}");
    if let Some(m) = FILE.get() {
        if let Ok(mut g) = m.lock() {
            if let Some(f) = g.as_mut() {
                let _ = f.write_all(line.as_bytes());
            }
        }
    }
}

#[macro_export]
macro_rules! log {
    ($($t:tt)*) => { $crate::log::write(&format!($($t)*)) };
}

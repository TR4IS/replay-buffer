//! Sync test: once a second, flashes a white square on the primary monitor
//! and plays a short beep at the same instant (like a game showing a frame and
//! playing a sound together). Video is captured within ~2 ms of the flash, so
//! the time until the beep shows up in the desktop-audio capture is the delay
//! of the sound path (Voicemeeter alone adds ~120 ms on this PC). Saves
//! subtract that delay so sound lines up with the picture.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

unsafe extern "system" fn wndproc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(h, m, w, l) }
}

/// Runs the flashes until `stop`; returns the flash times (QPC, 100 ns units).
pub fn spawn(stop: Arc<AtomicBool>) -> std::thread::JoinHandle<Vec<i64>> {
    std::thread::spawn(move || unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let hinst = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default();
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            lpszClassName: w!("ReplayBufferFlash"),
            hbrBackground: HBRUSH(GetStockObject(BLACK_BRUSH).0),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            w!("ReplayBufferFlash"),
            w!(""),
            WS_POPUP | WS_VISIBLE,
            860,
            440,
            200,
            200,
            None,
            None,
            Some(hinst.into()),
            None,
        )
        .unwrap();
        let white = HBRUSH(GetStockObject(WHITE_BRUSH).0);
        let black = HBRUSH(GetStockObject(BLACK_BRUSH).0);

        // Low-latency shared render stream on the default output.
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).unwrap();
        let dev = en.GetDefaultAudioEndpoint(eRender, eConsole).unwrap();
        let client: IAudioClient = dev.Activate(CLSCTX_ALL, None).unwrap();
        let fmt = client.GetMixFormat().unwrap();
        let (rate, ch) = ((*fmt).nSamplesPerSec, (*fmt).nChannels as usize);
        client.Initialize(AUDCLNT_SHAREMODE_SHARED, 0, 200_000, 0, fmt, None).unwrap();
        let size = client.GetBufferSize().unwrap();
        let render: IAudioRenderClient = client.GetService().unwrap();
        client.Start().unwrap();
        let beep_frames = rate as usize * 30 / 1000;
        let mut beep_left = 0usize;
        let mut beep_pos = 0usize;

        let mut times = Vec::new();
        let start = Instant::now();
        let mut next = 1u64;
        let mut lit_until: Option<Instant> = None;
        let fill = |brush: HBRUSH| {
            let dc = GetDC(Some(hwnd));
            let r = RECT { left: 0, top: 0, right: 200, bottom: 200 };
            FillRect(dc, &r, brush);
            ReleaseDC(Some(hwnd), dc);
        };
        fill(black);
        while !stop.load(Ordering::Relaxed) {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                DispatchMessageW(&msg);
            }
            // 1.013 s apart, so the beeps land at different points of the
            // audio engine's 10 ms cycle and the average is fair.
            if start.elapsed() >= Duration::from_millis(next * 1013) {
                next += 1;
                fill(white);
                times.push(crate::capture::qpc_to_hns(crate::capture::qpc_now(), crate::capture::qpc_freq()));
                let _ = windows::Win32::Graphics::Dwm::DwmFlush();
                lit_until = Some(Instant::now() + Duration::from_millis(100));
                beep_left = beep_frames;
                beep_pos = 0;
            }
            if lit_until.is_some_and(|t| Instant::now() >= t) {
                fill(black);
                lit_until = None;
            }
            // Keep the render buffer topped up: beep when due, silence otherwise.
            let pad = client.GetCurrentPadding().unwrap_or(size);
            let free = (size - pad) as usize;
            if free > 0 {
                if let Ok(p) = render.GetBuffer(free as u32) {
                    let out = std::slice::from_raw_parts_mut(p as *mut f32, free * ch);
                    for f in 0..free {
                        let v = if beep_left > 0 {
                            beep_left -= 1;
                            beep_pos += 1;
                            (beep_pos as f32 * 2.0 * std::f32::consts::PI * 1000.0 / rate as f32).sin() * 0.5
                        } else {
                            0.0
                        };
                        for c in 0..ch {
                            out[f * ch + c] = v;
                        }
                    }
                    let _ = render.ReleaseBuffer(free as u32, 0);
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let _ = client.Stop();
        let _ = DestroyWindow(hwnd);
        times
    })
}

/// Flashes and beeps 7 times and measures how late desktop audio arrives.
/// Returns the delay in ms (trimmed mean of the beeps that were heard).
pub fn measure_desktop_delay(track: &crate::audio::Track) -> Result<f64, String> {
    if !track.active.load(Ordering::Relaxed) {
        return Err("Desktop audio isn't being captured.".into());
    }
    let stop = Arc::new(AtomicBool::new(false));
    let h = spawn(stop.clone());
    std::thread::sleep(Duration::from_millis(7700));
    stop.store(true, Ordering::Relaxed);
    let flashes = h.join().map_err(|_| "sync test crashed".to_string())?;
    std::thread::sleep(Duration::from_millis(600)); // let the last beep arrive
    let mut delays = Vec::new();
    for f in flashes {
        let to = f + 6_000_000; // look 600 ms ahead
        let chunks = track.range(f, to);
        let (rate, s) = crate::audio::build_timeline(&chunks, f, to);
        if let Some(i) = tone_onset(&s, rate, 1000.0) {
            delays.push(i as f64 * 1000.0 / rate as f64);
        }
    }
    crate::log!("sync test: desktop audio delays {delays:?} ms");
    if delays.len() < 3 {
        return Err("Couldn't hear the test beeps. Is desktop audio muted or going to another device?".into());
    }
    delays.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mid = &delays[1..delays.len() - 1]; // drop the extremes
    Ok(mid.iter().sum::<f64>() / mid.len() as f64)
}

/// First 2 ms window (stereo samples) dominated by a tone at `hz`.
fn tone_onset(stereo: &[f32], rate: u32, hz: f32) -> Option<usize> {
    let w = (rate as usize / 500).max(16);
    let frames = stereo.len() / 2;
    let mut j = 0;
    while j + w <= frames {
        let (mut re, mut im, mut tot) = (0.0f32, 0.0f32, 1e-12f32);
        for i in 0..w {
            let x = (stereo[(j + i) * 2] + stereo[(j + i) * 2 + 1]) * 0.5;
            let ph = 2.0 * std::f32::consts::PI * hz * i as f32 / rate as f32;
            re += x * ph.cos();
            im += x * ph.sin();
            tot += x * x;
        }
        let ratio = (re * re + im * im) / (w as f32 / 2.0) / tot;
        if ratio > 0.6 && tot / w as f32 > 0.005 {
            return Some(j);
        }
        j += w;
    }
    None
}

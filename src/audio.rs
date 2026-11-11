//! Desktop sound (WASAPI loopback) and microphone capture, straight on WASAPI.
//!
//! Every packet Windows hands us carries the QPC time of its first sample, so
//! audio lands on the same clock as the video's grid. The capture threads only
//! copy samples into a small ring (as stereo f32); lining up, gap filling,
//! mixing and AAC encoding all happen when you save.
//!
//! Loopback delivers nothing while nothing plays, so we play a silent stream
//! on the same device to keep it flowing (the trick OBS uses too).

use anyhow::{Result, anyhow};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Threading::*;
use windows::core::{GUID, w};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Desktop,
    Mic,
}

/// Samples captured together. `hns` = QPC time of the first sample, in 100 ns units.
pub struct Chunk {
    pub hns: i64,
    pub rate: u32,
    pub samples: Vec<f32>, // stereo, interleaved
}

/// One audio source's recent history plus live info for the UI.
pub struct Track {
    pub source: Source,
    pub chunks: Mutex<VecDeque<Chunk>>,
    pub device: Mutex<String>,
    pub glitches: AtomicU64,
    /// Peak level since the UI last asked (f32 bits).
    pub peak: AtomicU32,
    pub active: AtomicBool,
}

impl Track {
    pub fn new(source: Source) -> Arc<Track> {
        Arc::new(Track {
            source,
            chunks: Mutex::new(VecDeque::new()),
            device: Mutex::new(String::new()),
            glitches: AtomicU64::new(0),
            peak: AtomicU32::new(0),
            active: AtomicBool::new(false),
        })
    }

    pub fn take_peak(&self) -> f32 {
        f32::from_bits(self.peak.swap(0, Ordering::Relaxed))
    }

    /// Copies out the chunks that overlap [from, to) (100 ns units).
    pub fn range(&self, from: i64, to: i64) -> Vec<(i64, u32, Vec<f32>)> {
        let q = self.chunks.lock().unwrap();
        q.iter()
            .filter(|c| {
                let dur = (c.samples.len() / 2) as i64 * 10_000_000 / c.rate.max(1) as i64;
                c.hns + dur > from && c.hns < to
            })
            .map(|c| (c.hns, c.rate, c.samples.clone()))
            .collect()
    }
}

/// Starts a capture thread for `track`, keeping `keep_secs` of history.
pub fn spawn(track: Arc<Track>, keep_secs: u32, stop: Arc<AtomicBool>) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name(format!("{:?} audio", track.source))
        .spawn(move || {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                // Windows' multimedia scheduler keeps audio threads on time without us burning CPU.
                let mut idx = 0u32;
                let _ = AvSetMmThreadCharacteristicsW(w!("Audio"), &mut idx);
            }
            let keep_hns = (keep_secs as i64 + 15) * 10_000_000;
            let mut fails = 0u32;
            while !stop.load(Ordering::Relaxed) {
                match run_device(&track, keep_hns, &stop) {
                    Ok(()) => fails = 0,
                    Err(e) => {
                        if fails < 3 || fails % 60 == 0 {
                            crate::log!("{:?} audio: {e:#}; retrying", track.source);
                        }
                        fails += 1;
                    }
                }
                track.active.store(false, Ordering::Relaxed);
                // Wait a moment (in small steps so quitting stays instant).
                for _ in 0..10 {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
            unsafe { CoUninitialize() };
        })
        .expect("spawn audio thread")
}

struct Format {
    rate: u32,
    channels: usize,
    bits: u16,
    float: bool,
    /// Per input channel: (weight into left, weight into right).
    mix: Vec<(f32, f32)>,
}

const KSDATAFORMAT_SUBTYPE_IEEE_FLOAT: GUID = GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);

unsafe fn parse_format(p: *const WAVEFORMATEX) -> Format {
    let wf = unsafe { std::ptr::read_unaligned(p) };
    let mut float = wf.wFormatTag == 3; // WAVE_FORMAT_IEEE_FLOAT
    let mut mask: u32 = 0;
    if wf.wFormatTag == 0xFFFE && wf.cbSize >= 22 {
        let ext = unsafe { std::ptr::read_unaligned(p as *const WAVEFORMATEXTENSIBLE) };
        let sub = ext.SubFormat;
        float = sub == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT;
        mask = ext.dwChannelMask;
    }
    let ch = wf.nChannels as usize;
    // Speaker positions in channel order, from the mask (or the usual order).
    let mut positions = Vec::new();
    if mask != 0 {
        for bit in 0..32 {
            if mask & (1 << bit) != 0 {
                positions.push(1u32 << bit);
            }
        }
    }
    const DEFAULT: [u32; 8] = [0x1, 0x2, 0x4, 0x8, 0x10, 0x20, 0x200, 0x400];
    let h = std::f32::consts::FRAC_1_SQRT_2;
    let mix = (0..ch)
        .map(|i| {
            if ch == 1 {
                return (1.0, 1.0);
            }
            let pos = positions.get(i).copied().or(DEFAULT.get(i).copied()).unwrap_or(0);
            match pos {
                0x1 | 0x40 => (1.0, 0.0),               // front left (of centre)
                0x2 | 0x80 => (0.0, 1.0),               // front right (of centre)
                0x4 | 0x100 => (h, h),                  // centre / back centre
                0x8 => (0.0, 0.0),                      // LFE
                0x10 | 0x200 => (h, 0.0),               // back / side left
                0x20 | 0x400 => (0.0, h),               // back / side right
                _ => (0.0, 0.0),
            }
        })
        .collect();
    Format { rate: wf.nSamplesPerSec, channels: ch, bits: wf.wBitsPerSample, float, mix }
}

fn to_stereo(f: &Format, data: *const u8, frames: usize, silent: bool, out: &mut Vec<f32>) {
    out.clear();
    out.resize(frames * 2, 0.0);
    if silent || data.is_null() {
        return;
    }
    let bps = (f.bits / 8) as usize;
    let stride = bps * f.channels;
    let bytes = unsafe { std::slice::from_raw_parts(data, frames * stride) };
    for i in 0..frames {
        let (mut l, mut r) = (0.0f32, 0.0f32);
        for c in 0..f.channels {
            let o = i * stride + c * bps;
            let s = match (f.float, bps) {
                (true, 4) => f32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]),
                (false, 2) => i16::from_le_bytes([bytes[o], bytes[o + 1]]) as f32 / 32768.0,
                (false, 3) => (i32::from_le_bytes([0, bytes[o], bytes[o + 1], bytes[o + 2]]) as f32) / 2147483648.0,
                (false, 4) => i32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]) as f32 / 2147483648.0,
                _ => 0.0,
            };
            let (wl, wr) = f.mix[c];
            l += s * wl;
            r += s * wr;
        }
        out[i * 2] = l;
        out[i * 2 + 1] = r;
    }
}

fn device_id(d: &IMMDevice) -> String {
    unsafe {
        match d.GetId() {
            Ok(p) => {
                let s = p.to_string().unwrap_or_default();
                CoTaskMemFree(Some(p.0 as *const _));
                s
            }
            Err(_) => String::new(),
        }
    }
}

fn friendly_name(d: &IMMDevice) -> String {
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
    use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
    unsafe {
        let Ok(store) = d.OpenPropertyStore(STGM_READ) else { return String::new() };
        let Ok(v) = store.GetValue(&PKEY_Device_FriendlyName) else { return String::new() };
        match PropVariantToStringAlloc(&v) {
            Ok(p) => {
                let s = p.to_string().unwrap_or_default();
                CoTaskMemFree(Some(p.0 as *const _));
                s
            }
            Err(_) => String::new(),
        }
    }
}

struct EventHandle(HANDLE);
impl Drop for EventHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Captures from the current default device until it changes, fails or we stop.
fn run_device(track: &Track, keep_hns: i64, stop: &AtomicBool) -> Result<()> {
    let desktop = track.source == Source::Desktop;
    unsafe {
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let flow = if desktop { eRender } else { eCapture };
        let dev = en.GetDefaultAudioEndpoint(flow, eConsole).map_err(|e| anyhow!("no default device ({e})"))?;
        let id = device_id(&dev);
        let name = friendly_name(&dev);

        let client: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
        let fmt_ptr = client.GetMixFormat()?;
        let fmt = parse_format(fmt_ptr);
        let mut flags = AUDCLNT_STREAMFLAGS_EVENTCALLBACK;
        if desktop {
            flags |= AUDCLNT_STREAMFLAGS_LOOPBACK;
        }
        // 1 s of buffer: a hiccup on our side never loses samples.
        let init = client.Initialize(AUDCLNT_SHAREMODE_SHARED, flags, 10_000_000, 0, fmt_ptr, None);
        let event = EventHandle(windows::Win32::System::Threading::CreateEventW(None, false, false, None)?);

        // Keep-alive: a silent stream on the same device, so loopback never pauses.
        let mut keep_alive: Option<(IAudioClient, IAudioRenderClient, u32)> = None;
        if desktop && init.is_ok() {
            let rc: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
            if rc.Initialize(AUDCLNT_SHAREMODE_SHARED, 0, 10_000_000, 0, fmt_ptr, None).is_ok() {
                let size = rc.GetBufferSize()?;
                let render: IAudioRenderClient = rc.GetService()?;
                if render.GetBuffer(size).is_ok() {
                    let _ = render.ReleaseBuffer(size, AUDCLNT_BUFFERFLAGS_SILENT.0 as u32);
                }
                rc.Start()?;
                keep_alive = Some((rc, render, size));
            }
        }
        CoTaskMemFree(Some(fmt_ptr as *const _));
        init.map_err(|e| anyhow!("opening {name} ({e})"))?;
        client.SetEventHandle(event.0)?;
        let cap: IAudioCaptureClient = client.GetService()?;
        client.Start()?;

        *track.device.lock().unwrap() = name.clone();
        track.active.store(true, Ordering::Relaxed);
        crate::log!(
            "{:?} audio: {name}, {} Hz, {} ch, {}-bit{}",
            track.source,
            fmt.rate,
            fmt.channels,
            fmt.bits,
            if fmt.float { " float" } else { "" }
        );

        let dbg = std::env::var_os("RB_AUDIODBG").is_some();
        if dbg {
            let lat = client.GetStreamLatency().unwrap_or(0);
            let (mut def, mut min) = (0i64, 0i64);
            let _ = client.GetDevicePeriod(Some(&mut def), Some(&mut min));
            crate::log!("{:?} dbg: stream latency {:.1} ms, period {:.1} ms", track.source, lat as f64 / 1e4, def as f64 / 1e4);
        }
        let mut npk = 0u64;
        let mut buf = Vec::new();
        let mut first = true;
        let mut last_check = Instant::now();
        while !stop.load(Ordering::Relaxed) {
            WaitForSingleObject(event.0, 10);
            if let Some((rc, render, size)) = &keep_alive {
                let pad = rc.GetCurrentPadding()?;
                let free = size - pad;
                if free > 0 && render.GetBuffer(free).is_ok() {
                    let _ = render.ReleaseBuffer(free, AUDCLNT_BUFFERFLAGS_SILENT.0 as u32);
                }
            }
            loop {
                if cap.GetNextPacketSize()? == 0 {
                    break;
                }
                let mut data = std::ptr::null_mut();
                let mut frames = 0u32;
                let mut fl = 0u32;
                let mut qpc = 0u64;
                cap.GetBuffer(&mut data, &mut frames, &mut fl, None, Some(&mut qpc))?;
                let silent = fl & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0;
                npk += 1;
                if dbg && npk % 300 == 0 {
                    let now = crate::capture::qpc_to_hns(crate::capture::qpc_now(), crate::capture::qpc_freq());
                    crate::log!("{:?} dbg: packet {} frames, arrived {:.1} ms after its timestamp", track.source, frames, (now - qpc as i64) as f64 / 1e4);
                }
                if fl & AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY.0 as u32 != 0 && !first {
                    track.glitches.fetch_add(1, Ordering::Relaxed);
                }
                first = false;
                to_stereo(&fmt, data, frames as usize, silent, &mut buf);
                cap.ReleaseBuffer(frames)?;
                if frames == 0 {
                    continue;
                }
                let peak = buf.iter().fold(0.0f32, |m, s| m.max(s.abs()));
                // Non-negative floats order the same as their bit patterns.
                track.peak.fetch_max(peak.to_bits(), Ordering::Relaxed);
                let mut q = track.chunks.lock().unwrap();
                q.push_back(Chunk { hns: qpc as i64, rate: fmt.rate, samples: buf.clone() });
                let newest = qpc as i64;
                while q.front().is_some_and(|c| newest - c.hns > keep_hns) {
                    q.pop_front();
                }
            }
            // Follow the default device when the user switches it.
            if last_check.elapsed() > Duration::from_secs(2) {
                last_check = Instant::now();
                if let Ok(d) = en.GetDefaultAudioEndpoint(flow, eConsole) {
                    if device_id(&d) != id {
                        crate::log!("{:?} audio: default device changed", track.source);
                        break;
                    }
                }
            }
        }
        let _ = client.Stop();
        if let Some((rc, _, _)) = &keep_alive {
            let _ = rc.Stop();
        }
    }
    Ok(())
}

/// Builds one continuous stereo stream at 48 kHz-or-device rate for [from, to)
/// (100 ns units) from captured chunks. Gaps become silence; small timing
/// jitter is ignored so it never clicks. Returns (rate, samples).
pub fn build_timeline(chunks: &[(i64, u32, Vec<f32>)], from: i64, to: i64) -> (u32, Vec<f32>) {
    let rate = chunks.last().map_or(48000, |c| c.1);
    let total = (((to - from) as i128 * rate as i128) / 10_000_000) as usize;
    let mut out = vec![0.0f32; total * 2];
    let tolerance = rate as i64 * 5 / 1000; // 5 ms
    let mut cursor: i64 = i64::MIN; // next frame index if the stream is continuous
    for (hns, r, s) in chunks {
        if *r != rate {
            continue; // device changed mid-buffer; skip the other format
        }
        let frames = (s.len() / 2) as i64;
        let exact = (((hns - from) as i128 * rate as i128) / 10_000_000) as i64;
        let pos = if cursor != i64::MIN && (exact - cursor).abs() <= tolerance { cursor } else { exact };
        for i in 0..frames {
            let t = pos + i;
            if t >= 0 && (t as usize) < total {
                out[t as usize * 2] = s[i as usize * 2];
                out[t as usize * 2 + 1] = s[i as usize * 2 + 1];
            }
        }
        cursor = pos + frames;
    }
    (rate, out)
}

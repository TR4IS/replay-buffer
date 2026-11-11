//! Runs the pipeline and restarts it if anything breaks.
//!
//!   capture thread --encoded packets--> RAM/disk ring (buffer.rs)
//!   audio threads  --raw samples------> per-source rings (audio.rs)
//!
//! Nothing else runs while you play. On save we cut everything at a keyframe,
//! line the audio up on the shared clock, and let ffmpeg write the MP4
//! (video is copied, audio is encoded to AAC), at below-normal priority.

use crate::audio::{self, Source, Track};
use crate::buffer::{Snapshot, Store};
use crate::capture::{self, Timeline, VideoParams, VideoStats};
use crate::config::{Codec, Config};
use anyhow::{Context, Result, bail};
use std::io::Write;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;

pub fn ffmpeg_path() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    let dir = exe.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    for cand in [dir.join("ffmpeg.exe"), dir.join("../../vendor/ffmpeg.exe")] {
        if cand.exists() {
            return cand;
        }
    }
    PathBuf::from("ffmpeg.exe")
}

pub struct Recorder {
    pub cfg: Config,
    pub store: Arc<Mutex<Store>>,
    pub stats: Arc<VideoStats>,
    pub tracks: Vec<Arc<Track>>,
    pub timeline: Arc<Mutex<Option<Timeline>>>,
    pub last_error: Arc<Mutex<Option<String>>>,
    pub saving: Arc<AtomicBool>,
    /// Live copy of cfg.desktop_delay_ms, so a sync test applies without a restart.
    pub desktop_delay: Arc<AtomicI32>,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl Recorder {
    pub fn start(cfg: Config) -> Recorder {
        let store = Arc::new(Mutex::new(Store::new(
            cfg.buffer_mode,
            cfg.disk_buffer_dir.clone(),
            cfg.replay_minutes,
            cfg.fps,
        )));
        let stop = Arc::new(AtomicBool::new(false));
        let stats = Arc::new(VideoStats::default());
        let last_error = Arc::new(Mutex::new(None));
        let timeline = Arc::new(Mutex::new(None));
        let mut threads = Vec::new();

        let mut tracks = Vec::new();
        if cfg.desktop_audio {
            tracks.push(Track::new(Source::Desktop));
        }
        if cfg.microphone {
            tracks.push(Track::new(Source::Mic));
        }
        for t in &tracks {
            threads.push(audio::spawn(t.clone(), cfg.replay_minutes * 60, stop.clone()));
        }

        let vp = VideoParams {
            monitor: cfg.monitor,
            fps: cfg.fps,
            codec: cfg.codec.nvenc_id(),
            ten_bit: cfg.ten_bit,
            preset: cfg.preset,
            aq: cfg.spatial_aq,
            quality: cfg.quality,
            max_kbps: cfg.max_bitrate_mbps * 1000,
            cursor: cfg.capture_cursor,
            gpu_priority: cfg.gpu_priority,
        };
        {
            let (store, stop, stats, last_error, timeline) =
                (store.clone(), stop.clone(), stats.clone(), last_error.clone(), timeline.clone());
            threads.push(
                std::thread::Builder::new()
                    .name("capture".into())
                    .spawn(move || {
                        let mut backoff = 1;
                        while !stop.load(Ordering::Relaxed) {
                            let started = Instant::now();
                            let res = {
                                let store2 = store.clone();
                                let (tl, le) = (timeline.clone(), last_error.clone());
                                capture::run(
                                    vp.clone(),
                                    stop.clone(),
                                    stats.clone(),
                                    move |t| {
                                        *tl.lock().unwrap() = Some(t);
                                        *le.lock().unwrap() = None;
                                    },
                                    move |p| store2.lock().unwrap().push(p.pts, p.key, &p.data),
                                )
                            };
                            if let Err(e) = res {
                                crate::log!("capture stopped: {e:#}");
                                *last_error.lock().unwrap() = Some(format!("{e:#}"));
                            }
                            if stop.load(Ordering::Relaxed) {
                                break;
                            }
                            if started.elapsed() > Duration::from_secs(30) {
                                backoff = 1;
                            }
                            for _ in 0..backoff * 10 {
                                if stop.load(Ordering::Relaxed) {
                                    break;
                                }
                                std::thread::sleep(Duration::from_millis(100));
                            }
                            backoff = (backoff * 2).min(30);
                            // The clock restarts with capture, so old video can't be joined to new.
                            store.lock().unwrap().reset();
                        }
                    })
                    .expect("spawn capture"),
            );
        }
        crate::log!("recording ({} min buffer, {:?} mode)", cfg.replay_minutes, cfg.buffer_mode);
        Recorder {
            desktop_delay: Arc::new(AtomicI32::new(cfg.desktop_delay_ms)),
            cfg,
            store,
            stats,
            tracks,
            timeline,
            last_error,
            saving: Arc::new(AtomicBool::new(false)),
            stop,
            threads,
        }
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for h in self.threads.drain(..) {
            let _ = h.join();
        }
    }

    /// Writes the buffered replay to an MP4 on a background thread.
    /// `done` receives the saved path and its length, or an error message.
    pub fn save(&self, done: impl FnOnce(Result<(PathBuf, f64), String>) + Send + 'static) {
        if self.saving.swap(true, Ordering::SeqCst) {
            done(Err("Already saving a replay.".into()));
            return;
        }
        let (mut cfg, store, timeline, tracks, saving) =
            (self.cfg.clone(), self.store.clone(), self.timeline.clone(), self.tracks.clone(), self.saving.clone());
        cfg.desktop_delay_ms = self.desktop_delay.load(Ordering::Relaxed);
        std::thread::spawn(move || {
            unsafe {
                use windows::Win32::System::Threading::*;
                let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
            }
            // Let the frames still inside the encoder (a few ms) land first,
            // so the clip ends at the moment you pressed the key.
            std::thread::sleep(Duration::from_millis(80));
            let snap = store.lock().unwrap().snapshot();
            let tl = *timeline.lock().unwrap();
            let r = match (snap, tl) {
                (Some(snap), Some(tl)) => {
                    let secs = (snap.last_pts() - snap.first_pts() + 1) as f64 / tl.fps as f64;
                    write_mp4(&cfg, &snap, &tl, &tracks).map(|p| (p, secs)).map_err(|e| format!("{e:#}"))
                }
                _ => Err("Nothing recorded yet. Give it a few seconds.".to_string()),
            };
            saving.store(false, Ordering::SeqCst);
            done(r);
        });
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.stop();
    }
}

struct TempFile(PathBuf);
impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn write_mp4(cfg: &Config, snap: &Snapshot, tl: &Timeline, tracks: &[Arc<Track>]) -> Result<PathBuf> {
    std::fs::create_dir_all(&cfg.save_dir).context("creating save folder")?;
    let path = unique_path(&cfg.save_dir, &format!("Replay {}", local_timestamp()));
    let part = path.with_extension("mp4.part");
    let first = snap.first_pts();
    let last = snap.last_pts();

    // Audio: the exact same time span as the video, on the shared clock.
    // Desktop sound reaches the capture late by the output path's delay
    // (measured by the sync test), so it's taken from that much later.
    let span = |shift_ms: i32| {
        let s = shift_ms as i64 * 10_000;
        (tl.slot_hns(first) + s, tl.slot_hns(last + 1) + s)
    };
    let tmp_dir = std::env::temp_dir().join("ReplayBuffer");
    let _ = std::fs::create_dir_all(&tmp_dir);
    let mut audio_inputs: Vec<(Source, u32, TempFile)> = Vec::new();
    for t in tracks {
        let shift = match t.source {
            Source::Desktop => cfg.desktop_delay_ms,
            Source::Mic => cfg.mic_delay_ms,
        };
        let (from, to) = span(shift);
        let chunks = t.range(from, to);
        if chunks.is_empty() && !t.active.load(Ordering::Relaxed) {
            crate::log!("{:?}: no audio captured, leaving the track out", t.source);
            continue;
        }
        let (rate, samples) = audio::build_timeline(&chunks, from, to);
        let file = TempFile(tmp_dir.join(format!("save-{}-{:?}.f32", std::process::id(), t.source)));
        let mut w = std::io::BufWriter::with_capacity(1 << 20, std::fs::File::create(&file.0)?);
        for s in &samples {
            w.write_all(&s.to_le_bytes())?;
        }
        w.flush()?;
        audio_inputs.push((t.source, rate, file));
    }

    let mut cmd = Command::new(ffmpeg_path());
    cmd.args(["-hide_banner", "-loglevel", "error", "-y"]);
    // ffmpeg's IVF reader has no HEVC, so HEVC travels as MPEG-TS.
    let ts = cfg.codec == Codec::Hevc;
    cmd.args(["-f", if ts { "mpegts" } else { "ivf" }, "-i", "pipe:0"]);
    for (_, rate, file) in &audio_inputs {
        cmd.args(["-f", "f32le", "-ar", &rate.to_string(), "-ac", "2", "-i"]).arg(&file.0);
    }
    // Track 1 is what players and apps play: desktop + mic mixed. The separate
    // tracks follow for editing.
    let mut titles: Vec<&str> = Vec::new();
    let mut rates: Vec<&str> = Vec::new();
    cmd.args(["-map", "0:v"]);
    match audio_inputs.len() {
        2 => {
            cmd.args([
                "-filter_complex",
                "[1:a]asplit[d0][d1];[2:a]asplit[m0][m1];[d0][m0]amix=inputs=2:duration=first:normalize=0[mix]",
                "-map",
                "[mix]",
                "-map",
                "[d1]",
                "-map",
                "[m1]",
            ]);
            titles = vec!["Desktop + microphone", "Desktop", "Microphone"];
            rates = vec!["256k", "192k", "128k"];
        }
        1 => {
            cmd.args(["-map", "1:a"]);
            titles = vec![if audio_inputs[0].0 == Source::Desktop { "Desktop" } else { "Microphone" }];
            rates = vec!["256k"];
        }
        _ => {}
    }
    cmd.args(["-c:v", "copy"]);
    if !titles.is_empty() {
        cmd.args(["-c:a", "aac", "-ar", "48000"]);
    }
    for (i, (title, rate)) in titles.iter().zip(&rates).enumerate() {
        cmd.arg(format!("-b:a:{i}")).arg(rate);
        cmd.arg(format!("-metadata:s:a:{i}")).arg(format!("title={title}"));
        cmd.arg(format!("-disposition:a:{i}")).arg(if i == 0 { "default" } else { "0" });
    }
    match cfg.codec {
        Codec::Hevc => {
            cmd.args(["-tag:v", "hvc1"]); // lets QuickTime / iPhone play it
        }
        Codec::H264 => {
            cmd.args(["-tag:v", "avc1"]);
        }
        Codec::Av1 => {}
    }
    cmd.args(["-movflags", "+faststart", "-f", "mp4"]).arg(&part);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);
    let mut child = cmd.spawn().context("starting ffmpeg for save")?;
    let write_res = {
        let mut stdin = std::io::BufWriter::with_capacity(1 << 20, child.stdin.take().unwrap());
        if ts { write_ts(&mut stdin, tl, snap) } else { write_ivf(&mut stdin, cfg.codec, tl, snap) }
            .and_then(|_| stdin.flush())
    };
    let out = child.wait_with_output()?;
    if !out.status.success() {
        let _ = std::fs::remove_file(&part);
        bail!("ffmpeg failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    write_res.context("sending video to ffmpeg")?;
    std::fs::rename(&part, &path).context("finishing the file")?;
    Ok(path)
}

/// IVF: a tiny container with one timestamp per frame (time base 1/fps).
fn write_ivf(w: &mut impl Write, codec: Codec, tl: &Timeline, snap: &Snapshot) -> std::io::Result<()> {
    let fourcc: &[u8; 4] = match codec {
        Codec::H264 => b"H264",
        Codec::Hevc => b"HEVC",
        Codec::Av1 => b"AV01",
    };
    let count: usize = snap.groups.iter().map(|g| g.0.len()).sum();
    let mut h = Vec::with_capacity(32);
    h.extend_from_slice(b"DKIF");
    h.extend_from_slice(&0u16.to_le_bytes());
    h.extend_from_slice(&32u16.to_le_bytes());
    h.extend_from_slice(fourcc);
    h.extend_from_slice(&(tl.width as u16).to_le_bytes());
    h.extend_from_slice(&(tl.height as u16).to_le_bytes());
    h.extend_from_slice(&tl.fps.to_le_bytes()); // time base denominator
    h.extend_from_slice(&1u32.to_le_bytes()); // time base numerator
    h.extend_from_slice(&(count as u32).to_le_bytes());
    h.extend_from_slice(&0u32.to_le_bytes());
    w.write_all(&h)?;
    let first = snap.first_pts();
    for (packets, data) in &snap.groups {
        let bytes = data.load()?;
        for p in packets {
            let frame = &bytes[p.offset as usize..(p.offset + p.len) as usize];
            w.write_all(&(frame.len() as u32).to_le_bytes())?;
            w.write_all(&((p.pts - first) as u64).to_le_bytes())?;
            w.write_all(frame)?;
        }
    }
    Ok(())
}

/// MPEG-TS with 90 kHz timestamps (used for HEVC).
fn write_ts(w: &mut impl Write, tl: &Timeline, snap: &Snapshot) -> std::io::Result<()> {
    let mut mux = crate::tsmux::TsMux::new(true);
    let first = snap.first_pts();
    for (packets, data) in &snap.groups {
        let bytes = data.load()?;
        for p in packets {
            let frame = &bytes[p.offset as usize..(p.offset + p.len) as usize];
            let pts90 = ((p.pts - first) as u64 * 90_000 / tl.fps as u64) + 90_000;
            w.write_all(mux.frame(frame, pts90, p.key))?;
        }
    }
    Ok(())
}

fn unique_path(dir: &Path, stem: &str) -> PathBuf {
    let mut p = dir.join(format!("{stem}.mp4"));
    let mut n = 2;
    while p.exists() {
        p = dir.join(format!("{stem} ({n}).mp4"));
        n += 1;
    }
    p
}

pub fn local_timestamp() -> String {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!(
        "{:04}-{:02}-{:02} {:02}-{:02}-{:02}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
    )
}

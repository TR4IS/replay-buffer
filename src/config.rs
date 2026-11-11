use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BufferMode {
    Ram,
    Disk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Codec {
    H264,
    Hevc,
    Av1,
}

impl Codec {
    pub fn nvenc_id(self) -> i32 {
        match self {
            Codec::H264 => 0,
            Codec::Hevc => 1,
            Codec::Av1 => 2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// "ram" keeps the encoded video in memory; "disk" keeps it in `disk_buffer_dir`.
    pub buffer_mode: BufferMode,
    pub disk_buffer_dir: PathBuf,
    /// How much history to keep, in minutes.
    pub replay_minutes: u32,
    /// Constant frame rate of the recording: 30, 60, 120 or 144.
    pub fps: u32,
    /// "h264" plays everywhere; "hevc" and "av1" make smaller files.
    pub codec: Codec,
    /// 10-bit colour (less banding). HEVC and AV1 only.
    pub ten_bit: bool,
    /// NVENC preset 1 (fastest) to 7 (best). The encoder is a separate chip, so this barely touches the game.
    pub preset: u32,
    /// Adaptive quantization: spends bits where the eye notices. Runs on the GPU's
    /// shader cores (~2% of them), so it's off by default.
    pub spatial_aq: bool,
    /// Lower = better quality and bigger files. 16-20 is the useful range.
    pub quality: u32,
    /// Hard cap on bitrate so fast motion can't balloon the buffer size.
    pub max_bitrate_mbps: u32,
    pub desktop_audio: bool,
    pub microphone: bool,
    /// How late desktop sound reaches the capture, in ms. Set by the sync test.
    pub desktop_delay_ms: i32,
    /// Same for the microphone.
    pub mic_delay_ms: i32,
    pub capture_cursor: bool,
    pub save_dir: PathBuf,
    pub hotkey: String,
    /// Which monitor to record: 0 = primary, 1/2/3 = the order Windows lists them.
    pub monitor: u32,
    /// Recorder GPU priority, -7..7. 0 lets the game win.
    pub gpu_priority: i32,
    /// Port of the settings page (http://127.0.0.1:port).
    pub ui_port: u16,
}

impl Default for Config {
    fn default() -> Self {
        let disk_dir = if PathBuf::from("D:\\").exists() {
            PathBuf::from("D:\\ReplayBuffer")
        } else {
            std::env::temp_dir().join("ReplayBuffer")
        };
        let save_dir = dirs::video_dir()
            .unwrap_or_else(|| PathBuf::from("C:\\"))
            .join("Replays");
        Config {
            buffer_mode: BufferMode::Ram,
            disk_buffer_dir: disk_dir,
            replay_minutes: 5,
            fps: 144,
            codec: Codec::H264,
            ten_bit: true,
            preset: 4,
            spatial_aq: false,
            quality: 18,
            max_bitrate_mbps: 100,
            desktop_audio: true,
            microphone: true,
            desktop_delay_ms: 0,
            mic_delay_ms: 0,
            capture_cursor: true,
            save_dir,
            hotkey: "Alt+F10".into(),
            monitor: 0,
            gpu_priority: 0,
            ui_port: 47811,
        }
    }
}

pub fn app_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("ReplayBuffer")
}

pub fn config_path() -> PathBuf {
    app_dir().join("config.toml")
}

const HEADER: &str = "# Replay Buffer settings. Easier: right-click the tray icon -> Settings.\n\
# If you edit this file by hand, choose \"Restart recorder\" in the tray menu afterwards.\n\n";

pub fn load() -> Config {
    let path = config_path();
    let cfg = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| match toml::from_str::<Config>(&s) {
            Ok(c) => Some(c),
            Err(e) => {
                crate::log!("config error, using defaults: {e}");
                None
            }
        })
        .unwrap_or_default();
    let cfg = sanitize(cfg);
    // Always write back so new options show up in the file.
    let _ = save(&cfg);
    cfg
}

pub fn save(cfg: &Config) -> std::io::Result<()> {
    std::fs::create_dir_all(app_dir())?;
    let body = toml::to_string_pretty(cfg).map_err(std::io::Error::other)?;
    std::fs::write(config_path(), format!("{HEADER}{body}"))
}

pub fn sanitize(mut c: Config) -> Config {
    c.fps = c.fps.clamp(10, 240);
    c.replay_minutes = c.replay_minutes.clamp(1, 60);
    c.quality = c.quality.clamp(1, 51);
    c.preset = c.preset.clamp(1, 7);
    c.max_bitrate_mbps = c.max_bitrate_mbps.clamp(5, 400);
    c.gpu_priority = c.gpu_priority.clamp(-7, 7);
    c.desktop_delay_ms = c.desktop_delay_ms.clamp(-1000, 1000);
    c.mic_delay_ms = c.mic_delay_ms.clamp(-1000, 1000);
    if c.ui_port < 1024 {
        c.ui_port = 47811;
    }
    c
}

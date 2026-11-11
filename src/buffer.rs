//! The replay ring buffer: encoded video packets, grouped by keyframe.
//!
//! Like OBS, we keep already-encoded packets and trim whole keyframe groups
//! (~2 s each) from the front, so the buffer always starts on a keyframe.
//! In RAM mode the groups live in memory; in disk mode each finished group is
//! written to one file and only a tiny index stays in memory.

use crate::config::BufferMode;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;

/// Where one packet sits inside its group's data.
#[derive(Clone, Copy)]
pub struct PacketInfo {
    pub pts: i64,
    pub key: bool,
    pub offset: u32,
    pub len: u32,
}

/// Group data. A disk file is deleted when the last reference goes away, so a
/// save in progress keeps its files even if the buffer moved on.
pub enum GroupData {
    Ram(Vec<u8>),
    Disk(PathBuf),
}

impl Drop for GroupData {
    fn drop(&mut self) {
        if let GroupData::Disk(p) = self {
            let _ = std::fs::remove_file(p);
        }
    }
}

impl GroupData {
    pub fn load(&self) -> std::io::Result<std::borrow::Cow<'_, [u8]>> {
        match self {
            GroupData::Ram(v) => Ok(std::borrow::Cow::Borrowed(v)),
            GroupData::Disk(p) => std::fs::read(p).map(std::borrow::Cow::Owned),
        }
    }
}

pub struct Group {
    pub packets: Vec<PacketInfo>,
    pub data: Arc<GroupData>,
    pub bytes: u64,
}

impl Group {
    fn start(&self) -> i64 {
        self.packets.first().map_or(0, |p| p.pts)
    }
}

pub struct Store {
    mode: BufferMode,
    dir: PathBuf,
    keep_slots: i64, // replay length in grid slots
    groups: VecDeque<Group>,
    // group being filled
    cur_packets: Vec<PacketInfo>,
    cur_data: Vec<u8>,
    seq: u64,
    last_pts: i64,
    pub fps: u32,
    total_bytes: u64,
}

impl Store {
    pub fn new(mode: BufferMode, dir: PathBuf, minutes: u32, fps: u32) -> Store {
        if mode == BufferMode::Disk {
            let _ = std::fs::create_dir_all(&dir);
            clean_dir(&dir);
        }
        Store {
            mode,
            dir,
            keep_slots: minutes as i64 * 60 * fps as i64,
            groups: VecDeque::new(),
            cur_packets: Vec::new(),
            cur_data: Vec::with_capacity(8 << 20),
            seq: 0,
            last_pts: 0,
            fps,
            total_bytes: 0,
        }
    }

    /// Forget everything (used when the recorder restarts, since the clock resets).
    pub fn reset(&mut self) {
        self.groups.clear();
        self.cur_packets.clear();
        self.cur_data.clear();
        self.total_bytes = 0;
        self.last_pts = 0;
    }

    pub fn held_seconds(&self) -> f64 {
        let first = self.groups.front().map(|g| g.start()).or(self.cur_packets.first().map(|p| p.pts));
        match first {
            Some(f) => (self.last_pts - f + 1) as f64 / self.fps as f64,
            None => 0.0,
        }
    }

    /// Bytes held (RAM in RAM mode, disk in disk mode).
    pub fn held_bytes(&self) -> u64 {
        self.total_bytes + self.cur_data.len() as u64
    }

    pub fn push(&mut self, pts: i64, key: bool, data: &[u8]) {
        if key && !self.cur_packets.is_empty() {
            self.close_group();
        }
        if self.cur_packets.is_empty() && !key {
            return; // nothing before the first keyframe is playable
        }
        self.cur_packets.push(PacketInfo { pts, key, offset: self.cur_data.len() as u32, len: data.len() as u32 });
        self.cur_data.extend_from_slice(data);
        self.last_pts = pts;
        self.trim();
    }

    fn close_group(&mut self) {
        let bytes = std::mem::replace(&mut self.cur_data, Vec::with_capacity(8 << 20));
        let packets = std::mem::take(&mut self.cur_packets);
        let n = bytes.len() as u64;
        let data = match self.mode {
            BufferMode::Ram => GroupData::Ram(bytes),
            BufferMode::Disk => {
                let path = self.dir.join(format!("group_{:08}.bin", self.seq));
                match std::fs::write(&path, &bytes) {
                    Ok(()) => GroupData::Disk(path),
                    Err(e) => {
                        crate::log!("disk buffer write failed ({e}); keeping this part in RAM");
                        GroupData::Ram(bytes)
                    }
                }
            }
        };
        self.seq += 1;
        self.total_bytes += n;
        self.groups.push_back(Group { packets, data: Arc::new(data), bytes: n });
    }

    fn trim(&mut self) {
        // Drop the oldest group while the rest still covers the replay length.
        while self.groups.len() >= 2 {
            let second = self.groups[1].start();
            if self.last_pts - second + 1 >= self.keep_slots {
                let g = self.groups.pop_front().unwrap();
                self.total_bytes -= g.bytes;
            } else {
                break;
            }
        }
    }

    /// Everything needed to write a replay, taken quickly under the lock.
    pub fn snapshot(&self) -> Option<Snapshot> {
        let mut groups: Vec<(Vec<PacketInfo>, Arc<GroupData>)> =
            self.groups.iter().map(|g| (g.packets.clone(), g.data.clone())).collect();
        if !self.cur_packets.is_empty() {
            groups.push((self.cur_packets.clone(), Arc::new(GroupData::Ram(self.cur_data.clone()))));
        }
        if groups.is_empty() {
            return None;
        }
        // Keep only the last `keep_slots` worth, starting at a keyframe group.
        let last = self.last_pts;
        while groups.len() >= 2 && last - groups[1].0[0].pts + 1 >= self.keep_slots {
            groups.remove(0);
        }
        Some(Snapshot { groups })
    }
}

pub struct Snapshot {
    pub groups: Vec<(Vec<PacketInfo>, Arc<GroupData>)>,
}

impl Snapshot {
    pub fn first_pts(&self) -> i64 {
        self.groups[0].0[0].pts
    }
    pub fn last_pts(&self) -> i64 {
        self.groups.last().and_then(|g| g.0.last()).map_or(0, |p| p.pts)
    }
}

/// Remove leftover files from a previous run (only our own files).
fn clean_dir(dir: &PathBuf) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if (name.starts_with("group_") && name.ends_with(".bin")) || (name.starts_with("chunk_") && name.ends_with(".ts")) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

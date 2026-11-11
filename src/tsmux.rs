//! Minimal MPEG-TS writer for one video stream. Lets us hand ffmpeg each
//! frame with its exact capture time, so we only encode frames that changed
//! (variable frame rate) and never need filler repeats to stay in sync.

const PID_PMT: u16 = 0x1000;
const PID_VIDEO: u16 = 0x0100;

pub struct TsMux {
    stream_type: u8,
    cc_pat: u8,
    cc_pmt: u8,
    cc_vid: u8,
    out: Vec<u8>,
}

impl TsMux {
    /// `hevc`: true for H.265, false for H.264.
    pub fn new(hevc: bool) -> TsMux {
        TsMux { stream_type: if hevc { 0x24 } else { 0x1B }, cc_pat: 0, cc_pmt: 0, cc_vid: 0, out: Vec::with_capacity(1 << 20) }
    }

    /// Packs one encoded frame. `pts` is in 90 kHz ticks. Returns the bytes to send.
    pub fn frame(&mut self, data: &[u8], pts: u64, keyframe: bool) -> &[u8] {
        self.out.clear();
        if keyframe {
            self.psi_tables();
        }
        // PES header: video stream, unbounded length, PTS only (no B-frames, so DTS == PTS).
        let mut pes = Vec::with_capacity(14);
        pes.extend_from_slice(&[0x00, 0x00, 0x01, 0xE0, 0x00, 0x00, 0x80, 0x80, 0x05]);
        pes.extend_from_slice(&encode_ts(0x2, pts));

        let total = pes.len() + data.len();
        let mut written = 0usize;
        let mut first = true;
        while written < total {
            let mut pkt = [0xFFu8; 188];
            pkt[0] = 0x47;
            pkt[1] = (if first { 0x40 } else { 0 }) | ((PID_VIDEO >> 8) as u8 & 0x1f);
            pkt[2] = PID_VIDEO as u8;
            let remaining = total - written;
            // Adaptation field contents (after its length byte), or None for no field.
            let mut af: Option<Vec<u8>> = None;
            if first {
                // PCR, plus the random-access flag on keyframes.
                let mut v = vec![0x10 | if keyframe { 0x40 } else { 0 }];
                v.extend_from_slice(&encode_pcr(pts.saturating_sub(9000)));
                af = Some(v);
            }
            let room = 184 - af.as_ref().map_or(0, |v| v.len() + 1);
            if remaining < room {
                // Last packet of the frame: stuff so the payload ends exactly at byte 188.
                let af_total = 184 - remaining; // including the length byte
                let v = af.get_or_insert_with(Vec::new);
                if af_total >= 2 && v.is_empty() {
                    v.push(0x00); // flags byte, nothing set
                }
                while v.len() + 1 < af_total {
                    v.push(0xFF);
                }
            }
            let has_af = af.is_some();
            let af = af.unwrap_or_default();
            pkt[3] = (if has_af { 0x30 } else { 0x10 }) | (self.cc_vid & 0x0f);
            self.cc_vid = self.cc_vid.wrapping_add(1);
            let mut pos = 4;
            if has_af {
                pkt[4] = af.len() as u8;
                pkt[5..5 + af.len()].copy_from_slice(&af);
                pos = 5 + af.len();
            }
            let take = (188 - pos).min(remaining);
            for i in 0..take {
                let idx = written + i;
                pkt[pos + i] = if idx < pes.len() { pes[idx] } else { data[idx - pes.len()] };
            }
            written += take;
            self.out.extend_from_slice(&pkt);
            first = false;
        }
        &self.out
    }

    fn psi_tables(&mut self) {
        // PAT: program 1 -> PMT PID
        let pat = [
            0x00, 0xB0, 0x0D, 0x00, 0x01, 0xC1, 0x00, 0x00, 0x00, 0x01,
            0xE0 | (PID_PMT >> 8) as u8, PID_PMT as u8,
        ];
        let pkt = section_packet(0x0000, &mut self.cc_pat, &pat);
        self.out.extend_from_slice(&pkt);
        // PMT: PCR on the video PID, one video stream.
        let pmt = [
            0x02, 0xB0, 0x12, 0x00, 0x01, 0xC1, 0x00, 0x00,
            0xE0 | (PID_VIDEO >> 8) as u8, PID_VIDEO as u8, 0xF0, 0x00,
            self.stream_type, 0xE0 | (PID_VIDEO >> 8) as u8, PID_VIDEO as u8, 0xF0, 0x00,
        ];
        let pkt = section_packet(PID_PMT, &mut self.cc_pmt, &pmt);
        self.out.extend_from_slice(&pkt);
    }
}

fn section_packet(pid: u16, cc: &mut u8, section: &[u8]) -> [u8; 188] {
    let mut pkt = [0xFFu8; 188];
    pkt[0] = 0x47;
    pkt[1] = 0x40 | ((pid >> 8) as u8 & 0x1f);
    pkt[2] = pid as u8;
    pkt[3] = 0x10 | (*cc & 0x0f);
    *cc = cc.wrapping_add(1);
    pkt[4] = 0; // pointer field
    pkt[5..5 + section.len()].copy_from_slice(section);
    let crc = crc32(section);
    pkt[5 + section.len()..9 + section.len()].copy_from_slice(&crc.to_be_bytes());
    pkt
}

fn encode_ts(prefix: u8, ts: u64) -> [u8; 5] {
    let ts = ts & 0x1_FFFF_FFFF;
    [
        (prefix << 4) | (((ts >> 30) as u8 & 0x07) << 1) | 1,
        (ts >> 22) as u8,
        (((ts >> 15) as u8) << 1) | 1,
        (ts >> 7) as u8,
        ((ts as u8) << 1) | 1,
    ]
}

fn encode_pcr(base: u64) -> [u8; 6] {
    let b = base & 0x1_FFFF_FFFF;
    [
        (b >> 25) as u8,
        (b >> 17) as u8,
        (b >> 9) as u8,
        (b >> 1) as u8,
        (((b & 1) as u8) << 7) | 0x7E,
        0x00,
    ]
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= (b as u32) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 { (crc << 1) ^ 0x04C1_1DB7 } else { crc << 1 };
        }
    }
    crc
}

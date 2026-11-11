# Replay buffer storage, containers, frame timing and playback compatibility

Research date: 2026-11-06. Target: an always-on Windows 10 IoT Enterprise LTSC 2021 (build 19044, no Microsoft Store) replay buffer, 1-10 min, 1080p up to 144 fps, desktop audio and mic, RTX 40 GPU, clips watched in Windows Media Player and shared on Discord/WhatsApp. The current prototype uses a custom MPEG-TS muxer piped into a long-running ffmpeg that encodes AAC, cuts 10 s TS chunks, keeps them in RAM or on a SATA SSD, and remuxes to MP4 on save. It uses VFR.

Arithmetic in these notes (bitrates, RAM, TBW) is computed here and labelled as an inference. It is not quoted from a source.

---

## 1. How OBS, ShadowPlay and Steam store their replay buffers

### Takeaway
OBS keeps refcounted encoded packets in an in-memory deque. It trims the deque so it always starts on a video keyframe, and on save it streams the packets to a separate `obs-ffmpeg-mux.exe` that writes the MP4. ShadowPlay buffers to temp files on disk (by default on the OS drive). Steam's background recording is an on-disk rolling buffer of fragmented MP4 (DASH `.m4s`) chunks with a user-set disk cap. None of the three reruns an encoder or a long-lived ffmpeg on the hot path.

### Cited Findings
**OBS Studio (`plugins/obs-ffmpeg/obs-ffmpeg-mux.c`, master branch, read 2026-11)**
- The replay buffer packet store is a `deque` (`stream->packets`) of `struct encoder_packet` copies, not a circlebuf. The data is refcounted and released with `obs_encoder_packet_release`. Bookkeeping fields are `cur_size` (bytes), `cur_time` (dts in µs of the oldest packet), `keyframes` (count of video keyframes held), `max_size`, `max_time`, `save_ts` and `muxing`. — [OBS source, obs-ffmpeg-mux.c](https://raw.githubusercontent.com/obsproject/obs-studio/master/plugins/obs-ffmpeg/obs-ffmpeg-mux.c)
- Plugin defaults (`replay_buffer_defaults`): `max_time_sec` = 15, `max_size_mb` = 500, `extension` = mp4. `replay_buffer_start` converts these to `max_time = max_time_sec * 1000000` µs and `max_size = max_size_mb * 1024*1024` bytes. Both a time limit and a memory limit apply. — [OBS source](https://raw.githubusercontent.com/obsproject/obs-studio/master/plugins/obs-ffmpeg/obs-ffmpeg-mux.c)
- Trimming: `replay_buffer_purge()` runs before each new packet is pushed. If `max_size` is set and more than two keyframes are held, it purges while `cur_size + pkt->size > max_size`. It then purges while `pkt->dts_usec - cur_time > max_time`. `purge()` removes the front packet. If that packet was a video keyframe, it keeps popping until the new front is a video keyframe, so **the buffer always begins on a keyframe** and trimming happens one GOP at a time. Only video keyframes count as trim boundaries. — [OBS source](https://raw.githubusercontent.com/obsproject/obs-studio/master/plugins/obs-ffmpeg/obs-ffmpeg-mux.c)
- Save is deferred. `save()` records `save_ts = os_gettime_ns()/1000`. The save actually fires in `replay_buffer_data()` when a packet with `sys_dts_usec >= save_ts` arrives, so the clip runs up to the moment of the hotkey press, including encoder latency. — [OBS source](https://raw.githubusercontent.com/obsproject/obs-studio/master/plugins/obs-ffmpeg/obs-ffmpeg-mux.c)
- `replay_buffer_save()` copies the deque into `mux_packets`, sorted by `dts_usec`. It rebases timestamps against the first video PTS and against the first audio DTS of each track, then starts `replay_buffer_mux_thread()`. That thread launches the external `obs-ffmpeg-mux.exe` with `os_process_pipe_create2(args,"w")`. The command line carries codec, size, colour, fps num/den and the audio tracks. The thread sends codec extradata (headers) and then each packet as a small `ffm_packet_info` header (pts, dts, size, index, type, keyframe) followed by the payload. **OBS muxes the saved file out-of-process, but only at save time.** Nothing runs continuously in a subprocess. — [OBS source](https://raw.githubusercontent.com/obsproject/obs-studio/master/plugins/obs-ffmpeg/obs-ffmpeg-mux.c)
- Users have asked OBS for a disk-backed replay buffer, which shows the OBS buffer is RAM-only. — [OBS forum: Enable Replay Buffer to Disk instead of RAM](https://obsproject.com/forum/threads/enable-replay-buffer-to-disk-instead-of-ram.64095)

**NVIDIA ShadowPlay / Instant Replay**
- ShadowPlay's temp files go to the OS drive, not the output folder. — [Techgage](https://techgage.com/news/nvidia-adds-shadowplay-fine-tuning-options-in-latest-geforce-experience-release/)
- The temp folder can be changed. One guide recommends putting temp and save locations on a drive other than the game drive. — [Tom's Hardware forum](https://forums.tomshardware.com/threads/if-i-manually-record-with-the-geforce-experience%E2%80%99s-video-software-does-it-still-create-tmp-temporary-files-on-my-ssd.3659059); [Linus Tech Tips](https://linustechtips.com/topic/812083-performance-impact-of-nvidia-shadowplay-instant-replay-testing/)
- Users set up RAM disks for Instant Replay, which implies the buffer is file-based. — [SweClockers guide](https://www.sweclockers.com/forum/post/20133225)
- In the NVIDIA App, the save location can only be changed while Instant Replay is off. — [Shacknews](https://shacknews.com/article/142202/change-shadowplay-save-file-location-nvidia-app?amphtml=1)
- Adobe's official FAQ names ShadowPlay output as a common source of VFR media. — [Adobe FAQ: VFR in Premiere Pro](https://community.adobe.com/t5/premiere-pro/faq-variable-frame-rate-vfr-video-files/td-p/10348229)

**Steam Game Recording (Valve, out of beta since 2024)**
- The user sets the maximum disk space for background recording. When it fills, the oldest recordings are overwritten. Nothing is kept permanently unless clipped or saved. — [Steam Support](https://help.steampowered.com/faqs/view/23B7-49AD-4A28-9590)
- Steam estimates the disk space needed from length and quality. The default "High" quality is about 12 Mbps. Users report 10.8 to 34.2 GB for 120 min at High (user-reported figures). — [Steam Support](https://help.steampowered.com/faqs/view/23B7-49AD-4A28-9590); [Tom's Guide](https://tomsguide.com/gaming/pc-gaming/how-to-record-steam-games-and-make-your-videos-look-their-best)
- On disk, Steam stores fragmented MP4 DASH segments. Each folder (`bg_<appid>_…` for background recording, `fg_…` for manual) holds `session.mpd`, `init-stream0.m4s` and numbered `chunk-stream0-*.m4s` files, with audio as separate streams. Third-party tools rebuild an MP4 by concatenating init plus chunks, or by stream-copying `session.mpd` with ffmpeg, without re-encoding. (These come from community tools, not Valve documentation.) — [steamclipconverter crate](https://rust-digger.code-maven.com/crates/steamclipconverter); [GitHub gist by safijari](https://gist.github.com/safijari/afa41cb017eb2d0cadb20bf9fcfecc93)
- A Nov 2024 user report says Steam HEVC recordings showed a frozen image with sound in Discord on PC. The suggested workaround was to turn off HEVC recording in Steam. — [Steam Client Beta discussion](https://steamcommunity.com/groups/SteamClientBeta/discussions/5/6400272691576721584)

### Inferences
- Two proven patterns exist. One is an in-RAM encoded-packet ring with trimming per GOP and muxing only on save (OBS). The other is an on-disk rolling set of fMP4 or temp-file segments (ShadowPlay, Steam). Neither runs a persistent transcoding subprocess. The prototype's long-running ffmpeg is the outlier.
- OBS's "wait until a packet past the press time arrives" rule makes the clip include the last frames before the hotkey. A design that saves immediately loses the frames still inside the encoder pipeline, which is usually tens of ms.
- Because OBS trims per GOP, the saved clip is 0 to 1 GOP longer than the setting. With 10 s TS chunks, the prototype's granularity is coarser, up to 10 s, unless it cuts inside chunks.

### Gaps
- No official NVIDIA documentation was found on ShadowPlay's temp-file format, segment size or default path in the NVIDIA App. The Techgage information dates from the GeForce Experience era.
- It was not confirmed whether `obs-ffmpeg-mux` writes a non-fragmented MP4 with moov at the end or at the front for replay saves.
- No official Valve documentation was found on the chunk duration of Steam background recording.

---

## 2. Packet ring vs chunked files vs subprocess pipeline; writing MP4 in-process vs ffmpeg on save

### Takeaway
An in-process ring of encoded packets (plus a small index of pts, dts, keyframe flag and size) is the simplest and cheapest design with the finest trim granularity, and is the one OBS uses. Add a disk spill tier (fMP4 fragments) only when RAM cost becomes a problem. Writing the MP4 in-process at save time is easy because every packet size is known in advance, so `moov` can be written at the front in a single pass. That removes the faststart second pass and the dependency on ffmpeg.

### Cited Findings
- OBS shows the in-process ring works in production. Packets are refcounted, trimmed per keyframe, and limited by both time and bytes (see section 1). — [OBS source](https://raw.githubusercontent.com/obsproject/obs-studio/master/plugins/obs-ffmpeg/obs-ffmpeg-mux.c)
- ffmpeg's MP4 muxer: a normal MOV/MP4 "is undecodable if it is not properly finished". With fragmentation, "the file is decodable even if the writing is interrupted". `faststart` will "Run a second pass moving the index (moov atom) to the beginning of the file". `frag_keyframe` will "start a new fragment at each video keyframe". `delay_moov` will "delay writing the initial moov until the first fragment is cut". — [FFmpeg formats documentation](https://ffmpeg.org/ffmpeg-formats.html)
- ffmpeg segment muxer: splitting "may not be accurate, unless you force the reference stream key-frames at the given time". `break_non_keyframes` allows non-keyframe starts. `segment_wrap` wraps the index. `reset_timestamps` gives each segment near-zero timestamps. The `fifo` pseudo-muxer separates encoding from muxing with a FIFO queue. — [FFmpeg formats documentation](https://ffmpeg.org/ffmpeg-formats.html)
- Matroska muxer: cues (the seek index) go at the end by default and "are only written if the output is seekable". — [FFmpeg formats documentation](https://ffmpeg.org/ffmpeg-formats.html)
- Rust options: `muxide` (v0.2.0) is a pure-Rust MP4 muxer for already-encoded streams. It "does not encode or decode" and has a `fragmented::FragmentedMuxer` (init segment, `flush_segment()`) and a builder for regular MP4. Its roadmap still lists some items, such as CLI fMP4, as unchecked, so the crate is young. — [docs.rs muxide](https://docs.rs/muxide); [muxide ROADMAP](https://docs.rs/crate/muxide/latest/source/ROADMAP.md)
- Steam rebuilds clips from fMP4 chunks by plain concatenation (init + chunks) or ffmpeg stream copy, which shows that an fMP4 disk ring can be assembled into a file without re-encoding. — [GitHub gist](https://gist.github.com/safijari/afa41cb017eb2d0cadb20bf9fcfecc93); [steamclipconverter](https://rust-digger.code-maven.com/crates/steamclipconverter)

### Inferences
Trade-off matrix (my synthesis):

| | In-process packet ring (RAM) | Chunked files on disk (fMP4/TS) | Long-running ffmpeg subprocess (current) |
|---|---|---|---|
| CPU | Lowest. A memcpy per packet and trims that pop from the front | Low. Fragment writes about every GOP or every N s | Extra process: TS mux in Rust, then pipe, then TS demux, AAC encode and TS mux in ffmpeg. Small in absolute terms, but always on |
| RAM | Bitrate × duration (see section 5). 3.75 GB for 10 min at 50 Mbps | Near zero, just the current fragment | Depends on whether chunks sit in RAM |
| Trim granularity | One GOP, typically 1-2 s | One fragment (1 GOP if `frag_keyframe`) | 10 s chunk |
| Crash safety | Buffer lost if the app crashes. Acceptable for a replay buffer | Survives an app crash. fMP4 stays decodable if interrupted | ffmpeg crash or stall loses or corrupts the buffer. Two processes to supervise |
| Save latency | Write N bytes to the output disk. No re-read | Read the fragments back and write the output (2× I/O), or rename and concat | Remux of TS to MP4 by ffmpeg: read, parse and rewrite |
| Complexity | Low. One data structure plus an MP4 writer | Medium. Rotation, index, cleanup of stale files after a crash | High. Timestamps cross a pipe and a second timebase, plus process lifecycle |
| AV1 | Fine | Fine with fMP4 or MKV | Blocked if TS is the carrier (section 3) |

- Writing MP4 in-process on save: all samples are in hand, so the writer can compute `stsz`, `stco`/`co64`, `stts`, `ctts` and `stss` first and emit `ftyp`, `moov` and then `mdat` in one sequential pass. That gives a "faststart" file without ffmpeg's second pass. At 7.5 GB (10 min at 100 Mbps) the `mdat` needs a 64-bit size and `co64` chunk offsets, because files over 4 GiB overflow 32-bit `stco`. Any crate chosen must support this; check muxide before relying on it.
- Calling ffmpeg on save as a separate process (OBS's approach) is a reasonable fallback. It keeps libavformat's movenc quirks (edit lists, AV1 `av1C`, HEVC `hvcC`) out of your own code. The cost is shipping `ffmpeg.exe` and piping GBs at save time. Linking libavformat (for example the `ffmpeg-next` or `rsmpeg` crates) gives the same muxer in-process but adds a large DLL dependency.
- Audio: keeping raw PCM in the ring and encoding AAC at save time pushes CPU into the save. A better approach is to encode audio continuously in-process, for example with the Windows Media Foundation AAC encoder or libfdk/ffmpeg's AAC, and keep audio packets in the same ring. Audio is tiny (about 24 MB for two 160 kbps tracks over 10 min).
- Hybrid for long or high-bitrate buffers: keep the newest M seconds in RAM and spill closed GOPs to a pre-allocated fMP4 ring file on the SSD. Save then writes RAM packets plus file-backed packets into one MP4.

### Gaps
- No benchmarks were found for the CPU cost of a persistent ffmpeg TS remux at 50-100 Mbps versus an in-process ring. The assessment that it is small but nonzero is my inference.
- muxide's support for AV1 (`av1C`), HEVC (`hvcC`), Opus, 64-bit `co64` and multiple audio tracks was not checked.

---

## 3. Containers (MP4, fMP4, MKV, MPEG-TS) for AV1/HEVC/H.264 and AAC/Opus/FLAC; ffmpeg AV1-in-TS; platform acceptance; faststart

### Takeaway
**ffmpeg still cannot mux AV1 into MPEG-TS as of the latest changelog (9.1 / `<next>`).** The patch series was split, and the muxer part was pulled while the AOM spec is still a draft. TS is therefore a dead end for AV1. The final deliverable should be a regular (non-fragmented) MP4 with moov at the front, H.264 or HEVC/AV1 video and AAC-LC audio. MKV is fine for local use but does not embed on Discord. Use fMP4 or plain packet memory as the intermediate.

### Cited Findings
**AV1 in MPEG-TS (ffmpeg)**
- The FFmpeg master Changelog has headers up to `<next>`, 9.1, 9.0, 8.1 and 8.0, with no entry for AV1 in MPEG-TS, muxer or demuxer. The nearest TS entries are "LCEVC track muxing support in MPEG-TS muxer" (9.1) and "LCEVC enhancement layer exporting in MPEG-TS" (8.1). AV1 in MP4 has been supported since 4.1 and "CENC AV1 support in MP4 muxer" since 8.0. (Release dates are not in the file.) — [FFmpeg Changelog (master)](https://raw.githubusercontent.com/FFmpeg/FFmpeg/master/Changelog)
- Dec 2025: Jun Zhao opened PR #21301 (an AV1 TS bitstream filter wired into the mpegts muxer and demuxer) and then a V2, PR #21307, following the AOM draft "Carriage of AV1 in MPEG-2 TS". It converts AV1 from low-overhead format to start-code format and adds an `AV01` registration descriptor. — [ffmpeg-devel PR #21307](https://mail-archive.com/ffmpeg-devel@ffmpeg.org/msg187019.html); [ffmpeg-devel PR #21301](https://go.mail-archive.com/ffmpeg-devel@ffmpeg.org/msg187008.html)
- Jan 2026: Christophe Gisquet objected to merging before the draft spec is final. Jun Zhao replied that the muxer part had been removed after James Almer's comment. — [ffmpeg-devel Jan 2026](https://go.mail-archive.com/ffmpeg-devel@ffmpeg.org/msg187203.html)
- GStreamer added a "provisional AV1 mapping" for mpegts in late 2024. VLC has an open MR for AV1-in-TS demuxing. — [GStreamer mpegtsmux commits](https://git.fasttube.de/FaSTTUBe/GST-Tensordecoder-ov_ep/commits/branch/OpenCL_acceleration/subprojects/gst-plugins-bad/gst/mpegtsmux); [VLC MR 6836](https://code.videolan.org/videolan/vlc/-/merge_requests/6836)

**MP4 / fMP4 / MKV behaviour**
- Non-fragmented MP4 is undecodable if not finalised. Fragmented MP4 is decodable after interruption. `faststart` is a second pass that moves `moov` to the front. — [FFmpeg formats documentation](https://ffmpeg.org/ffmpeg-formats.html)
- The ffmpeg mov muxer's `use_editlist` defaults to `auto`. — [FFmpeg formats documentation](https://ffmpeg.org/ffmpeg-formats.html)
- Matroska writes cues only to seekable output. — [FFmpeg formats documentation](https://ffmpeg.org/ffmpeg-formats.html)

**YouTube**
- Recommended upload: MP4 with "No Edit Lists (or the video might not get processed correctly)" and "moov atom at the front of the file (Fast Start)". Audio is AAC-LC, Opus or Eclipsa at 48 kHz. Video is H.264 progressive, High Profile, closed GOP of half the frame rate, CABAC, 4:2:0. "Content should be encoded and uploaded in the same frame rate it was recorded". 1080p60 SDR is 12 Mbps, VBR. The page does not mention VFR, HEVC or AV1. — [YouTube recommended upload encoding settings](https://support.google.com/youtube/answer/1722171?hl=en)

**Discord**
- Official patch notes, 1 July 2024: "Discord now supports in-app playback of HEVC (H.265) video attachments uploaded from desktop and web platforms." No hardware requirement is stated. — [Discord patch notes 2024-07-01](https://discord.com/blog/discord-patch-notes-july-1-2024)
- Contradicting reports: Steam HEVC clips froze on Discord PC in Nov 2024 ([Steam discussion](https://steamcommunity.com/groups/SteamClientBeta/discussions/5/6400272691576721584)), and HEVC failed in the Linux Discord client and Firefox in 2025 ([Arch forum](https://bbs.archlinux.org/viewtopic.php?id=304003)).
- AV1: a community post claims linked AV1 videos with the right headers embed, but uploaded files are treated as generic downloads. This is anecdotal. — [Discord community: H.265/AV1 for uploaded files](https://support.discord.com/hc/ru/community/posts/13931826693143-Support-for-H265-or-AV1-for-uploaded-video-files-to-play-in-app)
- MKV does not embed. A user workaround renames an MKV with H.264/AAC to `.webm`. — [Discord community: MKV support](https://support.discord.com/hc/vi/community/posts/360052829453-MKV-support)
- Older threads (2019-2023) say HEVC was not embedded and recipients had to download. These are superseded for desktop and web by the July 2024 notes. — [Discord community: HEVC playback](https://support.discord.com/hc/it/community/posts/360051091674-Support-Video-Playback-for-HEVC-H-265-Videos)

**WhatsApp / Telegram**
- No official WhatsApp codec documentation was found. Third-party guides agree that MP4 with H.264 and AAC is the safe choice, and that HEVC, AV1 and VP9 may fail or be transcoded. WhatsApp re-encodes videos sent as media. Sending as a document avoids re-encoding but gives no inline playback. Size limits conflict between sources: 16 MB inline versus 2 GB. — [xconvert guide](https://www.xconvert.com/blog/?p=774); [convertintomp4 guide](https://convertintomp4.com/blog/best-video-format-for-whatsapp); [Samsung community: WhatsApp doesn't support HEVC](https://r2.community.samsung.com/t5/Galaxy-S/Whats-app-doesn-t-support-HEVC-H-265-VIDEO-FORMAT-IN-S-SERIES/td-p/19252312)
- Telegram plays HEVC in regular messages, and ordinary sent videos may be transcoded server-side. Video avatars require 8-bit H.264. (Developer blog posts and the Telegram bug tracker, not official documentation.) — [dev.to liveavabot](https://dev.to/liveavabot/telegram-video-avatars-fixing-iphone-hevc-files-with-ffmpeg-24fo); [Telegram bugs #29058](https://bugs.telegram.org/c/29058)

### Inferences
- Container per stage:
  - Hot buffer: no container. Keep raw packets in memory, or use fMP4 fragments on disk, which are crash-safe and accept AV1, HEVC, H.264, AAC, Opus and FLAC.
  - Saved clip: non-fragmented MP4 with moov at the front (YouTube requires it, and it lets Discord and WhatsApp start playback before the full download) and no edit lists where possible. Avoiding B-frames, which low-latency NVENC presets usually do, together with zero-based rebased timestamps avoids the need for an edit list.
  - Audio: AAC-LC at 48 kHz in MP4 is universally safe. Opus-in-MP4 and FLAC-in-MP4 are valid (ffmpeg muxes them) but carry risk in WMP legacy, WhatsApp and some editors. Keep FLAC or Opus as an "archival/MKV" option only.
- MPEG-TS adds 188-byte packet overhead (about 2-4%) and offers nothing a replay buffer needs once the long-running ffmpeg pipe is gone. AV1 makes it unusable today.
- Multiple audio tracks (desktop plus mic as separate tracks) are legal in MP4, but Discord, WhatsApp and browsers play only the first track. A shareable clip needs a mixed track as track 1. Separate tracks are useful for editing.

### Gaps
- No official 2026 confirmation of Discord AV1 upload playback, or of Discord HEVC playback in the in-browser embed on browsers without HEVC hardware decode.
- No official WhatsApp Help Center page on codecs or the current inline size limit was retrieved.
- It was not verified whether WMP legacy on Windows 10 plays fragmented MP4 smoothly, especially seeking.
- It was not verified that YouTube accepts HEVC and AV1 uploads; the supported-formats page was not fetched.

---

## 4. VFR vs CFR; what ShadowPlay and OBS output; timebase choice

### Takeaway
Editors are the weak point for VFR. Adobe officially says VFR (explicitly from ShadowPlay and OBS) can cause audio drift and recommends transcoding to CFR. Resolve users report similar problems. Players and web embeds handle VFR fine. Because NVENC is a fixed-function block and a repeated frame encodes to a near-empty P-frame, a CFR default (resubmit the last surface when no new frame arrives) costs little and removes the editor problem. VFR can stay as an opt-in "smaller files" mode. Use a timebase that divides evenly by the target fps.

### Cited Findings
- Adobe (official Premiere Pro FAQ): VFR sources include "game-play recording software like NVIDIA ShadowPlay and other screen capture applications like OBS Studio". "Playing VFR media in Premiere Pro may result in the audio being out of sync". Premiere's "Preserve Audio Sync" option "may not work as intended due to extent of variation between the Minimum and Maximum frame rate". The FAQ recommends transcoding to CFR. MediaInfo's "Frame Rate Mode: Variable" identifies VFR. — [Adobe FAQ: VFR media in Premiere Pro](https://community.adobe.com/t5/premiere-pro-discussions/faq-how-to-work-with-variable-frame-rate-vfr-media-in-premiere-pro/td-p/10348229)
- A 60 fps game recording on a 60 fps Premiere timeline drifts progressively. The fix is conversion to CFR. — [PremiumBeat](https://www.premiumbeat.com/blog/sync-audio-video-game-capture/)
- DaVinci Resolve: "Variable framerates are very bad for post" (user, Blackmagic forum, OBS NVENC thread). A Resolve product manager noted that an odd rate like 23.976026 suggests a VFR clip. — [Blackmagic forum](https://forum.blackmagicdesign.com/viewtopic.php?p=474425); [Blackmagic forum: FPS not matching](https://forum.blackmagicdesign.com/viewtopic.php?p=736857)
- Vegas vs Resolve: a VEGAS forum thread is titled "OBS created VFR video out of sync - vegas in sync - Resolve", meaning Vegas kept sync where Resolve did not (title only; not read in full). — [VEGAS forum](https://www.vegascreativesoftware.info/us/forum/obs-created-vfr-video-out-of-sync-vegas-in-sync-resolve--115691)
- OBS: users report that even with CFR, MediaInfo sometimes shows "Variable", and that a late frame forces OBS to duplicate the previous frame. Users describe ShadowPlay as encoding only the frames the card produces. (Forum claims, unverified.) — [OBS forum: duplicate frames](https://obsproject.com/forum/threads/local-recording-too-many-duplicate-frames.42201); [OBS forum: NVENC OBS vs ShadowPlay](https://obsproject.com/forum/threads/nvenc-obs-vs-nvenc-shadowplay.54698)
- OBS passes a fixed `fps num/den` to the muxer on save (part of the `build_command_line` arguments). OBS's output is nominally CFR at the canvas fps. — [OBS source](https://raw.githubusercontent.com/obsproject/obs-studio/master/plugins/obs-ffmpeg/obs-ffmpeg-mux.c)
- Remuxing an MKV recording to MP4 can produce a file editors flag as VFR. The suggestion is to record to hybrid or fragmented MP4 instead. (Forum, user claim.) — [VEGAS forum](https://www.vegascreativesoftware.info/us/forum/60-fps-footage-does-not-look-60-fps-after-rendering--147095)
- YouTube: "Content should be encoded and uploaded in the same frame rate it was recorded." — [YouTube encoding settings](https://support.google.com/youtube/answer/1722171?hl=en)

### Inferences
- Encode-time decision: VFR to CFR cannot be fixed losslessly at save time. Duplicating an encoded P-frame packet is not a repeat, because its residual would apply twice. Synthesising skip frames in the bitstream is possible but codec-specific and fragile. CFR must therefore be produced by the encoder, by resubmitting the last captured (already NV12-converted) surface when DXGI Desktop Duplication reports no new frame. The cost is NVENC ASIC time plus a nearly empty bitstream frame. No colour conversion is repeated and no extra 3D or compute work is needed, so the game impact should be negligible. (No benchmark found; see Gaps.)
- Rate control: NVENC's rate control budgets bits per frame from the declared frame rate. Feeding VFR into an encoder configured for 144 fps makes actual bitrate and quality drift with the true frame rate. CFR makes the bitrate setting honest, which also makes the RAM and disk maths in section 5 predictable.
- Timebase:
  - CFR: use a stream timebase of 1/fps (for example 1/144), or 1/90000. 90 kHz divides evenly by 24, 25, 30, 48, 50, 60, 120, 144 and 240 fps (90000/144 = 625). It does not divide evenly by 165 fps (545.45) or NTSC 59.94. For odd refresh rates (165, 175, 240) use 1/fps or a multiple of it.
  - VFR: 90 kHz (11.1 µs) or a 1 µs base is fine. The 1 ms base that Matroska uses by default is too coarse at 144 fps: 6.944 ms gets rounded to 7/7/7/7/6 ms, which adds jitter.
  - QPC 100 ns ticks (10 MHz) work internally. In MP4, a 32-bit `mdhd` duration at 10 MHz overflows after about 429 s, so a version-1 `mdhd` would be required. Rescale to 90 kHz or 1/fps when muxing.
- Snapping VFR timestamps to the 1/fps grid gives a "CFR with holes" file. It is cleaner than raw QPC times, but MediaInfo and Premiere still treat it as VFR.
- Playback: browsers and Media Foundation honour per-frame timestamps, so VFR plays correctly in WMP, Discord and WhatsApp. VFR mainly affects editing (Premiere, Resolve) and some re-encoders.

### Gaps
- No primary source was found on how CapCut handles VFR game clips.
- No measurement was found of NVENC cost per repeated or static frame on Ada.
- It was not confirmed whether current ShadowPlay (NVIDIA App 2025-2026) still writes VFR. The Adobe FAQ listing it predates the NVIDIA App.
- Discord's own handling of VFR was not documented. "Plays fine" is inferred from HTML5 video behaviour.

---

## 5. Playback on Windows 10 LTSC 19044, hardware decode, and disk/RAM cost

### Takeaway
On LTSC 2021 without the Store, H.264+AAC MP4 plays in WMP legacy with no extra codecs. HEVC and AV1 need the Microsoft extension appx packages sideloaded with `Add-AppxPackage`, which is possible but unofficial for the free HEVC "Device Manufacturer" package, and WMP legacy support is mixed. An RTX 40 card hardware-decodes H.264, HEVC and AV1. A 24/7 disk buffer at 50-100 Mbps would wear out an 860 EVO's warranty TBW in under 1.5 to 3 years. RAM for 10 min ranges from about 1.1 GB at 15 Mbps to about 7.5 GB at 100 Mbps.

### Cited Findings
**HEVC/AV1 extensions on LTSC**
- A Habr guide (May 2024) gets the HEVC Video Extensions bundle through a third-party Store link generator and installs it from elevated PowerShell with `Add-AppxPackage <bundle>`. A commenter reports doing the same on Windows 10 LTSC Enterprise, for example with `Microsoft.HEVCVideoExtension_2.2.33.0_neutral_~_8wekyb3d8bbwe.AppxBundle` (an old version). — [Habr](https://habr.com/ru/sandbox/217109); [chipp.in comment](https://chipp.in/uncategorized/how-)
- The free "HEVC Video Extensions from Device Manufacturer" package (OEM preinstall) is reported as no longer listed on the Store. The paid HEVC Video Extensions ($0.99) is the same codec. — [Microsoft Q&A](https://learn.microsoft.com/en-us/answers/a/1180572); [ravbug HEVC on Windows 10](https://www.ravbug.com/tutorials/hevc-windows10)
- Microsoft Q&A notes an RCE vulnerability in these HEVC apps, fixed in versions 2.0.51121.0 / 2.0.51122.0 and later. Any sideloaded package should be at least that version. — [Microsoft Q&A](https://learn.microsoft.com/en-us/answers/a/1180572)
- The extension lets apps including Media Player, Movies & TV and browsers decode HEVC (Microsoft Q&A). However, one user found Movies & TV played HEVC `.MOV` while legacy WMP still would not. Another article says WMP's free HEVC option will not play 10-bit HEVC. — [Microsoft Q&A](https://learn.microsoft.com/en-us/answers/a/12798237); [Imagineer Systems note](https://blog.imagineersystems.com/documentation/syntheyes/syntheyesum_files/hevc_h_265_on_windows); [XDA: HEVC in Windows Media Player](https://www.xda-developers.com/play-hevc-encoded-media-windows-media-player/)
- AV1 Video Extension: the Store listing exists (product 9MVZQVXJBQ9V). A store install auto-updates. No official offline-install documentation for AV1 was found. — [Microsoft Store: AV1 Video Extension](https://apps.microsoft.com/detail/9MVZQVXJBQ9V); [Windows Central](https://windowscentral.com/how-add-av1-video-playback-support-windows-10)

**Hardware decode**
- NVDEC supports MPEG-2, VC-1, H.264, HEVC, VP8, VP9 and AV1 on Windows and Linux. Limits vary by generation: H.264 decode is 4:2:0 only, and 4:2:2 H.264/HEVC decode is new in Blackwell. — [NVIDIA Video Codec SDK](https://developer.nvidia.com/video-codec-sdk); [NVIDIA dev forum](https://forums.developer.nvidia.com/t/nvdec-not-supporting-h-264-4-4-4-when-nvenc-does/157198)

**SSD endurance (Samsung 860 EVO, official)**
- Warranty is 5 years or the TBW limit, whichever comes first: 250 GB 150 TBW, 500 GB 300 TBW, 1 TB 600 TBW, 2 TB 1,200 TBW, 4 TB 2,400 TBW. (One retailer, Quiet PC, lists 150 TBW for all sizes. Treat that as an error.) — [Samsung US warranty](https://semiconductor.samsung.com/us/consumer-storage/support/warranty/); [Samsung IE 860 EVO 1TB](https://www.samsung.com/ie/memory-storage/sata-ssd/ssd-860-evo-sata-3-m-2-1tb-mz-n6e1t0bw); [Newegg Business](https://www.neweggbusiness.com/samsung-860-evo-series-250gb/p/9B-20-147-668)

### Inferences
**Write rates (computed)**

| Video bitrate | MB/s | GB/hour | GB/day 24/7 | TB/year 24/7 |
|---|---|---|---|---|
| 25 Mbps | 3.1 | 11.3 | 270 | 99 |
| 50 Mbps | 6.25 | 22.5 | 540 | 197 |
| 100 Mbps | 12.5 | 45 | 1,080 | 394 |

**860 EVO warranty TBW consumed (computed, write amplification about 1 for large sequential writes)**
- 500 GB (300 TBW): about 1.5 years at 50 Mbps 24/7, about 9 months at 100 Mbps 24/7.
- 1 TB (600 TBW): about 3 years at 50 Mbps, about 1.5 years at 100 Mbps.
- More realistic usage (buffer active only while gaming, 6 h/day at 50 Mbps, about 49 TB/year): 500 GB lasts about 6 years and 1 TB about 12 years, so the 5-year warranty is reached first.
- TBW is a warranty figure, not a failure point. Samsung TLC drives typically far exceed it, but SATA SSDs also slow down once the SLC cache fills. At 6-12 MB/s that is not a concern.
- Conclusion: a RAM-first buffer avoids constant writes. If the disk is used, only write when a game is active, or keep a fixed pre-allocated ring file that is overwritten in place so the filesystem doesn't churn metadata.

**RAM needed for N minutes (computed; video only, plus about 2.4 MB/min for two 160 kbps AAC tracks)**

| Bitrate | 1 min | 5 min | 10 min |
|---|---|---|---|
| 15 Mbps | 113 MB | 563 MB | 1.13 GB |
| 25 Mbps | 188 MB | 938 MB | 1.88 GB |
| 50 Mbps | 375 MB | 1.88 GB | 3.75 GB |
| 100 Mbps | 750 MB | 3.75 GB | 7.5 GB |

- Add about one GOP of headroom, as OBS's trimming implies, plus Vec/allocator overhead. A slab or arena of 1-4 MB blocks avoids fragmentation from millions of small `Vec<u8>` packets. 144 fps × 600 s is 86,400 video packets, which is a small index.

**Save latency (computed)**
- A save must write the clip once. At about 450-500 MB/s SATA sequential write, a 10 min clip at 50 Mbps (3.75 GB) takes about 8 s, and at 100 Mbps (7.5 GB) about 15-17 s. A 1 min clip at 50 Mbps (375 MB) takes under 1 s.
- ffmpeg `+faststart` re-reads and rewrites the whole file, roughly doubling this. An in-process writer that emits moov first avoids the second pass.
- Reading back from an SSD disk buffer to write to the same SSD also roughly doubles the I/O.

**Playback recommendations for this PC**
- Movies & TV is not part of LTSC (general knowledge; see Gaps). WMP legacy plus sideloaded extensions, or a third-party player such as mpv or VLC, is the realistic path. For "clips play anywhere without setup", H.264 8-bit 4:2:0 + AAC-LC in MP4 is the only choice with no caveats on this OS and for WhatsApp recipients.
- HEVC is a reasonable second option: Discord desktop and web play it, and the RTX 40 decodes it. Use it only if the user installs the HEVC extension for WMP and accepts WhatsApp transcoding or failures.
- AV1 is the most efficient and the RTX 40 encodes and decodes it, but WMP needs the AV1 extension, Discord upload playback is unconfirmed, and WhatsApp support is undocumented. Treat it as an "archive" option or for re-encoding on export, not as the default share codec.

### Gaps
- No Microsoft source was retrieved confirming that LTSC 2021 lacks Movies & TV and the Store. This is widely known and consistent with the LTSC servicing model, but uncited here.
- The AV1 Video Extension's price (generally reported as free) and its offline appx availability could not be confirmed. The Store page did not render.
- No official answer on whether the HEVC and AV1 extensions plug into WMP legacy's Media Foundation pipeline on 19044. Evidence is mixed: one user report says no for `.MOV`, while XDA says it works.
- No Ada-specific NVDEC matrix entry was retrieved. AV1 and HEVC decode on RTX 40 is per the generic SDK page.

---

## 6. Recommended design (synthesis)

### Takeaway
Replace the "custom TS → long-running ffmpeg → 10 s TS chunks → remux" pipeline with an in-process encoded-packet ring like OBS's, using fixed CFR timestamps. Encode audio in-process. Save to a single-pass MP4 with moov at the front, written by your own writer or a crate, with H.264+AAC as the shareable default and HEVC/AV1 as opt-in. Add an fMP4 disk spill only for buffers that exceed a RAM budget.

### Cited Findings
- The in-RAM deque with GOP-aligned trimming, a dual time and byte limit, deferred save and mux-on-save design is proven in OBS. — [OBS source](https://raw.githubusercontent.com/obsproject/obs-studio/master/plugins/obs-ffmpeg/obs-ffmpeg-mux.c)
- The fMP4 on-disk ring is proven in Steam and is crash-decodable. — [Steam Support](https://help.steampowered.com/faqs/view/23B7-49AD-4A28-9590); [FFmpeg formats doc](https://ffmpeg.org/ffmpeg-formats.html)
- ffmpeg has no AV1-in-TS muxer through 9.1. — [FFmpeg Changelog](https://raw.githubusercontent.com/FFmpeg/FFmpeg/master/Changelog)
- YouTube wants moov at the front and no edit lists. — [YouTube](https://support.google.com/youtube/answer/1722171?hl=en)
- Premiere and Resolve mishandle VFR game captures. — [Adobe FAQ](https://community.adobe.com/t5/premiere-pro/faq-variable-frame-rate-vfr-video-files/td-p/10348229)
- Discord desktop and web play HEVC since July 2024. — [Discord](https://discord.com/blog/discord-patch-notes-july-1-2024)

### Inferences
1. **Capture and encode (game-impact first):** DDA, then NV12 conversion, then NVENC in-process. Run CFR at the user's chosen fps. When no new frame arrives by the next tick, resubmit the previous NV12 surface; NVENC ASIC time only, no extra shader work. Fixed GOP of 1-2 s (`gopLength = fps` or `2*fps`), closed GOP, no B-frames, which keeps edit lists away and latency low. VFR is an advanced toggle labelled "smaller files, may desync in editors".
2. **Timestamps:** keep an internal int64 tick count. Video pts = frame_index in timebase 1/fps (exact for any refresh rate). Audio pts in samples at 48 kHz (1/48000). Anchor both to one QPC start time and rebase to zero at save, as OBS does per track.
3. **Audio:** WASAPI loopback and mic, each encoded continuously in-process to AAC-LC 48 kHz (160-192 kbps). Write two separate tracks, desktop and mic, plus optionally a pre-mixed track 1 for sharing, because Discord, WhatsApp and browsers play only the first audio track.
4. **Buffer:** a ring of encoded packets in large arena blocks, with a side index (pts, dts, size, keyframe, track, offset). Trim from the front one GOP at a time when either `duration > N min + 1 GOP` or `bytes > RAM budget`. Never trim the last two keyframes, matching OBS's `keyframes <= 2` guard.
5. **Optional disk tier:** if `bitrate × duration` exceeds the RAM budget (for example over 2 GB), spill closed GOPs to a pre-allocated fMP4 ring file on the SSD and keep only the index in RAM. Enable it only while a game is running to limit TBW use (see the section 5 maths).
6. **Save:** use OBS's deferred save (wait for the first packet at or after the hotkey time). Snapshot the index, then on a worker thread write `ftyp` + `moov` (with `co64` when over 4 GiB) + `mdat` sequentially to a `.part` file, `fsync`, and rename to `.mp4`. Expected time is under 1 s for a 1 min clip and about 8 s for 10 min at 50 Mbps on SATA. Use a crate such as muxide only after verifying `co64`, `hvcC`/`av1C` and multi-track support. Otherwise write the roughly 15 box types by hand, or fall back to a libavformat binding (`movflags` without faststart, since the moov-first layout is done manually).
7. **Codec presets:** "Share (H.264, 1080p, about 15-25 Mbps CBR/VBR)" as the default, safe for WMP legacy, WhatsApp, Discord and YouTube. "Quality (HEVC)" for Discord desktop and local viewing once the HEVC extension is installed. "Archive (AV1)" for local storage. Drop MPEG-TS entirely. MKV is optional for archival or FLAC use only.
8. **Bitrate guidance:** 15-25 Mbps H.264 at 1080p60-144 is a sensible default. It is 1.1-1.9 GB of RAM for 10 min and above YouTube's 12 Mbps 1080p60 recommendation. 50-100 Mbps is only warranted for an editing master, and then the disk tier is needed for 10 min buffers.

### Gaps
- No direct benchmark compares in-process ring and subprocess pipeline CPU and latency on this hardware. The prototype could measure it.
- WhatsApp's current inline size limit is unknown and will cap shareable clip length at a given bitrate. Discord's current free-tier upload limit was not researched.

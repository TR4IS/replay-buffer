# Audio capture, sync and encoding for a Windows 10 (19044) game replay buffer

Scope note: researched 2026-11-06. Primary sources were Microsoft Learn, the OBS Studio source on GitHub (master branch), the FFmpeg source and docs, and the cpal 0.18.2 source in the local Cargo registry. The VB-Audio forum and the Hydrogenaudio wiki returned HTTP 403, and trac.ffmpeg.org refused the connection, so the Voicemeeter and AAC-quality points below rest on search-result snippets and are marked as weaker evidence.

## 1. System loopback vs process loopback, and how other recorders capture

### Takeaway
Use system (endpoint) loopback as the base layer. It is documented, has been stable since Vista, and has supported event-driven capture since Windows 10 1703. Process loopback (`AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK`) works on 19041+ in practice, and OBS turns it on from 19041. Microsoft only documents it for build 20348+, though, and on Windows 10 it has open crackle and desync reports. Treat it as an opt-in "per-app tracks" feature with a fallback to system loopback. Its big advantage on this PC is that it is not tied to an endpoint, so it captures a game's audio whatever the Voicemeeter routing is.

### Cited Findings
- **System loopback basics.** Loopback captures "the audio stream that is being played by a rendering endpoint device". It is opened by initializing a capture stream on the render endpoint with `AUDCLNT_STREAMFLAGS_LOOPBACK`. It works only in shared mode: "Exclusive-mode streams cannot operate in loopback mode." — [MS Learn: Loopback Recording](https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording)
- **Event-driven loopback.** Before Windows 10 1703, an event-driven loopback client received no events, and the workaround was a separate event-driven render stream. "In Windows 10 versions 1703 and higher, event-driven loopback clients are supported." Build 19044 is past that. — [MS Learn: Loopback Recording](https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording)
- **What loopback copies.** When the hardware has no loopback pin, "WASAPI copies the output stream from the audio engine into the loopback application's capture buffer". Loopback is therefore taken at the audio-engine level, not at the DAC. — [MS Learn: Loopback Recording](https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording)
- **Process loopback build requirement.** `AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS` has the members `TargetProcessId` and `ProcessLoopbackMode` (include or exclude the target process tree). It lists "Minimum supported client: Windows 10 Build 20348". — [MS Learn: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS](https://learn.microsoft.com/en-us/windows/win32/api/audioclientactivationparams/ns-audioclientactivationparams-audioclient_process_loopback_params)
- **Microsoft's ApplicationLoopback sample.** The sample also says it "requires Windows 10 build 20348 or later". It supports `includetree` and `excludetree`. Include mode captures a process and its children, and exclude mode captures everything except that tree. "The capture is not tied to a specific audio endpoint." If the target has no render streams, "the capturing process receives silence". — [microsoft/windows-classic-samples ApplicationLoopback](https://github.com/microsoft/windows-classic-samples/tree/main/Samples/ApplicationLoopback)
- **OBS gating.** OBS registers Application Audio Capture on 10.0.19041 and later, with the comment `/* MS says 20348, but process filtering seems to work earlier */` and `minimum.build = 19041`. — [OBS plugin-main.cpp](https://github.com/obsproject/obs-studio/blob/master/plugins/win-wasapi/plugin-main.cpp)
- **OBS process-loopback setup.** OBS:
  - loads `ActivateAudioInterfaceAsync` from Mmdevapi at runtime, and throws if the function is missing;
  - finds the PID from the target window with `GetWindowThreadProcessId`;
  - uses `AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK` with `PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE` on `VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK`;
  - builds the format by hand (`WAVE_FORMAT_EXTENSIBLE`, 32-bit `KSDATAFORMAT_SUBTYPE_IEEE_FLOAT`, OBS's own sample rate and channel mask) because a mix format is not available;
  - treats the capture as invalid when `IsWindow(hwnd)` goes false, then reconnects every 3 s (`RECONNECT_INTERVAL 3000`).

  — [OBS win-wasapi.cpp](https://github.com/obsproject/obs-studio/blob/master/plugins/win-wasapi/win-wasapi.cpp)
- **win-capture-audio plugin (bozbez).** This was the predecessor third-party OBS plugin on the same API. It needs "an updated version of Windows 10 2004 … or later". It says the API is "only officially available on Windows 11" but "appears to work additionally on relatively recent versions of Windows 10". Its troubleshooting notes that older builds may capture no audio until more Windows updates are installed. — [bozbez/win-capture-audio](https://github.com/bozbez/win-capture-audio)
- **Reliability reports.** OBS issue #8086 reports that Application Audio Capture (beta) crackles and then drifts out of sync 10–30 minutes in, on OBS 29.0.0 under Windows 10, while desktop (endpoint) capture was fine. Maintainers linked it to #8064. The thread notes that the bozbez plugin can hit the same problem because it uses the same Windows API. — [OBS issue #8086](https://github.com/obsproject/obs-studio/issues/8086) (as summarized by search; not opened directly)
- **Forum workarounds for the same problem** (mostly Windows 11, anecdotal): lower the device sample rate or bit depth in mmsys.cpl, or toggle the source off and on, which only fixes it temporarily. — [OBS forum thread 166472](https://obsproject.com/forum/threads/audio-crackling-at-ranom-interval-with-application-audio-capture.166472/)
- **Steam Game Recording.** It offers "Game Audio Only", "All System Audio" and "Game and Selected Programs". The per-program choice implies per-process capture. Community posts report silent recordings in "Game Audio Only" on Windows 10, fixed by switching to "All System Audio" (anecdotal, no Valve documentation). — [Steam Client Beta discussion](https://steamcommunity.com/groups/SteamClientBeta/discussions/5/6655846466491375040), [PC Gamer](https://www.pcgamer.com/games/steams-background-recording-feature-is-the-new-gold-standard-of-game-clipping/)
- **NVIDIA ShadowPlay.** GeForce Experience 3.8 added "Separate Both Tracks", which puts the microphone on its own audio track. No per-app capture is described. — [NVIDIA: GeForce Experience 3.8 multi-track audio](https://www.nvidia.com/en-gb/geforce/news/gfecnt/geforce-experience-3-8-beta-multi-track-audio/)
- **Medal.** It has a "Separate Game and Voice Tracks" toggle (Settings > Audio & Camera). Its sources include "Record all PC audio" or specific sources such as Game Audio and Discord Audio. — [Medal support: Multi-Track Recording](https://support.medal.tv/support/solutions/articles/48001220541-multi-track-recording), [Medal: separate audio tracks](https://medal.tv/features/separate-audio-tracks)

### Inferences
- Process loopback captures by process tree, not by endpoint, so the game's audio can be captured even though it is rendered to "Voicemeeter AUX Input". This audio is pre-Voicemeeter: no Voicemeeter EQ or gain, and none of Voicemeeter's added latency. Not tested on this PC.
- For per-app tracks (game, Discord, music), process loopback is the only API route; endpoint loopback only separates by device. The alternative is user routing in Voicemeeter: game to AUX → B1, Discord to VAIO → B2, and so on.
- The OBS and Win10 crackle/desync reports suggest a design with two layers:
  - Always capture the endpoint loopback, which is the reliable baseline.
  - Add process-loopback tracks as optional extras, and watch their timestamps and discontinuity flags. If a track misbehaves, drop it from the clip rather than ruin the whole clip.
- The capture methods of Steam, Medal and NVIDIA are not documented. Steam's "Game and Selected Programs" and Medal's "Discord Audio" strongly suggest process loopback, but that is speculation.

### Gaps
- No primary source says exactly what Microsoft changed between 19041 and 20348. No Microsoft statement confirms that the API is supported on 19044.
- Not determined whether process loopback has its own clock or timestamps that drift differently from endpoint loopback. The OBS desync reports hint at a problem but give no root cause.
- No official documentation was found on how ShadowPlay, Steam or Medal implement capture.

## 2. Accurate A/V sync: QPC timestamps, flags, silence, drift, latency, and the ~100 ms offset

### Takeaway
Timestamp every packet with `GetBuffer`'s `u64QPCPosition`. It is already in 100 ns units of QPC, which is the same clock as DXGI `LastPresentTime`, but `LastPresentTime` is raw ticks and must be converted with `QueryPerformanceFrequency`. Then build a continuous sample timeline per source the way OBS does: smooth small jitter, treat large jumps as gaps filled with silence, and correct long-term drift.

The "buffer underrun or overrun" errors in the prototype are cpal turning every `AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY` into `Xrun`. On a loopback stream that flag is normal whenever audio resumes after silence.

A constant ~100 ms offset measured with a browser test is confounded by the browser's own latency compensation (66 ms). Measure instead with a test pattern you generate yourself, with known QPC times.

### Cited Findings
- **`u64QPCPosition`.** It is "the value of the performance counter at the time that the audio endpoint device recorded the device position of the first audio frame in the data packet". It is computed as `10,000,000 · t / f` (100 ns units), so it is not the raw counter. — [MS Learn: IAudioCaptureClient::GetBuffer](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudiocaptureclient-getbuffer)
- **`u64DevicePosition`.** It is the stream-relative frame position of the first frame in the packet. "Through the pdwFlags output parameter, the method indicates whether the reported device position is valid" (that is, `AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR`). — [MS Learn: GetBuffer](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudiocaptureclient-getbuffer)
- **`DATA_DISCONTINUITY`.** In Windows 7 and later, GetBuffer "sets the AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY flag to indicate a glitch in the buffer". The client must read a whole packet or none of it. "Clients that delay releasing a packet for more than one period risk losing sample data." — [MS Learn: GetBuffer](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudiocaptureclient-getbuffer)
- **No data during silence.** WASAPI loopback only delivers data while some stream is active on the render endpoint. When playback resumed after silence, the user saw the discontinuity flag. The suggested workaround was to play a silent stream in the background, and an Audacity maintainer said a silent stream is OK but no stream means no recording. — [Audacity forum: "Recording no longer starts until sound starts"](https://forum.audacityteam.org/t/recording-no-longer-starts-until-sound-starts/46934)
- **Microsoft's own sample.** Matthew van Eerde's (Microsoft) loopback-capture sample treats a discontinuity on the first packet as probably spurious, and otherwise logs it without exiting. — [MS blog archive: draining the WASAPI capture buffer fully](https://learn.microsoft.com/en-my/archive/blogs/matthew_van_eerde/draining-the-wasapi-capture-buffer-fully) (via search snippet)
- **Debugger pauses.** A Microsoft Q&A thread is titled "Persistent audio discontinuity in WASAPI loopback capture when debugging", which suggests that reading late produces real overrun discontinuities. — [MS Q&A 1188388](https://learn.microsoft.com/en-us/answers/questions/1188388/persistent-audio-discontinuity-in-wasapi-loopback) (title only)
- **What cpal does with the flag.** cpal 0.18.2 calls `GetBuffer` with device and QPC positions. If `device_position != 0 && flags & AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY` it calls `try_emit_error(error_callback, ErrorKind::Xrun)`. `Xrun` displays as "A buffer underrun or overrun occurred." It does not pass the flags, the device position or the SILENT flag to the data callback. — local source `%USERPROFILE%\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\cpal-0.18.2\src\host\wasapi\stream.rs` lines 802–826 and `src\error.rs` lines 77 and 127; upstream [RustAudio/cpal](https://github.com/RustAudio/cpal)
- **cpal's capture timestamp.** It is `qpc_position * 100` ns (`InputStreamTimestamp.capture`), which is correct. The `callback` timestamp comes from `IAudioClock::GetPosition`. — same local `stream.rs` lines 937–955
- **OBS timestamps.**
  - Process output: `data.timestamp = ts * 100` (GetBuffer QPC converted to ns, no offset).
  - Device sources: `useDeviceTiming ? ts * 100 : os_gettime_ns()`. Without device timing, OBS subtracts the packet duration from `os_gettime_ns()`.
  - `use_device_timing` defaults to true for device output (desktop audio) and false for input (mic).
  - `AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR` is logged.
  - On `AUDCLNT_BUFFERFLAGS_SILENT`, OBS substitutes a zeroed buffer.

  — [OBS win-wasapi.cpp](https://github.com/obsproject/obs-studio/blob/master/plugins/win-wasapi/win-wasapi.cpp)
- **OBS keep-alive for silent loopback.** `ClearBuffer` runs only for desktop (device output) sources. It opens a render client on the same endpoint and zero-fills a buffer, with a comment that this keeps the stream from stopping during silence. Process output does not do this. — [OBS win-wasapi.cpp](https://github.com/obsproject/obs-studio/blob/master/plugins/win-wasapi/win-wasapi.cpp)
- **libobs timing model** (`source_output_audio_data`):
  - The first packet anchors `timing_adjust = os_time - timestamp`. If the incoming timestamp is within `MAX_TS_VAR` of the current system time, it is used directly (`timing_adjust = 0`).
  - Each later packet is compared with `next_audio_ts_min`, which is the previous timestamp plus the frames converted to time.
    - A difference under `TS_SMOOTHING_THRESHOLD` (`70000000ULL`, 70 ms) is snapped to the expected value, which keeps the stream sample-contiguous.
    - A difference over `MAX_TS_VAR` triggers `handle_ts_jump` → `reset_audio_timing` and `reset_audio_data`.
  - Then `in.timestamp += sync_offset` (the user's per-source offset) and `-= resample_offset`.
  - Samples are placed into the source buffer at `get_buf_placement(audio, in->timestamp - source->audio_ts)`.

  — [OBS libobs/obs-source.c](https://github.com/obsproject/obs-studio/blob/master/libobs/obs-source.c)
- **OBS mixing.** Mixing runs per output tick (`audio_callback`). A source whose `audio_ts` is behind the tick start adds global audio buffering, logged as "adding %d milliseconds of audio buffering". Once buffering is maxed out, `ignore_audio()` drops a lagging source's oldest samples. A source starting partway through a tick is mixed from an offset `start_point` frame. — [OBS libobs/obs-audio.c](https://github.com/obsproject/obs-studio/blob/master/libobs/obs-audio.c)

### Inferences
- **The "underrun or overrun" log on this PC.** With loopback of "Voicemeeter AUX Input", any time no app is rendering to that endpoint the loopback stops producing packets. The next packet then carries `DATA_DISCONTINUITY`, which cpal reports as `Xrun`, so most of these logs are probably benign silence-to-sound transitions rather than lost samples. A real overrun is also possible if the cpal callback blocks, for example on a named-pipe write while ffmpeg is slow, because a late `ReleaseBuffer` loses data, per the GetBuffer remarks. cpal cannot tell the two apart because it hides the flags and the device position. With raw WASAPI, a gap can be classified:
  - expected next device position = last position + last frames;
  - when the next packet arrives, compute both the position delta and the QPC delta;
  - if the QPC delta is much larger than the frame count implies, it was a silence gap;
  - if the device position jumped, samples were lost.
- **Ways to deal with silence:**
  - (a) Keep the endpoint running with a silent render stream on the same device, as OBS's `ClearBuffer` and the Audacity advice do. Loopback then delivers continuous packets, many with `AUDCLNT_BUFFERFLAGS_SILENT`.
  - (b) Keep filling gaps by QPC, as the prototype does now.

  Option (a) makes the stream continuous, so drift estimation and gap detection become trivial, and discontinuity flags mean real glitches again. Recommended.
- **Drift.** A sample-count timeline assumes the nominal 48 000 Hz. The device's real clock (here the USB headset's clock, which also drives Voicemeeter's engine through A1) differs from QPC by some ppm, so a timeline anchored once will drift. Fixes:
  - Re-fit a linear map from sample index to QPC over a sliding window, for example the last 30–60 s of packets, and place samples with that map when cutting a clip. This is timestamp correction.
  - Or resample each source to exact QPC-rate 48 kHz at save time with an asynchronous resampler.

  OBS avoids long-term drift by re-anchoring to `os_gettime_ns`, with smoothing and jump resets, and by resampling. Typical USB crystal tolerance is about ±50–100 ppm, which is my assumption, not sourced; at 100 ppm, a 120 s clip drifts 12 ms if anchored only at its start.
- **Latency between a game playing a sound and loopback reporting it.** The chain is: the game writes into its WASAPI render buffer (its own buffering, unknown, often tens of ms) → the engine mixes it on its period (about 10 ms by default) → loopback copies the engine output (MS doc above) → the QPC stamp. The game-side buffering cannot be seen from outside. What the user *hears* comes later than the loopback timestamp, by the endpoint plus Voicemeeter plus USB output latency. Loopback timestamps are therefore at the "submission/mix" stage, the same stage as `LastPresentTime` for video (desktop composition, not scan-out). Matching the two is the right target for a replay clip.
- **Likely contributors to a constant ~100 ms offset.** These are speculative until measured, and the sign of the offset matters:
  1. **Browser A/V sync compensation.** Browsers present video frames timed to the audio clock plus the reported output latency, here 66 ms. Loopback captures before that latency, so in the recording the audio leads the picture by about the browser's output latency. A browser test cannot isolate the recorder.
  2. **Wrong time units.** Mixing raw QPC ticks (`LastPresentTime`) with 100 ns units (GetBuffer). If QueryPerformanceFrequency is not 10 MHz this skews, and an offset or start-time error becomes constant.
  3. **Wrong time origin.** Different zero points for the audio and video pipes into ffmpeg: raw pipe inputs start at PTS 0 when ffmpeg first reads them, unless timestamps are passed explicitly.
  4. **AAC priming.** 1024 samples = 21.33 ms per frame at 48 kHz. If the muxer or player ignores the edit list, the audio is shifted by 1–2 AAC frames.
  5. **Using the callback time instead of the capture time.** The prototype already uses capture time.
  6. **Scan-out versus present.** `LastPresentTime` is not scan-out. Display lag is at least one refresh.
- **Correct measurement.** Write a small test app that, at known QPC times:
  - renders a click into a WASAPI render stream on the default endpoint, so its loopback QPC is known via the same capture path, and
  - flips a full-screen white frame, whose `LastPresentTime` the recorder sees anyway.

  Then measure the PTS difference between the flash and the click in the saved MP4 with ffprobe or an audio-onset detector. A correct pipeline gives about 0 ms. Separately, filming the screen and the earphone with a high-fps phone measures the system's output latency, which a recorder should *not* compensate.

### Gaps
- No primary Microsoft statement says that loopback sets `DATA_DISCONTINUITY` after silence. That evidence is a user report on the Audacity forum and van Eerde's sample behaviour.
- No documentation was found on whether the loopback device position keeps advancing during silence. Test this on this PC.
- `MAX_TS_VAR`'s numeric value is not in `obs-source.c`; it is defined elsewhere and was not retrieved.
- Not verified whether QueryPerformanceFrequency equals 10 MHz on this PC. Check it at runtime.

## 3. Event-driven vs polling, buffer size, thread priority (MMCSS)

### Takeaway
Use event-driven shared-mode capture, as OBS does:
- a 10–20 ms wait timeout as a safety net for loopback;
- a large endpoint buffer (OBS asks for 5 s, and 1–2 s is plenty);
- drain every packet on each wake;
- MMCSS "Audio" on the capture thread;
- the only work on that thread is copying into a lock-free ring buffer.

Real overruns come from a capture thread that blocks (pipe writes, locks, allocation), not from too low a priority. "Pro Audio" (High category, priority 23–26) is unnecessary here.

### Cited Findings
- **OBS event and polling.** OBS uses `AUDCLNT_STREAMFLAGS_EVENTCALLBACK` with `SetEventHandle`. For non-input (loopback) sources its wait has a 10 ms timeout, with the comment "Windows 7 does not seem to wake up for LOOPBACK". — [OBS win-wasapi.cpp](https://github.com/obsproject/obs-studio/blob/master/plugins/win-wasapi/win-wasapi.cpp)
- **OBS buffer and priority.**
  - It passes `#define BUFFER_TIME_100NS (5 * 10000000)` (5 s) as `hnsBufferDuration`, with periodicity 0.
  - The capture thread calls `AvSetMmThreadCharacteristics(L"Audio", ...)`.
  - When RTWQ (the real-time work queue) is available, on Windows 10 1703 or later, it uses `rtwq_lock_shared_work_queue(L"Capture", ...)` instead of a dedicated thread.

  — [OBS win-wasapi.cpp](https://github.com/obsproject/obs-studio/blob/master/plugins/win-wasapi/win-wasapi.cpp)
- **Packet handling rules.** Read all available packets each time the thread runs. A GetBuffer and its ReleaseBuffer must be on the same thread and within one period, or data can be lost. — [MS Learn: GetBuffer](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudiocaptureclient-getbuffer)
- **MMCSS.**
  - The default tasks are Audio, Capture, Distribution, Games, Playback, Pro Audio and Window Manager, configured under `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile\Tasks`.
  - Scheduling categories: High = thread priority 23–26 ("designed for Pro Audio tasks"), Medium = 16–22 ("part of the application that is in the foreground"), Low = 8–15.
  - `SystemResponsiveness` reserves a percentage of CPU for low-priority tasks; the default example is 20%, and 100 disables MMCSS.
  - Threads join with `AvSetMmThreadCharacteristics`.

  — [MS Learn: Multimedia Class Scheduler Service](https://learn.microsoft.com/en-us/windows/win32/procthread/multimedia-class-scheduler-service)

### Inferences
- **Buffer size.** A 1–2 s endpoint buffer costs about 384–768 KB per stereo float stream at 48 kHz. It makes a hiccup in the capture thread (a game spiking the CPU, a page fault) recoverable instead of an overrun. Latency does not matter for a replay buffer, so a big buffer has no downside.
- **CPU cost.** Event-driven capture wakes about 100 times a second per stream (10 ms engine period) and does only memcpy. Polling at 10 ms costs the same but can miss events, so there is no reason to poll except as the timeout fallback.
- **The likely real problem in the prototype.** It does more than memcpy in the callback: f32 conversion, placement and named-pipe writes to ffmpeg. A pipe write blocks whenever ffmpeg falls behind, which delays `ReleaseBuffer` and loses data. Fix: callback → SPSC ring → separate writer or encoder thread.
- **Which MMCSS task.** "Audio", as OBS uses, or "Capture" is enough. "Pro Audio" puts the thread in the High category, above the game's threads, which is unnecessary and could take time from the game.

### Gaps
- The registry default values per task (for example "Audio": Priority 6, Scheduling Category Medium) were not retrieved from a primary source. Read them from this PC's registry.
- cpal 0.18.2's WASAPI buffer duration and whether it sets MMCSS were not checked. If cpal is kept, read `src\host\wasapi\device.rs` and `stream.rs` locally.

## 4. Voicemeeter: what to capture, extra latency, getting the mic separately

### Takeaway
On this PC there are three candidates for "system audio":

| Option | What it contains | Drawbacks | When to use |
|---|---|---|---|
| (a) Loopback of "Voicemeeter AUX Input" (the default device, what the prototype uses now) | Pre-Voicemeeter audio from apps that play to the default device | Stops during silence; misses apps routed to other Voicemeeter inputs | Default today |
| (b) Capture of a Voicemeeter virtual output (B1/B2) | Post-mix audio of exactly what the user routes to that bus | Adds Voicemeeter's internal buffering; needs user routing | Best if the user routes a bus |
| (c) Loopback of the hardware "Headset (EarPods)" | Exactly what the user hears, including any mic monitoring | Likely fails if Voicemeeter opens A1 exclusively (WDM/KS), since loopback needs a shared stream | Possible, but risky |

Option (b) is a capture endpoint, so it delivers continuous data and has no silence gaps.

For the mic, open the hardware headset mic in shared mode. Voicemeeter opens inputs shared by default, and "WDM Input Exclusive" is off by default. If that fails with "device in use", fall back to a Voicemeeter B bus that carries only the mic strip.

### Cited Findings
- **Exclusive streams and loopback.** Loopback works only for shared-mode streams. The audio engine copies its mixed output to the loopback client, so a device driven exclusively bypasses the engine. — [MS Learn: Loopback Recording](https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording)
- **Voicemeeter's internal buffer** (forum user estimate, weak evidence). Latency added by Voicemeeter virtual I/O depends on an internal 2048-sample buffer, about 21.3 ms at 48 kHz. The same user estimated about 14.2 ms for VB-CABLE. These are one user's calculations, not official figures. — [VB-Audio forum p=1612](https://forum.vb-audio.com/viewtopic.php?p=1612) (search snippet; page returned 403)
- **A1 sets the latency** (forum reply). Voicemeeter's overall latency is set first by the main output device on A1, and choosing the best device there gives the most stable, lowest-latency stream. — [VB-Audio forum p=1612](https://forum.vb-audio.com/viewtopic.php?p=1612), [p=1594](https://forum.vb-audio.com/viewtopic.php?p=1594) (search snippets)
- **MME vs WDM** (VB-Audio staff reply, via snippet). MME is not designed for very small buffers, so use WDM or ASIO on A1 for buffers below 512 samples. WDM's default buffer is smaller than MME's. — [VB-Audio forum p=1970](https://forum.vb-audio.com/viewtopic.php?p=1970), [p=4764](https://forum.vb-audio.com/viewtopic.php?p=4764) (search snippets)
- **The WDM Input Exclusive option** (forum, via snippet). It applies only to the selected input device. "Voicemeeter opens input devices always in share mode per default". Setting it to YES shows a warning, and it "is not stable in many configurations". — [VB-Audio forum p=474](https://forum.vb-audio.com/viewtopic.php?p=474), [p=502](https://forum.vb-audio.com/viewtopic.php?p=502) (search snippets)
- **A third-party OBS forum post** says that with Voicemeeter's MME mode, monitoring works "at the cost of higher latency than WDM mode". — [OBS forum: audio monitoring with Voicemeeter](https://obsproject.com/forum/threads/audio-monitoring-in-obs-27-0-1-with-voicemeeter.147500/latest) (search snippet)
- **NVIDIA ShadowPlay users** commonly use Voicemeeter to split tracks (community videos), showing that bus routing is a known way to get separate tracks. — [letsplayindex: ShadowPlay & Voicemeeter separate tracks](https://www.letsplayindex.com/video/how-to-record-audio-and-separate-both-tracks-with-nvidia-6gV81HYy14g)

### Inferences
- **Loopback of AUX Input (current).** It is pre-Voicemeeter: no Voicemeeter latency and no EQ or gain, and the timestamps line up with the game's submission time. Its weak points:
  - starvation during silence (fix with a silent keep-alive render stream on AUX Input);
  - it only contains apps that render to the default device. Apps the user sends to "Voicemeeter Input" (VAIO) or "AUX" explicitly differ, and Discord is often routed elsewhere.
- **Voicemeeter Out B1/B2.** This is a capture endpoint driven by Voicemeeter's engine, which is clocked by A1 (the USB headset). It delivers continuous packets (zeros during silence), so no keep-alive is needed and drift estimation is clean. It contains Voicemeeter's internal buffering, maybe about 20–45 ms by forum estimates. That latency is roughly the same delay the user hears, but it is *later* than the video's present timestamp. Apply a fixed negative offset, calibrated with the test in section 2, if this route is used.
- **Hardware headset loopback (A1).** Whether it works depends on whether Voicemeeter opens A1 shared or exclusive. If A1 uses WDM, Voicemeeter very likely opens it exclusively. That is speculation: the forum says *inputs* default to shared, which implies outputs differ. Exclusive access would make Windows loopback of the headset silent. Do not rely on this route; test it.
- **Mic: hardware headset mic in shared mode.**
  - Pros: a continuous capture stream, the lowest latency, and independent of Voicemeeter's state.
  - Risks: `AUDCLNT_E_DEVICE_IN_USE` if Voicemeeter (or anything else) has it exclusively (WDM Input Exclusive = YES, or KS driver type). The shared-mode engine resamples if formats differ, which is harmless.
  - It does not include Voicemeeter's mic processing (gate, compressor) or the strip mute. If the user mutes the mic in Voicemeeter, the replay still records the raw mic.
- **Mic: a B-bus fallback.** Route only the mic strip to B2 and capture "Voicemeeter Out B2". This respects Voicemeeter mute and processing, adds Voicemeeter's latency, and needs user setup.
- **Shared clock.** The mic and output are on the same USB device, so they probably share one clock domain (speculative). Still, estimate drift per source independently.

### Gaps
- No official VB-Audio latency figures were retrieved for Potato virtual I/O; the VB-Audio forum pages returned 403. Measure on this PC with the click and flash test.
- It was not confirmed whether Voicemeeter's WDM output for A1 is exclusive, or whether loopback of the headset works while Voicemeeter drives it.
- The Voicemeeter Potato user manual was not retrieved.

## 5. Audio codec, encoder, bitrate, compatibility and multi-track MP4 layout

### Takeaway
Ship **AAC-LC 48 kHz in MP4**. It is the only audio codec that every target (Windows players, WhatsApp, Discord, editors) handles reliably. Encode with FFmpeg's native `aac` encoder: default `twoloop` coder, LGPL-safe, no external library. Use about 192–256 kbps for the stereo mix and game/desktop tracks, and about 96–128 kbps for a mono mic. Avoid the Media Foundation AAC encoder (capped at 192 kbps CBR, 16-bit input only) and libfdk_aac (non-free and GPL-incompatible, so it cannot be redistributed). Avoid Opus or FLAC in MP4 for shareable clips.

Put the **mixed track first, with the "default" disposition**. FFmpeg's MP4 muxer puts all audio tracks into one alternate group and enables only the default (or first) one, so most players play only that track. Offer a single-track export for WhatsApp, which documents "single audio stream or no audio stream only".

### Cited Findings
- **FFmpeg native `aac`.** It is "the default AAC encoder, natively implemented into FFmpeg". If `b` is unset, it defaults to 128 kbps.
  - `-q:a`: `1` lands near 128 kbps for typical stereo music, and values above about 6 make no further difference.
  - `aac_coder`: `twoloop` is the default. `fast` is "Worse with low bitrates (less than 64kbps), but is better and much faster at higher bitrates". `anmr` is "Not currently recommended".

  — [FFmpeg Codecs documentation](https://ffmpeg.org/ffmpeg-codecs.html)
- **libfdk_aac.** The library is "incompatible with GPL" and needs `--enable-nonfree`. Its VBR "is experimental"; VBR mode 5 is about 80–96 kbps per channel. Afterburner (on by default) "improves the quality but also the required processing power". — [FFmpeg Codecs documentation](https://ffmpeg.org/ffmpeg-codecs.html)
- **libopus.** Defaults are `application=audio`, 20 ms frames, VBR on, `compression_level` 10. FFmpeg's native `opus` encoder "only implements the CELT part" and is "usually worse and at best is equal to the libopus encoder". FLAC `compression_level` is 0–12, default 5. — [FFmpeg Codecs documentation](https://ffmpeg.org/ffmpeg-codecs.html)
- **Media Foundation AAC encoder.**
  - AAC-LC only.
  - Input must be 16-bit PCM at 44.1 or 48 kHz, in 1, 2 or 6 channels.
  - `MF_MT_AUDIO_AVG_BYTES_PER_SECOND` is limited to 12000, 16000, 20000 or 24000, which is 96, 128, 160 or 192 kbps (default 96 kbps).
  - Each output sample is one AAC frame of 1024 PCM samples, which is 21.33 ms at 48 kHz.

  — [MS Learn: AAC Encoder](https://learn.microsoft.com/en-us/windows/win32/medfound/aac-encoder)
- **Media Foundation's native format table** lists MPEG-4 (.mp4/.m4a/.mov) containers and AAC, MP3 and WMA audio decoders. Opus and FLAC are not listed. The page is dated 2018 and predates later Windows 10 additions such as Store codec extensions. — [MS Learn: Supported Media Formats in Media Foundation](https://learn.microsoft.com/en-us/windows/win32/medfound/supported-media-formats-in-media-foundation)
- **WhatsApp (Business Platform docs).** Video must be H.264 with AAC audio, "Single audio stream or no audio stream only". Containers are video/mp4 and video/3gpp, with a 16 MB cap. One developer guide says Android clients reject H.264 High profile with B-frames. Consumer-app limits are not officially documented. — [WhatsApp Business Platform: video messages](https://developers.secure.facebook.com/documentation/business-messaging/whatsapp/messages/video-messages/), [ChatArchitect mirror](https://support.chatarchitect.com/books/meta-whatsapp/page/video-messages-developer-documentation/export/html)
- **FFmpeg's MP4 muxer, track groups and the enabled flag.**
  - In `mov_write_tkhd_tag`, `group = st->index` when `mov->per_stream_grouping` is set, otherwise `group = st->codecpar->codec_type`. So by default all audio tracks share one alternate group, meaning they are alternatives of each other.
  - The tkhd flags start as `MOV_TKHD_FLAG_IN_MOVIE`, with `MOV_TKHD_FLAG_ENABLED` added only if `track->flags & MOV_TRACK_ENABLED`.
  - `enable_tracks` records the first stream of each media type and sets `MOV_TRACK_ENABLED` on streams with `AV_DISPOSITION_DEFAULT`. A comment says QuickTime needs at least one enabled track per type.

  — [FFmpeg libavformat/movenc.c](https://github.com/FFmpeg/FFmpeg/blob/master/libavformat/movenc.c) (the end of `enable_tracks` was cut off in retrieval)
- **Extra tracks look silent in some players.** Users report that with separate tracks some players and editors play only the first track by default, so clips look as if they have no mic or game audio. — [search summary of GeForce Experience multi-track guides](https://www.nvidia.com/en-gb/geforce/news/gfecnt/geforce-experience-3-8-beta-multi-track-audio/)
- **Other recorders put voice on a separate track:** NVIDIA "Separate Both Tracks" and Medal "Separate Game and Voice Tracks". — [NVIDIA](https://www.nvidia.com/en-gb/geforce/news/gfecnt/geforce-experience-3-8-beta-multi-track-audio/), [Medal support](https://support.medal.tv/support/solutions/articles/48001220541-multi-track-recording)

### Inferences
- **The rest of `enable_tracks`**, from memory of the FFmpeg source and only partly verified: if no stream of a type has the default disposition, the first stream of that type is enabled. If more than one audio stream is marked default, `per_stream_grouping` is switched on, which puts each track in its own alternate group so players may play all of them. Leave only track 1 (the mix) as default. That gives one alternate group and one enabled track, the standard MP4 "pick one" behaviour, so Windows Media Player, Films & TV, browsers and Discord all play the mix.
- **Editors** (Premiere, Resolve, Vegas, Shotcut) generally import every audio track of an MP4 as separate clips (unsourced; based on common knowledge). Separate tracks are for editing, and the mixed track is for viewing.
- **Bitrate.** No listening-test data could be retrieved (Hydrogenaudio 403, FFmpeg trac unreachable). The commonly cited guidance is that FFmpeg's native AAC is near-transparent for most material at about 192 kbps stereo and above, and that 256 kbps leaves headroom for game audio with lots of transients. Treat that as unsourced. Cost: 256 kbps is 1.9 MB/min, which is small next to video bitrates, so choosing 256 over 192 costs little.
- **Opus in MP4.** FFmpeg can mux it, but Windows' built-in Media Foundation table lists no Opus decoder, and Firefox and Opera users report MP4+Opus playback problems. That rules it out for "send to a friend" clips. FLAC or PCM in MKV is fine as an internal or editing intermediate, but not for sharing.
- **AAC priming.** AAC has 1024 samples (21.33 ms) of encoder delay per frame. The MP4 edit list (`mov_write_edts_tag`) compensates, but players that ignore edit lists shift the audio by about 21–43 ms. Measure the final file, not just the pipeline.

### Gaps
- No primary source was found for Discord's in-client playback codec support. Standard H.264 + AAC MP4 is assumed safe; check by uploading a test clip.
- No Hydrogenaudio or listening-test numbers were retrieved for FFmpeg `aac` vs fdk vs Apple at 192 vs 256 kbps.
- Not verified which Windows 10 19044 apps (Films & TV, legacy WMP 12) play Opus-in-MP4 or FLAC-in-MP4 with the Store extensions installed.

## 6. Raw PCM ring buffer + encode on save vs continuous encoding

### Takeaway
Keep raw PCM per source in RAM and encode only on save. RAM is modest: 23 MB per minute per stereo f32 source, or half that as i16. AAC encoding of a 1–2 minute clip takes about a second on save. This gives sample-accurate cuts aligned to the video keyframe, a mixed track built from the separate sources at save time, drift correction, and no AAC frame-boundary or priming problems at the cut. It also removes the need to run ffmpeg continuously through named pipes.

### Cited Findings
- **Bitrate arithmetic.** 1024 PCM samples per AAC frame is 21.33 ms at 48 kHz, so a continuously encoded stream can only be cut on 21.33 ms frame boundaries. — [MS Learn: AAC Encoder](https://learn.microsoft.com/en-us/windows/win32/medfound/aac-encoder)
- **OBS keeps decoded float PCM per source** in deques (`audio_input_buf`), places it by timestamp and mixes per tick, then encodes the mixed output. Mixing happens on PCM, before encoding. — [OBS libobs/obs-audio.c](https://github.com/obsproject/obs-studio/blob/master/libobs/obs-audio.c), [obs-source.c](https://github.com/obsproject/obs-studio/blob/master/libobs/obs-source.c)

### Inferences
- **RAM.** 48 000 Hz × 2 ch × 4 B = 384 KB/s, which is 23 MB/min per stereo f32 source. Example, for a 2-minute buffer:
  - system (stereo) + mic (mono): about 46 + 11.5 = 58 MB;
  - plus two per-app stereo tracks: about 150 MB total;
  - the same as i16: about 75 MB.

  AAC at 256 kbps is 1.9 MB/min, about 12× smaller, but the savings (tens of MB) do not matter on a gaming PC whose video ring is likely hundreds of MB.
- **CPU.**
  - Continuous encoding: a constant small cost, mostly wasted on audio that is never saved. With the current design it also needs a persistent ffmpeg process and pipe plumbing, which caused real blocking risks (section 3).
  - Encode on save: a burst of CPU. FFmpeg native AAC typically runs at dozens of times real time per core (unsourced estimate), so 2 minutes × 2–4 tracks is roughly 1–3 s on one core, done after the user presses save. It can run at below-normal priority so it does not disturb the game.
- **Correctness benefits of PCM:**
  - the clip start is cut exactly at the video keyframe's QPC;
  - the mixed track is computed from the per-source PCM, with gains;
  - drift correction (resampling or re-timing) is applied at save time using the fitted QPC map;
  - silence gaps are filled exactly;
  - no AAC priming or decoder warm-up artefacts at an arbitrary cut point.
- **Hybrid option:** a continuous AAC encode of the mix only (cheap, instant save), plus PCM for the separate tracks. This keeps most of the complexity, so it is not recommended unless save latency must be under about 1 s.

### Gaps
- No benchmark was found for FFmpeg native AAC encode speed. Measure on this PC with `ffmpeg -f f32le -ar 48000 -ac 2 -i x.raw -c:a aac -b:a 256k -f null -`, using `-benchmark`.

## 7. Recommended design for this PC (Win10 LTSC 19044, Voicemeeter Potato, USB EarPods)

### Takeaway
Use raw WASAPI through the `windows` crate instead of cpal, because cpal hides the flags and the device position and cannot do process loopback. Capture event-driven into PCM rings timestamped with QPC, keep loopback alive with a silent render stream, and correct drift and gaps on save. Encode AAC-LC with FFmpeg native `aac` only when saving: a default mixed track first, then separate desktop and mic tracks (and optional per-app tracks). Calibrate sync with a self-generated click and flash test, not a browser.

### Cited Findings
The supporting evidence is in sections 1–6. The key primary sources are:
- [MS Learn: Loopback Recording](https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording)
- [MS Learn: GetBuffer](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudiocaptureclient-getbuffer)
- [MS Learn: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS](https://learn.microsoft.com/en-us/windows/win32/api/audioclientactivationparams/ns-audioclientactivationparams-audioclient_process_loopback_params)
- [OBS win-wasapi.cpp](https://github.com/obsproject/obs-studio/blob/master/plugins/win-wasapi/win-wasapi.cpp) and [plugin-main.cpp](https://github.com/obsproject/obs-studio/blob/master/plugins/win-wasapi/plugin-main.cpp)
- [OBS obs-source.c](https://github.com/obsproject/obs-studio/blob/master/libobs/obs-source.c)
- [MS Learn: MMCSS](https://learn.microsoft.com/en-us/windows/win32/procthread/multimedia-class-scheduler-service)
- [MS Learn: AAC Encoder](https://learn.microsoft.com/en-us/windows/win32/medfound/aac-encoder)
- [FFmpeg codecs doc](https://ffmpeg.org/ffmpeg-codecs.html)
- [FFmpeg movenc.c](https://github.com/FFmpeg/FFmpeg/blob/master/libavformat/movenc.c)
- [WhatsApp video spec](https://developers.secure.facebook.com/documentation/business-messaging/whatsapp/messages/video-messages/)
- local cpal 0.18.2 `src\host\wasapi\stream.rs` (Xrun on `DATA_DISCONTINUITY`)

### Inferences (the recommended design)

**1. Sources (one `IAudioClient` each, shared mode, 48 kHz float, `AUTOCONVERTPCM` if the mix format differs):**
- **S-desktop.** Loopback of the default render endpoint (currently "Voicemeeter AUX Input").
  - Also open a **silent render stream on the same endpoint** (OBS `ClearBuffer` idea, kept running) so the loopback never starves. Discontinuity flags then mean real glitches again, and the stream has a continuous clock.
  - Follow default-device changes (`IMMNotificationClient`) and reconnect on `AUDCLNT_E_DEVICE_INVALIDATED`, as OBS does every 3 s.
- **S-mic.** The hardware "Headset (EarPods)" capture endpoint in shared mode (Voicemeeter opens inputs shared by default).
  - On `AUDCLNT_E_DEVICE_IN_USE`, offer "Voicemeeter Out B2" (mic strip routed only to B2) as a setting.
  - Mention in the UI that Voicemeeter's mute does not apply to the hardware-mic route.
- **Optional S-app[n]** (setting: "per-app tracks (beta)"): process loopback, `INCLUDE_TARGET_PROCESS_TREE`, for the game's PID (from the captured window, as OBS does) and optionally Discord.
  - Gate on build ≥ 19041, as OBS does. Warn that Microsoft officially supports it only on 20348+ (Windows 11).
  - Monitor for timestamp jumps or crackle (OBS #8086), and silently drop a misbehaving app track from the saved clip rather than failing.
  - Optionally use EXCLUDE mode on the recorder's own PID to make sure the keep-alive stream and any notification sounds from the recorder are not recorded. That is another reason the keep-alive is harmless: it is silent anyway.

**2. Capture threads:**
- One thread per source, or one thread with `WaitForMultipleObjects`.
- Event-driven with a 10–20 ms timeout.
- Endpoint buffer about 1–2 s.
- `AvSetMmThreadCharacteristics("Audio")`.
- Loop `GetNextPacketSize` → `GetBuffer` → copy into an SPSC ring of `(qpc_100ns, device_pos, flags, frames, samples)` chunks → `ReleaseBuffer` immediately. No allocation, locks or I/O on this thread.

**3. Timeline per source** (on a non-real-time thread):
- Convert the qpc to ns (×100). Convert DXGI `LastPresentTime` with `ticks * 1e9 / QueryPerformanceFrequency` so both share one time base.
- Keep `expected_next = last_ts + frames/48 kHz`:
  - |diff| < a few ms: treat as contiguous (OBS smooths up to 70 ms; a tighter 5–10 ms is reasonable for an offline cut);
  - a larger forward jump: insert silence;
  - a backward jump or device-position jump: log a real glitch.
- Every few seconds, fit sample index → QPC by least squares over the last about 60 s to estimate ppm drift.

**4. RAM ring:** PCM (f32, or i16 to halve RAM) per source, covering the video ring length + 2 s margin, indexed by fitted QPC.

**5. Save:**
- Take the video clip [t0, t1] from the first keyframe's QPC.
- Cut each PCM source by the fitted map, resampling by the drift ratio if more than about 20 ppm.
- Mix desktop + mic (+ app tracks only if they are not already contained in desktop). With process loopback, app tracks are *subsets* of the desktop loopback, so the mix should be desktop + mic, not a sum of everything.
- Encode with FFmpeg native `aac` (spawned at save, or in-process via libavcodec), 48 kHz:
  - Track 1 "Mix" (default disposition): stereo 256 kbps (192 is fine).
  - Track 2 "Desktop/Game": stereo 192–256 kbps.
  - Track 3 "Mic": mono 96–128 kbps.
  - Tracks 4+ "App: …": stereo 160–192 kbps.
- MP4 with `-movflags +faststart`, `-disposition:a:0 default` and the others `0`, plus track titles (`-metadata:s:a:N title=...`).
- A "Share" export: remux `-map 0:v -map 0:a:0`, one audio track for WhatsApp/Discord, plus an H.264 profile that suits WhatsApp Android if video settings allow.

**6. Sync calibration:**
- Keep a user-visible "audio offset (ms)" setting, like OBS `sync_offset`, default 0.
- Ship a built-in test mode: flash a full-screen white frame and render a click at the same QPC; save; detect the onsets; report the offset.
- Expect about 0 ms with S-desktop/S-mic, and about +20–45 ms (audio late) if the Voicemeeter B-bus routes are used. The latter is a forum-estimate range; measure it.
- Do not use the browser test as ground truth: Chrome's reported 66 ms output-latency compensation shifts its picture against its own audio.

**7. Windows 11 differences:** on build 20348+/Windows 11 the process loopback API is officially supported, so S-app could be on by default there. Event-driven loopback and everything else is the same as on 19044.

### Gaps
- Not tested on this PC:
  - whether process loopback captures a process rendering to the Voicemeeter AUX virtual endpoint;
  - whether the hardware mic opens in shared mode while Voicemeeter runs;
  - whether loopback of "Headset (EarPods)" works while Voicemeeter drives A1;
  - the real ppm drift of the EarPods USB clock against QPC.
- The sign (audio early or late) of the observed ~100 ms offset was not given, so the cause cannot be pinned down without the self-generated test.

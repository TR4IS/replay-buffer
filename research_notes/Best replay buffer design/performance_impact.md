# Game-performance impact of always-on recording / replay buffers on Windows (and how to minimize it)

Scope note: Researched 2026-11-06. Target is Win10 LTSC 2021 (19044), RTX 4060 Ti (Ada, one NVENC), 3x1080p, up to 144 fps. The prototype uses DXGI Desktop Duplication (DDA), then a GPU texture copy, then NVENC. It measured about 4% average fps loss in a GPU-bound browser scene. It already sets BELOW_NORMAL process priority, a THREAD_PRIORITY_HIGHEST capture thread and SetGPUThreadPriority(7).

Evidence labels used below:
- **[MEASURED]**: a published test with numbers.
- **[VENDOR]**: a vendor or project document or source code.
- **[ANECDOTE]**: a forum or user report.

Overall finding: I found almost no rigorous RTX 30/40-era reviewer data with 1%/0.1% lows or latency for recorders. Most published numbers are average fps only, from low-rigor sites or forum posts. The strongest material is NVIDIA's own documentation (what runs on NVENC and what runs on CUDA) and OBS source code and history (how OBS handles GPU priority).

---

## Q1. What do benchmarks show for ShadowPlay / Instant Replay vs OBS vs AMD ReLive vs Xbox Game Bar vs Steam Game Recording (avg fps, 1%/0.1% lows, latency)?

### Takeaway
Hardware-encoder recorders (ShadowPlay/NVIDIA App, OBS with NVENC, Steam Game Recording) usually cost about 1–6% average fps in the few published tests. Software x264 in OBS costs much more (about 8% at 1080p, more at 4K). I found no credible 2022–2026 test that reports 1%/0.1% lows, frame-time variance or input latency for any of these recorders. The largest documented "recorder" regression (up to 15%) came from the NVIDIA App's overlay Game Filters, not from recording itself.

### Cited Findings
- **[MEASURED, low-rigor site, 2026, GPU not shown in summary]** Cyberpunk 2077 at Ultra 1080p from a 142 fps baseline: ShadowPlay gave 139 fps (−3 fps, about −2%) and OBS with x264 gave 131 fps (−11 fps, about −8%). Black Myth: Wukong at 4K60: ShadowPlay cost about 7 fps and OBS x264 cost more than 21 fps. No 1% lows reported. The article title claims "2–15% CPU" across OBS, ShadowPlay and Xbox Game Bar. — [tech-insider.org, "OBS vs ShadowPlay vs Xbox Game Bar 2026"](https://tech-insider.org/obs-vs-shadowplay-vs-xbox-game-bar-2026/)
- **[MEASURED, informal, 2023]** ShadowPlay's impact was under 1% and OBS CPU usage was under 2% in Cyberpunk 2077. No frame-time or 1%-low data. — [MKs Guide, "OBS vs. ShadowPlay 2023"](https://www.mksguide.com/obs-vs-shadowplay/)
- **[VENDOR-adjacent claim]** Clip-tool vendors say OBS matches ShadowPlay's low impact when set to NVENC, and that NVENC is "physically separate from the CUDA cores that draw the game". This is a claim, not measured data. — [DOR blog, OBS vs ShadowPlay](https://clip.dor.gg/en/blog/obs-vs-nvidia-shadowplay); [DOR blog, game recording fps drop fix](https://clip.dor.gg/en/blog/game-recording-fps-drop-fix)
- **[MEASURED, community, 2024]** Steam Game Recording (background recording) in CS2: 372 fps fell to 350 fps (about −6%), tested by CS2 content creator ThourCS2. — [bo3.gg](https://bo3.gg/news/thourcs2-conducted-tests-on-how-fps-drops-with-a-new-steam-feature)
- **[ANECDOTE]** One user said Steam background recording took The Witcher 3 from about 101–120 fps down to 60–65 fps. A Deadlock forum thread is titled "Steam Game Recording causes massive FPS drop". The Witcher 3 report surfaced in search results for Steam Game Recording. I did not open the exact page it came from, so treat it as an unverified anecdote. — [Deadlock forum thread](https://forums.playdeadlock.com/threads/steam-game-recording-causes-massive-fps-drop.65228/latest)
- **[ANECDOTE]** On Linux, one user saw no visible impact from Steam recording in Satisfactory at 3840x1600. — [Level1Techs forum](https://forum.level1techs.com/t/how-is-steam-record-on-linux/233125)
- **[MEASURED, Tom's Hardware, Dec 2024]** With the NVIDIA App installed, games could run "up to 15 percent slower". The root cause was traced to the overlay's "Game Filters and Photo Mode" setting, whether or not any filter was active. NVIDIA's workaround was to disable that setting. — [Tom's Hardware](https://tomshardware.com/pc-components/gpus/we-tested-nvidia-app-performance-problems-games-run-up-to-15-percent-slower-with-the-app); [PC Gamer](https://www.pcgamer.com/hardware/the-nvidia-app-is-damaging-frame-rates-in-some-games-on-some-pcs-with-nvidia-staffers-recommending-you-nix-the-game-filters-overlay-as-a-potential-solution-to-the-problem)
  - Coverage of this issue quoted one Cyberpunk 2077 run: 72.43 fps with overlay and filters on, 78.66 fps with them disabled, and 82.98 fps with the app removed. I did not verify which tester produced these numbers, because the Tom's article text was truncated when fetched.
  - App release 11.0.1 "fixed" the issue only by turning Game Filters off by default. — [Tom's Hardware, 11.0.1](https://www.tomshardware.com/pc-components/gpu-drivers/nvidia-app-performance-issue-gets-fixed-with-release-11-0-1-but-all-it-has-done-is-turn-off-game-filters-by-default)
- **[MEASURED/claim, TechSpot, Dec 2024]** TechSpot reported that other overlay features, such as the performance metrics overlay and video recording, have minimal to no fps impact. I could not open the article (HTTP 403), so this comes from the search summary and I have no numbers. — [TechSpot](https://www.techspot.com/news/105999-nvidia-app-hurting-performance-here-what-you-need.html)
- **[ANECDOTE]** OBS users report measurable losses with NVENC. One thread is titled "opening OBS causes 10 fps loss in game, recording with NVENC causes 20 fps loss". Another snippet said a game went from 70 fps uncapped, to 62 fps with OBS open, to 55 fps while recording with NVENC. — [OBS forum](https://obsproject.com/forum/threads/opening-obs-causes-10-fps-loss-in-game-recording-with-nvenc-causes-20-fps-loss.111903/)
- **[ANECDOTE]** A 2022 bug report on the NVIDIA developer forum is titled "Using NVENC in OBS decimates the performance of the game it's recording". — [NVIDIA dev forum](https://forums.developer.nvidia.com/t/bug-report-using-nvenc-in-obs-decimates-the-performance-of-the-game-its-recording/227276)
- **[MEASURED, older data, Reflex context only]** TechPowerUp measured Reflex with LDAT and a 1200 fps camera (Overwatch). End-to-end latency fell from 25.7 ms to 16.8 ms at High settings and from 82.3 ms to 35.0 ms at Epic settings. No recorder was running. This shows the scale of latency that GPU saturation adds, which is the regime where recorder overhead matters. — [TechPowerUp Reflex LDAT review](https://techpowerup.com/review/nvidia-reflex-review-test-ldat-v2/4.html)

### Inferences
- A loss of about 2–6% average fps is the realistic published range for always-on hardware-encoded recorders in GPU-bound scenes. The prototype's 4% is in the same band as ShadowPlay and Steam, so it is not yet "unnoticeable". The goal should be below about 1% average and no measurable change in 1% lows.
- Recorder overhead is not one number. Average fps hides stutter, and the anecdotes of large drops (for example 120 to 60 fps) are more likely contention or pacing pathologies (GPU scheduling, a misconfigured encoder, overlays) than steady-state encode cost.
- "Recording" products bundle other costs, such as overlays and filters (the NVIDIA App case). A dedicated recorder with no overlay hook avoids that whole class of regression.

### Gaps
- I found no RTX 30/40-series data from Gamers Nexus, Hardware Unboxed, Digital Foundry, Puget or TechPowerUp that measures ShadowPlay vs OBS with 1%/0.1% lows or latency. My searches surfaced none, so this is unverified rather than proven absent.
- I found no data on AMD ReLive in the RDNA2/3 era, and no measured Xbox Game Bar background-recording numbers beyond one low-rigor article title.
- I found no measured input-latency delta (LDAT, high-speed camera or PCL) for any recorder. An OBS forum user reported a "slight but definite delay" while streaming but could not measure it. — [OBS forum, Input Lag](https://obsproject.com/forum/threads/input-lag.64/)

---

## Q2. Where does the cost come from (capture copy, RGB->NV12, DWM composition, memory bandwidth, CPU contention, disk)? Does it grow GPU-bound vs CPU-bound?

### Takeaway
The NVENC encode core is a separate engine, but RGB input and several quality features (AQ, look-ahead, 2-pass, weighted prediction, temporal filter, 8-to-10-bit conversion before Blackwell) run as CUDA work. That CUDA work, plus the capture copy and any colour conversion, competes with the game for SM time and memory bandwidth. In a GPU-bound game this competition shows up directly as lost fps. In a CPU-bound or frame-capped game it mostly fits into GPU idle time.

### Cited Findings
- **[VENDOR, NVIDIA App Note SDK 13.0]** NVENC "is independent of graphics/CUDA cores". With encoding offloaded, "the graphics/CUDA cores and the CPU cores are free for other operations", and in game recording this "makes the graphics engine fully available for game rendering". — [NVENC Application Note 13.0](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/nvenc-application-note/index.html)
- **[VENDOR, NVENC Programming Guide 13.0, sec. 6.4]** The guide says: "Although the core video encoder hardware on GPU is completely independent of CUDA cores or graphics engine", some features "internally use CUDA for hardware acceleration". It lists two-pass rate control for HQ presets, look-ahead, all adaptive quantization modes, weighted prediction, **encoding of RGB content**, and temporal filtering. NVIDIA calls the impact "minimal". **So yes, the RGB/ARGB input path uses CUDA (SM) time.** — [NVENC Programming Guide 13.0](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/nvenc-video-encoder-api-prog-guide/index.html)
- **[VENDOR, same guide]**
  - "Temporal AQ uses CUDA pre-processing and hence requires CUDA processing power, depending upon resolution and content."
  - Weighted prediction "uses CUDA pre-processing".
  - "prior to Blackwell, NVENC performs an internal CUDA 8 to 10-bit conversion of the input prior to encoding". On Ada, 10-bit HEVC/AV1 from 8-bit input therefore adds CUDA work.
  - "performance of 2-pass rate control mode is lower than that of 1-pass".
  - Source: [NVENC Programming Guide 13.0](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/nvenc-video-encoder-api-prog-guide/index.html)
- **[VENDOR, same guide, sec. 6.3, directly relevant to DDA]**
  - Calling `IDXGIOutputDuplication::AcquireNextFrame` on one thread while calling `NvEncLockBitstream`/`NvEncUnlockBitstream` on another "can lead to suboptimal or undefined behavior", because "NvEncLockBitstream can internally use the application's DirectX device".
  - For such applications the recommended settings are `enableEncodeAsync = 1`, `doNotWait = 0` and `enableOutputInVidmem = 0`.
  - NVIDIA recommends asynchronous mode "whenever possible" (Windows 10+). The client "should avoid making any blocking calls from the main encoder processing thread".
  - Source: [NVENC Programming Guide 13.0](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/nvenc-video-encoder-api-prog-guide/index.html)
- **[ANECDOTE, OBS forum, RTX 2060 Super]** A user found NVENC encoding "uses quite a bit of CUDA computing power and watts". A reply attributes this to OBS options that use CUDA: Max Quality (multipass), Psycho Visual Tuning (AQ) and Look-ahead. — [OBS forum, "NVENC actually uses a lot of GPU resources and watts"](https://obsproject.com/forum/threads/nvenc-actually-uses-a-lot-of-gpu-resources-and-watts.154685/post-566395)
- **[MEASURED, GStreamer discussion]** In GStreamer's tests, a single pass that combined colour conversion with encoding took about one third of the time of a separate conversion followed by encoding. NVENC does not expose its RGB-to-YUV conversion parameters. — [GStreamer Discourse](https://discourse.gstreamer.org/t/nvcudah264enc-vs-nv264enc-sinks-performance/1223)
- **[ANECDOTE, OBS staff]**
  - Running a game without VSync or a frame limiter "will frequently cause performance issues with OBS because your GPU will be maxed out". The recommended fixes are capping fps or enabling VSync so the GPU stays below full usage.
  - OBS's CPU "Process priority" setting only affects CPU scheduling, not the GPU.
  - Sources: [OBS forum, game lag when recording](https://obsproject.com/forum/threads/game-lag-when-recording-using-obs-log-file-included.176335/post-650036); [OBS forum, process priority](https://obsproject.com/forum/threads/is-there-a-way-to-give-obs-more-process-priority.174910/post-644588)
- **[ANECDOTE]** "The game is prioritised over OBS", so whenever the game maxes out the GPU, recording fps suffers badly. — [OBS forum, losing fps with NVENC](https://obsproject.com/forum/threads/losing-fps-if-i-start-recording-with-nvenc-new.115820/latest)
- **[Context, forum/StackOverflow]** Composed presentation (DWM) vs Independent Flip can differ by about one frame of latency. One developer reported about 25–30 ms more latency on machines stuck in "Composed: Flip". PresentMon labels Independent Flip as the app having been granted a hardware overlay plane. — [gamedev.net](https://gamedev.net/forums/topic/688536-how-to-get-only-1-frame-of-latency-with-capped-framerate-and-vsync/); [StackOverflow mirror](https://kiwix.lema.org/stackoverflow.com_en_all_2023-11/questions/72558096/enforce-use-of-independent-flip-mode-with-dxgi-flip-swapchain); [Blur Busters forum](https://forums.blurbusters.com/viewtopic.php?p=88577)

### Inferences
- Cost buckets for the DDA → copy → NVENC pipeline, ordered by likely impact on a GPU-bound game:
  1. **GPU time-slicing and preemption.** Every DDA frame needs DWM to produce or compose the desktop image, plus the recorder's own copy or convert work. These packets context-switch against the game. The prototype's SetGPUThreadPriority(7) makes them preempt the game sooner (see Q3).
  2. **Colour conversion.** Whether done by NVENC's internal CUDA path for ARGB or by your own shader, it is SM work. It is cheap per frame, but it is not free while the game uses 100% of the GPU.
  3. **NVENC CUDA-assisted features** (AQ, look-ahead, multipass, weighted prediction, temporal filter, 10-bit on Ada). Avoidable.
  4. **Memory bandwidth.** By arithmetic, a 1080p BGRA frame is 8.29 MB, so 144 fps means about 1.19 GB/s per pass. A copy (read + write) is about 2.4 GB/s, and conversion plus encoder reads add a few GB/s more. That is small next to a 4060 Ti's memory bandwidth (several hundred GB/s; not sourced here), but not zero.
  5. **CPU.** Thread wakeups and D3D11 immediate-context locking, which matter mostly when the game is CPU-bound.
  6. **Disk.** Negligible for a RAM ring buffer. It only matters while flushing a save.
- **GPU-bound vs CPU-bound.** When the GPU is at 100%, any extra GPU work (including DWM work triggered by duplication) comes straight out of game frames. When CPU-bound or frame-capped, the GPU has idle gaps and the same work is mostly hidden. The prototype's 4% loss was measured in the worst case (GPU-bound), so expect less in capped play.
- **Possible hidden cost.** If running DDA changes the game's PresentMode from "Hardware: Independent Flip" to "Composed: Flip" (forced DWM composition), that would add latency and some GPU work even if fps barely moves. This is untested. Check it with PresentMon's PresentMode column with the recorder on vs off.
- Feeding NVENC ARGB directly (one internal CUDA pass) is probably cheaper than your own copy, then convert, then encode. The GStreamer single-pass evidence supports this. But NVENC does not expose the colour matrix, so verify the BT.709 limited-range output.

### Gaps
- No source quantifies the per-frame CUDA cost of NVENC's internal ARGB→YUV conversion on Ada.
- No source confirms whether DXGI Desktop Duplication forces fullscreen or borderless games out of Independent Flip / MPO into DWM composition. This needs empirical testing.
- Not researched or sourced: whether the D3D11 VideoProcessor (VideoProcessorBlt) path for RGB→NV12 runs on dedicated video hardware or on SMs on NVIDIA.
- No published measurement of disk-write impact for replay-buffer flushes.

---

## Q3. How do GPU scheduling priority, HAGS and CPU priority affect recording smoothness vs game fps? What does OBS do about "encoding overloaded" and render lag?

### Takeaway
OBS deliberately raises its own GPU priority (REALTIME process class, or HIGH when HAGS is on, plus SetGPUThreadPriority) so that its capture and compositing are not starved by fullscreen games. That protects the recording at the expense of the game. For a recorder whose #1 rule is "never affect the game", this is the wrong default. Windows' normal behaviour (the foreground game wins) is closer to the goal, at the cost of occasionally dropped capture frames when the GPU is pegged.

### Cited Findings
- **[VENDOR, OBS source, libobs-d3d11/d3d11-subsystem.cpp]** OBS calls `D3DKMTSetProcessSchedulingPriorityClass(GetCurrentProcess(), hags_enabled ? D3DKMT_SCHEDULINGPRIORITYCLASS_HIGH : D3DKMT_SCHEDULINGPRIORITYCLASS_REALTIME)` and then `dxgiDevice->SetGPUThreadPriority(GPU_PRIORITY_VAL)`. This is done only when `desc.VendorId != 0x8086` (not on Intel). On failure it logs "D3D11 GPU priority setup failed (not admin?)". When HAGS is on it logs the warning "Hardware-Accelerated GPU Scheduling enabled on adapter!". — [OBS d3d11-subsystem.cpp](https://github.com/obsproject/obs-studio/blob/master/libobs-d3d11/d3d11-subsystem.cpp)
- **[VENDOR, OBS commit history]**
  - A 2019 commit, "Set maximum GPU priority", was made because OBS "would be deprioritized by Windows over fullscreen games, causing OBS to lag out whereas the games would still run fine".
  - An Oct 2019 commit, "Don't set GPU priority on Intel adapters", followed.
  - A May 2023 commit, "libobs-d3d11: Set priority to high instead of realtime if HAGS enabled", shipped in OBS 29.1.2 as "Adjusted GPU priority level when Hardware-Accelerated GPU Scheduling (HAGS) is enabled". The rationale for HIGH instead of REALTIME was not found.
  - Sources: [obs-studio mirror, libobs-d3d11 commits](https://git.tjdev.de/mirror/obs-studio/commits/commit/993c46c8a2e74f34454daf58fec58e1636e557c5/libobs-d3d11); [OBS 29.1.2 tag](https://gitlab.dsi.universite-paris-saclay.fr/khuon-viet.pham/obs-studio/-/tags/29.1.2)
- **[ANECDOTE, OBS staff]** When OBS runs without admin rights, it "will not be able to request a higher GPU priority for itself", which is the likely cause of render lag. Staff also describe HAGS as "known to cause issues with OBS" and recommend disabling it. — [OBS forum, game lag when recording](https://obsproject.com/forum/threads/game-lag-when-recording-using-obs-log-file-included.176335/post-650036); [OBS forum, choppy and laggy videos](https://obsproject.com/forum/threads/videos-come-out-choppy-and-laggy.174381/post-642622)
- **[ANECDOTE, OBS staff/users]** For GPU-maxed games the standard fixes are capping the game's fps, enabling VSync, lowering settings, or using single-pass encoding. Adding OBS under Windows Graphics settings only selects the GPU and does not change priority. — [OBS forum, OBS prioritize GPU?](https://obsproject.com/forum/threads/obs-prioritize-gpu.60196/latest)
- **[ANECDOTE, OBS]** "Encoding overloaded" (encoder lag) is a separate problem from render lag. For CPU-encoder overload the advice is to switch to the GPU encoder and run the Auto-Configuration Wizard. — [OBS forum, game lag when recording](https://obsproject.com/forum/threads/game-lag-when-recording-using-obs-log-file-included.176335/)
- **[VENDOR/OSS, Looking Glass]** The Looking Glass host (DXGI and NvFBC capture) calls `D3DKMTSetProcessSchedulingPriorityClass` with REALTIME and reports a capture performance benefit. When the call fails it suggests running the host as a service. — [Looking Glass commit (mirror)](https://ayakael.net/mirrors/looking-glass/commit/78b8e2a73c8cb197a5d58cc56555bd4c066a18ea)
- **[VENDOR, Microsoft]** The public doc for D3DKMTSetProcessSchedulingPriorityClass documents only the function signature and the return codes STATUS_SUCCESS and STATUS_INVALID_PARAMETER. It does not document privilege requirements or the semantics of each class. — [Microsoft Learn](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/d3dkmthk/nf-d3dkmthk-d3dkmtsetprocessschedulingpriorityclass)

### Inferences
- **Priority direction matters.** The prototype calls SetGPUThreadPriority(7), the maximum, which is OBS's "protect the recorder" choice. That likely contributes to the 4% loss, because the recorder's GPU packets (and DWM work it triggers) preempt the game more readily.
  - For a game-first design, A/B test GPU thread priority 7 vs 0 (default) vs a negative value. Expect fps loss to fall as priority falls, while capture-frame drops rise under 100% GPU load.
  - Pick the lowest priority that still yields acceptable capture smoothness at a realistic frame cap.
- Do not use REALTIME GPU process class. It needs elevation (per OBS's "not admin?" log and the Looking Glass service suggestion). It would also strengthen the recorder's claim on the GPU against the game.
- **CPU side.** A BELOW_NORMAL process with a THREAD_PRIORITY_HIGHEST thread ends up at roughly normal base priority under standard Windows rules (not sourced here). That is reasonable for a lightweight capture thread. The bigger CPU risk is blocking calls and lock contention on the D3D11 immediate context (see NVIDIA's DDA threading note in Q2), not raw priority.
- **HAGS.** OBS treats HAGS as a risk (it warns and downgrades priority to HIGH). Since HAGS can be toggled on 19044, test with HAGS on and off. Frame generation (DLSS 3) requires HAGS on, so a user with FG enabled cannot simply turn it off. That requirement is from NVIDIA's DLSS 3 requirements, which I did not fetch here.
- **Graceful degradation.** Under GPU saturation, the "game-first" recorder should drop or duplicate capture frames rather than push its priority up. OBS's 2019 commit message confirms that, at default priority, Windows already favours the fullscreen game ("games would still run fine").

### Gaps
- No public benchmark measures game fps or 1% lows as a function of the recorder's GPU priority (SetGPUThreadPriority −7..7, or the D3DKMT classes), with HAGS on vs off.
- The exact reason OBS moved from REALTIME to HIGH under HAGS (the 29.1.2 PR discussion) was not found.
- Microsoft does not document whether non-admin processes can set the HIGH class (vs REALTIME) via D3DKMTSetProcessSchedulingPriorityClass.

---

## Q4. Does recording at 144 fps cost much more than 60 fps for the game?

### Takeaway
NVENC itself has large headroom: one Ada engine does about 650 fps of 1080p H.264 at P3, or about 1000 fps of HEVC/AV1 at P1. The encoder block will not be the bottleneck at 144 fps. But every per-frame GPU cost (capture copy, conversion, CUDA-assisted features, DWM work) scales linearly with capture rate, so 144 fps capture costs roughly 2.4x the per-second GPU work of 60 fps.

### Cited Findings
- **[VENDOR, NVIDIA App Note 13.0, per NVENC engine, 1080p 8-bit 4:2:0, highest clocks]**
  - Ada H.264: P1 LL 910 fps, P3 LL 652, P5 LL 291, P7 LL 247.
  - Ada HEVC: P1 LL 1055 fps, P3 LL 494, P5 LL 343, P7 HQ 181.
  - Ada AV1: P1 LL 1090 fps, P3 LL 774, P5 LL 512, P7 LL 356.
  - "Unless Split Frame Encoding is enabled, performance with single encoding session cannot exceed performance per NVENC."
  - Some Ada chips have 2 or 3 NVENCs (AD104 is cited). AD106/RTX 4060 Ti is not listed; the user reports one engine.
  - Source: [NVENC Application Note 13.0](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/nvenc-application-note/index.html)

### Inferences
- At 1080p144 with a P1–P4 preset, one NVENC engine is roughly 15–25% busy for H.264 (144/910 to 144/652). The P7/HQ presets get close to the limit, especially for HEVC P7 HQ at 181 fps. Use P1–P4 with low-latency tuning.
- Capturing all three 1080p monitors at 144 fps (432 encode fps total) would approach the limits of P5+ presets. Capture only the game's monitor.
- The game-side cost of 144 vs 60 fps comes from the per-frame copy, convert and DWM work. A practical compromise is to capture at the game's actual present rate capped to the display refresh, and skip frames when the desktop has not changed (DDA only returns a frame on update). Measure 60 vs 144 capture directly in the A/B protocol (Q6), because no published data compares them.

### Gaps
- No published test compares game fps or 1% lows when a recorder captures at 60 vs 120/144 fps.

---

## Q5. Interaction with NVIDIA Reflex, G-SYNC and frame generation (DLSS 3)

### Takeaway
Evidence is thin. OBS game capture has repeatedly failed to record DLSS frame-generated frames, while ShadowPlay records them. Whether DDA sees generated frames depends on what reaches DWM or the display, so it must be tested. I found no measured data on Reflex latency or G-SYNC behaviour with a recorder running.

### Cited Findings
- **[ANECDOTE, OBS forum, 2025]**
  - With DLSS Frame Generation, OBS records only rendered frames. For example, 2x FG on a 30 fps base yields 30 fps in OBS, while FSR FG records normally.
  - NVIDIA's own recorder (ShadowPlay) captures the generated frames.
  - A later report says driver 610.47 allowed capture of DLSS-generated frames only with "Capture third-party overlays" enabled in Game Capture. Another user still could not capture them on 576.40.
  - A separate thread says driver 580.88 "corrupts OBS capability to record frame generation".
  - Sources: [OBS forum, FG capture](https://obsproject.com/forum/threads/obs-cannot-capture-frames-generated-by-frame-generation-technologies.187614/); [OBS forum, driver 580.88](https://obsproject.com/forum/threads/nvidia-driver-580-88-corrupts-obs-capability-to-record-frame-generation.189039)
- **[MEASURED, TechPowerUp, older Reflex data]** Reflex cut Overwatch end-to-end latency from 25.7 ms to 16.8 ms at High settings and from 82.3 ms to 35.0 ms at Epic settings (GPU-bound). Reflex gains are largest when the GPU is saturated. — [TechPowerUp](https://techpowerup.com/review/nvidia-reflex-review-test-ldat-v2/4.html)
- **[VENDOR, PresentMon]** PresentMon can tag each frame as application-rendered or driver/SDK-generated (FrameType, needs `--track_frame_type`). It reports `MsPCLatency` ("Time between PC receiving input and frame being sent to the display") for apps instrumented with PC Latency events. — [PresentMon console README](https://github.com/GameTechDev/PresentMon/blob/main/README-ConsoleApplication.md)

### Inferences
- **Reflex.** Reflex keeps the render queue near empty by pacing the CPU to the GPU. Recorder GPU work that preempts the game lengthens GPU frame time, and Reflex then lowers fps rather than queueing. So the recorder's cost shows up as lost fps plus slightly higher PC latency, not as hidden queueing. A lower recorder GPU priority directly protects this.
- **G-SYNC / VRR.** A recorder that perturbs GPU frame times adds frame-time variance, which VRR displays show as uneven pacing even when average fps barely moves. Track MsBetweenDisplayChange variance, not just fps.
- **Independent Flip.** If DDA forces the game from Independent Flip into DWM composition (see Q2 gap), that could add latency and affect how VRR is applied. Verify the PresentMode column with the recorder on vs off.
- **Frame generation.** DDA duplicates the desktop image. Whether FG frames appear in it depends on whether they reach DWM's composition path (borderless or windowed) or go out through independent flip. Test: compare the recorded file's unique frame count to PresentMon's displayed-frame count with FrameType tracking.

### Gaps
- No measurement found of Reflex/PCL latency, G-SYNC behaviour, or DLSS FG frame pacing with ShadowPlay, OBS or a DDA recorder running vs not running.
- No official NVIDIA or OBS statement on FG capture compatibility was found.

---

## Q6. How do reviewers measure overhead, and a concrete A/B test protocol

### Takeaway
Use PresentMon 2.x (the frame-time engine that FrameView and CapFrameX also build on; that relationship is not sourced here). Capture a fixed scene for a fixed duration with the recorder fully off vs running. Compare average fps, 1%/0.1% lows from frame times, frame-time and display-change variance, GPU busy time, PresentMode, and PC latency where Reflex markers exist. Repeat enough interleaved runs that the difference can be separated from run-to-run noise.

### Cited Findings
- **[VENDOR, PresentMon]**
  - Example command: `PresentMon --process_name game.exe --timed 60 --terminate_after_timed --output_file capture.csv --v2_metrics --track_pc_latency`.
  - Metrics: MsBetweenPresents; MsBetweenDisplayChange ("how long the previous frame was displayed"); MsGPUBusy ("How long the GPU was actively working on this frame"); MsGPUWait; MsCPUBusy; MsAnimationError; MsClickToPhotonLatency; MsAllInputToPhotonLatency; MsPCLatency (requires PC Latency instrumentation); PresentMode ("Hardware: Independent Flip" vs "Composed: Flip"); FrameType (`--track_frame_type`, Beta).
  - Source: [PresentMon console README](https://github.com/GameTechDev/PresentMon/blob/main/README-ConsoleApplication.md)
- **[MEASURED method, TechPowerUp]** End-to-end latency measured with LDAT and a 1200 fps camera, where each camera frame is about 0.833 ms, with mouse click latency subtracted. — [TechPowerUp Reflex methodology](https://techpowerup.com/review/nvidia-reflex-review-test-ldat-v2/3.html)
- **[MEASURED method, Tom's Hardware]** A high-speed camera (1000+ fps) records the mouse and the monitor; results averaged over 20 clicks. — [Tom's Hardware Reflex Latency Analyzer](https://www.tomshardware.com/news/nvidia-reflex-latency-analyzer)
- **[Method, Bright Side of News]** Capture-path latency measured with a synchronized flash-and-beep clip, comparing captured footage to the source. — [BSN input-lag testing](https://brightsideofnews.com/blog/input-lag-testing-bsn/)

### Inferences (proposed protocol, synthesized; not from a single source)

1. **Fix the system.** Same driver, HAGS state, power plan and display mode on every run. Record the game's PresentMode with the recorder off (it should be "Hardware: Independent Flip" for fullscreen or borderless on the primary monitor). Close other overlays (NVIDIA App overlay, Steam overlay, Game Bar) so only the recorder varies.
2. **Conditions**, each tested separately:
   - A = recorder not running.
   - B = recorder running with the replay buffer active.
   - Optional: B60 (capture capped at 60 fps) and B144 (capture at 144 fps), and B with GPU priority 7 vs 0 vs −3.
3. **Scenarios**, each tested separately:
   - (i) GPU-bound and uncapped (worst case).
   - (ii) The user's real setup: G-SYNC with an fps cap below 144 Hz, Reflex on if available.
   - (iii) A CPU-bound title (for example a high-fps esports game at low settings).
   - (iv) Optionally DLSS FG on.
4. **Runs.** Deterministic scene: a built-in benchmark or a fixed replay/demo. Use 60–90 s captures with PresentMon `--v2_metrics`. Discard a warm-up run. Interleave the order as A B B A A B ... for at least 5 runs per condition, which cancels thermal and boost drift.
5. **Metrics per run:**
   - Average fps = frames / total time.
   - 1% and 0.1% lows = fps at the 99th and 99.9th percentile frame time. Use percentile frame time, not "average of the slowest 1%", and state which method you use.
   - Frame-time standard deviation, and the count of frames over 2x the median.
   - MsBetweenDisplayChange variance (what the user sees on a VRR display).
   - Mean MsGPUBusy (GPU time stolen from the game).
   - PresentMode distribution, so you notice if the recorder forces composition.
   - MsPCLatency for Reflex games, or MsAllInputToPhotonLatency otherwise.
6. **Statistics.** Compute the mean difference B − A and its 95% confidence interval across runs.
7. **Pass criteria for "no noticeable impact":**
   - Average fps delta ≤ 1%, and the confidence interval includes or nearly includes 0.
   - 1% low delta ≤ 2%, 0.1% low delta within noise.
   - No PresentMode change.
   - PC latency delta ≤ 0.5 ms.
   - In scenario (ii), zero extra frames missing the cap (frame-time p99 unchanged).
   - These thresholds are proposed engineering targets, not industry standards.
8. **Check the recorder's own health in the same runs.** Captured-frame count vs expected, duplicated or dropped frames in the output file, and NVENC queue depth. A "game-first" recorder is allowed to drop its own frames under GPU saturation but must log it.
9. **Optional subjective check.** A blind A/B where someone else toggles the recorder and the player guesses on/off over 10+ trials. Better than chance means the impact is noticeable.

### Gaps
- No primary CapFrameX or FrameView documentation was fetched. Their exact percentile definitions (for example CapFrameX's "P1" vs "1% low average") are not sourced here.
- No source documents the typical run-to-run noise floor for PresentMon captures, which is needed to choose the number of runs.

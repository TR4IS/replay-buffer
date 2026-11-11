# NVENC Encoder Settings for High-Quality, Low-Impact Game Recording (RTX 4060 Ti / Ada)

Research date: 2026-11-06. Target app: an always-on replay buffer that captures 1920x1080 at up to 144 fps through nvEncodeAPI (header 12.1). It currently uses HEVC, then H.264 after a playback problem; P5; HIGH_QUALITY tuning; VBR with CQ 19 and a 100 Mbps cap; spatial AQ; no B-frames; IDR every 2 s; VFR (only changed frames); and ARGB textures passed straight in.

Source-quality legend: "(snippet)" means the claim came from a search-result summary and the full page was not read. "(older data)" means pre-Ada hardware or a pre-2024 article.

Version context: SDK 12.2 added UHQ tuning (HEVC only), lookahead level, temporal filter, unidirectional B-frames and 8-to-10-bit input/output bit-depth fields. SDK 13.0 (January 2025) extended UHQ to AV1 and added H.264 10-bit and 4:2:2 for Blackwell. SDK 13.1 combined UHQ with iterative encoding. The app's 12.1 header therefore cannot use UHQ, `lookaheadLevel`, `tfLevel`, `enableUniDirectionalB` or `inputBitDepth`/`outputBitDepth` ([SDK 12.2 release notes](https://docs.nvidia.com/video-technologies/video-codec-sdk/12.2/read-me/index.html); [NVIDIA 12.2 blog](https://developer.nvidia.com/blog/improving-video-quality-with-nvidia-video-codec-sdk-12-2-for-hevc/); [NVIDIA Video Codec SDK page](https://developer.nvidia.com/nvidia-video-codec-Sdk); [NVIDIA SDK 13.1 blog](https://developer.nvidia.com/blog/?p=120592)).

---

## 1. AV1 vs HEVC vs H.264 on Ada NVENC: quality at 20-100 Mbps 1080p, and size at equal quality

### Takeaway
NVIDIA's own figure is that Ada AV1 needs about 40% less bitrate than Ada H.264 at equal quality at 1080p60, and the gap narrows as bitrate rises. At recording bitrates (above about 20-30 Mbps at 1080p), all three codecs sit near the VMAF ceiling, so the codec choice mostly changes file size and buffer RAM, not visible quality. I found no Ada-specific, high-bitrate 1080p VMAF table covering all three codecs on game content.

### Cited Findings
- On NVIDIA's test set of more than 100 videos, NVENC AV1 gets "~1.5-2 dB higher PSNR compared to NVENC H.264 at the same bit rate". At equal quality, AV1 reaches 42 dB PSNR at 7 Mbps where H.264 needs 11 Mbps. NVIDIA puts the saving at about 40% for AV1 over H.264 at 1080p60, with similar savings at 720p, 1440p and 4K — [NVIDIA dev blog: AV1 and Ada](https://developer.nvidia.com/blog/improving-video-quality-and-performance-with-av1-and-nvidia-ada-lovelace-architecture)
- The same blog shows AV1 ahead of H.264 on VMAF at low bitrates, with the gap narrowing as bitrate rises. The VMAF values are only in chart images, so no numbers are available — [NVIDIA dev blog: AV1 and Ada](https://developer.nvidia.com/blog/improving-video-quality-and-performance-with-av1-and-nvidia-ada-lovelace-architecture)
- NVIDIA's side-by-side example rates Ada AV1 at 18 Mbps (high-performance preset) as "comparable" to x264 medium at 30 Mbps, with AV1 encoding at 500 fps, "almost 9x faster" than x264 — [NVIDIA dev blog: AV1 and Ada](https://developer.nvidia.com/blog/improving-video-quality-and-performance-with-av1-and-nvidia-ada-lovelace-architecture)
- Ada HEVC data (RTX 4080 Super, hevc_nvenc P7, UHQ, fullres multipass, lookahead level 3, temporal AQ, B-ref middle, main10, 2160p HDR "Butterfly" clip, driver 572.16, FFmpeg 7.1, posted 2025-02-14): VMAF 68.2 at 2.4 Mbps, 81.8 at 4.6, 87.7 at 6.7, 90.7 at 8.6, 92.6 at 10.5 and 96.6 at 20.1 Mbps. The RTX 5090 was only +0.14 VMAF better at 20 Mbps but showed a -5.59% BD-rate overall: "The largest gains are at smaller bitrates, and the gap closes as they increase." This is 4K content, so 1080p at the same bitrate would score higher — [Codecalamity, RTX 5090 encoding first look](https://codecalamity.com/nvidia-rtx-5090-video-encoding-first-look/)
- An academic longitudinal study (arXiv 2605.01187, May 2026; GPUs GTX 1070, RTX 3060/3070, RTX 4070 Ti SUPER and RTX 5070 Ti) measured Blackwell HQ as 5.94% BD-rate better than Ada HQ for HEVC P7 — [Arunruangsirilert & Katto, summary on EmergentMind](https://www.emergentmind.com/papers/2605.01187)
- Tom's Hardware's VMAF tests (FFmpeg NVENC, RTX 4090/3090/1650) are the most-cited cross-codec comparison. A Maximum PC summary says the RTX 4070 Ti matched the 4090 in performance and quality, and NVENC took first place for H.264 at 1080p and AV1 at 1080p and 4K. Those tests used streaming-class bitrates (single-digit Mbps) — [Tom's Hardware](https://www.tomshardware.com/news/amd-intel-nvidia-video-encoding-performance-quality-tested) (snippet; the page could not be fetched); [Maximum PC via PressReader](https://www.pressreader.com/usa/maximum-pc/20230501/282239489925958) (snippet)
- H.264 NVENC with HQ tuning, VBR+CQ, spatial and temporal AQ, 3 B-frames and B-ref middle reached VMAF 99 at CQ 24.5 and 99.5 at CQ 21.5. Above about 10 Mbps, NVENC beat libx264 on VMAF; below that, libx264 led. (older data: GTX 1070 Ti, Pascal; Dec 2023) — [Gough's Tech Zone, Codec Round-Up 2023 Part 8](https://goughlui.com/?p=48157)
- A Doom9 user's view is that NVENC AV1 is not up to software AV1 at high quality settings but beats older HEVC and AVC hardware encoders — [Doom9 NVENC AV1 discussion](https://forum.doom9.org/archive/index.php/t-184832.html) (snippet, anecdotal)
- Ada NVENC speed per engine at 1080p (RTX 4090 at 2415 MHz, VBR HQ): AV1 P7 is 323 fps, HEVC P7 is 181 fps and H.264 P7 is 211 fps. So on Ada, AV1 at the slowest preset is far faster than HEVC at the slowest preset — [NVENC Application Note, SDK 13.0](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/nvenc-application-note/index.html)

### Inferences
- At 30-100 Mbps 1080p, H.264 on Ada NVENC is already near-transparent (the Pascal H.264 data reaches VMAF 99.5 around CQ 21; Ada is newer). The practical difference between codecs at recording bitrates is file and RAM size at equal quality, roughly AV1 at 0.6x of H.264 by NVIDIA's figure, with HEVC in between. For a RAM-held replay buffer, that size ratio matters more than any visible quality gap.
- Codec efficiency gains are smaller at high bitrates (NVIDIA: the gap narrows with bitrate). A 40% equal-quality saving measured around 7-11 Mbps will likely be smaller at 40-80 Mbps game footage. This is not measured.
- On Ada, AV1 P7 (323 fps) has more 1080p144 headroom than HEVC P7 (181 fps), so AV1 is also the "slow preset, still real-time" option, if playback compatibility allows it (see section 5).

### Gaps
- No source gives Ada VMAF for H.264, HEVC and AV1 side by side at 20-100 Mbps 1080p on game content. Tom's Hardware's page could not be fetched, and its bitrates are streaming-class anyway.
- No Ada-measured HEVC-vs-H.264 equal-quality bitrate ratio was found; only AV1-vs-H.264 (NVIDIA, about 40%).
- Rigaya's vq_results page (per-vendor VMAF comparison) was referenced in forums but not retrieved.

---

## 2. Presets P1-P7 and tuning (HQ, UHQ): quality vs encode cost, fps per preset, P7 at 1080p144, and GPU resource use

### Takeaway
On one Ada NVENC at 1080p (HQ, VBR), H.264 runs at about 885/647/283/211 fps for P1/P3/P5/P7, HEVC at 1037/706/411/181 and AV1 at 741/549/440/323. So P7 can, on paper, sustain 1080p144 for all three codecs, with thin headroom for HEVC (about 1.26x). P5 to P7 brings only a few percent quality, and P1 is the only clearly bad preset. The encoder core is a separate engine, but AQ, lookahead, two-pass, RGB input, temporal filtering and UHQ run on CUDA cores. UHQ in particular costs 13-18% of SM time, about 40% more board power and 20-38 frames of latency, so it is a poor fit while gaming.

### Cited Findings
- NVENC per-engine throughput, 1080p 4:2:0 8-bit, Ada measured on an RTX 4090 at 2415 MHz (SDK 13.0 Application Note). The note says performance "scales (almost) linearly with the clock speeds", and that one session cannot exceed the per-NVENC figure unless split-frame encoding is on — [NVENC Application Note 13.0](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/nvenc-application-note/index.html):

  | Codec | P1 VBR HQ | P3 VBR HQ | P5 VBR HQ | P7 VBR HQ | P7 CBR LL |
  |---|---|---|---|---|---|
  | H.264 | 885 | 647 | 283 | 211 | 247 |
  | HEVC | 1037 | 706 | 411 | 181 | 343 |
  | AV1 | 741 | 549 | 440 | 323 | 356 |

- Ada has 1-3 NVENC engines per chip; the Application Note names AD104 as having 2 — [NVENC Application Note 13.0](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/nvenc-application-note/index.html). OBS says split encoding needs an Ada GPU with two or more NVENCs, "such as a 4070 Ti or higher" — [OBS KB: Advanced NVENC Options](https://obsproject.com/kb/advanced-nvenc-options). Extra NVENCs do not speed up a single stream; they only add parallel-stream throughput unless split-frame encoding is used — [NVIDIA dev blog: AV1 and Ada](https://developer.nvidia.com/blog/improving-video-quality-and-performance-with-av1-and-nvidia-ada-lovelace-architecture)
- Split-frame encoding turns on automatically only for fast presets (HQ P1-P2; LL/ULL P1-P4) and only above 4K for HEVC/AV1 (OBS: 2160p and above). It may reduce quality, and at low bitrates OBS reports a visible seam — [NVIDIA dev blog: AV1 and Ada](https://developer.nvidia.com/blog/improving-video-quality-and-performance-with-av1-and-nvidia-ada-lovelace-architecture); [OBS KB](https://obsproject.com/kb/advanced-nvenc-options)
- Preset quality: P1 is an outlier, "so poor that it reaches" libvpx and libxvid levels. P2-P5 are similar, P6-P7 are similar to each other, and P7 has a slight PSNR edge and the best SSIM (older data: GTX 1070 Ti, H.264) — [Gough's Tech Zone](https://goughlui.com/?p=48157)
- Tom's Hardware found that switching to P7 did little for VMAF while cutting encode performance by 30-50% — [Tom's Hardware](https://www.tomshardware.com/news/amd-intel-nvidia-video-encoding-performance-quality-tested) (snippet). An OBS forum answer puts P5 vs P7 at "only a few percent" depending on content — [OBS forum: NVENC preset and multipass](https://obsproject.com/forum/threads/nvenc-streaming-preset-and-multipass-mode-what-settings-are-correct-for-streaming.163979/) (snippet, community)
- CUDA usage: the programming guide (section 6.4) lists CUDA as used for "two-pass rate control in high-quality presets, lookahead, all adaptive quantization modes, weighted prediction, RGB content encoding, and temporal filtering". It also states "the core video encoder hardware on GPU is completely independent of CUDA cores or graphics engine" and that the impact on CUDA or graphics performance is minimal — [NVENC Programming Guide 13.0](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/nvenc-video-encoder-api-prog-guide/index.html). The Application Note says NVENC is "independent of graphics/CUDA cores" — [NVENC Application Note 13.0](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/nvenc-application-note/index.html)
- For 8-bit input encoded as 10-bit, H.264 and AV1 use an internal hardware conversion; HEVC uses a CUDA conversion before Blackwell and hardware from Blackwell on — [NVENC Programming Guide 13.0](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/nvenc-video-encoder-api-prog-guide/index.html)
- UHQ (SDK 12.2, HEVC; extended to AV1 in SDK 13.0) "combines lookahead level and temporal filtering", "fixes the number of B-frames to five, while using the middle B-frames as a reference" and "disables adaptive I and adaptive B-frames and uses a fixed GOP". UHQ P4 and P7 beat x265 Slow on bit-rate savings; UHQ P1 is "only 3% higher than x265 Slow" — [NVIDIA 12.2 blog](https://developer.nvidia.com/blog/improving-video-quality-with-nvidia-video-codec-sdk-12-2-for-hevc/). The OBS code forces 4 B-frames under UHQ, which conflicts slightly with NVIDIA's "five" — [OBS nvenc.c](https://github.com/obsproject/obs-studio/blob/master/plugins/obs-nvenc/nvenc.c)
- UHQ measured costs (arXiv 2605.01187, May 2026):
  - BD-rate gain over HQ: about 12-13% for HEVC and 16-19% for AV1.
  - Throughput drops 50-60%. At 4K P7: HEVC 45 fps HQ vs 37 UHQ; AV1 104 HQ vs 46 UHQ.
  - SM (CUDA) use reaches up to 18% on Ampere and 13-15% on Blackwell, vs 1-2% in HQ.
  - Ada board power for HEVC P1 rises from 52.2 W (HQ) to 72.5 W (UHQ).
  - End-to-end latency: about 7 frames in HQ vs 20-38 frames in UHQ; GOP settings are locked.

  Source: [Arunruangsirilert & Katto, EmergentMind summary](https://www.emergentmind.com/papers/2605.01187). This is a secondary summary; some numbers come from the abstract.
- The temporal filter "is done using a CUDA kernel". It gives 4-5% average gains on natural video but "may not provide significant gains for synthetic or screen content" — [NVIDIA 12.2 blog](https://developer.nvidia.com/blog/improving-video-quality-with-nvidia-video-codec-sdk-12-2-for-hevc/). The programming guide adds that the temporal filter "uses CUDA pre-processing and hence requires CUDA processing power", and that UHQ "enables lookahead and temporal Filter, that have higher memory requirements" — [NVENC Programming Guide 13.0](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/nvenc-video-encoder-api-prog-guide/index.html)
- OBS describes UHQ as "optimised for live-action with artifacts such as camera sensor noise", advises against it for gaming, and says it "Significantly reduces throughput" — [OBS KB](https://obsproject.com/kb/advanced-nvenc-options). The OBS code comment says UHQ is "only supported on Turing or later" — [OBS nvenc-properties.c](https://github.com/obsproject/obs-studio/blob/master/plugins/obs-nvenc/nvenc-properties.c)
- Conflict on AV1 UHQ: NVIDIA's SDK page says UHQ was extended to AV1 in SDK 13.0 — [NVIDIA SDK page](https://developer.nvidia.com/nvidia-video-codec-Sdk). Wikipedia credits Blackwell with "AV1 Ultra High Quality mode" — [Wikipedia: NVENC](https://en.wikipedia.org/wiki/NVENC). Whether AV1 UHQ runs on Ada is unconfirmed.

### Inferences
- RTX 4060 Ti (AD106) almost certainly has one NVENC: AD104 (4070 Ti) is the smallest dual-engine Ada chip per NVIDIA and OBS, and AD107 is listed with 1. So the 1080p figures above are the ceiling, with no split-frame encoding to help.
- At 144 fps every frame, H.264 P5 uses about 51% of the engine (144/283 at 4090 clocks), H.264 P7 about 68%, HEVC P7 about 80% and AV1 P7 about 45%. With VFR, the average load is lower. The 4060 Ti's clocks differ from the 4090's 2415 MHz, and the table's exact feature settings are unknown, so keep 20-30% headroom. That makes HEVC P7 at a constant 144 fps risky, while H.264 P6/P7 and AV1 P7 look feasible. This is not measured on a 4060 Ti.
- Because NVENC is a separate engine, the preset choice itself barely touches game fps. Game impact comes from the CUDA-side features (AQ, lookahead, two-pass, RGB-to-YUV conversion, temporal filter, UHQ), from VRAM, and from memory bandwidth. "Negligible impact" therefore means avoiding UHQ and the temporal filter, rather than avoiding P6/P7.
- UHQ is the wrong tool here. Its main gain (the temporal filter) targets camera noise, it costs 13-18% SM and about 40% board power, it needs SDK 12.2+, and it adds 20-38 frames of latency.

### Gaps
- No Ada fps data for P2, P4 or P6, or for 1080p with AQ, lookahead or multipass enabled; the table's exact feature settings are not stated.
- No measured 4060 Ti NVENC clock or throughput.
- No Ada-measured VMAF/BD-rate per preset (P1-P7) for H.264, HEVC or AV1 on game content.
- How much SM time spatial AQ alone or the RGB-to-YUV conversion uses on Ada is not quantified (the paper's "1-2% in HQ" is the only figure).
- The NVIDIA support matrix row for the RTX 4060 Ti could not be fetched to confirm the engine count.

---

## 3. Multipass, lookahead, temporal AQ, spatial AQ strength, B-frames, B-frame references and reference frames

### Takeaway
For recording, the cheap, clearly positive levers are B-frames with B-frame references (HEVC/AV1; documented as free in performance), spatial AQ (OBS uses strength 8) and, for HEVC/AV1 on SDK 12.2+, 8-to-10-bit encoding. Temporal AQ, lookahead and multipass use CUDA and add buffering. They matter mainly for bitrate-constrained rate control, and they add frame latency that interacts badly with "encode only changed frames". No numeric quality data was found for multipass, temporal AQ or reference-frame count.

### Cited Findings
- Multipass: quarter-resolution and full-resolution two-pass modes exist; "performance of 2-pass rate control mode is lower than that of 1-pass rate control mode" — [NVENC Programming Guide 13.0](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/nvenc-video-encoder-api-prog-guide/index.html). Two-pass rate control in HQ presets uses CUDA (same source). OBS defaults multipass to `qres` and sets `rcParams.multiPass` directly; OBS disables multipass for lossless — [OBS nvenc-properties.c](https://github.com/obsproject/obs-studio/blob/master/plugins/obs-nvenc/nvenc-properties.c); [OBS nvenc.c](https://github.com/obsproject/obs-studio/blob/master/plugins/obs-nvenc/nvenc.c)
- Lookahead:
  - It buffers up to 32 frames and adaptively inserts I and B frames. Until enough frames are queued, the encode call returns `NV_ENC_ERR_NEED_MORE_INPUT`. `lookaheadLevel` 0-3 (SDK 12.2+) trades performance for quality — [NVENC Programming Guide 13.0](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/nvenc-video-encoder-api-prog-guide/index.html)
  - "A higher lookahead level gives better quality at reduced performance and increased video memory usage" — [NVIDIA 12.2 blog](https://developer.nvidia.com/blog/improving-video-quality-with-nvidia-video-codec-sdk-12-2-for-hevc/)
  - OBS: `lookaheadDepth` ranges from 0 to (31 minus the number of B-frames), and `disableIadapt`/`disableBadapt` only take effect when lookahead is on — [OBS KB](https://obsproject.com/kb/advanced-nvenc-options)
  - OBS defaults lookahead to the device capability with a default depth of 8, clamped by its output-buffer budget — [OBS nvenc.c](https://github.com/obsproject/obs-studio/blob/master/plugins/obs-nvenc/nvenc.c)
  - OBS forum users advise disabling lookahead and psycho-visual tuning when the encoder overloads, because they use CUDA — [OBS forum thread](https://obsproject.com/forum/threads/which-obs-setting-give-the-best-recording-quality-to-me.120279/post-451833) (snippet, community)
- Spatial AQ: `aqStrength` ranges from 1 (least aggressive) to 15 (most); if unset, the driver chooses; PSNR may drop — [NVENC Programming Guide 13.0](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/nvenc-video-encoder-api-prog-guide/index.html). OBS's "Adaptive Quantization" (formerly "Psycho Visual Tuning") sets `enableAQ = 1`, `aqStrength = 8`, and `enableTemporalAQ` whenever the GPU supports it. It is on by default — [OBS nvenc.c](https://github.com/obsproject/obs-studio/blob/master/plugins/obs-nvenc/nvenc.c); [OBS KB](https://obsproject.com/kb/advanced-nvenc-options)
- Temporal AQ can cause bit-count fluctuation within a GOP and is not recommended where strict per-frame CBR is needed. All AQ modes use CUDA — [NVENC Programming Guide 13.0](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/nvenc-video-encoder-api-prog-guide/index.html)
- B-frames as reference "improves subjective and objective encoded quality with no performance impact" and is strongly recommended with multiple B-frames — [NVENC Programming Guide 13.0](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/nvenc-video-encoder-api-prog-guide/index.html). OBS describes it as able to "slightly improve quality with no performance impact" — [OBS KB](https://obsproject.com/kb/advanced-nvenc-options)
- OBS defaults to 2 B-frames when the GPU supports them. It exposes B-frame reference mode only for HEVC and AV1, because for H.264 it "seems to cause issues with some decoders" — [OBS nvenc-properties.c](https://github.com/obsproject/obs-studio/blob/master/plugins/obs-nvenc/nvenc-properties.c)
- Unidirectional B-frames (HEVC, SDK 12.2+): 3-4% average BD-BR savings over P-frames, up to 15% on some sequences, designed for low-latency use — [NVIDIA 12.2 blog](https://developer.nvidia.com/blog/improving-video-quality-with-nvidia-video-codec-sdk-12-2-for-hevc/)
- Buffer requirement with B-frames: allocate at least 1 + N_B input/output buffers; async mode needs at least 4 + N_B output buffers — [NVENC Programming Guide 13.0](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/nvenc-video-encoder-api-prog-guide/index.html)
- Memory-saving guidance from NVIDIA lists B-frames, lookahead, temporal filter, UHQ, AQ, weighted prediction, more reference frames and multipass as memory consumers — [NVENC Programming Guide 13.0](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/nvenc-video-encoder-api-prog-guide/index.html)
- Reference frames: OBS sets only AV1 `numFwdRefs = 1` and `numBwdRefs = 1`; it leaves H.264/HEVC reference counts at the preset default — [OBS nvenc.c](https://github.com/obsproject/obs-studio/blob/master/plugins/obs-nvenc/nvenc.c)

### Inferences
- With "encode only changed frames", any B-frame or lookahead delay holds the last N frames inside the encoder until more input arrives. On a static screen those frames stay unflushed, so a clip saved at that moment would be missing its last frames. Fixes: send an EOS/flush before muxing a saved clip, or keep feeding a frame at a minimum rate (for example, a repeat of the last frame every 1/30 s or at the IDR point). The app does not hit this today because B-frames are off; it becomes relevant if they are enabled.
- Under CONSTQP, two-pass rate control and lookahead-driven rate control have nothing to allocate. Lookahead would still drive adaptive I/B placement, but the main benefit of both features is in bitrate-targeted modes. For CQ or CQP recording at a generous cap, they are low-value features that still cost CUDA time and latency.
- B-frames are the cheapest quality-per-bit gain at a fixed QP: no performance cost per NVIDIA, about 1 + N_B extra surfaces, and a few frames of latency. 2-3 B-frames is a reasonable default. With H.264, keep B-frame references off (as OBS does) for decoder safety; with HEVC/AV1, use middle B-frame references.
- Spatial AQ moves bits from flat to busy regions, which helps the gradients and dark areas common in games; it lowers PSNR by design. OBS's strength of 8 is a reasonable middle value; no tuning data was found.

### Gaps
- No measured quality gain (VMAF/BD-rate) found for quarter-res vs full-res multipass, temporal AQ, aqStrength values or reference-frame count on Ada or game content.
- No measured 1080p fps cost of AQ, lookahead or multipass on Ada.
- Whether temporal AQ requires lookahead in the 12.x API was not confirmed.

---

## 4. Rate control for recording: CQP vs VBR+CQ vs constant quality; CQ/QP for visually lossless 1080p; OBS and ShadowPlay comparison

### Takeaway
For recording, NVIDIA recommends HQ or UHQ tuning, VBR, a large VBV (about 4 s), lookahead, B-frame references, a finite GOP of about 2 s, and AQ. OBS's quality-based options are CQP (default 20) and "Target Quality" (VBR+CQ, default 20, optional max-bitrate cap). Metric data for H.264 NVENC puts VMAF 99.5 at about CQ 21.5, and a careful tester preferred CQ 14-20 visually. The app's VBR+CQ 19 with a 100 Mbps cap is already in the "visually lossless" range; CQ 16-18 is the conservative choice for fast game motion. ShadowPlay's data is weak, but suggests 1080p60 tops out around 50 Mbps in GeForce Experience.

### Cited Findings
- NVIDIA's recommended settings for recording and archiving: High or Ultra High Quality tuning, VBR, a large VBV buffer (about 4 seconds), lookahead, B-frames as reference, a finite GOP (about 2 seconds) and AQ. Game-casting is similar but uses CBR with about 1 s VBV — [NVENC Programming Guide 13.0, section 9](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/nvenc-video-encoder-api-prog-guide/index.html)
- Target-quality mode is VBR-based, with a range of 0-51 (H.264/HEVC) and 0-63 (AV1); fractional values have been supported since SDK 8.0. Setting the max bitrate too low can stop the target quality from being reached — [NVENC Programming Guide 13.0](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/nvenc-video-encoder-api-prog-guide/index.html)
- OBS NVENC defaults: rate control CBR at 10000 Kbps, CQP 20, target quality 20, preset p5, tune hq, multipass qres, AQ on and 2 B-frames. CQP ranges 1-51 (H.264/HEVC) and 1-63 (AV1) — [OBS nvenc-properties.c](https://github.com/obsproject/obs-studio/blob/master/plugins/obs-nvenc/nvenc-properties.c)
- OBS CQP applies the same QP to I, P and B frames. For AV1 it multiplies the user value by 4, because "AV1 uses 0-255 instead of 0-51 for QP". OBS's CQ+VBR mode sets `averageBitRate = 0`, `maxBitRate = max_bitrate`, `targetQuality = value` and `vbvBufferSize = 0` (driver default) — [OBS nvenc.c](https://github.com/obsproject/obs-studio/blob/master/plugins/obs-nvenc/nvenc.c); [OBS nvenc-properties.c](https://github.com/obsproject/obs-studio/blob/master/plugins/obs-nvenc/nvenc-properties.c)
- OBS docs: "VBR with Target Quality" works "like CQP" but adds an optional maximum bitrate — [OBS KB](https://obsproject.com/kb/advanced-nvenc-options)
- H.264 NVENC CQ sweep (older data: GTX 1070 Ti, HQ, VBR+CQ, AQ, 3 B-frames with B-ref):
  - PSNR is about 45 dB at CQ 17.5; SSIM 0.99 at CQ 17.5; VMAF 99 at CQ 24.5 and 99.5 at CQ 21.5.
  - The worst-frame tail-off starts around CQ 24.
  - The metrics suggest CQ 17 (26.1 Mbps) to CQ 24 (10.5 Mbps). On images, the author prefers CQ 14 (37.5 Mbps) to CQ 20 (17.6 Mbps).
  - Grain and noise detail is lost even at the bitrate cap (CQ 12).

  Source: [Gough's Tech Zone](https://goughlui.com/?p=48157)
- Community advice favors CQP for local recording, since CBR "is only really used for streaming". Reported CQP values vary (one user says the OBS default suggests 25; another at CQ 20 was told to go lower) — [OBS forum](https://obsproject.com/forum/threads/which-obs-setting-give-the-best-recording-quality-to-me.120279/post-451833) (snippet, community)
- ShadowPlay (GeForce Experience era): custom bitrate of 10-50 Mbps in 5 Mbps steps; 1080p60 commonly at 50 Mbps — [Techgage](https://techgage.com/news/nvidia-adds-shadowplay-fine-tuning-options-in-latest-geforce-experience-release/) (snippet, older data); [GamerHardware guide](https://gamerhardware.org/nvidia-shadowplay-recording-guide/) (snippet, low-quality source). ShadowPlay has moved to the NVIDIA App, which added 120 fps capture, and its current limits may differ — [Yahoo/PC Gamer report](https://malaysia.news.yahoo.com/shadowplay-now-capture-games-120-141725194.html) (snippet)
- Bitrate caps remain hard limits in NVENC VBR modes ("the bitrate field sets an upper limit") — [OBS forum: NVENC HEVC VBR settings](https://obsproject.com/forum/threads/nvenc-hevc-vbr-settings.151747/) (snippet, community)

### Inferences
- VBR+CQ with a generous cap is the right recording mode. It is CQP-like, but the cap bounds the worst case for the RAM buffer. For a 1080p game recording, CQ 16-18 (H.264) gives margin over CQ 19 for fast motion and foliage, at the cost of a larger buffer. At a 100 Mbps cap, the worst case is about 750 MB per minute.
- Pure CONSTQP gives a slightly more predictable quality per frame type but has no cap; NVIDIA's own recording guidance prefers VBR. If using CQP directly, a lower I-frame QP (for example 2 below P) is a common practice, but no source here measures it. (OBS uses equal QPs.)
- ShadowPlay at 50 Mbps for 1080p60 corresponds to about 120 Mbps at 1080p144 when scaled linearly per frame. The app's VBR+CQ 19 capped at 100 Mbps is therefore at or above ShadowPlay quality in bits per frame (scaling assumption, not measured).
- For HEVC or AV1 at equal visual quality, the same CQ number does not map 1:1 across codecs; AV1 also uses a different scale (0-63 target quality, 0-255 internal QP). Recalibrate by file size or VMAF if the codec changes.

### Gaps
- No Ada-measured CQ-to-VMAF curve on game content at 1080p for any codec.
- No authoritative "visually lossless" QP for HEVC or AV1 NVENC was found.
- Current NVIDIA App ShadowPlay bitrate limits and quality presets (2025-2026) were not verified from a primary source.

---

## 5. 10-bit, 4:4:4 chroma and full vs limited range: quality effect and compatibility

### Takeaway
On Ada, 10-bit output exists for HEVC Main10 and AV1 Main, but not H.264 (H.264 High10 is Blackwell-only). Encoding 8-bit content as 10-bit gives about a 3-4% efficiency gain at negligible cost in NVIDIA's data (HEVC uses CUDA for the conversion on Ada). HEVC 4:4:4 is supported, H.264 4:4:4 is CAVLC-only, and AV1 4:4:4 is not supported on NVENC. On this Windows 10 LTSC PC, the bigger compatibility issue is that the Store-delivered HEVC and AV1 Video Extensions are missing, so only H.264 plays in the built-in players.

### Cited Findings
- Codec support (Application Note 13.0):
  - H.264 High10 is N through Ada and Y on Blackwell; H.264 4:2:2 likewise; H.264 4:4:4 (CAVLC only) is Y on all generations.
  - HEVC Main10 is Y from Pascal on; HEVC 4:4:4 is Y from Pascal on; HEVC 4:2:2 is Blackwell-only.
  - AV1 Main is "YUV 420 8-bit and 10-bit" on Ada and Blackwell; AV1 4:2:2 and 4:4:4 are not listed.

  Source: [NVENC Application Note 13.0](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/nvenc-application-note/index.html)
- 8-to-10-bit encoding of 8-bit input (HEVC and AV1, `inputBitDepth = 8`, `outputBitDepth = 10`, SDK 12.2+): 3-4% efficiency gains, "negligible" performance cost, higher GPU memory use. HEVC 8-to-10-bit conversion is done "using CUDA" — [NVIDIA 12.2 blog](https://developer.nvidia.com/blog/improving-video-quality-with-nvidia-video-codec-sdk-12-2-for-hevc/); [NVENC Programming Guide 13.0](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/nvenc-video-encoder-api-prog-guide/index.html)
- OBS handles 10-bit by feeding P010 (`NV_ENC_BUFFER_FORMAT_YUV420_10BIT`) and setting `pixelBitDepthMinus8 = 2` (before SDK 12.2) or `inputBitDepth` (12.2+). It forces H.264 high10 for P010, which is only valid with NVENC 13.0+ on Blackwell — [OBS nvenc.c](https://github.com/obsproject/obs-studio/blob/master/plugins/obs-nvenc/nvenc.c); [OBS nvenc-properties.c](https://github.com/obsproject/obs-studio/blob/master/plugins/obs-nvenc/nvenc-properties.c)
- A developer reports that ARGB input can produce YUV 4:4:4 output with HEVC when `chromaFormatIDC = 3` — [NVIDIA developer forum: YUV444 from ARGB](https://forums.developer.nvidia.com/t/nvenc-yuv444-from-argb-format/189415) (snippet, user report)
- Range signaling: OBS writes `videoFullRangeFlag` (HEVC/H.264) and `colorRange` (AV1) from the source range, and writes VUI primaries/transfer/matrix = 1/1/1 for BT.709 or 6/6/6 for BT.601 — [OBS nvenc.c](https://github.com/obsproject/obs-studio/blob/master/plugins/obs-nvenc/nvenc.c)
- Windows 10 LTSC has no Microsoft Store, so the Store-delivered HEVC Video Extensions cannot be installed the normal way — [Microsoft Q&A](https://learn.microsoft.com/en-us/answers/a/1180572) (snippet). AV1 hardware decoding on Windows also needs the AV1 Video Extension from the Store — [Mozilla Bugzilla 1764200](https://bugzilla.mozilla.org/show_bug.cgi?id=1764200) (snippet). Microsoft moved HEVC out of Windows into the Store in the Fall Creators Update — [gHacks](https://www.ghacks.net/?p=139355) (snippet, older data)

### Inferences
- The reported HEVC playback failure on this PC is a missing-decoder-extension problem, not an encoder problem. AV1 will hit the same wall in Windows Media Player and Movies & TV. VLC or mpv bundle their own decoders and play both. If the clips are mainly shared to Discord, YouTube or phones, HEVC and AV1 are widely decodable elsewhere, but that was not verified here.
- 10-bit output reduces banding in gradients (skies, fog, dark scenes) mainly because the encoder's internal precision is higher. NVIDIA's 3-4% efficiency figure is the only number found. 10-bit HEVC/AV1 is low-risk on modern hardware decoders, but it is not available for H.264 on Ada.
- 4:4:4 roughly doubles chroma data, and hardware decoder support for HEVC Range Extensions and H.264 High 4:4:4 is far less common than for 4:2:0. It mainly helps text and UI (desktop recording), not games. For a broadly playable game replay buffer, 4:2:0 is the safe choice.
- Full-range YUV is a frequent cause of crushed blacks or washed-out output in players and editors that ignore the range flag. Limited range with BT.709 tagging is what most players assume for HD content. This is common knowledge, not sourced in this research; verify before relying on it.

### Gaps
- No measured banding or VMAF data for 10-bit vs 8-bit encoding of 8-bit game content beyond NVIDIA's 3-4% BD-rate figure.
- No sourced compatibility matrix for which players or hardware decoders handle HEVC 4:4:4, H.264 4:4:4 or full-range flags.
- No confirmed non-Store installation route for the HEVC or AV1 extensions on LTSC 2021.

---

## 6. ARGB input: who converts to YUV, which color matrix, and whether to convert yourself

### Takeaway
NVENC accepts ARGB and converts it to YUV internally, and NVIDIA lists "RGB content encoding" among the features that use CUDA. FFmpeg developers found that this internal converter ignores color tags and produces limited-range BT.601 (BT.470BG) YUV from sRGB input. If the stream is tagged BT.709 or left unspecified, HD players will decode it with BT.709 and show a slight hue shift. Converting ARGB to NV12/P010 yourself (compute shader or D3D11 VideoProcessor) with BT.709 limited range, and tagging the VUI 1/1/1, gives correct colors, enables 10-bit, and is the documented safe route.

### Cited Findings
- The programming guide lists "RGB content encoding" among the encoder features that use CUDA — [NVENC Programming Guide 13.0](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/nvenc-video-encoder-api-prog-guide/index.html)
- An FFmpeg commit that hardcodes color characteristics for NVENC's RGB input says the encoder ignores the given color tags for its internal RGB-to-YUV conversion. Testing indicates the input is treated as sRGB and the output is limited-range BT.470 (BT.601 matrix). The patch tags such output as BT.470BG. It is on a fork, and upstream merge was not confirmed — [FFmpeg commit (Traneptora fork)](https://code.ffmpeg.org/Traneptora/FFmpeg/commit/b71069879010426ef8d7dce614086f8fb90eb0f3)
- FFmpeg trac #11541, "All NVENC encoders change bt709 color space to bt470bg when pixel format is not specified": the workaround is to set the pixel format explicitly or add a format filter, so FFmpeg converts before NVENC — [ffmpeg-trac mailing list, #11541](https://lists.ffmpeg.org/archives/list/ffmpeg-trac@ffmpeg.org/thread/FX4W5NEWSALFJS34ISU675BWG2IPB6WO/) (snippet; [ticket page](https://trac.ffmpeg.org/ticket/11541) could not be fetched)
- A different user claims that the HEVC VUI color parameters (`hevcVUIParameters`) control NVENC's ARGB-to-YUV matrix. This contradicts the FFmpeg finding that tags are ignored — [NVIDIA developer forum](https://forums.developer.nvidia.com/t/nvenc-yuv444-from-argb-format/189415) (snippet, user report)
- A VideoHelp user converting with `scale=out_color_matrix=bt709` into `hevc_nvenc` reports colors within about ±2 RGB values, attributed to 8-bit rounding — [VideoHelp forum](https://forum.videohelp.com/showthread.php?p=2605603) (snippet)
- OBS never passes RGB to NVENC: it converts on the GPU itself and feeds NV12 or P010 textures (`NV_ENC_BUFFER_FORMAT_NV12` / `YUV420_10BIT`), with VUI set from its own color space (709 = 1/1/1) and range — [OBS nvenc.c](https://github.com/obsproject/obs-studio/blob/master/plugins/obs-nvenc/nvenc.c)
- NVIDIA recommends that the application "implement the full (or the majority) video pipeline" on the GPU to avoid PCIe transfers — [NVENC Programming Guide 13.0](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/nvenc-video-encoder-api-prog-guide/index.html)

### Inferences
- Current app risk: ARGB passed straight in probably yields BT.601 limited-range YUV. Unless the VUI says matrix 6 (BT.601 / SMPTE 170M), players decoding 1080p as BT.709 will shift colors slightly (reds and greens are most visible). This is fixable either way: tag 6/6/6 (or 1/1/6, since the primaries and transfer are still BT.709/sRGB) to match NVENC's conversion, or convert yourself to BT.709 and tag 1/1/1. Check the current output's tags with ffprobe/MediaInfo and compare a color chart.
- Converting yourself costs a small compute pass (an RGB read and NV12 write at 1080p is about 8 MB plus 3 MB per frame), comparable to NVENC's own CUDA conversion. It lets you choose BT.709 limited range, enable P010 10-bit for HEVC/AV1, and use NV12 for the encoder's reference path. Both the D3D11 VideoProcessor (fixed-function, BT.709 configurable via the color-space API) and a compute shader are standard options. Neither was benchmarked in sources found here.
- "Players expect": for SDR HD content, the de facto expectation is BT.709 primaries, transfer and matrix in limited (16-235) range. That is also what OBS emits by default for 709. Unspecified tags at 1080p generally get BT.709 treatment (unsourced convention).

### Gaps
- No official NVIDIA statement found on the exact matrix or range of NVENC's internal ARGB conversion, or on whether it honors the VUI; the evidence is FFmpeg developer testing vs a forum user report.
- No measurement of CUDA/SM time for NVENC's internal RGB conversion vs a D3D11 VideoProcessor or compute-shader conversion at 1080p144.

---

## 7. Variable frame rate: does NVENC handle irregular frame timing, and how to pass timestamps?

### Takeaway
NVENC has no notion of real time. `frameRateNum/Den` only seeds rate-control budgets, and `inputTimeStamp` is an opaque, monotonically increasing value echoed back on output. VFR is therefore handled by the application and muxer: pass the capture timestamps through, write them into the container, and use a quality-based rate-control mode, so the per-frame bit budget doesn't depend on the nominal frame rate.

### Cited Findings
- `inputTimeStamp` is a unique per-frame identifier; any monotonically increasing value can be used — [NVENC Programming Guide 13.0](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/nvenc-video-encoder-api-prog-guide/index.html)
- NVIDIA developer forum thread: the frame-rate fields are used to estimate average input frame rate for rate control, and "The frame rate has no meaning in NVENC other than deciding rate control parameters". Changing it mid-stream was not supported through the API in that thread — [NVIDIA developer forum: dynamic frame rate](https://forums.developer.nvidia.com/t/dynamic-changing-of-frame-rate-in-nvenc/53068) (snippet; older data)
- OBS sets `frameRateNum/Den` from the output fps and passes the frame's pts as both `inputTimeStamp` and `frameIdx`. It reads `outputTimeStamp` back as packet pts and offsets DTS by the B-frame count (except AV1) — [OBS nvenc.c](https://github.com/obsproject/obs-studio/blob/master/plugins/obs-nvenc/nvenc.c)
- In an Avidemux report on NVIDIA HEVC, a timebase not matching 1/fps disrupted rate control on a VFR clip ("timebase != 1/fps messes up rate control") — [Avidemux forum](https://avidemux.org/smif/index.php?msg=90327) (snippet, user report)

### Inferences
- In VBR with an average-bitrate target, NVENC budgets about bitrate/fps per frame. If fewer frames arrive than `frameRateNum` declares, the actual bitrate falls short (static desktop), and if more arrive, the rate overshoots. With constant-quality modes (CONSTQP, or VBR+targetQuality with averageBitRate = 0 and only a max cap), quality is set per frame by QP, so irregular timing is mostly harmless. That makes the app's current mode a good fit for VFR. Set `frameRateNum` to the maximum expected rate (144) so the max-bitrate/VBV per-frame budget is not over-generous.
- Pass the capture time (for example QPC converted to 100 ns or 1/90000 units) as `inputTimeStamp`, read `outputTimeStamp`, and write those times as container PTS. With B-frames, also generate DTS (OBS shifts DTS by the B-frame count). Keep timestamps strictly increasing; duplicate or backwards timestamps break muxers more than NVENC.
- The forced IDR "every 2 s" should key off capture time, not frame count, under VFR. NVIDIA's recording guidance recommends a finite GOP of about 2 s ([NVENC Programming Guide 13.0](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/nvenc-video-encoder-api-prog-guide/index.html)), and the replay buffer needs those keyframes as cut points.
- Temporal AQ and lookahead assume roughly regular motion between frames. Long gaps between changed frames (VFR) make their temporal statistics less meaningful (unverified).

### Gaps
- No NVIDIA documentation found that describes rate-control behavior with irregular input timing explicitly, or whether `NvEncReconfigureEncoder` can change the frame rate on current drivers.
- No measurements of NVENC quality or bitrate accuracy under VFR input.

---

## 8. What OBS Studio, NVIDIA and the Video Codec SDK recommend for high-quality recording

### Takeaway
NVIDIA (SDK section 9) recommends for recording: HQ or UHQ tuning, VBR, about 4 s VBV, lookahead, B-frame references, about 2 s GOP and AQ. OBS ships P5 + HQ + qres multipass + AQ (strength 8 plus temporal AQ) + 2 B-frames, with CQP or target quality defaulting to 20. OBS explicitly discourages UHQ for gaming. Community advice converges on CQP or target quality, P5-P6, and dropping CUDA-heavy extras if the encoder overloads.

### Cited Findings
- NVIDIA use-case recommendations (Programming Guide section 9):
  - Recording/archiving: HQ or UHQ tuning, VBR, large VBV (about 4 s), lookahead, B-frames as reference, finite GOP (about 2 s), AQ.
  - Game-casting: the same but CBR with about 1 s VBV.
  - Low latency: ULL/LL tuning and CBR, plus multi-pass, a small VBV, unidirectional B-frames, an infinite GOP and intra refresh.
  - For DirectX apps sharing the device with screen capture: `enableEncodeAsync = 1`, `doNotWait = 0`, `enableOutputInVidmem = 0`, and keep blocking calls off the main thread.

  Source: [NVENC Programming Guide 13.0](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/nvenc-video-encoder-api-prog-guide/index.html)
- Tuning-info mapping (section 3.3): high quality for archiving and OTT; low latency with CBR for streaming and conferencing — [NVENC Programming Guide 13.0](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/nvenc-video-encoder-api-prog-guide/index.html)
- OBS defaults:
  - preset p5, tune hq, multipass qres, lookahead per device capability (depth 8), AQ on (aqStrength 8 plus temporal AQ where supported), 2 B-frames, CQP 20, target quality 20.
  - Profile high (H.264) or main (HEVC/AV1); GOP 250 frames if keyint is 0.
  - UHQ is offered only if the GPU supports it, and B-frame references only for HEVC/AV1.

  Sources: [OBS nvenc-properties.c](https://github.com/obsproject/obs-studio/blob/master/plugins/obs-nvenc/nvenc-properties.c); [OBS nvenc.c](https://github.com/obsproject/obs-studio/blob/master/plugins/obs-nvenc/nvenc.c)
- OBS KB: UHQ is "optimised for live-action with artifacts such as camera sensor noise", not recommended for gaming, and "Significantly reduces throughput". Split encode allows p7 at higher resolutions in real time on multi-NVENC Ada GPUs — [OBS KB](https://obsproject.com/kb/advanced-nvenc-options)
- Gough's metric and visual testing of H.264 NVENC: CQ 14-20 is preferred visually (older data) — [Gough's Tech Zone](https://goughlui.com/?p=48157)

### Inferences
- NVIDIA's "lookahead" and "UHQ" recommendations for recording assume a dedicated or latency-tolerant encode (archiving or transcoding), not a GPU that is also rendering a game. OBS's guidance (no UHQ for games) is the better fit for "negligible game impact".
- NVIDIA's "VBR + large VBV" maps to the app's VBR + target quality + max cap; a 4 s VBV at a 100 Mbps cap would be about 400 Mbit.

### Gaps
- NVIDIA's consumer "NVIDIA NVENC OBS Guide" page was not retrieved, so its current recommended recording values (CQP number, preset) are not captured here.
- No official NVIDIA recommendation of a specific CQ/QP number for recording was found.

---

## 9. Recommended settings for "best quality at negligible game impact" (RTX 4060 Ti, 1080p up to 144 fps, replay buffer)

### Takeaway
Keep the encode core busy rather than the CUDA cores. Use a quality-targeted VBR with a cap, P5-P6 (P7 only for AV1), HQ tuning (never UHQ), spatial AQ, and 2-3 B-frames. Convert ARGB to NV12/P010 yourself with BT.709 limited range and tag it. Pick H.264 for playback on this LTSC PC, or HEVC Main10/AV1 if clips are played with VLC/mpv or elsewhere and smaller buffers matter.

### Cited Findings
- Throughput headroom at 1080p (per engine, RTX 4090 clocks): H.264 P5 283 / P7 211 fps; HEVC P5 411 / P7 181; AV1 P5 440 / P7 323 — [NVENC Application Note 13.0](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/nvenc-application-note/index.html)
- CUDA-using features: two-pass rate control, lookahead, all AQ modes, weighted prediction, RGB input and temporal filter — [NVENC Programming Guide 13.0](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/nvenc-video-encoder-api-prog-guide/index.html). UHQ uses 13-18% of SM time vs 1-2% for HQ, with about 40% more board power on Ada — [arXiv 2605.01187 summary](https://www.emergentmind.com/papers/2605.01187)
- B-frame references add quality at no performance cost (HEVC/AV1; OBS avoids it for H.264 decoders) — [NVENC Programming Guide 13.0](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.0/nvenc-video-encoder-api-prog-guide/index.html); [OBS nvenc-properties.c](https://github.com/obsproject/obs-studio/blob/master/plugins/obs-nvenc/nvenc-properties.c)
- 8-to-10-bit gives 3-4% efficiency at negligible cost (HEVC/AV1, SDK 12.2+) — [NVIDIA 12.2 blog](https://developer.nvidia.com/blog/improving-video-quality-with-nvidia-video-codec-sdk-12-2-for-hevc/)
- H.264 NVENC is near VMAF 99.5 by about CQ 21.5; visual preference is CQ 14-20 — [Gough's Tech Zone](https://goughlui.com/?p=48157)

### Inferences (recommended configuration; synthesized from the findings above, not measured on a 4060 Ti)

**Profile A: H.264 (default for this LTSC PC, plays in Windows Media Player)**
- `NV_ENC_CODEC_H264_GUID`, High profile, preset **P5** (or **P6** if a test shows NVENC load stays below about 70% at a constant 144 fps; P7 only if measured OK). Tuning **HIGH_QUALITY**.
- Rate control: **VBR, `averageBitRate = 0`, `targetQuality` = 17-18** (fractional allowed), **`maxBitRate` = 100-130 Mbps**, `vbvBufferSize` left at default or about 1-4 s of max rate. CONSTQP at QP 17-18 (I-frames about 2 lower) is the alternative if no cap is wanted.
- `multiPass` = **DISABLED** (or QUARTER_RESOLUTION; it matters little in quality-targeted mode and it uses CUDA).
- **Spatial AQ on, `aqStrength` about 8** (the OBS value). Temporal AQ is optional; leave it off for minimum CUDA use and VFR friendliness.
- **B-frames: `frameIntervalP = 3` (2 B-frames), `useBFramesAsRef = DISABLED`** for H.264 decoder safety. Requires flushing before a clip save (VFR + B-frame hold-back).
- Lookahead **off** (avoids CUDA, latency and VFR hold-back; small benefit with quality-targeted rate control).
- GOP: `gopLength` = infinite or large, with **forced IDR every 2 s of capture time** (as now); repeat SPS/PPS on IDR so any 2 s segment can be cut.
- Input: **convert ARGB to NV12 yourself (BT.709, limited range)**; VUI `colourPrimaries = 1`, `transferCharacteristics = 1`, `colourMatrix = 1`, `videoFullRangeFlag = 0`, `videoSignalTypePresentFlag`/`colourDescriptionPresentFlag = 1`. If you keep ARGB input, tag the matrix as BT.601 (6) to match NVENC's internal conversion.
- Timestamps: capture-time `inputTimeStamp`, `frameRateNum/Den = 144/1`, PTS/DTS written by the muxer.

**Profile B: HEVC Main10 (best quality per byte with mature decoding; needs VLC/mpv or the HEVC extension on this PC)**
- `NV_ENC_CODEC_HEVC_GUID`, **Main10 from 8-bit input** (SDK 12.2+ `inputBitDepth = 8`, `outputBitDepth = 10`, or feed P010 from your own conversion), preset **P5** (HEVC P7 at 181 fps is too close to 144), tuning **HQ**.
- VBR + `targetQuality` about 19-21 with the same cap; spatial AQ (strength about 8); **3 B-frames with `useBFramesAsRef = MIDDLE`**; lookahead off; IDR every 2 s; BT.709 limited tagging.

**Profile C: AV1 (smallest files; Ada AV1 P7 is still 323 fps; not playable in WMP on LTSC without the Store extension)**
- `NV_ENC_CODEC_AV1_GUID`, Main profile, 10-bit output, preset **P6-P7**, tuning **HQ**, VBR + `targetQuality` (0-63 scale; calibrate by size or VMAF), 2-3 B-frames with middle references, spatial AQ, lookahead off, IDR every 2 s, BT.709 limited range (`colorRange = 0`).

**Avoid for "negligible game impact"**: UHQ tuning, the temporal filter (`tfLevel`), lookahead level 3, full-resolution multipass, 4:4:4, and full-range output.

**Validation steps the app owner should run** (to fill the gaps):
1. Measure NVENC utilization (`nvidia-smi dmon -s u`, enc %) and game fps at a constant 144 fps for P5/P6/P7 per codec on the 4060 Ti.
2. Run VMAF on a recorded game clip (a 4:4:4 or lossless reference capture vs the encodes) at CQ 16/18/20.
3. Check the ARGB-path color tags and a color chart, against a self-converted NV12 BT.709 encode.

### Gaps
- None of these profiles has been measured on an RTX 4060 Ti with game load; all of the numbers above are extrapolated from RTX 4090 throughput tables, older-GPU quality curves and vendor or academic BD-rate claims.
- UHQ, `tfLevel`, unidirectional B-frames and 8-to-10-bit `inputBitDepth`/`outputBitDepth` require upgrading the SDK header from 12.1 to 12.2+ (and a matching driver). The minimum driver versions for SDK 12.2/13.x were not retrieved in this research.

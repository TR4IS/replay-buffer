# Capture methods for an always-on, anti-cheat-safe replay buffer (Windows 10 19044, RTX 4060 Ti, 3x1080p, up to 144 fps)

Research date: 2026-11-06. Items marked "older" predate 2022. "Local check" means a check I ran on the target PC itself (Windows build 19044, NVIDIA driver file version 32.0.16.2101) during this research.

## 1. How do the major recorders capture frames?

### Takeaway
Only OBS publishes its method: Game Capture injects a signed hook DLL into the game and shares the game's back buffer as a GPU texture, and Display Capture uses DXGI Desktop Duplication (DDA). Sunshine on Windows uses DDA, or WGC as a beta. NVIDIA, AMD, Valve, Microsoft and Medal do not document their capture paths. What is known about ShadowPlay (NvFBC for the full screen, NvIFR for single windows) is 2013–2019-era information.

### Cited Findings
**OBS Studio**
- Game Capture injects a hook DLL. `inject_hook()` takes one of two paths. In the "direct" path, `hook_direct()` opens the game with `PROCESS_ALL_ACCESS` and calls `inject_library_obf()`. In the "helper" path, OBS launches `inject-helper64.exe`/`inject-helper32.exe`. With the "anti-cheat compatibility hook" on (`anti_cheat_hook`, default **true**), OBS passes the helper the game's **thread id**. With it off, it passes the process id. The log line reads `"using helper (compatibility hook)"` or `"(direct hook)"`. — [obs-studio game-capture.c](https://github.com/obsproject/obs-studio/blob/master/plugins/win-capture/game-capture.c)
- Frames move from the hooked game to OBS as a shared GPU texture (`shtex`, opened with `gs_texture_open_shared(tex_handle)`, no CPU copy). The fallback is shared memory (`shmem`, double-buffered CPU copy). OBS forces shmem when "SLI compatibility" is on or when shared textures are unavailable. — [game-capture.c](https://github.com/obsproject/obs-studio/blob/master/plugins/win-capture/game-capture.c)
- OBS has a built-in blacklist that it refuses to hook (explorer, steam, chrome, discord, firefox, cmd, taskmgr…). Its hook-failure warning says failure "is most likely due to security software". — [game-capture.c](https://github.com/obsproject/obs-studio/blob/master/plugins/win-capture/game-capture.c)
- Hook retry timing: a 2 s base retry interval and a 4 s interval after errors. The "hook rate" setting multiplies these (slow 2.0, normal 1.0, fast 0.5, fastest 0.1). — [game-capture.c](https://github.com/obsproject/obs-studio/blob/master/plugins/win-capture/game-capture.c)
- Display Capture ("DXGI" method) uses DDA from a `video_tick` callback. `AcquireNextFrame`'s timeout "is a max wait time, not a frame rate limiter". DDA "is basically tied to the Windows compositor (DWM), which runs at your monitor refresh rate". This is a community answer that cites OBS source files, not a maintainer statement. — [OBS discussion #12058](https://github.com/obsproject/obs-studio/discussions/12058)

**Sunshine (Moonlight host)**
- Capture backends: `nvfbc` is **Linux only** ("captures directly to GPU memory… usually the fastest option on NVIDIA"). On Windows the choices are `ddx` (DXGI Desktop Duplication, "well-supported") and `wgc` (Windows.Graphics.Capture, **beta**, "not compatible with the Sunshine service"). When the screen is static, Sunshine re-sends duplicate frames down to `minimum_fps_target` (default: half the stream fps). — [Sunshine configuration docs](https://docs.lizardbyte.dev/projects/sunshine/latest/md_docs_2configuration.html)

**NVIDIA ShadowPlay / NVIDIA App Instant Replay**
- (Older) ShadowPlay supported two methods: NVFBC (frame buffer capture, used in full screen) and NVIFR (in-band frame readback, which captures a single window). It required a GTX 600 or newer. — [Wikipedia: Nvidia ShadowPlay/Share](https://en.wikipedia.com/wiki/Nvidia_Share)
- Local check: `NvFBC64.dll` and `NvIFR64.dll` are still installed in `C:\Windows\System32` by the current driver on the target PC, so the driver still ships both libraries. — Local check (Test-Path, 2026-11-06)
- In the current NVIDIA App the Alt+Z overlay has a "Desktop capture" toggle. With it off, Instant Replay records only games. With it on, Instant Replay can switch itself off when DRM-protected content is open. These come from third-party troubleshooting guides, not NVIDIA docs. — [BGR](https://www.bgr.com/2222483/5-solutions-nvidia-instant-replay-issues/), [TheWindowsClub](https://www.thewindowsclub.com/nvidia-instant-replay-stopped-working-in-windows-11)
- Forum users disagree on whether GeForce Experience and Steam use NvFBC on consumer cards. An NVIDIA reply in the same forum says NvFBC is a licensed product and not supported on GeForce for all users. — [NVIDIA dev forum](https://forums.developer.nvidia.com/t/nvfbc-on-geforce/54460), [OBS forum NVFBC request](https://obsproject.com/forum/threads/feature-request-nvfbc-api-capture-support.81703/)

**Steam Game Recording (2024)**
- It is built into the Steam overlay and records the game window only, not the rest of the desktop. — [Engadget](https://www.engadget.com/steam-deck-gets-a-built-in-game-recorder-120019196.html), [PC Gamer](https://pcgamer.com/software/platforms/steam-game-recording-is-now-available-for-everyone-and-its-packed-with-neat-features)
- Steamworks says "Your game does not need to do anything special for the Game Recording to work". The overlay is automatically disabled for "Software" app types. The docs do not describe the capture mechanism. — [Steamworks Timeline docs](https://partner.steamgames.com/doc/features/timeline)
- Steam has an NvFBC code path (presumably for Remote Play): the nvfbcwrp patch notes that Steam additionally needs the `NVFBCEnable` DWORD set under the nvlddmkm service key. — [keylase/nvidia-patch issue #359](https://github.com/keylase/nvidia-patch/issues/359)

**Overwolf Outplayed**
- Overwolf describes it as "OBS based and hardware accelerated", with "4 different recording modes that you can choose from, per game". The page says nothing about anti-cheat. — [Overwolf Outplayed capture page](https://go.overwolf.com/outplayed-tv/capture.html)

**Medal.tv**
- Medal's own comparison page does not describe its capture stack. I found no primary source saying whether it uses libobs or OBS's hook. — [Medal vs OBS](https://medal.tv/compare/medal-vs-obs)

**Xbox Game Bar / Game DVR**
- Recording runs in the Broadcast DVR server process (`bcastdvr.exe`) and is controlled by `HKCU\...\GameDVR\AppCaptureEnabled`. Users report it cannot record some full-screen games and costs frame rate on slow PCs. — [WindowsReport](https://windowsreport.com/?p=90503), [WindowsReport (greyed out)](https://windowsreport.com/xbox-game-bar-record-greyed-out/)

**Other DDA-based tools**
- A 2025–26 open-source monitor mirror calls DXGI Desktop Duplication the "default, lowest overhead" monitor-capture option and uses it in FP16 for HDR. It describes WGC as "works across GPUs, cursor drawn by Windows". — [ritschy-mirror README](https://github.com/gloomycoder/ritschy-mirror)

### Inferences
- In practice there are four capture families: (a) in-process swapchain hook (OBS Game Capture, likely Outplayed, likely Steam's overlay, older ShadowPlay NvIFR), (b) OS desktop duplication (DDA: OBS Display Capture, Sunshine ddx), (c) WGC (OBS Window/Display Capture "Windows 10 1903+" mode, Sunshine wgc), and (d) vendor driver-level framebuffer capture (NvFBC, AMD's equivalent). Commercial tools mix these and fall back between them.
- Steam's recording being overlay-based and window-only strongly suggests it reuses the overlay's existing Present hook (`gameoverlayrenderer`). This is speculation; Valve does not document it.

### Gaps
- AMD ReLive/Adrenalin capture path: not researched; no primary source found.
- Medal.tv internals: no source found. Inspecting the installed app's DLLs (for example for `obs`/`graphics-hook` files) would settle it.
- What the modern NVIDIA App uses on Windows 10 (still NvFBC/NvIFR, or DDA plus an in-game hook): NVIDIA does not document it. A forum search for an NVIDIA DLL injected into games (`nvspcap64.dll`) found nothing; that DLL is not in System32 on the target PC (local check).
- Xbox Game Bar's internal capture API (WGC vs DWM-internal): no primary source.

## 2. NvFBC: usable on GeForce today, and its advantages over DDA

### Takeaway
On Windows, NvFBC is effectively dead for this project. NVIDIA deprecated it on Windows 10 (last supported build 1803/17134), it is not licensed for GeForce, and the only way to enable it is a third-party wrapper that replaces driver DLLs in System32. That wrapper is a poor idea next to Vanguard/Ricochet. The modern Capture SDK (9.0) targets Linux.

### Cited Findings
- "NVFBC has been deprecated on Windows 10 and above… Last supported Windows 10 version is 1803, build 17134." NVIDIA points developers to native Windows 10 capture APIs instead. — [NVIDIA Capture SDK](https://developer.nvidia.com/capture-sdk)
- The archived release notes give the deprecation as starting with the Windows 10 October 2019 Update. That differs slightly from the "1803" wording above, so the two NVIDIA pages disagree on the exact cutoff. — [NVIDIA Capture SDK archive](https://developer.nvidia.com/capture-sdk-archive) (via search summary)
- Capture SDK 9.0.0 (formerly GRID SDK) adds Linux PipeWire/Wayland capture, a Vulkan direct-PID capture, and cursor APIs; it targets Vulkan/OpenGL and requires Linux driver 570.86.16+. Supported hardware is Tesla/Quadro/GRID. — [NVIDIA Capture SDK](https://developer.nvidia.com/capture-sdk)
- NVIDIA forum: NvFBC "is a licensed product and not supported on GeForce" for all users. — [NVIDIA dev forum](https://forums.developer.nvidia.com/t/nvfbc-on-geforce/54460)
- keylase/nvidia-patch ships a Windows "NvFBC patch (wrapper) for consumer-grade GPUs" (`win/nvfbcwrp`). It is a wrapper for `NvFBC64.dll` that "injects private keys into NvFBC_CreateEx calls". You rename the original DLLs in System32/SysWOW64 and drop the wrapper in their place, and must redo this after every driver update. It works with apps built on Capture SDK 5.0–7.1. — [keylase/nvidia-patch](https://github.com/keylase/nvidia-patch), [nvfbcwrp README (archived)](https://git.blob42.xyz/Archives/nvidia-patch/src/commit/c5f08495b625cce23808c07bcceb8533376b564a/win/nvfbcwrp/README.md)
- Sunshine offers NvFBC only on Linux and uses DDA/WGC on Windows. — [Sunshine configuration docs](https://docs.lizardbyte.dev/projects/sunshine/latest/md_docs_2configuration.html)
- Local check: `NvFBC64.dll` is still present in System32 on the target PC (driver 32.0.16.2101). — Local check (2026-11-06)

### Inferences
- Even with the DLL present, a GeForce card on build 19044 is both outside NVIDIA's supported OS range and outside its licensed hardware, so any NvFBC use would rely on the wrapper hack. Replacing a signed NVIDIA System32 DLL with an unsigned third-party wrapper on a PC running Vanguard and Ricochet is a needless integrity risk (speculative, but the downside is a ban or blocked launch). It also breaks on every driver update.
- NvFBC's claimed advantages (capture straight from the scan-out surface, independent of DWM, low latency) mattered most before DDA existed or when DWM could be disabled. On Windows 10, DWM is always on and DDA already returns GPU textures, so the practical gain over DDA is small. This is unverified: I found no Windows-10 benchmark comparing them.

### Gaps
- `nvlax` (Linux NvFBC/NVENC patch): not fetched. It is Linux-only and irrelevant to this Windows target.
- Whether current Windows drivers still honour NvFBC on GeForce for whitelisted apps (Steam, NVIDIA's own tools): no primary source.

## 3. DXGI Desktop Duplication: composition, flip modes, latency, timing, ACCESS_LOST

### Takeaway
DDA is documented to capture full-screen applications, including their native back-buffer format via `DuplicateOutput1`. It costs an OS-side copy into the duplication surface. In the one measurement found (Windows 11, RTX 4080) it did **not** knock a borderless game out of Hardware Independent Flip and added no measurable latency. Frames arrive at most once per display update (≤ refresh rate), with `AccumulatedFrames` counting merged presents and `LastPresentTime` giving a QPC timestamp. The prototype's "keyed mutex was abandoned 0x887A0026" is just Windows' message text for **DXGI_ERROR_ACCESS_LOST**; the fix is to release and recreate the duplication.

### Cited Findings
**Composition / independent flip / MPO**
- Flip-model games can bypass DWM composition entirely ("DirectFlip"; "Independent Flip", where "the DWM can go to sleep"), "with the same efficiency as fullscreen exclusive". When other content appears on top, DWM can "seamlessly transition back to composed mode", "reverse compose", or "leverage MPO to maintain the independent flip mode". PresentMon shows which mode is active. — [Microsoft: For best performance, use DXGI flip model](https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/for-best-performance--use-dxgi-flip-model)
- In Independent Flip with a waitable swapchain an app can reach 1 frame of latency. Composed mode adds latency. — [Microsoft flip model doc](https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/for-best-performance--use-dxgi-flip-model)
- PresentMon test (Windows 11, RTX 4080, DX11 game borderless 1080p60, VSync on, 10 s runs). Mirror off: Hardware: Independent Flip, 49.5/56.1 ms until displayed (avg/p95), GPU busy 4.1 ms. **DXGI DDA**: Hardware: Independent Flip, 49.3/55.9 ms, 4.1 ms. **WGC**: Hardware: Independent Flip, 49.6/56.2 ms, 4.0 ms. The author warns that composition, if a capture tool triggered it, "adds up to one refresh of display latency and drops tearing / VRR". VSync-off/uncapped and legacy exclusive fullscreen were **not** measured. Single system, single game, Windows 11. — [ritschy-mirror README](https://github.com/gloomycoder/ritschy-mirror)

**Fullscreen exclusive and formats**
- `DuplicateOutput1` "allows directly receiving the original back buffer format used by a running fullscreen application". Plain `DuplicateOutput` "always converts the fullscreen surface to a 32-bit BGRA format", and that conversion "incurs a performance penalty" when the game uses another format. If the game's format is not in the supplied list, DXGI converts to one of the listed formats. The list should always include `DXGI_FORMAT_B8G8R8A8_UNORM`. — [IDXGIOutput5::DuplicateOutput1](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_5/nf-dxgi1_5-idxgioutput5-duplicateoutput1)
- DDA fails with `DXGI_ERROR_UNSUPPORTED` for "8bpp and non-DWM desktop modes". The app should wait for `EVENT_SYSTEM_DESKTOPSWITCH` / `WM_DISPLAYCHANGE` and retry. `E_ACCESSDENIED` means no access to the current desktop: "only an application that runs at LOCAL_SYSTEM can access the secure desktop". `DXGI_ERROR_NOT_CURRENTLY_AVAILABLE` means the system limit of concurrent duplicating apps ("default of four") has been reached. — [DuplicateOutput1](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_5/nf-dxgi1_5-idxgioutput5-duplicateoutput1)

**Timing and one capture per present**
- `AcquireNextFrame(timeout)` waits up to the timeout for a new frame. A timeout of 0 polls, and INFINITE never times out; the wait cannot be cancelled. A frame is produced "when the operating system either updates the desktop bitmap image or changes the shape or position of a hardware pointer". — [AcquireNextFrame](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/nf-dxgi1_2-idxgioutputduplication-acquirenextframe)
- `LastPresentTime` is a QueryPerformanceCounter timestamp of the last desktop-image update, and 0 means the image did not change (pointer-only update). `AccumulatedFrames` = 1 means the app kept up; > 1 means "more desktop image updates have occurred while the application processed the last desktop update" and were merged. For pointer-only frames, AccumulatedFrames, TotalMetadataBufferSize and LastPresentTime are all 0. — [DXGI_OUTDUPL_FRAME_INFO](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/ns-dxgi1_2-dxgi_outdupl_frame_info)
- "For performance reasons, we recommend that you release the frame just before you call AcquireNextFrame… When the client does not own the frame, the operating system copies all desktop updates to the surface… we recommend that you minimize the time between the call to release the current frame and the call to acquire the next frame." — [ReleaseFrame](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/nf-dxgi1_2-idxgioutputduplication-releaseframe)
- DDA is tied to DWM and the refresh rate, and the AcquireNextFrame timeout does not limit frame rate (community answer, not an OBS maintainer). — [OBS discussion #12058](https://github.com/obsproject/obs-studio/discussions/12058)

**ACCESS_LOST and the "keyed mutex" message**
- `DXGI_ERROR_ACCESS_LOST` = **0x887A0026**: "The desktop duplication interface is invalid… typically becomes invalid when a different type of image is displayed on the desktop." — [DXGI_ERROR](https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/dxgi-error)
- Causes: desktop switch, mode change, "Switch from DWM on, DWM off, or other full-screen application". The app "must release the IDXGIOutputDuplication interface and create a new IDXGIOutputDuplication". Both AcquireNextFrame and ReleaseFrame can return it. — [AcquireNextFrame](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/nf-dxgi1_2-idxgioutputduplication-acquirenextframe), [ReleaseFrame](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/nf-dxgi1_2-idxgioutputduplication-releaseframe)
- Local check: on the target PC, `FormatMessage` for 0x887A0026 returns **"The keyed mutex was abandoned"**, and for 0x887A0027 (`DXGI_ERROR_WAIT_TIMEOUT`) it returns "The timeout value has elapsed and the resource is not yet available". The system message table has stale or misleading text for these DXGI codes, so the prototype's log line is ACCESS_LOST, not a keyed-mutex problem. — Local check (`System.ComponentModel.Win32Exception(0x887A0026).Message`, 2026-11-06), cross-checked with [DXGI_ERROR](https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/dxgi-error)

### Inferences
- **Fullscreen exclusive (FSE):** entering or leaving true FSE, alt-tabbing out of it, or changing the mode (resolution/refresh/HDR) will produce ACCESS_LOST, which matches the documented causes. A recorder must treat ACCESS_LOST as routine. Reacquire in a loop: release the duplication; if DuplicateOutput1 then returns UNSUPPORTED or ACCESS_DENIED (UAC or secure desktop, mid mode-change), back off roughly 100–250 ms or wait for WM_DISPLAYCHANGE / EVENT_SYSTEM_DESKTOPSWITCH. Re-enumerate the output, because a mode change can alter the output's description. Mark the replay timeline with a gap rather than duplicating stale frames across it. The prototype's log should translate 0x887A0026 to "ACCESS_LOST (duplication invalidated)".
- **Borderless / independent flip:** the only measurement (Windows 11) shows DDA keeps the game in Hardware Independent Flip. The ReleaseFrame remarks imply the OS copies updates into the duplication surface rather than forcing DWM to compose. This is plausible on 19044 but **unverified**. Run PresentMon with the recorder on and off in Valorant and CoD and compare the PresentMode and MsUntilDisplayed columns.
- **G-SYNC/VRR:** VRR survives as long as the game stays in independent flip; if capture forced composition, VRR would drop (per the ritschy-mirror note). With VRR and the game below refresh, each game present becomes one scan-out update, so DDA should yield about one capture per present with AccumulatedFrames = 1. Speculation consistent with the docs.
- **Above refresh rate (VSync off, game > 144 fps on a 144 Hz monitor):** DDA delivers at most one image per display update, and AccumulatedFrames > 1 shows the presents that were merged. That is acceptable for a 144 fps replay, since the display never showed more distinct frames either; with tearing, the captured frame is the last complete present.
- **One capture per present, without Sleep:** block in `AcquireNextFrame` with a modest timeout (for example 16–100 ms) and do no Sleep-based pacing. Skip the copy when `LastPresentTime == 0`. Use `LastPresentTime` (QPC) directly as the VFR PTS, and use `AccumulatedFrames` as a dropped/merged-frame counter. This removes ffmpeg ddagrab's Sleep-granularity cap (~64 fps) because the wait is event-driven in DXGI. Keep the copy short and release just before the next acquire, per the ReleaseFrame guidance.
- **Concurrency:** if OBS Display Capture, Discord screen share or another recorder is also duplicating, the four-app limit can be hit (`NOT_CURRENTLY_AVAILABLE`). Handle it with a retry and a user-visible warning.

### Gaps
- No primary Microsoft statement found on whether DDA forces composition or disables MPO/independent flip on Windows 10 21H2 specifically. The only data point is a Windows 11 community measurement.
- No measurement found of DDA behaviour with true legacy FSE plus VSync off, or with G-SYNC on, on Windows 10.
- I did not confirm from docs whether DuplicateOutput1 requires per-monitor-v2 DPI awareness (often reported by developers). The fetched doc text does not mention it.

## 4. Windows.Graphics.Capture on build 19044

### Takeaway
On 19044 the yellow capture border cannot be turned off for a normal desktop app. `IsBorderRequired` only exists from build 20348 (Windows 11/Server 2022 era), and `MinUpdateInterval` (frame-rate control) only from 26100 (Windows 11 24H2). WGC is a fine API on Windows 11 but a non-starter on this PC.

### Cited Findings
- `GraphicsCaptureSession.IsBorderRequired` was introduced in "Windows 10, version 2104 (introduced in 10.0.20348.0)", UniversalApiContract v12.0. Turning the border off also requires calling `GraphicsCaptureAccess.RequestAccessAsync(GraphicsCaptureAccessKind.Borderless)`, which shows a user prompt, and declaring the `graphicsCaptureWithoutBorder` capability in the package manifest. If another app on the device requires a border for the same window or display, the border is shown anyway. — [IsBorderRequired](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscapturesession.isborderrequired)
- `GraphicsCaptureSession.MinUpdateInterval` is documented only for 26100+ (no 19041/22000 moniker). — [MinUpdateInterval](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscapturesession.minupdateinterval)
- `GraphicsCaptureSession` itself dates from 1803 (17134). The version history lists `IsCursorCaptureEnabled` as added in 2004 (19041), so it is available on 19044. Other properties: `IncludeSecondaryWindows`, `DirtyRegionMode`, `IsBorderRequired`, `MinUpdateInterval`. — [GraphicsCaptureSession class](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscapturesession)
- On Microsoft Q&A, a desktop developer found the border can only be avoided for in-app windows cloaked with DWM (`DWMWA_CLOAK`), which rules out capturing a game. The thread had no resolution. — [Microsoft Q&A](https://learn.microsoft.com/en-us/answers/questions/108678/how-to-remove-yellow-boarder-capture-indicator-fro)
- In one Windows 11 measurement, WGC monitor capture also kept the game in Hardware Independent Flip with unchanged latency. — [ritschy-mirror README](https://github.com/gloomycoder/ritschy-mirror)
- Sunshine marks WGC as beta and says it does not work under the Sunshine service. — [Sunshine configuration docs](https://docs.lizardbyte.dev/projects/sunshine/latest/md_docs_2configuration.html)
- WGC support has been merged into ffmpeg as `gfxcapture`. — [ffmpeg trac #9413](https://trac.ffmpeg.org/ticket/9413)

### Inferences
- 19044 is a 19041-family build (UniversalApiContract v10), so the 20348 and 26100 APIs are absent. Any border-free WGC claim on Windows 10 would rely on undocumented hacks (patching DWM or injecting into dwm.exe). Those are worse for anti-cheat than the border itself. Speculation; I found no working method.
- Without `MinUpdateInterval`, WGC on 19044 delivers frames at the DWM update rate with no throttling, which is acceptable for 144 fps. The forced border is the deal-breaker.
- Per-window WGC capture would avoid capturing the other two monitors and survive window moves, but the border makes it unusable here. It becomes the cleanest non-hook option if the PC ever moves to Windows 11 (22000+ for borderless, 26100+ for MinUpdateInterval).

### Gaps
- No primary data on WGC behaviour with legacy fullscreen exclusive on Windows 10, or on its exact per-frame GPU cost compared with DDA.

## 5. Swapchain Present hooks and anti-cheat

### Takeaway
Hooks get the game's real back buffer at every Present, before DWM, so they are exact, per-present and capture only the game. But they work under Vanguard, Ricochet, EAC and BattlEye only because anti-cheats explicitly allow-list specific signed binaries (OBS's graphics-hook). The allow-list broke whenever OBS changed its hook files or signing certificate. A custom hook DLL would have no allow-list entry, so writing our own hook is not realistic for Valorant or CoD.

### Cited Findings
- OBS 31.1.0 changelog: "updated game capture hook signatures on Windows and fixes game capture with Riot Games titles that use Vanguard". This followed OBS 31.0's code-signing certificate change, which OBS warned "may impact game capture compatibility with some anti-cheat solutions". — [OBS 31.1.0-beta tag notes (mirror)](https://gitlab.dsi.universite-paris-saclay.fr/khuon-viet.pham/obs-studio/-/tags/31.1.0-beta1), [linuxcompatible: OBS 31.1.0](https://www.linuxcompatible.org/story/obs-studio-3110-released)
- Community diagnosis of the 31.0 problem: the hook files in `obs-studio-hook` carried only the new certificate, whereas 30.2 was dual-signed, and the Valorant freeze/black screen happened only with the new-cert hook. Users downgraded to 30.2.x. — [OBS forum: capturing VALORANT with 31.0.3](https://obsproject.com/forum/threads/i-was-able-to-capture-valorant-gameplay-with-the-latest-obs-31-0-3.185315)
- OBS publicly blamed Vanguard for a Game Capture black screen in Valorant/League ("an issue on the Vanguard side that we have no control over"), and Riot later shipped a Vanguard update. — [win.gg](https://win.gg/obs-game-capture-not-working-valorant-league-of-legends-fix/)
- (Older, 2017–2019) An EAC game stopped allowing OBS Game Capture after OBS 19.0.3 changed its hook files; downgrading to 19.0.2 fixed it. — [OBS forum: Black Screen 19.0.3](https://obsproject.com/forum/threads/black-screen-19-0-3.70638/)
- (Older) An OBS staff member said OBS "cannot update Game Capture to bypass common anti-cheat as that would be considered malicious and harmful code". Anti-cheat blocks injection unless the tool is allow-listed. — [OBS forum: game capture won't hook](https://obsproject.com/forum/threads/game-capture-wont-hook-game-capture-is-just-a-black-screen.115678/)
- Third-party Rust wrappers reuse OBS's graphics-hook precisely because it is "signed and whitelisted by all anti-cheats". This is the project's own claim. — [obs-rs](https://github.com/hzqst/obs-rs)
- (Older, 2021) Ricochet's kernel driver "checks the software and applications that attempt to interact with and manipulate" the game and is active only while Call of Duty runs. A user reported OBS performance problems in Warzone after Ricochet launched. — [esports.gg on Ricochet](https://esports.gg/news/call-of-duty/call-of-duty-announce-their-new-anti-cheat-system-ricochet/), [OBS forum: Warzone Ricochet](https://obsproject.com/forum/threads/warzone-ricochet-update-causing-issues-with-obs.151379)
- OBS Game Capture's two injection routes (direct `PROCESS_ALL_ACCESS` + library injection, or a helper process given the game's thread id in "anti-cheat compatibility" mode) and the shared-texture transport are in [game-capture.c](https://github.com/obsproject/obs-studio/blob/master/plugins/win-capture/game-capture.c).

### Inferences
- **Why hooks see the real frames:** the hook patches the game's `IDXGISwapChain::Present` (or the Vulkan/GL equivalent), so it runs once per game frame and can copy the back buffer before presentation. That gives exactly one capture per present, no DWM dependency, works in FSE, and no other window's content. OBS then shares it across processes as a GPU texture (shtex). The per-present copy runs on the game's own GPU queue and inside its frame time.
- **Why OBS passes and a custom DLL would not:** Vanguard (kernel, boot-start) and Ricochet (kernel) police modules loaded into the game process. Being allowed depends on a known signer or hash. OBS's experience shows that even a legitimate vendor's re-signed hook is blocked until the anti-cheat vendor updates. An unknown DLL injected with `PROCESS_ALL_ACCESS` or a hooks-based helper looks exactly like a cheat loader. Best case: the game refuses to start or crashes. Worst case: a flag or ban. Building our own hook is **dangerous** for Valorant and CoD and should be ruled out (speculation about enforcement outcomes; vendors do not publish their rules).
- **Using OBS's own signed hook via libobs** (as Outplayed appears to, "OBS based") is the only realistic hook path. The costs: OBS-release lockstep, with breakage whenever OBS's signatures change; libobs's GPL licensing (not verified here); and a dependency on anti-cheat vendors continuing to allow it. It offers little over DDA for a single-game, monitor-wide replay.

### Gaps
- No official Riot, Activision, EAC or BattlEye document lists allowed recording or overlay software, or states whether DDA-based recorders are always fine. I found none, so the claim that "out-of-process DDA is anti-cheat-safe" rests on the absence of reported problems and on the API not touching the game process.
- Exact GPU cost of OBS's in-game copy per present: not measured in any source found.

## 6. HDR, mouse cursor, multi-monitor

### Takeaway
DDA covers all three: use `DuplicateOutput1` with FP16/10-bit formats for HDR, the pointer metadata (position plus shape buffer) to draw the cursor yourself on the GPU, and one duplication per output for multiple monitors. For a game replay, duplicate only the monitor showing the game.

### Cited Findings
- HDR: `DuplicateOutput1` can return the native full-screen format, such as R10G10B10A2 ("full gamut of colors"); scan-out formats only; BGRA8 should always be in the list. — [DuplicateOutput1](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_5/nf-dxgi1_5-idxgioutput5-duplicateoutput1)
- HDR swapchain content (R10G10B10A2_UNORM or R16G16B16A16_FLOAT) "isn't clamped unless it's composed to an SDR desktop". — [Microsoft flip model doc](https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/for-best-performance--use-dxgi-flip-model)
- An FP16 DDA capture path for HDR→SDR tonemapping is used in practice. — [ritschy-mirror README](https://github.com/gloomycoder/ritschy-mirror)
- Cursor: each acquired frame carries `LastMouseUpdateTime`, `PointerPosition` (the top-left of the shape, not the hotspot) and `PointerShapeBufferSize`. A non-zero size means a new shape to fetch with `GetFramePointerShape`. Pointer-only updates also wake `AcquireNextFrame`. — [DXGI_OUTDUPL_FRAME_INFO](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/ns-dxgi1_2-dxgi_outdupl_frame_info), [AcquireNextFrame](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/nf-dxgi1_2-idxgioutputduplication-acquirenextframe)
- WGC can include the cursor itself (`IsCursorCaptureEnabled`, 19041+). — [GraphicsCaptureSession](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscapturesession)
- Multi-monitor: DDA duplicates one output. The device "must be created from the adapter to which the output is connected", and an app cannot duplicate the same output twice. — [DuplicateOutput1](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_5/nf-dxgi1_5-idxgioutput5-duplicateoutput1)

### Inferences
- Cursor plan: cache the shape texture (monochrome, color and masked-color shapes need different blending), and composite it in the same GPU pass that converts BGRA→NV12 for NVENC. That adds negligible cost. FPS games hide the cursor in-game, so this matters mainly for menus and desktop clips. Pointer-only acquires (LastPresentTime == 0) should update cursor state without copying a frame.
- Multi-monitor plan: find the game's monitor (`MonitorFromWindow` on the foreground window, matched to `DXGI_OUTPUT_DESC.Monitor`) and duplicate only that output. Re-target when the foreground window moves. Three simultaneous 144 fps duplications would triple copy and encode cost for content nobody wants in a game clip.
- HDR plan: HDR is probably off on these 1080p monitors (not verified). Request `{B8G8R8A8_UNORM, R10G10B10A2_UNORM, R16G16B16A16_FLOAT}` so DXGI never does a hidden conversion, then tonemap or convert to P010/NV12 in your own shader.

### Gaps
- Did not verify whether the desktop image returned by DDA ever includes a software-drawn cursor (as some games draw their own); assume the hardware cursor is excluded and delivered via metadata.

## 7. GPU time and memory bandwidth at 1080p / 144 fps

### Takeaway
No source gives per-method GPU-time numbers at 1080p144. Back-of-envelope figures put DDA plus copy plus NV12 conversion at about 5–7 GB/s, which is a low single-digit percent of an RTX 4060 Ti's memory bandwidth. The one real measurement (60 fps) found DDA and WGC GPU cost within run-to-run noise. The hidden cost to watch is VRAM if raw frames are buffered for long.

### Cited Findings
- Windows 11 / RTX 4080 / 1080p60 measurement: GPU busy 4.1 ms with no capture, 4.1 ms with DDA, 4.0 ms with WGC ("within run-to-run noise"). The capture cost "scales with the mirror's own frame rate and output size, not the game's" and "still uses some GPU time, which competes with the game when the GPU is already fully loaded". — [ritschy-mirror README](https://github.com/gloomycoder/ritschy-mirror)
- DDA has a built-in OS-side copy: while the client does not hold the frame, "the operating system copies all desktop updates to the surface", which wastes GPU cycles if regions are rewritten repeatedly. — [ReleaseFrame](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/nf-dxgi1_2-idxgioutputduplication-releaseframe)
- Plain `DuplicateOutput` adds a format-conversion "performance penalty" when the game's buffer is not BGRA8; `DuplicateOutput1` avoids it. — [DuplicateOutput1](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_5/nf-dxgi1_5-idxgioutput5-duplicateoutput1)
- OBS Game Capture's shtex path shares the texture with no CPU copy; shmem adds a CPU round-trip. — [game-capture.c](https://github.com/obsproject/obs-studio/blob/master/plugins/win-capture/game-capture.c)

### Inferences (arithmetic; my own, not from a source)
- One 1920×1080 BGRA8 frame = 8,294,400 B ≈ 8.3 MB; FP16 = 16.6 MB; NV12 = 3.1 MB.
- Per captured frame at 144 fps:
  - OS copy into the duplication surface: ≈ 8.3 MB read + 8.3 MB write. This happens whenever the screen updates, captured or not.
  - Our `CopyResource` into the ring: another ≈ 16.6 MB.
  - BGRA→NV12 conversion (with cursor composite): ≈ 8.3 MB read + 3.1 MB write.
  - NVENC input read: ≈ 3.1 MB, plus reference-frame traffic.
  - Total: about 45–50 MB per frame, or roughly 6.5–7 GB/s at 144 fps. That is about 2–2.5% of the 4060 Ti's 288 GB/s spec bandwidth (spec figure from memory, not re-verified here).
  - GPU time per 8 MB copy at that bandwidth is on the order of 0.06–0.1 ms, so about 0.3–0.5 ms per captured frame across all passes, or about 5–7% of one 6.94 ms frame budget if the GPU is fully saturated. Much less in practice, since copy and conversion overlap with the game's work and NVENC is a separate fixed-function engine.
- Skipping the ring copy (convert directly from the acquired DDA surface into the NV12 encoder input, then release) removes about 16.6 MB per frame, roughly a third of the traffic.
- Hook capture costs about one back-buffer copy per **game** present (≈ 16.6 MB at the game's frame rate, possibly above 144). That cost lands inside the game's own frame, so it is not cheaper than DDA for this use.
- **VRAM:** raw 1080p BGRA at 144 fps is about 1.19 GB per second of history. The GPU texture ring must stay a few frames deep (an encoder queue), and the replay buffer must hold **encoded** packets in system RAM. At 50 Mbps, 30 s is about 190 MB.
- Three monitors duplicated at 144 fps would roughly triple all of the above. Another reason to capture only the game's output.

### Gaps
- No published per-method GPU-time benchmark at 1080p144 on Ada/Windows 10. Measure with PresentMon (GPU busy) and GPUView / Nsight Systems with the recorder on and off.

## 8. Which capture method is best for this PC?

### Takeaway
**Keep DXGI Desktop Duplication**, upgraded to `DuplicateOutput1` and driven by blocking `AcquireNextFrame` waits, as the only capture path. Capture only the game's monitor and treat ACCESS_LOST as routine. WGC is out on 19044 (forced border; `IsBorderRequired` only from 20348). NvFBC is out (deprecated on Windows 10, not licensed for GeForce, needs a System32 DLL hack). A custom Present hook is out (no anti-cheat allow-list; ban risk under Vanguard and Ricochet).

### Cited Findings
- DDA is what Sunshine (the reference open-source GPU-streaming host) uses on Windows, with WGC as beta and NvFBC Linux-only. — [Sunshine configuration docs](https://docs.lizardbyte.dev/projects/sunshine/latest/md_docs_2configuration.html)
- DDA supports full-screen apps in their native format and returns GPU textures; ACCESS_LOST requires recreating the interface. — [DuplicateOutput1](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_5/nf-dxgi1_5-idxgioutput5-duplicateoutput1), [AcquireNextFrame](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/nf-dxgi1_2-idxgioutputduplication-acquirenextframe)
- WGC border control requires 20348+; MinUpdateInterval requires 26100+. — [IsBorderRequired](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscapturesession.isborderrequired), [MinUpdateInterval](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscapturesession.minupdateinterval)
- NvFBC is deprecated on Windows 10, with 1803 the last supported build. — [NVIDIA Capture SDK](https://developer.nvidia.com/capture-sdk)
- Hook capture survives anti-cheat only through vendor allow-listing of signed binaries, which broke even for OBS on certificate changes. — [OBS 31.1.0 notes](https://www.linuxcompatible.org/story/obs-studio-3110-released), [OBS forum 19.0.3](https://obsproject.com/forum/threads/black-screen-19-0-3.70638/)

### Inferences (design recommendations)
1. **Pacing:** use one capture thread per duplicated output, blocking in `AcquireNextFrame(timeout ≈ 50–100 ms)` with no Sleep. Release the previous frame immediately before acquiring. On WAIT_TIMEOUT, do nothing; the encoder side can repeat the last frame if a constant frame rate is ever needed, as Sunshine does.
2. **Change detection:** process only when `LastPresentTime != 0`. Use `LastPresentTime` (QPC) as the VFR timestamp, log `AccumulatedFrames - 1` as merged presents, and handle pointer-only frames as cursor updates.
3. **Formats:** call `DuplicateOutput1` with `{BGRA8, R10G10B10A2, RGBA16F}` and do conversion and tonemapping in your own compute or pixel pass straight into NVENC's NV12/P010 input. Avoid an extra full-frame ring copy where possible.
4. **Recovery:** on ACCESS_LOST (0x887A0026; fix the misleading "keyed mutex" log text), release, re-enumerate outputs, and recreate. On E_ACCESSDENIED/UNSUPPORTED/NOT_CURRENTLY_AVAILABLE, back off and wait for WM_DISPLAYCHANGE / EVENT_SYSTEM_DESKTOPSWITCH. Mark a discontinuity in the replay.
5. **Scope:** duplicate only the output under the foreground game window. Optionally run a slow (≤ 30 fps) duplication of the other monitors only if the user explicitly wants them.
6. **Verify on this PC:** run PresentMon with the recorder off and on in Valorant and CoD (borderless and fullscreen), checking PresentMode (must stay "Hardware: Independent Flip" or "Hardware Composed: Independent Flip"), MsUntilDisplayed and GPU busy. Also confirm G-SYNC stays active. This is the one claim (no forced composition on 19044) that rests on a Windows 11 data point.
7. **Anti-cheat posture:** DDA never opens, reads or injects into the game process, so there is nothing for Vanguard or Ricochet to flag. Keep the recorder free of game-process handles, overlays in the game, and kernel components (inference; no vendor statement found).
8. **If the PC moves to Windows 11 (22000+ / 24H2):** borderless WGC per-window capture with `MinUpdateInterval` becomes a credible second backend. The only hook route worth considering is libobs with OBS's signed graphics-hook.

### Gaps
- No vendor (Riot, Activision) statement confirms that DDA-based recorders are always allowed. The recommendation rests on mechanism (out-of-process, OS API) and the absence of reported bans.
- The 19044-specific independent-flip, MPO and G-SYNC behaviour under DDA needs the on-PC PresentMon test above.

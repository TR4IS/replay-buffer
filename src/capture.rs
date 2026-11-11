//! Screen capture with DXGI Desktop Duplication, encoded on the GPU with NVENC.
//!
//! Per new desktop image there is exactly one small GPU pass: a shader reads
//! the duplication surface directly, draws the mouse cursor in, and writes
//! NV12 (or 10-bit P010) with BT.709 colour into a texture NVENC reads. No
//! extra full-frame copy, and NVENC never borrows the game's shader cores.
//!
//! Frames are emitted on a fixed grid (e.g. every 1/144 s). When the screen
//! didn't change, the previous image is sent again; NVENC turns that into an
//! almost empty packet and it costs the game nothing. A fixed rate is what
//! video editors expect. If we ever fall behind, missed grid slots are skipped
//! (never caught up), so timestamps always match real time.

use anyhow::{Context, Result, anyhow, bail};
use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{HANDLE, HMODULE};
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::System::Threading::*;
use windows::core::{Interface, PCSTR, PCWSTR};

unsafe extern "C" {
    fn nvs_create(
        device: *mut c_void,
        width: u32,
        height: u32,
        fps: u32,
        codec: i32,
        ten_bit: i32,
        preset: i32,
        aq: i32,
        cq: u32,
        max_kbps: u32,
        out: *mut *mut c_void,
    ) -> i32;
    fn nvs_encode(shim: *mut c_void, tex: *mut c_void, force_idr: i32, data: *mut *const u8, size: *mut u32) -> i32;
    fn nvs_drain(shim: *mut c_void, data: *mut *const u8, size: *mut u32) -> i32;
    fn nvs_destroy(shim: *mut c_void);
    fn nvs_depth() -> i32;
}

#[derive(Clone, Debug)]
pub struct VideoParams {
    pub monitor: u32,
    pub fps: u32,
    pub codec: i32,
    pub ten_bit: bool,
    pub preset: u32,
    pub aq: bool,
    pub quality: u32,
    pub max_kbps: u32,
    pub cursor: bool,
    pub gpu_priority: i32,
}

/// Live counters for the UI and logs.
#[derive(Default)]
pub struct VideoStats {
    /// Frames sent to the encoder (grid slots filled).
    pub frames: AtomicU64,
    /// Desktop updates we converted (new images or cursor moves).
    pub new_frames: AtomicU64,
    /// Grid slots that repeated the previous image.
    pub repeats: AtomicU64,
    /// Grid slots skipped because we were late.
    pub dropped: AtomicU64,
    /// Game frames that Windows merged before we read them.
    pub merged: AtomicU64,
    /// Encoded bytes, for a live bitrate readout.
    pub bytes: AtomicU64,
    pub width: AtomicU64,
    pub height: AtomicU64,
}

/// One encoded video frame. `pts` counts grid slots (1/fps s each).
pub struct Packet {
    pub pts: i64,
    pub key: bool,
    pub data: Vec<u8>,
}

/// Maps grid slots to the shared clock (QPC) so audio can be lined up.
#[derive(Clone, Copy, Debug)]
pub struct Timeline {
    pub qpc0: i64,
    pub qpc_freq: i64,
    pub fps: u32,
    pub width: u32,
    pub height: u32,
}

impl Timeline {
    /// QPC time of grid slot `pts`, in 100 ns units (the unit WASAPI uses).
    pub fn slot_hns(&self, pts: i64) -> i64 {
        let start = qpc_to_hns(self.qpc0, self.qpc_freq);
        start + (pts as i128 * 10_000_000 / self.fps as i128) as i64
    }
}

pub fn qpc_to_hns(q: i64, freq: i64) -> i64 {
    (q as i128 * 10_000_000 / freq as i128) as i64
}

pub fn qpc_now() -> i64 {
    let mut c = 0i64;
    unsafe {
        let _ = windows::Win32::System::Performance::QueryPerformanceCounter(&mut c);
    }
    c
}

pub fn qpc_freq() -> i64 {
    let mut f = 0i64;
    unsafe {
        let _ = windows::Win32::System::Performance::QueryPerformanceFrequency(&mut f);
    }
    f
}

struct Encoder(*mut c_void);
impl Drop for Encoder {
    fn drop(&mut self) {
        unsafe { nvs_destroy(self.0) }
    }
}

/// `index` 0 = the primary monitor; 1, 2, 3... = monitors in the order Windows lists them.
fn find_output(index: u32) -> Result<(IDXGIAdapter1, IDXGIOutput1, u32, u32)> {
    let mut found = None;
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1()?;
        let mut n = 0;
        let mut a = 0;
        while let Ok(adapter) = factory.EnumAdapters1(a) {
            let mut o = 0;
            while let Ok(output) = adapter.EnumOutputs(o) {
                let desc = output.GetDesc()?;
                if desc.AttachedToDesktop.as_bool() {
                    n += 1;
                    let r = desc.DesktopCoordinates;
                    let (w, h) = ((r.right - r.left) as u32, (r.bottom - r.top) as u32);
                    let primary = r.left == 0 && r.top == 0;
                    crate::log!(
                        "monitor {n}: {w}x{h} at ({}, {}){}",
                        r.left,
                        r.top,
                        if primary { " (primary)" } else { "" }
                    );
                    if found.is_none() && (index == n || (index == 0 && primary)) {
                        found = Some((adapter.clone(), output.cast()?, w, h));
                    }
                }
                o += 1;
            }
            a += 1;
        }
    }
    found.ok_or_else(|| anyhow!("monitor {index} not found"))
}

/// Lists monitors for the settings page: (number, width, height, primary).
pub fn list_monitors() -> Vec<(u32, u32, u32, bool)> {
    let mut v = Vec::new();
    unsafe {
        let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else { return v };
        let mut n = 0;
        let mut a = 0;
        while let Ok(adapter) = factory.EnumAdapters1(a) {
            let mut o = 0;
            while let Ok(output) = adapter.EnumOutputs(o) {
                if let Ok(desc) = output.GetDesc() {
                    if desc.AttachedToDesktop.as_bool() {
                        n += 1;
                        let r = desc.DesktopCoordinates;
                        v.push((n, (r.right - r.left) as u32, (r.bottom - r.top) as u32, r.left == 0 && r.top == 0));
                    }
                }
                o += 1;
            }
            a += 1;
        }
    }
    v
}

const SHADER: &str = r#"
Texture2D<float4> src : register(t0);
Texture2D<float4> cur : register(t1);
cbuffer C : register(b0) {
    int2 cur_pos;
    int2 cur_size;
    int cur_on;
    int linear_src;
    float y_off;
    float y_range;
    float c_off;
    float c_range;
    float scale;
    float pad;
};
float3 to_srgb(float3 l) {
    l = saturate(l);
    float3 lo = l * 12.92;
    float3 hi = 1.055 * pow(l, 1.0 / 2.4) - 0.055;
    return lerp(lo, hi, step(0.0031308, l));
}
float3 fetch(int2 p) {
    float3 c = src.Load(int3(p, 0)).rgb;
    if (linear_src) c = to_srgb(c);
    if (cur_on) {
        int2 q = p - cur_pos;
        if (q.x >= 0 && q.y >= 0 && q.x < cur_size.x && q.y < cur_size.y) {
            float4 k = cur.Load(int3(q, 0));
            if (k.a > 0.0) c = lerp(c, k.rgb, k.a);
            else if (k.r > 0.5) c = 1.0 - c;
        }
    }
    return saturate(c);
}
struct V { float4 pos : SV_Position; };
V vs(uint id : SV_VertexID) {
    float2 t = float2((id << 1) & 2, id & 2);
    V o;
    o.pos = float4(t * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
    return o;
}
static const float3 K = float3(0.2126, 0.7152, 0.0722); // BT.709
float ps_y(V i) : SV_Target {
    float y = dot(fetch(int2(i.pos.xy)), K);
    return (y_off + y_range * y) * scale;
}
float2 ps_uv(V i) : SV_Target {
    int2 p = int2(i.pos.xy) * 2;
    float3 c = (fetch(p) + fetch(p + int2(1, 0)) + fetch(p + int2(0, 1)) + fetch(p + int2(1, 1))) * 0.25;
    float y = dot(c, K);
    float u = (c.b - y) / 1.8556;
    float v = (c.r - y) / 1.5748;
    return float2(c_off + c_range * u, c_off + c_range * v) * scale;
}
"#;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Consts {
    cur_pos: [i32; 2],
    cur_size: [i32; 2],
    cur_on: i32,
    linear_src: i32,
    y_off: f32,
    y_range: f32,
    c_off: f32,
    c_range: f32,
    scale: f32,
    pad: f32,
}

fn compile(entry: &str, target: &str) -> Result<ID3DBlob> {
    let entry = std::ffi::CString::new(entry)?;
    let target = std::ffi::CString::new(target)?;
    let mut code = None;
    let mut errs = None;
    let r = unsafe {
        D3DCompile(
            SHADER.as_ptr() as *const c_void,
            SHADER.len(),
            PCSTR::null(),
            None,
            None,
            PCSTR(entry.as_ptr() as *const u8),
            PCSTR(target.as_ptr() as *const u8),
            0,
            0,
            &mut code,
            Some(&mut errs),
        )
    };
    if let Err(e) = r {
        let msg = errs
            .map(|b: ID3DBlob| unsafe {
                String::from_utf8_lossy(std::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize()))
                    .to_string()
            })
            .unwrap_or_default();
        bail!("shader compile failed: {e} {msg}");
    }
    code.ok_or_else(|| anyhow!("no shader code"))
}

fn blob(b: &ID3DBlob) -> &[u8] {
    unsafe { std::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize()) }
}

/// One output surface NVENC reads, with render-target views for its two planes.
struct Surface {
    tex: ID3D11Texture2D,
    y: ID3D11RenderTargetView,
    uv: ID3D11RenderTargetView,
}

/// Mouse cursor state from Desktop Duplication.
#[derive(Default)]
struct Cursor {
    pos: (i32, i32),
    visible: bool,
    size: (i32, i32),
    srv: Option<ID3D11ShaderResourceView>,
}

struct Gpu {
    device: ID3D11Device,
    ctx: ID3D11DeviceContext,
    vs: ID3D11VertexShader,
    ps_y: ID3D11PixelShader,
    ps_uv: ID3D11PixelShader,
    cbuf: ID3D11Buffer,
    width: u32,
    height: u32,
    ten_bit: bool,
    // The duplication surface usually stays the same texture; keep its view.
    src_cache: Option<(usize, ID3D11ShaderResourceView, bool)>,
    // Fallback when the duplication surface can't be read by a shader directly.
    staging: Option<ID3D11Texture2D>,
}

impl Gpu {
    fn new(device: ID3D11Device, ctx: ID3D11DeviceContext, width: u32, height: u32, ten_bit: bool) -> Result<Gpu> {
        let vsb = compile("vs", "vs_5_0")?;
        let pyb = compile("ps_y", "ps_5_0")?;
        let puvb = compile("ps_uv", "ps_5_0")?;
        let (mut vs, mut ps_y, mut ps_uv, mut cbuf) = (None, None, None, None);
        unsafe {
            device.CreateVertexShader(blob(&vsb), None, Some(&mut vs))?;
            device.CreatePixelShader(blob(&pyb), None, Some(&mut ps_y))?;
            device.CreatePixelShader(blob(&puvb), None, Some(&mut ps_uv))?;
            let bd = D3D11_BUFFER_DESC {
                ByteWidth: std::mem::size_of::<Consts>().next_multiple_of(16) as u32,
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                ..Default::default()
            };
            device.CreateBuffer(&bd, None, Some(&mut cbuf))?;
        }
        Ok(Gpu {
            device,
            ctx,
            vs: vs.unwrap(),
            ps_y: ps_y.unwrap(),
            ps_uv: ps_uv.unwrap(),
            cbuf: cbuf.unwrap(),
            width,
            height,
            ten_bit,
            src_cache: None,
            staging: None,
        })
    }

    fn make_surface(&self) -> Result<Surface> {
        let (fmt, yf, uvf) = if self.ten_bit {
            (DXGI_FORMAT_P010, DXGI_FORMAT_R16_UNORM, DXGI_FORMAT_R16G16_UNORM)
        } else {
            (DXGI_FORMAT_NV12, DXGI_FORMAT_R8_UNORM, DXGI_FORMAT_R8G8_UNORM)
        };
        let desc = D3D11_TEXTURE2D_DESC {
            Width: self.width,
            Height: self.height,
            MipLevels: 1,
            ArraySize: 1,
            Format: fmt,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut tex = None;
        unsafe { self.device.CreateTexture2D(&desc, None, Some(&mut tex)) }
            .context("creating NV12/P010 texture (GPU can't render to it?)")?;
        let tex: ID3D11Texture2D = tex.unwrap();
        let rtv = |f: DXGI_FORMAT| -> Result<ID3D11RenderTargetView> {
            let d = D3D11_RENDER_TARGET_VIEW_DESC {
                Format: f,
                ViewDimension: D3D11_RTV_DIMENSION_TEXTURE2D,
                Anonymous: D3D11_RENDER_TARGET_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_RTV { MipSlice: 0 } },
            };
            let mut v = None;
            unsafe { self.device.CreateRenderTargetView(&tex, Some(&d), Some(&mut v))? };
            Ok(v.unwrap())
        };
        Ok(Surface { y: rtv(yf)?, uv: rtv(uvf)?, tex })
    }

    /// Converts the desktop image (plus cursor) into `dst`.
    fn convert(&mut self, desktop: &ID3D11Texture2D, cursor: &Cursor, draw_cursor: bool, dst: &Surface) -> Result<()> {
        let key = desktop.as_raw() as usize;
        if self.src_cache.as_ref().map(|c| c.0) != Some(key) {
            let mut d = D3D11_TEXTURE2D_DESC::default();
            unsafe { desktop.GetDesc(&mut d) };
            let linear = d.Format == DXGI_FORMAT_R16G16B16A16_FLOAT;
            let readable = d.BindFlags & D3D11_BIND_SHADER_RESOURCE.0 as u32 != 0;
            let view_tex = if readable {
                desktop.clone()
            } else {
                // Rare: copy into a shader-readable texture of the same format.
                let mut sd = d;
                sd.BindFlags = D3D11_BIND_SHADER_RESOURCE.0 as u32;
                sd.MiscFlags = 0;
                sd.CPUAccessFlags = 0;
                sd.Usage = D3D11_USAGE_DEFAULT;
                let mut t = None;
                unsafe { self.device.CreateTexture2D(&sd, None, Some(&mut t))? };
                let t: ID3D11Texture2D = t.unwrap();
                self.staging = Some(t.clone());
                crate::log!("desktop surface is not shader-readable; using a copy");
                t
            };
            let mut srv = None;
            unsafe { self.device.CreateShaderResourceView(&view_tex, None, Some(&mut srv))? };
            crate::log!("desktop surface format {:?}{}", d.Format, if linear { " (linear HDR)" } else { "" });
            self.src_cache = Some((key, srv.unwrap(), linear));
            if readable {
                self.staging = None;
            }
        }
        let (_, srv, linear) = self.src_cache.clone().unwrap();
        let ctx = &self.ctx;
        unsafe {
            if let Some(st) = &self.staging {
                ctx.CopyResource(st, desktop);
            }
            let on = draw_cursor && cursor.visible && cursor.srv.is_some();
            let (y_off, y_range, c_off, c_range, scale) = if self.ten_bit {
                (64.0, 876.0, 512.0, 896.0, 64.0 / 65535.0)
            } else {
                (16.0, 219.0, 128.0, 224.0, 1.0 / 255.0)
            };
            let k = Consts {
                cur_pos: [cursor.pos.0, cursor.pos.1],
                cur_size: [cursor.size.0, cursor.size.1],
                cur_on: on as i32,
                linear_src: linear as i32,
                y_off,
                y_range,
                c_off,
                c_range,
                scale,
                pad: 0.0,
            };
            ctx.UpdateSubresource(&self.cbuf, 0, None, &k as *const Consts as *const c_void, 0, 0);
            ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            ctx.IASetInputLayout(None);
            ctx.VSSetShader(&self.vs, None);
            ctx.PSSetConstantBuffers(0, Some(&[Some(self.cbuf.clone())]));
            ctx.PSSetShaderResources(0, Some(&[Some(srv), if on { cursor.srv.clone() } else { None }]));
            ctx.OMSetBlendState(None, None, 0xffff_ffff);
            ctx.RSSetState(None);

            let vp = |w: u32, h: u32| D3D11_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: w as f32,
                Height: h as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            };
            ctx.OMSetRenderTargets(Some(&[Some(dst.y.clone())]), None);
            ctx.RSSetViewports(Some(&[vp(self.width, self.height)]));
            ctx.PSSetShader(&self.ps_y, None);
            ctx.Draw(3, 0);

            ctx.OMSetRenderTargets(Some(&[Some(dst.uv.clone())]), None);
            ctx.RSSetViewports(Some(&[vp(self.width / 2, self.height / 2)]));
            ctx.PSSetShader(&self.ps_uv, None);
            ctx.Draw(3, 0);

            // Unbind so the duplication surface can be released cleanly.
            ctx.OMSetRenderTargets(None, None);
            ctx.PSSetShaderResources(0, Some(&[None, None]));
        }
        Ok(())
    }

    /// Builds the cursor texture from a Desktop Duplication pointer shape.
    fn set_cursor_shape(&self, cur: &mut Cursor, info: &DXGI_OUTDUPL_POINTER_SHAPE_INFO, buf: &[u8]) -> Result<()> {
        let w = info.Width as usize;
        let pitch = info.Pitch as usize;
        let mono = info.Type == DXGI_OUTDUPL_POINTER_SHAPE_TYPE_MONOCHROME.0 as u32;
        let h = if mono { info.Height as usize / 2 } else { info.Height as usize };
        if w == 0 || h == 0 {
            cur.srv = None;
            return Ok(());
        }
        // RGBA, straight alpha. a == 0 with white rgb means "invert the screen here".
        let mut px = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let o = (y * w + x) * 4;
                let out: [u8; 4] = if mono {
                    let byte = x / 8;
                    let bit = 0x80 >> (x % 8);
                    let and = buf.get(y * pitch + byte).map_or(1, |b| (b & bit != 0) as u8);
                    let xor = buf.get((y + h) * pitch + byte).map_or(0, |b| (b & bit != 0) as u8);
                    match (and, xor) {
                        (0, 0) => [0, 0, 0, 255],
                        (0, _) => [255, 255, 255, 255],
                        (_, 0) => [0, 0, 0, 0],
                        _ => [255, 255, 255, 0],
                    }
                } else {
                    let i = y * pitch + x * 4;
                    let (b, g, r, a) = (buf[i], buf[i + 1], buf[i + 2], buf[i + 3]);
                    if info.Type == DXGI_OUTDUPL_POINTER_SHAPE_TYPE_COLOR.0 as u32 {
                        [r, g, b, a]
                    } else if a == 0 {
                        [r, g, b, 255] // masked colour: replace
                    } else if r == 0 && g == 0 && b == 0 {
                        [0, 0, 0, 0] // XOR with black: no change
                    } else {
                        [255, 255, 255, 0] // XOR with colour: approximate as invert
                    }
                };
                px[o..o + 4].copy_from_slice(&out);
            }
        }
        let desc = D3D11_TEXTURE2D_DESC {
            Width: w as u32,
            Height: h as u32,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_IMMUTABLE,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let init = D3D11_SUBRESOURCE_DATA { pSysMem: px.as_ptr() as *const c_void, SysMemPitch: (w * 4) as u32, SysMemSlicePitch: 0 };
        let mut t = None;
        let mut srv = None;
        unsafe {
            self.device.CreateTexture2D(&desc, Some(&init), Some(&mut t))?;
            self.device.CreateShaderResourceView(&t.unwrap(), None, Some(&mut srv))?;
        }
        cur.srv = srv;
        cur.size = (w as i32, h as i32);
        Ok(())
    }
}

struct Duplicator {
    output: IDXGIOutput1,
    device: ID3D11Device,
    dup: Option<IDXGIOutputDuplication>,
    holding: bool,
}

impl Duplicator {
    fn reopen(&mut self) -> Result<(u32, u32)> {
        self.dup = None;
        self.holding = false;
        // DuplicateOutput1 hands us the desktop in its own format (no hidden
        // conversion). Fall back to the old call if it isn't available.
        let formats = [DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R10G10B10A2_UNORM, DXGI_FORMAT_R16G16B16A16_FLOAT];
        let dup = match self.output.cast::<IDXGIOutput5>() {
            Ok(o5) => match unsafe { o5.DuplicateOutput1(&self.device, 0, &formats) } {
                Ok(d) => d,
                Err(e) => {
                    crate::log!("DuplicateOutput1 failed ({e}); using DuplicateOutput");
                    unsafe { self.output.DuplicateOutput(&self.device)? }
                }
            },
            Err(_) => unsafe { self.output.DuplicateOutput(&self.device)? },
        };
        let d = unsafe { dup.GetDesc() };
        self.dup = Some(dup);
        Ok((d.ModeDesc.Width, d.ModeDesc.Height))
    }

    fn release(&mut self) {
        if self.holding {
            if let Some(d) = &self.dup {
                unsafe {
                    let _ = d.ReleaseFrame();
                }
            }
            self.holding = false;
        }
    }
}

/// Runs the capture loop until `stop` is set or an unrecoverable error occurs.
/// `ready` is called once capture and encoder are up, with the shared timeline.
pub fn run(
    p: VideoParams,
    stop: Arc<AtomicBool>,
    stats: Arc<VideoStats>,
    ready: impl FnOnce(Timeline),
    mut sink: impl FnMut(Packet),
) -> Result<()> {
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST);
    }
    let (adapter, output, width, height) = find_output(p.monitor)?;
    if width % 2 != 0 || height % 2 != 0 {
        bail!("monitor size {width}x{height} must be even");
    }

    let mut device = None;
    let mut ctx = None;
    unsafe {
        D3D11CreateDevice(
            &adapter,
            D3D_DRIVER_TYPE_UNKNOWN,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut ctx),
        )
        .context("D3D11CreateDevice")?;
    }
    let device: ID3D11Device = device.ok_or_else(|| anyhow!("no device"))?;
    let ctx: ID3D11DeviceContext = ctx.ok_or_else(|| anyhow!("no context"))?;
    // By default Windows lets the foreground game win the GPU. We keep it that
    // way unless the user asks otherwise (settings: gpu_priority).
    if p.gpu_priority != 0 {
        if let Ok(dx) = device.cast::<IDXGIDevice>() {
            let r = unsafe { dx.SetGPUThreadPriority(p.gpu_priority) };
            crate::log!("GPU priority {}: {}", p.gpu_priority, if r.is_ok() { "set" } else { "refused" });
        }
    }

    let mut dupl = Duplicator { output, device: device.clone(), dup: None, holding: false };
    dupl.reopen().context("starting Desktop Duplication (is another app using it?)")?;

    let ten_bit = p.ten_bit && p.codec != 0;
    let mut gpu = Gpu::new(device.clone(), ctx.clone(), width, height, ten_bit)?;
    let depth = unsafe { nvs_depth() } as usize;
    // DEPTH in flight + one waiting for its slot + one being written.
    let pool: Vec<Surface> = (0..depth + 2).map(|_| gpu.make_surface()).collect::<Result<_>>()?;

    let mut raw = std::ptr::null_mut();
    let rc = unsafe {
        nvs_create(
            device.as_raw(),
            width,
            height,
            p.fps,
            p.codec,
            ten_bit as i32,
            p.preset as i32,
            p.aq as i32,
            p.quality,
            p.max_kbps,
            &mut raw,
        )
    };
    if rc != 0 {
        bail!("NVENC init failed (code {rc}). Is the NVIDIA driver up to date?");
    }
    let enc = Encoder(raw);
    stats.width.store(width as u64, Ordering::Relaxed);
    stats.height.store(height as u64, Ordering::Relaxed);

    let timer = unsafe {
        CreateWaitableTimerExW(None, PCWSTR::null(), CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, TIMER_ALL_ACCESS.0)?
    };
    let freq = qpc_freq();
    let tick = freq as f64 / p.fps as f64;
    let qpc0 = qpc_now();
    ready(Timeline { qpc0, qpc_freq: freq, fps: p.fps, width, height });
    crate::log!(
        "video: {width}x{height} at {} fps (fixed), codec {}{}, preset P{}, AQ {}, quality {}, cursor {}",
        p.fps,
        p.codec,
        if ten_bit { " 10-bit" } else { "" },
        p.preset,
        p.aq,
        p.quality,
        if p.cursor { "on" } else { "off" }
    );

    // Test switch: RB_STAGE=dup (capture only) or convert (capture + colour pass, no encode).
    let stage = std::env::var("RB_STAGE").unwrap_or_default();
    let (do_convert, do_encode) = match stage.as_str() {
        "dup" => (false, false),
        "convert" => (true, false),
        _ => (true, true),
    };
    if !stage.is_empty() {
        crate::log!("TEST STAGE: {stage}");
    }
    let key_every = p.fps as i64 * 2; // a keyframe every 2 s
    let mut k: i64 = 0; // next grid slot
    let mut next_key: i64 = 0;
    let mut latest: Option<usize> = None; // converted, not yet encoded
    let mut last_sent: Option<usize> = None;
    let mut recent: VecDeque<usize> = VecDeque::new(); // surfaces the encoder may still read
    let mut in_flight: VecDeque<(i64, bool)> = VecDeque::new();
    let mut cursor = Cursor::default();
    let mut shape_buf: Vec<u8> = Vec::new();
    let mut lost_since: Option<Instant> = None;

    let emit = |data: *const u8, size: u32, in_flight: &mut VecDeque<(i64, bool)>, sink: &mut dyn FnMut(Packet)| {
        if size == 0 {
            return;
        }
        let (pts, key) = in_flight.pop_front().unwrap_or((0, false));
        let bytes = unsafe { std::slice::from_raw_parts(data, size as usize) }.to_vec();
        stats.bytes.fetch_add(size as u64, Ordering::Relaxed);
        sink(Packet { pts, key, data: bytes });
    };

    while !stop.load(Ordering::Relaxed) {
        let deadline = qpc0 as f64 + k as f64 * tick;
        let now = qpc_now() as f64;

        if now < deadline {
            // Until the next grid slot: wait for desktop updates.
            let wait_ms = (((deadline - now) / freq as f64) * 1000.0).ceil().max(1.0) as u32;
            if dupl.dup.is_none() {
                if lost_since.is_none_or(|t| t.elapsed() > Duration::from_millis(250)) {
                    match dupl.reopen() {
                        Ok((w, h)) if w != width || h != height => bail!("screen size changed to {w}x{h}"),
                        Ok(_) => {
                            crate::log!("desktop duplication restored");
                            lost_since = None;
                        }
                        Err(_) => lost_since = Some(Instant::now()),
                    }
                }
                if dupl.dup.is_none() {
                    sleep_precise(timer, (deadline - now) / freq as f64);
                    continue;
                }
            }
            // Release the previous frame right before acquiring the next (as Microsoft recommends).
            dupl.release();
            let dup = dupl.dup.clone().unwrap();
            let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut res: Option<IDXGIResource> = None;
            match unsafe { dup.AcquireNextFrame(wait_ms, &mut info, &mut res) } {
                Ok(()) => {
                    dupl.holding = true;
                    let image_changed = info.LastPresentTime != 0;
                    let mut cursor_changed = false;
                    if p.cursor && info.LastMouseUpdateTime != 0 {
                        let pp = info.PointerPosition;
                        cursor.pos = (pp.Position.x, pp.Position.y);
                        cursor.visible = pp.Visible.as_bool();
                        cursor_changed = true;
                    }
                    if p.cursor && info.PointerShapeBufferSize > 0 {
                        shape_buf.resize(info.PointerShapeBufferSize as usize, 0);
                        let mut needed = 0u32;
                        let mut si = DXGI_OUTDUPL_POINTER_SHAPE_INFO::default();
                        let ok = unsafe {
                            dup.GetFramePointerShape(
                                shape_buf.len() as u32,
                                shape_buf.as_mut_ptr() as *mut c_void,
                                &mut needed,
                                &mut si,
                            )
                        };
                        if ok.is_ok() {
                            if let Err(e) = gpu.set_cursor_shape(&mut cursor, &si, &shape_buf[..needed as usize]) {
                                crate::log!("cursor shape: {e}");
                            }
                            cursor_changed = true;
                        }
                    }
                    if info.AccumulatedFrames > 1 {
                        stats.merged.fetch_add(info.AccumulatedFrames as u64 - 1, Ordering::Relaxed);
                    }
                    if do_convert && (image_changed || cursor_changed) {
                        if let Some(res) = res {
                            // Write into the waiting surface if there is one, else a free one.
                            let target = latest.unwrap_or_else(|| {
                                (0..pool.len())
                                    .find(|i| !recent.contains(i) && Some(*i) != last_sent)
                                    .unwrap_or(0)
                            });
                            let tex: ID3D11Texture2D = res.cast()?;
                            gpu.convert(&tex, &cursor, p.cursor, &pool[target])?;
                            latest = Some(target);
                            stats.new_frames.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => {}
                Err(e) => {
                    // ACCESS_LOST is routine: mode change, UAC/lock screen, fullscreen switch.
                    let what = if e.code() == DXGI_ERROR_ACCESS_LOST { "access lost".to_string() } else { e.to_string() };
                    crate::log!("desktop duplication interrupted ({what}); recreating");
                    dupl.dup = None;
                    dupl.holding = false;
                    gpu.src_cache = None;
                    lost_since = None;
                }
            }
            continue;
        }

        if !do_encode {
            k += 1;
            latest = None;
            continue;
        }
        // A grid slot is due. If we're more than one slot late, skip the missed ones.
        let behind = ((now - deadline) / tick) as i64;
        if behind > 0 {
            k += behind;
            stats.dropped.fetch_add(behind as u64, Ordering::Relaxed);
        }
        let (surf, repeat) = match latest.take() {
            Some(s) => (s, false),
            None => match last_sent {
                Some(s) => (s, true),
                None => {
                    k += 1; // nothing captured yet
                    continue;
                }
            },
        };
        let key = k >= next_key;
        if key {
            next_key = k + key_every;
        }
        let mut data = std::ptr::null();
        let mut size = 0u32;
        let rc = unsafe { nvs_encode(enc.0, pool[surf].tex.as_raw(), key as i32, &mut data, &mut size) };
        if rc != 0 {
            bail!("NVENC encode failed (code {rc})");
        }
        in_flight.push_back((k, key));
        emit(data, size, &mut in_flight, &mut sink);
        recent.push_back(surf);
        while recent.len() > depth {
            recent.pop_front();
        }
        last_sent = Some(surf);
        if repeat {
            stats.repeats.fetch_add(1, Ordering::Relaxed);
        }
        stats.frames.fetch_add(1, Ordering::Relaxed);
        k += 1;
    }

    // Flush the frames still inside the encoder.
    loop {
        let mut data = std::ptr::null();
        let mut size = 0u32;
        if unsafe { nvs_drain(enc.0, &mut data, &mut size) } != 1 {
            break;
        }
        emit(data, size, &mut in_flight, &mut sink);
    }
    dupl.release();
    drop(enc);
    unsafe {
        let _ = windows::Win32::Foundation::CloseHandle(timer);
    }
    Ok(())
}

fn sleep_precise(timer: HANDLE, secs: f64) {
    if secs <= 0.0 {
        return;
    }
    let due = -((secs * 10_000_000.0) as i64).max(1); // relative, 100 ns units
    unsafe {
        if SetWaitableTimer(timer, &due, 0, None, None, false).is_ok() {
            WaitForSingleObject(timer, 1000);
        } else {
            std::thread::sleep(Duration::from_secs_f64(secs));
        }
    }
}

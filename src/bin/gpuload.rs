//! Test tool: a game-like GPU load. Opens a flip-model D3D11 window, renders a
//! heavy shader as fast as the GPU allows, and writes each frame's end time
//! (Unix ms, fractional) to a file so recorder on/off runs can be compared.
//!
//! usage: gpuload <seconds> <iterations> <out.txt> [x y w h]

#![windows_subsystem = "windows"]

use std::io::Write;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{Interface, PCSTR, w};

const SHADER: &str = r#"
cbuffer C : register(b0) { float t; int iters; float2 pad; };
float4 vs(uint id : SV_VertexID) : SV_Position {
    float2 p = float2((id << 1) & 2, id & 2);
    return float4(p * float2(2, -2) + float2(-1, 1), 0, 1);
}
float4 ps(float4 pos : SV_Position) : SV_Target {
    float2 uv = pos.xy / float2(1920, 1080);
    float3 col = 0;
    for (int i = 0; i < iters; i++) {
        float f = i;
        uv = frac(uv * 1.01 + float2(sin(t + f * 0.1), cos(t * 0.7 + f * 0.13)) * 0.01);
        col += float3(uv, sin(t + f)) * 0.002;
    }
    return float4(col, 1);
}
"#;

unsafe extern "system" fn wndproc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(h, m, wp, lp) }
}

fn compile(entry: &[u8], target: &[u8]) -> ID3DBlob {
    let mut code = None;
    unsafe {
        D3DCompile(
            SHADER.as_ptr() as _,
            SHADER.len(),
            PCSTR::null(),
            None,
            None,
            PCSTR(entry.as_ptr()),
            PCSTR(target.as_ptr()),
            0,
            0,
            &mut code,
            None,
        )
        .unwrap();
    }
    code.unwrap()
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let secs: f64 = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(30.0);
    let iters: i32 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(1300);
    let out = a.get(3).cloned().unwrap_or("gpuload.txt".into());
    let g = |i: usize, d: i32| a.get(i).and_then(|s| s.parse().ok()).unwrap_or(d);
    let (x, y, ww, wh) = (g(4, 0), g(5, 0), g(6, 1920), g(7, 1080));
    unsafe {
        let _ = windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        );
        let hinst = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap();
        let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: hinst.into(), lpszClassName: w!("GpuLoad"), ..Default::default() };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("GpuLoad"),
            w!("GPU load test"),
            WS_POPUP | WS_VISIBLE,
            x,
            y,
            ww,
            wh,
            None,
            None,
            Some(hinst.into()),
            None,
        )
        .unwrap();

        let sd = DXGI_SWAP_CHAIN_DESC {
            BufferDesc: DXGI_MODE_DESC { Width: 1920, Height: 1080, Format: DXGI_FORMAT_B8G8R8A8_UNORM, ..Default::default() },
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            OutputWindow: hwnd,
            Windowed: true.into(),
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
            Flags: DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING.0 as u32,
        };
        let (mut sc, mut dev, mut ctx) = (None, None, None);
        D3D11CreateDeviceAndSwapChain(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_FLAG(0),
            None,
            D3D11_SDK_VERSION,
            Some(&sd),
            Some(&mut sc),
            Some(&mut dev),
            None,
            Some(&mut ctx),
        )
        .unwrap();
        let (sc, dev, ctx): (IDXGISwapChain, ID3D11Device, ID3D11DeviceContext) = (sc.unwrap(), dev.unwrap(), ctx.unwrap());
        let bb: ID3D11Texture2D = sc.GetBuffer(0).unwrap();
        let mut rtv = None;
        dev.CreateRenderTargetView(&bb, None, Some(&mut rtv)).unwrap();
        let (vsb, psb) = (compile(b"vs\0", b"vs_5_0\0"), compile(b"ps\0", b"ps_5_0\0"));
        let sl = |b: &ID3DBlob| std::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize());
        let (mut vs, mut ps, mut cb) = (None, None, None);
        dev.CreateVertexShader(sl(&vsb), None, Some(&mut vs)).unwrap();
        dev.CreatePixelShader(sl(&psb), None, Some(&mut ps)).unwrap();
        let bd = D3D11_BUFFER_DESC { ByteWidth: 16, Usage: D3D11_USAGE_DEFAULT, BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32, ..Default::default() };
        dev.CreateBuffer(&bd, None, Some(&mut cb)).unwrap();
        let cb = cb.unwrap();
        let _ = sc.cast::<IDXGISwapChain>();

        let mut f = std::io::BufWriter::new(std::fs::File::create(&out).unwrap());
        let start = std::time::Instant::now();
        let epoch = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64() * 1000.0;
        while start.elapsed().as_secs_f64() < secs {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                DispatchMessageW(&msg);
            }
            let t = start.elapsed().as_secs_f32();
            let data: [u32; 4] = [t.to_bits(), iters as u32, 0, 0];
            ctx.UpdateSubresource(&cb, 0, None, data.as_ptr() as _, 0, 0);
            ctx.OMSetRenderTargets(Some(&[rtv.clone()]), None);
            ctx.RSSetViewports(Some(&[D3D11_VIEWPORT { Width: 1920.0, Height: 1080.0, MaxDepth: 1.0, ..Default::default() }]));
            ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            ctx.VSSetShader(vs.as_ref(), None);
            ctx.PSSetShader(ps.as_ref(), None);
            ctx.PSSetConstantBuffers(0, Some(&[Some(cb.clone())]));
            ctx.Draw(3, 0);
            let _ = sc.Present(0, DXGI_PRESENT_ALLOW_TEARING);
            writeln!(f, "{:.3}", epoch + start.elapsed().as_secs_f64() * 1000.0).unwrap();
        }
    }
}

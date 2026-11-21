use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use image::{codecs::jpeg::JpegEncoder, imageops::{self, FilterType}, RgbImage};
use std::{cell::RefCell, thread::sleep, time::Duration};
use windows::core::Interface;
use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureAccess, GraphicsCaptureAccessKind, GraphicsCaptureItem};
use windows::Graphics::DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat};
use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;

const WIDTH: u32 = 240;
const MIN_MINIMIZED: u64 = 10;
const TRIM_EVERY: u64 = 300;

#[derive(Clone)]
struct Gpu { device: ID3D11Device, ctx: ID3D11DeviceContext, winrt: IDirect3DDevice }

thread_local! { static GPU: RefCell<Option<Gpu>> = const { RefCell::new(None) }; }

fn gpu() -> Result<Gpu> {
  GPU.with(|g| {
    if let Some(gpu) = g.borrow().as_ref() {
      return Ok(gpu.clone());
    }
    let (mut device, mut ctx) = (None, None);
    unsafe { D3D11CreateDevice(None, D3D_DRIVER_TYPE_HARDWARE, HMODULE::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut device), None, Some(&mut ctx))? };
    let device: ID3D11Device = device.context("no D3D11 device")?;
    let winrt: IDirect3DDevice = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&device.cast::<IDXGIDevice>()?)? }.cast()?;
    // Without this, Windows draws a yellow capture border around the Roblox window.
    let _ = GraphicsCaptureAccess::RequestAccessAsync(GraphicsCaptureAccessKind::Borderless).and_then(|op| op.join());
    let gpu = Gpu { device, ctx: ctx.context("no D3D11 context")?, winrt };
    *g.borrow_mut() = Some(gpu.clone());
    Ok(gpu)
  })
}

/// One BGRA frame of `hwnd` via Windows.Graphics.Capture. PrintWindow made the whole screen flash on Roblox.
fn grab(hwnd: HWND) -> Result<(u32, u32, Vec<u8>)> {
  let gpu = gpu()?;
  let item: GraphicsCaptureItem = unsafe { windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?.CreateForWindow(hwnd)? };
  let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(&gpu.winrt, DirectXPixelFormat::B8G8R8A8UIntNormalized, 1, item.Size()?)?;
  let session = pool.CreateCaptureSession(&item)?;
  let _ = session.SetIsBorderRequired(false);
  let _ = session.SetIsCursorCaptureEnabled(false);
  session.StartCapture()?;
  let frame = (0..50).find_map(|_| pool.TryGetNextFrame().ok().or_else(|| { sleep(Duration::from_millis(10)); None }));
  let _ = session.Close();
  let frame = frame.context("no frame")?;
  let tex: ID3D11Texture2D = unsafe { frame.Surface()?.cast::<IDirect3DDxgiInterfaceAccess>()?.GetInterface()? };
  let mut desc = D3D11_TEXTURE2D_DESC::default();
  unsafe { tex.GetDesc(&mut desc) };
  desc.Usage = D3D11_USAGE_STAGING;
  desc.BindFlags = 0;
  desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
  desc.MiscFlags = 0;
  let mut staging = None;
  unsafe { gpu.device.CreateTexture2D(&desc, None, Some(&mut staging))? };
  let staging = staging.context("no staging texture")?;
  let mut map = D3D11_MAPPED_SUBRESOURCE::default();
  let (w, h) = (desc.Width, desc.Height);
  let mut bgra = Vec::with_capacity(w as usize * h as usize * 4);
  // SAFETY: the mapped rows are RowPitch apart and each holds at least w*4 bytes; unmapped before returning.
  unsafe {
    gpu.ctx.CopyResource(&staging, &tex);
    gpu.ctx.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut map))?;
    for y in 0..h as usize {
      bgra.extend_from_slice(std::slice::from_raw_parts((map.pData as *const u8).add(y * map.RowPitch as usize), w as usize * 4));
    }
    gpu.ctx.Unmap(&staging, 0);
  }
  let _ = frame.Close();
  let _ = pool.Close();
  Ok((w, h, bgra))
}

/// Window `hwnd` as a 240px-wide JPEG data URL.
pub fn capture_data_url(hwnd: isize) -> Result<String> {
  let (w, h, bgra) = grab(HWND(hwnd as *mut _))?;
  if w == 0 || h == 0 {
    bail!("empty window");
  }
  let rgb: Vec<u8> = bgra.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0]]).collect();
  if is_black(&rgb) {
    bail!("black");
  }
  let img = RgbImage::from_raw(w as u32, h as u32, rgb).unwrap();
  let small = imageops::resize(&img, WIDTH, (WIDTH * h as u32 / w as u32).max(1), FilterType::Triangle);
  let mut jpeg = Vec::new();
  JpegEncoder::new_with_quality(&mut jpeg, 70).encode_image(&small)?;
  Ok(format!("data:image/jpeg;base64,{}", STANDARD.encode(jpeg)))
}

/// Every byte near zero: the capture returned nothing drawn.
pub fn is_black(rgb: &[u8]) -> bool {
  rgb.iter().all(|&b| b < 8)
}

/// Trim a window minimized >=10s, then at most every 300s (sooner if re-minimized since the last trim).
pub fn should_trim(now: u64, minimized_since: Option<u64>, last_trim: Option<u64>) -> bool {
  let Some(since) = minimized_since else { return false };
  now.saturating_sub(since) >= MIN_MINIMIZED
    && last_trim.is_none_or(|t| t < since || now.saturating_sub(t) >= TRIM_EVERY)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn black_detection() {
    assert!(is_black(&[0; 300]));
    assert!(!is_black(&[0, 0, 200]));
  }

  #[test]
  fn trim_timing() {
    assert!(!should_trim(100, None, None));
    assert!(!should_trim(105, Some(100), None));
    assert!(should_trim(110, Some(100), None));
    assert!(!should_trim(300, Some(100), Some(110)));
    assert!(should_trim(410, Some(100), Some(110)));
    assert!(should_trim(520, Some(500), Some(110)));
  }
}

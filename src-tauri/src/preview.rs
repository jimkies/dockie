use anyhow::{Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use image::{codecs::jpeg::JpegEncoder, imageops::{self, FilterType}, RgbImage};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;

const WIDTH: u32 = 240;
const MIN_MINIMIZED: u64 = 10;
const TRIM_EVERY: u64 = 300;

/// Client area of `hwnd` as a 240px-wide JPEG data URL.
pub fn capture_data_url(hwnd: isize) -> Result<String> {
  let hwnd = HWND(hwnd as *mut _);
  let mut rect = RECT::default();
  unsafe { GetClientRect(hwnd, &mut rect)? };
  let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
  if w <= 0 || h <= 0 {
    bail!("empty window");
  }
  let mut bgra = vec![0u8; w as usize * h as usize * 4];
  // SAFETY: every GDI object created here is released below on all paths.
  let ok = unsafe {
    let screen = GetDC(None);
    let dc = CreateCompatibleDC(Some(screen));
    let bmp = CreateCompatibleBitmap(screen, w, h);
    let old = SelectObject(dc, bmp.into());
    // 2 = PW_RENDERFULLCONTENT, so GPU-drawn windows render too
    let printed = PrintWindow(hwnd, dc, PRINT_WINDOW_FLAGS(2)).as_bool();
    let mut bi = BITMAPINFO {
      bmiHeader: BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: w,
        biHeight: -h, // negative = top-down
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
      },
      ..Default::default()
    };
    let lines = GetDIBits(dc, bmp, 0, h as u32, Some(bgra.as_mut_ptr().cast()), &mut bi, DIB_RGB_COLORS);
    SelectObject(dc, old);
    let _ = DeleteObject(bmp.into());
    let _ = DeleteDC(dc);
    ReleaseDC(None, screen);
    printed && lines != 0
  };
  if !ok {
    bail!("capture failed");
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

/// Every byte near zero: PrintWindow returned nothing drawn.
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

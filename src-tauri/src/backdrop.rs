//! Wallpaper sampling for the notch's glass theme.
//!
//! Windows' Mica material is the desktop wallpaper, heavily blurred and
//! tinted; it never shows the windows behind. The overlay is layered and
//! click-through, so DWM can't give it a real backdrop, but the same look
//! comes from painting the wallpaper (`PaintDesktop`, which applies the
//! user's fill/fit/span mode) and averaging the part behind the overlay down
//! to a tiny image. The UI stretches it, which blurs it the way Mica does,
//! and tints it. The detached window uses the same sample and tint, so the
//! two surfaces always match.

use serde::Serialize;

/// A tiny RGB image of a whole monitor's wallpaper. Windows position it by
/// their own offset into the monitor, so moving a window only shifts the
/// image; nothing is re-sampled.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Backdrop {
    pub width: u32,
    pub height: u32,
    /// Row-major RGB triples.
    pub pixels: Vec<u8>,
    /// The monitor this covers, in physical screen pixels.
    pub monitor: (i32, i32, u32, u32),
    /// The monitor's scale factor, to convert to CSS pixels.
    pub scale: f64,
    /// The picked image this was made from, or `None` for the live desktop.
    /// Lets the UI tell a stale sample from the one its settings ask for.
    pub image: Option<String>,
}

/// Physical pixels per sample cell. A fixed density (not a fixed column
/// count) keeps the blur identical on the notch and on a window of any size.
const CELL: usize = 31;

/// Sample grid for a region of `w` × `h` physical pixels.
pub(crate) fn grid(w: usize, h: usize) -> (usize, usize) {
    ((w / CELL).clamp(4, 160), (h / CELL).clamp(3, 120))
}

/// How a pixel buffer is laid out for [`downsample`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct Layout {
    /// Bytes per pixel.
    bpp: usize,
    /// Byte offsets of red, green and blue within a pixel.
    rgb: [usize; 3],
    /// Read every `step`th pixel in each direction.
    step: usize,
}

/// GDI's 32-bit DIBs. PERF: every 4th pixel is plenty for an average this
/// coarse over a full-resolution monitor.
pub(crate) const BGRA: Layout = Layout { bpp: 4, rgb: [2, 1, 0], step: 4 };
/// Already-shrunk RGB images, where every pixel counts.
pub(crate) const RGB: Layout = Layout { bpp: 3, rgb: [0, 1, 2], step: 1 };

/// Box-averages a region of `px` down to `out_w` × `out_h` RGB.
pub(crate) fn downsample(
    px: &[u8],
    layout: Layout,
    stride_px: usize,
    region: (usize, usize, usize, usize),
    out_w: usize,
    out_h: usize,
) -> Vec<u8> {
    let (rx, ry, rw, rh) = region;
    let [ri, gi, bi] = layout.rgb;
    let mut out = Vec::with_capacity(out_w * out_h * 3);
    for oy in 0..out_h {
        let y0 = ry + oy * rh / out_h;
        let y1 = (ry + (oy + 1) * rh / out_h).max(y0 + 1);
        for ox in 0..out_w {
            let x0 = rx + ox * rw / out_w;
            let x1 = (rx + (ox + 1) * rw / out_w).max(x0 + 1);
            let (mut r, mut g, mut b, mut n) = (0u64, 0u64, 0u64, 0u64);
            for y in (y0..y1).step_by(layout.step) {
                for x in (x0..x1).step_by(layout.step) {
                    let i = (y * stride_px + x) * layout.bpp;
                    if let Some(p) = px.get(i..i + layout.bpp) {
                        r += p[ri] as u64;
                        g += p[gi] as u64;
                        b += p[bi] as u64;
                        n += 1;
                    }
                }
            }
            let n = n.max(1);
            out.extend_from_slice(&[(r / n) as u8, (g / n) as u8, (b / n) as u8]);
        }
    }
    out
}

/// Samples the wallpaper under `window` on `monitor`; both are
/// `(x, y, width, height)` in physical screen pixels.
#[cfg(windows)]
pub fn sample(window: (i32, i32, u32, u32), monitor: (i32, i32, u32, u32)) -> Option<Backdrop> {
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, PaintDesktop, ReleaseDC,
        SelectObject, SetViewportOrgEx, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    };

    let (wx, wy, ww, wh) = window;
    let (mx, my, mw, mh) = monitor;
    if ww == 0 || wh == 0 || mw == 0 || mh == 0 {
        return None;
    }
    // Paint the whole monitor: PaintDesktop lays the wallpaper out per
    // monitor, so painting just the window's rect would misplace it.
    let (w, h) = (mw as i32, mh as i32);
    unsafe {
        let screen = GetDC(None);
        let dc = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(dc, bmp.into());
        // Shift so screen point (mx, my) lands at the bitmap's origin.
        let _ = SetViewportOrgEx(dc, -mx, -my, None);
        let painted = PaintDesktop(dc).as_bool();
        let _ = SetViewportOrgEx(dc, 0, 0, None);

        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // top-down rows
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let lines = GetDIBits(dc, bmp, 0, h as u32, Some(buf.as_mut_ptr().cast()), &mut info, DIB_RGB_COLORS);

        SelectObject(dc, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(dc);
        ReleaseDC(None, screen);
        if !painted || lines == 0 {
            return None;
        }

        // The window's rect inside the monitor bitmap, clamped to it.
        let rx = (wx - mx).clamp(0, w - 1) as usize;
        let ry = (wy - my).clamp(0, h - 1) as usize;
        let rw = (ww as usize).min(w as usize - rx);
        let rh = (wh as usize).min(h as usize - ry);
        let (out_w, out_h) = grid(rw, rh);
        Some(Backdrop {
            width: out_w as u32,
            height: out_h as u32,
            pixels: downsample(&buf, BGRA, w as usize, (rx, ry, rw, rh), out_w, out_h),
            monitor,
            scale: 1.0,
            image: None,
        })
    }
}

#[cfg(not(windows))]
pub fn sample(_window: (i32, i32, u32, u32), _monitor: (i32, i32, u32, u32)) -> Option<Backdrop> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn averages_a_region_down() {
        // 8×8 BGRA: left half red, right half blue.
        let mut px = Vec::new();
        for _y in 0..8 {
            for x in 0..8 {
                px.extend_from_slice(if x < 4 { &[0, 0, 255, 255] } else { &[255, 0, 0, 255] });
            }
        }
        let out = downsample(&px, BGRA, 8, (0, 0, 8, 8), 2, 1);
        assert_eq!(out, vec![255, 0, 0, 0, 0, 255]);
    }

    #[test]
    fn averages_rgb_pixels_exactly() {
        // 2×1 RGB: black then white averages to mid gray.
        let px = [0, 0, 0, 255, 255, 255];
        assert_eq!(downsample(&px, RGB, 2, (0, 0, 2, 1), 1, 1), vec![127, 127, 127]);
    }
}

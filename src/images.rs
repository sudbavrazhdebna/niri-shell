//! Decoding images (wallpaper, cover art) at the size they are shown, on any thread.
//!
//! Not through gdk-pixbuf: on this system it hands every image to glycin, which starts a
//! sandboxed loader process each time (~400 ms for the wallpaper's size alone, the
//! processes lingering after). The `image` crate decodes in this process.

use std::path::Path;

use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;

/// How the image meets the size asked for.
#[derive(Clone, Copy)]
pub enum Fit {
    /// Covers it (the shown picture crops the rest): wallpapers.
    Cover,
    /// Fits inside it: cover art.
    Within,
}

/// Decoded RGBA pixels: plain data, so they can come from another thread.
pub struct Pixels {
    width: u32,
    height: u32,
    bytes: glib::Bytes,
}

impl Pixels {
    /// On the main thread.
    pub fn texture(&self) -> Option<gdk::Texture> {
        let (w, h) = (i32::try_from(self.width).ok()?, i32::try_from(self.height).ok()?);
        let stride = usize::try_from(self.width).ok()?.checked_mul(4)?;
        Some(gdk::MemoryTexture::new(w, h, gdk::MemoryFormat::R8g8b8a8, &self.bytes, stride).upcast())
    }
}

/// How much smaller than shown the blurred copy is kept: blurred, it loses nothing that
/// GTK's smooth scaling up doesn't bring back, and takes 1/16 of the memory.
const BLUR_SHRINK: u32 = 4;

impl Pixels {
    /// A blurred copy, `radius` screen pixels strong, at 1/`BLUR_SHRINK` the size (shown
    /// stretched). Three box blurs, close to a Gaussian; at the borders the edge pixels
    /// repeat, so the edges don't darken as a blur with nothing beyond them does.
    pub fn blurred(&self, radius: u32) -> Option<Pixels> {
        let full = image::RgbaImage::from_raw(self.width, self.height, self.bytes.to_vec())?;
        let (w, h) = ((self.width / BLUR_SHRINK).max(1), (self.height / BLUR_SHRINK).max(1));
        let mut small = image::DynamicImage::ImageRgba8(full).thumbnail_exact(w, h).into_rgba8();
        let r = (radius / BLUR_SHRINK).max(1) as usize;
        let (w, h) = (w as usize, h as usize);
        let buf: &mut [u8] = &mut small;
        for _ in 0..3 {
            box_pass(buf, w, h, r, 4, w * 4);
            box_pass(buf, h, w, r, w * 4, 4);
        }
        let (width, height) = (small.width(), small.height());
        Some(Pixels { width, height, bytes: glib::Bytes::from_owned(small.into_raw()) })
    }
}

/// One box blur along lines of `len` pixels (`step` bytes apart), `lines` of them
/// (`stride` bytes apart), edge pixels repeated beyond the ends. A running sum: the
/// cost doesn't grow with the radius.
fn box_pass(buf: &mut [u8], len: usize, lines: usize, r: usize, step: usize, stride: usize) {
    let window = (2 * r + 1) as u32;
    let mut line = vec![[0u8; 4]; len];
    for l in 0..lines {
        let base = l * stride;
        for (i, px) in line.iter_mut().enumerate() {
            let at = base + i * step;
            px.copy_from_slice(&buf[at..at + 4]);
        }
        let get = |i: isize| line[i.clamp(0, len as isize - 1) as usize];
        let mut sum = [0u32; 4];
        for i in -(r as isize)..=(r as isize) {
            let p = get(i);
            for c in 0..4 {
                sum[c] += u32::from(p[c]);
            }
        }
        for i in 0..len {
            let at = base + i * step;
            for c in 0..4 {
                buf[at + c] = (sum[c] / window) as u8;
            }
            let (add, sub) = (get((i + r + 1) as isize), get(i as isize - r as isize));
            for c in 0..4 {
                sum[c] = sum[c] + u32::from(add[c]) - u32::from(sub[c]);
            }
        }
    }
}

/// The image at `path`, scaled down (never up) to `width`×`height` as `fit` says.
pub fn decode(path: &Path, width: u32, height: u32, fit: Fit) -> Option<Pixels> {
    let image = image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .decode()
        .inspect_err(|err| tracing::warn!("{}: {err}", path.display()))
        .ok()?;
    let (iw, ih) = (image.width(), image.height());
    if iw == 0 || ih == 0 || width == 0 || height == 0 {
        return None;
    }
    let (sx, sy) = (f64::from(width) / f64::from(iw), f64::from(height) / f64::from(ih));
    let scale = match fit {
        Fit::Cover => sx.max(sy),
        Fit::Within => sx.min(sy),
    }
    .min(1.0);
    let (w, h) = (
        ((f64::from(iw) * scale).ceil() as u32).max(1),
        ((f64::from(ih) * scale).ceil() as u32).max(1),
    );
    // A box filter: fast, and fine for scaling down.
    let image = if (w, h) == (iw, ih) { image } else { image.thumbnail_exact(w, h) };
    let rgba = image.into_rgba8();
    Some(Pixels { width: rgba.width(), height: rgba.height(), bytes: glib::Bytes::from_owned(rgba.into_raw()) })
}


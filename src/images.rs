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


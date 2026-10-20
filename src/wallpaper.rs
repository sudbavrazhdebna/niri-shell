//! The wallpaper: one image on every monitor, filling it (cropped, never stretched), on
//! the background layer. The image is decoded at the size it is shown, off the main
//! thread: a 6000×4000 photo would take ~96 MB as it is and freeze the shell while
//! decoding. Monitors that come and go get theirs; a new image in the config replaces it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

#[derive(Default)]
struct State {
    image: Option<PathBuf>,
    windows: Vec<(gdk::Monitor, gtk4::Window)>,
    /// Decoded images by shown size, for monitors of the same size.
    textures: HashMap<(i32, i32), gdk::Texture>,
    following: bool,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::default();
}

/// Shows `image` on every monitor (at start and on config reload); None removes it.
pub fn configure(image: Option<PathBuf>) {
    let (changed, first) = STATE.with_borrow_mut(|state| {
        let changed = state.image != image;
        if changed {
            state.image = image;
            state.textures.clear();
        }
        (changed, !std::mem::replace(&mut state.following, true))
    });
    let Some(display) = gdk::Display::default() else { return };
    if first {
        display.monitors().connect_items_changed(|monitors, _, _, _| sync(monitors));
    }
    if changed {
        // Taken out of the cell first: destroying a window runs handlers.
        let old = STATE.with_borrow_mut(|state| std::mem::take(&mut state.windows));
        for (_, window) in old {
            window.destroy();
        }
        sync(&display.monitors());
    }
}

/// A window for each monitor without one; none for monitors that are gone.
fn sync(monitors: &gio::ListModel) {
    let current: Vec<gdk::Monitor> =
        (0..monitors.n_items()).filter_map(|i| monitors.item(i).and_downcast::<gdk::Monitor>()).collect();
    let gone: Vec<gtk4::Window> = STATE.with_borrow_mut(|state| {
        let (keep, gone) = std::mem::take(&mut state.windows).into_iter().partition(|(m, _)| current.contains(m));
        state.windows = keep;
        gone.into_iter().map(|(_, w)| w).collect()
    });
    for window in gone {
        window.destroy();
    }
    let Some(image) = STATE.with_borrow(|state| state.image.clone()) else { return };
    for monitor in current {
        if STATE.with_borrow(|state| state.windows.iter().any(|(m, _)| *m == monitor)) {
            continue;
        }
        glib::spawn_future_local(show(monitor, image.clone()));
    }
}

async fn show(monitor: gdk::Monitor, image: PathBuf) {
    let geometry = monitor.geometry();
    // Device pixels: a scaled monitor shows more of them than its logical size.
    let scale = monitor.scale_factor().max(1);
    let size = (geometry.width() * scale, geometry.height() * scale);
    if size.0 <= 0 || size.1 <= 0 {
        return;
    }
    let cached = STATE.with_borrow(|state| state.textures.get(&size).cloned());
    let texture = match cached {
        Some(texture) => texture,
        None => {
            let path = image.clone();
            let decoded = gio::spawn_blocking(move || decode(&path, size)).await;
            let Some(texture) = decoded.ok().flatten().and_then(|pixels| pixels.texture()) else {
                tracing::warn!("wallpaper: can't load {}", image.display());
                return;
            };
            STATE.with_borrow_mut(|state| state.textures.insert(size, texture.clone()));
            texture
        }
    };
    // Still wanted: the image unchanged, the monitor still there and without one.
    let wanted = STATE.with_borrow(|state| {
        state.image.as_ref() == Some(&image) && !state.windows.iter().any(|(m, _)| *m == monitor)
    });
    let present = gdk::Display::default().is_some_and(|d| {
        let list = d.monitors();
        (0..list.n_items()).any(|i| list.item(i).and_downcast::<gdk::Monitor>().as_ref() == Some(&monitor))
    });
    if !wanted || !present {
        return;
    }
    let window = window(&monitor, &texture);
    STATE.with_borrow_mut(|state| state.windows.push((monitor, window)));
}

fn window(monitor: &gdk::Monitor, texture: &gdk::Texture) -> gtk4::Window {
    let window = gtk4::Window::new();
    window.add_css_class("nshell-wallpaper");
    window.init_layer_shell();
    window.set_namespace(Some("nshell-wallpaper"));
    window.set_layer(Layer::Background);
    window.set_monitor(Some(monitor));
    window.set_keyboard_mode(KeyboardMode::None);
    for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        window.set_anchor(edge, true);
    }
    // The whole screen, under the bar too.
    window.set_exclusive_zone(-1);
    let picture = gtk4::Picture::for_paintable(texture);
    picture.set_content_fit(gtk4::ContentFit::Cover);
    picture.set_can_shrink(true);
    window.set_child(Some(&picture));
    window.set_visible(true);
    window
}

/// Decoded pixels; plain data, so they can come from another thread.
struct Pixels {
    width: i32,
    height: i32,
    stride: usize,
    alpha: bool,
    bytes: glib::Bytes,
}

impl Pixels {
    fn texture(self) -> Option<gdk::Texture> {
        let format = if self.alpha { gdk::MemoryFormat::R8g8b8a8 } else { gdk::MemoryFormat::R8g8b8 };
        let texture = gdk::MemoryTexture::new(self.width, self.height, format, &self.bytes, self.stride);
        Some(texture.upcast())
    }
}

/// Decodes the image just large enough to cover `size` (aspect kept; the picture crops
/// the rest). JPEG decodes at a fraction of its size directly, which is much faster.
fn decode(path: &Path, (width, height): (i32, i32)) -> Option<Pixels> {
    let (_, image_width, image_height) = gtk4::gdk_pixbuf::Pixbuf::file_info(path)?;
    if image_width <= 0 || image_height <= 0 {
        return None;
    }
    let scale = (f64::from(width) / f64::from(image_width))
        .max(f64::from(height) / f64::from(image_height))
        .min(1.0);
    let (w, h) = (
        (f64::from(image_width) * scale).ceil() as i32,
        (f64::from(image_height) * scale).ceil() as i32,
    );
    let pixbuf = gtk4::gdk_pixbuf::Pixbuf::from_file_at_scale(path, w, h, true).ok()?;
    Some(Pixels {
        width: pixbuf.width(),
        height: pixbuf.height(),
        stride: usize::try_from(pixbuf.rowstride()).ok()?,
        alpha: pixbuf.has_alpha(),
        bytes: pixbuf.read_pixel_bytes(),
    })
}

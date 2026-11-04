//! The wallpaper: one image on every monitor, filling it (cropped, never stretched), on
//! the background layer. The image is decoded at the size it is shown, off the main
//! thread (`images`): a 6000×4000 photo would take ~96 MB as it is and freeze the shell
//! while decoding. Monitors that come and go get theirs; a new image in the config replaces it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

#[derive(Default)]
struct State {
    image: Option<PathBuf>,
    /// The overview's blur, in pixels; 0: none.
    blur: u32,
    windows: Vec<(gdk::Monitor, gtk4::Window)>,
    /// Decoded images (and their blurred copies) by shown size, for monitors of the same
    /// size.
    textures: HashMap<(i32, i32), (gdk::Texture, Option<gdk::Texture>)>,
    following: bool,
    /// Monitors whose size isn't known yet (just plugged in or turned on).
    waiting: Vec<gdk::Monitor>,
    /// niri's overview is open: the wallpaper windows have the `overview` class (blurred
    /// in style.css).
    overview: bool,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::default();
}

/// Shows `image` on every monitor (at start and on config reload); None removes it.
/// `blur`: how strong the overview's blurred copy is, in pixels (0: none).
pub fn configure(image: Option<PathBuf>, blur: u32) {
    let (changed, first) = STATE.with_borrow_mut(|state| {
        let changed = state.image != image || state.blur != blur;
        if changed {
            state.image = image;
            state.blur = blur;
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

/// Marks the wallpaper with `overview` while niri's overview is open: its blurred copy
/// fades in.
pub fn follow_overview(niri: &crate::niri::Niri) {
    niri.state.subscribe(|state| {
        let open = state.overview_open;
        let windows = STATE.with_borrow_mut(|s| {
            if std::mem::replace(&mut s.overview, open) == open {
                return Vec::new();
            }
            s.windows.iter().map(|(_, w)| w.clone()).collect()
        });
        for window in windows {
            mark(&window, open);
        }
        true
    });
}

fn mark(window: &gtk4::Window, overview: bool) {
    if overview {
        window.add_css_class("overview");
    } else {
        window.remove_css_class("overview");
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
        // A monitor just turned on tells its size a moment later: the laptop screen came
        // back after sleep without its wallpaper. Try again once it does.
        let first = STATE.with_borrow_mut(|state| {
            let first = !state.waiting.contains(&monitor);
            if first {
                state.waiting.push(monitor.clone());
            }
            first
        });
        if first {
            tracing::info!(output = monitor.connector().as_deref().unwrap_or("?"), "wallpaper: waiting for the monitor's size");
            monitor.connect_geometry_notify(move |monitor| {
                let was_waiting = STATE.with_borrow_mut(|state| {
                    let before = state.waiting.len();
                    state.waiting.retain(|m| m != monitor);
                    state.waiting.len() != before
                });
                if was_waiting && let Some(display) = gdk::Display::default() {
                    sync(&display.monitors());
                }
            });
        }
        return;
    }
    let (cached, blur) = STATE.with_borrow(|state| (state.textures.get(&size).cloned(), state.blur));
    let (texture, blurred) = match cached {
        Some(textures) => textures,
        None => {
            let path = image.clone();
            let (w, h) = (u32::try_from(size.0).unwrap_or(0), u32::try_from(size.1).unwrap_or(0));
            // The blurred copy is made here too, once: the overview only fades it in.
            let decoded = gio::spawn_blocking(move || {
                let pixels = crate::images::decode(&path, w, h, crate::images::Fit::Cover)?;
                let blurred = (blur > 0).then(|| pixels.blurred(blur)).flatten();
                Some((pixels, blurred))
            })
            .await;
            let Some((pixels, blurred)) = decoded.ok().flatten() else {
                tracing::warn!("wallpaper: can't load {}", image.display());
                return;
            };
            let Some(texture) = pixels.texture() else { return };
            let blurred = blurred.and_then(|b| b.texture());
            STATE.with_borrow_mut(|state| state.textures.insert(size, (texture.clone(), blurred.clone())));
            (texture, blurred)
        }
    };
    // Still wanted: the image unchanged, the monitor still there and without one.
    let wanted = STATE.with_borrow(|state| {
        state.image.as_ref() == Some(&image) && state.blur == blur && !state.windows.iter().any(|(m, _)| *m == monitor)
    });
    let present = gdk::Display::default().is_some_and(|d| {
        let list = d.monitors();
        (0..list.n_items()).any(|i| list.item(i).and_downcast::<gdk::Monitor>().as_ref() == Some(&monitor))
    });
    if !wanted || !present {
        return;
    }
    let window = window(&monitor, &texture, blurred.as_ref());
    STATE.with_borrow_mut(|state| state.windows.push((monitor, window)));
}

fn window(monitor: &gdk::Monitor, texture: &gdk::Texture, blurred: Option<&gdk::Texture>) -> gtk4::Window {
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
    let picture = |texture: &gdk::Texture| {
        let picture = gtk4::Picture::for_paintable(texture);
        picture.set_content_fit(gtk4::ContentFit::Cover);
        picture.set_can_shrink(true);
        picture
    };
    // The blurred copy over the image, see-through until the overview (style.css).
    let stack = gtk4::Overlay::new();
    stack.set_child(Some(&picture(texture)));
    if let Some(blurred) = blurred {
        let over = picture(blurred);
        over.add_css_class("wallpaper-blurred");
        stack.add_overlay(&over);
    }
    window.set_child(Some(&stack));
    mark(&window, STATE.with_borrow(|s| s.overview));
    window.set_visible(true);
    window
}

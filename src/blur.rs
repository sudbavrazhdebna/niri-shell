//! Compositor blur behind parts of a surface (`ext-background-effect-v1`, niri 26.04+).
//!
//! The shell says exactly which area to blur, so rounded cards keep sharp shadows and
//! corners. A protocol error kills the whole Wayland connection, so a [`Blur`] must be
//! dropped before its surface goes away: create it on map, drop it on unmap.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::gdk;
use gtk4::prelude::*;
use wayland_protocols::ext::background_effect::v1::client::ext_background_effect_surface_v1::ExtBackgroundEffectSurfaceV1;

use crate::wayland;

/// How far inside a card's edge the blur stops.
const INSET: i32 = 1;

/// A rectangle in surface coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Blur request for one surface; removed when dropped.
pub struct Blur {
    effect: ExtBackgroundEffectSurfaceV1,
}

impl Blur {
    /// None if the compositor can't blur or the surface isn't on Wayland.
    pub fn new(surface: &gdk::Surface) -> Option<Blur> {
        let wl_surface = wayland::wl_surface(surface)?;
        wayland::with(|g| {
            let effect = g.blur.as_ref()?.get_background_effect(&wl_surface, &g.qh, ());
            Some(Blur { effect })
        })
    }

    /// Blurs behind the union of `rects` (empty: nothing). Takes effect with the
    /// surface's next commit, which this asks GDK for.
    pub fn set_region(&self, surface: &gdk::Surface, rects: &[Rect]) {
        wayland::with(|g| {
            if rects.is_empty() {
                self.effect.set_blur_region(None);
            } else {
                let region = g.compositor.create_region(&g.qh, ());
                for r in rects {
                    region.add(r.x, r.y, r.width, r.height);
                }
                self.effect.set_blur_region(Some(&region));
                // Regions are copied on use.
                region.destroy();
            }
            Some(())
        });
        surface.queue_render();
    }
}

impl Drop for Blur {
    fn drop(&mut self) {
        self.effect.destroy();
        wayland::with(|_| Some(()));
    }
}

/// Wraps `content` so that the area behind `cards` (rounded by `radius`) stays blurred,
/// following every change in their size or number. Set the returned widget as the
/// window's child. `on_resize` runs on each size change too (e.g. to reposition).
///
/// The blur lives from map to unmap, never longer than the surface.
pub fn behind(
    window: &gtk4::Window,
    content: &impl IsA<gtk4::Widget>,
    cards: impl Fn() -> Vec<gtk4::Widget> + 'static,
    radius: i32,
    on_resize: impl Fn() + 'static,
) -> gtk4::Overlay {
    let overlay = gtk4::Overlay::new();
    overlay.set_child(Some(content));
    // An invisible layer over the content: its size is the content's size, and GTK
    // tells it about every change.
    let sizer = gtk4::DrawingArea::new();
    sizer.set_can_target(false);
    overlay.add_overlay(&sizer);

    let blur: Rc<RefCell<Option<Blur>>> = Rc::default();
    let cards: Rc<dyn Fn() -> Vec<gtk4::Widget>> = Rc::new(cards);
    let update = {
        let (window, blur, cards) = (window.downgrade(), blur.clone(), cards.clone());
        move || {
            let Some(window) = window.upgrade() else { return };
            let (Some(blur), Some(surface)) = (&*blur.borrow(), window.surface()) else { return };
            let rects: Vec<Rect> = cards()
                .iter()
                .filter(|card| card.is_visible())
                .filter_map(|card| card.compute_bounds(&window))
                // Not laid out yet (right after mapping): wait for the resize.
                .filter(|b| b.width() > 0.0 && b.height() > 0.0)
                .flat_map(|b| {
                    // A pixel inside the card's edge: the blur's corners are pixel steps,
                    // and level with the card's smooth edge they showed as jagged corners.
                    rounded_rect(
                        b.x().round() as i32 + INSET,
                        b.y().round() as i32 + INSET,
                        b.width().round() as i32 - 2 * INSET,
                        b.height().round() as i32 - 2 * INSET,
                        radius - INSET,
                    )
                })
                .collect();
            blur.set_region(&surface, &rects);
        }
    };
    let update = Rc::new(update);
    let u = update.clone();
    sizer.connect_resize(move |_, _, _| {
        on_resize();
        u();
    });
    let (b, u) = (blur.clone(), update.clone());
    window.connect_map(move |window| {
        if let Some(surface) = window.surface() {
            b.replace(Blur::new(&surface));
        }
        u();
    });
    window.connect_unmap(move |_| {
        blur.take();
    });
    overlay
}

/// The union of strips that approximates a rounded rectangle: one strip per pixel row
/// in the corners and one for the straight middle.
pub fn rounded_rect(x: i32, y: i32, width: i32, height: i32, radius: i32) -> Vec<Rect> {
    if width <= 0 || height <= 0 {
        return Vec::new();
    }
    let radius = radius.clamp(0, width.min(height) / 2);
    let mut rects = Vec::with_capacity(2 * radius as usize + 1);
    for row in 0..radius {
        // Distance from the corner circle's center to the middle of this pixel row.
        let dy = f64::from(radius) - f64::from(row) - 0.5;
        let half = (f64::from(radius).powi(2) - dy * dy).max(0.0).sqrt();
        let inset = (f64::from(radius) - half).round() as i32;
        let strip = width - 2 * inset;
        rects.push(Rect { x: x + inset, y: y + row, width: strip, height: 1 });
        rects.push(Rect { x: x + inset, y: y + height - 1 - row, width: strip, height: 1 });
    }
    if height > 2 * radius {
        rects.push(Rect { x, y: y + radius, width, height: height - 2 * radius });
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounded_rect_covers_inside_only() {
        let rects = rounded_rect(0, 0, 100, 50, 10);
        let covered = |px: i32, py: i32| {
            rects
                .iter()
                .any(|r| (r.x..r.x + r.width).contains(&px) && (r.y..r.y + r.height).contains(&py))
        };
        assert!(covered(50, 25));
        assert!(covered(0, 25));
        assert!(!covered(0, 0), "corner pixel is outside the rounding");
        assert!(covered(10, 0));
    }
}

//! Compositor blur behind parts of a surface (`ext-background-effect-v1`, niri 26.04+).
//!
//! The shell says exactly which area to blur, so rounded cards keep sharp shadows and
//! corners. A protocol error kills the whole Wayland connection, so a [`Blur`] must be
//! dropped before its surface goes away: create it on map, drop it on unmap.

use std::cell::RefCell;

use gdk4_wayland::prelude::*;
use gtk4::gdk;

use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_compositor, wl_region, wl_registry};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, WEnum};
use wayland_protocols::ext::background_effect::v1::client::{
    ext_background_effect_manager_v1::{self, Capability, ExtBackgroundEffectManagerV1},
    ext_background_effect_surface_v1::ExtBackgroundEffectSurfaceV1,
};

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
        let wl_surface = surface.downcast_ref::<gdk4_wayland::WaylandSurface>()?.wl_surface()?;
        GLOBALS.with_borrow_mut(|globals| {
            let g = globals.get_or_insert_with(Globals::bind).as_mut()?;
            let effect = g.manager.get_background_effect(&wl_surface, &g.qh, ());
            Some(Blur { effect })
        })
    }

    /// Blurs behind the union of `rects` (empty: nothing). Takes effect with the
    /// surface's next commit, which this asks GDK for.
    pub fn set_region(&self, surface: &gdk::Surface, rects: &[Rect]) {
        GLOBALS.with_borrow_mut(|globals| {
            let Some(Some(g)) = globals.as_mut() else { return };
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
            let _ = g.conn.flush();
        });
        surface.queue_render();
    }
}

impl Drop for Blur {
    fn drop(&mut self) {
        self.effect.destroy();
        GLOBALS.with_borrow(|globals| {
            if let Some(Some(g)) = globals.as_ref() {
                let _ = g.conn.flush();
            }
        });
    }
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

thread_local! {
    /// Bound once on first use; None inside if blur isn't available.
    static GLOBALS: RefCell<Option<Option<Globals>>> = const { RefCell::new(None) };
}

struct Globals {
    conn: Connection,
    qh: QueueHandle<State>,
    manager: ExtBackgroundEffectManagerV1,
    compositor: wl_compositor::WlCompositor,
    /// Our own queue on GDK's connection. Nothing on it needs handling after binding,
    /// it is kept so the objects stay valid.
    _queue: EventQueue<State>,
}

#[derive(Default)]
struct State {
    can_blur: bool,
}

impl Globals {
    fn bind() -> Option<Globals> {
        let display = gdk::Display::default()?.downcast::<gdk4_wayland::WaylandDisplay>().ok()?;
        let wl_display = display.wl_display()?;
        let backend = wayland_client::Proxy::backend(&wl_display).upgrade()?;
        let conn = Connection::from_backend(backend);
        let (globals, mut queue) = registry_queue_init::<State>(&conn)
            .inspect_err(|err| tracing::warn!("blur: {err}"))
            .ok()?;
        let qh = queue.handle();
        let manager: ExtBackgroundEffectManagerV1 = globals
            .bind(&qh, 1..=1, ())
            .inspect_err(|_| tracing::info!("blur: the compositor doesn't support it"))
            .ok()?;
        let compositor: wl_compositor::WlCompositor = globals.bind(&qh, 1..=6, ()).ok()?;

        // The manager announces what it can do right after binding.
        let mut state = State::default();
        queue.roundtrip(&mut state).ok()?;
        if !state.can_blur {
            tracing::info!("blur: not offered by the compositor");
            return None;
        }
        tracing::info!("blur: ready");
        Some(Globals { conn, qh, manager, compositor, _queue: queue })
    }
}

impl Dispatch<ExtBackgroundEffectManagerV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ExtBackgroundEffectManagerV1,
        event: ext_background_effect_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_background_effect_manager_v1::Event::Capabilities { flags } = event {
            state.can_blur = matches!(flags, WEnum::Value(f) if f.contains(Capability::Blur));
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<ExtBackgroundEffectSurfaceV1, ()> for State {
    fn event(_: &mut Self, _: &ExtBackgroundEffectSurfaceV1, _: <ExtBackgroundEffectSurfaceV1 as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<wl_compositor::WlCompositor, ()> for State {
    fn event(_: &mut Self, _: &wl_compositor::WlCompositor, _: wl_compositor::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<wl_region::WlRegion, ()> for State {
    fn event(_: &mut Self, _: &wl_region::WlRegion, _: wl_region::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
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

//! Wayland protocols GTK doesn't cover, on GDK's own connection: background blur and
//! idle inhibition. Bound once, on first use.

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
use wayland_protocols::wp::idle_inhibit::zv1::client::{
    zwp_idle_inhibit_manager_v1::ZwpIdleInhibitManagerV1, zwp_idle_inhibitor_v1::ZwpIdleInhibitorV1,
};

thread_local! {
    /// None inside if the display isn't Wayland or binding failed.
    static GLOBALS: RefCell<Option<Option<Globals>>> = const { RefCell::new(None) };
}

pub struct Globals {
    pub conn: Connection,
    pub qh: QueueHandle<State>,
    pub compositor: wl_compositor::WlCompositor,
    /// Present only if the compositor offers blur.
    pub blur: Option<ExtBackgroundEffectManagerV1>,
    pub idle_inhibit: Option<ZwpIdleInhibitManagerV1>,
    /// Our own queue on GDK's connection. Nothing on it needs handling after binding;
    /// it is kept so the objects stay valid.
    _queue: EventQueue<State>,
}

/// Runs `f` with the bound globals, if there are any.
pub fn with<R>(f: impl FnOnce(&Globals) -> Option<R>) -> Option<R> {
    GLOBALS.with_borrow_mut(|globals| {
        let globals = globals.get_or_insert_with(Globals::bind).as_ref()?;
        let result = f(globals);
        let _ = globals.conn.flush();
        result
    })
}

pub fn wl_surface(surface: &gdk::Surface) -> Option<wayland_client::protocol::wl_surface::WlSurface> {
    surface.downcast_ref::<gdk4_wayland::WaylandSurface>()?.wl_surface()
}

#[derive(Default)]
pub struct State {
    can_blur: bool,
}

impl Globals {
    fn bind() -> Option<Globals> {
        let display = gdk::Display::default()?.downcast::<gdk4_wayland::WaylandDisplay>().ok()?;
        let wl_display = display.wl_display()?;
        let backend = wayland_client::Proxy::backend(&wl_display).upgrade()?;
        let conn = Connection::from_backend(backend);
        let (globals, mut queue) = registry_queue_init::<State>(&conn)
            .inspect_err(|err| tracing::warn!("wayland globals: {err}"))
            .ok()?;
        let qh = queue.handle();
        let compositor: wl_compositor::WlCompositor = globals.bind(&qh, 1..=6, ()).ok()?;
        let blur: Option<ExtBackgroundEffectManagerV1> = globals.bind(&qh, 1..=1, ()).ok();
        let idle_inhibit: Option<ZwpIdleInhibitManagerV1> = globals.bind(&qh, 1..=1, ()).ok();

        // The blur manager announces what it can do right after binding.
        let mut state = State::default();
        queue.roundtrip(&mut state).ok()?;
        let blur = blur.filter(|_| state.can_blur);
        tracing::info!(blur = blur.is_some(), idle_inhibit = idle_inhibit.is_some(), "wayland extras");
        Some(Globals { conn, qh, compositor, blur, idle_inhibit, _queue: queue })
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

/// Objects without events (or whose events don't matter here).
macro_rules! ignore_events {
    ($($ty:ty: $udata:ty),* $(,)?) => {$(
        impl Dispatch<$ty, $udata> for State {
            fn event(
                _: &mut Self,
                _: &$ty,
                _: <$ty as wayland_client::Proxy>::Event,
                _: &$udata,
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
            }
        }
    )*};
}

ignore_events!(
    wl_registry::WlRegistry: GlobalListContents,
    wl_compositor::WlCompositor: (),
    wl_region::WlRegion: (),
    ExtBackgroundEffectSurfaceV1: (),
    ZwpIdleInhibitManagerV1: (),
    ZwpIdleInhibitorV1: (),
);

//! Wayland protocols GTK doesn't cover, on GDK's own connection: idle inhibition.
//! Bound once, on first use. (Blur is GTK's own since 4.24: see `blur.rs`.)

use std::cell::RefCell;

use gdk4_wayland::prelude::*;
use gtk4::gdk;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_registry;
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle};
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

pub struct State;

impl Globals {
    fn bind() -> Option<Globals> {
        let display = gdk::Display::default()?.downcast::<gdk4_wayland::WaylandDisplay>().ok()?;
        let wl_display = display.wl_display()?;
        let backend = wayland_client::Proxy::backend(&wl_display).upgrade()?;
        let conn = Connection::from_backend(backend);
        let (globals, queue) = registry_queue_init::<State>(&conn)
            .inspect_err(|err| tracing::warn!("wayland globals: {err}"))
            .ok()?;
        let qh = queue.handle();
        let idle_inhibit: Option<ZwpIdleInhibitManagerV1> = globals.bind(&qh, 1..=1, ()).ok();
        tracing::info!(idle_inhibit = idle_inhibit.is_some(), "wayland extras");
        Some(Globals { conn, qh, idle_inhibit, _queue: queue })
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
    ZwpIdleInhibitManagerV1: (),
    ZwpIdleInhibitorV1: (),
);

//! Idle notifications from the compositor (`ext-idle-notify-v1`): one per stage, each with
//! its time from the start of idle. niri counts the time and takes idle inhibitors into
//! account, so nothing here runs a timer.
//!
//! A Wayland connection of its own: on GDK's connection only GDK's queue is dispatched,
//! and events for ours would sit unread until something else woke the shell.

use std::cell::{Cell, RefCell};
use std::os::fd::AsRawFd;
use std::rc::Rc;
use std::time::Duration;

use gtk4::glib;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle};
use wayland_protocols::ext::idle_notify::v1::client::{
    ext_idle_notification_v1::{self, ExtIdleNotificationV1},
    ext_idle_notifier_v1::ExtIdleNotifierV1,
};

/// (stage index, true when idle began / false when activity resumed).
type Handler = Box<dyn Fn(usize, bool)>;

pub struct Notifier {
    conn: Connection,
    queue: RefCell<EventQueue<State>>,
    state: RefCell<State>,
    qh: QueueHandle<State>,
    seat: wl_seat::WlSeat,
    notifier: ExtIdleNotifierV1,
    notifications: RefCell<Vec<ExtIdleNotificationV1>>,
    /// Bumped whenever the notifications are made anew: events of older ones are dropped.
    generation: Cell<u64>,
    on_event: Handler,
}

#[derive(Default)]
pub struct State {
    generation: u64,
    events: Vec<(usize, bool)>,
}

impl Notifier {
    /// None if there is no Wayland display or the compositor can't tell idle time.
    pub fn connect(on_event: impl Fn(usize, bool) + 'static) -> Option<Rc<Notifier>> {
        let conn = Connection::connect_to_env()
            .inspect_err(|err| tracing::warn!("idle: no Wayland connection: {err}"))
            .ok()?;
        let (globals, queue) = registry_queue_init::<State>(&conn)
            .inspect_err(|err| tracing::warn!("idle: wayland globals: {err}"))
            .ok()?;
        let qh = queue.handle();
        let seat: wl_seat::WlSeat = globals.bind(&qh, 1..=1, ()).ok()?;
        let Ok(notifier) = globals.bind::<ExtIdleNotifierV1, _, _>(&qh, 1..=1, ()) else {
            tracing::warn!("idle: the compositor has no ext-idle-notify");
            return None;
        };
        let notifier = Rc::new(Notifier {
            conn,
            queue: RefCell::new(queue),
            state: RefCell::default(),
            qh,
            seat,
            notifier,
            notifications: RefCell::default(),
            generation: Cell::new(0),
            on_event: Box::new(on_event),
        });
        notifier.watch();
        Some(notifier)
    }

    /// Replaces the notifications: one per stage, `after` the start of idle.
    pub fn set_stages(&self, after: &[Duration]) {
        for old in self.notifications.take() {
            old.destroy();
        }
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        self.state.borrow_mut().generation = generation;
        let made = after
            .iter()
            .enumerate()
            .map(|(i, time)| {
                let ms = u32::try_from(time.as_millis()).unwrap_or(u32::MAX);
                self.notifier.get_idle_notification(ms, &self.seat, &self.qh, (generation, i))
            })
            .collect();
        self.notifications.replace(made);
        if let Err(err) = self.conn.flush() {
            tracing::warn!("idle: {err}");
        }
    }

    /// Reads and handles events whenever the connection has some.
    fn watch(self: &Rc<Self>) {
        let fd = self.conn.backend().poll_fd().as_raw_fd();
        let weak = Rc::downgrade(self);
        let condition = glib::IOCondition::IN | glib::IOCondition::HUP | glib::IOCondition::ERR;
        glib_unix::unix_fd_add_local(fd, condition, move |_, condition| {
            let Some(this) = weak.upgrade() else { return glib::ControlFlow::Break };
            if condition.intersects(glib::IOCondition::HUP | glib::IOCondition::ERR) {
                tracing::error!("idle: the Wayland connection closed");
                return glib::ControlFlow::Break;
            }
            this.read_and_dispatch();
            glib::ControlFlow::Continue
        });
    }

    fn read_and_dispatch(&self) {
        let events = {
            let mut queue = self.queue.borrow_mut();
            let mut state = self.state.borrow_mut();
            // None: events are already queued, nothing to read first.
            if let Some(guard) = queue.prepare_read()
                && let Err(err) = guard.read()
                && !matches!(&err, wayland_client::backend::WaylandError::Io(io) if io.kind() == std::io::ErrorKind::WouldBlock)
            {
                tracing::warn!("idle: {err}");
            }
            if let Err(err) = queue.dispatch_pending(&mut state) {
                tracing::warn!("idle: {err}");
            }
            std::mem::take(&mut state.events)
        };
        // Outside the borrows: handling a stage may make new notifications.
        for (stage, idle) in events {
            (self.on_event)(stage, idle);
        }
        let _ = self.conn.flush();
    }
}

impl Dispatch<ExtIdleNotificationV1, (u64, usize)> for State {
    fn event(
        state: &mut Self,
        _: &ExtIdleNotificationV1,
        event: ext_idle_notification_v1::Event,
        &(generation, stage): &(u64, usize),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if generation != state.generation {
            return;
        }
        match event {
            ext_idle_notification_v1::Event::Idled => state.events.push((stage, true)),
            ext_idle_notification_v1::Event::Resumed => state.events.push((stage, false)),
            _ => {}
        }
    }
}

/// Objects whose events don't matter here.
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
    wl_seat::WlSeat: (),
    ExtIdleNotifierV1: (),
);

//! Idle manager, replacing stasis: after a while without input the screen dims, locks,
//! the monitors go off and the machine sleeps; any input undoes what can be undone, in
//! reverse order. The times count from the start of idle and differ on AC and battery.
//!
//! niri counts idle time and knows what keeps the session awake (a browser playing
//! video, mpv, the Stay awake tile). The shell adds its own reasons, local sound and some
//! app windows, by holding an idle inhibitor of its own. Before any sleep (the lid too)
//! the screen is locked first. Nothing here polls.

mod notify;

use std::cell::{Cell, RefCell};
use std::os::fd::OwnedFd;
use std::rc::{Rc, Weak};
use std::time::Duration;

use gtk4::prelude::*;
use gtk4::{gio, glib};
use niri_ipc::Action;

use crate::config::{IdleAction, IdleConfig, Stage};
use crate::services::Services;
use crate::services::battery::Charge;
use crate::services::session::Inhibitor;

/// Screen brightness while dimmed.
const DIM_PERCENT: u32 = 5;
/// How long a reason to stay awake is kept after it ends: between two tracks, say.
const RELEASE_DELAY: Duration = Duration::from_secs(4);

const LOGIN1: &str = "org.freedesktop.login1";
const LOGIN1_PATH: &str = "/org/freedesktop/login1";
const LOGIN1_MANAGER: &str = "org.freedesktop.login1.Manager";

/// What a stage did, to undo when the user is back.
enum Done {
    Dim { brightness: Option<u32>, keyboard: Option<i32> },
    MonitorsOff,
}

pub struct Idle {
    services: Weak<Services>,
    notifier: Option<Rc<notify::Notifier>>,
    config: RefCell<Option<IdleConfig>>,
    on_battery: Cell<bool>,
    /// The stages in effect, in the order of the notifications.
    stages: RefCell<Vec<Stage>>,
    done: RefCell<Vec<Done>>,
    /// Reasons of our own to stay awake: an app window, local sound.
    app_open: Cell<bool>,
    sound: Cell<bool>,
    inhibitor: RefCell<Option<Inhibitor>>,
    release: Cell<Option<glib::SourceId>>,
    /// logind waits for the screen lock while we hold this (a "delay" inhibitor).
    sleep_lock: RefCell<Option<OwnedFd>>,
    /// In the sandbox (NSHELL_SANDBOX set) stages are only logged: its niri never gets
    /// input, and brightness, the session lock and suspend are the real machine's.
    dry_run: bool,
}

impl Idle {
    pub fn start(services: &Rc<Services>) -> Rc<Idle> {
        Rc::new_cyclic(|weak: &Weak<Idle>| {
            let w = weak.clone();
            let notifier = notify::Notifier::connect(move |stage, idle| {
                if let Some(this) = w.upgrade() {
                    if idle {
                        this.enter(stage);
                    } else {
                        this.back();
                    }
                }
            });
            Idle {
                services: Rc::downgrade(services),
                notifier,
                config: RefCell::new(None),
                on_battery: Cell::new(false),
                stages: RefCell::default(),
                done: RefCell::default(),
                app_open: Cell::new(false),
                sound: Cell::new(false),
                inhibitor: RefCell::new(None),
                release: Cell::new(None),
                sleep_lock: RefCell::new(None),
                dry_run: std::env::var_os("NSHELL_SANDBOX").is_some(),
            }
        })
    }

    /// Takes new settings (at start and on every config reload).
    pub fn configure(self: &Rc<Self>, config: &IdleConfig) {
        let first = self.config.replace(Some(config.clone())).is_none();
        if first {
            self.follow();
        }
        self.apply_stages();
        if let Some(services) = self.services.upgrade() {
            self.check_apps(&services.niri.state.get());
        }
    }

    /// Subscribes to what changes the stages or the reasons to stay awake.
    fn follow(self: &Rc<Self>) {
        let Some(services) = self.services.upgrade() else { return };

        let weak = Rc::downgrade(self);
        services.battery().state.subscribe(move |state| {
            let Some(this) = weak.upgrade() else { return false };
            let on_battery = state.present && state.charge == Charge::Discharging;
            if this.on_battery.replace(on_battery) != on_battery {
                // Not from inside the notification: this makes Wayland requests.
                glib::idle_add_local_once(move || this.apply_stages());
            }
            true
        });

        let (weak, serial) = (Rc::downgrade(self), Cell::new(None::<u64>));
        services.niri.state.subscribe(move |state| {
            let Some(this) = weak.upgrade() else { return false };
            // Title changes don't open or close windows.
            if serial.replace(Some(state.layout_serial)) != Some(state.layout_serial) {
                this.check_apps(state);
            }
            true
        });

        if let Some(audio) = services.audio() {
            let weak = Rc::downgrade(self);
            audio.apps.subscribe(move |streams| {
                let Some(this) = weak.upgrade() else { return false };
                let playing = streams.iter().any(|s| s.keeps_awake);
                if this.sound.replace(playing) != playing {
                    tracing::info!(playing, "idle: local sound");
                    glib::idle_add_local_once(move || this.update_inhibitor());
                }
                true
            });
        }

        if !self.dry_run {
            glib::spawn_future_local(lock_before_sleep(Rc::downgrade(self)));
        }
    }

    /// Makes the notifications for the profile in use (AC or battery).
    fn apply_stages(&self) {
        let stages = match &*self.config.borrow() {
            Some(config) if self.on_battery.get() => config.on_battery.clone(),
            Some(config) => config.on_ac.clone(),
            None => return,
        };
        let profile = if self.on_battery.get() { "battery" } else { "AC" };
        tracing::info!(
            "idle: on {profile}: {}",
            stages.iter().map(|s| format!("{:?} after {}s", s.action, s.after.as_secs())).collect::<Vec<_>>().join(", ")
        );
        if let Some(notifier) = &self.notifier {
            notifier.set_stages(&stages.iter().map(|s| s.after).collect::<Vec<_>>());
        }
        self.stages.replace(stages);
    }

    fn check_apps(self: &Rc<Self>, state: &crate::niri::State) {
        let open = {
            let config = self.config.borrow();
            let Some(config) = config.as_ref() else { return };
            state.windows.values().find_map(|w| {
                let app_id = w.app_id.as_deref()?;
                config.inhibit_apps.iter().any(|re| re.is_match(app_id)).then(|| app_id.to_string())
            })
        };
        if self.app_open.replace(open.is_some()) != open.is_some() {
            tracing::info!(app = open.as_deref().unwrap_or("none"), "idle: app that keeps awake");
            self.update_inhibitor();
        }
    }

    /// Holds our inhibitor while there is a reason, and a moment longer.
    fn update_inhibitor(self: &Rc<Self>) {
        if let Some(source) = self.release.take() {
            source.remove();
        }
        if self.app_open.get() || self.sound.get() {
            if self.inhibitor.borrow().is_none() {
                tracing::info!("idle: kept awake");
                let inhibitor = Inhibitor::new();
                self.inhibitor.replace(inhibitor);
            }
            return;
        }
        if self.inhibitor.borrow().is_some() {
            let weak = Rc::downgrade(self);
            let source = glib::timeout_add_local_once(RELEASE_DELAY, move || {
                if let Some(this) = weak.upgrade() {
                    this.release.set(None);
                    tracing::info!("idle: no longer kept awake");
                    // Dropped outside the cell's borrow: it destroys a window.
                    let inhibitor = this.inhibitor.take();
                    drop(inhibitor);
                }
            });
            self.release.set(Some(source));
        }
    }

    /// A stage's time has come.
    fn enter(&self, index: usize) {
        let Some(stage) = self.stages.borrow().get(index).copied() else { return };
        let Some(services) = self.services.upgrade() else { return };
        tracing::info!("idle: {:?}", stage.action);
        if self.dry_run {
            tracing::info!("idle: sandbox, not done");
            return;
        }
        match stage.action {
            IdleAction::Dim => {
                let brightness = *services.brightness().percent.get();
                let keyboard = services.keyboard_light().level.get().map(|(now, _)| now);
                crate::osd::hush();
                if brightness.is_some_and(|b| b > DIM_PERCENT) {
                    services.brightness().set_percent(DIM_PERCENT);
                }
                if keyboard.is_some_and(|k| k > 0) {
                    services.keyboard_light().set(0);
                }
                self.done.borrow_mut().push(Done::Dim { brightness, keyboard });
            }
            IdleAction::Lock => {
                if let Some(config) = &*self.config.borrow() {
                    crate::launch::command(&config.lock_command);
                }
            }
            IdleAction::Dpms => {
                services.niri.action(Action::PowerOffMonitors {});
                self.done.borrow_mut().push(Done::MonitorsOff);
            }
            IdleAction::Suspend => logind_call("Suspend"),
        }
    }

    /// The user is back: undo the stages, latest first. The lock stays (the user
    /// unlocks), sleep is over by now.
    fn back(&self) {
        let done = std::mem::take(&mut *self.done.borrow_mut());
        if done.is_empty() {
            return;
        }
        let Some(services) = self.services.upgrade() else { return };
        tracing::info!("idle: back");
        for step in done.into_iter().rev() {
            match step {
                Done::MonitorsOff => services.niri.action(Action::PowerOnMonitors {}),
                Done::Dim { brightness, keyboard } => {
                    crate::osd::hush();
                    if let Some(brightness) = brightness.filter(|b| *b > DIM_PERCENT) {
                        services.brightness().set_percent(brightness);
                    }
                    if let Some(keyboard) = keyboard.filter(|k| *k > 0) {
                        services.keyboard_light().set(keyboard);
                    }
                }
            }
        }
    }
}

/// logind Manager method taking `interactive` = false (no password prompt).
fn logind_call(method: &'static str) {
    glib::spawn_future_local(async move {
        let Ok(bus) = gio::bus_get_future(gio::BusType::System).await else { return };
        let result = bus
            .call_future(
                Some(LOGIN1),
                LOGIN1_PATH,
                LOGIN1_MANAGER,
                method,
                Some(&(false,).to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                -1,
            )
            .await;
        if let Err(err) = result {
            tracing::warn!("logind {method}: {err}");
        }
    });
}

/// Locks the screen before every sleep, the lid's too: logind waits while we hold a
/// "delay" inhibitor, which is let go once the lock command has exited (it should wait
/// for the lock, like `veila lock --wait-ready`). After waking, the clock catches up.
async fn lock_before_sleep(idle: Weak<Idle>) {
    let Ok(bus) = gio::bus_get_future(gio::BusType::System).await else { return };
    take_sleep_lock(&idle, &bus).await;
    let weak = idle.clone();
    let b = bus.clone();
    let subscription = bus.subscribe_to_signal(
        Some(LOGIN1),
        Some(LOGIN1_MANAGER),
        Some("PrepareForSleep"),
        Some(LOGIN1_PATH),
        None,
        gio::DBusSignalFlags::NONE,
        move |signal| {
            let Some(this) = weak.upgrade() else { return };
            let Some((going,)) = signal.parameters.get::<(bool,)>() else { return };
            if going {
                let command = this.config.borrow().as_ref().map(|c| c.lock_command.clone()).unwrap_or_default();
                tracing::info!("idle: locking before sleep");
                let weak = Rc::downgrade(&this);
                crate::launch::command_then(&command, move || {
                    if let Some(this) = weak.upgrade() {
                        // Closing it tells logind to go on.
                        this.sleep_lock.take();
                    }
                });
            } else {
                tracing::info!("idle: woke up");
                if let Some(services) = this.services.upgrade() {
                    services.clock().resync();
                }
                let (weak, bus) = (Rc::downgrade(&this), b.clone());
                glib::spawn_future_local(async move { take_sleep_lock(&weak, &bus).await });
            }
        },
    );
    // Lives as long as the process.
    std::mem::forget(subscription);
}

async fn take_sleep_lock(idle: &Weak<Idle>, bus: &gio::DBusConnection) {
    let args = ("sleep", "nshell", "Lock the screen before sleep", "delay").to_variant();
    let reply = bus
        .call_with_unix_fd_list_future(
            Some(LOGIN1),
            LOGIN1_PATH,
            LOGIN1_MANAGER,
            "Inhibit",
            Some(&args),
            None,
            gio::DBusCallFlags::NONE,
            -1,
            None::<&gio::UnixFDList>,
        )
        .await;
    let fd = match reply {
        Ok((reply, Some(fds))) => crate::services::dbus::child(&reply, 0)
            .and_then(|v| v.get::<glib::variant::Handle>())
            .and_then(|h| fds.get(h.0).ok()),
        Ok(_) => None,
        Err(err) => {
            tracing::warn!("idle: no lock before sleep: {err}");
            None
        }
    };
    if let (Some(fd), Some(idle)) = (fd, idle.upgrade()) {
        idle.sleep_lock.replace(Some(fd));
    }
}

//! The session: lock, suspend, reboot, power off (logind), and "stay awake".

use gtk4::prelude::*;
use gtk4::{gio, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use wayland_protocols::wp::idle_inhibit::zv1::client::zwp_idle_inhibitor_v1::ZwpIdleInhibitorV1;

use crate::watch::Watch;
use crate::wayland;

const LOGIN1: &str = "org.freedesktop.login1";

pub struct Session {
    /// "Stay awake" is on: the screen doesn't dim, lock or sleep by itself.
    pub awake: Watch<bool>,
    inhibitor: std::cell::RefCell<Option<Inhibitor>>,
    lock_command: std::cell::RefCell<String>,
}

impl Session {
    pub fn start() -> Session {
        Session {
            awake: Watch::new(false),
            inhibitor: std::cell::RefCell::new(None),
            lock_command: std::cell::RefCell::new(String::new()),
        }
    }

    pub fn set_lock_command(&self, command: &str) {
        self.lock_command.replace(command.to_string());
    }

    pub fn lock(&self) {
        crate::launch::command(&self.lock_command.borrow());
    }

    pub fn suspend(&self) {
        manager_call("Suspend");
    }

    pub fn reboot(&self) {
        manager_call("Reboot");
    }

    pub fn power_off(&self) {
        manager_call("PowerOff");
    }

    pub fn set_awake(&self, on: bool) {
        if on {
            if self.inhibitor.borrow().is_none() {
                self.inhibitor.replace(Inhibitor::new());
            }
        } else {
            self.inhibitor.take();
        }
        self.awake.set(self.inhibitor.borrow().is_some());
    }
}

/// logind Manager method taking `interactive` (asks for a password via polkit if needed).
fn manager_call(method: &'static str) {
    glib::spawn_future_local(async move {
        let Ok(bus) = gio::bus_get_future(gio::BusType::System).await else { return };
        let result = bus
            .call_future(
                Some(LOGIN1),
                "/org/freedesktop/login1",
                "org.freedesktop.login1.Manager",
                method,
                Some(&(true,).to_variant()),
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

/// Keeps the session from going idle while it lives. The compositor honours an inhibitor
/// only for a surface it actually presents, so it gets a one-pixel, all but transparent
/// surface of its own on the overlay layer, where nothing covers it (see DECISIONS.md).
pub struct Inhibitor {
    window: gtk4::Window,
}

impl Inhibitor {
    pub fn new() -> Option<Inhibitor> {
        let window = gtk4::Window::new();
        window.add_css_class("idle-inhibitor");
        window.init_layer_shell();
        window.set_namespace(Some("nshell-idle-inhibitor"));
        // Not the background layer: niri 26.04 draws it through an offscreen, so its
        // surfaces never count as presented. Not below windows either: a maximized one
        // would cover it. The overlay layer is above even fullscreen windows.
        window.set_layer(Layer::Overlay);
        window.set_anchor(Edge::Top, true);
        window.set_anchor(Edge::Left, true);
        window.set_exclusive_zone(0);
        window.set_keyboard_mode(KeyboardMode::None);
        window.set_default_size(1, 1);
        window.set_can_target(false);

        let inhibitor: std::rc::Rc<std::cell::RefCell<Option<ZwpIdleInhibitorV1>>> =
            std::rc::Rc::default();
        let slot = inhibitor.clone();
        window.connect_map(move |window| {
            // Clicks pass through the pixel.
            if let Some(surface) = window.surface() {
                surface.set_input_region(Some(&gtk4::cairo::Region::create()));
            }
            let Some(surface) = window.surface().and_then(|s| wayland::wl_surface(&s)) else {
                return;
            };
            let made = wayland::with(|g| {
                Some(g.idle_inhibit.as_ref()?.create_inhibitor(&surface, &g.qh, ()))
            });
            if made.is_none() {
                tracing::warn!("stay awake: the compositor has no idle inhibit");
            }
            slot.replace(made);
        });
        // The inhibitor must go before its surface.
        let slot = inhibitor.clone();
        window.connect_unmap(move |_| {
            if let Some(inhibitor) = slot.take() {
                inhibitor.destroy();
                wayland::with(|_| Some(()));
            }
        });
        window.set_visible(true);
        Some(Inhibitor { window })
    }
}

impl Drop for Inhibitor {
    fn drop(&mut self) {
        // Unmapping destroys the inhibitor first (see `new`).
        self.window.destroy();
    }
}

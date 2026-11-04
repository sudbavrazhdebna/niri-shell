//! Bar modules. Each one builds its widget for one bar (one monitor) and keeps it updated.

mod battery;
mod bluetooth;
mod clock;
mod control;
mod display;
mod keyboard;
mod media;
mod network;
mod tray;
mod volume;
mod window_title;

use std::rc::Rc;

use gtk4::prelude::*;

use crate::config::Config;
use crate::services::Services;

/// What a module gets when it is built.
pub struct Context {
    pub config: Rc<Config>,
    pub services: Rc<Services>,
    /// niri output name of the bar's monitor (e.g. "eDP-1").
    pub output: String,
}

pub fn build(name: &str, ctx: &Context) -> Option<gtk4::Widget> {
    match name {
        "clock" => Some(clock::build(ctx)),
        "window-title" => Some(window_title::build(ctx)),
        "keyboard" => Some(keyboard::build(ctx)),
        "media" => Some(media::build(ctx)),
        "display" => Some(display::build(ctx)),
        "volume" => volume::build(ctx),
        "battery" => Some(battery::build(ctx)),
        "network" => Some(network::build(ctx)),
        "bluetooth" => Some(bluetooth::build(ctx)),
        "tray" => Some(tray::build(ctx)),
        "control" => Some(control::build(ctx)),
        "taskbar" => {
            let apps = crate::dock::apps(&ctx.config, &ctx.services.niri);
            apps.add_css_class("taskbar");
            Some(apps.upcast())
        }
        // Accepted by the config, not written yet.
        _ => None,
    }
}

/// An icon module: one image from the bundled icons, sized by CSS.
fn icon_module(class: &str) -> gtk4::Image {
    let image = gtk4::Image::new();
    image.add_css_class("module");
    image.add_css_class("icon");
    image.add_css_class(class);
    image
}

pub const LEFT: u32 = gtk4::gdk::BUTTON_PRIMARY;
pub const MIDDLE: u32 = gtk4::gdk::BUTTON_MIDDLE;
pub const RIGHT: u32 = gtk4::gdk::BUTTON_SECONDARY;

/// Runs `action` when the widget is clicked with `button`.
fn on_click(widget: &impl IsA<gtk4::Widget>, button: u32, action: impl Fn() + 'static) {
    let gesture = gtk4::GestureClick::new();
    gesture.set_button(button);
    gesture.connect_released(move |_, _, _, _| action());
    widget.add_controller(gesture);
}

/// Runs `action` with +1 for each wheel step up and -1 for each step down.
/// Touchpads are accumulated into the same steps.
fn on_scroll(widget: &impl IsA<gtk4::Widget>, action: impl Fn(i32) + 'static) {
    let scroll = gtk4::EventControllerScroll::new(
        gtk4::EventControllerScrollFlags::VERTICAL | gtk4::EventControllerScrollFlags::DISCRETE,
    );
    scroll.connect_scroll(move |_, _, dy| {
        let steps = -dy.round() as i32;
        if steps != 0 {
            action(steps);
        }
        gtk4::glib::Propagation::Stop
    });
    widget.add_controller(scroll);
}

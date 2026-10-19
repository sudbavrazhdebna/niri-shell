//! On-screen display for volume, brightness, keyboard backlight and Caps Lock: a glass
//! card low on the focused monitor that fades after a moment. The window exists only
//! while shown, and clicks pass through it.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gtk4::prelude::*;
use gtk4::{gdk, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::blur::{Blur, rounded_rect};
use crate::services::Services;
use crate::services::audio::AudioState;

/// How long the card stays after the last change.
const SHOW_TIME: Duration = Duration::from_millis(1500);
/// Vertical position of the card's center, as a share of the monitor height.
const POSITION: f64 = 0.84;
/// Corner radius of `.osd-card` in style.css, for the blur shape.
const CARD_RADIUS: i32 = 18;

/// What one OSD shows.
pub enum Content {
    Level { icon: String, percent: u32 },
    Text { icon: String, text: String },
}

pub struct Osd {
    services: Rc<Services>,
    shown: RefCell<Option<Shown>>,
}

struct Shown {
    window: gtk4::Window,
    icon: gtk4::Image,
    level: gtk4::LevelBar,
    text: gtk4::Label,
    hide: Option<glib::SourceId>,
}

impl Osd {
    /// Starts following everything the OSD reports.
    pub fn start(services: Rc<Services>) -> Rc<Osd> {
        let osd = Rc::new(Osd { services, shown: RefCell::new(None) });
        osd.follow();
        osd
    }

    fn follow(self: &Rc<Self>) {
        let services = &self.services;

        if let Some(audio) = services.audio() {
            let (weak, last) = (Rc::downgrade(self), RefCell::new(None::<(u32, bool)>));
            audio.state.subscribe(move |state| {
                let now = state.available.then_some((state.percent, state.muted));
                // The first value and device switches aren't changes the user made here.
                let previous = last.replace(now);
                if let (Some(osd), Some(_), Some(_)) = (weak.upgrade(), previous, now)
                    && previous != now
                {
                    osd.show(volume(state));
                }
                true
            });
        }

        let (weak, last) = (Rc::downgrade(self), RefCell::new(None::<u32>));
        services.brightness().percent.subscribe(move |percent| {
            let previous = last.replace(*percent);
            if let (Some(osd), Some(_), Some(percent)) = (weak.upgrade(), previous, percent)
                && previous != Some(*percent)
            {
                osd.show(Content::Level {
                    icon: "display-brightness-symbolic".into(),
                    percent: *percent,
                });
            }
            true
        });

        let weak = Rc::downgrade(self);
        let last = RefCell::new(None::<bool>);
        services.caps_lock().subscribe(move |caps| {
            let previous = last.replace(*caps);
            if let (Some(osd), Some(_), Some(on)) = (weak.upgrade(), previous, caps) {
                osd.show(Content::Text {
                    icon: "input-keyboard-symbolic".into(),
                    text: if *on { "Caps Lock on" } else { "Caps Lock off" }.into(),
                });
            }
            true
        });

        let weak = Rc::downgrade(self);
        services.keyboard_backlight().subscribe(move |level| {
            if let (Some(osd), Some((percent, _))) = (weak.upgrade(), level) {
                osd.show(Content::Level {
                    icon: "keyboard-brightness-symbolic".into(),
                    percent: *percent,
                });
            }
            true
        });
    }

    pub fn show(self: &Rc<Self>, content: Content) {
        // Dragging a slider in an open menu already shows the value.
        if crate::menu::is_open() {
            return;
        }
        if self.shown.borrow().is_none() {
            let Some(shown) = self.build() else { return };
            self.shown.replace(Some(shown));
        }
        let mut shown = self.shown.borrow_mut();
        let Some(shown) = shown.as_mut() else { return };
        match content {
            Content::Level { icon, percent } => {
                shown.icon.set_icon_name(Some(&icon));
                shown.level.set_value(f64::from(percent.min(100)) / 100.0);
                shown.level.set_visible(true);
                shown.text.set_visible(false);
            }
            Content::Text { icon, text } => {
                shown.icon.set_icon_name(Some(&icon));
                shown.text.set_label(&text);
                shown.text.set_visible(true);
                shown.level.set_visible(false);
            }
        }
        if let Some(source) = shown.hide.take() {
            source.remove();
        }
        let weak = Rc::downgrade(self);
        shown.hide = Some(glib::timeout_add_local_once(SHOW_TIME, move || {
            if let Some(osd) = weak.upgrade()
                && let Some(mut shown) = osd.shown.take()
            {
                shown.hide = None;
                shown.window.destroy();
            }
        }));
    }

    fn build(&self) -> Option<Shown> {
        let monitor = self.focused_monitor()?;
        let window = gtk4::Window::new();
        window.add_css_class("nshell-osd");
        window.init_layer_shell();
        window.set_namespace(Some("nshell-osd"));
        window.set_layer(Layer::Overlay);
        window.set_monitor(Some(&monitor));
        window.set_keyboard_mode(KeyboardMode::None);
        window.set_anchor(Edge::Bottom, true);
        window.set_exclusive_zone(-1);

        let card = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
        card.add_css_class("osd-card");
        let icon = gtk4::Image::new();
        icon.add_css_class("osd-icon");
        let level = gtk4::LevelBar::new();
        level.add_css_class("osd-level");
        level.set_hexpand(true);
        level.set_valign(gtk4::Align::Center);
        // A plain bar: no low/high colouring.
        level.remove_offset_value(Some(gtk4::LEVEL_BAR_OFFSET_LOW));
        level.remove_offset_value(Some(gtk4::LEVEL_BAR_OFFSET_HIGH));
        level.remove_offset_value(Some(gtk4::LEVEL_BAR_OFFSET_FULL));
        let text = gtk4::Label::new(None);
        text.add_css_class("osd-text");
        text.set_hexpand(true);
        card.append(&icon);
        card.append(&level);
        card.append(&text);
        window.set_child(Some(&card));

        let blur: Rc<RefCell<Option<Blur>>> = Rc::default();
        let (b, card_weak) = (blur.clone(), card.downgrade());
        window.connect_map(move |window| {
            let Some(surface) = window.surface() else { return };
            // Click-through: an empty input region.
            surface.set_input_region(Some(&gtk4::cairo::Region::create()));
            b.replace(Blur::new(&surface));
            if let (Some(blur), Some(card)) = (&*b.borrow(), card_weak.upgrade())
                && let Some(bounds) = card.compute_bounds(window)
            {
                let rects = rounded_rect(
                    bounds.x().round() as i32,
                    bounds.y().round() as i32,
                    bounds.width().round() as i32,
                    bounds.height().round() as i32,
                    CARD_RADIUS,
                );
                blur.set_region(&surface, &rects);
            }
        });
        // The blur must not outlive the surface.
        window.connect_unmap(move |_| {
            blur.take();
        });

        // Center of the card at POSITION of the height: margin from the bottom.
        let height = monitor.geometry().height();
        let card_height = 64;
        let margin = (f64::from(height) * (1.0 - POSITION)) as i32 - card_height / 2;
        window.set_margin(Edge::Bottom, margin.max(0));
        window.present();
        Some(Shown { window, icon, level, text, hide: None })
    }

    /// The monitor of niri's focused workspace, or the first one.
    fn focused_monitor(&self) -> Option<gdk::Monitor> {
        let output = {
            let state = self.services.niri.state.get();
            state
                .workspaces
                .values()
                .find(|ws| ws.is_focused)
                .and_then(|ws| ws.output.clone())
        };
        let monitors = gdk::Display::default()?.monitors();
        let all: Vec<gdk::Monitor> = (0..monitors.n_items())
            .filter_map(|i| monitors.item(i).and_downcast::<gdk::Monitor>())
            .collect();
        all.iter()
            .find(|m| m.connector().as_deref() == output.as_deref())
            .or(all.first())
            .cloned()
    }
}

fn volume(state: &AudioState) -> Content {
    let icon = match state.percent {
        _ if state.muted => "audio-volume-muted-symbolic",
        0 => "audio-volume-muted-symbolic",
        1..=33 => "audio-volume-low-symbolic",
        34..=66 => "audio-volume-medium-symbolic",
        _ => "audio-volume-high-symbolic",
    };
    let percent = if state.muted { 0 } else { state.percent };
    Content::Level { icon: icon.into(), percent }
}

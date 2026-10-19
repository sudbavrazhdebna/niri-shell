//! On-screen display for volume, brightness, keyboard backlight and Caps Lock: a glass
//! card low on the focused monitor that fades after a moment. The window exists only
//! while shown, and clicks pass through it.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gtk4::prelude::*;
use gtk4::glib;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::blur;
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
    /// `dimmed`: the level holds but is off (muted volume), drawn grey.
    Level { icon: String, percent: u32, dimmed: bool },
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
    /// Where the bar is heading; it slides there over a few frames.
    target: Rc<std::cell::Cell<f64>>,
    sliding: Rc<std::cell::Cell<bool>>,
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
                    dimmed: false,
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
        services.keyboard_light().fn_key.subscribe(move |level| {
            if let (Some(osd), Some((percent, _))) = (weak.upgrade(), level) {
                osd.show(Content::Level {
                    icon: "keyboard-brightness-symbolic".into(),
                    percent: *percent,
                    dimmed: false,
                });
            }
            true
        });
    }

    pub fn show(self: &Rc<Self>, content: Content) {
        // A slider in a menu shows its value itself; keys always get the OSD.
        if crate::ui::slider_moved_recently() {
            return;
        }
        if self.shown.borrow().is_none() {
            let Some(shown) = self.build() else { return };
            self.shown.replace(Some(shown));
        }
        let mut shown = self.shown.borrow_mut();
        let Some(shown) = shown.as_mut() else { return };
        match content {
            Content::Level { icon, percent, dimmed } => {
                for widget in [shown.level.upcast_ref::<gtk4::Widget>(), shown.icon.upcast_ref()] {
                    if dimmed {
                        widget.add_css_class("dimmed");
                    } else {
                        widget.remove_css_class("dimmed");
                    }
                }
                shown.icon.set_icon_name(Some(&icon));
                let target = f64::from(percent.min(100)) / 100.0;
                if shown.level.is_visible() {
                    slide(&shown.level, &shown.target, &shown.sliding, target);
                } else {
                    // A fresh OSD starts at the value, no slide from zero.
                    shown.target.set(target);
                    shown.level.set_value(target);
                }
                shown.level.set_visible(true);
                shown.text.set_visible(false);
            }
            Content::Text { icon, text } => {
                shown.icon.remove_css_class("dimmed");
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
        let monitor = self.services.niri.focused_monitor()?;
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
        let card_weak = card.downgrade();
        let wrapped = blur::behind(
            &window,
            &card,
            move || card_weak.upgrade().map(|c| c.upcast()).into_iter().collect(),
            CARD_RADIUS,
            || {},
        );
        window.set_child(Some(&wrapped));
        // Click-through: an empty input region.
        window.connect_map(|window| {
            if let Some(surface) = window.surface() {
                surface.set_input_region(Some(&gtk4::cairo::Region::create()));
            }
        });

        // Center of the card at POSITION of the height: margin from the bottom.
        let height = monitor.geometry().height();
        let card_height = 64;
        let margin = (f64::from(height) * (1.0 - POSITION)) as i32 - card_height / 2;
        window.set_margin(Edge::Bottom, margin.max(0));
        window.set_visible(true);
        // Built hidden, so the first value doesn't slide in from zero.
        level.set_visible(false);
        Some(Shown {
            window,
            icon,
            level,
            target: Rc::default(),
            sliding: Rc::default(),
            text,
            hide: None,
        })
    }
}

/// Moves the bar towards `target` a little each frame (ease-out over ~150 ms) instead
/// of jumping; the frame clock runs only while it moves.
fn slide(level: &gtk4::LevelBar, target: &Rc<std::cell::Cell<f64>>, sliding: &Rc<std::cell::Cell<bool>>, to: f64) {
    target.set(to);
    if sliding.replace(true) {
        return;
    }
    let (target, sliding) = (target.clone(), sliding.clone());
    level.add_tick_callback(move |level, _| {
        let (now, to) = (level.value(), target.get());
        if (to - now).abs() < 0.004 {
            level.set_value(to);
            sliding.set(false);
            return glib::ControlFlow::Break;
        }
        level.set_value(now + (to - now) * 0.3);
        glib::ControlFlow::Continue
    });
}

fn volume(state: &AudioState) -> Content {
    // The same icons as the bar.
    let icon = match state.percent {
        _ if state.muted => "nshell-volume-mute",
        0 => "nshell-volume-mute",
        1..=33 => "nshell-volume-20",
        34..=66 => "nshell-volume-50",
        _ => "nshell-volume-100",
    };
    // Muted keeps its level: changing it while muted shows where it will be, in grey.
    Content::Level { icon: icon.into(), percent: state.percent, dimmed: state.muted }
}

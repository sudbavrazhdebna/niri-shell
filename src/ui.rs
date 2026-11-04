//! Building blocks for menu contents, so every menu looks the same.

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk4::glib;
use gtk4::prelude::*;

/// Vertical column that holds a menu's rows.
pub fn column() -> gtk4::Box {
    let column = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    column.add_css_class("menu-body");
    column
}

/// Bold heading at the top of a menu or section.
pub fn title(text: &str) -> gtk4::Label {
    let label = gtk4::Label::new(Some(text));
    label.add_css_class("menu-title");
    label.set_xalign(0.0);
    label
}

/// Secondary text under a heading.
pub fn note(text: &str) -> gtk4::Label {
    let label = gtk4::Label::new(Some(text));
    label.add_css_class("menu-note");
    label.set_xalign(0.0);
    label.set_wrap(true);
    label
}

pub fn separator() -> gtk4::Separator {
    let sep = gtk4::Separator::new(gtk4::Orientation::Horizontal);
    sep.add_css_class("menu-separator");
    sep
}

/// A clickable row with a check mark when `checked`.
pub fn item(text: &str, checked: bool, on_click: impl Fn() + 'static) -> gtk4::Button {
    checked_item(None, text, checked, on_click)
}

/// A clickable row without the check mark column: for menus where nothing is checked,
/// which would otherwise have an empty gap on the left.
pub fn plain_item(text: &str, on_click: impl Fn() + 'static) -> gtk4::Button {
    row_item(None, text, None, on_click)
}

/// Like [`item`], with an icon between the check mark and the text.
pub fn checked_item(icon: Option<&str>, text: &str, checked: bool, on_click: impl Fn() + 'static) -> gtk4::Button {
    row_item(icon, text, Some(checked), on_click)
}

fn row_item(icon: Option<&str>, text: &str, checked: Option<bool>, on_click: impl Fn() + 'static) -> gtk4::Button {
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    if let Some(checked) = checked {
        let check = gtk4::Image::from_icon_name("object-select-symbolic");
        check.add_css_class("menu-check");
        check.set_opacity(if checked { 1.0 } else { 0.0 });
        row.append(&check);
    }
    if let Some(icon) = icon {
        let image = gtk4::Image::from_icon_name(icon);
        image.add_css_class("menu-row-icon");
        row.append(&image);
    }
    let label = gtk4::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    label.set_max_width_chars(36);
    row.append(&label);

    let button = gtk4::Button::new();
    button.set_child(Some(&row));
    button.add_css_class("menu-item");
    button.add_css_class("flat");
    button.connect_clicked(move |_| on_click());
    button
}

/// A horizontal slider for a system value (volume, brightness).
///
/// While the user drags it — and for a moment after — values coming back from the
/// system don't move it: they lag behind the pointer and made the knob jitter. Changes
/// go out at most every [`SLIDER_THROTTLE`], always ending with the last one.
#[derive(Clone)]
pub struct Slider(Rc<SliderInner>);

struct SliderInner {
    scale: gtk4::Scale,
    held: Cell<bool>,
    let_go: Cell<Option<Instant>>,
    last_sent: Cell<Option<Instant>>,
    /// A value waiting for its send (a timer is due).
    pending: Cell<Option<f64>>,
    on_change: Box<dyn Fn(f64)>,
}

thread_local! {
    /// When a slider last sent a value: the OSD stays quiet for changes the user makes
    /// with a slider (the slider already shows them).
    static SLIDER_SENT: Cell<Option<Instant>> = const { Cell::new(None) };
}

/// Whether a slider changed something just now.
pub fn slider_moved_recently() -> bool {
    SLIDER_SENT.get().is_some_and(|t| t.elapsed() < Duration::from_millis(600))
}

/// How often a dragged slider sends its value.
const SLIDER_THROTTLE: Duration = Duration::from_millis(100);
/// How long after letting go system updates are still ignored.
const SLIDER_GRACE: Duration = Duration::from_millis(300);

impl Slider {
    pub fn new(max: f64, on_change: impl Fn(f64) + 'static) -> Slider {
        let scale = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, 0.0, max, 1.0);
        scale.add_css_class("menu-slider");
        scale.set_hexpand(true);
        scale.set_draw_value(false);
        scale.set_cursor_from_name(Some("pointer"));
        let slider = Slider(Rc::new(SliderInner {
            scale: scale.clone(),
            held: Cell::new(false),
            let_go: Cell::new(None),
            last_sent: Cell::new(None),
            pending: Cell::new(None),
            on_change: Box::new(on_change),
        }));

        // Raw button events: the scale's own drag gesture claims them, so a gesture of
        // ours wouldn't see the release.
        let events = gtk4::EventControllerLegacy::new();
        events.set_propagation_phase(gtk4::PropagationPhase::Capture);
        let weak = Rc::downgrade(&slider.0);
        events.connect_event(move |_, event| {
            if let Some(inner) = weak.upgrade() {
                match event.event_type() {
                    gtk4::gdk::EventType::ButtonPress | gtk4::gdk::EventType::TouchBegin => inner.held.set(true),
                    gtk4::gdk::EventType::ButtonRelease | gtk4::gdk::EventType::TouchEnd => {
                        inner.held.set(false);
                        inner.let_go.set(Some(Instant::now()));
                    }
                    gtk4::gdk::EventType::Scroll => inner.let_go.set(Some(Instant::now())),
                    _ => {}
                }
            }
            glib::Propagation::Proceed
        });
        scale.add_controller(events);

        let weak = Rc::downgrade(&slider.0);
        // Clamped to the range in effect: a slider's range can change (a track's length).
        scale.connect_change_value(move |scale, _, value| {
            if let Some(inner) = weak.upgrade() {
                Slider(inner).send(value.clamp(0.0, scale.adjustment().upper()));
            }
            glib::Propagation::Proceed
        });
        slider
    }

    pub fn widget(&self) -> &gtk4::Scale {
        &self.0.scale
    }

    /// Grey fill: the level holds but is off (muted).
    pub fn set_dimmed(&self, dimmed: bool) {
        if dimmed {
            self.0.scale.add_css_class("dimmed");
        } else {
            self.0.scale.remove_css_class("dimmed");
        }
    }

    /// Shows a value from the system, unless the user is moving the slider.
    pub fn show(&self, value: f64) {
        let inner = &self.0;
        let recently = inner.let_go.get().is_some_and(|t| t.elapsed() < SLIDER_GRACE);
        if !inner.held.get() && !recently && inner.pending.get().is_none() {
            inner.scale.set_value(value);
        }
    }

    fn send(&self, value: f64) {
        let inner = &self.0;
        let wait = inner
            .last_sent
            .get()
            .map_or(Duration::ZERO, |t| SLIDER_THROTTLE.saturating_sub(t.elapsed()));
        if wait.is_zero() {
            inner.pending.set(None);
            inner.last_sent.set(Some(Instant::now()));
            SLIDER_SENT.set(Some(Instant::now()));
            (inner.on_change)(value);
            return;
        }
        // A send is due later: it takes the newest value.
        if inner.pending.replace(Some(value)).is_none() {
            let weak = Rc::downgrade(&self.0);
            glib::timeout_add_local_once(wait, move || {
                if let Some(inner) = weak.upgrade()
                    && let Some(value) = inner.pending.take() {
                        inner.last_sent.set(Some(Instant::now()));
                        SLIDER_SENT.set(Some(Instant::now()));
                        inner.let_go.set(Some(Instant::now()));
                        (inner.on_change)(value);
                    }
            });
        }
    }
}

/// Heading with an on/off switch on the right. `on_toggle` runs only when the user
/// flips it; the returned setter mirrors the system state without calling it.
pub fn header_switch(
    text: &str,
    on_toggle: impl Fn(bool) + 'static,
) -> (gtk4::Box, impl Fn(bool) + 'static) {
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    row.add_css_class("menu-header");
    let label = title(text);
    label.set_hexpand(true);
    let switch = gtk4::Switch::new();
    switch.set_valign(gtk4::Align::Center);
    let mirroring = std::rc::Rc::new(std::cell::Cell::new(false));
    let m = mirroring.clone();
    switch.connect_state_set(move |_, on| {
        if !m.get() {
            on_toggle(on);
        }
        gtk4::glib::Propagation::Proceed
    });
    row.append(&label);
    row.append(&switch);
    let weak = switch.downgrade();
    let set = move |on: bool| {
        let Some(switch) = weak.upgrade() else { return };
        mirroring.set(true);
        switch.set_active(on);
        switch.set_state(on);
        mirroring.set(false);
    };
    (row, set)
}

/// A clickable row with an icon in front and optional dim text at the end.
pub fn icon_item(icon: &str, text: &str, trailing: &str, on_click: impl Fn() + 'static) -> gtk4::Button {
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 10);
    let image = gtk4::Image::from_icon_name(icon);
    image.add_css_class("menu-row-icon");
    let label = gtk4::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    label.set_max_width_chars(30);
    row.append(&image);
    row.append(&label);
    if !trailing.is_empty() {
        let end = gtk4::Label::new(Some(trailing));
        end.add_css_class("menu-trailing");
        row.append(&end);
    }
    let button = gtk4::Button::new();
    button.set_child(Some(&row));
    button.add_css_class("menu-item");
    button.add_css_class("flat");
    button.connect_clicked(move |_| on_click());
    button
}

/// Starts a program (e.g. a settings app) that outlives the shell.
pub fn launch(command: &str) {
    crate::launch::command(command);
}

/// Sets the image's icon unless it already shows it. GTK looks a name up again even
/// when it didn't change (every icon theme folder is stat'ed) and redraws: the Wi-Fi
/// icon did that on each signal strength update, every few seconds.
pub fn set_icon(image: &gtk4::Image, name: &str) {
    if image.icon_name().as_deref() != Some(name) {
        image.set_icon_name(Some(name));
    }
}

/// Removes all children of a box (to rebuild a list), and lets its window shrink in
/// case the new list is shorter.
pub fn clear(column: &gtk4::Box) {
    while let Some(child) = column.first_child() {
        column.remove(&child);
    }
    // After the caller has refilled the list: shrinking an empty list first would make
    // the window jump small and back.
    if let Some(window) = column.root().and_downcast::<gtk4::Window>() {
        let window = window.downgrade();
        glib::idle_add_local_once(move || {
            if let Some(window) = window.upgrade() {
                shrink_to_fit(&window);
            }
        });
    }
}

/// How long the menus' and popups' CSS animations run (style.css: 150ms), in ms.
pub const ANIMATION_MS: i64 = 150;

/// GTK windows grow with their content but never shrink back by themselves; a layer
/// window that got smaller content would keep an empty (and click-eating) area. Call
/// after content got smaller.
pub fn shrink_to_fit(window: &gtk4::Window) {
    // The content's natural size, not 1×1: GTK would then use the minimum size, which
    // for wrapping text is much narrower than the menu was (it visibly shrank).
    let Some(child) = window.child() else { return };
    let (_, width, _, _) = child.measure(gtk4::Orientation::Horizontal, -1);
    let (_, height, _, _) = child.measure(gtk4::Orientation::Vertical, width);
    window.set_default_size(width, height);
}

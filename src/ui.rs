//! Building blocks for menu contents, so every menu looks the same.

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
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let check = gtk4::Image::from_icon_name("object-select-symbolic");
    check.add_css_class("menu-check");
    check.set_opacity(if checked { 1.0 } else { 0.0 });
    let label = gtk4::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    label.set_max_width_chars(36);
    row.append(&check);
    row.append(&label);

    let button = gtk4::Button::new();
    button.set_child(Some(&row));
    button.add_css_class("menu-item");
    button.add_css_class("flat");
    button.connect_clicked(move |_| on_click());
    button
}

/// Horizontal slider from 0 to `max`; `on_change` runs only for the user's moves.
pub fn slider(max: f64, on_change: impl Fn(f64) + 'static) -> gtk4::Scale {
    let scale = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, 0.0, max, 1.0);
    scale.add_css_class("menu-slider");
    scale.set_hexpand(true);
    scale.set_draw_value(false);
    scale.connect_change_value(move |_, _, value| {
        on_change(value.clamp(0.0, max));
        gtk4::glib::Propagation::Proceed
    });
    scale
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

/// Starts a program (e.g. a settings app) detached from the shell.
pub fn launch(command: &str) {
    if let Err(err) = gtk4::glib::spawn_command_line_async(command) {
        tracing::warn!("can't start {command}: {err}");
    }
}

/// Removes all children of a box.
pub fn clear(column: &gtk4::Box) {
    while let Some(child) = column.first_child() {
        column.remove(&child);
    }
}

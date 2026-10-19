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

//! One notification as a card, shared by popups and the notification center.

use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gdk, glib};

use crate::services::notifications::{Image, Notification, Notifications, Reason};

/// Builds the card. `on_done` runs after a click or an action, e.g. to close a menu.
pub fn card(n: &Rc<Notification>, server: &Rc<Notifications>, on_done: Rc<dyn Fn()>) -> gtk4::Box {
    let card = gtk4::Box::new(gtk4::Orientation::Horizontal, 14);
    card.add_css_class("notification");
    if n.critical {
        card.add_css_class("critical");
    }

    let image = gtk4::Image::new();
    image.add_css_class("notification-image");
    image.set_valign(gtk4::Align::Start);
    set_image(&image, n);
    card.append(&image);

    let text = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    text.set_hexpand(true);
    let top = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    let summary = gtk4::Label::new(Some(if n.summary.is_empty() { &n.app_name } else { &n.summary }));
    summary.add_css_class("summary");
    summary.set_xalign(0.0);
    summary.set_hexpand(true);
    summary.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    summary.set_max_width_chars(30);
    let time = gtk4::Label::new(Some(&relative_time(n.received.as_ref())));
    time.add_css_class("time");
    let close = gtk4::Button::from_icon_name("window-close-symbolic");
    close.add_css_class("close-button");
    close.set_valign(gtk4::Align::Start);
    let (s, id) = (server.clone(), n.id);
    close.connect_clicked(move |_| s.close(id, Reason::Dismissed));
    top.append(&summary);
    top.append(&time);
    top.append(&close);
    text.append(&top);

    if !n.body.is_empty() {
        let body = gtk4::Label::new(None);
        body.set_markup(&n.body);
        body.add_css_class("body");
        body.set_xalign(0.0);
        body.set_wrap(true);
        body.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
        body.set_lines(4);
        body.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        body.set_max_width_chars(36);
        text.append(&body);
    }

    let buttons: Vec<_> = n.actions.iter().filter(|(key, _)| key != "default").collect();
    if !buttons.is_empty() {
        let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        row.add_css_class("actions");
        row.set_homogeneous(true);
        for (key, label) in buttons {
            let button = gtk4::Button::with_label(label);
            button.add_css_class("action");
            let (s, id, key, done) = (server.clone(), n.id, key.clone(), on_done.clone());
            button.connect_clicked(move |_| {
                s.invoke(id, &key);
                done();
            });
            row.append(&button);
        }
        text.append(&row);
    }
    card.append(&text);

    // A click on the card: its default action, or just dismiss it.
    let click = gtk4::GestureClick::new();
    let (s, id, default, done) = (server.clone(), n.id, n.default_action().map(String::from), on_done);
    // Buttons inside the card claim their clicks, so this sees only clicks elsewhere.
    click.connect_released(move |_, _, _, _| {
        match &default {
            Some(action) => s.invoke(id, action),
            None => s.close(id, Reason::Dismissed),
        }
        done();
    });
    card.add_controller(click);
    card
}

fn set_image(image: &gtk4::Image, n: &Notification) {
    match &n.image {
        Some(Image::Pixels { width, height, stride, alpha, data }) => {
            let format = if *alpha { gdk::MemoryFormat::R8g8b8a8 } else { gdk::MemoryFormat::R8g8b8 };
            let texture = gdk::MemoryTexture::new(
                *width,
                *height,
                format,
                &glib::Bytes::from(data),
                usize::try_from(*stride).unwrap_or(0),
            );
            image.set_paintable(Some(&texture));
        }
        Some(Image::File(path)) => image.set_from_file(Some(path)),
        Some(Image::Name(name)) => image.set_icon_name(Some(name)),
        None => {
            // The app's own icon, from its desktop entry or its name.
            let gicon = n
                .desktop_entry
                .as_deref()
                .and_then(crate::apps::icon)
                .or_else(|| crate::apps::icon(&n.app_name.to_lowercase()));
            match gicon {
                Some(gicon) => image.set_from_gicon(&gicon),
                None => image.set_icon_name(Some("dialog-information-symbolic")),
            }
        }
    }
}

/// "now", "5m", "2h", "3d".
pub fn relative_time(at: Option<&glib::DateTime>) -> String {
    let (Some(at), Ok(now)) = (at, glib::DateTime::now_local()) else {
        return String::new();
    };
    let minutes = now.difference(at).as_minutes();
    match minutes {
        ..1 => "now".into(),
        1..60 => format!("{minutes}m"),
        60..1440 => format!("{}h", minutes / 60),
        _ => format!("{}d", minutes / 1440),
    }
}

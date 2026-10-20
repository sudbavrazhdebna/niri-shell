//! Displays: a brightness icon; the wheel changes the built-in screen's brightness. Left
//! click: every display that is on, by name, with a brightness slider. A display whose
//! brightness the shell can't set (an external monitor: that needs DDC/CI) shows its
//! slider greyed out.

use std::rc::Rc;

use gtk4::gdk;
use gtk4::prelude::*;

use super::{Context, icon_module, on_scroll};
use crate::services::Services;
use crate::{menu, ui};

/// Brightness change per wheel step, in percent.
const STEP: i32 = 5;

pub fn build(ctx: &Context) -> gtk4::Widget {
    let image = icon_module("display");
    image.set_icon_name(Some("display-brightness-symbolic"));
    let s = ctx.services.clone();
    on_scroll(&image, move |steps| {
        let brightness = s.brightness();
        if let Some(now) = *brightness.percent.get() {
            let target = (i32::try_from(now).unwrap_or(0) + steps * STEP).clamp(1, 100);
            brightness.set_percent(u32::try_from(target).unwrap_or(1));
        }
    });
    let services = ctx.services.clone();
    menu::attach(&image, move |anchor| menu::show(anchor, &menu_content(&services)));
    image.upcast()
}

/// Built-in panels by connector name.
fn is_internal(connector: &str) -> bool {
    ["eDP", "LVDS", "DSI"].iter().any(|prefix| connector.starts_with(prefix))
}

/// "Built-in display", or maker and model ("Samsung LF24T450F"): the full description
/// repeats the connector and the serial number.
fn display_name(monitor: &gdk::Monitor, connector: &str) -> String {
    if is_internal(connector) {
        return "Built-in display".into();
    }
    let maker = monitor.manufacturer().map(String::from).unwrap_or_default();
    let maker = maker.split_whitespace().next().unwrap_or_default();
    let model = monitor.model().map(String::from).unwrap_or_default();
    let name = format!("{maker} {model}").trim().to_string();
    if name.is_empty() { connector.to_string() } else { name }
}

fn menu_content(services: &Rc<Services>) -> gtk4::Box {
    let column = ui::column();
    column.set_width_request(300);
    column.append(&ui::title("Displays"));
    column.append(&ui::separator());

    let brightness = services.brightness();
    let monitors: Vec<gdk::Monitor> = gdk::Display::default()
        .map(|d| d.monitors())
        .map(|list| (0..list.n_items()).filter_map(|i| list.item(i).and_downcast::<gdk::Monitor>()).collect())
        .unwrap_or_default();
    for monitor in &monitors {
        let connector = monitor.connector().map(String::from).unwrap_or_default();
        // The backlight belongs to the screen sysfs names, or to the built-in one.
        let has_backlight = brightness.percent.get().is_some()
            && match &brightness.connector {
                Some(lit) => *lit == connector,
                None => is_internal(&connector),
            };

        let head = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        head.add_css_class("display-head");
        let name = gtk4::Label::new(Some(&display_name(monitor, &connector)));
        name.add_css_class("display-name");
        name.set_xalign(0.0);
        name.set_hexpand(true);
        name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        let port = gtk4::Label::new(Some(&connector));
        port.add_css_class("menu-trailing");
        head.append(&name);
        head.append(&port);
        column.append(&head);

        let s = services.clone();
        let slider = ui::Slider::new(100.0, move |v| s.brightness().set_percent(v.round() as u32));
        let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        row.add_css_class("menu-slider-row");
        let icon = gtk4::Image::from_icon_name("display-brightness-symbolic");
        icon.add_css_class("menu-row-icon");
        let value = gtk4::Label::new(None);
        value.add_css_class("menu-value");
        value.set_width_chars(4);
        value.set_xalign(1.0);
        row.append(&icon);
        row.append(slider.widget());
        row.append(&value);
        if has_backlight {
            let sl = slider.clone();
            brightness.percent.bind(&value, move |value, percent| {
                let percent = percent.unwrap_or(0);
                sl.show(f64::from(percent));
                value.set_label(&format!("{percent}%"));
            });
        } else {
            // Its brightness is set on the display itself (DDC/CI isn't available).
            row.set_sensitive(false);
            row.set_tooltip_text(Some("Set this display's brightness on the display itself"));
            value.set_label("—");
        }
        column.append(&row);
    }
    if monitors.is_empty() {
        column.append(&ui::note("No displays"));
    }
    column
}

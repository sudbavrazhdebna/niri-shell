//! Displays: an icon for one or multiple connected outputs. The wheel changes the
//! built-in screen's brightness. Left click: every connected display, on or off, with
//! an on/off switch and a brightness slider. The built-in screen's brightness can be
//! set while it is off. A display whose
//! brightness the shell can't set (an external monitor: that needs DDC/CI) shows its
//! slider greyed out. The last display that is on can't be turned off.

use std::rc::Rc;

use gtk4::glib;
use gtk4::prelude::*;

use super::{Context, icon_module, on_scroll};
use crate::displays::{self, Output};
use crate::services::Services;
use crate::{menu, ui};

/// Brightness change per wheel step, in percent.
const STEP: i32 = 5;

pub fn build(ctx: &Context) -> gtk4::Widget {
    let image = icon_module("display");
    displays::watch_outputs().bind(&image, |image, outputs| {
        let name = if outputs.len() > 1 { "nshell-displays-symbolic" } else { "nshell-display-symbolic" };
        ui::set_icon(image, name);
    });
    super::tooltips::text(&image, || Some("Displays and brightness".into()));
    let s = ctx.services.clone();
    on_scroll(&image, move |steps| {
        let brightness = s.brightness();
        if let Some(now) = *brightness.percent.get() {
            let target = (i32::try_from(now).unwrap_or(0) + steps * STEP).clamp(1, 100);
            brightness.set_percent(u32::try_from(target).unwrap_or(1));
        }
    });
    let services = ctx.services.clone();
    menu::attach(&image, move |anchor| {
        // Shown once niri has listed the displays (a few ms).
        let (anchor, ticket, services) = (anchor.clone(), menu::ticket(), services.clone());
        glib::spawn_future_local(async move {
            displays::refresh().await;
            let column = ui::column();
            column.add_css_class("display-menu");
            column.set_width_request(300);
            column.append(&ui::title("Displays"));
            column.append(&ui::separator());
            let rows = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
            column.append(&rows);
            displays::watch_outputs().bind(&rows, move |rows, outputs| fill(rows, &services, outputs));
            menu::show_for(ticket, &anchor, &column);
        });
    });
    image.upcast()
}

/// The rows: one block per connected display.
fn fill(rows: &gtk4::Box, services: &Rc<Services>, outputs: &[Output]) {
    ui::clear(rows);
    let lit = outputs.iter().filter(|o| o.on).count();
    let brightness = services.brightness();
    for output in outputs {
        // The backlight belongs to the screen sysfs names, or to the built-in one.
        let has_backlight = brightness.percent.get().is_some()
            && match &brightness.connector {
                Some(lit) => *lit == output.name,
                None => output.internal,
            };

        let head = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        head.add_css_class("display-head");
        let name = gtk4::Label::new(Some(&output.label));
        name.add_css_class("display-name");
        name.set_xalign(0.0);
        name.set_hexpand(true);
        name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        let port = gtk4::Label::new(Some(&output.name));
        port.add_css_class("menu-trailing");
        let switch = gtk4::Switch::new();
        switch.set_valign(gtk4::Align::Center);
        switch.set_active(output.on);
        // Never the last screen that is on: that would leave nothing to see.
        switch.set_sensitive(!(output.on && lit == 1));
        let (connector, weak, s) = (output.name.clone(), rows.downgrade(), services.clone());
        switch.connect_state_set(move |_, on| {
            let (connector, weak, s) = (connector.clone(), weak.clone(), s.clone());
            glib::spawn_future_local(async move {
                displays::set_on(&connector, on).await;
                // A refused change leaves the snapshot equal, but the switch must go back.
                if !displays::refresh().await && let Some(rows) = weak.upgrade() {
                    fill(&rows, &s, &displays::watch_outputs().get());
                }
            });
            glib::Propagation::Proceed
        });
        head.append(&name);
        head.append(&port);
        head.append(&switch);
        rows.append(&head);

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
        rows.append(&row);
    }
    if outputs.is_empty() {
        rows.append(&ui::note("No displays"));
    }
}

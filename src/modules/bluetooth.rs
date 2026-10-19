//! Bluetooth icon, shown only while the adapter is on. Left click: on/off switch and
//! paired devices to connect or disconnect.

use std::rc::Rc;

use gtk4::prelude::*;

use super::{Context, icon_module};
use crate::services::Services;
use crate::{menu, ui};

pub fn build(ctx: &Context) -> gtk4::Widget {
    let image = icon_module("bluetooth");
    image.set_icon_name(Some("nshell-bluetooth"));
    ctx.services.bluetooth().state.bind(&image, |image, state| {
        image.set_visible(state.powered);
        if state.any_connected() {
            image.add_css_class("connected");
        } else {
            image.remove_css_class("connected");
        }
    });
    let services = ctx.services.clone();
    menu::attach(&image, move |anchor| menu::show(anchor, &menu_content(&services)));
    image.upcast()
}

/// How long a scan for nearby devices runs.
const SCAN_SECONDS: u32 = 15;

fn menu_content(services: &Rc<Services>) -> gtk4::Box {
    let column = ui::column();
    column.set_width_request(300);
    let s = services.clone();
    let (header, set_switch) = ui::header_switch("Bluetooth", move |on| s.bluetooth().set_powered(on));
    column.append(&header);

    let devices = ui::column();
    column.append(&devices);

    // Nearby devices: only after the user asks for a scan.
    let nearby_head = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    let nearby_title = ui::title("Nearby devices");
    nearby_title.set_hexpand(true);
    let spinner = gtk4::Spinner::new();
    let scan = gtk4::Button::from_icon_name("view-refresh-symbolic");
    scan.add_css_class("flat");
    scan.add_css_class("menu-icon-button");
    scan.set_cursor_from_name(Some("pointer"));
    scan.set_tooltip_text(Some("Look for devices"));
    let s = services.clone();
    scan.connect_clicked(move |_| s.bluetooth().scan(SCAN_SECONDS));
    nearby_head.append(&nearby_title);
    nearby_head.append(&spinner);
    nearby_head.append(&scan);
    column.append(&nearby_head);
    let nearby = ui::column();
    column.append(&nearby);

    let (s, nearby_weak, head_weak) = (services.clone(), nearby.downgrade(), nearby_head.downgrade());
    services.bluetooth().state.bind(&devices, move |devices, state| {
        set_switch(state.powered);
        ui::clear(devices);
        if let Some(head) = head_weak.upgrade() {
            head.set_visible(state.powered);
        }
        spinner.set_spinning(state.scanning);
        spinner.set_visible(state.scanning);
        if let Some(nearby) = nearby_weak.upgrade() {
            ui::clear(&nearby);
            for device in state.nearby.iter().filter(|_| state.powered) {
                let (s, path) = (s.clone(), device.path.clone());
                nearby.append(&ui::icon_item(&device_icon(device), &device.name, "", move || {
                    s.bluetooth().pair(&path);
                }));
            }
        }
        if !state.powered {
            return;
        }
        devices.append(&ui::title("My devices"));
        if state.devices.is_empty() {
            devices.append(&ui::note("No paired devices"));
        }
        for device in &state.devices {
            let trailing = match (device.connected, device.battery) {
                (true, Some(battery)) => format!("{battery}%"),
                (true, None) => "connected".into(),
                (false, _) => String::new(),
            };
            let (s, path, connected) = (s.clone(), device.path.clone(), device.connected);
            let row = ui::icon_item(&device_icon(device), &device.name, &trailing, move || {
                if connected {
                    s.bluetooth().disconnect(&path);
                } else {
                    s.bluetooth().connect(&path);
                }
            });
            if device.connected {
                row.add_css_class("active");
            }
            devices.append(&row);
        }
    });
    column.append(&ui::separator());
    column.append(&ui::item("Bluetooth Settings…", false, || {
        menu::close();
        ui::launch("blueman-manager");
    }));
    column
}

fn device_icon(device: &crate::services::bluetooth::Device) -> String {
    if device.icon.is_empty() {
        "bluetooth-symbolic".to_string()
    } else {
        format!("{}-symbolic", device.icon)
    }
}

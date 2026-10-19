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

fn menu_content(services: &Rc<Services>) -> gtk4::Box {
    let column = ui::column();
    column.set_width_request(300);
    let s = services.clone();
    let (header, set_switch) = ui::header_switch("Bluetooth", move |on| s.bluetooth().set_powered(on));
    column.append(&header);

    let devices = ui::column();
    let s = services.clone();
    services.bluetooth().state.bind(&devices, move |devices, state| {
        set_switch(state.powered);
        ui::clear(devices);
        if !state.powered {
            return;
        }
        if state.devices.is_empty() {
            devices.append(&ui::note("No paired devices"));
        }
        for device in &state.devices {
            let trailing = match (device.connected, device.battery) {
                (true, Some(battery)) => format!("{battery}%"),
                (true, None) => "connected".into(),
                (false, _) => String::new(),
            };
            let icon = if device.icon.is_empty() {
                "bluetooth-symbolic".to_string()
            } else {
                format!("{}-symbolic", device.icon)
            };
            let (s, path, connected) = (s.clone(), device.path.clone(), device.connected);
            let row = ui::icon_item(&icon, &device.name, &trailing, move || {
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
    column.append(&devices);
    column.append(&ui::separator());
    column.append(&ui::item("Bluetooth Settings…", false, || {
        menu::close();
        ui::launch("blueman-manager");
    }));
    column
}

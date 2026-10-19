//! Network icon: Wi-Fi by signal strength, wired, or off. Left click: Wi-Fi menu with
//! the networks around, connecting (asking for a password when needed).

use std::rc::Rc;

use gtk4::glib;
use gtk4::prelude::*;

use super::{Context, icon_module};
use crate::services::network::Link;
use crate::services::wifi::{Network, Wifi};
use crate::{menu, ui};

/// Networks shown in the menu; the rest are usually far away.
const MAX_NETWORKS: usize = 10;

pub fn build(ctx: &Context) -> gtk4::Widget {
    let image = icon_module("network");
    ctx.services.network().state.bind(&image, |image, state| {
        let icon = match state.link {
            Link::Wifi { strength, .. } => wifi_icon(strength),
            Link::Offline => "nshell-wifi-off",
            Link::Wired => "network-wired-symbolic",
        };
        image.set_icon_name(Some(icon));
    });
    menu::attach(&image, |anchor| menu::show(anchor, &menu_content()));
    image.upcast()
}

/// Thresholds as in waybar: 66+ full, 33+ two bars, 10+ one bar.
fn wifi_icon(strength: u8) -> &'static str {
    match strength {
        66.. => "nshell-wifi-100",
        33.. => "nshell-wifi-66",
        10.. => "nshell-wifi-33",
        _ => "nshell-wifi-off",
    }
}

fn menu_content() -> gtk4::Box {
    let column = ui::column();
    column.set_width_request(320);
    let list = ui::column();
    list.append(&ui::note("Searching for networks…"));
    column.append(&list);
    column.append(&ui::separator());
    column.append(&ui::item("Network Settings…", false, || {
        menu::close();
        ui::launch("nm-connection-editor");
    }));

    let list_weak = list.downgrade();
    glib::spawn_future_local(async move {
        let Some(wifi) = Wifi::open().await else { return };
        let Some(list) = list_weak.upgrade() else { return };
        let (header, set_switch) = {
            let w = wifi.clone();
            ui::header_switch("Wi-Fi", move |on| w.set_enabled(on))
        };
        if let Some(column) = list.parent().and_downcast::<gtk4::Box>() {
            column.prepend(&header);
        }
        let w = wifi.clone();
        wifi.state.bind(&list, move |list, state| {
            set_switch(state.enabled);
            ui::clear(list);
            if !state.enabled {
                return;
            }
            if let Some(error) = &state.error {
                list.append(&ui::note(error));
            }
            if state.networks.is_empty() {
                list.append(&ui::note("No networks found"));
            }
            for network in state.networks.iter().take(MAX_NETWORKS) {
                list.append(&network_row(&w, network));
            }
        });
    });
    column
}

/// One network; a secured unknown one opens a password field under it.
fn network_row(wifi: &Rc<Wifi>, network: &Network) -> gtk4::Box {
    let holder = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    let trailing = if network.active {
        "connected"
    } else if network.secured && !network.known {
        "🔒"
    } else {
        ""
    };
    let (w, n, h) = (wifi.clone(), network.clone(), holder.downgrade());
    let row = ui::icon_item(wifi_icon(network.strength), &network.ssid, trailing, move || {
        if n.active {
            return;
        }
        if n.secured && !n.known {
            if let Some(holder) = h.upgrade() {
                show_password(&holder, &w, &n);
            }
            return;
        }
        w.connect(&n, None);
    });
    if network.active {
        row.add_css_class("active");
    }
    holder.append(&row);
    holder
}

fn show_password(holder: &gtk4::Box, wifi: &Rc<Wifi>, network: &Network) {
    if holder.last_child().is_some_and(|c| c.has_css_class("menu-password")) {
        return;
    }
    let entry = gtk4::PasswordEntry::new();
    entry.add_css_class("menu-password");
    entry.set_show_peek_icon(true);
    entry.set_placeholder_text(Some("Password"));
    let (w, n) = (wifi.clone(), network.clone());
    entry.connect_activate(move |entry| {
        let password = entry.text();
        if !password.is_empty() {
            w.connect(&n, Some(&password));
        }
    });
    holder.append(&entry);
    entry.grab_focus();
}

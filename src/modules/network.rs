//! Network icon: Wi-Fi by signal strength, wired, or off. Left click: Wi-Fi menu with
//! the networks around, connecting (asking for a password when needed).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::glib;
use gtk4::prelude::*;

use super::{Context, icon_module};
use crate::services::network::Link;
use crate::services::wifi::{Network, Wifi, WifiState};
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
        ui::set_icon(image, icon);
    });
    menu::attach(&image, |anchor| {
        // Shown once the known networks are read (~0.05 s), so it doesn't build up in pieces.
        let (anchor, ticket) = (anchor.clone(), menu::ticket());
        glib::spawn_future_local(async move {
            if let Some(wifi) = Wifi::open().await {
                menu::show_for(ticket, &anchor, &menu_content(wifi));
            }
        });
    });
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

fn menu_content(wifi: Rc<Wifi>) -> gtk4::Box {
    let column = ui::column();
    column.set_width_request(320);
    // The switch's handler keeps the Wi-Fi session alive for as long as the menu shows.
    let w = wifi.clone();
    let (header, set_switch) = ui::header_switch("Wi-Fi", move |on| w.set_enabled(on));
    column.append(&header);
    column.append(&ui::separator());
    let card = ConnectedCard::new();
    column.append(&card.widget);
    let list = ui::column();
    column.append(&list);
    column.append(&ui::separator());
    column.append(&ui::plain_item("Network Settings…", || {
        menu::close();
        ui::launch("nm-connection-editor");
    }));

    // Weak: the session's own state holds this listener, and a strong reference here
    // kept the session (and its NetworkManager signals) after the menu closed.
    let w = Rc::downgrade(&wifi);
    wifi.state.bind(&list, move |list, state| {
        let Some(w) = w.upgrade() else { return };
        set_switch(state.enabled);
        card.update(state.networks.iter().find(|n| n.active).filter(|_| state.enabled), state);
        // Scans rebuild the list every few seconds: an open password field (and what's
        // typed in it) moves to its network's new row instead of vanishing.
        let kept = take_open_password(list);
        ui::clear(list);
        if !state.enabled {
            return;
        }
        if let Some(error) = &state.error {
            list.append(&ui::note(error));
        }
        let others: Vec<_> = state.networks.iter().filter(|n| !n.active).take(MAX_NETWORKS).collect();
        if others.is_empty() {
            list.append(&ui::note("No other networks"));
        } else {
            list.append(&ui::title("Other networks"));
        }
        for network in others {
            let row = network_row(&w, network);
            if let Some((ssid, revealer)) = &kept
                && *ssid == network.ssid
            {
                row.append(revealer);
            }
            list.append(&row);
        }
        // The typing goes on: focus once it's in the list again, the cursor at the end
        // (a plain grab_focus selects all, and the next key replaced the password).
        if let Some(entry) = kept
            .as_ref()
            .filter(|(_, r)| r.parent().is_some())
            .and_then(|(_, r)| r.child())
            .and_downcast::<gtk4::PasswordEntry>()
        {
            match entry.delegate().and_downcast::<gtk4::Text>() {
                Some(text) => {
                    text.grab_focus_without_selecting();
                    entry.set_position(-1);
                }
                None => {
                    entry.grab_focus();
                }
            }
        }
    });
    column
}

/// The network in use: its name, address and live traffic. Built once per menu and
/// updated in place, so the traffic counter keeps its last sample across scan results.
struct ConnectedCard {
    widget: gtk4::Box,
    icon: gtk4::Image,
    name: gtk4::Label,
    details: gtk4::Label,
    address: Rc<RefCell<String>>,
    interface: Rc<RefCell<Option<String>>>,
}

impl ConnectedCard {
    fn new() -> ConnectedCard {
        let widget = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
        widget.add_css_class("connected-card");
        widget.set_visible(false);
        let top = gtk4::Box::new(gtk4::Orientation::Horizontal, 10);
        let icon = gtk4::Image::new();
        icon.add_css_class("menu-row-icon");
        let name = gtk4::Label::new(None);
        name.add_css_class("connected-name");
        name.set_xalign(0.0);
        name.set_hexpand(true);
        name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        let status = gtk4::Label::new(Some("Connected"));
        status.add_css_class("menu-trailing");
        top.append(&icon);
        top.append(&name);
        top.append(&status);
        widget.append(&top);
        let details = gtk4::Label::new(None);
        details.add_css_class("connected-details");
        details.set_xalign(0.0);
        widget.append(&details);

        let card = ConnectedCard {
            widget,
            icon,
            name,
            details,
            address: Rc::default(),
            interface: Rc::default(),
        };
        // Traffic: counters read once a second, only while the menu is open.
        let (weak, address, interface) = (card.details.downgrade(), card.address.clone(), card.interface.clone());
        let last = Cell::new(None::<(u64, u64)>);
        glib::timeout_add_seconds_local(1, move || {
            let Some(details) = weak.upgrade() else { return glib::ControlFlow::Break };
            let now = interface.borrow().as_deref().and_then(traffic);
            if let (Some((rx0, tx0)), Some((rx1, tx1))) = (last.get(), now) {
                details.set_label(&format!(
                    "{}    ↓ {}   ↑ {}",
                    address.borrow(),
                    speed(rx1.saturating_sub(rx0)),
                    speed(tx1.saturating_sub(tx0))
                ));
            }
            last.set(now);
            glib::ControlFlow::Continue
        });
        card
    }

    fn update(&self, active: Option<&Network>, state: &WifiState) {
        self.widget.set_visible(active.is_some());
        let Some(active) = active else { return };
        ui::set_icon(&self.icon, wifi_icon(active.strength));
        self.name.set_label(&active.ssid);
        let address = state.ip.as_deref().map(|ip| format!("IP {ip}")).unwrap_or_default();
        if *self.address.borrow() != address {
            self.details.set_label(&address);
            self.address.replace(address);
        }
        self.interface.replace(state.interface.clone());
    }
}

/// (received, sent) bytes of an interface so far.
fn traffic(interface: &str) -> Option<(u64, u64)> {
    let read = |name: &str| {
        std::fs::read_to_string(format!("/sys/class/net/{interface}/statistics/{name}"))
            .ok()?
            .trim()
            .parse::<u64>()
            .ok()
    };
    Some((read("rx_bytes")?, read("tx_bytes")?))
}

/// Bytes per second, short: "820 B/s", "34 KB/s", "1.2 MB/s".
fn speed(bytes: u64) -> String {
    match bytes {
        0..1000 => format!("{bytes} B/s"),
        1000..1_000_000 => format!("{} KB/s", bytes / 1000),
        _ => format!("{:.1} MB/s", bytes as f64 / 1_000_000.0),
    }
}

/// One network; a secured unknown one opens a password field under it.
fn network_row(wifi: &Rc<Wifi>, network: &Network) -> gtk4::Box {
    // No spacing: a closed field's spacing stayed in the height (the field's own margin
    // spaces it instead).
    let holder = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    // Which network the row is, for a password field kept over a rebuild.
    holder.set_widget_name(&network.ssid);
    let (w, n, h) = (wifi.clone(), network.clone(), holder.downgrade());
    let row = ui::icon_item(wifi_icon(network.strength), &network.ssid, "", move || {
        let Some(holder) = h.upgrade() else { return };
        // One password field at a time: a click on its network again closes it, a click
        // on any other network closes it too.
        let was_open = close_password(&holder);
        let mut other_open = false;
        let list = holder.parent().and_downcast::<gtk4::Box>();
        if let Some(list) = &list {
            let mut child = list.first_child();
            while let Some(other) = child {
                child = other.next_sibling();
                if let Ok(other) = other.downcast::<gtk4::Box>() {
                    other_open |= close_password(&other);
                }
            }
        }
        if n.active || was_open {
            return;
        }
        if n.secured && !n.known {
            // From one field to another: one closes while the other opens, and the list
            // keeps its height meanwhile (each field's height rounds up, so the two
            // together came out a pixel taller and the menu twitched).
            if other_open && let Some(list) = &list {
                hold_height(list);
            }
            show_password(&holder, &w, &n);
            return;
        }
        w.connect(&n, None);
    });
    if network.active {
        row.add_css_class("active");
    }
    // A password is needed: a lock at the end, dim like the other trailing details.
    if network.secured && !network.known {
        ui::trailing_icon(&row, "nshell-lock-symbolic");
    }
    holder.append(&row);
    holder
}

/// Takes the open password field out of the list, with its network's name.
fn take_open_password(list: &gtk4::Box) -> Option<(String, gtk4::Revealer)> {
    let mut child = list.first_child();
    while let Some(row) = child {
        child = row.next_sibling();
        let Ok(holder) = row.downcast::<gtk4::Box>() else { continue };
        if let Some(revealer) = password_revealer(&holder).filter(|r| r.reveals_child()) {
            holder.remove(&revealer);
            return Some((holder.widget_name().to_string(), revealer));
        }
    }
    None
}

/// The password field under a network, open or sliding closed.
fn password_revealer(holder: &gtk4::Box) -> Option<gtk4::Revealer> {
    holder.last_child().and_downcast::<gtk4::Revealer>().filter(|r| r.has_css_class("password-revealer"))
}

/// Keeps the list at its present height for as long as the fields slide.
fn hold_height(list: &gtk4::Box) {
    list.set_size_request(-1, list.height());
    let list = list.downgrade();
    let ms = u64::try_from(ui::ANIMATION_MS).unwrap_or(150) + 50;
    glib::timeout_add_local_once(std::time::Duration::from_millis(ms), move || {
        if let Some(list) = list.upgrade() {
            list.set_size_request(-1, -1);
        }
    });
}

/// Slides the password field under a network closed (then removes it); says whether
/// one was open. Twice as fast as opening: when one closes as another opens, the two
/// together never get taller than one (at 3/4 the closing one's last pixel still met
/// the opened one).
fn close_password(holder: &gtk4::Box) -> bool {
    match password_revealer(holder).filter(|r| r.reveals_child()) {
        Some(revealer) => {
            revealer.set_transition_duration(u32::try_from(ui::ANIMATION_MS / 2).unwrap_or(75));
            revealer.set_reveal_child(false);
            true
        }
        None => false,
    }
}

/// Slides a password field open under a network (150 ms, like the other animations).
fn show_password(holder: &gtk4::Box, wifi: &Rc<Wifi>, network: &Network) {
    // Still sliding closed: open that one again.
    if let Some(revealer) = password_revealer(holder) {
        revealer.set_transition_duration(u32::try_from(ui::ANIMATION_MS).unwrap_or(150));
        revealer.set_reveal_child(true);
        if let Some(entry) = revealer.child() {
            entry.grab_focus();
        }
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
    let revealer = gtk4::Revealer::new();
    revealer.add_css_class("password-revealer");
    revealer.set_transition_type(gtk4::RevealerTransitionType::SlideDown);
    revealer.set_transition_duration(u32::try_from(ui::ANIMATION_MS).unwrap_or(150));
    revealer.set_child(Some(&entry));
    // Closed: gone from the list (the menu's height follows by itself). Its parent, not
    // the holder it was made in: a rebuild moves it to the network's new row.
    revealer.connect_child_revealed_notify(move |revealer| {
        if !revealer.reveals_child()
            && !revealer.is_child_revealed()
            && let Some(holder) = revealer.parent().and_downcast::<gtk4::Box>()
        {
            holder.remove(revealer);
        }
    });
    holder.append(&revealer);
    revealer.set_reveal_child(true);
    entry.grab_focus();
}

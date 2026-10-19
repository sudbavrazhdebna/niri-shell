//! Battery from UPower's display device (the combined state of all batteries).

use gtk4::gio;
use gtk4::prelude::*;

use super::dbus::{on_properties_changed, prop, system_proxy};
use crate::watch::Watch;

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Charge {
    Charging,
    #[default]
    Discharging,
    /// On AC, not charging: full, or held at a charge threshold.
    Plugged,
}

#[derive(Clone, Default, PartialEq, Debug)]
pub struct BatteryState {
    /// False on machines without a battery, and until UPower answers.
    pub present: bool,
    pub percent: f64,
    pub charge: Charge,
    /// Seconds; 0 when UPower doesn't know.
    pub time_to_empty: i64,
    pub time_to_full: i64,
}

pub struct Battery {
    pub state: Watch<BatteryState>,
}

impl Battery {
    pub fn start() -> Battery {
        let state = Watch::new(BatteryState::default());
        let watch = state.clone();
        gtk4::glib::spawn_future_local(async move {
            let Some(proxy) = system_proxy(
                "org.freedesktop.UPower",
                "/org/freedesktop/UPower/devices/DisplayDevice",
                "org.freedesktop.UPower.Device",
            )
            .await
            else {
                return;
            };
            watch.set(read(&proxy));
            // The handler keeps the proxy alive for the life of the process.
            on_properties_changed(&proxy, move |proxy| watch.set(read(proxy)));
        });
        Battery { state }
    }
}

fn read(proxy: &gio::DBusProxy) -> BatteryState {
    let present = prop(proxy, "IsPresent").unwrap_or(false);
    // UPower device states: 1 charging, 2 discharging, 3 empty, 4 fully charged,
    // 5 pending charge, 6 pending discharge.
    let charge = match prop::<u32>(proxy, "State") {
        Some(1) => Charge::Charging,
        Some(4 | 5) => Charge::Plugged,
        _ => Charge::Discharging,
    };
    BatteryState {
        present,
        percent: prop(proxy, "Percentage").unwrap_or(0.0),
        charge,
        time_to_empty: prop(proxy, "TimeToEmpty").unwrap_or(0),
        time_to_full: prop(proxy, "TimeToFull").unwrap_or(0),
    }
}

/// Wear and power of the battery, for the menu.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Details {
    /// Wh now, when full, when new.
    pub energy: f64,
    pub energy_full: f64,
    pub energy_full_design: f64,
    /// W going in or out.
    pub rate: f64,
    /// Health: full capacity as a share of the design, in percent.
    pub capacity: f64,
}

/// Follows the first battery's details while `owner` lives (the battery menu): the
/// subscription ends with the menu.
pub fn follow_details(owner: &impl gtk4::prelude::IsA<gtk4::Widget>, show: impl Fn(&Details) + 'static) {
    let slot: std::rc::Rc<std::cell::RefCell<Option<gio::DBusProxy>>> = std::rc::Rc::default();
    let s = slot.clone();
    // Dropping the proxy at the end ends the subscription.
    owner.as_ref().connect_destroy(move |_| {
        s.take();
    });
    let owner = owner.as_ref().downgrade();
    gtk4::glib::spawn_future_local(async move {
        let Some(path) = first_battery().await else { return };
        let Some(proxy) = system_proxy("org.freedesktop.UPower", &path, "org.freedesktop.UPower.Device").await
        else {
            return;
        };
        if owner.upgrade().is_none() {
            return;
        }
        show(&details(&proxy));
        on_properties_changed(&proxy, move |proxy| show(&details(proxy)));
        slot.replace(Some(proxy));
    });
}

async fn first_battery() -> Option<String> {
    let upower = system_proxy("org.freedesktop.UPower", "/org/freedesktop/UPower", "org.freedesktop.UPower").await?;
    let paths = upower
        .call_future("EnumerateDevices", None, gio::DBusCallFlags::NONE, -1)
        .await
        .ok()?
        .get::<(Vec<gtk4::glib::variant::ObjectPath>,)>()?
        .0;
    for path in paths {
        let Some(device) = system_proxy("org.freedesktop.UPower", path.as_str(), "org.freedesktop.UPower.Device").await
        else {
            continue;
        };
        // Type 2 is a battery; PowerSupply sets the laptop's own apart from mice etc.
        if prop::<u32>(&device, "Type") == Some(2) && prop(&device, "PowerSupply").unwrap_or(false) {
            return Some(path.as_str().to_string());
        }
    }
    None
}

fn details(proxy: &gio::DBusProxy) -> Details {
    Details {
        energy: prop(proxy, "Energy").unwrap_or(0.0),
        energy_full: prop(proxy, "EnergyFull").unwrap_or(0.0),
        energy_full_design: prop(proxy, "EnergyFullDesign").unwrap_or(0.0),
        rate: prop(proxy, "EnergyRate").unwrap_or(0.0),
        capacity: prop(proxy, "Capacity").unwrap_or(0.0),
    }
}

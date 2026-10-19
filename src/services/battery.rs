//! Battery from UPower's display device (the combined state of all batteries).

use gtk4::gio;

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

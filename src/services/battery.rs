//! Battery from UPower's display device (the combined state of all batteries).

use gtk4::gio;
use gtk4::prelude::*;

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
            let proxy = gio::DBusProxy::for_bus_future(
                gio::BusType::System,
                gio::DBusProxyFlags::NONE,
                None,
                "org.freedesktop.UPower",
                "/org/freedesktop/UPower/devices/DisplayDevice",
                "org.freedesktop.UPower.Device",
            )
            .await;
            let proxy = match proxy {
                Ok(proxy) => proxy,
                Err(err) => {
                    tracing::warn!("UPower: {err}");
                    return;
                }
            };
            watch.set(read(&proxy));
            // The handler keeps the proxy alive for the life of the process.
            proxy.connect_local("g-properties-changed", false, move |args| {
                if let Some(proxy) = args.first().and_then(|v| v.get::<gio::DBusProxy>().ok()) {
                    watch.set(read(&proxy));
                }
                None
            });
        });
        Battery { state }
    }
}

fn read(proxy: &gio::DBusProxy) -> BatteryState {
    let prop = |name: &str| proxy.cached_property(name);
    let present = prop("IsPresent").and_then(|v| v.get::<bool>()).unwrap_or(false);
    // UPower device states: 1 charging, 2 discharging, 3 empty, 4 fully charged,
    // 5 pending charge, 6 pending discharge.
    let charge = match prop("State").and_then(|v| v.get::<u32>()) {
        Some(1) => Charge::Charging,
        Some(4 | 5) => Charge::Plugged,
        _ => Charge::Discharging,
    };
    BatteryState {
        present,
        percent: prop("Percentage").and_then(|v| v.get::<f64>()).unwrap_or(0.0),
        charge,
        time_to_empty: prop("TimeToEmpty").and_then(|v| v.get::<i64>()).unwrap_or(0),
        time_to_full: prop("TimeToFull").and_then(|v| v.get::<i64>()).unwrap_or(0),
    }
}

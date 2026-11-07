//! Waybar-style information, read from service caches only when GTK asks to show it.

use gtk4::prelude::*;

use crate::services::audio::AudioState;
use crate::services::battery::{BatteryState, Charge};
use crate::services::bluetooth::BluetoothState;
use crate::services::network::{Link, NetworkState, WifiDetails};

pub fn text(widget: &impl IsA<gtk4::Widget>, read: impl Fn() -> Option<String> + 'static) {
    widget.set_has_tooltip(true);
    widget.connect_query_tooltip(move |widget, _, _, _, tooltip| {
        if widget.has_css_class("menu-open") {
            return false;
        }
        let Some(text) = read().filter(|text| !text.is_empty()) else { return false };
        // Device names and SSIDs are plain text, not Pango markup.
        tooltip.set_custom(None::<&gtk4::Widget>);
        tooltip.set_text(Some(&text));
        true
    });
}

pub fn battery(state: &BatteryState, power: Option<f64>) -> String {
    let seconds = match state.charge {
        Charge::Charging => state.time_to_full,
        Charge::Discharging => state.time_to_empty,
        Charge::Plugged => 0,
    };
    let time = if seconds > 0 {
        let (hours, minutes) = (seconds / 3600, seconds / 60 % 60);
        format!("{hours} h {minutes:02} min")
    } else {
        "—".into()
    };
    let power = power.filter(|p| p.is_finite() && *p >= 0.0)
        .map_or_else(|| "—".into(), |p| format!("{p:.1}W"));
    format!("Capacity: {:.0}%\nTime: {time}\nPower: {power}", state.percent)
}

pub fn network(state: &NetworkState, details: &WifiDetails) -> String {
    match &state.link {
        Link::Offline => "Disconnected".into(),
        Link::Wired => "Wired connection".into(),
        Link::Wifi { ssid, .. } => {
            let signal = details.strength.map_or_else(|| "—".into(), |s| format!("{s}%"));
            let frequency = details.frequency.filter(|f| *f > 0)
                .map_or_else(|| "—".into(), |f| format!("{f}MHz"));
            format!("SSID: {ssid}\nSignal: {signal}\nFrequency: {frequency}")
        }
    }
}

pub fn audio(state: &AudioState) -> String {
    if !state.available {
        return "No audio output".into();
    }
    let device = state.outputs.iter().find(|o| o.name == state.default)
        .map(|o| if o.description.is_empty() { o.name.as_str() } else { o.description.as_str() })
        .unwrap_or(&state.default);
    let device = if device.is_empty() { "—" } else { device };
    let muted = if state.muted { " (muted)" } else { "" };
    format!("Volume: {}%{muted}\nDevice: {device}", state.percent)
}

pub fn bluetooth(state: &BluetoothState) -> String {
    let connected: Vec<_> = state.devices.iter().filter(|d| d.connected).map(|d| {
        match d.battery {
            Some(battery) => format!("{} — {battery}%", d.name),
            None => d.name.clone(),
        }
    }).collect();
    if connected.is_empty() {
        format!("Bluetooth {}", if state.powered { "on" } else { "off" })
    } else {
        connected.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::audio::Output;
    use crate::services::bluetooth::Device;

    #[test]
    fn battery_uses_the_estimate_for_its_current_charge_state() {
        let mut state = BatteryState {
            percent: 72.0, charge: Charge::Charging, time_to_full: 4500,
            time_to_empty: 9000, ..BatteryState::default()
        };
        assert_eq!(battery(&state, Some(12.34)), "Capacity: 72%\nTime: 1 h 15 min\nPower: 12.3W");
        state.charge = Charge::Discharging;
        assert!(battery(&state, Some(0.0)).contains("Time: 2 h 30 min\nPower: 0.0W"));
        state.charge = Charge::Plugged;
        assert_eq!(battery(&state, None), "Capacity: 72%\nTime: —\nPower: —");
        assert!(battery(&state, Some(f64::NAN)).ends_with("Power: —"));
    }

    #[test]
    fn wifi_uses_exact_strength_not_the_icon_step_and_keeps_names_literal() {
        let mut state = NetworkState {
            link: Link::Wifi { ssid: "Office <5G> & guests".into(), strength: 66 },
            wifi_enabled: true,
        };
        let details = WifiDetails { strength: Some(87), frequency: Some(5180) };
        assert_eq!(network(&state, &details), "SSID: Office <5G> & guests\nSignal: 87%\nFrequency: 5180MHz");
        assert!(network(&state, &WifiDetails::default()).ends_with("Signal: —\nFrequency: —"));
        state.link = Link::Offline;
        assert_eq!(network(&state, &details), "Disconnected");
    }

    #[test]
    fn audio_follows_the_default_device_and_shows_muting() {
        let state = AudioState {
            available: true, percent: 42, muted: true, default: "headphones".into(),
            outputs: vec![
                Output { name: "speakers".into(), description: "Speakers".into() },
                Output { name: "headphones".into(), description: "USB Headphones".into() },
            ],
        };
        assert_eq!(audio(&state), "Volume: 42% (muted)\nDevice: USB Headphones");
    }

    #[test]
    fn bluetooth_lists_connected_devices_without_inventing_missing_charge() {
        let state = BluetoothState {
            powered: true,
            devices: vec![
                Device { name: "Headphones".into(), connected: true, battery: Some(0), ..Device::default() },
                Device { name: "Mouse".into(), connected: true, ..Device::default() },
                Device { name: "Offline keyboard".into(), battery: Some(80), ..Device::default() },
            ],
            ..BluetoothState::default()
        };
        assert_eq!(bluetooth(&state), "Headphones — 0%\nMouse");
    }
}

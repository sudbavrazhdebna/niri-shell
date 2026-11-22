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
        Link::Wifi => {
            let ssid = state.wifi.as_ref().map_or("", |wifi| wifi.ssid.as_str());
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

/// A single text layout keeps global GTK tooltip-label styling off individual days.
pub fn calendar(now: &gtk4::glib::DateTime, first_weekday: i32) -> Option<String> {
    use gtk4::glib;

    let first = glib::DateTime::from_local(now.year(), now.month(), 1, 12, 0, 0.0).ok()?;
    let last = first.add_months(1).ok()?.add_days(-1).ok()?.day_of_month();
    let start = first_weekday.rem_euclid(7);
    let offset = (first.day_of_week() - 1 - start).rem_euclid(7);
    let title = now.format("%A, %-e %B %Y").ok()?;
    let mut headings = Vec::new();
    for column in 0..7 {
        let date = first.add_days((start + column - first.day_of_week() + 1).rem_euclid(7)).ok()?;
        let name: String = date.format("%a").ok()?.chars().take(3).collect();
        headings.push(format!("{name:^3}"));
    }
    let mut rows = vec![glib::markup_escape_text(&headings.join(" ")).to_string()];
    let weeks = (offset + last + 6) / 7;
    for week in 0..weeks {
        let cells: Vec<_> = (0..7).map(|column| {
            let day = week * 7 + column - offset + 1;
            if day < 1 || day > last {
                "   ".into()
            } else if day == now.day_of_month() {
                format!("{}<b><u>{day}</u></b>", if day < 10 { "  " } else { " " })
            } else {
                format!("{day:>3}")
            }
        }).collect();
        rows.push(cells.join(" ").trim_end().to_string());
    }
    Some(format!("<b>{}</b>\n\n<tt>{}</tt>", glib::markup_escape_text(&title), rows.join("\n")))
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
            link: Link::Wifi,
            wifi_enabled: true,
            wifi: Some(crate::services::network::WifiLink { ssid: "Office <5G> & guests".into(), strength: 66 }),
            ..NetworkState::default()
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
        let mut state = BluetoothState::default();
        state.powered = true;
        state.devices = vec![
            Device { name: "Headphones".into(), connected: true, battery: Some(0), ..Device::default() },
            Device { name: "Mouse".into(), connected: true, ..Device::default() },
            Device { name: "Offline keyboard".into(), battery: Some(80), ..Device::default() },
        ];
        assert_eq!(bluetooth(&state), "Headphones — 0%\nMouse");
    }

    #[test]
    fn calendar_handles_leap_day_and_both_week_starts_without_colours() {
        let now = gtk4::glib::DateTime::from_local(2024, 2, 29, 12, 0, 0.0)
            .unwrap_or_else(|err| panic!("test date: {err}"));
        for start in [0, 6] {
            let markup = calendar(&now, start).unwrap_or_else(|| panic!("calendar missing"));
            let (_, plain, _) = gtk4::pango::parse_markup(&markup, '\0')
                .unwrap_or_else(|err| panic!("calendar markup: {err}"));
            assert!(markup.contains("<b><u>29</u></b>"));
            assert!(!markup.contains("color"));
            assert!(!plain.contains(" 30"));
            assert!(!plain.contains(" 31"));
            assert_eq!(plain.lines().count(), 8);
        }
        assert_ne!(calendar(&now, 0), calendar(&now, 6));
    }

    #[test]
    fn calendar_uses_six_rows_when_month_needs_them() {
        let now = gtk4::glib::DateTime::from_local(2026, 3, 31, 12, 0, 0.0)
            .unwrap_or_else(|err| panic!("test date: {err}"));
        let markup = calendar(&now, 0).unwrap_or_else(|| panic!("calendar missing"));
        let (_, plain, _) = gtk4::pango::parse_markup(&markup, '\0')
            .unwrap_or_else(|err| panic!("calendar markup: {err}"));
        assert_eq!(plain.lines().count(), 9);
        assert!(markup.contains("<b><u>31</u></b>"));
    }

    #[test]
    fn calendar_shows_today_in_the_heading_and_marks_its_number() {
        let now = gtk4::glib::DateTime::from_local(2026, 11, 7, 12, 0, 0.0)
            .unwrap_or_else(|err| panic!("test date: {err}"));
        let markup = calendar(&now, 0).unwrap_or_else(|| panic!("calendar missing"));
        let (_, plain, _) = gtk4::pango::parse_markup(&markup, '\0')
            .unwrap_or_else(|err| panic!("calendar markup: {err}"));
        let heading = plain.lines().next().unwrap_or_default();
        let weekday = now.format("%A").unwrap_or_else(|err| panic!("weekday: {err}"));
        assert!(heading.contains(weekday.as_str()));
        assert!(heading.contains('7') && heading.contains("2026"));
        assert!(markup.contains("<b><u>7</u></b>"));
    }
}

//! Battery icon by charge level, with a plug while on AC. Hidden without a battery.
//! Left click: charge details and the power profile.

use std::rc::Rc;

use gtk4::prelude::*;

use super::{Context, icon_module};
use crate::services::Services;
use crate::services::battery::{BatteryState, Charge};
use crate::{menu, ui};

pub fn build(ctx: &Context) -> gtk4::Widget {
    let image = icon_module("battery");
    ctx.services.battery().state.bind(&image, |image, state| {
        image.set_visible(state.present);
        // Like waybar's states: 41..50% shows the 50 icon.
        let level = ((state.percent / 10.0).ceil() as i32 * 10).clamp(10, 100);
        let kind = match state.charge {
            Charge::Discharging => "",
            Charge::Charging | Charge::Plugged => "charging-",
        };
        image.set_icon_name(Some(&format!("nshell-battery-{kind}{level}")));
    });

    let services = ctx.services.clone();
    menu::attach(&image, move |anchor| {
        menu::show(anchor, &menu_content(&services));
    });
    image.upcast()
}

fn menu_content(services: &Rc<Services>) -> gtk4::Box {
    let column = ui::column();
    column.set_width_request(260);
    let heading = ui::title("");
    let status = ui::note("");
    column.append(&heading);
    column.append(&status);
    column.append(&ui::separator());
    services.battery().state.bind(&column, move |_, state| {
        heading.set_label(&format!("Battery {:.0}%", state.percent));
        status.set_label(&status_text(state));
    });

    // Health and power: followed only while this menu is open.
    let facts = gtk4::Grid::new();
    facts.add_css_class("battery-facts");
    facts.set_column_spacing(16);
    facts.set_row_spacing(2);
    let fact = |row: i32, name: &str| {
        let key = gtk4::Label::new(Some(name));
        key.add_css_class("fact-name");
        key.set_xalign(0.0);
        let value = gtk4::Label::new(Some("…"));
        value.add_css_class("fact-value");
        value.set_xalign(0.0);
        facts.attach(&key, 0, row, 1, 1);
        facts.attach(&value, 1, row, 1, 1);
        value
    };
    let (health, energy, power) = (fact(0, "Health"), fact(1, "Energy"), fact(2, "Power"));
    let services_weak = Rc::downgrade(services);
    crate::services::battery::follow_details(&facts, move |d| {
        health.set_label(&format!("{:.0}%  ({:.1} of {:.0} Wh)", d.capacity, d.energy_full, d.energy_full_design));
        energy.set_label(&format!("{:.1} / {:.1} Wh", d.energy, d.energy_full));
        let charging = services_weak
            .upgrade()
            .is_some_and(|s| s.battery().state.get().charge == Charge::Charging);
        power.set_label(&match (d.rate, charging) {
            (r, _) if r < 0.05 => "—".to_string(),
            (r, true) => format!("charging at {r:.1} W"),
            (r, false) => format!("{r:.1} W"),
        });
    });
    column.append(&facts);

    let profiles = ui::column();
    let s = services.clone();
    services.power().state.bind(&profiles, move |profiles, state| {
        while let Some(child) = profiles.first_child() {
            profiles.remove(&child);
        }
        if state.profiles.is_empty() {
            return;
        }
        profiles.append(&ui::separator());
        profiles.append(&ui::title("Power mode"));
        for profile in &state.profiles {
            let (s, name) = (s.clone(), profile.clone());
            let icon = profile_icon(profile);
            profiles.append(&ui::checked_item(Some(icon), profile_name(profile), *profile == state.active, move || {
                s.power().set(&name);
            }));
        }
    });
    column.append(&profiles);
    column
}

/// The same icons as the control center's buttons.
fn profile_icon(profile: &str) -> &'static str {
    match profile {
        "power-saver" => "power-profile-power-saver-symbolic",
        "performance" => "power-profile-performance-symbolic",
        _ => "power-profile-balanced-symbolic",
    }
}

fn profile_name(profile: &str) -> &str {
    match profile {
        "power-saver" => "Power saver",
        "balanced" => "Balanced",
        "performance" => "Performance",
        other => other,
    }
}

fn status_text(state: &BatteryState) -> String {
    match state.charge {
        Charge::Charging if state.time_to_full > 0 => {
            format!("Charging, full in {}", duration(state.time_to_full))
        }
        Charge::Charging => "Charging".into(),
        Charge::Plugged if state.percent >= 99.0 => "Fully charged".into(),
        Charge::Plugged => "Plugged in, not charging (charge limit)".into(),
        Charge::Discharging if state.time_to_empty > 0 => {
            format!("On battery, {} left", duration(state.time_to_empty))
        }
        Charge::Discharging => "On battery".into(),
    }
}

/// 4500 → "1 h 15 min".
fn duration(seconds: i64) -> String {
    let minutes = seconds / 60;
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn durations() {
        assert_eq!(super::duration(4500), "1 h 15 min");
        assert_eq!(super::duration(600), "10 min");
        assert_eq!(super::duration(7200), "2 h");
    }
}

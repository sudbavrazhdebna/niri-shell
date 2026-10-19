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
    services.battery().state.bind(&column, move |_, state| {
        heading.set_label(&format!("Battery {:.0}%", state.percent));
        status.set_label(&status_text(state));
    });

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
            profiles.append(&ui::item(profile_name(profile), *profile == state.active, move || {
                s.power().set(&name);
            }));
        }
    });
    column.append(&profiles);
    column
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

//! Battery icon by charge level, with a plug while on AC. Hidden without a battery.

use gtk4::prelude::*;

use super::{Context, icon_module};
use crate::services::battery::Charge;

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
    image.upcast()
}

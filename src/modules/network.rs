//! Network icon: Wi-Fi by signal strength, wired, or off.

use gtk4::prelude::*;

use super::{Context, icon_module};
use crate::services::network::Link;

pub fn build(ctx: &Context) -> gtk4::Widget {
    let image = icon_module("network");
    ctx.services.network().state.bind(&image, |image, state| {
        // Thresholds as in waybar: 66+ full, 33+ two bars, 10+ one bar.
        let icon = match state.link {
            Link::Wifi { strength, .. } if strength >= 66 => "nshell-wifi-100",
            Link::Wifi { strength, .. } if strength >= 33 => "nshell-wifi-66",
            Link::Wifi { strength, .. } if strength >= 10 => "nshell-wifi-33",
            Link::Wifi { .. } | Link::Offline => "nshell-wifi-off",
            Link::Wired => "network-wired-symbolic",
        };
        image.set_icon_name(Some(icon));
    });
    image.upcast()
}

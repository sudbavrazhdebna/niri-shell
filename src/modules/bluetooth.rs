//! Bluetooth icon, shown only while the adapter is on.

use gtk4::prelude::*;

use super::{Context, icon_module};

pub fn build(ctx: &Context) -> gtk4::Widget {
    let image = icon_module("bluetooth");
    image.set_icon_name(Some("nshell-bluetooth"));
    ctx.services.bluetooth().state.bind(&image, |image, state| {
        image.set_visible(state.powered);
        if state.connected.is_empty() {
            image.remove_css_class("connected");
        } else {
            image.add_css_class("connected");
        }
    });
    image.upcast()
}

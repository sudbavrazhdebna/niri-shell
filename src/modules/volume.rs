//! Output volume icon. Scroll changes the volume, right or middle click mutes.

use gtk4::prelude::*;

use super::{Context, MIDDLE, RIGHT, icon_module, on_click, on_scroll};

pub fn build(ctx: &Context) -> Option<gtk4::Widget> {
    let audio = ctx.services.audio()?.clone();
    let image = icon_module("volume");

    audio.state.bind(&image, |image, state| {
        image.set_visible(state.available);
        let level = match state.percent {
            _ if state.muted => "mute",
            0 => "mute",
            1..=33 => "20",
            34..=66 => "50",
            _ => "100",
        };
        image.set_icon_name(Some(&format!("nshell-volume-{level}")));
    });

    let step = ctx.config.volume.scroll_step;
    let a = audio.clone();
    on_scroll(&image, move |steps| a.change_percent(steps * step));
    let a = audio.clone();
    on_click(&image, RIGHT, move || a.toggle_mute());
    on_click(&image, MIDDLE, move || audio.toggle_mute());
    Some(image.upcast())
}

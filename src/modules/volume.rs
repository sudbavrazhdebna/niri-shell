//! Output volume icon. Left click: menu with a slider and the output devices; scroll
//! changes the volume; right or middle click mutes.

use std::rc::Rc;

use gtk4::prelude::*;

use super::{Context, MIDDLE, RIGHT, icon_module, on_click, on_scroll};
use crate::services::audio::{Audio, AudioState, MAX_PERCENT};
use crate::{menu, ui};

pub fn build(ctx: &Context) -> Option<gtk4::Widget> {
    let audio = ctx.services.audio()?.clone();
    let image = icon_module("volume");

    audio.state.bind(&image, |image, state| {
        image.set_visible(state.available);
        image.set_icon_name(Some(icon(state)));
    });

    let step = ctx.config.volume.scroll_step;
    let a = audio.clone();
    on_scroll(&image, move |steps| a.change_percent(steps * step));
    let a = audio.clone();
    on_click(&image, RIGHT, move || a.toggle_mute());
    let a = audio.clone();
    on_click(&image, MIDDLE, move || a.toggle_mute());
    menu::attach(&image, move |anchor| {
        menu::show(anchor, &menu_content(&audio));
    });
    Some(image.upcast())
}

fn icon(state: &AudioState) -> &'static str {
    match state.percent {
        _ if state.muted => "nshell-volume-mute",
        0 => "nshell-volume-mute",
        1..=33 => "nshell-volume-20",
        34..=66 => "nshell-volume-50",
        _ => "nshell-volume-100",
    }
}

fn menu_content(audio: &Rc<Audio>) -> gtk4::Box {
    let column = ui::column();
    column.set_width_request(300);
    column.append(&ui::title("Звук"));

    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    row.add_css_class("menu-slider-row");
    let mute = gtk4::Button::new();
    mute.add_css_class("flat");
    mute.add_css_class("menu-icon-button");
    let mute_icon = gtk4::Image::new();
    mute.set_child(Some(&mute_icon));
    let a = audio.clone();
    mute.connect_clicked(move |_| a.toggle_mute());
    let a = audio.clone();
    let slider = ui::slider(f64::from(MAX_PERCENT), move |value| a.set_percent(value.round() as u32));
    let percent = gtk4::Label::new(None);
    percent.add_css_class("menu-value");
    percent.set_width_chars(4);
    percent.set_xalign(1.0);
    row.append(&mute);
    row.append(&slider);
    row.append(&percent);
    column.append(&row);

    audio.state.bind(&row, move |_, state| {
        mute_icon.set_icon_name(Some(icon(state)));
        slider.set_value(f64::from(state.percent));
        percent.set_label(&format!("{}%", state.percent));
    });

    let outputs = ui::column();
    let a = audio.clone();
    audio.state.bind(&outputs, move |outputs, state| {
        while let Some(child) = outputs.first_child() {
            outputs.remove(&child);
        }
        if state.outputs.len() < 2 {
            outputs.set_visible(false);
            return;
        }
        outputs.set_visible(true);
        outputs.append(&ui::separator());
        outputs.append(&ui::title("Выход"));
        for output in &state.outputs {
            let (a, name) = (a.clone(), output.name.clone());
            outputs.append(&ui::item(&output.description, output.name == state.default, move || {
                a.set_default(&name);
            }));
        }
    });
    column.append(&outputs);
    column
}

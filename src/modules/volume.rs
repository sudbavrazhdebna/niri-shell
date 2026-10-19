//! Output volume icon. Left click: menu with a slider and the output devices; scroll
//! changes the volume; right or middle click mutes.

use std::cell::RefCell;
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
    column.set_width_request(320);
    column.append(&ui::title("Sound"));

    let (a, b) = (audio.clone(), audio.clone());
    let output = LevelRow::new(move || a.toggle_mute(), move |v| b.set_percent(v));
    column.append(&output.row);
    let row = output.row.clone();
    audio.state.bind(&row, move |_, state| {
        output.show(icon(state), state.percent, state.muted);
    });

    // The microphone, if there is one.
    let (a, b) = (audio.clone(), audio.clone());
    let mic = LevelRow::new(move || a.toggle_mic_mute(), move |v| b.set_mic_percent(v));
    let mic_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    mic_box.append(&ui::title("Microphone"));
    mic_box.append(&mic.row);
    audio.mic.bind(&mic_box, move |mic_box, state| {
        mic_box.set_visible(state.is_some());
        if let Some(state) = state {
            let icon = if state.muted || state.percent == 0 {
                "microphone-sensitivity-muted-symbolic"
            } else {
                "audio-input-microphone-symbolic"
            };
            mic.show(icon, state.percent, state.muted);
        }
    });
    column.append(&mic_box);

    // Programs playing sound, each with its own volume.
    let apps = ui::column();
    let rows: Rc<RefCell<Vec<(u32, LevelRow)>>> = Rc::default();
    let a = audio.clone();
    audio.apps.bind(&apps, move |apps, streams| {
        let same = rows.borrow().iter().map(|(i, _)| *i).eq(streams.iter().map(|s| s.index));
        if !same {
            ui::clear(apps);
            rows.borrow_mut().clear();
            if !streams.is_empty() {
                apps.append(&ui::separator());
                apps.append(&ui::title("Apps"));
            }
            for stream in streams {
                let (a1, a2, index) = (a.clone(), a.clone(), stream.index);
                let row = LevelRow::new(move || a1.toggle_app_mute(index), move |v| a2.set_app_percent(index, v));
                let name = gtk4::Label::new(Some(&stream.name));
                name.add_css_class("app-stream-name");
                name.set_xalign(0.0);
                name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
                apps.append(&name);
                apps.append(&row.row);
                rows.borrow_mut().push((index, row));
            }
        }
        for (stream, (_, row)) in streams.iter().zip(rows.borrow().iter()) {
            let icon = if stream.icon.is_empty() { "audio-volume-high-symbolic" } else { &stream.icon };
            row.show(if stream.muted { "audio-volume-muted-symbolic" } else { icon }, stream.percent, stream.muted);
        }
    });
    column.append(&apps);

    let outputs = ui::column();
    let a = audio.clone();
    let shown = RefCell::new(None::<(Vec<crate::services::audio::Output>, String)>);
    audio.state.bind(&outputs, move |outputs, state| {
        // Volume steps change the state too; the list only when the devices do.
        let now = (state.outputs.clone(), state.default.clone());
        if shown.borrow().as_ref() == Some(&now) {
            return;
        }
        shown.replace(Some(now));
        ui::clear(outputs);
        if state.outputs.len() < 2 {
            outputs.set_visible(false);
            return;
        }
        outputs.set_visible(true);
        outputs.append(&ui::separator());
        outputs.append(&ui::title("Output"));
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

/// Icon button (mute), slider and percent: one volume.
struct LevelRow {
    row: gtk4::Box,
    icon: gtk4::Image,
    slider: ui::Slider,
    percent: gtk4::Label,
}

impl LevelRow {
    fn new(on_mute: impl Fn() + 'static, on_change: impl Fn(u32) + 'static) -> LevelRow {
        let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        row.add_css_class("menu-slider-row");
        let mute = gtk4::Button::new();
        mute.add_css_class("flat");
        mute.add_css_class("menu-icon-button");
        mute.set_cursor_from_name(Some("pointer"));
        let icon = gtk4::Image::new();
        mute.set_child(Some(&icon));
        mute.connect_clicked(move |_| on_mute());
        let slider = ui::Slider::new(f64::from(MAX_PERCENT), move |value| on_change(value.round() as u32));
        let percent = gtk4::Label::new(None);
        percent.add_css_class("menu-value");
        percent.set_width_chars(4);
        percent.set_xalign(1.0);
        row.append(&mute);
        row.append(slider.widget());
        row.append(&percent);
        LevelRow { row, icon, slider, percent }
    }

    fn show(&self, icon: &str, percent: u32, muted: bool) {
        self.icon.set_icon_name(Some(icon));
        self.slider.show(f64::from(percent));
        self.slider.set_dimmed(muted);
        self.percent.set_label(&format!("{percent}%"));
    }
}

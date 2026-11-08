//! Output volume icon. Left click: menu with a slider and the output devices; scroll
//! changes the volume; right or middle click mutes.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;

use super::{Context, MIDDLE, RIGHT, icon_module, on_click, on_scroll, tooltips};
use crate::services::audio::{AppStream, Audio, AudioState, MAX_PERCENT};
use crate::{menu, ui};

pub fn build(ctx: &Context) -> Option<gtk4::Widget> {
    let audio = ctx.services.audio()?.clone();
    let image = icon_module("volume");
    let a = audio.clone();
    tooltips::text(&image, move || Some(tooltips::audio(&a.state.get())));

    audio.state.bind(&image, |image, state| {
        image.set_visible(state.available);
        ui::set_icon(image, icon(state));
        image.trigger_tooltip_query();
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
    column.add_css_class("sound-menu");
    column.set_width_request(320);
    column.append(&ui::title("Sound"));
    column.append(&ui::separator());

    let (a, b) = (Rc::downgrade(audio), Rc::downgrade(audio));
    let output = LevelRow::new(
        move || { if let Some(a) = a.upgrade() { a.toggle_mute(); } },
        move |v| { if let Some(b) = b.upgrade() { b.set_percent(v); } },
    );
    column.append(&output.row);
    // Bound to the column, not the row: the closure owns the row, which would then
    // never go away, and neither would the listener.
    audio.state.bind(&column, move |_, state| {
        output.show(icon(state), state.percent, state.muted);
    });

    // Programs playing sound, each with its own volume.
    let apps = ui::column();
    apps.append(&ui::separator());
    let toggle = gtk4::ToggleButton::new();
    toggle.add_css_class("flat");
    toggle.add_css_class("sound-apps-toggle");
    toggle.set_cursor_from_name(Some("pointer"));
    toggle.update_state(&[gtk4::accessible::State::Expanded(Some(false))]);
    let heading = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let title = ui::title("Apps");
    title.set_hexpand(true);
    let count = gtk4::Label::new(None);
    count.add_css_class("sound-apps-count");
    let chevron = gtk4::Image::from_icon_name("pan-end-symbolic");
    chevron.add_css_class("sound-apps-chevron");
    heading.append(&title);
    heading.append(&count);
    heading.append(&chevron);
    toggle.set_child(Some(&heading));
    apps.append(&toggle);
    let list = ui::column();
    let revealer = gtk4::Revealer::new();
    revealer.set_transition_type(gtk4::RevealerTransitionType::SlideDown);
    revealer.set_transition_duration(ui::ANIMATION_MS);
    revealer.set_child(Some(&list));
    apps.append(&revealer);
    let weak = revealer.downgrade();
    toggle.connect_toggled(move |toggle| {
        if let Some(revealer) = weak.upgrade() {
            let open = toggle.is_active();
            revealer.set_reveal_child(open);
            toggle.update_state(&[gtk4::accessible::State::Expanded(Some(open))]);
        }
    });
    let rows: Rc<RefCell<Vec<(u32, AppRow)>>> = Rc::default();
    let a = Rc::downgrade(audio);
    audio.apps.bind(&apps, move |apps, streams| {
        apps.set_visible(!streams.is_empty());
        count.set_label(&streams.len().to_string());
        toggle.update_property(&[gtk4::accessible::Property::Label(&format!("Apps, {} audio streams", streams.len()))]);
        if streams.is_empty() {
            toggle.set_active(false);
        }
        let same = rows.borrow().iter().map(|(i, _)| *i).eq(streams.iter().map(|s| s.index));
        if !same {
            // Rebuild only the list: changing streams must not reset the disclosure.
            ui::clear(&list);
            rows.borrow_mut().clear();
            for stream in streams {
                let (a1, a2, index) = (a.clone(), a.clone(), stream.index);
                let level = LevelRow::new(
                    move || { if let Some(a) = a1.upgrade() { a.toggle_app_mute(index); } },
                    move |v| { if let Some(a) = a2.upgrade() { a.set_app_percent(index, v); } },
                );
                level.row.add_css_class("app-stream-row");
                list.append(&level.row);
                let row = AppRow { level, identity: RefCell::default() };
                rows.borrow_mut().push((index, row));
            }
        }
        for (stream, (_, row)) in streams.iter().zip(rows.borrow().iter()) {
            row.show(stream);
        }
    });

    let outputs = ui::column();
    let a = Rc::downgrade(audio);
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
                if let Some(a) = a.upgrade() { a.set_default(&name); }
            }));
        }
    });
    column.append(&outputs);
    column.append(&apps);
    column.append(&ui::separator());
    column.append(&ui::item("Sound Settings…", false, || {
        menu::close();
        ui::launch("pavucontrol");
    }));
    column
}

/// Icon button (mute), slider and percent: one volume.
struct LevelRow {
    row: gtk4::Box,
    icon: gtk4::Image,
    mute: gtk4::Button,
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
        LevelRow { row, icon, mute, slider, percent }
    }

    fn show(&self, icon: &str, percent: u32, muted: bool) {
        ui::set_icon(&self.icon, icon);
        self.show_level(percent, muted);
    }

    fn show_level(&self, percent: u32, muted: bool) {
        self.slider.show(f64::from(percent));
        self.slider.set_dimmed(muted);
        self.percent.set_label(&format!("{percent}%"));
    }
}

type AppIdentity = (String, String, String, String);

struct AppRow {
    level: LevelRow,
    identity: RefCell<Option<AppIdentity>>,
}

impl AppRow {
    fn show(&self, stream: &AppStream) {
        let identity = (stream.name.clone(), stream.icon.clone(), stream.app_id.clone(), stream.binary.clone());
        if self.identity.borrow().as_ref() != Some(&identity) {
            self.identity.replace(Some(identity));
            match app_icon(stream) {
                Some(icon) => self.level.icon.set_from_gicon(&icon),
                None => ui::set_icon(&self.level.icon, "application-x-executable-symbolic"),
            }
        }
        // Muting must not replace the app's identity with a generic speaker glyph.
        self.level.icon.set_opacity(if stream.muted { 0.4 } else { 1.0 });
        self.level.show_level(stream.percent, stream.muted);
        let name = if stream.name.is_empty() { "Application" } else { &stream.name };
        let action = if stream.muted { "Unmute" } else { "Mute" };
        self.level.mute.set_tooltip_text(Some(&format!("{name}\n{action}")));
        self.level.mute.update_property(&[gtk4::accessible::Property::Label(&format!("{action} {name}"))]);
        self.level.slider.widget().update_property(&[gtk4::accessible::Property::Label(&format!("{name} volume"))]);
    }
}

fn app_icon(stream: &AppStream) -> Option<gtk4::gio::Icon> {
    if !stream.icon.is_empty()
        && let Some(display) = gtk4::gdk::Display::default()
        && gtk4::IconTheme::for_display(&display).has_icon(&stream.icon)
    {
        return Some(gtk4::gio::ThemedIcon::new(&stream.icon).upcast());
    }
    [&stream.app_id, &stream.binary, &stream.name]
        .into_iter()
        .filter(|id| !id.is_empty())
        .find_map(|id| crate::apps::icon(id))
}

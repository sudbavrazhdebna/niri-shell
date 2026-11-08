//! Control center: one menu with the everyday switches, sliders, the player and the
//! session buttons.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gtk4::glib;
use gtk4::prelude::*;

use super::{Context, power_profiles};
use crate::services::Services;
use crate::services::network::{Link, NetworkState};
use crate::services::audio::MAX_PERCENT;
use crate::{menu, ui};

/// A dangerous button (reboot, power off) acts on a second click within this time.
const CONFIRM_TIME: Duration = Duration::from_secs(3);

pub fn build(ctx: &Context) -> gtk4::Widget {
    // Rotate only the image; the padded hit area and hover plate stay in place.
    let root = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    for class in ["module", "icon", "control"] {
        root.add_css_class(class);
    }
    let image = gtk4::Image::from_icon_name("nshell-control");
    image.add_css_class("control-chevron");
    root.append(&image);
    super::tooltips::text(&root, || Some("Control center".into()));
    ctx.services.session().set_lock_command(&ctx.config.idle.lock_command);
    // Start what the menu shows now, so it is known when the menu opens: started on the
    // first opening, the power profiles row came a few frames late and the menu grew and
    // jumped sideways. (Network and Bluetooth usually start with their bar modules.)
    ctx.services.media();
    ctx.services.power();
    ctx.services.network();
    ctx.services.bluetooth();
    ctx.services.keyboard_light();
    // Sandbox testing: NSHELL_DEV_AWAKE=1 turns "stay awake" on at start.
    #[cfg(debug_assertions)]
    if std::env::var_os("NSHELL_DEV_AWAKE").is_some() {
        ctx.services.session().set_awake(true);
    }
    let services = ctx.services.clone();
    menu::attach(&root, move |anchor| menu::show(anchor, &content(&services)));
    root.upcast()
}

fn content(services: &Rc<Services>) -> gtk4::Box {
    let column = ui::column();
    column.add_css_class("control-center");
    column.set_width_request(340);
    column.append(&tiles(services));
    append_sliders(&column, services);
    column.append(&system_stats());
    column.append(&player(services));
    column.append(&power_profiles::build(services));
    column.append(&session_buttons(services));
    column
}

fn tiles(services: &Rc<Services>) -> gtk4::Box {
    // Hidden hardware controls leave no empty column in the homogeneous row.
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    row.add_css_class("tiles");
    row.set_homogeneous(true);

    let s = services.clone();
    let wifi = tile("network-wireless-symbolic", "Wi-Fi", move |on| s.network().set_wifi_enabled(on));
    services.network().state.bind(&wifi.button, {
        let (state_label, set, image) = (wifi.subtitle.clone(), wifi.set.clone(), wifi.image.clone());
        move |button, state| {
            set(state.wifi_enabled);
            ui::set_icon(&image, wifi_icon(state));
            state_label.set_label(&match &state.link {
                _ if !state.wifi_enabled => "Off".into(),
                Link::Wifi { ssid, .. } => ssid.clone(),
                Link::Wired => "Wired".into(),
                Link::Offline => "Not connected".into(),
            });
            button.set_tooltip_text(Some(&format!("Wi-Fi\n{}", state_label.text())));
        }
    });

    let s = services.clone();
    let bt = tile("bluetooth-symbolic", "Bluetooth", move |on| s.bluetooth().set_powered(on));
    services.bluetooth().state.bind(&bt.button, {
        let (state_label, set, image) = (bt.subtitle.clone(), bt.set.clone(), bt.image.clone());
        move |button, state| {
            set(state.powered);
            let connected: Vec<_> = state.devices.iter().filter(|d| d.connected).map(|d| d.name.clone()).collect();
            ui::set_icon(&image, if state.powered { "bluetooth-symbolic" } else { "bluetooth-disabled-symbolic" });
            state_label.set_label(&match (state.powered, connected.first()) {
                (false, _) => "Off".to_string(),
                (true, Some(name)) => name.clone(),
                (true, None) => "On".into(),
            });
            button.set_tooltip_text(Some(&format!("Bluetooth\n{}", state_label.text())));
        }
    });

    let s = services.clone();
    let awake = tile("weather-clear-night-symbolic", "Stay awake", move |on| s.session().set_awake(on));
    services.session().awake.bind(&awake.button, {
        let (state_label, set) = (awake.subtitle.clone(), awake.set.clone());
        move |button, on| {
            set(*on);
            state_label.set_label(if *on { "On" } else { "Off" });
            button.set_tooltip_text(Some(if *on { "Stay awake: screen stays on" } else { "Stay awake: off" }));
        }
    });

    row.append(&wifi.button);
    row.append(&bt.button);
    row.append(&keyboard_light_tile(services));
    row.append(&awake.button);
    row
}

/// Keyboard backlight: a click steps through the hardware levels (Off → 50% → 100%).
fn keyboard_light_tile(services: &Rc<Services>) -> gtk4::Button {
    let light = services.keyboard_light();
    // Hidden until UPower answers, and for good on machines without a backlight.
    let s = services.clone();
    let tile = tile("keyboard-brightness-symbolic", "Key light", move |_| {
        let light = s.keyboard_light();
        if let Some((now, max)) = *light.level.get() {
            light.set(if now >= max { 0 } else { now + 1 });
        }
    });
    tile.button.add_css_class("keyboard-light");
    let (subtitle, set, image) = (tile.subtitle.clone(), tile.set.clone(), tile.image.clone());
    light.level.bind(&tile.button, move |button, level| {
        button.set_visible(level.is_some());
        let Some((now, max)) = *level else { return };
        set(now > 0);
        ui::set_icon(&image, keyboard_light_icon(now, max));
        subtitle.set_label(&level_name(now, max));
        button.set_tooltip_text(Some(&format!("Keyboard light: {}\nClick to cycle levels", subtitle.text())));
    });
    tile.button
}

/// Show the hardware level as a percentage, including the two lit levels 50% / 100%.
fn level_name(now: i32, max: i32) -> String {
    if now <= 0 { "Off".into() } else { format!("{}%", i64::from(now) * 100 / i64::from(max.max(1))) }
}

struct Tile {
    button: gtk4::Button,
    image: gtk4::Image,
    subtitle: gtk4::Label,
    /// Shows on/off without calling the toggle callback.
    set: Rc<dyn Fn(bool)>,
}

/// One compact tile: an icon circle above its name and state.
fn tile(icon: &str, name: &str, on_toggle: impl Fn(bool) + 'static) -> Tile {
    let button = gtk4::Button::new();
    button.add_css_class("tile");
    let column = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    column.set_valign(gtk4::Align::Center);
    let image = gtk4::Image::from_icon_name(icon);
    image.add_css_class("tile-icon");
    image.set_halign(gtk4::Align::Center);
    let title = gtk4::Label::new(Some(name));
    title.add_css_class("tile-title");
    title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    title.set_max_width_chars(10);
    let subtitle = gtk4::Label::new(None);
    subtitle.add_css_class("tile-subtitle");
    subtitle.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    subtitle.set_max_width_chars(10);
    column.append(&image);
    column.append(&title);
    column.append(&subtitle);
    button.set_child(Some(&column));

    let on = Rc::new(Cell::new(false));
    let o = on.clone();
    button.connect_clicked(move |_| on_toggle(!o.get()));
    let weak = button.downgrade();
    let set = Rc::new(move |value: bool| {
        on.set(value);
        if let Some(button) = weak.upgrade() {
            if value {
                button.add_css_class("active");
            } else {
                button.remove_css_class("active");
            }
        }
    });
    Tile { button, image, subtitle, set }
}

fn wifi_icon(state: &NetworkState) -> &'static str {
    // This button only sets WirelessEnabled; it does not enable airplane mode.
    if !state.wifi_enabled {
        return "network-wireless-disabled-symbolic";
    }
    match state.link {
        Link::Wifi { strength: 66.., .. } => "network-wireless-signal-excellent-symbolic",
        Link::Wifi { strength: 33.., .. } => "network-wireless-signal-ok-symbolic",
        Link::Wifi { .. } => "network-wireless-signal-weak-symbolic",
        Link::Offline => "network-wireless-offline-symbolic",
        Link::Wired => "network-wireless-symbolic",
    }
}

fn volume_icon(percent: u32, muted: bool) -> &'static str {
    match percent {
        _ if muted => "audio-volume-muted-symbolic",
        0 => "audio-volume-muted-symbolic",
        1..=33 => "audio-volume-low-symbolic",
        34..=66 => "audio-volume-medium-symbolic",
        _ => "audio-volume-high-symbolic",
    }
}

fn brightness_icon(percent: u32) -> &'static str {
    match percent {
        0..=33 => "nshell-brightness-low-symbolic",
        34..=66 => "nshell-brightness-medium-symbolic",
        _ => "display-brightness-symbolic",
    }
}

fn keyboard_light_icon(now: i32, max: i32) -> &'static str {
    if now <= 0 {
        "keyboard-brightness-symbolic"
    } else if i64::from(now) * 2 <= i64::from(max) {
        "nshell-keyboard-light-low-symbolic"
    } else {
        "keyboard-brightness-symbolic"
    }
}

fn append_sliders(column: &gtk4::Box, services: &Rc<Services>) {
    if let Some(audio) = services.audio() {
        let a = audio.clone();
        let slider = ui::Slider::new(f64::from(MAX_PERCENT), move |v| a.set_percent(v.round() as u32)).with_smoothing();
        let (row, image) = slider_card("audio-volume-high-symbolic", "Sound", slider.widget());
        let s = slider.clone();
        audio.state.bind(&row, move |row, state| {
            row.set_visible(state.available);
            s.show(f64::from(state.percent));
            s.set_dimmed(state.muted);
            ui::set_icon(&image, volume_icon(state.percent, state.muted));
        });
        column.append(&row);
    }

    let brightness = services.brightness();
    if brightness.percent.get().is_some() {
        let s = services.clone();
        let slider = ui::Slider::new(100.0, move |v| s.brightness().set_percent(v.round() as u32)).with_smoothing();
        let (row, image) = slider_card("display-brightness-symbolic", "Brightness", slider.widget());
        let s = slider.clone();
        brightness
            .percent
            .bind(&row, move |row, percent| {
                row.set_visible(percent.is_some());
                let percent = percent.unwrap_or(0);
                s.show(f64::from(percent));
                ui::set_icon(&image, brightness_icon(percent));
            });
        column.append(&row);
    }
}

fn slider_card(icon: &str, name: &str, slider: &gtk4::Scale) -> (gtk4::Box, gtk4::Image) {
    let card = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    card.add_css_class("control-slider");
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let image = gtk4::Image::from_icon_name(icon);
    image.add_css_class("menu-row-icon");
    let label = gtk4::Label::new(Some(name));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    let value = gtk4::Label::new(Some("0%"));
    value.add_css_class("menu-value");
    row.append(&image);
    row.append(&label);
    row.append(&value);
    card.append(&row);
    card.append(slider);
    slider.connect_value_changed(move |slider| value.set_label(&format!("{:.0}%", slider.value())));
    (card, image)
}

/// CPU and memory use. Read every 2 s, and only while the menu is open: the timer
/// stops by itself once the widget is gone.
fn system_stats() -> gtk4::Box {
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 16);
    row.add_css_class("system-stats");
    row.set_homogeneous(true);
    let gauge = |name: &str| {
        let column = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        let top = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        let title = gtk4::Label::new(Some(name));
        title.add_css_class("stat-name");
        title.set_xalign(0.0);
        title.set_hexpand(true);
        let value = gtk4::Label::new(Some("…"));
        value.add_css_class("stat-value");
        top.append(&title);
        top.append(&value);
        let bar = gtk4::LevelBar::new();
        bar.add_css_class("stat-bar");
        bar.remove_offset_value(Some(gtk4::LEVEL_BAR_OFFSET_LOW));
        bar.remove_offset_value(Some(gtk4::LEVEL_BAR_OFFSET_HIGH));
        bar.remove_offset_value(Some(gtk4::LEVEL_BAR_OFFSET_FULL));
        column.append(&top);
        column.append(&bar);
        (column, value, bar)
    };
    let (cpu_box, cpu_value, cpu_bar) = gauge("CPU");
    let (ram_box, ram_value, ram_bar) = gauge("Memory");
    row.append(&cpu_box);
    row.append(&ram_box);

    let previous = Cell::new(cpu_times());
    let update = move || {
        if let (Some((busy0, total0)), Some((busy1, total1))) = (previous.get(), cpu_times()) {
            // Saturating: the counters drop when a CPU goes offline.
            let share = busy1.saturating_sub(busy0) as f64 / total1.saturating_sub(total0).max(1) as f64;
            cpu_value.set_label(&format!("{:.0}%", share * 100.0));
            cpu_bar.set_value(share.clamp(0.0, 1.0));
            previous.set(Some((busy1, total1)));
        }
        if let Some((used, total)) = memory() {
            ram_value.set_label(&format!("{:.1} / {:.0} GB", used as f64 / 1048576.0, total as f64 / 1048576.0));
            ram_bar.set_value(used as f64 / total.max(1) as f64);
        }
    };
    update();
    let weak = row.downgrade();
    glib::timeout_add_seconds_local(2, move || {
        if weak.upgrade().is_none() {
            return glib::ControlFlow::Break;
        }
        update();
        glib::ControlFlow::Continue
    });
    row
}

/// (busy, total) jiffies of all CPUs from /proc/stat.
fn cpu_times() -> Option<(u64, u64)> {
    let stat = std::fs::read_to_string("/proc/stat").ok()?;
    let fields: Vec<u64> = stat.lines().next()?.split_whitespace().skip(1).filter_map(|f| f.parse().ok()).collect();
    let total: u64 = fields.iter().sum();
    // idle + iowait don't count as busy.
    let idle = fields.get(3)? + fields.get(4).unwrap_or(&0);
    Some((total.saturating_sub(idle), total))
}

/// (used, total) memory in KiB from /proc/meminfo; "used" leaves out caches.
fn memory() -> Option<(u64, u64)> {
    let info = std::fs::read_to_string("/proc/meminfo").ok()?;
    let field = |name: &str| {
        info.lines()
            .find(|l| l.starts_with(name))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse::<u64>().ok())
    };
    let total = field("MemTotal:")?;
    Some((total.saturating_sub(field("MemAvailable:")?), total))
}

/// What plays, with previous / play-pause / next. Hidden when nothing plays.
fn player(services: &Rc<Services>) -> gtk4::Box {
    let card = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    card.add_css_class("player");
    let text = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    text.set_hexpand(true);
    let title = gtk4::Label::new(None);
    title.add_css_class("player-title");
    title.set_xalign(0.0);
    title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    title.set_max_width_chars(24);
    let artist = gtk4::Label::new(None);
    artist.add_css_class("player-artist");
    artist.set_xalign(0.0);
    artist.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    artist.set_max_width_chars(24);
    text.append(&title);
    text.append(&artist);
    card.append(&text);

    let button = |icon: &str, action: fn(&crate::services::media::Media), s: &Rc<Services>| {
        let b = gtk4::Button::new();
        let image = gtk4::Image::from_icon_name(icon);
        b.set_child(Some(&image));
        b.add_css_class("flat");
        b.add_css_class("image-button");
        b.add_css_class("player-button");
        let s = s.clone();
        b.connect_clicked(move |_| action(s.media()));
        (b, image)
    };
    let (previous, _) = button("media-skip-backward-symbolic", |m| m.previous(), services);
    let (play, play_image) = button("media-playback-start-symbolic", |m| m.play_pause(), services);
    let (next, _) = button("media-skip-forward-symbolic", |m| m.next(), services);
    card.append(&previous);
    card.append(&play);
    card.append(&next);

    services.media().current.bind(&card, move |card, track| {
        card.set_visible(track.is_some());
        let Some(track) = track else { return };
        title.set_label(if track.title.is_empty() { "Untitled" } else { &track.title });
        artist.set_label(&track.artist);
        artist.set_visible(!track.artist.is_empty());
        // Player updates must not replace a button's child during pointer interaction.
        ui::set_icon(&play_image, if track.playing {
            "media-playback-pause-symbolic"
        } else {
            "media-playback-start-symbolic"
        });
        previous.set_sensitive(track.can_previous);
        next.set_sensitive(track.can_next);
    });
    card
}

fn session_buttons(services: &Rc<Services>) -> gtk4::Box {
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    row.add_css_class("session-buttons");
    row.set_homogeneous(true);
    let s = services.clone();
    row.append(&session_button("system-lock-screen-symbolic", "Lock", false, move || {
        menu::close();
        s.session().lock();
    }));
    let s = services.clone();
    row.append(&session_button("weather-clear-night-symbolic", "Sleep", false, move || {
        menu::close();
        s.session().suspend();
    }));
    let s = services.clone();
    row.append(&session_button("system-reboot-symbolic", "Restart", true, move || {
        s.session().reboot();
    }));
    let s = services.clone();
    row.append(&session_button("system-shutdown-symbolic", "Shut down", true, move || {
        s.session().power_off();
    }));
    row
}

/// Round icon button with a caption. With `confirm`, the first click only arms it
/// ("Again?") for a few seconds.
fn session_button(icon: &str, caption: &str, confirm: bool, action: impl Fn() + 'static) -> gtk4::Box {
    let holder = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    let button = gtk4::Button::from_icon_name(icon);
    button.add_css_class("session-button");
    button.set_halign(gtk4::Align::Center);
    let label = gtk4::Label::new(Some(caption));
    label.add_css_class("session-caption");
    holder.append(&button);
    holder.append(&label);

    let armed = Rc::new(Cell::new(false));
    let caption = caption.to_string();
    let label_weak = label.downgrade();
    button.connect_clicked(move |button| {
        if !confirm || armed.get() {
            action();
            return;
        }
        armed.set(true);
        button.add_css_class("armed");
        if let Some(label) = label_weak.upgrade() {
            label.set_label("Again?");
        }
        let (armed, button, label, caption) =
            (armed.clone(), button.downgrade(), label_weak.clone(), caption.clone());
        glib::timeout_add_local_once(CONFIRM_TIME, move || {
            armed.set(false);
            if let Some(button) = button.upgrade() {
                button.remove_css_class("armed");
            }
            if let Some(label) = label.upgrade() {
                label.set_label(&caption);
            }
        });
    });
    holder
}

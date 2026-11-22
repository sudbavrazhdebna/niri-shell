//! Control center: one menu with the everyday switches, sliders, the player and the
//! session buttons.

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk4::glib;
use gtk4::prelude::*;

use super::{Context, power_profiles};
use crate::config::ControlLayout;
use crate::services::Services;
use crate::services::network::NetworkState;
use crate::services::audio::MAX_PERCENT;
use crate::{menu, ui, watch::Watch};

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
    let layout = ctx.config.control_layout;
    menu::attach(&root, move |anchor| menu::show(anchor, &content(&services, layout)));
    root.upcast()
}

fn content(services: &Rc<Services>, layout: ControlLayout) -> gtk4::Box {
    let column = ui::column();
    column.add_css_class("control-center");
    column.set_width_request(340);
    column.append(&tiles(services, layout));
    append_sliders(&column, services);
    column.append(&system_stats());
    column.append(&player(services));
    column.append(&power_profiles::build(services));
    column.append(&session_menu(services));
    column
}

fn tiles(services: &Rc<Services>, layout: ControlLayout) -> gtk4::Widget {
    let s = services.clone();
    let wifi = tile(layout, "network-wireless-symbolic", "Wi-Fi", move |on| s.network().set_wifi_enabled(on));
    services.network().state.bind(&wifi.button, {
        let (state_label, set, image) = (wifi.subtitle.clone(), wifi.set.clone(), wifi.image.clone());
        move |button, state| {
            button.set_visible(state.has_wifi_adapter);
            set(state.wifi_enabled);
            ui::set_icon(&image, wifi_icon(state));
            let label = if state.wifi_enabled {
                state.wifi.as_ref().map_or("Not connected", |wifi| wifi.ssid.as_str())
            } else {
                "Off"
            };
            state_label.set_label(label);
            button.set_tooltip_text(Some(&format!("Wi-Fi\n{}", state_label.text())));
        }
    });

    let s = services.clone();
    let bt = tile(layout, "bluetooth-symbolic", "Bluetooth", move |on| s.bluetooth().set_powered(on));
    services.bluetooth().state.bind(&bt.button, {
        let (state_label, set, image) = (bt.subtitle.clone(), bt.set.clone(), bt.image.clone());
        move |button, state| {
            button.set_visible(state.has_adapter);
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
    let awake = tile(layout, "weather-clear-night-symbolic", "Stay awake", move |on| s.session().set_awake(on));
    services.session().awake.bind(&awake.button, {
        let (state_label, set) = (awake.subtitle.clone(), awake.set.clone());
        move |button, on| {
            set(*on);
            state_label.set_label(if *on { "On" } else { "Off" });
            button.set_tooltip_text(Some(if *on { "Stay awake: screen stays on" } else { "Stay awake: off" }));
        }
    });

    let buttons = [wifi.button, bt.button, keyboard_light_tile(services, layout), awake.button];
    match layout {
        ControlLayout::Grid => tile_grid(&buttons).upcast(),
        ControlLayout::Row => {
            let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
            row.add_css_class("tiles");
            row.set_homogeneous(true);
            for button in buttons {
                row.append(&button);
            }
            row.upcast()
        }
    }
}

fn tile_grid(buttons: &[gtk4::Button]) -> gtk4::Grid {
    let grid = gtk4::Grid::builder()
        .column_spacing(8)
        .row_spacing(8)
        .column_homogeneous(true)
        .build();
    grid.add_css_class("tiles");
    grid.add_css_class("tiles-grid");
    for (index, button) in buttons.iter().enumerate() {
        grid.attach(button, (index % 2) as i32, (index / 2) as i32, 1, 1);
        let weak = grid.downgrade();
        button.connect_visible_notify(move |_| {
            if let Some(grid) = weak.upgrade() {
                arrange_tiles(&grid);
            }
        });
    }
    arrange_tiles(&grid);
    grid
}

/// Pack the visible tiles without replacing their widgets. A lone tile fills its row.
fn arrange_tiles(grid: &gtk4::Grid) {
    let Some(manager) = grid.layout_manager() else { return };
    let tiles: Vec<_> = std::iter::successors(grid.first_child(), |child| child.next_sibling())
        .filter(|child| child.get_visible())
        .collect();
    let count = tiles.len();
    for (index, tile) in tiles.iter().enumerate() {
        let Some(cell) = manager.layout_child(tile).downcast::<gtk4::GridLayoutChild>().ok() else { continue };
        let last_incomplete_row = index % 2 == 0 && index + 1 == count;
        cell.set_column((index % 2) as i32);
        cell.set_row((index / 2) as i32);
        cell.set_column_span(if last_incomplete_row { 2 } else { 1 });
    }
}

/// Keyboard backlight: a click steps through the hardware levels (Off → 50% → 100%).
fn keyboard_light_tile(services: &Rc<Services>, layout: ControlLayout) -> gtk4::Button {
    let light = services.keyboard_light();
    // Hidden until UPower answers, and for good on machines without a backlight.
    let s = services.clone();
    let tile = tile(layout, "keyboard-brightness-symbolic", "Key light", move |_| {
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

fn tile(layout: ControlLayout, icon: &str, name: &str, on_toggle: impl Fn(bool) + 'static) -> Tile {
    let button = gtk4::Button::new();
    button.add_css_class("tile");
    let image = gtk4::Image::from_icon_name(icon);
    image.add_css_class("tile-icon");
    image.set_halign(gtk4::Align::Center);
    image.set_valign(gtk4::Align::Center);
    let title = gtk4::Label::new(Some(name));
    title.add_css_class("tile-title");
    title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    let subtitle = gtk4::Label::new(None);
    subtitle.add_css_class("tile-subtitle");
    subtitle.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    match layout {
        ControlLayout::Grid => {
            title.set_xalign(0.0);
            subtitle.set_xalign(0.0);
            subtitle.set_max_width_chars(18);
            let labels = gtk4::Box::new(gtk4::Orientation::Vertical, 1);
            labels.set_hexpand(true);
            labels.append(&title);
            labels.append(&subtitle);
            let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
            row.set_valign(gtk4::Align::Center);
            row.append(&image);
            row.append(&labels);
            button.set_child(Some(&row));
        }
        ControlLayout::Row => {
            title.set_max_width_chars(10);
            subtitle.set_max_width_chars(10);
            let column = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
            column.set_valign(gtk4::Align::Center);
            column.append(&image);
            column.append(&title);
            column.append(&subtitle);
            button.set_child(Some(&column));
        }
    }

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
    match state.wifi.as_ref().map(|wifi| wifi.strength) {
        Some(66..) => "network-wireless-signal-excellent-symbolic",
        Some(33..) => "network-wireless-signal-ok-symbolic",
        Some(_) => "network-wireless-signal-weak-symbolic",
        None => "network-wireless-offline-symbolic",
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum SessionAction {
    Lock,
    Sleep,
    Restart,
    ShutDown,
}

fn session_menu(services: &Rc<Services>) -> gtk4::Box {
    use SessionAction::{Lock, Restart, ShutDown, Sleep};

    let section = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    section.add_css_class("session-menu");
    section.append(&ui::separator());
    let (toggle, _) = ui::icon_row("system-shutdown-symbolic", "Session", "");
    toggle.add_css_class("session-row");
    ui::trailing_icon(&toggle, "pan-down-symbolic");
    toggle.update_state(&[gtk4::accessible::State::Expanded(Some(false))]);
    section.append(&toggle);

    let actions = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    actions.set_sensitive(false);
    let pending = Watch::new(None::<(SessionAction, Instant)>);
    for action in [Lock, Sleep, Restart, ShutDown] {
        let (icon, caption, confirmation) = match action {
            Lock => ("system-lock-screen-symbolic", "Lock", None),
            Sleep => ("weather-clear-night-symbolic", "Sleep", None),
            Restart => ("system-reboot-symbolic", "Restart", Some("Confirm restart")),
            ShutDown => ("system-shutdown-symbolic", "Shut down", Some("Confirm shut down")),
        };
        if action == Restart {
            actions.append(&ui::separator());
        }
        let (button, label) = ui::icon_row(icon, caption, "");
        button.add_css_class("session-row");
        if let Some(confirmation) = confirmation {
            pending.bind(&button, move |button, watched| {
                let armed = watched.is_some_and(|(armed, _)| armed == action);
                label.set_label(if armed { confirmation } else { caption });
                if armed {
                    button.add_css_class("armed");
                } else {
                    button.remove_css_class("armed");
                }
            });
        }
        let (s, pending) = (services.clone(), pending.clone());
        button.connect_clicked(move |_| {
            let confirmed = pending.get().is_some_and(|(armed, at)| armed == action && at.elapsed() < CONFIRM_TIME);
            if confirmation.is_some() && !confirmed {
                let attempt = Some((action, Instant::now()));
                pending.set(attempt);
                let pending = pending.clone();
                glib::timeout_add_local_once(CONFIRM_TIME, move || {
                    if *pending.get() == attempt {
                        pending.set(None);
                    }
                });
                return;
            }
            pending.set(None);
            menu::close();
            let session = s.session();
            match action {
                Lock => session.lock(),
                Sleep => session.suspend(),
                Restart => session.reboot(),
                ShutDown => session.power_off(),
            }
        });
        actions.append(&button);
    }

    let revealer = gtk4::Revealer::new();
    revealer.set_transition_type(gtk4::RevealerTransitionType::SlideDown);
    revealer.set_transition_duration(ui::ANIMATION_MS);
    revealer.set_child(Some(&actions));
    section.append(&revealer);
    toggle.update_relation(&[gtk4::accessible::Relation::Controls(&[revealer.upcast_ref()])]);
    toggle.connect_clicked(move |toggle| {
        let open = !revealer.reveals_child();
        actions.set_sensitive(open);
        revealer.set_reveal_child(open);
        toggle.update_state(&[gtk4::accessible::State::Expanded(Some(open))]);
        if open {
            toggle.add_css_class("expanded");
        } else {
            toggle.remove_css_class("expanded");
            pending.set(None);
        }
    });
    section
}

//! What plays (MPRIS): a play/pause button and the title, hidden when nothing plays.
//! A click on the title (or a right click) opens the player's menu: cover, track, a seekable position and
//! previous/next. The position is counted on by a one-second timer, only while the menu
//! is open and something plays.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::glib;
use gtk4::prelude::*;

use super::Context;
use crate::services::Services;
use crate::services::media::{self, Track};
use crate::{menu, ui};

/// Cover size in the menu, in pixels (the art is scaled to it once and cached).
const COVER: i32 = 72;

pub fn build(ctx: &Context) -> gtk4::Widget {
    let root = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    root.add_css_class("module");
    root.add_css_class("media");
    let play = gtk4::Button::from_icon_name("media-playback-start-symbolic");
    play.add_css_class("flat");
    play.add_css_class("media-play");
    let title = gtk4::Label::new(None);
    title.add_css_class("media-title");
    title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    title.set_single_line_mode(true);
    title.set_max_width_chars(24);
    root.append(&play);
    root.append(&title);
    // `media { title "off" }`: only the play button.
    title.set_visible(ctx.config.media_title);

    let services = ctx.services.clone();
    let s = services.clone();
    super::tooltips::text(&root, move || {
        s.media().current.get().as_ref().map(|track| {
            let title = display_title(track);
            if track.album.is_empty() { title } else { format!("{title}\n{}", track.album) }
        })
    });
    let s = services.clone();
    play.connect_clicked(move |_| s.media().play_pause());
    services.media().current.bind(&root, move |root, track| {
        root.set_visible(track.is_some());
        let Some(track) = track else { return };
        play.set_icon_name(play_icon(track));
        title.set_label(&display_title(track));
    });
    // The button takes its own clicks; the rest of the module opens the menu, and a
    // right click anywhere does (the only way when the title is hidden).
    menu::attach(&root, move |anchor| menu::show(anchor, &menu_content(&services)));
    menu::on_button(&root, gtk4::gdk::BUTTON_SECONDARY);
    root.upcast()
}

fn play_icon(track: &Track) -> &'static str {
    if track.playing { "media-playback-pause-symbolic" } else { "media-playback-start-symbolic" }
}

fn display_title(track: &Track) -> String {
    match (track.title.is_empty(), track.artist.is_empty()) {
        (true, _) => "Untitled".into(),
        (false, true) => track.title.clone(),
        (false, false) => format!("{} · {}", track.title, track.artist),
    }
}

fn menu_content(services: &Rc<Services>) -> gtk4::Box {
    let column = ui::column();
    column.add_css_class("media-menu");
    column.set_width_request(320);

    // Cover and what plays.
    let head = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    head.add_css_class("media-head");
    let cover = gtk4::Image::new();
    cover.add_css_class("media-cover");
    cover.set_pixel_size(COVER);
    let text = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    text.set_valign(gtk4::Align::Center);
    text.set_hexpand(true);
    let label = |class: &str| {
        let label = gtk4::Label::new(None);
        label.add_css_class(class);
        label.set_xalign(0.0);
        label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        label.set_max_width_chars(28);
        text.append(&label);
        label
    };
    let (name, artist, album) = (label("media-name"), label("media-artist"), label("media-album"));
    head.append(&cover);
    head.append(&text);
    column.append(&head);

    // Position: a slider that seeks, with the time gone and the length.
    let s = services.clone();
    let slider = ui::Slider::new(1.0, move |secs| s.media().set_position((secs * 1e6) as i64));
    slider.widget().add_css_class("media-position");
    let times = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    times.add_css_class("media-times");
    let gone = gtk4::Label::new(Some("0:00"));
    gone.set_hexpand(true);
    gone.set_xalign(0.0);
    let total = gtk4::Label::new(None);
    times.append(&gone);
    times.append(&total);
    let position_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    position_box.append(slider.widget());
    position_box.append(&times);
    column.append(&position_box);

    // Previous, play/pause, next.
    let controls = gtk4::Box::new(gtk4::Orientation::Horizontal, 16);
    controls.add_css_class("media-controls");
    controls.set_halign(gtk4::Align::Center);
    let button = |icon: &str, action: fn(&media::Media), s: &Rc<Services>| {
        let b = gtk4::Button::from_icon_name(icon);
        b.add_css_class("flat");
        b.add_css_class("media-control");
        let s = s.clone();
        b.connect_clicked(move |_| action(s.media()));
        controls.append(&b);
        b
    };
    let previous = button("media-skip-backward-symbolic", |m| m.previous(), services);
    let play = button("media-playback-start-symbolic", |m| m.play_pause(), services);
    let next = button("media-skip-forward-symbolic", |m| m.next(), services);
    column.append(&controls);

    let media = services.media();
    media.refresh_position();
    // Shows the position: on every position or track change, and each second while playing.
    let show_position = {
        let (services, slider, gone, total) = (services.clone(), slider.clone(), gone.downgrade(), total.downgrade());
        move || {
            let media = services.media();
            let Some(track) = media.current.get().clone() else { return };
            let length = track.length as f64 / 1e6;
            let now = media.position.get().map_or(0, |p| p.now(track.playing)).clamp(0, track.length.max(0));
            slider.widget().set_range(0.0, length.max(1.0));
            slider.show(now as f64 / 1e6);
            if let (Some(gone), Some(total)) = (gone.upgrade(), total.upgrade()) {
                gone.set_label(&clock_time(now));
                total.set_label(&if track.length > 0 { clock_time(track.length) } else { String::new() });
            }
        }
    };
    let show_position: Rc<dyn Fn()> = Rc::new(show_position);
    let sp = show_position.clone();
    media.position.bind(&position_box, move |_, _| sp());

    let ticking: Rc<RefCell<Option<glib::SourceId>>> = Rc::default();
    let last = RefCell::new(None::<(String, String, bool)>);
    let shown_art: Rc<RefCell<String>> = Rc::default();
    let (services2, sp, tick) = (services.clone(), show_position.clone(), ticking.clone());
    let position_weak = position_box.downgrade();
    media.current.bind(&column, move |_, track| {
        let Some(track) = track else {
            menu::close();
            return;
        };
        name.set_label(if track.title.is_empty() { "Untitled" } else { &track.title });
        artist.set_label(&track.artist);
        artist.set_visible(!track.artist.is_empty());
        album.set_label(&track.album);
        album.set_visible(!track.album.is_empty());
        // The cover may load a moment later: shown only if the track is still the same.
        let (weak, url) = (cover.downgrade(), track.art_url.clone());
        shown_art.replace(url.clone());
        let wanted = shown_art.clone();
        media::art(&track.art_url, COVER, move |texture| {
            let Some(cover) = weak.upgrade() else { return };
            if *wanted.borrow() != url {
                return;
            }
            match texture {
                Some(texture) => cover.set_paintable(Some(&texture)),
                None => cover.set_icon_name(Some("audio-x-generic-symbolic")),
            }
        });
        play.set_icon_name(play_icon(track));
        previous.set_sensitive(track.can_previous);
        next.set_sensitive(track.can_next);
        if let Some(position_box) = position_weak.upgrade() {
            position_box.set_visible(track.length > 0);
            position_box.set_sensitive(track.can_seek);
        }

        // Another track, or playing started or stopped: the position is read anew.
        let key = (track.track_id.clone(), track.title.clone(), track.playing);
        if last.replace(Some(key.clone())).is_some_and(|old| old != key) {
            services2.media().refresh_position();
        }
        sp();

        // Counting on only while playing; the timer ends with the menu.
        if track.playing && track.length > 0 {
            if tick.borrow().is_none() {
                let (sp, alive, t) = (sp.clone(), position_weak.clone(), tick.clone());
                let source = glib::timeout_add_seconds_local(1, move || {
                    if alive.upgrade().is_none() {
                        t.take();
                        return glib::ControlFlow::Break;
                    }
                    sp();
                    glib::ControlFlow::Continue
                });
                tick.replace(Some(source));
            }
        } else if let Some(source) = tick.take() {
            source.remove();
        }
    });
    column
}

/// 83 s → "1:23", an hour or more → "1:02:03".
fn clock_time(micros: i64) -> String {
    let secs = micros / 1_000_000;
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

#[cfg(test)]
mod tests {
    #[test]
    fn clock_times() {
        assert_eq!(super::clock_time(83_000_000), "1:23");
        assert_eq!(super::clock_time(3_723_000_000), "1:02:03");
        assert_eq!(super::clock_time(0), "0:00");
    }
}

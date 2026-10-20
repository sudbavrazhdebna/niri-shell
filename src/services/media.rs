//! The media player in use (MPRIS): what plays, play/pause/next/previous and seeking.
//! MPRIS doesn't announce the position as it moves: it is read when asked (the open
//! menu) and after a seek, and counted forward locally while playing.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use gtk4::prelude::*;
use gtk4::{gio, glib};

use super::dbus::{lookup, prop};
use crate::watch::Watch;

const PREFIX: &str = "org.mpris.MediaPlayer2.";
const PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
/// Mirrors other players; showing it would show everything twice.
const SKIP: &str = "org.mpris.MediaPlayer2.playerctld";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// Microseconds; 0 when the player doesn't say.
    pub length: i64,
    /// Cover art: a file:// (or other) URL, empty if none.
    pub art_url: String,
    /// MPRIS track id, needed to seek to a position.
    pub track_id: String,
    pub playing: bool,
    pub can_next: bool,
    pub can_previous: bool,
    pub can_seek: bool,
}

/// Where the track was at a moment; it moves on from there while playing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Position {
    /// Microseconds.
    pub at: i64,
    pub since: Instant,
}

impl Position {
    /// Microseconds now, counting on while `playing`.
    pub fn now(&self, playing: bool) -> i64 {
        let gone = if playing { i64::try_from(self.since.elapsed().as_micros()).unwrap_or(0) } else { 0 };
        self.at.saturating_add(gone)
    }
}

pub struct Media {
    /// The player to show: the one playing, else the one that changed last.
    pub current: Watch<Option<Track>>,
    /// The current player's position, when known (see [`Media::refresh_position`]).
    pub position: Watch<Option<Position>>,
    players: Rc<RefCell<Vec<gio::DBusProxy>>>,
}

impl Media {
    pub fn start() -> Media {
        let media = Media {
            current: Watch::new(None),
            position: Watch::new(None),
            players: Rc::default(),
        };
        let (current, players, position) = (media.current.clone(), media.players.clone(), media.position.clone());
        glib::spawn_future_local(async move {
            let Ok(bus) = gio::bus_get_future(gio::BusType::Session).await else { return };
            let (c, p, b, pos) = (current.clone(), players.clone(), bus.clone(), position.clone());
            let subscription = bus.subscribe_to_signal(
                Some("org.freedesktop.DBus"),
                Some("org.freedesktop.DBus"),
                Some("NameOwnerChanged"),
                Some("/org/freedesktop/DBus"),
                None,
                gio::DBusSignalFlags::NONE,
                move |signal| {
                    let Some((name, _, owner)) = signal.parameters.get::<(String, String, String)>()
                    else {
                        return;
                    };
                    if !name.starts_with(PREFIX) || name == SKIP {
                        return;
                    }
                    if owner.is_empty() {
                        p.borrow_mut().retain(|proxy| proxy.name().as_deref() != Some(name.as_str()));
                        pick(&p, &c, None);
                    } else {
                        glib::spawn_future_local(add(b.clone(), name, p.clone(), c.clone(), pos.clone()));
                    }
                },
            );
            std::mem::forget(subscription);

            let names = bus
                .call_future(
                    Some("org.freedesktop.DBus"),
                    "/org/freedesktop/DBus",
                    "org.freedesktop.DBus",
                    "ListNames",
                    None,
                    None,
                    gio::DBusCallFlags::NONE,
                    -1,
                )
                .await
                .ok()
                .and_then(|v| v.get::<(Vec<String>,)>())
                .map(|(names,)| names)
                .unwrap_or_default();
            for name in names.into_iter().filter(|n| n.starts_with(PREFIX) && n != SKIP) {
                add(bus.clone(), name, players.clone(), current.clone(), position.clone()).await;
            }
        });
        media
    }

    pub fn play_pause(&self) {
        self.call("PlayPause");
    }

    pub fn next(&self) {
        self.call("Next");
    }

    pub fn previous(&self) {
        self.call("Previous");
    }

    /// Reads the current player's position once (MPRIS sends no updates of it).
    pub fn refresh_position(&self) {
        let Some(proxy) = self.players.borrow().first().cloned() else { return };
        let position = self.position.clone();
        glib::spawn_future_local(async move {
            let reply = proxy
                .connection()
                .call_future(
                    proxy.name().as_deref(),
                    PATH,
                    "org.freedesktop.DBus.Properties",
                    "Get",
                    Some(&(PLAYER, "Position").to_variant()),
                    None,
                    gio::DBusCallFlags::NONE,
                    -1,
                )
                .await;
            let at = reply
                .ok()
                .and_then(|r| super::dbus::child(&r, 0))
                .and_then(|v| v.as_variant())
                .and_then(|v| v.get::<i64>());
            position.set(at.map(|at| Position { at, since: Instant::now() }));
        });
    }

    /// Jumps to `at` microseconds into the current track.
    pub fn set_position(&self, at: i64) {
        let Some(proxy) = self.players.borrow().first().cloned() else { return };
        let Some(track) = self.current.get().clone() else { return };
        // SetPosition needs the track's id; without a valid one, seek by the difference.
        let args = match glib::variant::ObjectPath::try_from(track.track_id.as_str()) {
            Ok(id) => ("SetPosition", (id, at).to_variant()),
            Err(_) => {
                let now = self.position.get().map_or(0, |p| p.now(track.playing));
                ("Seek", (at - now,).to_variant())
            }
        };
        self.position.set(Some(Position { at, since: Instant::now() }));
        glib::spawn_future_local(async move {
            if let Err(err) = proxy.call_future(args.0, Some(&args.1), gio::DBusCallFlags::NONE, -1).await {
                tracing::warn!("media {}: {err}", args.0);
            }
        });
    }

    fn call(&self, method: &'static str) {
        let Some(proxy) = self.players.borrow().first().cloned() else { return };
        glib::spawn_future_local(async move {
            if let Err(err) = proxy.call_future(method, None, gio::DBusCallFlags::NONE, -1).await {
                tracing::warn!("media {method}: {err}");
            }
        });
    }
}

async fn add(
    bus: gio::DBusConnection,
    name: String,
    players: Rc<RefCell<Vec<gio::DBusProxy>>>,
    current: Watch<Option<Track>>,
    position: Watch<Option<Position>>,
) {
    let proxy =
        gio::DBusProxy::new_future(&bus, gio::DBusProxyFlags::DO_NOT_AUTO_START, None, Some(&name), PATH, PLAYER)
            .await;
    let Ok(proxy) = proxy else { return };
    // A player that quit while the proxy was made: its NameOwnerChanged came before it
    // was in the list, and an empty "Untitled" player stayed for good.
    if proxy.name_owner().is_none() {
        return;
    }
    let (p, c) = (players.clone(), current.clone());
    proxy.connect_local("g-properties-changed", false, move |args| {
        let changed = args.first().and_then(|v| v.get::<gio::DBusProxy>().ok());
        pick(&p, &c, changed);
        None
    });
    // A seek, by the user or the app, tells the new position.
    let p = players.clone();
    proxy.connect_local("g-signal", false, move |args| {
        let proxy = args.first().and_then(|v| v.get::<gio::DBusProxy>().ok())?;
        let signal = args.get(2).and_then(|v| v.get::<String>().ok())?;
        let params = args.get(3).and_then(|v| v.get::<glib::Variant>().ok())?;
        let is_front = p.borrow().first() == Some(&proxy);
        if signal == "Seeked"
            && is_front
            && let Some((at,)) = params.get::<(i64,)>()
        {
            position.set(Some(Position { at, since: Instant::now() }));
        }
        None
    });
    players.borrow_mut().retain(|p| p.name().as_deref() != Some(name.as_str()));
    players.borrow_mut().push(proxy.clone());
    pick(&players, &current, Some(proxy));
}

/// Moves the player to show to the front: a playing one wins, then the one that just
/// changed.
fn pick(players: &RefCell<Vec<gio::DBusProxy>>, current: &Watch<Option<Track>>, changed: Option<gio::DBusProxy>) {
    let track = reorder(&mut players.borrow_mut(), changed);
    // Not while the list is borrowed: listeners may call the players.
    current.set(track);
}

fn reorder(players: &mut Vec<gio::DBusProxy>, changed: Option<gio::DBusProxy>) -> Option<Track> {
    let playing = |p: &gio::DBusProxy| prop::<String>(p, "PlaybackStatus").as_deref() == Some("Playing");
    let front_plays = players.first().is_some_and(playing);
    if let Some(changed) = changed
        && (playing(&changed) || !front_plays)
        && let Some(i) = players.iter().position(|p| *p == changed)
    {
        let p = players.remove(i);
        players.insert(0, p);
    } else if let Some(i) = players.iter().position(playing) {
        let p = players.remove(i);
        players.insert(0, p);
    }
    players.first().map(read)
}

thread_local! {
    /// Cover art scaled to its shown size, by URL: players repeat the same few.
    static ART: RefCell<HashMap<String, Option<gtk4::gdk::Texture>>> = RefCell::default();
}

/// Cover art from a file:// URL, scaled to `size` pixels and cached; the original image
/// (often a large JPEG) is not kept. Other URL kinds would need a download: none.
pub fn art(url: &str, size: i32) -> Option<gtk4::gdk::Texture> {
    if url.is_empty() {
        return None;
    }
    if let Some(cached) = ART.with_borrow(|art| art.get(url).cloned()) {
        return cached;
    }
    let texture = glib::filename_from_uri(url).ok().and_then(|(path, _)| {
        let pixbuf = gtk4::gdk_pixbuf::Pixbuf::from_file_at_scale(&path, size, size, true).ok()?;
        let format = if pixbuf.has_alpha() { gtk4::gdk::MemoryFormat::R8g8b8a8 } else { gtk4::gdk::MemoryFormat::R8g8b8 };
        let stride = usize::try_from(pixbuf.rowstride()).ok()?;
        Some(gtk4::gdk::MemoryTexture::new(pixbuf.width(), pixbuf.height(), format, &pixbuf.read_pixel_bytes(), stride).upcast())
    });
    ART.with_borrow_mut(|art| {
        // A handful of covers is all anyone flips between.
        if art.len() >= 16 {
            art.clear();
        }
        art.insert(url.to_string(), texture.clone());
    });
    texture
}

fn read(proxy: &gio::DBusProxy) -> Track {
    let metadata = proxy.cached_property("Metadata");
    let field = |key: &str| metadata.as_ref().and_then(|m| lookup(m, key));
    let text = |key: &str| field(key).and_then(|v| v.get::<String>()).unwrap_or_default();
    Track {
        title: text("xesam:title"),
        artist: field("xesam:artist")
            .and_then(|v| v.get::<Vec<String>>())
            .map(|a| a.join(", "))
            .unwrap_or_default(),
        album: text("xesam:album"),
        // Players send it as x, some as t or i.
        length: field("mpris:length")
            .and_then(|v| v.get::<i64>().or_else(|| v.get::<u64>().and_then(|n| i64::try_from(n).ok())).or_else(|| v.get::<i32>().map(i64::from)))
            .unwrap_or(0)
            .max(0),
        art_url: text("mpris:artUrl"),
        track_id: field("mpris:trackid")
            .and_then(|v| v.get::<glib::variant::ObjectPath>().map(|p| p.as_str().to_string()).or_else(|| v.get::<String>()))
            .unwrap_or_default(),
        playing: prop::<String>(proxy, "PlaybackStatus").as_deref() == Some("Playing"),
        can_next: prop(proxy, "CanGoNext").unwrap_or(false),
        can_previous: prop(proxy, "CanGoPrevious").unwrap_or(false),
        can_seek: prop(proxy, "CanSeek").unwrap_or(false),
    }
}

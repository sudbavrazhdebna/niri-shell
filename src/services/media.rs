//! The media player in use (MPRIS): what plays, and play/pause/next/previous.

use std::cell::RefCell;
use std::rc::Rc;

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
    pub playing: bool,
    pub can_next: bool,
    pub can_previous: bool,
}

pub struct Media {
    /// The player to show: the one playing, else the one that changed last.
    pub current: Watch<Option<Track>>,
    players: Rc<RefCell<Vec<gio::DBusProxy>>>,
}

impl Media {
    pub fn start() -> Media {
        let media = Media {
            current: Watch::new(None),
            players: Rc::default(),
        };
        let (current, players) = (media.current.clone(), media.players.clone());
        glib::spawn_future_local(async move {
            let Ok(bus) = gio::bus_get_future(gio::BusType::Session).await else { return };
            let (c, p, b) = (current.clone(), players.clone(), bus.clone());
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
                        glib::spawn_future_local(add(b.clone(), name, p.clone(), c.clone()));
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
                add(bus.clone(), name, players.clone(), current.clone()).await;
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

fn read(proxy: &gio::DBusProxy) -> Track {
    let metadata = proxy.cached_property("Metadata");
    let field = |key: &str| metadata.as_ref().and_then(|m| lookup(m, key));
    Track {
        title: field("xesam:title").and_then(|v| v.get::<String>()).unwrap_or_default(),
        artist: field("xesam:artist")
            .and_then(|v| v.get::<Vec<String>>())
            .map(|a| a.join(", "))
            .unwrap_or_default(),
        playing: prop::<String>(proxy, "PlaybackStatus").as_deref() == Some("Playing"),
        can_next: prop(proxy, "CanGoNext").unwrap_or(false),
        can_previous: prop(proxy, "CanGoPrevious").unwrap_or(false),
    }
}

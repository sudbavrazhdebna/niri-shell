//! Network status from NetworkManager: the Wi-Fi or wired connection in use and the
//! Wi-Fi signal strength.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gio, glib};

use super::dbus::{on_properties_changed, prop, system_proxy};
use crate::watch::Watch;

const NM: &str = "org.freedesktop.NetworkManager";
const WIFI: &str = "802-11-wireless";
const WIRED: &str = "802-3-ethernet";
/// NM_ACTIVE_CONNECTION_STATE_ACTIVATED.
const ACTIVATED: u32 = 2;

#[derive(Clone, Default, PartialEq, Debug)]
pub enum Link {
    #[default]
    Offline,
    Wifi {
        ssid: String,
        /// The step of the signal (see [`step`]), not the exact percent.
        strength: u8,
    },
    Wired,
}

#[derive(Clone, Default, PartialEq, Debug)]
pub struct NetworkState {
    pub link: Link,
    pub wifi_enabled: bool,
}

pub struct Network {
    pub state: Watch<NetworkState>,
    inner: Rc<Inner>,
}

#[derive(Default)]
pub struct WifiDetails {
    pub strength: Option<u8>,
    pub frequency: Option<u32>,
}

#[derive(Default)]
struct Inner {
    nm: RefCell<Option<gio::DBusProxy>>,
    /// The access point being followed for signal strength.
    access_point: RefCell<Option<gio::DBusProxy>>,
    /// Bumped on every rescan, so a slow older one can't overwrite a newer result.
    generation: Cell<u64>,
}

impl Network {
    /// Exact tooltip values, without publishing every signal-percent change to the bar.
    pub fn wifi_details(&self) -> WifiDetails {
        let ap = self.inner.access_point.borrow();
        WifiDetails {
            strength: ap.as_ref().and_then(|ap| prop(ap, "Strength")),
            frequency: ap.as_ref().and_then(|ap| prop(ap, "Frequency")),
        }
    }

    pub fn start() -> Network {
        let network = Network {
            state: Watch::new(NetworkState::default()),
            inner: Rc::new(Inner::default()),
        };
        let (state, inner) = (network.state.clone(), network.inner.clone());
        glib::spawn_future_local(async move {
            let Some(nm) = system_proxy(NM, "/org/freedesktop/NetworkManager", NM).await else {
                return;
            };
            let (s, i) = (state.clone(), Rc::downgrade(&inner));
            // VPN tunnels, new devices and (dis)connects all show up here; rescanning is a
            // handful of property reads.
            on_properties_changed(&nm, move |_| {
                if let Some(inner) = i.upgrade() {
                    glib::spawn_future_local(rescan(inner, s.clone()));
                }
            });
            inner.nm.replace(Some(nm));
            rescan(inner, state).await;
        });
        network
    }
}

impl Network {
    pub fn set_wifi_enabled(&self, on: bool) {
        let Some(nm) = self.inner.nm.borrow().clone() else { return };
        glib::spawn_future_local(async move {
            let result = nm
                .connection()
                .call_future(
                    Some(NM),
                    "/org/freedesktop/NetworkManager",
                    "org.freedesktop.DBus.Properties",
                    "Set",
                    Some(&(NM, "WirelessEnabled", on.to_variant()).to_variant()),
                    None,
                    gio::DBusCallFlags::NONE,
                    -1,
                )
                .await;
            if let Err(err) = result {
                tracing::warn!("Wi-Fi on/off: {err}");
            }
        });
    }
}

/// Finds the activated Wi-Fi or wired connection (not the VPN on top of it) and
/// follows its access point.
async fn rescan(inner: Rc<Inner>, state: Watch<NetworkState>) {
    let generation = inner.generation.get() + 1;
    inner.generation.set(generation);

    let Some(nm) = inner.nm.borrow().clone() else {
        return;
    };
    let wifi_enabled = prop(&nm, "WirelessEnabled").unwrap_or(false);
    let primary: Option<String> = prop::<glib::variant::ObjectPath>(&nm, "PrimaryConnection")
        .map(|p| p.as_str().to_string());
    let mut paths: Vec<String> = prop::<Vec<glib::variant::ObjectPath>>(&nm, "ActiveConnections")
        .unwrap_or_default()
        .into_iter()
        .map(|p| p.as_str().to_string())
        .collect();
    // Prefer the primary connection when it is a physical one.
    paths.sort_by_key(|p| Some(p) != primary.as_ref());

    let mut found: Option<(Link, Option<String>)> = None;
    for path in paths {
        let Some(active) = system_proxy(NM, &path, "org.freedesktop.NetworkManager.Connection.Active").await
        else {
            continue;
        };
        if prop::<u32>(&active, "State") != Some(ACTIVATED) {
            continue;
        }
        match prop::<String>(&active, "Type").as_deref() {
            Some(WIFI) => {
                let ap = prop::<glib::variant::ObjectPath>(&active, "SpecificObject")
                    .map(|p| p.as_str().to_string())
                    .filter(|p| p != "/");
                found = Some((Link::Wifi { ssid: String::new(), strength: 0 }, ap));
                break;
            }
            Some(WIRED) if found.is_none() => found = Some((Link::Wired, None)),
            _ => {}
        }
    }

    let ap = match found.as_ref().and_then(|(_, ap)| ap.clone()) {
        Some(path) => system_proxy(NM, &path, "org.freedesktop.NetworkManager.AccessPoint").await,
        None => None,
    };
    if inner.generation.get() != generation {
        return;
    }

    let link = found.map_or(Link::Offline, |(link, _)| link);
    if let Some(ap) = &ap {
        let s = state.clone();
        on_properties_changed(ap, move |ap| {
            s.update(|state| match &mut state.link {
                Link::Wifi { strength, .. } => {
                    let new = step(prop(ap, "Strength").unwrap_or(0));
                    std::mem::replace(strength, new) != new
                }
                _ => false,
            });
        });
    }
    let link = match (link, &ap) {
        (Link::Wifi { .. }, Some(ap)) => Link::Wifi {
            ssid: prop::<Vec<u8>>(ap, "Ssid")
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .unwrap_or_default(),
            strength: step(prop(ap, "Strength").unwrap_or(0)),
        },
        (link, _) => link,
    };
    inner.access_point.replace(ap);
    state.set(NetworkState { link, wifi_enabled });
}

/// The signal strength by the steps the icon shows (66+, 33+, 10+, below): NetworkManager
/// sends the exact percent every few seconds, and only a new step is worth waking anyone.
fn step(strength: u8) -> u8 {
    match strength {
        66.. => 66,
        33.. => 33,
        10.. => 10,
        _ => 0,
    }
}

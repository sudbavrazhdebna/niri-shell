//! Network status from NetworkManager: the Wi-Fi or wired connection in use and the
//! Wi-Fi signal strength.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gio, glib};

use super::dbus::{child, lookup, on_properties_changed, prop, system_proxy};
use crate::watch::Watch;

const NM: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";
const WIFI: &str = "802-11-wireless";
const WIRED: &str = "802-3-ethernet";
/// NM_RADIO_FLAG_WLAN_AVAILABLE.
const WLAN_AVAILABLE: u32 = 1;
/// NM_DEVICE_TYPE_WIFI.
const DEVICE_WIFI: u32 = 2;
/// NM_ACTIVE_CONNECTION_STATE_ACTIVATED.
const ACTIVATED: u32 = 2;

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Link {
    #[default]
    Offline,
    Wifi,
    Wired,
}

#[derive(Clone, Default, PartialEq, Debug)]
pub struct WifiLink {
    pub ssid: String,
    /// The step of the signal (see [`step`]), not the exact percent.
    pub strength: u8,
}

#[derive(Clone, PartialEq, Debug)]
pub struct WiredLink {
    pub interface: Option<String>,
    pub ip: Option<String>,
}

#[derive(Clone, Default, PartialEq, Debug)]
pub struct NetworkState {
    pub link: Link,
    pub has_wifi_adapter: bool,
    pub wifi_enabled: bool,
    pub wifi: Option<WifiLink>,
    pub wired: Option<WiredLink>,
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
    /// Connection and IP configuration proxies followed for property changes.
    observed: RefCell<Vec<gio::DBusProxy>>,
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
            let Some(nm) = system_proxy(NM, NM_PATH, NM).await else {
                return;
            };
            // VPN tunnels, new devices and (dis)connects all show up here; rescanning is a
            // handful of property reads.
            watch_connection(&nm, &inner, &state);
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
                    NM_PATH,
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

/// Tracks the active Wi-Fi and wired links and resolves the primary link for the bar.
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

    let mut wifi = None;
    let mut wired = None;
    let mut observed = Vec::new();
    for path in paths {
        let Some(active) = system_proxy(NM, &path, "org.freedesktop.NetworkManager.Connection.Active").await
        else {
            continue;
        };
        let kind = prop::<String>(&active, "Type");
        if !matches!(kind.as_deref(), Some(WIFI | WIRED)) {
            continue;
        }
        watch_connection(&active, &inner, &state);
        if prop::<u32>(&active, "State") == Some(ACTIVATED) {
            match kind.as_deref() {
                Some(WIFI) if wifi.is_none() => wifi = Some(active.clone()),
                Some(WIRED) if wired.is_none() => wired = Some(active.clone()),
                _ => {}
            }
        }
        observed.push(active);
    }

    let link = select_link(
        primary.as_deref(),
        wifi.as_ref().map(|proxy| proxy.object_path()).as_deref(),
        wired.as_ref().map(|proxy| proxy.object_path()).as_deref(),
    );
    let ap = match wifi.as_ref().and_then(|active| object_path(active, "SpecificObject")) {
        Some(path) => system_proxy(NM, path.as_str(), "org.freedesktop.NetworkManager.AccessPoint").await,
        None => None,
    };
    let wired = match wired.as_ref() {
        Some(active) => Some(wired_details(active, &inner, &state, &mut observed).await),
        None => None,
    };
    let has_wifi_adapter = match prop::<u32>(&nm, "RadioFlags") {
        Some(flags) => flags & WLAN_AVAILABLE != 0,
        None => wifi.is_some() || wifi_device(&nm).await.is_some(),
    };
    if inner.generation.get() != generation {
        return;
    }

    if let Some(ap) = &ap {
        let s = state.clone();
        on_properties_changed(ap, move |ap| {
            s.update(|state| state.wifi.as_mut().is_some_and(|wifi| {
                let strength = step(prop(ap, "Strength").unwrap_or(0));
                std::mem::replace(&mut wifi.strength, strength) != strength
            }));
        });
    }
    let wifi = wifi.map(|_| ap.as_ref().map_or_else(WifiLink::default, wifi_status));
    inner.access_point.replace(ap);
    inner.observed.replace(observed);
    state.set(NetworkState { link, has_wifi_adapter, wifi_enabled, wifi, wired });
}

pub(super) async fn wifi_device(nm: &gio::DBusProxy) -> Option<gio::DBusProxy> {
    let devices = nm.call_future("GetDevices", None, gio::DBusCallFlags::NONE, -1).await
        .inspect_err(|err| tracing::debug!("NetworkManager GetDevices: {err}"))
        .ok().and_then(|result| result.get::<(Vec<glib::variant::ObjectPath>,)>())?.0;
    for path in devices {
        if let Some(device) = system_proxy(NM, path.as_str(), "org.freedesktop.NetworkManager.Device").await
            && prop::<u32>(&device, "DeviceType") == Some(DEVICE_WIFI)
        {
            return Some(device);
        }
    }
    None
}

// Fall back to Wi-Fi, then Ethernet, when NetworkManager has no physical primary.
fn select_link(primary: Option<&str>, wifi: Option<&str>, wired: Option<&str>) -> Link {
    if wired.is_some_and(|path| Some(path) == primary) {
        Link::Wired
    } else if wifi.is_some() {
        Link::Wifi
    } else if wired.is_some() {
        Link::Wired
    } else {
        Link::Offline
    }
}

fn watch_connection(proxy: &gio::DBusProxy, inner: &Rc<Inner>, state: &Watch<NetworkState>) {
    let (inner, state) = (Rc::downgrade(inner), state.clone());
    on_properties_changed(proxy, move |_| {
        if let Some(inner) = inner.upgrade() {
            glib::spawn_future_local(rescan(inner, state.clone()));
        }
    });
}

fn object_path(proxy: &gio::DBusProxy, property: &str) -> Option<glib::variant::ObjectPath> {
    prop::<glib::variant::ObjectPath>(proxy, property).filter(|path| path.as_str() != "/")
}

async fn wired_details(
    active: &gio::DBusProxy,
    inner: &Rc<Inner>,
    state: &Watch<NetworkState>,
    observed: &mut Vec<gio::DBusProxy>,
) -> WiredLink {
    let devices: Vec<glib::variant::ObjectPath> = prop(active, "Devices").unwrap_or_default();
    let interface = match devices.first() {
        Some(path) => system_proxy(NM, path.as_str(), "org.freedesktop.NetworkManager.Device").await
            .and_then(|device| prop(&device, "Interface")),
        None => None,
    };
    let mut ip = None;
    for (property, iface) in [
        ("Ip4Config", "org.freedesktop.NetworkManager.IP4Config"),
        ("Ip6Config", "org.freedesktop.NetworkManager.IP6Config"),
    ] {
        if let Some(path) = object_path(active, property)
            && let Some(config) = system_proxy(NM, path.as_str(), iface).await
        {
            watch_connection(&config, inner, state);
            ip = first_address(&config);
            observed.push(config);
            if ip.is_some() {
                break;
            }
        }
    }
    WiredLink { interface, ip }
}

pub(super) fn first_address(config: &gio::DBusProxy) -> Option<String> {
    let addresses = config.cached_property("AddressData")?;
    let first = child(&addresses, 0)?;
    lookup(&first, "address").and_then(|value| value.get::<String>())
}

fn wifi_status(ap: &gio::DBusProxy) -> WifiLink {
    WifiLink {
        ssid: prop::<Vec<u8>>(ap, "Ssid")
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default(),
        strength: step(prop(ap, "Strength").unwrap_or(0)),
    }
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

#[cfg(test)]
mod tests {
    use super::{Link, select_link};

    #[test]
    fn physical_primary_wins_when_both_connections_are_active() {
        let wifi = Some("/wifi");
        let ethernet = Some("/ethernet");
        assert_eq!(select_link(ethernet, wifi, ethernet), Link::Wired);
        assert_eq!(select_link(wifi, wifi, ethernet), Link::Wifi);
        assert_eq!(select_link(Some("/vpn"), wifi, ethernet), Link::Wifi);
        assert_eq!(select_link(None, None, ethernet), Link::Wired);
        assert_eq!(select_link(None, None, None), Link::Offline);
    }
}

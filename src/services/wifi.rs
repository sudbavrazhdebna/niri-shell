//! Wi-Fi actions for the network menu: scanning, the list of networks, connecting.
//! Runs only while the menu is open; the bar icon itself comes from `network`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gio, glib};

use super::dbus::{child, lookup, on_properties_changed, prop, system_proxy};
use crate::watch::Watch;

const NM: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";
const WIRELESS: &str = "org.freedesktop.NetworkManager.Device.Wireless";
/// NM_DEVICE_TYPE_WIFI.
const DEVICE_WIFI: u32 = 2;

#[derive(Clone, Debug, PartialEq)]
pub struct Network {
    pub ssid: String,
    /// 0..=100
    pub strength: u8,
    pub secured: bool,
    /// A saved profile exists, so connecting needs no password.
    pub known: bool,
    pub active: bool,
    access_point: Option<String>,
}

impl Network {
    pub fn available(&self) -> bool {
        self.access_point.is_some()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WifiState {
    /// Kernel name of the Wi-Fi interface (e.g. "wlp1s0"), for traffic counters.
    pub interface: Option<String>,
    /// IPv4 address of the connection, if connected.
    pub ip: Option<String>,
    pub enabled: bool,
    /// Active first, then available by strength, then unavailable saved profiles.
    /// One entry per network name.
    pub networks: Vec<Network>,
    /// What went wrong with the last connection attempt, for the menu to show.
    pub error: Option<String>,
}

/// Lives while the network menu is open.
pub struct Wifi {
    pub state: Watch<WifiState>,
    nm: gio::DBusProxy,
    device: Option<gio::DBusProxy>,
    /// The same device's generic interface: its name and IP configuration.
    device_info: Option<gio::DBusProxy>,
    /// Saved profiles (network name → path), read once per menu.
    profiles: RefCell<HashMap<String, String>>,
    /// Access point proxies by path: scans repeat mostly the same ones.
    access_points: RefCell<HashMap<String, gio::DBusProxy>>,
    /// A refresh is running; `again` asks it for one more round when done.
    refreshing: Cell<bool>,
    again: Cell<bool>,
}

impl Wifi {
    /// Finds the Wi-Fi device, asks it to scan, and keeps the list up to date.
    pub async fn open() -> Option<Rc<Wifi>> {
        let started = std::time::Instant::now();
        // The saved profiles are read alongside the device lookup, not after it.
        let profiles = glib::spawn_future_local(saved_profiles());
        let nm = system_proxy(NM, NM_PATH, NM).await?;
        let mut device = None;
        let mut device_info = None;
        let devices = call(&nm, "GetDevices", None).await.and_then(|r| child(&r, 0));
        for path in devices.iter().flat_map(|d| d.iter()) {
            let Some(path) = path.get::<glib::variant::ObjectPath>() else { continue };
            let Some(dev) = system_proxy(NM, path.as_str(), "org.freedesktop.NetworkManager.Device").await
            else {
                continue;
            };
            if prop::<u32>(&dev, "DeviceType") == Some(DEVICE_WIFI) {
                device = system_proxy(NM, path.as_str(), WIRELESS).await;
                device_info = Some(dev);
                break;
            }
        }

        let wifi = Rc::new(Wifi {
            state: Watch::new(WifiState::default()),
            nm: nm.clone(),
            device,
            device_info,
            profiles: RefCell::new(profiles.await.unwrap_or_default()),
            access_points: RefCell::default(),
            refreshing: Cell::new(false),
            again: Cell::new(false),
        });
        let weak = Rc::downgrade(&wifi);
        on_properties_changed(&nm, move |_| {
            if let Some(wifi) = weak.upgrade() {
                glib::spawn_future_local(async move { wifi.refresh().await });
            }
        });
        if let Some(device) = &wifi.device_info {
            let weak = Rc::downgrade(&wifi);
            on_properties_changed(device, move |_| {
                if let Some(wifi) = weak.upgrade() {
                    glib::spawn_future_local(async move { wifi.refresh().await });
                }
            });
        }
        if let Some(device) = &wifi.device {
            let weak = Rc::downgrade(&wifi);
            // New scan results arrive as AccessPoints/LastScan changes.
            on_properties_changed(device, move |_| {
                if let Some(wifi) = weak.upgrade() {
                    glib::spawn_future_local(async move { wifi.refresh().await });
                }
            });
            // Not waited for (a third of the menu's opening time): the networks NetworkManager
            // knows already are shown, and the scan's results come in as changes.
            let device = device.clone();
            glib::spawn_future_local(async move {
                let options = glib::VariantDict::new(None).end();
                // tuple_from_iter: a Variant inside a Rust tuple would be boxed as "v".
                let _ = call(&device, "RequestScan", Some(glib::Variant::tuple_from_iter([options]))).await;
            });
        }
        wifi.refresh().await;
        tracing::debug!(took = ?started.elapsed(), found = wifi.state.get().networks.len(), "wifi menu ready");
        Some(wifi)
    }

    pub fn set_enabled(&self, on: bool) {
        let nm = self.nm.clone();
        glib::spawn_future_local(async move {
            let args = (NM, "WirelessEnabled", on.to_variant()).to_variant();
            let _ = properties_set(&nm, args).await;
        });
    }

    /// Disconnect this adapter without turning off Wi-Fi or deleting its profiles.
    /// Device.Disconnect also blocks automatic reconnection until a manual activation.
    pub fn disconnect(self: &Rc<Self>) {
        let Some(device) = self.device_info.clone() else { return };
        let this = self.clone();
        glib::spawn_future_local(async move {
            let error = call(&device, "Disconnect", None).await
                .is_none().then(|| "Couldn't disconnect Wi-Fi".to_string());
            this.state.update(|s| std::mem::replace(&mut s.error, error.clone()) != error);
            this.refresh().await;
        });
    }

    /// Connects to a network: with its saved profile, or a new one with `password`.
    pub fn connect(self: &Rc<Self>, network: &Network, password: Option<&str>) {
        let Some(access_point) = network.access_point.clone() else { return };
        let (Some(device), this) = (self.device.clone(), self.clone()) else { return };
        let (network, password) = (network.clone(), password.map(str::to_string));
        glib::spawn_future_local(async move {
            let device_path = glib::variant::ObjectPath::try_from(device.object_path().to_string());
            let ap_path = glib::variant::ObjectPath::try_from(access_point);
            let (Ok(device_path), Ok(ap_path)) = (device_path, ap_path) else { return };
            let saved = this.profiles.borrow().get(&network.ssid).cloned();
            let result = match saved {
                Some(profile) => {
                    let profile = glib::variant::ObjectPath::try_from(profile).ok();
                    let Some(profile) = profile else { return };
                    call(&this.nm, "ActivateConnection", Some((profile, device_path, ap_path).to_variant())).await
                }
                None => {
                    let args = glib::Variant::tuple_from_iter([
                        new_profile(&network.ssid, password.as_deref()),
                        device_path.to_variant(),
                        ap_path.to_variant(),
                    ]);
                    call(&this.nm, "AddAndActivateConnection", Some(args)).await
                }
            };
            if result.is_some() {
                // A new profile was just saved.
                this.profiles.replace(saved_profiles().await);
            }
            let error = result.is_none().then(|| format!("Couldn't connect to {}", network.ssid));
            this.state.update(|s| std::mem::replace(&mut s.error, error.clone()) != error);
            this.refresh().await;
        });
    }

    /// The device's first IPv4 address, from its current IP configuration.
    async fn ip(&self) -> Option<String> {
        let config = self
            .device_info
            .as_ref()
            .and_then(|d| prop::<glib::variant::ObjectPath>(d, "Ip4Config"))
            .filter(|p| p.as_str() != "/")?;
        let proxy = system_proxy(NM, config.as_str(), "org.freedesktop.NetworkManager.IP4Config").await?;
        let addresses = proxy.cached_property("AddressData")?;
        let first = child(&addresses, 0)?;
        lookup(&first, "address").and_then(|v| v.get::<String>())
    }

    /// Re-reads the list. Change signals come in bursts during a scan; overlapping
    /// calls fold into one more round instead of running side by side.
    async fn refresh(&self) {
        if self.refreshing.replace(true) {
            self.again.set(true);
            return;
        }
        loop {
            self.again.set(false);
            self.read_networks().await;
            if !self.again.get() {
                break;
            }
        }
        self.refreshing.set(false);
    }

    async fn read_networks(&self) {
        let interface = self.device_info.as_ref().and_then(|d| prop::<String>(d, "Interface"));
        let ip = self.ip().await;
        let enabled = prop(&self.nm, "WirelessEnabled").unwrap_or(false);
        let mut networks: HashMap<String, Network> = HashMap::new();
        if let Some(device) = &self.device {
            let active_ap = prop::<glib::variant::ObjectPath>(device, "ActiveAccessPoint")
                .map(|p| p.as_str().to_string());
            let points = prop::<Vec<glib::variant::ObjectPath>>(device, "AccessPoints").unwrap_or_default();
            self.access_points
                .borrow_mut()
                .retain(|path, _| points.iter().any(|p| p.as_str() == path));
            for path in points {
                let cached = self.access_points.borrow().get(path.as_str()).cloned();
                let ap = match cached {
                    Some(ap) => ap,
                    None => {
                        let Some(ap) =
                            system_proxy(NM, path.as_str(), "org.freedesktop.NetworkManager.AccessPoint").await
                        else {
                            continue;
                        };
                        self.access_points.borrow_mut().insert(path.as_str().to_string(), ap.clone());
                        ap
                    }
                };
                let ssid = prop::<Vec<u8>>(&ap, "Ssid")
                    .map(|b| String::from_utf8_lossy(&b).into_owned())
                    .unwrap_or_default();
                if ssid.is_empty() {
                    continue; // hidden network
                }
                let strength = prop(&ap, "Strength").unwrap_or(0);
                let secured = prop::<u32>(&ap, "WpaFlags").unwrap_or(0) != 0
                    || prop::<u32>(&ap, "RsnFlags").unwrap_or(0) != 0
                    || prop::<u32>(&ap, "Flags").unwrap_or(0) & 1 != 0;
                let network = Network {
                    known: self.profiles.borrow().contains_key(&ssid),
                    active: active_ap.as_deref() == Some(path.as_str()),
                    ssid: ssid.clone(),
                    strength,
                    secured,
                    access_point: Some(path.as_str().to_string()),
                };
                // Several access points of one network: keep the strongest, or the active one.
                let keep = networks
                    .get(&ssid)
                    .is_none_or(|old| !old.active && (network.active || network.strength > old.strength));
                if keep {
                    networks.insert(ssid, network);
                }
            }
        }
        let networks = with_saved(networks, &self.profiles.borrow());
        self.state.update(|s| {
            let new = WifiState { enabled, networks, error: s.error.clone(), interface: interface.clone(), ip: ip.clone() };
            std::mem::replace(s, new.clone()) != new
        });
    }
}

/// Saved profiles remain visible when their AP disappears; never invent a live AP
/// path for them. Available saved networks retain their scan data and active status.
fn with_saved(mut networks: HashMap<String, Network>, profiles: &HashMap<String, String>) -> Vec<Network> {
    for ssid in profiles.keys() {
        networks.entry(ssid.clone()).and_modify(|network| network.known = true).or_insert_with(|| Network {
            ssid: ssid.clone(), strength: 0, secured: false, known: true,
            active: false, access_point: None,
        });
    }
    let mut networks: Vec<_> = networks.into_values().collect();
    networks.sort_by(|a, b| b.active.cmp(&a.active)
        .then(b.available().cmp(&a.available()))
        .then(b.strength.cmp(&a.strength))
        .then(a.ssid.cmp(&b.ssid)));
    networks
}

/// Saved Wi-Fi profiles: network name → profile path.
async fn saved_profiles() -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Some(settings) = system_proxy(NM, "/org/freedesktop/NetworkManager/Settings", "org.freedesktop.NetworkManager.Settings").await
    else {
        return out;
    };
    let list = call(&settings, "ListConnections", None).await.and_then(|r| child(&r, 0));
    for path in list.iter().flat_map(|l| l.iter()) {
        let Some(path) = path.get::<glib::variant::ObjectPath>() else { continue };
        let Some(conn) = system_proxy(NM, path.as_str(), "org.freedesktop.NetworkManager.Settings.Connection").await
        else {
            continue;
        };
        let Some(all) = call(&conn, "GetSettings", None).await.and_then(|r| child(&r, 0)) else { continue };
        let ssid = lookup(&all, "802-11-wireless")
            .and_then(|w| lookup(&w, "ssid"))
            .and_then(|v| v.get::<Vec<u8>>())
            .map(|b| String::from_utf8_lossy(&b).into_owned());
        if let Some(ssid) = ssid {
            out.insert(ssid, path.as_str().to_string());
        }
    }
    out
}

/// Settings for a new Wi-Fi profile (`a{sa{sv}}`), WPA-PSK when a password is given.
fn new_profile(ssid: &str, password: Option<&str>) -> glib::Variant {
    let mut settings: HashMap<String, HashMap<String, glib::Variant>> = HashMap::new();
    settings.insert(
        "802-11-wireless".into(),
        HashMap::from([("ssid".into(), ssid.as_bytes().to_vec().to_variant())]),
    );
    if let Some(password) = password {
        settings.insert(
            "802-11-wireless-security".into(),
            HashMap::from([
                ("key-mgmt".into(), "wpa-psk".to_variant()),
                ("psk".into(), password.to_variant()),
            ]),
        );
    }
    settings.to_variant()
}

async fn call(proxy: &gio::DBusProxy, method: &str, args: Option<glib::Variant>) -> Option<glib::Variant> {
    proxy
        .call_future(method, args.as_ref(), gio::DBusCallFlags::NONE, -1)
        .await
        .inspect_err(|err| tracing::warn!("NetworkManager {method}: {err}"))
        .ok()
}

async fn properties_set(proxy: &gio::DBusProxy, args: glib::Variant) -> Option<glib::Variant> {
    proxy
        .connection()
        .call_future(
            proxy.name().as_deref(),
            &proxy.object_path(),
            "org.freedesktop.DBus.Properties",
            "Set",
            Some(&args),
            None,
            gio::DBusCallFlags::NONE,
            -1,
        )
        .await
        .inspect_err(|err| tracing::warn!("NetworkManager: {err}"))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen(ssid: &str, strength: u8, active: bool) -> Network {
        Network {
            ssid: ssid.into(), strength, secured: true, known: false, active,
            access_point: Some(format!("/ap/{ssid}")),
        }
    }

    #[test]
    fn saved_networks_survive_missing_scan_results() {
        let profiles = HashMap::from([
            ("Home".into(), "/profile/home".into()),
            ("Cafe".into(), "/profile/cafe".into()),
        ]);
        let active = seen("Home", 30, true);
        let networks = with_saved(HashMap::from([
            ("Home".into(), active), ("Guest".into(), seen("Guest", 90, false)),
        ]), &profiles);
        assert_eq!(networks.iter().map(|n| n.ssid.as_str()).collect::<Vec<_>>(), ["Home", "Guest", "Cafe"]);
        assert!(networks[0].active && networks[0].known && networks[0].available());
        assert_eq!(networks[0].strength, 30);
        assert!(!networks[1].known);
        assert!(networks[2].known && !networks[2].available() && !networks[2].active);
        assert!(networks[2].access_point.is_none());
    }

    #[test]
    fn saved_network_becomes_available_without_duplicate_rows() {
        let profiles = HashMap::from([("Cafe".into(), "/profile/cafe".into())]);
        assert!(!with_saved(HashMap::new(), &profiles)[0].available());
        let networks = with_saved(HashMap::from([("Cafe".into(), seen("Cafe", 65, false))]), &profiles);
        assert_eq!(networks.len(), 1);
        assert!(networks[0].known && networks[0].available());
        assert_eq!(networks[0].strength, 65);
    }

    #[test]
    fn profile_shape() {
        let v = super::new_profile("home", Some("secret"));
        assert_eq!(v.type_().as_str(), "a{sa{sv}}");
        let text = v.to_string();
        assert!(text.contains("wpa-psk") && text.contains("secret"), "{text}");
    }
}

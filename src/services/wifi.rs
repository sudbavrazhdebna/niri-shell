//! Wi-Fi actions for the network menu: scanning, the list of networks, connecting.
//! Runs only while the menu is open; the bar icon itself comes from `network`.

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
    access_point: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WifiState {
    pub enabled: bool,
    /// Strongest first, one entry per network name.
    pub networks: Vec<Network>,
    /// What went wrong with the last connection attempt, for the menu to show.
    pub error: Option<String>,
}

/// Lives while the network menu is open.
pub struct Wifi {
    pub state: Watch<WifiState>,
    nm: gio::DBusProxy,
    device: Option<gio::DBusProxy>,
}

impl Wifi {
    /// Finds the Wi-Fi device, asks it to scan, and keeps the list up to date.
    pub async fn open() -> Option<Rc<Wifi>> {
        let started = std::time::Instant::now();
        let nm = system_proxy(NM, NM_PATH, NM).await?;
        let mut device = None;
        let devices = call(&nm, "GetDevices", None).await.and_then(|r| child(&r, 0));
        for path in devices.iter().flat_map(|d| d.iter()) {
            let Some(path) = path.get::<glib::variant::ObjectPath>() else { continue };
            let Some(dev) = system_proxy(NM, path.as_str(), "org.freedesktop.NetworkManager.Device").await
            else {
                continue;
            };
            if prop::<u32>(&dev, "DeviceType") == Some(DEVICE_WIFI) {
                device = system_proxy(NM, path.as_str(), WIRELESS).await;
                break;
            }
        }

        let wifi = Rc::new(Wifi {
            state: Watch::new(WifiState::default()),
            nm: nm.clone(),
            device,
        });
        let weak = Rc::downgrade(&wifi);
        on_properties_changed(&nm, move |_| {
            if let Some(wifi) = weak.upgrade() {
                glib::spawn_future_local(async move { wifi.refresh().await });
            }
        });
        if let Some(device) = &wifi.device {
            let weak = Rc::downgrade(&wifi);
            // New scan results arrive as AccessPoints/LastScan changes.
            on_properties_changed(device, move |_| {
                if let Some(wifi) = weak.upgrade() {
                    glib::spawn_future_local(async move { wifi.refresh().await });
                }
            });
            let options = glib::VariantDict::new(None).end();
            // tuple_from_iter: a Variant inside a Rust tuple would be boxed as "v".
            let _ = call(device, "RequestScan", Some(glib::Variant::tuple_from_iter([options]))).await;
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

    /// Connects to a network: with its saved profile, or a new one with `password`.
    pub fn connect(self: &Rc<Self>, network: &Network, password: Option<&str>) {
        let (Some(device), this) = (self.device.clone(), self.clone()) else { return };
        let (network, password) = (network.clone(), password.map(str::to_string));
        glib::spawn_future_local(async move {
            let device_path = glib::variant::ObjectPath::try_from(device.object_path().to_string());
            let ap_path = glib::variant::ObjectPath::try_from(network.access_point.clone());
            let (Ok(device_path), Ok(ap_path)) = (device_path, ap_path) else { return };
            let saved = saved_profiles().await.remove(&network.ssid);
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
            let error = result.is_none().then(|| format!("Не удалось подключиться к {}", network.ssid));
            this.state.update(|s| std::mem::replace(&mut s.error, error.clone()) != error);
        });
    }

    async fn refresh(&self) {
        let enabled = prop(&self.nm, "WirelessEnabled").unwrap_or(false);
        let mut networks: HashMap<String, Network> = HashMap::new();
        if let Some(device) = &self.device {
            let active_ap = prop::<glib::variant::ObjectPath>(device, "ActiveAccessPoint")
                .map(|p| p.as_str().to_string());
            let known = saved_profiles().await;
            let points = prop::<Vec<glib::variant::ObjectPath>>(device, "AccessPoints").unwrap_or_default();
            for path in points {
                let Some(ap) =
                    system_proxy(NM, path.as_str(), "org.freedesktop.NetworkManager.AccessPoint").await
                else {
                    continue;
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
                    known: known.contains_key(&ssid),
                    active: active_ap.as_deref() == Some(path.as_str()),
                    ssid: ssid.clone(),
                    strength,
                    secured,
                    access_point: path.as_str().to_string(),
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
        let mut networks: Vec<Network> = networks.into_values().collect();
        networks.sort_by(|a, b| b.active.cmp(&a.active).then(b.strength.cmp(&a.strength)));
        self.state.update(|s| {
            let new = WifiState { enabled, networks, error: s.error.clone() };
            std::mem::replace(s, new.clone()) != new
        });
    }
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
    #[test]
    fn profile_shape() {
        let v = super::new_profile("home", Some("secret"));
        assert_eq!(v.type_().as_str(), "a{sa{sv}}");
        let text = v.to_string();
        assert!(text.contains("wpa-psk") && text.contains("secret"), "{text}");
    }
}

//! Power profiles (power-saver / balanced / performance) from power-profiles-daemon.

use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gio, glib};

use super::dbus::{lookup, on_properties_changed, prop, system_proxy};
use crate::watch::Watch;

const NAME: &str = "org.freedesktop.UPower.PowerProfiles";
const PATH: &str = "/org/freedesktop/UPower/PowerProfiles";

#[derive(Clone, Default, PartialEq, Debug)]
pub struct PowerState {
    /// Profiles this machine offers, in the daemon's order.
    pub profiles: Vec<String>,
    pub active: String,
}

pub struct Power {
    pub state: Watch<PowerState>,
    proxy: Rc<std::cell::RefCell<Option<gio::DBusProxy>>>,
}

impl Power {
    pub fn start() -> Power {
        let power = Power {
            state: Watch::new(PowerState::default()),
            proxy: Rc::default(),
        };
        let (state, slot) = (power.state.clone(), power.proxy.clone());
        glib::spawn_future_local(async move {
            let Some(proxy) = system_proxy(NAME, PATH, NAME).await else { return };
            state.set(read(&proxy));
            on_properties_changed(&proxy, move |proxy| state.set(read(proxy)));
            slot.replace(Some(proxy));
        });
        power
    }

    pub fn set(&self, profile: &str) {
        let Some(proxy) = self.proxy.borrow().clone() else { return };
        let args = (NAME, "ActiveProfile", profile.to_variant()).to_variant();
        glib::spawn_future_local(async move {
            let result = proxy
                .connection()
                .call_future(
                    proxy.name().as_deref(),
                    PATH,
                    "org.freedesktop.DBus.Properties",
                    "Set",
                    Some(&args),
                    None,
                    gio::DBusCallFlags::NONE,
                    -1,
                )
                .await;
            if let Err(err) = result {
                tracing::warn!("power profile: {err}");
            }
        });
    }
}

fn read(proxy: &gio::DBusProxy) -> PowerState {
    let profiles = proxy
        .cached_property("Profiles")
        .map(|list| {
            list.iter()
                .filter_map(|entry| lookup(&entry, "Profile").and_then(|v| v.get::<String>()))
                .collect()
        })
        .unwrap_or_default();
    PowerState {
        profiles,
        active: prop(proxy, "ActiveProfile").unwrap_or_default(),
    }
}

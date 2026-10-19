//! Bluetooth from BlueZ: adapter power and the paired devices.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gio, glib};

use super::dbus::prop;
use crate::watch::Watch;

#[derive(Clone, Default, PartialEq, Debug)]
pub struct Device {
    pub path: String,
    pub name: String,
    pub connected: bool,
    /// Battery percent, if the device reports it.
    pub battery: Option<u8>,
    /// BlueZ icon name, e.g. "audio-headset".
    pub icon: String,
}

#[derive(Clone, Default, PartialEq, Debug)]
pub struct BluetoothState {
    /// Some adapter exists and is powered on.
    pub powered: bool,
    pub has_adapter: bool,
    /// Paired devices, connected first.
    pub devices: Vec<Device>,
}

impl BluetoothState {
    pub fn any_connected(&self) -> bool {
        self.devices.iter().any(|d| d.connected)
    }
}

pub struct Bluetooth {
    pub state: Watch<BluetoothState>,
    manager: Rc<RefCell<Option<gio::DBusObjectManagerClient>>>,
}

impl Bluetooth {
    pub fn start() -> Bluetooth {
        let bluetooth = Bluetooth {
            state: Watch::new(BluetoothState::default()),
            manager: Rc::default(),
        };
        let (watch, slot) = (bluetooth.state.clone(), bluetooth.manager.clone());
        glib::spawn_future_local(async move {
            let manager = gio::DBusObjectManagerClient::new_for_bus_future(
                gio::BusType::System,
                gio::DBusObjectManagerClientFlags::NONE,
                "org.bluez",
                "/",
            )
            .await;
            let manager = match manager {
                Ok(manager) => manager,
                Err(err) => {
                    tracing::warn!("BlueZ: {err}");
                    return;
                }
            };
            watch.set(read(&manager));

            let w = watch.clone();
            manager.connect_object_added(move |m, _| w.set(read(m)));
            let w = watch.clone();
            manager.connect_object_removed(move |m, _| w.set(read(m)));
            let w = watch.clone();
            manager.connect_interface_added(move |m, _, _| w.set(read(m)));
            let w = watch.clone();
            manager.connect_interface_removed(move |m, _, _| w.set(read(m)));
            // The typed connect wants a Send closure; this one stays on the main thread.
            manager.connect_local("interface-proxy-properties-changed", false, move |args| {
                if let Some(m) = args.first().and_then(|v| v.get::<gio::DBusObjectManagerClient>().ok()) {
                    watch.set(read(&m));
                }
                None
            });
            slot.replace(Some(manager));
        });
        bluetooth
    }

    pub fn set_powered(&self, on: bool) {
        for adapter in self.proxies("org.bluez.Adapter1") {
            let args = ("org.bluez.Adapter1", "Powered", on.to_variant()).to_variant();
            spawn_call(adapter, "org.freedesktop.DBus.Properties", "Set", Some(args));
        }
    }

    pub fn connect(&self, path: &str) {
        self.device_call(path, "Connect");
    }

    pub fn disconnect(&self, path: &str) {
        self.device_call(path, "Disconnect");
    }

    fn device_call(&self, path: &str, method: &'static str) {
        let device = self
            .proxies("org.bluez.Device1")
            .into_iter()
            .find(|p| p.object_path() == path);
        if let Some(device) = device {
            spawn_call(device, "org.bluez.Device1", method, None);
        }
    }

    fn proxies(&self, iface: &str) -> Vec<gio::DBusProxy> {
        let Some(manager) = self.manager.borrow().clone() else { return Vec::new() };
        manager
            .objects()
            .iter()
            .filter_map(|o| gio::prelude::DBusObjectExt::interface(o, iface))
            .filter_map(|i| i.downcast::<gio::DBusProxy>().ok())
            .collect()
    }
}

fn spawn_call(proxy: gio::DBusProxy, iface: &'static str, method: &'static str, args: Option<glib::Variant>) {
    glib::spawn_future_local(async move {
        let result = proxy
            .connection()
            .call_future(
                proxy.name().as_deref(),
                &proxy.object_path(),
                iface,
                method,
                args.as_ref(),
                None,
                gio::DBusCallFlags::NONE,
                // Connecting to a device can take a while.
                30_000,
            )
            .await;
        if let Err(err) = result {
            tracing::warn!("BlueZ {method}: {err}");
        }
    });
}

fn read(manager: &impl IsA<gio::DBusObjectManager>) -> BluetoothState {
    let mut state = BluetoothState::default();
    for object in manager.objects() {
        let proxy = |iface: &str| {
            gio::prelude::DBusObjectExt::interface(&object, iface)
                .and_then(|i| i.downcast::<gio::DBusProxy>().ok())
        };
        if let Some(adapter) = proxy("org.bluez.Adapter1") {
            state.has_adapter = true;
            state.powered |= prop(&adapter, "Powered").unwrap_or(false);
        }
        if let Some(device) = proxy("org.bluez.Device1")
            && prop(&device, "Paired").unwrap_or(false)
        {
            state.devices.push(Device {
                path: device.object_path().to_string(),
                name: prop(&device, "Alias").unwrap_or_default(),
                connected: prop(&device, "Connected").unwrap_or(false),
                battery: proxy("org.bluez.Battery1").and_then(|b| prop(&b, "Percentage")),
                icon: prop(&device, "Icon").unwrap_or_default(),
            });
        }
    }
    state
        .devices
        .sort_by(|a, b| b.connected.cmp(&a.connected).then(a.name.cmp(&b.name)));
    state
}

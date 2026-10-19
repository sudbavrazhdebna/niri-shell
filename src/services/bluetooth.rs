//! Bluetooth from BlueZ: whether an adapter is on, and the connected devices.

use gtk4::prelude::*;
use gtk4::{gio, glib};

use super::dbus::prop;
use crate::watch::Watch;

#[derive(Clone, Default, PartialEq, Debug)]
pub struct Device {
    pub name: String,
    /// Battery percent, if the device reports it.
    pub battery: Option<u8>,
}

#[derive(Clone, Default, PartialEq, Debug)]
pub struct BluetoothState {
    /// Some adapter exists and is powered on.
    pub powered: bool,
    pub connected: Vec<Device>,
}

pub struct Bluetooth {
    pub state: Watch<BluetoothState>,
}

impl Bluetooth {
    pub fn start() -> Bluetooth {
        let state = Watch::new(BluetoothState::default());
        let watch = state.clone();
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
            // The handler keeps the manager alive for the life of the process.
            // The typed connect wants a Send closure; this one stays on the main thread.
            manager.connect_local("interface-proxy-properties-changed", false, move |args| {
                if let Some(m) = args.first().and_then(|v| v.get::<gio::DBusObjectManagerClient>().ok()) {
                    watch.set(read(&m));
                }
                None
            });
        });
        Bluetooth { state }
    }
}

fn read(manager: &impl IsA<gio::DBusObjectManager>) -> BluetoothState {
    let mut state = BluetoothState::default();
    for object in manager.objects() {
        let proxy = |iface: &str| {
            gio::prelude::DBusObjectExt::interface(&object, iface)
                .and_then(|i| i.downcast::<gio::DBusProxy>().ok())
        };
        if let Some(adapter) = proxy("org.bluez.Adapter1") {
            state.powered |= prop(&adapter, "Powered").unwrap_or(false);
        }
        if let Some(device) = proxy("org.bluez.Device1")
            && prop(&device, "Connected").unwrap_or(false)
        {
            state.connected.push(Device {
                name: prop(&device, "Alias").unwrap_or_default(),
                battery: proxy("org.bluez.Battery1").and_then(|b| prop(&b, "Percentage")),
            });
        }
    }
    state.connected.sort_by(|a, b| a.name.cmp(&b.name));
    state
}

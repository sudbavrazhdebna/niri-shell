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
    /// The adapter is looking for devices.
    pub scanning: bool,
    /// Paired devices, connected first.
    pub devices: Vec<Device>,
    /// Unpaired devices in range that have a name, nearest first.
    pub nearby: Vec<Device>,
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

    /// Off is an rfkill block, as in GNOME: the adapter's own power switch doesn't last.
    /// A USB adapter is reset on resume, and BlueZ powers every adapter it finds on
    /// (AutoEnable); a block holds for new adapters too, and systemd keeps it over a reboot.
    pub fn set_powered(&self, on: bool) {
        rfkill::block_bluetooth(!on);
        for adapter in self.proxies("org.bluez.Adapter1") {
            power(adapter, on, 10);
        }
    }

    pub fn connect(&self, path: &str) {
        self.device_call(path, "Connect");
    }

    /// Looks for devices for `seconds`, then stops.
    pub fn scan(&self, seconds: u32) {
        for adapter in self.proxies("org.bluez.Adapter1") {
            spawn_call(adapter.clone(), "org.bluez.Adapter1", "StartDiscovery", None);
            glib::timeout_add_seconds_local_once(seconds, move || {
                spawn_call(adapter, "org.bluez.Adapter1", "StopDiscovery", None);
            });
        }
    }

    /// Stops a scan before its time (the menu closed): searching keeps the radio busy.
    pub fn stop_scan(&self) {
        if !self.state.get().scanning {
            return;
        }
        for adapter in self.proxies("org.bluez.Adapter1") {
            spawn_call(adapter, "org.bluez.Adapter1", "StopDiscovery", None);
        }
    }

    /// Pairs with a nearby device, trusts it (so it reconnects by itself) and connects.
    pub fn pair(&self, path: &str) {
        let Some(device) = self
            .proxies("org.bluez.Device1")
            .into_iter()
            .find(|p| p.object_path() == path)
        else {
            return;
        };
        glib::spawn_future_local(async move {
            // BlueZ asks an agent to confirm the pairing; the shell answers yes.
            agent::register(&device.connection()).await;
            let call = |method: &'static str, args: Option<glib::Variant>, iface: &'static str| {
                device.connection().call_future(
                    device.name().as_deref(),
                    &device.object_path(),
                    iface,
                    method,
                    args.as_ref(),
                    None,
                    gio::DBusCallFlags::NONE,
                    60_000,
                )
            };
            if let Err(err) = call("Pair", None, "org.bluez.Device1").await {
                tracing::warn!("BlueZ Pair: {err}");
                return;
            }
            let trusted = ("org.bluez.Device1", "Trusted", true.to_variant()).to_variant();
            let _ = call("Set", Some(trusted), "org.freedesktop.DBus.Properties").await;
            if let Err(err) = call("Connect", None, "org.bluez.Device1").await {
                tracing::warn!("BlueZ Connect: {err}");
            }
        });
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

/// Sets the adapter's power. Right after an unblock BlueZ may not have heard of it yet
/// and refuses ("Blocked"): tried again a moment later, `tries` times at most.
fn power(adapter: gio::DBusProxy, on: bool, tries: u32) {
    glib::spawn_future_local(async move {
        let args = ("org.bluez.Adapter1", "Powered", on.to_variant()).to_variant();
        let result = adapter
            .connection()
            .call_future(
                adapter.name().as_deref(),
                &adapter.object_path(),
                "org.freedesktop.DBus.Properties",
                "Set",
                Some(&args),
                None,
                gio::DBusCallFlags::NONE,
                10_000,
            )
            .await;
        match result {
            Ok(_) => {}
            Err(err) if tries > 1 && on && err.to_string().contains("Blocked") => {
                glib::timeout_add_local_once(std::time::Duration::from_millis(200), move || {
                    power(adapter, on, tries - 1);
                });
            }
            Err(err) => tracing::warn!("BlueZ Powered: {err}"),
        }
    });
}

/// The kernel's radio switches (/dev/rfkill), writable by the user of the active session.
mod rfkill {
    use std::io::Write;

    const TYPE_BLUETOOTH: u8 = 2;
    const OP_CHANGE_ALL: u8 = 3;

    /// Soft-blocks (or unblocks) every Bluetooth radio, present and future.
    pub fn block_bluetooth(block: bool) {
        // struct rfkill_event: idx (u32), type, op, soft, hard.
        let mut event = [0u8; 8];
        event[4] = TYPE_BLUETOOTH;
        event[5] = OP_CHANGE_ALL;
        event[6] = u8::from(block);
        let result = std::fs::OpenOptions::new()
            .write(true)
            .open("/dev/rfkill")
            .and_then(|mut file| file.write_all(&event));
        if let Err(err) = result {
            tracing::warn!("rfkill: {err}");
        }
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
            state.has_adapter = true;
            state.powered |= prop(&adapter, "Powered").unwrap_or(false);
            state.scanning |= prop(&adapter, "Discovering").unwrap_or(false);
        }
        let Some(device) = proxy("org.bluez.Device1") else { continue };
        let entry = Device {
            path: device.object_path().to_string(),
            name: prop(&device, "Alias").unwrap_or_default(),
            connected: prop(&device, "Connected").unwrap_or(false),
            battery: proxy("org.bluez.Battery1").and_then(|b| prop(&b, "Percentage")),
            icon: prop(&device, "Icon").unwrap_or_default(),
        };
        if prop(&device, "Paired").unwrap_or(false) {
            state.devices.push(entry);
        } else if prop::<String>(&device, "Name").is_some() && prop::<i16>(&device, "RSSI").is_some() {
            // Only devices heard in this scan (they have a signal) and with a real name.
            state.nearby.push(entry);
        }
    }
    state
        .devices
        .sort_by(|a, b| b.connected.cmp(&a.connected).then(a.name.cmp(&b.name)));
    // By name: signal strength changes every second during a scan and would reshuffle
    // (and redraw) the list all the time.
    state.nearby.sort_by(|a, b| a.name.cmp(&b.name));
    state
}

/// A BlueZ pairing agent that says yes: it only ever runs for pairings the user
/// started from the menu. "NoInputNoOutput": no PIN to show or type (headphones,
/// speakers, mice).
mod agent {
    use std::cell::Cell;

    use gtk4::prelude::*;
    use gtk4::{gio, glib};

    const PATH: &str = "/dev/nshell/BluetoothAgent";
    const XML: &str = r#"
<node>
  <interface name="org.bluez.Agent1">
    <method name="Release"/>
    <method name="RequestPinCode"><arg type="o" direction="in"/><arg type="s" direction="out"/></method>
    <method name="DisplayPinCode"><arg type="o" direction="in"/><arg type="s" direction="in"/></method>
    <method name="RequestPasskey"><arg type="o" direction="in"/><arg type="u" direction="out"/></method>
    <method name="DisplayPasskey"><arg type="o" direction="in"/><arg type="u" direction="in"/><arg type="q" direction="in"/></method>
    <method name="RequestConfirmation"><arg type="o" direction="in"/><arg type="u" direction="in"/></method>
    <method name="RequestAuthorization"><arg type="o" direction="in"/></method>
    <method name="AuthorizeService"><arg type="o" direction="in"/><arg type="s" direction="in"/></method>
    <method name="Cancel"/>
  </interface>
</node>"#;

    thread_local! {
        static REGISTERED: Cell<bool> = const { Cell::new(false) };
    }

    pub async fn register(conn: &gio::DBusConnection) {
        if REGISTERED.get() {
            return;
        }
        let Some(info) = gio::DBusNodeInfo::for_xml(XML).ok().and_then(|n| n.lookup_interface("org.bluez.Agent1"))
        else {
            return;
        };
        let object = conn
            .register_object(PATH, &info)
            .method_call(|_, _, _, _, method, _, invocation| match method {
                // PIN-based pairing needs input the menu doesn't offer.
                "RequestPinCode" | "RequestPasskey" => {
                    invocation.return_dbus_error("org.bluez.Error.Rejected", "no PIN entry")
                }
                _ => invocation.return_value(None),
            })
            .build();
        if let Err(err) = object {
            tracing::warn!("Bluetooth agent: {err}");
            return;
        }
        let result = conn
            .call_future(
                Some("org.bluez"),
                "/org/bluez",
                "org.bluez.AgentManager1",
                "RegisterAgent",
                Some(&(glib::variant::ObjectPath::try_from(PATH).ok(), "NoInputNoOutput").to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                -1,
            )
            .await;
        match result {
            Ok(_) => REGISTERED.set(true),
            Err(err) => tracing::warn!("Bluetooth agent: {err}"),
        }
    }

}

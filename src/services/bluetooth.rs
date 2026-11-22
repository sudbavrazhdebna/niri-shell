//! Bluetooth from BlueZ: adapter power and the paired devices.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConnectionAttempt {
    Connecting(u64),
    Failed,
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
    attempts: HashMap<String, ConnectionAttempt>,
}

impl BluetoothState {
    pub fn any_connected(&self) -> bool {
        self.devices.iter().any(|d| d.connected)
    }

    pub fn connection_attempt(&self, path: &str) -> Option<ConnectionAttempt> {
        self.attempts.get(path).copied()
    }
}

pub struct Bluetooth {
    pub state: Watch<BluetoothState>,
    manager: Rc<RefCell<Option<gio::DBusObjectManagerClient>>>,
    attempt_serial: Cell<u64>,
}

impl Bluetooth {
    pub fn start() -> Bluetooth {
        let bluetooth = Bluetooth {
            state: Watch::new(BluetoothState::default()),
            manager: Rc::default(),
            attempt_serial: Cell::default(),
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
            refresh(&watch, &manager);

            let w = watch.clone();
            manager.connect_object_added(move |m, _| refresh(&w, m));
            let w = watch.clone();
            manager.connect_object_removed(move |m, _| refresh(&w, m));
            let w = watch.clone();
            manager.connect_interface_added(move |m, _, _| refresh(&w, m));
            let w = watch.clone();
            manager.connect_interface_removed(move |m, _, _| refresh(&w, m));
            // The typed connect wants a Send closure; this one stays on the main thread.
            manager.connect_local("interface-proxy-properties-changed", false, move |args| {
                if let Some(m) = args.first().and_then(|v| v.get::<gio::DBusObjectManagerClient>().ok()) {
                    refresh(&watch, &m);
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
        self.connect_device(path, false);
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
        self.connect_device(path, true);
    }

    fn connect_device(&self, path: &str, pair: bool) {
        if matches!(self.state.get().connection_attempt(path), Some(ConnectionAttempt::Connecting(_))) {
            return;
        }
        let Some(device) = self.device(path) else { return };
        let serial = self.attempt_serial.get() + 1;
        self.attempt_serial.set(serial);
        let attempt = ConnectionAttempt::Connecting(serial);
        let path = path.to_owned();
        self.state.update(|state| {
            state.attempts.insert(path.clone(), attempt);
            true
        });
        let watch = self.state.clone();
        glib::spawn_future_local(async move {
            let connected = pair_and_connect(&device, pair).await;
            watch.update(|state| {
                // A property change can finish the attempt first or remove this device.
                if state.connection_attempt(&path) != Some(attempt) {
                    return false;
                }
                if connected {
                    state.attempts.remove(&path);
                } else {
                    state.attempts.insert(path, ConnectionAttempt::Failed);
                }
                true
            });
        });
    }

    pub fn disconnect(&self, path: &str) {
        if let Some(device) = self.device(path) {
            spawn_call(device, "org.bluez.Device1", "Disconnect", None);
        }
    }

    fn device(&self, path: &str) -> Option<gio::DBusProxy> {
        self.proxies("org.bluez.Device1")
            .into_iter()
            .find(|p| p.object_path() == path)
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

async fn pair_and_connect(device: &gio::DBusProxy, pair: bool) -> bool {
    const DEVICE: &str = "org.bluez.Device1";
    if pair {
        agent::register(&device.connection()).await;
        if !call(device, DEVICE, "Pair", None, 60_000).await {
            return false;
        }
        let trusted = (DEVICE, "Trusted", true.to_variant()).to_variant();
        call(device, "org.freedesktop.DBus.Properties", "Set", Some(trusted), 60_000).await;
    }
    let timeout = if pair { 60_000 } else { 30_000 };
    call(device, DEVICE, "Connect", None, timeout).await
}

fn spawn_call(proxy: gio::DBusProxy, iface: &'static str, method: &'static str, args: Option<glib::Variant>) {
    glib::spawn_future_local(async move { call(&proxy, iface, method, args, 30_000).await });
}

async fn call(proxy: &gio::DBusProxy, iface: &str, method: &str, args: Option<glib::Variant>, timeout: i32) -> bool {
    proxy.connection().call_future(
        proxy.name().as_deref(),
        &proxy.object_path(),
        iface,
        method,
        args.as_ref(),
        None,
        gio::DBusCallFlags::NONE,
        timeout,
    ).await.inspect_err(|err| tracing::warn!("BlueZ {method}: {err}")).is_ok()
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

fn refresh(watch: &Watch<BluetoothState>, manager: &impl IsA<gio::DBusObjectManager>) {
    let mut state = read(manager);
    if state.powered {
        let previous = watch.get();
        for device in state.devices.iter().chain(&state.nearby).filter(|device| !device.connected) {
            if let Some(attempt) = previous.connection_attempt(&device.path) {
                state.attempts.insert(device.path.clone(), attempt);
            }
        }
    }
    watch.set(state);
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

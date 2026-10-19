//! System tray (StatusNotifierItem). nshell runs the watcher that apps register with;
//! if another one already runs (an old bar), it reads the items from that one instead.

mod item;
pub mod menu;

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use gtk4::prelude::*;
use gtk4::{gio, glib};

pub use item::{Item, Status};

use crate::watch::Watch;

const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const ITEM_PATH: &str = "/StatusNotifierItem";

const WATCHER_XML: &str = r#"
<node>
  <interface name="org.kde.StatusNotifierWatcher">
    <method name="RegisterStatusNotifierItem"><arg name="service" type="s" direction="in"/></method>
    <method name="RegisterStatusNotifierHost"><arg name="service" type="s" direction="in"/></method>
    <property name="RegisteredStatusNotifierItems" type="as" access="read"/>
    <property name="IsStatusNotifierHostRegistered" type="b" access="read"/>
    <property name="ProtocolVersion" type="i" access="read"/>
    <signal name="StatusNotifierItemRegistered"><arg type="s"/></signal>
    <signal name="StatusNotifierItemUnregistered"><arg type="s"/></signal>
    <signal name="StatusNotifierHostRegistered"/>
    <signal name="StatusNotifierHostUnregistered"/>
  </interface>
</node>"#;

pub struct Tray {
    pub items: Watch<Vec<Rc<Item>>>,
    conn: RefCell<Option<gio::DBusConnection>>,
    /// True while this process owns the watcher name and must announce changes.
    is_watcher: std::cell::Cell<bool>,
}

impl Tray {
    pub fn start() -> Rc<Tray> {
        let tray = Rc::new(Tray {
            items: Watch::new(Vec::new()),
            conn: RefCell::new(None),
            is_watcher: std::cell::Cell::new(false),
        });

        let (on_bus, on_name, on_lost) = (Rc::downgrade(&tray), Rc::downgrade(&tray), Rc::downgrade(&tray));
        // The name stays requested for the life of the process.
        let _owner = gio::bus_own_name(
            gio::BusType::Session,
            WATCHER,
            gio::BusNameOwnerFlags::NONE,
            move |conn, _| {
                if let Some(tray) = on_bus.upgrade() {
                    tray.on_bus(conn);
                }
            },
            move |_, _| {
                if let Some(tray) = on_name.upgrade() {
                    tracing::info!("tray: running the StatusNotifierWatcher");
                    tray.is_watcher.set(true);
                }
            },
            move |conn, _| {
                let (Some(tray), Some(conn)) = (on_lost.upgrade(), conn) else {
                    return;
                };
                if !tray.is_watcher.get() {
                    tracing::info!("tray: another watcher runs, reading items from it");
                    tray.follow_other_watcher(conn);
                }
            },
        );
        tray
    }

    fn on_bus(self: &Rc<Self>, conn: gio::DBusConnection) {
        self.conn.replace(Some(conn.clone()));
        self.register_watcher_object(&conn);

        // An app that quits without unregistering: drop its items.
        let weak = Rc::downgrade(self);
        let subscription = conn.subscribe_to_signal(
            Some("org.freedesktop.DBus"),
            Some("org.freedesktop.DBus"),
            Some("NameOwnerChanged"),
            Some("/org/freedesktop/DBus"),
            None,
            gio::DBusSignalFlags::NONE,
            move |signal| {
                let Some(tray) = weak.upgrade() else { return };
                if let Some((name, _, new)) = signal.parameters.get::<(String, String, String)>()
                    && new.is_empty()
                {
                    tray.remove_where(|item| item.bus == name);
                }
            },
        );
        std::mem::forget(subscription);
    }

    fn register_watcher_object(self: &Rc<Self>, conn: &gio::DBusConnection) {
        let info = gio::DBusNodeInfo::for_xml(WATCHER_XML)
            .ok()
            .and_then(|node| node.lookup_interface(WATCHER));
        let Some(info) = info else {
            tracing::error!("tray: bad watcher interface description");
            return;
        };
        let (calls, props) = (Rc::downgrade(self), Rc::downgrade(self));
        let result = conn
            .register_object(WATCHER_PATH, &info)
            .method_call(move |conn, sender, _, _, method, params, invocation| {
                let Some(tray) = calls.upgrade() else { return };
                match method {
                    "RegisterStatusNotifierItem" => {
                        let service = params.get::<(String,)>().map(|(s,)| s).unwrap_or_default();
                        let sender = sender.unwrap_or_default().to_string();
                        // Some apps pass an object path and expect the sender as bus name.
                        let (bus, path) = if service.starts_with('/') {
                            (sender, service)
                        } else {
                            (service, ITEM_PATH.to_string())
                        };
                        glib::spawn_future_local(add_item(Rc::downgrade(&tray), conn, bus, path));
                        invocation.return_value(None);
                    }
                    "RegisterStatusNotifierHost" => invocation.return_value(None),
                    _ => invocation.return_error(gio::IOErrorEnum::NotSupported, "unknown method"),
                }
            })
            .property(move |_, _, _, _, name| {
                let tray = props.upgrade();
                match name {
                    "RegisteredStatusNotifierItems" => tray
                        .map(|t| t.items.get().iter().map(|i| i.key()).collect::<Vec<_>>())
                        .unwrap_or_default()
                        .to_variant(),
                    "IsStatusNotifierHostRegistered" => true.to_variant(),
                    _ => 0i32.to_variant(),
                }
            })
            .build();
        if let Err(err) = result {
            tracing::error!("tray: can't register the watcher: {err}");
        }
    }

    /// Host-only mode: items come from the other watcher's list and signals.
    fn follow_other_watcher(self: &Rc<Self>, conn: gio::DBusConnection) {
        let weak = Rc::downgrade(self);
        let c = conn.clone();
        let subscription = conn.subscribe_to_signal(
            Some(WATCHER),
            Some(WATCHER),
            None,
            Some(WATCHER_PATH),
            None,
            gio::DBusSignalFlags::NONE,
            move |signal| {
                let Some(tray) = weak.upgrade() else { return };
                let Some((key,)) = signal.parameters.get::<(String,)>() else { return };
                match signal.signal_name {
                    "StatusNotifierItemRegistered" => {
                        let (bus, path) = split_key(&key);
                        glib::spawn_future_local(add_item(Rc::downgrade(&tray), c.clone(), bus, path));
                    }
                    "StatusNotifierItemUnregistered" => {
                        let (bus, path) = split_key(&key);
                        tray.remove_where(|item| item.bus == bus && item.path == path);
                    }
                    _ => {}
                }
            },
        );
        std::mem::forget(subscription);

        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let reply = conn
                .call_future(
                    Some(WATCHER),
                    WATCHER_PATH,
                    "org.freedesktop.DBus.Properties",
                    "Get",
                    Some(&(WATCHER, "RegisteredStatusNotifierItems").to_variant()),
                    None,
                    gio::DBusCallFlags::NONE,
                    -1,
                )
                .await;
            let keys = reply
                .ok()
                .and_then(|v| crate::services::dbus::child(&v, 0))
                .and_then(|v| v.as_variant())
                .and_then(|v| v.get::<Vec<String>>())
                .unwrap_or_default();
            for key in keys {
                let (bus, path) = split_key(&key);
                add_item(weak.clone(), conn.clone(), bus, path).await;
            }
        });
    }

    fn remove_where(&self, gone: impl Fn(&Item) -> bool) {
        let mut removed = Vec::new();
        self.items.update(|items| {
            let before = items.len();
            items.retain(|item| {
                let drop = gone(item);
                if drop {
                    removed.push(item.key());
                }
                !drop
            });
            items.len() != before
        });
        for key in removed {
            tracing::info!("tray: {key} removed");
            self.emit("StatusNotifierItemUnregistered", &key);
        }
    }

    fn emit(&self, signal: &str, key: &str) {
        if !self.is_watcher.get() {
            return;
        }
        if let Some(conn) = &*self.conn.borrow() {
            let _ = conn.emit_signal(None, WATCHER_PATH, WATCHER, signal, Some(&(key,).to_variant()));
        }
    }
}

/// "bus/object/path" → ("bus", "/object/path"); a bare bus name uses the default path.
fn split_key(key: &str) -> (String, String) {
    match key.find('/') {
        Some(i) => (key[..i].to_string(), key[i..].to_string()),
        None => (key.to_string(), ITEM_PATH.to_string()),
    }
}

async fn add_item(tray: Weak<Tray>, conn: gio::DBusConnection, bus: String, path: String) {
    // Apps register under a well-known name and sometimes again under their unique
    // name; key everything by the unique one so they show up once.
    let unique = if bus.starts_with(':') {
        Some(bus.clone())
    } else {
        conn.call_future(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "GetNameOwner",
            Some(&(bus.as_str(),).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            -1,
        )
        .await
        .ok()
        .and_then(|v| v.get::<(String,)>())
        .map(|(owner,)| owner)
    };
    let Some(bus) = unique else {
        tracing::debug!("tray: {bus} is gone before it registered");
        return;
    };
    if tray
        .upgrade()
        .is_none_or(|t| t.items.get().iter().any(|i| i.bus == bus && i.path == path))
    {
        return;
    }

    let Some(item) = Item::connect(&conn, &bus, &path).await else {
        return;
    };
    let Some(tray) = tray.upgrade() else { return };
    // It may have registered twice while we were connecting.
    if tray.items.get().iter().any(|i| i.bus == bus && i.path == path) {
        return;
    }
    let key = item.key();
    tracing::info!("tray: {key} added");
    tray.items.update(|items| {
        items.push(item);
        true
    });
    tray.emit("StatusNotifierItemRegistered", &key);
}

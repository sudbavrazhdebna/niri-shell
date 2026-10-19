//! The tray's StatusNotifierWatcher as a process of its own, so tray icons outlive
//! restarts of the shell: Qt apps register their icon once, and one whose watcher went
//! away (the shell restarting) never showed it again. The shell finds this watcher on
//! the bus and reads the items from it; without it, the shell is the watcher itself.
//!
//! No GTK: a few MB of memory, and it sleeps unless an app registers or quits.

use std::cell::RefCell;
use std::rc::Rc;

use gio::prelude::*;

const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const ITEM_PATH: &str = "/StatusNotifierItem";
/// Qt and KDE apps own a name like this for their item.
const ITEM_NAME_PREFIX: &str = "org.kde.StatusNotifierItem-";

const XML: &str = r#"
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

/// Registered items as (bus name, object path).
#[derive(Default)]
struct Watcher {
    items: RefCell<Vec<(String, String)>>,
    conn: RefCell<Option<gio::DBusConnection>>,
}

impl Watcher {
    fn add(&self, bus: String, path: String) {
        let key = format!("{bus}{path}");
        {
            let mut items = self.items.borrow_mut();
            if items.iter().any(|(b, p)| *b == bus && *p == path) {
                return;
            }
            items.push((bus, path));
        }
        tracing::info!("{key} registered");
        self.emit("StatusNotifierItemRegistered", Some((key,).to_variant()));
    }

    /// Drops the items of an app that left the bus.
    fn remove_bus(&self, name: &str) {
        let mut gone = Vec::new();
        self.items.borrow_mut().retain(|(bus, path)| {
            let keep = bus != name;
            if !keep {
                gone.push(format!("{bus}{path}"));
            }
            keep
        });
        for key in gone {
            tracing::info!("{key} gone");
            self.emit("StatusNotifierItemUnregistered", Some((key,).to_variant()));
        }
    }

    fn emit(&self, signal: &str, args: Option<glib::Variant>) {
        if let Some(conn) = &*self.conn.borrow()
            && let Err(err) = conn.emit_signal(None, WATCHER_PATH, WATCHER, signal, args.as_ref())
        {
            tracing::warn!("{signal}: {err}");
        }
    }
}

fn main() -> glib::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("NSHELL_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let watcher = Rc::new(Watcher::default());
    let main_loop = glib::MainLoop::new(None, false);
    let (on_bus, on_name, on_lost) = (watcher.clone(), watcher.clone(), main_loop.clone());
    // Queued if the shell already is the watcher: this takes over when the shell exits,
    // in the same instant, so the name is never without an owner.
    let _owner = gio::bus_own_name(
        gio::BusType::Session,
        WATCHER,
        gio::BusNameOwnerFlags::NONE,
        move |conn, _| serve(&on_bus, &conn),
        move |conn, _| {
            tracing::info!("running the StatusNotifierWatcher");
            glib::spawn_future_local(adopt_existing(on_name.clone(), conn));
        },
        move |conn, _| {
            if conn.is_none() {
                tracing::error!("no session bus");
                on_lost.quit();
            } else {
                tracing::info!("another watcher runs; waiting for the name");
            }
        },
    );
    main_loop.run();
    glib::ExitCode::FAILURE
}

/// Exports the watcher object and follows apps leaving the bus.
fn serve(watcher: &Rc<Watcher>, conn: &gio::DBusConnection) {
    watcher.conn.replace(Some(conn.clone()));
    let info = gio::DBusNodeInfo::for_xml(XML).ok().and_then(|n| n.lookup_interface(WATCHER));
    let Some(info) = info else {
        tracing::error!("bad interface description");
        return;
    };
    let (calls, props) = (watcher.clone(), watcher.clone());
    let result = conn
        .register_object(WATCHER_PATH, &info)
        .method_call(move |_, sender, _, _, method, params, invocation| match method {
            "RegisterStatusNotifierItem" => {
                let service = params.get::<(String,)>().map(|(s,)| s).unwrap_or_default();
                let sender = sender.unwrap_or_default().to_string();
                // Some apps pass an object path and mean their own bus name.
                let (bus, path) = if service.starts_with('/') {
                    (sender, service)
                } else if service.is_empty() {
                    (sender, ITEM_PATH.to_string())
                } else {
                    (service, ITEM_PATH.to_string())
                };
                calls.add(bus, path);
                invocation.return_value(None);
            }
            "RegisterStatusNotifierHost" => {
                calls.emit("StatusNotifierHostRegistered", None);
                invocation.return_value(None);
            }
            _ => invocation.return_error(gio::IOErrorEnum::NotSupported, "unknown method"),
        })
        .property(move |_, _, _, _, name| match name {
            "RegisteredStatusNotifierItems" => props
                .items
                .borrow()
                .iter()
                .map(|(bus, path)| format!("{bus}{path}"))
                .collect::<Vec<_>>()
                .to_variant(),
            // Always yes: apps then put up their icon even while the shell restarts, and
            // it shows as soon as the shell is back.
            "IsStatusNotifierHostRegistered" => true.to_variant(),
            _ => 0i32.to_variant(),
        })
        .build();
    if let Err(err) = result {
        tracing::error!("can't export the watcher: {err}");
    }

    let weak = Rc::downgrade(watcher);
    let subscription = conn.subscribe_to_signal(
        Some("org.freedesktop.DBus"),
        Some("org.freedesktop.DBus"),
        Some("NameOwnerChanged"),
        Some("/org/freedesktop/DBus"),
        None,
        gio::DBusSignalFlags::NONE,
        move |signal| {
            let Some(watcher) = weak.upgrade() else { return };
            if let Some((name, _, new)) = signal.parameters.get::<(String, String, String)>()
                && new.is_empty()
            {
                watcher.remove_bus(&name);
            }
        },
    );
    // Lives as long as the process.
    std::mem::forget(subscription);
}

/// Items of apps that registered with an earlier watcher (Qt and KDE apps own a
/// `org.kde.StatusNotifierItem-…` name): they don't register again by themselves.
async fn adopt_existing(watcher: Rc<Watcher>, conn: gio::DBusConnection) {
    let names = conn
        .call_future(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "ListNames",
            None,
            None,
            gio::DBusCallFlags::NONE,
            -1,
        )
        .await
        .ok()
        .and_then(|v| v.get::<(Vec<String>,)>())
        .map(|(names,)| names)
        .unwrap_or_default();
    for name in names.into_iter().filter(|n| n.starts_with(ITEM_NAME_PREFIX)) {
        watcher.add(name, ITEM_PATH.to_string());
    }
}

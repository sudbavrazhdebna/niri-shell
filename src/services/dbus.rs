//! Small helpers over GLib's D-Bus proxies.

use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;

/// A cached property of the proxy, if present and of the expected type.
pub fn prop<T: glib::variant::FromVariant>(proxy: &gio::DBusProxy, name: &str) -> Option<T> {
    proxy.cached_property(name).and_then(|v| v.get::<T>())
}

/// Proxy for one object on the system bus; None (logged) if the call fails.
pub async fn system_proxy(name: &str, path: &str, interface: &str) -> Option<gio::DBusProxy> {
    gio::DBusProxy::for_bus_future(
        gio::BusType::System,
        gio::DBusProxyFlags::GET_INVALIDATED_PROPERTIES,
        None,
        name,
        path,
        interface,
    )
    .await
    .inspect_err(|err| tracing::warn!("{name} {path}: {err}"))
    .ok()
}

/// Calls `changed` whenever the proxy's properties change. The handler lives as long
/// as the proxy does.
pub fn on_properties_changed(proxy: &gio::DBusProxy, changed: impl Fn(&gio::DBusProxy) + 'static) {
    proxy.connect_local("g-properties-changed", false, move |args| {
        if let Some(proxy) = args.first().and_then(|v| v.get::<gio::DBusProxy>().ok()) {
            changed(&proxy);
        }
        None
    });
}

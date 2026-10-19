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

/// Element `i` of a container, or None: apps send all kinds of malformed data and
/// `child_value` panics on a bad index.
pub fn child(v: &glib::Variant, i: usize) -> Option<glib::Variant> {
    (v.is_container() && i < v.n_children()).then(|| v.child_value(i))
}

/// Value under `key` in a dictionary with string keys (`a{sv}`, `a{sa{sv}}`, …), with
/// a `v` box removed. Unlike `VariantDict`, never panics on an unexpected type.
pub fn lookup(dict: &glib::Variant, key: &str) -> Option<glib::Variant> {
    let ty = dict.type_();
    if !ty.is_array() || !ty.element().is_dict_entry() || ty.element().key() != glib::VariantTy::STRING {
        return None;
    }
    dict.iter().find_map(|entry| {
        (child(&entry, 0)?.str()? == key).then(|| child(&entry, 1)).flatten()
    })
    .map(|v| if v.is::<glib::Variant>() { v.as_variant().unwrap_or(v) } else { v })
}

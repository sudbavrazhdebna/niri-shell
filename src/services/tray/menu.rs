//! Tray menus: the app's dbusmenu read into a plain tree that the bar draws itself.

use gtk4::prelude::*;
use gtk4::{gio, glib};

use crate::services::dbus::{child, lookup};

const IFACE: &str = "com.canonical.dbusmenu";

#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    Separator,
    Item {
        id: i32,
        label: String,
        enabled: bool,
        /// Some for check marks and radio items: whether it is on.
        toggle: Option<bool>,
    },
    Submenu {
        label: String,
        enabled: bool,
        children: Vec<Entry>,
    },
}

/// Where clicks on a menu's items go.
#[derive(Clone)]
pub struct Sender {
    conn: gio::DBusConnection,
    bus: String,
    path: String,
}

impl Sender {
    pub fn clicked(&self, id: i32) {
        let args = (id, "clicked", 0i32.to_variant(), 0u32).to_variant();
        let call = self.conn.call_future(
            Some(&self.bus),
            &self.path,
            IFACE,
            "Event",
            Some(&args),
            None,
            gio::DBusCallFlags::NONE,
            -1,
        );
        glib::spawn_future_local(async move {
            if let Err(err) = call.await {
                tracing::warn!("tray menu click: {err}");
            }
        });
    }
}

/// Fetches the menu fresh (apps fill it lazily).
pub async fn load(conn: &gio::DBusConnection, bus: &str, path: &str) -> Option<(Vec<Entry>, Sender)> {
    let call = |method: &str, args: glib::Variant| {
        conn.call_future(
            Some(bus),
            path,
            IFACE,
            method,
            Some(&args),
            None,
            gio::DBusCallFlags::NONE,
            -1,
        )
    };
    // Lets the app update the menu before it is shown; many don't implement it.
    let _ = call("AboutToShow", (0i32,).to_variant()).await;
    let reply = call("GetLayout", (0i32, -1i32, Vec::<String>::new()).to_variant())
        .await
        .inspect_err(|err| tracing::warn!("tray menu {bus}{path}: {err}"))
        .ok()?;
    let entries = entries(&child(&reply, 1)?);
    let sender = Sender {
        conn: conn.clone(),
        bus: bus.to_string(),
        path: path.to_string(),
    };
    Some((entries, sender))
}

/// Unwraps `v` boxes, however many the app put around a value.
fn unbox(mut v: glib::Variant) -> glib::Variant {
    while v.is::<glib::Variant>()
        && let Some(inner) = v.as_variant()
    {
        v = inner;
    }
    v
}

/// Children of a dbusmenu layout node `(id, properties, children)`.
fn entries(node: &glib::Variant) -> Vec<Entry> {
    let children: Vec<_> = child(node, 2)
        .map(|c| c.iter().map(unbox).collect())
        .unwrap_or_default();
    let mut out = Vec::new();
    for node in &children {
        let (Some(id), Some(props)) = (
            child(node, 0).and_then(|v| v.get::<i32>()),
            child(node, 1).map(unbox),
        ) else {
            continue;
        };
        let get = |key: &str| lookup(&props, key).map(unbox);
        let string = |key: &str| get(key).and_then(|v| v.get::<String>()).unwrap_or_default();
        if !get("visible").and_then(|v| v.get::<bool>()).unwrap_or(true) {
            continue;
        }
        if string("type") == "separator" {
            // No leading or doubled separators.
            if !matches!(out.last(), None | Some(Entry::Separator)) {
                out.push(Entry::Separator);
            }
            continue;
        }
        let label = strip_mnemonic(&string("label"));
        let enabled = get("enabled").and_then(|v| v.get::<bool>()).unwrap_or(true);
        let sub = entries(node);
        if !sub.is_empty() || string("children-display") == "submenu" {
            out.push(Entry::Submenu {
                label,
                enabled,
                children: sub,
            });
            continue;
        }
        let toggle = matches!(string("toggle-type").as_str(), "checkmark" | "radio")
            .then(|| get("toggle-state").and_then(|v| v.get::<i32>()) == Some(1));
        out.push(Entry::Item {
            id,
            label,
            enabled,
            toggle,
        });
    }
    if out.last() == Some(&Entry::Separator) {
        out.pop();
    }
    out
}

/// "_Open" → "Open", "Save __as" → "Save _as": the underscore marks a keyboard
/// shortcut letter, which the bar's menus don't use.
fn strip_mnemonic(label: &str) -> String {
    let mut out = String::with_capacity(label.len());
    let mut chars = label.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '_' {
            if chars.peek() == Some(&'_') {
                out.push('_');
                chars.next();
            }
            continue;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mnemonics() {
        assert_eq!(strip_mnemonic("_Open"), "Open");
        assert_eq!(strip_mnemonic("Save __as"), "Save _as");
    }

    #[test]
    fn layout_with_double_boxed_children() {
        let props = |pairs: &[(&str, glib::Variant)]| {
            let dict = glib::VariantDict::new(None);
            for (k, v) in pairs {
                dict.insert_value(k, v);
            }
            dict.end()
        };
        let leaf = |id: i32, p: glib::Variant| {
            (id, p, Vec::<glib::Variant>::new()).to_variant()
        };
        let a = leaf(1, props(&[("label", "_Hello".to_variant())]));
        let sep = leaf(2, props(&[("type", "separator".to_variant())]));
        let hidden = leaf(3, props(&[("label", "x".to_variant()), ("visible", false.to_variant())]));
        // Some apps box children twice.
        let boxed = |v: glib::Variant| glib::Variant::from_variant(&glib::Variant::from_variant(&v));
        let root = (0i32, props(&[]), vec![boxed(a), boxed(sep), boxed(hidden)]).to_variant();
        let got = entries(&root);
        assert_eq!(
            got,
            vec![Entry::Item { id: 1, label: "Hello".into(), enabled: true, toggle: None }]
        );
    }
}

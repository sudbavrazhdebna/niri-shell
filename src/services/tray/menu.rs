//! Tray menus: the app's dbusmenu turned into a GTK menu model.

use gtk4::prelude::*;
use gtk4::{gio, glib};

use crate::services::dbus::child;

const IFACE: &str = "com.canonical.dbusmenu";
/// Action group name the menu's items refer to ("tray.i42").
pub const GROUP: &str = "tray";

/// Fetches the menu fresh (apps fill it lazily) and builds the model and its actions.
pub async fn load(
    conn: &gio::DBusConnection,
    bus: &str,
    path: &str,
) -> Option<(gio::Menu, gio::SimpleActionGroup)> {
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

    let actions = gio::SimpleActionGroup::new();
    let root = child(&reply, 1)?;
    let sender = Sender {
        conn: conn.clone(),
        bus: bus.to_string(),
        path: path.to_string(),
    };
    let menu = build_menu(&root, &actions, &sender);
    tracing::debug!(items = menu.n_items(), "tray menu {bus}{path}");
    Some((menu, actions))
}

#[derive(Clone)]
struct Sender {
    conn: gio::DBusConnection,
    bus: String,
    path: String,
}

impl Sender {
    fn clicked(&self, id: i32) {
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

/// Unwraps `v` boxes, however many the app put around a value.
fn unbox(mut v: glib::Variant) -> glib::Variant {
    while v.is::<glib::Variant>()
        && let Some(inner) = v.as_variant()
    {
        v = inner;
    }
    v
}

/// `node` is a dbusmenu layout entry: (id, properties, children).
fn build_menu(node: &glib::Variant, actions: &gio::SimpleActionGroup, sender: &Sender) -> gio::Menu {
    let menu = gio::Menu::new();
    let mut section = gio::Menu::new();
    let children = child(node, 2).map(|c| c.iter().map(unbox).collect()).unwrap_or_else(Vec::new);

    for child_node in children {
        let child_node = &child_node;
        let (Some(id), Some(props)) = (
            child(child_node, 0).and_then(|v| v.get::<i32>()),
            child(child_node, 1).filter(|p| p.type_().is_subtype_of(glib::VariantTy::VARDICT)),
        ) else {
            continue;
        };
        let props = glib::VariantDict::new(Some(&props));
        let get = |key: &str| props.lookup_value(key, None).map(unbox);
        let string = |key: &str| get(key).and_then(|v| v.get::<String>()).unwrap_or_default();

        if !get("visible").and_then(|v| v.get::<bool>()).unwrap_or(true) {
            continue;
        }
        if string("type") == "separator" {
            if section.n_items() > 0 {
                menu.append_section(None, &section);
                section = gio::Menu::new();
            }
            continue;
        }

        let label = string("label");
        let has_children = child(child_node, 2).is_some_and(|c| c.n_children() > 0);
        if has_children || string("children-display") == "submenu" {
            let submenu = build_menu(child_node, actions, sender);
            section.append_submenu(Some(&label), &submenu);
            continue;
        }

        let name = format!("i{id}");
        let action = match string("toggle-type").as_str() {
            "checkmark" | "radio" => {
                let on = get("toggle-state").and_then(|v| v.get::<i32>()) == Some(1);
                gio::SimpleAction::new_stateful(&name, None, &on.to_variant())
            }
            _ => gio::SimpleAction::new(&name, None),
        };
        action.set_enabled(get("enabled").and_then(|v| v.get::<bool>()).unwrap_or(true));
        let s = sender.clone();
        action.connect_activate(move |_, _| s.clicked(id));
        actions.add_action(&action);
        section.append(Some(&label), Some(&format!("{GROUP}.{name}")));
    }
    if section.n_items() > 0 {
        menu.append_section(None, &section);
    }
    menu
}

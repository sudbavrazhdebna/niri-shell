//! Tray icons of running apps. Left click: the app's own action (or its menu),
//! right click: menu, middle click: secondary action, scroll: passed to the app.

use std::rc::Rc;

use gtk4::glib;
use gtk4::prelude::*;

use super::{Context, LEFT, MIDDLE, RIGHT, on_click, on_scroll};
use crate::services::tray::menu::{self as tray_menu, Entry, Sender};
use crate::services::tray::{Item, Status};
use crate::{menu, ui};

pub fn build(ctx: &Context) -> gtk4::Widget {
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    row.add_css_class("tray");

    ctx.services.tray().items.bind(&row, |row, items| {
        while let Some(child) = row.first_child() {
            row.remove(&child);
        }
        for item in items {
            let widget = item_widget(item);
            row.append(&widget);
            #[cfg(debug_assertions)]
            dev_auto_menu(item, &widget);
        }
        row.set_visible(row.first_child().is_some());
    });
    row.upcast()
}

fn item_widget(item: &Rc<Item>) -> gtk4::Image {
    let image = gtk4::Image::new();
    image.add_css_class("module");
    image.add_css_class("icon");
    image.add_css_class("tray-item");
    let i = item.clone();
    super::tooltips::text(&image, move || Some(i.tooltip_text()));

    item.state.bind(&image, |image, state| {
        image.set_visible(state.status != Status::Passive);
        state.icon.apply(image);
    });

    let i = item.clone();
    menu::register(&image, move |anchor| {
        let (item, anchor, ticket) = (i.clone(), anchor.clone(), menu::ticket());
        glib::spawn_future_local(async move {
            if let Some(image) = anchor.downcast_ref::<gtk4::Image>() {
                show_menu(&item, image, ticket).await;
            }
        });
    });
    menu::on_button(&image, RIGHT);
    let (i, w) = (item.clone(), image.downgrade());
    on_click(&image, LEFT, move || {
        let (item, weak) = (i.clone(), w.clone());
        glib::spawn_future_local(async move {
            let is_menu = item.state.get().item_is_menu;
            // Apps without an action of their own answer Activate with an error.
            if (is_menu || item.activate().await.is_err())
                && let Some(image) = weak.upgrade()
            {
                crate::menu::toggle(image.upcast_ref());
            }
        });
    });
    let i = item.clone();
    on_click(&image, MIDDLE, move || i.secondary_activate());
    let i = item.clone();
    on_scroll(&image, move |steps| i.scroll(steps));
    image
}

async fn show_menu(item: &Item, anchor: &gtk4::Image, ticket: u64) -> Option<()> {
    let path = item.state.get().menu.clone()?;
    let (entries, sender) = tray_menu::load(item.connection(), &item.bus, &path).await?;
    let stack = gtk4::Stack::new();
    stack.set_transition_type(gtk4::StackTransitionType::SlideLeftRight);
    stack.set_vhomogeneous(false);
    stack.set_interpolate_size(true);
    add_page(&stack, "root", None, &entries, &sender);
    // Submenu pages are added before their parent; start at the top.
    stack.set_visible_child_name("root");
    menu::show_for(ticket, anchor.upcast_ref(), &stack);
    Some(())
}

/// One level of the menu; submenus become further pages with a back row.
fn add_page(
    stack: &gtk4::Stack,
    name: &str,
    back: Option<(&str, &str)>,
    entries: &[Entry],
    sender: &Sender,
) {
    let column = ui::column();
    // Reserve one column for every row on a page that contains checkbox/radio items.
    let reserve_check = entries.iter().any(|entry| matches!(entry, Entry::Item { toggle: Some(_), .. }));
    let row = |text: &str, checked: Option<bool>, action: Box<dyn Fn()>| {
        if reserve_check { ui::item(text, checked.unwrap_or(false), action) } else { ui::action_item(text, action) }
    };
    if let Some((parent, title)) = back {
        let (stack_weak, parent) = (stack.downgrade(), parent.to_string());
        let row = row(&format!("‹  {title}"), None, Box::new(move || {
            if let Some(stack) = stack_weak.upgrade() {
                stack.set_visible_child_name(&parent);
            }
        }));
        row.add_css_class("menu-back");
        column.append(&row);
        column.append(&ui::separator());
    }
    for (i, entry) in entries.iter().enumerate() {
        match entry {
            Entry::Separator => column.append(&ui::separator()),
            Entry::Item { id, label, enabled, toggle } => {
                let (sender, id) = (sender.clone(), *id);
                let action = move || {
                    sender.clicked(id);
                    menu::close();
                };
                let row = row(label, *toggle, Box::new(action));
                row.set_sensitive(*enabled);
                column.append(&row);
            }
            Entry::Submenu { label, enabled, children } => {
                let page = format!("{name}/{i}");
                add_page(stack, &page, Some((name, label)), children, sender);
                let stack_weak = stack.downgrade();
                let row = row(&format!("{label}  ›"), None, Box::new(move || {
                    if let Some(stack) = stack_weak.upgrade() {
                        stack.set_visible_child_name(&page);
                    }
                }));
                row.set_sensitive(*enabled);
                column.append(&row);
            }
        }
    }
    stack.add_named(&column, Some(name));
}

/// Sandbox testing without a mouse: with NSHELL_DEV_TRAY_MENU set, a debug build opens
/// the menu of each new tray item 3 s after it appears (apps often map a window first).
#[cfg(debug_assertions)]
fn dev_auto_menu(item: &Rc<Item>, image: &gtk4::Image) {
    if std::env::var_os("NSHELL_DEV_TRAY_MENU").is_none() {
        return;
    }
    let (item, image) = (item.clone(), image.clone());
    glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
        glib::spawn_future_local(async move {
            show_menu(&item, &image, menu::ticket()).await;
        });
    });
}

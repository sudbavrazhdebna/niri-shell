//! Tray icons of running apps. Left click: the app's own action (or its menu),
//! right click: menu, middle click: secondary action, scroll: passed to the app.

use std::rc::Rc;

use gtk4::glib;
use gtk4::prelude::*;

use super::{Context, LEFT, MIDDLE, RIGHT, on_click, on_scroll};
use crate::services::tray::{Item, Status, menu};

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

    item.state.bind(&image, |image, state| {
        image.set_visible(state.status != Status::Passive);
        state.icon.apply(image);
        image.set_tooltip_text(Some(&state.title).filter(|t| !t.is_empty()).map(|t| t.as_str()));
    });

    let (i, w) = (item.clone(), image.downgrade());
    on_click(&image, LEFT, move || {
        let (item, weak) = (i.clone(), w.clone());
        glib::spawn_future_local(async move {
            let is_menu = item.state.get().item_is_menu;
            // Apps without an action of their own answer Activate with an error.
            if (is_menu || item.activate().await.is_err())
                && let Some(image) = weak.upgrade()
            {
                show_menu(&item, &image).await;
            }
        });
    });
    let (i, w) = (item.clone(), image.downgrade());
    on_click(&image, RIGHT, move || {
        let (item, weak) = (i.clone(), w.clone());
        glib::spawn_future_local(async move {
            if let Some(image) = weak.upgrade() {
                show_menu(&item, &image).await;
            }
        });
    });
    let i = item.clone();
    on_click(&image, MIDDLE, move || i.secondary_activate());
    let i = item.clone();
    on_scroll(&image, move |steps| i.scroll(steps));
    image
}

async fn show_menu(item: &Item, anchor: &gtk4::Image) -> Option<gtk4::PopoverMenu> {
    let path = item.state.get().menu.clone()?;
    let (model, actions) = menu::load(item.connection(), &item.bus, &path).await?;
    let popover = gtk4::PopoverMenu::from_model(Some(&model));
    popover.add_css_class("tray-menu");
    popover.set_has_arrow(false);
    popover.insert_action_group(menu::GROUP, Some(&actions));
    popover.set_parent(anchor);
    popover.connect_closed(|popover| {
        // Unparent after the click that closed it has been handled.
        let popover = popover.clone();
        glib::idle_add_local_once(move || popover.unparent());
    });
    popover.popup();
    Some(popover)
}

/// Sandbox testing without a mouse: with NSHELL_DEV_TRAY_MENU=<n> (or "open") set, a debug build
/// opens the menu of each new tray item 3 s after it appears (apps often map a window
/// first, which closes popups) and clicks its item with id n two seconds later.
#[cfg(debug_assertions)]
fn dev_auto_menu(item: &Rc<Item>, image: &gtk4::Image) {
    let Some(click) = std::env::var("NSHELL_DEV_TRAY_MENU").ok() else {
        return;
    };
    let (item, image) = (item.clone(), image.clone());
    glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
        glib::spawn_future_local(async move {
            let Some(popover) = show_menu(&item, &image).await else {
                return;
            };
            if click == "open" {
                return;
            }
            glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || {
                let _ = popover.activate_action(&format!("{}.i{click}", menu::GROUP), None);
                popover.popdown();
            });
        });
    });
}

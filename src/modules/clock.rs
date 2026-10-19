//! Clock, redrawn on the ticks of the shared `services::clock`. A dot next to it means
//! unseen notifications, a bell-off icon Do not disturb. Left click: calendar and the
//! notification center.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use gtk4::glib;
use gtk4::prelude::*;

use super::Context;
use crate::notification_card::card;
use crate::services::Services;
use crate::services::notifications::Notification;
use crate::{menu, ui};


pub fn build(ctx: &Context) -> gtk4::Widget {
    let root = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    root.add_css_class("module");
    root.add_css_class("clock");
    let label = gtk4::Label::new(None);
    let dnd = gtk4::Image::from_icon_name("notifications-disabled-symbolic");
    dnd.add_css_class("clock-dnd");
    let dot = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    dot.add_css_class("clock-dot");
    dot.set_valign(gtk4::Align::Center);
    root.append(&label);
    root.append(&dnd);
    root.append(&dot);

    let format = ctx.config.clock.format.clone();
    let clock = ctx.services.clock();
    clock.set_per_second(format.contains("%S") || format.contains("%T") || format.contains("%r"));
    clock.tick.bind(&label, move |label, _| {
        if let Some(text) = glib::DateTime::now_local().ok().and_then(|now| now.format(&format).ok()) {
            label.set_label(&text);
        }
    });

    let notifications = ctx.services.notifications();
    notifications.dnd.bind(&dnd, |dnd, on| dnd.set_visible(*on));
    notifications.unseen.bind(&dot, |dot, unseen| dot.set_visible(*unseen));

    let services = ctx.services.clone();
    menu::attach(&root, move |anchor| {
        services.notifications().mark_seen();
        menu::show(anchor, &menu_content(&services));
    });
    root.upcast()
}

fn menu_content(services: &Rc<Services>) -> gtk4::Box {
    let column = ui::column();
    column.set_width_request(420);
    let today = glib::DateTime::now_local()
        .ok()
        .and_then(|now| now.format("%A, %e %B").ok())
        .map(String::from)
        .unwrap_or_default();
    column.append(&ui::title(today.trim()));
    let calendar = gtk4::Calendar::new();
    calendar.add_css_class("menu-calendar");
    column.append(&calendar);
    column.append(&ui::separator());
    column.append(&center(services));
    column
}

/// The notification center: header with Do not disturb, the list in per-app stacks.
fn center(services: &Rc<Services>) -> gtk4::Box {
    let server = services.notifications().clone();
    let center = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    center.add_css_class("notification-center");

    let s = server.clone();
    let (header, set_dnd) = ui::header_switch("Do not disturb", move |on| s.set_dnd(on));
    server.dnd.bind(&header, move |_, on| set_dnd(*on));
    center.append(&header);

    let list = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    // Scrolls by itself past the first VISIBLE notifications, so the menu stays short.
    let scroll = gtk4::ScrolledWindow::new();
    scroll.add_css_class("notification-list");
    scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroll.set_propagate_natural_height(true);
    scroll.set_child(Some(&list));
    center.append(&scroll);

    let clear = gtk4::Button::with_label("Clear all");
    clear.add_css_class("clear-all");
    clear.set_halign(gtk4::Align::End);
    let s = server.clone();
    clear.connect_clicked(move |_| s.clear_all());
    center.append(&clear);

    // Apps whose stack the user opened, kept while the menu is open.
    let expanded: Rc<RefCell<HashSet<String>>> = Rc::default();
    let render = Rc::new(Render {
        list: list.downgrade(),
        scroll: scroll.downgrade(),
        clear: clear.downgrade(),
        expanded,
        server: server.clone(),
    });
    let r = render.clone();
    server.list.bind(&list, move |_, _| r.run());
    center
}

/// Notifications the center shows before its list scrolls.
const VISIBLE: usize = 4;

/// Redraws the notification list (on changes, and when a stack is opened).
struct Render {
    list: glib::WeakRef<gtk4::Box>,
    scroll: glib::WeakRef<gtk4::ScrolledWindow>,
    clear: glib::WeakRef<gtk4::Button>,
    expanded: Rc<RefCell<HashSet<String>>>,
    server: Rc<crate::services::notifications::Notifications>,
}

impl Render {
    fn run(self: &Rc<Self>) {
        let Some(list) = self.list.upgrade() else { return };
        let notifications = self.server.list.get().clone();
        ui::clear(&list);
        if let Some(clear) = self.clear.upgrade() {
            clear.set_visible(!notifications.is_empty());
        }
        if notifications.is_empty() {
            list.append(&ui::note("No notifications"));
            self.fit(&list);
            return;
        }
        for group in stacks(&notifications) {
            let app = group[0].app_name.clone();
            let open = group.len() == 1 || self.expanded.borrow().contains(&app);
            let shown = if open { &group[..] } else { &group[..1] };
            for n in shown {
                list.append(&card(n, &self.server, Rc::new(menu::close), true));
            }
            if !open {
                let label = format!("+{} more from {}", group.len() - 1, app_label(&group[0]));
                let more = gtk4::Button::with_label(&label);
                more.add_css_class("stack-more");
                let this = Rc::downgrade(self);
                more.connect_clicked(move |_| {
                    if let Some(this) = this.upgrade() {
                        this.expanded.borrow_mut().insert(app.clone());
                        this.run();
                    }
                });
                list.append(&more);
            }
        }
        self.fit(&list);
    }

    /// Caps the list at its first VISIBLE notifications (with a "more" row after the
    /// last of them); the rest scrolls inside it. Measured at the list's own width,
    /// since wrapped bodies make cards of different heights.
    fn fit(&self, list: &gtk4::Box) {
        let Some(scroll) = self.scroll.upgrade() else { return };
        let width = if list.width() > 0 {
            list.width()
        } else {
            list.measure(gtk4::Orientation::Horizontal, -1).1
        };
        let (mut height, mut cards) = (0, 0);
        let mut child = list.first_child();
        while let Some(widget) = &child {
            if widget.has_css_class("notification") {
                if cards == VISIBLE {
                    break;
                }
                cards += 1;
            }
            height += widget.measure(gtk4::Orientation::Vertical, width).1;
            child = widget.next_sibling();
        }
        // No cap when everything fits: a cap equal to the content can still round into
        // a scrollbar.
        scroll.set_max_content_height(if child.is_some() { height } else { -1 });
    }
}


/// Notifications grouped by app, groups ordered by their newest one.
fn stacks(notifications: &[Rc<Notification>]) -> Vec<Vec<Rc<Notification>>> {
    let mut groups: Vec<Vec<Rc<Notification>>> = Vec::new();
    for n in notifications {
        match groups.iter_mut().find(|g| g[0].app_name == n.app_name) {
            Some(group) => group.push(n.clone()),
            None => groups.push(vec![n.clone()]),
        }
    }
    groups
}

fn app_label(n: &Notification) -> &str {
    if n.app_name.is_empty() { "this app" } else { &n.app_name }
}

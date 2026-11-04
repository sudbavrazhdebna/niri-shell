//! Clock, redrawn on the ticks of the shared `services::clock`. A dot next to it means
//! unseen notifications, a bell-off icon Do not disturb. Left click: calendar and the
//! notification center.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use gtk4::glib;
use gtk4::prelude::*;

use super::Context;
use crate::notification_card::{app_icon, card, card_with_click};
use crate::services::Services;
use crate::services::notifications::{Image, Notification};
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
    column.set_width_request(380);
    let today = glib::DateTime::now_local()
        .ok()
        .and_then(|now| now.format("%A, %-e %B").ok())
        .map(String::from)
        .unwrap_or_default();
    column.append(&ui::title(today.trim()));
    let calendar = gtk4::Calendar::new();
    calendar.add_css_class("menu-calendar");
    center_days(&calendar);
    // A click picks nothing, so it doesn't take the focus (and its frame) either.
    calendar.set_focus_on_click(false);
    column.append(&calendar);
    column.append(&ui::separator());
    column.append(&center(services));
    column
}

/// GTK's day cells fill their column (wide rectangles); each sits in the middle of it
/// instead, so the marks of style.css stay square.
fn center_days(calendar: &gtk4::Calendar) {
    let mut stack: Vec<gtk4::Widget> = vec![calendar.clone().upcast()];
    while let Some(widget) = stack.pop() {
        if widget.has_css_class("day-number") {
            widget.set_halign(gtk4::Align::Center);
        }
        let mut child = widget.first_child();
        while let Some(c) = child {
            child = c.next_sibling();
            stack.push(c);
        }
    }
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
    // Scrolls by itself past LIST_HEIGHT, so the menu stays short.
    let scroll = gtk4::ScrolledWindow::new();
    scroll.add_css_class("notification-list");
    // Scrolls without a scrollbar: GTK's floating one covered the cards' right edge, and
    // the cut-off card at the bottom already says there is more.
    scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::External);
    scroll.set_propagate_natural_height(true);
    scroll.set_child(Some(&list));
    // A clicked button (a stack's arrow) goes with the old list; the focus then moves
    // to the top, and following it scrolled the list to the top.
    if let Some(viewport) = scroll.child().and_downcast::<gtk4::Viewport>() {
        viewport.set_scroll_to_focus(false);
    }
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
        restoring: Rc::default(),
    });
    let r = render.clone();
    // A change of the list (one closed, one came) keeps the list's height: see `run`.
    server.list.bind(&list, move |_, _| r.run(true));
    center
}

/// The notification list's height once it scrolls, in px: about four cards and the top
/// of a fifth, which peeks out to say there's more (there is no scrollbar).
const LIST_HEIGHT: i32 = 340;

/// Redraws the notification list (on changes, and when a stack is opened or folded).
struct Render {
    list: glib::WeakRef<gtk4::Box>,
    scroll: glib::WeakRef<gtk4::ScrolledWindow>,
    clear: glib::WeakRef<gtk4::Button>,
    expanded: Rc<RefCell<HashSet<String>>>,
    server: Rc<crate::services::notifications::Notifications>,
    /// The waiting scroll restore, if the layout hasn't come yet.
    restoring: Rc<RefCell<Option<glib::SignalHandlerId>>>,
}

impl Render {
    /// `hold`: while the menu is open, a closed notification leaves its room empty
    /// instead of the menu shrinking (it jumped with every one closed), and the next one
    /// moves up under the pointer. Folding a stack or clearing all does shrink it.
    fn run(self: &Rc<Self>, hold: bool) {
        let Some(list) = self.list.upgrade() else { return };
        let notifications = self.server.list.get().clone();
        let held = match self.scroll.upgrade() {
            Some(scroll) if hold && !notifications.is_empty() => scroll.vadjustment().page_size() as i32,
            _ => 0,
        };
        // Rebuilt, the list keeps where it was scrolled to (opening a stack at the bottom
        // jumped to the top).
        let scrolled = self.scroll.upgrade().map(|s| s.vadjustment().value()).unwrap_or(0.0);
        ui::clear(&list);
        if let Some(clear) = self.clear.upgrade() {
            clear.set_visible(!notifications.is_empty());
        }
        if notifications.is_empty() {
            list.append(&ui::note("No notifications"));
            self.fit(&list, 0);
            return;
        }
        for group in stacks(&notifications) {
            let app = group[0].app_name.clone();
            if group.len() == 1 {
                list.append(&card(&group[0], &self.server, Rc::new(menu::close), true));
                continue;
            }
            let open = self.expanded.borrow().contains(&app);
            list.append(&self.stack_header(&group[0], &app, open));
            if open {
                for n in &group {
                    list.append(&card(n, &self.server, Rc::new(menu::close), true));
                }
            } else {
                list.append(&self.stack(&group, &app));
            }
        }
        self.fit(&list, held);
        self.restore_scroll(scrolled);
    }

    /// Scrolls back to `value` once GTK has laid out the new list (until then the old
    /// height limits it).
    fn restore_scroll(&self, value: f64) {
        let Some(scroll) = self.scroll.upgrade() else { return };
        let adjustment = scroll.vadjustment();
        if let Some(id) = self.restoring.take() {
            adjustment.disconnect(id);
        }
        if value <= 0.0 {
            return;
        }
        adjustment.set_value(value);
        let h = self.restoring.clone();
        let id = adjustment.connect_changed(move |adjustment| {
            adjustment.set_value(value);
            if let Some(id) = h.take() {
                adjustment.disconnect(id);
            }
        });
        self.restoring.replace(Some(id));
    }

    /// A closed stack, like iOS: the newest card in front, up to two older ones behind
    /// it, each lower, narrower and darker. A click anywhere on it opens the stack.
    fn stack(self: &Rc<Self>, group: &[Rc<Notification>], app: &str) -> gtk4::Overlay {
        let depth = group.len().min(3) - 1;
        let open: Rc<dyn Fn()> = {
            let (this, app) = (Rc::downgrade(self), app.to_string());
            Rc::new(move || {
                if let Some(this) = this.upgrade() {
                    this.set_open(&app, true);
                }
            })
        };
        // Drawn in order: the farthest sheet first, the card last, on top.
        let sheet = |d: usize| {
            let sheet = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
            sheet.add_css_class("stack-sheet");
            sheet.add_css_class(&format!("depth-{d}"));
            sheet.set_valign(gtk4::Align::End);
            let click = gtk4::GestureClick::new();
            let open = open.clone();
            click.connect_released(move |_, _, _, _| open());
            sheet.add_controller(click);
            sheet
        };
        let stack = gtk4::Overlay::new();
        stack.add_css_class("notification-stack");
        stack.set_child(Some(&sheet(depth)));
        if depth == 2 {
            stack.add_css_class("deep");
            stack.add_overlay(&sheet(1));
        }
        let top = card_with_click(&group[0], &self.server, Rc::new(menu::close), true, Some(open));
        top.add_css_class("stacked");
        top.add_css_class(&format!("depth-{depth}"));
        top.set_valign(gtk4::Align::Start);
        top.set_tooltip_text(Some(&format!("{} more from {}", group.len() - 1, app_label(&group[0]))));
        stack.add_overlay(&top);
        stack.set_measure_overlay(&top, true);
        stack
    }

    /// Above every stack of two or more, as in GNOME: the app's icon and name, and a
    /// round button that opens or folds the stack. Always there, so opening one doesn't
    /// move what's above it.
    fn stack_header(self: &Rc<Self>, newest: &Notification, app: &str, open: bool) -> gtk4::Box {
        let header = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        header.add_css_class("stack-header");
        let icon = gtk4::Image::new();
        icon.add_css_class("stack-app-icon");
        match (app_icon(newest), &newest.image) {
            (Some(gicon), _) => icon.set_from_gicon(&gicon),
            // No desktop entry: the icon the app named for its notification.
            (None, Some(Image::Name(name))) => icon.set_icon_name(Some(name)),
            (None, _) => icon.set_icon_name(Some("dialog-information-symbolic")),
        }
        header.append(&icon);
        let name = gtk4::Label::new(Some(app_label(newest)));
        name.add_css_class("stack-app-name");
        name.set_xalign(0.0);
        name.set_hexpand(true);
        name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        header.append(&name);
        let fold = gtk4::Button::from_icon_name(if open { "pan-up-symbolic" } else { "pan-down-symbolic" });
        fold.add_css_class("stack-fold");
        fold.set_tooltip_text(Some(if open { "Show less" } else { "Show all" }));
        let (this, app) = (Rc::downgrade(self), app.to_string());
        fold.connect_clicked(move |_| {
            if let Some(this) = this.upgrade() {
                this.set_open(&app, !open);
            }
        });
        header.append(&fold);
        header
    }

    fn set_open(self: &Rc<Self>, app: &str, open: bool) {
        if open {
            self.expanded.borrow_mut().insert(app.to_string());
        } else {
            self.expanded.borrow_mut().remove(app);
        }
        self.run(false);
    }

    /// Caps the list at LIST_HEIGHT; the rest scrolls inside it. A fixed height, not
    /// "the first few cards": cards, closed stacks and stack headers differ in height, so
    /// opening a stack changed the menu's size. Measured at the list's own width, since
    /// wrapped bodies make cards of different heights.
    /// `held`: the height to keep at least (see `run`), 0 for none.
    fn fit(&self, list: &gtk4::Box, held: i32) {
        let Some(scroll) = self.scroll.upgrade() else { return };
        let width = if list.width() > 0 {
            list.width()
        } else {
            list.measure(gtk4::Orientation::Horizontal, -1).1
        };
        // Only as far as needed: with up to 100 in history, measuring all of them (wrapped
        // bodies) on every change was wasted.
        let mut height = 0;
        let mut child = list.first_child();
        while let Some(widget) = child
            && height <= LIST_HEIGHT
        {
            height += widget.measure(gtk4::Orientation::Vertical, width).1;
            child = widget.next_sibling();
        }
        // No cap when everything fits: a cap equal to the content can still round into
        // a scroll.
        let cap = if height > LIST_HEIGHT { LIST_HEIGHT } else { -1 };
        // The minimum must never exceed the maximum: drop it before changing either.
        scroll.set_min_content_height(-1);
        scroll.set_max_content_height(if cap >= 0 { cap.max(held) } else { -1 });
        if held > 0 {
            scroll.set_min_content_height(held);
        }
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

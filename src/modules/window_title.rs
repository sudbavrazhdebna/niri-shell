//! Title of the window shown on this bar's monitor, with the app's color icon.
//! Hidden when the monitor shows no window.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk4::glib;
use gtk4::prelude::*;

use niri_ipc::Action;

use super::Context;
use crate::niri::Niri;
use crate::{menu, ui};

/// Terminal titles animate (spinners, progress) many times a second, and every change
/// redraws the bar. Title-only changes are shown at most this often; switching windows
/// is always instant.
const TITLE_INTERVAL: Duration = Duration::from_millis(250);

/// Title and app id on screen.
type Shown = Option<(String, String)>;

struct View {
    /// Weak: the bar owns the widgets; when it closes, the module stops listening.
    root: glib::WeakRef<gtk4::Box>,
    icon: gtk4::Image,
    label: gtk4::Label,
    shown: RefCell<Shown>,
    /// Newest value waiting for the throttle timer.
    pending: RefCell<Option<Shown>>,
    last_update: Cell<Option<Instant>>,
}

pub fn build(ctx: &Context) -> gtk4::Widget {
    let root = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    root.add_css_class("module");
    root.add_css_class("window-title");
    root.set_visible(false);

    let icon = gtk4::Image::new();
    icon.add_css_class("app-icon");
    let label = gtk4::Label::new(None);
    label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    label.set_max_width_chars(ctx.config.window_title.max_length);
    root.append(&icon);
    root.append(&label);

    let view = Rc::new(View {
        root: root.downgrade(),
        icon,
        label,
        shown: RefCell::new(None),
        pending: RefCell::new(None),
        last_update: Cell::new(None),
    });

    let (niri, output) = (ctx.services.niri.clone(), ctx.output.clone());
    menu::attach(&root, move |anchor| {
        let window = niri.state.get().active_window_on(&output).cloned();
        if let Some(window) = window {
            menu::show(anchor, &window_menu(&window, &niri));
        }
    });

    let config = ctx.config.clone();
    let output = ctx.output.clone();
    ctx.services.niri.state.subscribe(move |state| {
        if view.root.upgrade().is_none() {
            return false;
        }
        let wanted = state.active_window_on(&output).map(|window| {
            let title = window.title.as_deref().unwrap_or_default();
            let title = config
                .window_title
                .rewrite
                .iter()
                .find(|(re, _)| re.is_match(title))
                .map_or_else(
                    || title.to_string(),
                    |(re, replace)| re.replace(title, replace.as_str()).into_owned(),
                );
            (title, window.app_id.clone().unwrap_or_default())
        });
        view.request(wanted);
        true
    });

    root.upcast()
}

impl View {
    fn request(self: &Rc<Self>, wanted: Shown) {
        if *self.shown.borrow() == wanted {
            self.pending.replace(None);
            return;
        }

        let same_app = match (&*self.shown.borrow(), &wanted) {
            (Some((_, a)), Some((_, b))) => a == b,
            _ => false,
        };
        let wait = self
            .last_update
            .get()
            .map(|at| TITLE_INTERVAL.saturating_sub(at.elapsed()))
            .unwrap_or_default();

        if !same_app || wait.is_zero() {
            self.pending.replace(None);
            self.show(wanted);
            return;
        }

        // A timer is already waiting: it will pick up the newest value.
        if self.pending.replace(Some(wanted)).is_some() {
            return;
        }
        let this = Rc::downgrade(self);
        glib::timeout_add_local_once(wait, move || {
            if let Some(this) = this.upgrade()
                && let Some(wanted) = this.pending.take()
            {
                this.show(wanted);
            }
        });
    }

    fn show(&self, wanted: Shown) {
        let Some(root) = self.root.upgrade() else {
            return;
        };
        self.last_update.set(Some(Instant::now()));
        match &wanted {
            Some((title, app_id)) => {
                self.label.set_label(title);
                let app_changed = self
                    .shown
                    .borrow()
                    .as_ref()
                    .is_none_or(|(_, prev)| prev != app_id);
                if app_changed {
                    match crate::apps::icon(app_id) {
                        Some(gicon) => self.icon.set_from_gicon(&gicon),
                        None => self.icon.clear(),
                    }
                    self.icon
                        .set_visible(self.icon.storage_type() != gtk4::ImageType::Empty);
                }
                root.set_visible(true);
            }
            None => root.set_visible(false),
        }
        self.shown.replace(wanted);
    }
}

/// The app's own actions (from its desktop entry), then what niri can do with the window.
fn window_menu(window: &niri_ipc::Window, niri: &Niri) -> gtk4::Box {
    let column = ui::column();
    let app_id = window.app_id.clone().unwrap_or_default();
    let entry = crate::apps::entry(&app_id);
    let name = entry.as_ref().map_or_else(|| app_id.clone(), |e| e.name().to_string());
    column.append(&ui::title(&name));
    if let Some(entry) = &entry {
        for action in entry.list_actions() {
            let label = entry.action_name(&action);
            let (entry, action) = (entry.clone(), action.to_string());
            column.append(&ui::item(&label, false, move || {
                let context = gtk4::gdk::Display::default().map(|d| d.app_launch_context());
                entry.launch_action(&action, context.as_ref());
                menu::close();
            }));
        }
        if !entry.list_actions().is_empty() {
            column.append(&ui::separator());
        }
    }

    let id = window.id;
    let action = |label: &str, checked: bool, make: fn(u64) -> Action| {
        let niri = niri.clone();
        ui::item(label, checked, move || {
            niri.action(make(id));
            menu::close();
        })
    };
    column.append(&action("Floating", window.is_floating, |id| Action::ToggleWindowFloating { id: Some(id) }));
    column.append(&action("Maximize", false, |id| Action::MaximizeWindowToEdges { id: Some(id) }));
    column.append(&action("Fullscreen", false, |id| Action::FullscreenWindow { id: Some(id) }));
    column.append(&ui::separator());
    column.append(&action("Close window", false, |id| Action::CloseWindow { id: Some(id) }));
    column
}

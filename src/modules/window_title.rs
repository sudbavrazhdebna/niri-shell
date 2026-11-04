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
            let title = without_spinner(window.title.as_deref().unwrap_or_default());
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
    column.append(&ui::separator());
    if let Some(entry) = &entry {
        for action in entry.list_actions() {
            let label = entry.action_name(&action);
            let (entry, action) = (entry.clone(), action.to_string());
            column.append(&ui::plain_item(&label, move || {
                crate::launch::action(&entry, &action);
                menu::close();
            }));
        }
        if !entry.list_actions().is_empty() {
            column.append(&ui::separator());
        }
    }

    // No check mark column: only Floating can be on, and a column for it left a gap
    // before every row (the app's actions too). Its check sits at the end instead.
    let id = window.id;
    let action = |label: &str, checked: bool, make: fn(u64) -> Action| {
        let niri = niri.clone();
        let item = ui::plain_item(label, move || {
            niri.action(make(id));
            menu::close();
        });
        if checked {
            ui::trailing_icon(&item, "object-select-symbolic");
        }
        item
    };
    column.append(&action("Floating", window.is_floating, |id| Action::ToggleWindowFloating { id: Some(id) }));
    column.append(&action("Maximize", false, |id| Action::MaximizeWindowToEdges { id: Some(id) }));
    column.append(&action("Fullscreen", false, |id| Action::FullscreenWindow { id: Some(id) }));
    column.append(&ui::separator());
    column.append(&action("Close window", false, |id| Action::CloseWindow { id: Some(id) }));
    column
}

/// The title without a leading activity spinner ("◐ Building" → "Building"). Terminal
/// tools animate one many times a second, and every frame of it redrew the bar (about
/// 0.2% CPU per change a second); the text after it is what matters. Only spinner glyphs
/// go, so a status emoji like "🔴 Recording" stays.
fn without_spinner(title: &str) -> &str {
    let mut chars = title.char_indices();
    let is_spinner = |c: char| {
        // Braille dots (⠋⠙⠹…), quarter circles, the stars and dots some CLIs spin.
        ('\u{2800}'..='\u{28FF}').contains(&c) || "◐◓◑◒◴◵◶◷✳✢✶✻✽·⏳⌛".contains(c)
    };
    match (chars.next(), chars.next()) {
        (Some((_, c)), Some((i, space))) if is_spinner(c) && space.is_whitespace() => title[i..].trim_start(),
        _ => title,
    }
}

#[cfg(test)]
mod tests {
    use super::without_spinner;

    #[test]
    fn spinners_go_emoji_stay() {
        assert_eq!(without_spinner("◐ Код nshell"), "Код nshell");
        assert_eq!(without_spinner("⠹ cargo build"), "cargo build");
        assert_eq!(without_spinner("✳ Claude Code"), "Claude Code");
        assert_eq!(without_spinner("🔴 Recording"), "🔴 Recording");
        assert_eq!(without_spinner("·"), "·");
        assert_eq!(without_spinner("Firefox"), "Firefox");
        assert_eq!(without_spinner(""), "");
    }
}

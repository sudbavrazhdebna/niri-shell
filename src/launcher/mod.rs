//! The launcher (`nshell msg launcher`, bound to a key in niri): a search field in the
//! middle of the focused monitor. Apps start, or come to the front when already running
//! (Ctrl+Enter starts another one); files under the configured folders open in their
//! app. Arrows move, Enter opens, Escape or a click beside it closes.

mod search;

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use niri_ipc::Action;

use crate::services::Services;
use search::{App, FileHit, History};

/// Results shown at most: apps, then files.
const MAX_APPS: usize = 8;
const MAX_FILES: usize = 6;
/// Corner radius of `.launcher-card` in style.css, for the blur shape.
const CARD_RADIUS: i32 = 16;

thread_local! {
    static OPEN: RefCell<Option<Rc<Launcher>>> = const { RefCell::new(None) };
    static SEARCH_PATHS: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
}

/// The folders whose files the launcher finds (at start and on config reload).
pub fn configure(paths: &[PathBuf]) {
    SEARCH_PATHS.set(paths.to_vec());
}

/// Opens the launcher, or closes it if it is open.
pub fn toggle(services: &Rc<Services>) {
    if OPEN.with_borrow(Option::is_some) {
        close();
    } else {
        open(services);
    }
}

pub fn close() {
    // Out of the cell first: destroying the window runs handlers that look at it.
    if let Some(launcher) = OPEN.take() {
        launcher.window.destroy();
    }
}

enum Hit {
    App(Rc<App>),
    File(PathBuf),
}

struct Launcher {
    services: Rc<Services>,
    window: gtk4::Window,
    entry: gtk4::Entry,
    list: gtk4::Box,
    scroll: gtk4::ScrolledWindow,
    apps: Vec<Rc<App>>,
    history: RefCell<History>,
    /// Desktop id → the app's windows, most recently focused first.
    running: Vec<(String, Vec<u64>)>,
    hits: RefCell<Vec<(Hit, gtk4::Widget)>>,
    selected: Cell<usize>,
}

fn open(services: &Rc<Services>) {
    let Some(monitor) = services.niri.focused_monitor() else { return };
    // The menus close: the launcher takes the keyboard.
    crate::menu::close();

    let window = gtk4::Window::new();
    window.add_css_class("nshell-launcher");
    window.init_layer_shell();
    window.set_namespace(Some("nshell-launcher"));
    window.set_layer(Layer::Overlay);
    window.set_monitor(Some(&monitor));
    // Typing goes here while it is open.
    window.set_keyboard_mode(KeyboardMode::Exclusive);
    for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        window.set_anchor(edge, true);
    }
    // The whole screen, so that a click beside the card closes it.
    window.set_exclusive_zone(-1);

    let card = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    card.add_css_class("launcher-card");
    card.set_halign(gtk4::Align::Center);
    card.set_valign(gtk4::Align::Start);
    card.set_margin_top(monitor.geometry().height() / 5);
    let entry = gtk4::Entry::new();
    entry.add_css_class("launcher-entry");
    entry.set_placeholder_text(Some("Search apps and files"));
    entry.set_primary_icon_name(Some("system-search-symbolic"));
    card.append(&entry);
    let list = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    list.add_css_class("launcher-list");
    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroll.set_propagate_natural_height(true);
    scroll.set_max_content_height(460);
    scroll.set_child(Some(&list));
    card.append(&scroll);

    let fill = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    fill.set_hexpand(true);
    fill.set_vexpand(true);
    fill.append(&card);
    let card_weak = card.downgrade();
    let wrapped = crate::blur::behind(
        &window,
        &fill,
        move || card_weak.upgrade().map(|c| c.upcast()).into_iter().collect(),
        CARD_RADIUS,
        || {},
    );
    window.set_child(Some(&wrapped));

    let launcher = Rc::new(Launcher {
        services: services.clone(),
        window: window.clone(),
        entry: entry.clone(),
        list,
        scroll,
        apps: search::apps(),
        history: RefCell::new(History::load()),
        running: running_apps(services),
        hits: RefCell::default(),
        selected: Cell::new(0),
    });

    let weak = Rc::downgrade(&launcher);
    entry.connect_changed(move |_| {
        if let Some(launcher) = weak.upgrade() {
            launcher.refresh();
        }
    });
    let weak = Rc::downgrade(&launcher);
    entry.connect_activate(move |_| {
        if let Some(launcher) = weak.upgrade() {
            launcher.activate(false);
        }
    });

    let keys = gtk4::EventControllerKey::new();
    keys.set_propagation_phase(gtk4::PropagationPhase::Capture);
    let weak = Rc::downgrade(&launcher);
    keys.connect_key_pressed(move |_, key, _, modifiers| {
        let Some(launcher) = weak.upgrade() else { return glib::Propagation::Proceed };
        match key {
            gdk::Key::Escape => close(),
            gdk::Key::Down | gdk::Key::Tab => launcher.select_by(1),
            gdk::Key::Up | gdk::Key::ISO_Left_Tab => launcher.select_by(-1),
            gdk::Key::Return | gdk::Key::KP_Enter => {
                launcher.activate(modifiers.contains(gdk::ModifierType::CONTROL_MASK));
            }
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    });
    window.add_controller(keys);

    // A click beside the card closes it.
    let click = gtk4::GestureClick::new();
    let card_weak = card.downgrade();
    click.connect_pressed(move |gesture, _, x, y| {
        let (Some(card), Some(widget)) = (card_weak.upgrade(), gesture.widget()) else { return };
        let inside = widget
            .compute_point(&card, &gtk4::graphene::Point::new(x as f32, y as f32))
            .is_some_and(|p| card.contains(f64::from(p.x()), f64::from(p.y())));
        if !inside {
            close();
        }
    });
    window.add_controller(click);

    OPEN.replace(Some(launcher.clone()));
    launcher.refresh();
    window.set_visible(true);
    entry.grab_focus();

    #[cfg(debug_assertions)]
    if let Ok(query) = std::env::var("NSHELL_DEV_LAUNCHER") {
        entry.set_text(&query);
    }
}

/// Which apps have windows, by desktop id, most recently focused window first.
fn running_apps(services: &Rc<Services>) -> Vec<(String, Vec<u64>)> {
    let state = services.niri.state.get();
    let mut windows: Vec<&niri_ipc::Window> = state.windows.values().collect();
    windows.sort_by_key(|w| std::cmp::Reverse(w.focus_timestamp.map(|t| (t.secs, t.nanos))));
    let mut running: Vec<(String, Vec<u64>)> = Vec::new();
    for window in windows {
        let Some(id) = window
            .app_id
            .as_deref()
            .and_then(crate::apps::entry)
            .and_then(|e| e.id())
            .map(String::from)
        else {
            continue;
        };
        match running.iter_mut().find(|(i, _)| *i == id) {
            Some((_, list)) => list.push(window.id),
            None => running.push((id, vec![window.id])),
        }
    }
    running
}

impl Launcher {
    fn windows_of(&self, app: &App) -> Option<&Vec<u64>> {
        self.running.iter().find(|(id, _)| *id == app.id).map(|(_, w)| w)
    }

    /// Searches again for the text in the field.
    fn refresh(self: &Rc<Self>) {
        let query = self.entry.text().to_string();
        let apps = search::rank(&self.apps, &query, &self.history.borrow());
        let roots = SEARCH_PATHS.with_borrow(Clone::clone);
        let weak = Rc::downgrade(self);
        // When a fresh file index is done, the same search runs again with it.
        let files = search::files(&roots, move || {
            if let Some(this) = weak.upgrade() {
                this.refresh();
            }
        });
        let file_hits: Vec<(PathBuf, bool)> = files
            .as_deref()
            .map(|files| {
                search::rank_files(files, &query, MAX_FILES).into_iter().map(|f: &FileHit| (f.path.clone(), f.is_dir)).collect()
            })
            .unwrap_or_default();

        crate::ui::clear(&self.list);
        let mut hits = Vec::new();
        for app in apps.into_iter().take(MAX_APPS) {
            let row = self.app_row(&app);
            self.list.append(&row);
            hits.push((Hit::App(app), row));
        }
        if !file_hits.is_empty() {
            let header = gtk4::Label::new(Some("Files"));
            header.add_css_class("launcher-section");
            header.set_xalign(0.0);
            self.list.append(&header);
        }
        for (path, is_dir) in file_hits {
            let row = file_row(&path, is_dir);
            self.list.append(&row);
            hits.push((Hit::File(path), row));
        }
        self.scroll.set_visible(!hits.is_empty());
        self.hits.replace(hits);
        self.select(0);
        // The window keeps its size otherwise (the card shrinks inside it anyway).
        crate::ui::shrink_to_fit(&self.window);
    }

    fn app_row(self: &Rc<Self>, app: &Rc<App>) -> gtk4::Widget {
        let icon = gtk4::Image::new();
        match crate::apps::icon(&app.id).or_else(|| app.info.icon()) {
            Some(gicon) => icon.set_from_gicon(&gicon),
            None => icon.set_icon_name(Some("application-x-executable")),
        }
        let badge = self.windows_of(app).map(|_| "Running");
        row(&icon, &app.name, &app.detail, badge)
    }

    fn select_by(&self, step: isize) {
        let count = self.hits.borrow().len();
        if count == 0 {
            return;
        }
        let next = (self.selected.get() as isize + step).rem_euclid(count as isize) as usize;
        self.select(next);
    }

    fn select(&self, index: usize) {
        let hits = self.hits.borrow();
        for (i, (_, row)) in hits.iter().enumerate() {
            if i == index {
                row.add_css_class("selected");
            } else {
                row.remove_css_class("selected");
            }
        }
        self.selected.set(index);
        // Keep it in sight when moving through a long list.
        if let Some((_, row)) = hits.get(index)
            && let Some(bounds) = row.compute_bounds(&self.list)
        {
            let adjustment = self.scroll.vadjustment();
            let (top, bottom) = (f64::from(bounds.y()), f64::from(bounds.y() + bounds.height()));
            if top < adjustment.value() {
                adjustment.set_value(top);
            } else if bottom > adjustment.value() + adjustment.page_size() {
                adjustment.set_value(bottom - adjustment.page_size());
            }
        }
    }

    /// Opens the selected hit; `another` starts a new instance of a running app.
    fn activate(&self, another: bool) {
        let chosen = {
            let hits = self.hits.borrow();
            match hits.get(self.selected.get()) {
                Some((Hit::App(app), _)) => Some(Hit::App(app.clone())),
                Some((Hit::File(path), _)) => Some(Hit::File(path.clone())),
                None => None,
            }
        };
        let Some(chosen) = chosen else { return };
        match chosen {
            Hit::App(app) => {
                self.history.borrow_mut().record(&app.id);
                match self.windows_of(&app).and_then(|w| w.first()).filter(|_| !another) {
                    Some(&id) => self.services.niri.action(Action::FocusWindow { id }),
                    None => crate::launch::app(&app.info),
                }
            }
            Hit::File(path) => crate::launch::open_file(&path),
        }
        // After this event: a click on a row is still being handled inside the window.
        glib::idle_add_local_once(close);
    }
}

fn file_row(path: &std::path::Path, is_dir: bool) -> gtk4::Widget {
    let icon = gtk4::Image::new();
    let gicon = if is_dir {
        gio::ThemedIcon::new("folder").upcast::<gio::Icon>()
    } else {
        // By the name only: no reading of the file.
        gio::content_type_get_icon(&gio::content_type_guess(Some(path), None::<&[u8]>).0)
    };
    icon.set_from_gicon(&gicon);
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let folder = path.parent().map(search::short_path).unwrap_or_default();
    row(&icon, &name, &folder, None)
}

/// One result: icon, name, a dim line under it, and an optional badge at the end.
fn row(icon: &gtk4::Image, name: &str, detail: &str, badge: Option<&str>) -> gtk4::Widget {
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    row.add_css_class("launcher-row");
    icon.add_css_class("launcher-icon");
    let text = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    text.set_valign(gtk4::Align::Center);
    text.set_hexpand(true);
    let title = gtk4::Label::new(Some(name));
    title.add_css_class("launcher-name");
    title.set_xalign(0.0);
    title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    text.append(&title);
    if !detail.is_empty() {
        let line = gtk4::Label::new(Some(detail));
        line.add_css_class("launcher-detail");
        line.set_xalign(0.0);
        line.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
        text.append(&line);
    }
    row.append(icon);
    row.append(&text);
    if let Some(badge) = badge {
        let label = gtk4::Label::new(Some(badge));
        label.add_css_class("launcher-badge");
        label.set_valign(gtk4::Align::Center);
        row.append(&label);
    }
    // A click opens it, like Enter.
    let click = gtk4::GestureClick::new();
    click.connect_released(|gesture, _, _, _| {
        let Some(row) = gesture.widget() else { return };
        let Some(launcher) = OPEN.with_borrow(Clone::clone) else { return };
        let index = launcher.hits.borrow().iter().position(|(_, r)| *r == row);
        if let Some(index) = index {
            launcher.select(index);
            launcher.activate(false);
        }
    });
    row.add_controller(click);
    row.upcast()
}

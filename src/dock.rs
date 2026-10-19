//! Dock: pinned apps and running ones, with a dot per open window (up to three).
//! Left click starts the app, brings up its last window, or cycles through its
//! windows; middle click opens a new window; the wheel steps through windows; right
//! click opens a menu (windows, new window, pin, close).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;

use gio_unix::DesktopAppInfo;
use gtk4::prelude::*;
use gtk4::{gdk, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use niri_ipc::Action;

use crate::config::{self, Config};
use crate::niri::{Niri, State};
use crate::{blur, menu, ui};

/// Corner radius of `.dock` in style.css, for the blur shape.
const CARD_RADIUS: i32 = 16;
const MAX_DOTS: usize = 3;

/// One app in the dock, as drawn. Compared to skip redraws when nothing visible changed
/// (window titles change all the time and don't show here).
#[derive(Clone, PartialEq, Debug)]
struct Item {
    /// The pinned name, or the app id of a running unpinned app.
    key: String,
    pinned: bool,
    /// Window ids in a stable order (for cycling).
    windows: Vec<u64>,
    /// The window to bring up first: the most recently focused one.
    recent: Option<u64>,
    focused: Option<u64>,
    urgent: bool,
}

/// How long the dock stays after the pointer leaves it (with autohide).
const HIDE_DELAY: Duration = Duration::from_millis(500);

thread_local! {
    /// Docks by output name, for `nshell msg show-dock`.
    static DOCKS: RefCell<Vec<(String, Weak<Autohide>)>> = const { RefCell::new(Vec::new()) };
}

/// Slides the dock in and out: in while hovered (or asked to show), out after a moment.
struct Autohide {
    enabled: bool,
    window: glib::WeakRef<gtk4::ApplicationWindow>,
    /// The invisible strip along the screen's bottom edge that brings a hidden dock back.
    hot_edge: Option<gtk4::Window>,
    hovered: Cell<bool>,
    /// The pointer rests on the hot edge.
    on_edge: Cell<bool>,
    /// The monitor's workspace has no windows: nothing to cover, the dock stays up.
    empty: Cell<bool>,
    hide: Cell<Option<glib::SourceId>>,
    /// How far out the dock is: 0 hidden below the screen edge, 1 fully up.
    position: Cell<f64>,
    target: Cell<f64>,
    sliding: Cell<bool>,
    /// The dock's height when last seen, for sliding it out from under the edge.
    height: Cell<i32>,
}

/// How fast the dock slides: time constant of the ease-out, in microseconds.
const SLIDE_TAU_US: f64 = 45_000.0;
/// Animation step while sliding (about 60 per second).
const SLIDE_STEP: Duration = Duration::from_millis(16);

/// How far down a hidden dock goes: all but one pixel. A surface wholly off the screen
/// gets no frames from niri, and GTK then never commits the next position.
fn hidden_offset(height: i32) -> i32 {
    (height - 1).max(0)
}

impl Autohide {
    fn show(self: &Rc<Self>) {
        self.cancel_hide();
        self.slide_to(1.0);
    }

    /// Slides the whole window in or out by its bottom margin, so the dock moves as one
    /// piece. A dock that is all the way out has no window on screen.
    fn slide_to(self: &Rc<Self>, target: f64) {
        let Some(window) = self.window.upgrade() else { return };
        self.target.set(target);
        if target > 0.0 && !window.is_visible() {
            // Its real height, measured: a guess too large put it wholly off the screen,
            // where it never got a frame and stayed blank.
            if let Some(child) = window.child() {
                let (_, natural, _, _) = child.measure(gtk4::Orientation::Vertical, -1);
                if natural > 0 {
                    self.height.set(natural);
                }
            }
            // Starts below the screen edge (but one pixel), so its first frame isn't seen.
            self.position.set(0.0);
            window.set_margin(Edge::Bottom, -hidden_offset(self.height.get()));
            window.set_visible(true);
        }
        if self.sliding.replace(true) {
            return;
        }
        // A timer, not the frame clock: niri sends no frames to a surface that is fully
        // below the screen edge, so a frame-driven slide never started.
        let weak = Rc::downgrade(self);
        let last = Cell::new(std::time::Instant::now());
        glib::timeout_add_local(SLIDE_STEP, move || {
            let Some(this) = weak.upgrade() else { return glib::ControlFlow::Break };
            let Some(window) = this.window.upgrade() else { return glib::ControlFlow::Break };
            let dt = last.replace(std::time::Instant::now()).elapsed().as_micros() as f64;
            let (pos, target) = (this.position.get(), this.target.get());
            let mut next = pos + (target - pos) * (1.0 - (-dt / SLIDE_TAU_US).exp());
            let done = (target - next).abs() < 0.01;
            if done {
                next = target;
            }
            this.position.set(next);
            if window.height() > 0 {
                this.height.set(window.height());
            }
            let offset = ((1.0 - next) * f64::from(hidden_offset(this.height.get()))).round() as i32;
            window.set_margin(Edge::Bottom, -offset);
            if !done {
                return glib::ControlFlow::Continue;
            }
            this.sliding.set(false);
            if target == 0.0 {
                window.set_visible(false);
            }
            glib::ControlFlow::Break
        });
    }

    /// Hides after `delay`, unless hovered or a dock menu is open then.
    fn hide_after(self: &Rc<Self>, delay: Duration) {
        if !self.enabled {
            return;
        }
        self.cancel_hide();
        let weak = Rc::downgrade(self);
        let source = glib::timeout_add_local_once(delay, move || {
            let Some(this) = weak.upgrade() else { return };
            this.hide.set(None);
            if this.hovered.get() || this.on_edge.get() || this.empty.get() {
                return;
            }
            if menu::is_open() {
                this.hide_after(HIDE_DELAY);
                return;
            }
            this.slide_to(0.0);
        });
        self.hide.set(Some(source));
    }

    fn cancel_hide(&self) {
        if let Some(source) = self.hide.take() {
            source.remove();
        }
    }
}

/// Shows the dock on niri's focused monitor for a while (bound to a key in niri).
pub fn peek(niri: &Niri, seconds: u32) {
    let Some(output) = niri.focused_monitor().and_then(|m| m.connector()) else { return };
    let dock = DOCKS.with_borrow(|docks| {
        docks.iter().find(|(o, _)| *o == output.as_str()).and_then(|(_, d)| d.upgrade())
    });
    if let Some(dock) = dock {
        dock.show();
        if !dock.hovered.get() {
            dock.hide_after(Duration::from_secs(u64::from(seconds)));
        }
    }
}

pub fn build(app: &gtk4::Application, monitor: &gdk::Monitor, config: &Rc<Config>, niri: &Niri) -> gtk4::ApplicationWindow {
    let window = gtk4::ApplicationWindow::new(app);
    window.add_css_class("nshell-dock");
    window.init_layer_shell();
    window.set_namespace(Some("nshell-dock"));
    window.set_layer(Layer::Top);
    window.set_monitor(Some(monitor));
    window.set_keyboard_mode(KeyboardMode::None);
    window.set_anchor(Edge::Bottom, true);
    // Over windows, reserving no space.
    window.set_exclusive_zone(0);

    let dock = gtk4::Box::new(gtk4::Orientation::Horizontal, 2);
    dock.add_css_class("dock");
    let apps = gtk4::Box::new(gtk4::Orientation::Horizontal, 2);
    dock.append(&apps);
    dock.append(&gtk4::Separator::new(gtk4::Orientation::Vertical));
    let overview = gtk4::Button::from_icon_name("view-app-grid-symbolic");
    overview.add_css_class("dock-button");
    overview.set_cursor_from_name(Some("pointer"));
    let n = niri.clone();
    overview.connect_clicked(move |_| n.action(Action::ToggleOverview {}));
    dock.append(&overview);

    // With autohide the dock slides below the screen edge and its window goes away
    // entirely; a separate two-pixel strip along the bottom edge brings it back.
    let autohide = config.dock.autohide;
    let dock_weak = dock.downgrade();
    let wrapped = blur::behind(
        window.upcast_ref(),
        &dock,
        move || dock_weak.upgrade().map(|d| d.upcast()).into_iter().collect(),
        CARD_RADIUS,
        || {},
    );
    window.set_child(Some(&wrapped));

    let state = Rc::new(Autohide {
        enabled: autohide,
        window: window.downgrade(),
        hot_edge: autohide.then(|| hot_edge(monitor)),
        hovered: Cell::new(false),
        on_edge: Cell::new(false),
        empty: Cell::new(false),
        hide: Cell::new(None),
        position: Cell::new(if autohide { 0.0 } else { 1.0 }),
        target: Cell::new(if autohide { 0.0 } else { 1.0 }),
        sliding: Cell::new(false),
        height: Cell::new(80),
    });
    if let Some(edge) = &state.hot_edge {
        let touch = gtk4::EventControllerMotion::new();
        let weak = Rc::downgrade(&state);
        touch.connect_enter(move |_, _, _| {
            if let Some(state) = weak.upgrade() {
                state.on_edge.set(true);
                state.show();
            }
        });
        let weak = Rc::downgrade(&state);
        touch.connect_leave(move |_| {
            if let Some(state) = weak.upgrade() {
                state.on_edge.set(false);
                // Left the strip without reaching the dock: it goes again.
                state.hide_after(HIDE_DELAY);
            }
        });
        edge.add_controller(touch);
    }
    let motion = gtk4::EventControllerMotion::new();
    let weak = Rc::downgrade(&state);
    motion.connect_enter(move |_, _, _| {
        if let Some(state) = weak.upgrade() {
            state.hovered.set(true);
            state.show();
        }
    });
    let weak = Rc::downgrade(&state);
    motion.connect_leave(move |_| {
        if let Some(state) = weak.upgrade() {
            state.hovered.set(false);
            state.hide_after(HIDE_DELAY);
        }
    });
    window.add_controller(motion);
    if let Some(output) = monitor.connector() {
        DOCKS.with_borrow_mut(|docks| {
            docks.retain(|(_, d)| d.strong_count() > 0);
            docks.push((output.to_string(), Rc::downgrade(&state)));
        });
    }
    if autohide && let Some(output) = monitor.connector().map(String::from) {
        follow_empty_workspace(&state, niri, output);
    }

    // The handlers hold only weak references; this one keeps the state as long as the
    // window lives, and stops a pending hide with it.
    window.connect_destroy(move |_| {
        state.cancel_hide();
        if let Some(edge) = &state.hot_edge {
            edge.destroy();
        }
    });

    let (config, niri2) = (config.clone(), niri.clone());
    let last = RefCell::new(None::<Vec<Item>>);
    let serial = Cell::new(None::<u64>);
    niri.state.bind(&apps, move |apps, state| {
        // Title changes don't show in the dock.
        if serial.replace(Some(state.layout_serial)) == Some(state.layout_serial) {
            return;
        }
        let items = items(state, &config.dock.pinned);
        if last.borrow().as_ref() == Some(&items) {
            return;
        }
        ui::clear(apps);
        for (i, item) in items.iter().enumerate() {
            // Running apps that aren't pinned come after a line.
            if !item.pinned && items.get(i.wrapping_sub(1)).is_some_and(|prev| prev.pinned) {
                apps.append(&gtk4::Separator::new(gtk4::Orientation::Vertical));
            }
            apps.append(&item_widget(item, &config, &niri2));
        }
        last.replace(Some(items));
    });
    // With autohide the dock starts hidden: no window until it is called.
    window.set_visible(!autohide);
    window
}

/// With autohide: the dock comes up on an empty workspace (as if hovered) and goes once
/// a window opens there.
fn follow_empty_workspace(state: &Rc<Autohide>, niri: &Niri, output: String) {
    let (weak, serial) = (Rc::downgrade(state), Cell::new(None::<u64>));
    niri.state.subscribe(move |niri_state| {
        let Some(state) = weak.upgrade() else { return false };
        // Title changes don't open or close windows.
        if serial.replace(Some(niri_state.layout_serial)) == Some(niri_state.layout_serial) {
            return true;
        }
        let workspace = niri_state
            .workspaces
            .values()
            .find(|ws| ws.is_active && ws.output.as_deref() == Some(output.as_str()));
        // Unknown yet (niri hasn't told): leave the dock as it is.
        let Some(workspace) = workspace else { return true };
        let empty = !niri_state.windows.values().any(|w| w.workspace_id == Some(workspace.id));
        if state.empty.replace(empty) != empty {
            // Not from inside niri's notification: sliding touches the window.
            glib::idle_add_local_once(move || {
                if empty {
                    state.show();
                } else {
                    state.hide_after(HIDE_DELAY);
                }
            });
        }
        true
    });
}

/// A transparent strip, two pixels high, along the bottom of the screen.
fn hot_edge(monitor: &gdk::Monitor) -> gtk4::Window {
    let edge = gtk4::Window::new();
    edge.add_css_class("dock-hot-edge");
    edge.init_layer_shell();
    edge.set_namespace(Some("nshell-dock-edge"));
    edge.set_layer(Layer::Top);
    edge.set_monitor(Some(monitor));
    edge.set_keyboard_mode(KeyboardMode::None);
    for side in [Edge::Bottom, Edge::Left, Edge::Right] {
        edge.set_anchor(side, true);
    }
    edge.set_exclusive_zone(0);
    // Not -1: GTK would fall back to its default 200 px window height.
    edge.set_default_size(1, 2);
    let fill = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    fill.set_size_request(-1, 2);
    edge.set_child(Some(&fill));
    edge.set_visible(true);
    edge
}

/// Pinned apps in their order, then running ones that aren't pinned.
fn items(state: &State, pinned: &[String]) -> Vec<Item> {
    let mut items: Vec<Item> = pinned
        .iter()
        .map(|key| Item { key: key.clone(), pinned: true, windows: Vec::new(), recent: None, focused: None, urgent: false })
        .collect();
    let mut windows: Vec<_> = state.windows.values().collect();
    windows.sort_by_key(|w| w.id);
    for window in windows {
        let app_id = window.app_id.clone().unwrap_or_default();
        let i = match items.iter().position(|item| same_app(&item.key, &app_id)) {
            Some(i) => i,
            None => {
                items.push(Item { key: app_id, pinned: false, windows: Vec::new(), recent: None, focused: None, urgent: false });
                items.len() - 1
            }
        };
        let item = &mut items[i];
        item.windows.push(window.id);
        if window.is_focused {
            item.focused = Some(window.id);
        }
        item.urgent |= window.is_urgent;
        let newer = |id: u64| {
            let stamp = |id: u64| state.windows.get(&id).and_then(|w| w.focus_timestamp).map(|t| (t.secs, t.nanos));
            stamp(window.id) > stamp(id)
        };
        if item.recent.is_none_or(newer) {
            item.recent = Some(window.id);
        }
    }
    items
}

/// Whether a window's app id belongs to a pinned name: equal, or the same desktop entry.
fn same_app(key: &str, app_id: &str) -> bool {
    if key.eq_ignore_ascii_case(app_id) {
        return true;
    }
    let (Some(a), Some(b)) = (crate::apps::entry(key), crate::apps::entry(app_id)) else {
        return false;
    };
    a.id() == b.id()
}

fn item_widget(item: &Item, config: &Rc<Config>, niri: &Niri) -> gtk4::Box {
    let widget = gtk4::Box::new(gtk4::Orientation::Vertical, 3);
    widget.add_css_class("dock-item");
    if item.focused.is_some() {
        widget.add_css_class("focused");
    }
    if item.urgent {
        widget.add_css_class("urgent");
    }
    let entry = crate::apps::entry(&item.key);
    let icon = gtk4::Image::new();
    icon.add_css_class("dock-icon");
    match crate::apps::icon(&item.key) {
        Some(gicon) => icon.set_from_gicon(&gicon),
        None => icon.set_icon_name(Some("application-x-executable")),
    }
    widget.set_tooltip_text(Some(&entry.as_ref().map_or_else(|| item.key.clone(), |e| e.name().to_string())));
    widget.append(&icon);

    let dots = gtk4::Box::new(gtk4::Orientation::Horizontal, 3);
    dots.add_css_class("dots");
    dots.set_halign(gtk4::Align::Center);
    for _ in 0..item.windows.len().min(MAX_DOTS) {
        let dot = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        dot.add_css_class("dot");
        dots.append(&dot);
    }
    widget.append(&dots);

    let click = gtk4::GestureClick::new();
    click.set_button(0);
    let (it, n, e) = (item.clone(), niri.clone(), entry.clone());
    click.connect_released(move |gesture, _, _, _| match gesture.current_button() {
        gdk::BUTTON_PRIMARY => activate(&it, &n, e.as_ref()),
        gdk::BUTTON_MIDDLE => {
            if let Some(entry) = &e {
                crate::launch::new_window(entry);
            }
        }
        _ => {}
    });
    widget.add_controller(click);

    let (it, n) = (item.clone(), niri.clone());
    let scroll = gtk4::EventControllerScroll::new(
        gtk4::EventControllerScrollFlags::VERTICAL | gtk4::EventControllerScrollFlags::DISCRETE,
    );
    scroll.connect_scroll(move |_, _, dy| {
        cycle(&it, &n, if dy > 0.0 { 1 } else { -1 });
        glib::Propagation::Stop
    });
    widget.add_controller(scroll);

    let (it, c, n) = (item.clone(), config.clone(), niri.clone());
    menu::register(&widget, move |anchor| menu::show(anchor, &item_menu(&it, &c, &n, entry.as_ref())));
    menu::on_button(&widget, gdk::BUTTON_SECONDARY);
    widget
}

/// Left click: start it, bring up its last window, or go to its next window.
fn activate(item: &Item, niri: &Niri, entry: Option<&DesktopAppInfo>) {
    match (item.focused, item.recent) {
        (Some(_), _) => cycle(item, niri, 1),
        (None, Some(id)) => niri.action(Action::FocusWindow { id }),
        (None, None) => {
            if let Some(entry) = entry {
                crate::launch::app(entry);
            }
        }
    }
}

/// Focuses the next (or previous) window of the app.
fn cycle(item: &Item, niri: &Niri, step: isize) {
    let n = item.windows.len();
    if n == 0 {
        return;
    }
    let current = item
        .focused
        .and_then(|f| item.windows.iter().position(|&w| w == f))
        .map_or(-step, |i| i as isize);
    let next = (current + step).rem_euclid(n as isize) as usize;
    niri.action(Action::FocusWindow { id: item.windows[next] });
}

fn item_menu(item: &Item, config: &Rc<Config>, niri: &Niri, entry: Option<&DesktopAppInfo>) -> gtk4::Box {
    let column = ui::column();
    let name = entry.map_or_else(|| item.key.clone(), |e| e.name().to_string());
    column.append(&ui::title(&name));

    // The app's windows by title: the dock has no previews, this is the way to pick one.
    let state = niri.state.get();
    for id in &item.windows {
        let title = state
            .windows
            .get(id)
            .and_then(|w| w.title.clone())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| name.clone());
        let (n, id) = (niri.clone(), *id);
        column.append(&ui::item(&title, item.focused == Some(id), move || {
            n.action(Action::FocusWindow { id });
            menu::close();
        }));
    }
    drop(state);
    if !item.windows.is_empty() {
        column.append(&ui::separator());
    }

    if let Some(entry) = entry.cloned() {
        let running = !item.windows.is_empty();
        column.append(&ui::item(if running { "New window" } else { "Open" }, false, move || {
            if running {
                crate::launch::new_window(&entry);
            } else {
                crate::launch::app(&entry);
            }
            menu::close();
        }));
    }
    let (key, pinned, list) = (item.key.clone(), item.pinned, config.dock.pinned.clone());
    column.append(&ui::item(if pinned { "Unpin" } else { "Pin" }, false, move || {
        let mut list = list.clone();
        if pinned {
            list.retain(|k| *k != key);
        } else {
            list.push(key.clone());
        }
        if let Err(err) = config::save_pinned(&list) {
            tracing::warn!("can't save the dock: {err}");
        }
        menu::close();
    }));
    if !item.windows.is_empty() {
        let (n, windows) = (niri.clone(), item.windows.clone());
        let label = if windows.len() > 1 { "Close all" } else { "Close" };
        column.append(&ui::item(label, false, move || {
            for id in &windows {
                n.action(Action::CloseWindow { id: Some(*id) });
            }
            menu::close();
        }));
    }
    column
}

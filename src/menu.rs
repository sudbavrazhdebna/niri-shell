//! Menus of bar modules, like the macOS menu bar: a click opens a module's menu, and
//! while one is open, moving the pointer onto another module opens that one instead.
//!
//! A menu is its own layer-shell window under the bar, not a GTK popover: popovers get
//! closed by the compositor when their size changes, and menus here grow and shrink
//! while open (device lists, scan results). A transparent catcher window covers the
//! rest of the screen below the bar and closes the menu on a click outside it.
//!
//! A module registers an *opener* for its widget (the anchor). The opener builds the
//! content and passes it to [`show`]. Reaching for an open menu diagonally crosses
//! neighbouring modules; while the pointer heads into the menu, a switch waits
//! [`AIM_DELAY`] and happens only if the pointer is still on that module.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gtk4::prelude::*;
use gtk4::{gdk, glib, graphene};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::{blur, ui};

type Opener = Rc<dyn Fn(&gtk4::Widget)>;

thread_local! {
    /// The anchor of the last closed menu and when it closed: the press of a click on
    /// the module can close the menu (focus goes back to the app) before the release
    /// toggles it, which would open it again.
    static LAST_CLOSED: RefCell<Option<(glib::WeakRef<gtk4::Widget>, std::time::Instant)>> = const { RefCell::new(None) };
    /// Bumped on every open and close: a menu that loads its content first shows only if
    /// nothing else was asked for meanwhile.
    static REQUEST: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static OPENERS: RefCell<Vec<(glib::WeakRef<gtk4::Widget>, Opener)>> = const { RefCell::new(Vec::new()) };
    static OPEN: RefCell<Option<Open>> = const { RefCell::new(None) };
    static AIM: RefCell<Aim> = RefCell::new(Aim::default());
}

/// Gap between the bar and a menu.
const GAP: i32 = 6;
/// Menus keep this far from the screen edges.
const EDGE_MARGIN: i32 = 8;
/// Corner radius of the menu card (`.menu-card` in style.css), for the blur shape.
const CARD_RADIUS: i32 = 14;
/// How long the pointer must stay on a module it reached while heading into the open
/// menu before that module's menu opens.
const AIM_DELAY: Duration = Duration::from_millis(250);
/// Pointer travel before its heading is measured again; single events are too noisy.
const AIM_SAMPLE_PX: f64 = 6.0;
/// Slack around the open menu when checking whether the pointer heads into it.
const AIM_SLACK_PX: f64 = 16.0;

struct Open {
    anchor: glib::WeakRef<gtk4::Widget>,
    window: gtk4::Window,
    catcher: gtk4::Window,
    /// The glass card the content goes in.
    card: gtk4::Box,
    monitor: gdk::Monitor,
    upward: bool,
    /// Shared with the window's resize handler, which re-centers under this anchor.
    placed_under: Rc<RefCell<glib::WeakRef<gtk4::Widget>>>,
}

#[derive(Default)]
struct Aim {
    origin: Option<(f64, f64)>,
    heading_in: bool,
    last_pointer: Option<(glib::WeakRef<gtk4::Window>, f64, f64)>,
    pending: Option<(gtk4::Widget, glib::SourceId)>,
}

/// Makes `anchor` a menu owner: left click toggles its menu, and hovering it while
/// another menu is open switches to it.
pub fn attach(anchor: &impl IsA<gtk4::Widget>, opener: impl Fn(&gtk4::Widget) + 'static) {
    register(anchor, opener);
    on_button(anchor, gdk::BUTTON_PRIMARY);
}

/// Like [`attach`] without the left click: for owners whose left click does something
/// else (tray apps). Open the menu with [`toggle`] or [`on_button`].
pub fn register(anchor: &impl IsA<gtk4::Widget>, opener: impl Fn(&gtk4::Widget) + 'static) {
    let anchor = anchor.as_ref();
    anchor.add_css_class("has-menu");
    anchor.set_cursor_from_name(Some("pointer"));
    OPENERS.with_borrow_mut(|openers| {
        openers.retain(|(w, _)| w.upgrade().is_some());
        openers.push((anchor.downgrade(), Rc::new(opener)));
    });
}

/// Toggles the menu with this mouse button.
pub fn on_button(anchor: &impl IsA<gtk4::Widget>, button: u32) {
    let gesture = gtk4::GestureClick::new();
    gesture.set_button(button);
    gesture.connect_released(|gesture, _, _, _| {
        if let Some(anchor) = gesture.widget() {
            toggle(&anchor);
        }
    });
    anchor.as_ref().add_controller(gesture);
}

/// Opens the anchor's menu, or closes it if it is the open one.
pub fn toggle(anchor: &gtk4::Widget) {
    if open_anchor().as_ref() == Some(anchor) {
        close();
        return;
    }
    let just_closed = LAST_CLOSED.with_borrow(|last| {
        last.as_ref().is_some_and(|(w, at)| {
            w.upgrade().as_ref() == Some(anchor) && at.elapsed() < Duration::from_millis(300)
        })
    });
    if !just_closed {
        open(anchor);
    }
}

/// Opens the menu registered for `anchor`, closing any other first.
pub fn open(anchor: &gtk4::Widget) {
    let opener = OPENERS.with_borrow(|openers| {
        openers
            .iter()
            .find(|(w, _)| w.upgrade().as_ref() == Some(anchor))
            .map(|(_, opener)| opener.clone())
    });
    if let Some(opener) = opener {
        REQUEST.set(REQUEST.get() + 1);
        opener(anchor);
    }
}

/// For openers that load their content first: take a ticket when the opener runs, and
/// show with [`show_for`] when ready.
pub fn ticket() -> u64 {
    REQUEST.get()
}

/// Shows the menu if no other menu was opened or closed since `ticket` was taken.
pub fn show_for(ticket: u64, anchor: &gtk4::Widget, content: &impl IsA<gtk4::Widget>) {
    if ticket == REQUEST.get() {
        show(anchor, content);
    }
}

pub fn is_open() -> bool {
    OPEN.with_borrow(Option::is_some)
}

/// Closes the open menu, if any.
pub fn close() {
    cancel_pending();
    REQUEST.set(REQUEST.get() + 1);
    let Some(open) = OPEN.take() else { return };
    if let Some(anchor) = open.anchor.upgrade() {
        anchor.remove_css_class("menu-open");
        LAST_CLOSED.with_borrow_mut(|last| *last = Some((anchor.downgrade(), std::time::Instant::now())));
    }
    open.window.destroy();
    open.catcher.destroy();
}

/// Closes the menu when the user goes back to work in niri: a window gets the focus
/// (niri gives it to the menu while it shows, so clicking any app window, even the one
/// that had it before, counts) or another workspace is focused.
pub fn close_on_focus_change(niri: &crate::niri::Niri) {
    let last = RefCell::new(None::<(Option<u64>, Option<u64>)>);
    niri.state.subscribe(move |state| {
        let window = state.windows.values().find(|w| w.is_focused).map(|w| w.id);
        let workspace = state.workspaces.values().find(|w| w.is_focused).map(|w| w.id);
        let before = last.replace(Some((window, workspace)));
        let moved_on = before.is_some_and(|(old_window, old_workspace)| {
            (window.is_some() && window != old_window) || workspace != old_workspace
        });
        if moved_on && is_open() {
            // Not from inside this notification: closing touches widgets.
            glib::idle_add_local_once(close);
        }
        true
    });
}

/// Shows `content` as the menu of `anchor`, replacing any open menu.
pub fn show(anchor: &gtk4::Widget, content: &impl IsA<gtk4::Widget>) {
    let Some(bar) = anchor.root().and_downcast::<gtk4::Window>() else { return };
    let Some(monitor) = bar.monitor() else { return };
    // Menus open away from the screen edge their owner sits on: below the bar, above
    // the dock.
    let upward = bar.is_anchor(Edge::Bottom) && !bar.is_anchor(Edge::Top);
    cancel_pending();

    // Switching between menus keeps the window and swaps the content: the keyboard
    // focus stays with the menu (recreating it handed the focus to the app for a moment,
    // which closed the new menu and made the app's focus blink).
    let reusable = OPEN.with_borrow(|open| {
        open.as_ref()
            .filter(|o| o.monitor == monitor && o.upward == upward)
            .map(|o| (o.window.clone(), o.card.clone(), o.anchor.upgrade(), o.placed_under.clone()))
    });
    if let Some((window, card, old, placed_under)) = reusable {
        if let Some(old) = old {
            old.remove_css_class("menu-open");
        }
        while let Some(child) = card.first_child() {
            card.remove(&child);
        }
        card.append(content);
        placed_under.replace(anchor.downgrade());
        if let Some(wrapped) = window.child() {
            let (_, natural, _, _) = wrapped.measure(gtk4::Orientation::Horizontal, -1);
            place(&window, anchor, natural);
        }
        // Fit the new content once it is styled (measured right away it isn't yet, and the
        // window kept the previous menu's height).
        let weak = window.downgrade();
        glib::idle_add_local_once(move || {
            if let Some(window) = weak.upgrade() {
                ui::shrink_to_fit(&window);
            }
        });
        anchor.add_css_class("menu-open");
        OPEN.with_borrow_mut(|open| {
            if let Some(open) = open {
                open.anchor = anchor.downgrade();
            }
        });
        return;
    }

    // Another monitor or direction: a fresh window, keeping the catcher if possible.
    let catcher = match OPEN.take() {
        Some(open) => {
            if let Some(old) = open.anchor.upgrade() {
                old.remove_css_class("menu-open");
            }
            open.window.destroy();
            if open.monitor == monitor {
                open.catcher
            } else {
                open.catcher.destroy();
                catcher(&monitor)
            }
        }
        None => catcher(&monitor),
    };

    let window = gtk4::Window::new();
    window.add_css_class("nshell-menu");
    window.init_layer_shell();
    window.set_namespace(Some("nshell-menu"));
    // Above the catcher, which shares the screen area below the bar.
    window.set_layer(Layer::Overlay);
    window.set_monitor(Some(&monitor));
    // On demand, not exclusive: niri gives the menu the keyboard when it shows (Escape
    // works) and gives it back to the app on a click there, which also closes the menu.
    window.set_keyboard_mode(KeyboardMode::OnDemand);
    window.set_anchor(Edge::Top, !upward);
    window.set_anchor(Edge::Bottom, upward);
    window.set_anchor(Edge::Left, true);
    // Zero exclusive zone: placed below the bar, not over it.
    window.set_exclusive_zone(0);
    if upward {
        window.set_margin(Edge::Bottom, bar.margin(Edge::Bottom) + bar.height() + GAP);
    } else {
        window.set_margin(Edge::Top, GAP);
    }

    let card = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    card.add_css_class("menu-card");
    card.append(content);
    // Re-centered under its module and re-blurred whenever the card changes size.
    let placed_under = Rc::new(RefCell::new(anchor.downgrade()));
    let (window_weak, under, card_weak) = (window.downgrade(), placed_under.clone(), card.downgrade());
    let wrapped = blur::behind(
        &window,
        &card,
        move || card_weak.upgrade().map(|c| c.upcast()).into_iter().collect(),
        CARD_RADIUS,
        move || {
            if let (Some(window), Some(anchor)) = (window_weak.upgrade(), under.borrow().upgrade()) {
                place(&window, &anchor, window.width());
            }
        },
    );
    window.set_child(Some(&wrapped));

    let keys = gtk4::EventControllerKey::new();
    keys.connect_key_pressed(|_, key, _, _| {
        if key == gdk::Key::Escape {
            close();
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    window.add_controller(keys);
    watch_motion(&window, &bar);

    // Placed at its real width before it shows, so it doesn't jump sideways.
    let (_, natural, _, _) = wrapped.measure(gtk4::Orientation::Horizontal, -1);
    place(&window, anchor, natural);
    window.set_visible(true);
    anchor.add_css_class("menu-open");
    OPEN.replace(Some(Open {
        anchor: anchor.downgrade(),
        window,
        catcher,
        card,
        monitor,
        upward,
        placed_under,
    }));
}

/// Transparent window over the screen below the bar: a click on it closes the menu.
fn catcher(monitor: &gdk::Monitor) -> gtk4::Window {
    let catcher = gtk4::Window::new();
    catcher.add_css_class("menu-catcher");
    catcher.init_layer_shell();
    catcher.set_namespace(Some("nshell-menu-catcher"));
    catcher.set_layer(Layer::Top);
    catcher.set_monitor(Some(monitor));
    catcher.set_keyboard_mode(KeyboardMode::None);
    for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        catcher.set_anchor(edge, true);
    }
    // Zero exclusive zone keeps it below the bar, so the bar stays hoverable.
    catcher.set_exclusive_zone(0);
    // Something to fill the window, so it surely has a size and takes input everywhere.
    let fill = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    fill.set_hexpand(true);
    fill.set_vexpand(true);
    catcher.set_child(Some(&fill));
    let click = gtk4::GestureClick::new();
    click.set_button(0);
    click.connect_pressed(|_, _, x, y| {
        tracing::debug!(x, y, "menu catcher clicked");
        close();
    });
    catcher.add_controller(click);
    catcher.set_visible(true);
    catcher
}

/// Centers the menu (`width` wide) under its module, keeping it on the screen.
fn place(window: &gtk4::Window, anchor: &gtk4::Widget, width: i32) {
    let Some(owner) = anchor.root().and_downcast::<gtk4::Window>() else { return };
    let Some(bounds) = anchor.compute_bounds(&owner) else { return };
    let screen = owner.monitor().map_or(owner.width(), |m| m.geometry().width());
    // Where the owner window starts on the screen: the bar spans it, the dock is centered.
    let origin = if owner.is_anchor(Edge::Left) && owner.is_anchor(Edge::Right) {
        0
    } else if owner.is_anchor(Edge::Left) {
        owner.margin(Edge::Left)
    } else {
        (screen - owner.width()) / 2
    };
    let width = width.max(1);
    let center = f64::from(origin) + f64::from(bounds.x()) + f64::from(bounds.width()) / 2.0;
    let left = (center - f64::from(width) / 2.0).round() as i32;
    let left = left.clamp(EDGE_MARGIN, (screen - width - EDGE_MARGIN).max(EDGE_MARGIN));
    if window.margin(Edge::Left) != left {
        window.set_margin(Edge::Left, left);
    }
}

fn open_anchor() -> Option<gtk4::Widget> {
    OPEN.with_borrow(|open| open.as_ref().and_then(|o| o.anchor.upgrade()))
}

/// The open menu's area in bar coordinates (bar and menu share the monitor's origin).
fn open_rect(bar: &gtk4::Window) -> Option<graphene::Rect> {
    OPEN.with_borrow(|open| {
        let open = open.as_ref()?;
        let left = open.window.margin(Edge::Left) as f32;
        let top = (bar.height() + GAP) as f32;
        Some(graphene::Rect::new(
            left,
            top,
            open.window.width() as f32,
            open.window.height() as f32,
        ))
    })
}

/// Sandbox testing without a mouse: with NSHELL_DEV_MENU=<module>[,<module>…][@seconds]
/// set, a debug build opens those modules' menus one after another, a second apart,
/// starting that long (default 2 s) after the bar appears — like hovering along the bar.
#[cfg(debug_assertions)]
pub fn dev_open(window: &gtk4::Window) {
    let Ok(spec) = std::env::var("NSHELL_DEV_MENU") else { return };
    let (list, secs) = match spec.split_once('@') {
        Some((list, secs)) => (list.to_string(), secs.parse().unwrap_or(2)),
        None => (spec, 2),
    };
    for (i, class) in list.split(',').map(String::from).enumerate() {
        let window = window.downgrade();
        glib::timeout_add_local_once(Duration::from_secs(secs + i as u64), move || {
            let Some(window) = window.upgrade() else { return };
            let mut stack: Vec<gtk4::Widget> = window.first_child().into_iter().collect();
            while let Some(widget) = stack.pop() {
                if widget.has_css_class(&class) && widget.has_css_class("has-menu") {
                    open(&widget);
                    return;
                }
                let mut child = widget.first_child();
                while let Some(c) = child {
                    child = c.next_sibling();
                    stack.push(c);
                }
            }
            tracing::warn!("NSHELL_DEV_MENU: no menu on {class:?}");
        });
    }
}

/// Follows the pointer over a bar window to switch menus, and closes the menu on a
/// click on the bar outside any module that has one.
pub fn watch_bar(window: &gtk4::Window) {
    let motion = gtk4::EventControllerMotion::new();
    motion.set_propagation_phase(gtk4::PropagationPhase::Capture);
    let weak = window.downgrade();
    motion.connect_motion(move |_, x, y| {
        if let Some(window) = weak.upgrade() {
            on_motion(&window, x, y);
        }
    });
    window.add_controller(motion);

    let click = gtk4::GestureClick::new();
    click.set_button(0);
    click.set_propagation_phase(gtk4::PropagationPhase::Capture);
    let weak = window.downgrade();
    click.connect_pressed(move |_, _, x, y| {
        let Some(window) = weak.upgrade() else { return };
        if OPEN.with_borrow(Option::is_some) && menu_owner_at(&window, x, y).is_none() {
            close();
        }
    });
    window.add_controller(click);
}

/// The menu window gets the pointer while it is over it; report that to the bar so the
/// heading towards the menu stays up to date.
fn watch_motion(menu: &gtk4::Window, bar: &gtk4::Window) {
    let motion = gtk4::EventControllerMotion::new();
    let bar = bar.downgrade();
    let menu_weak = menu.downgrade();
    motion.connect_motion(move |_, x, y| {
        let (Some(bar), Some(menu)) = (bar.upgrade(), menu_weak.upgrade()) else { return };
        let (bx, by) = (
            x + f64::from(menu.margin(Edge::Left)),
            y + f64::from(bar.height() + GAP),
        );
        AIM.with_borrow_mut(|aim| aim.last_pointer = Some((bar.downgrade(), bx, by)));
    });
    menu.add_controller(motion);
}

fn on_motion(window: &gtk4::Window, x: f64, y: f64) {
    AIM.with_borrow_mut(|aim| aim.last_pointer = Some((window.downgrade(), x, y)));
    let Some(current) = open_anchor() else {
        cancel_pending();
        return;
    };
    update_heading(window, x, y);

    let Some(target) = menu_owner_at(window, x, y).filter(|t| *t != current) else {
        cancel_pending();
        return;
    };
    if !AIM.with_borrow(|aim| aim.heading_in) {
        cancel_pending();
        open(&target);
        return;
    }
    let waiting = AIM.with_borrow(|aim| aim.pending.as_ref().map(|(w, _)| w) == Some(&target));
    if !waiting {
        cancel_pending();
        let source = glib::timeout_add_local_once(AIM_DELAY, fire_pending);
        AIM.with_borrow_mut(|aim| aim.pending = Some((target, source)));
    }
}

/// The delayed switch is due: switch if the pointer is still on its target.
fn fire_pending() {
    let Some((target, _)) = AIM.with_borrow_mut(|aim| aim.pending.take()) else { return };
    let pointer = AIM.with_borrow(|aim| {
        aim.last_pointer
            .as_ref()
            .and_then(|(w, x, y)| Some((w.upgrade()?, *x, *y)))
    });
    let Some((window, x, y)) = pointer else { return };
    if open_anchor().is_some() && menu_owner_at(&window, x, y).as_ref() == Some(&target) {
        open(&target);
    }
}

fn cancel_pending() {
    if let Some((_, source)) = AIM.with_borrow_mut(|aim| aim.pending.take()) {
        source.remove();
    }
}

/// Measures again whether the pointer heads into the open menu, once it has moved
/// far enough to have a direction.
fn update_heading(window: &gtk4::Window, x: f64, y: f64) {
    let Some((ox, oy)) = AIM.with_borrow(|aim| aim.origin) else {
        AIM.with_borrow_mut(|aim| aim.origin = Some((x, y)));
        return;
    };
    let (dx, dy) = (x - ox, y - oy);
    if dx.hypot(dy) < AIM_SAMPLE_PX {
        return;
    }
    let heading_in = open_rect(window).is_some_and(|menu| heads_into((x, y), (dx, dy), &menu));
    AIM.with_borrow_mut(|aim| {
        aim.origin = Some((x, y));
        aim.heading_in = heading_in;
    });
}

/// Whether moving from `point` along `delta` reaches the top edge of `menu`
/// (menus open below the bar).
fn heads_into((x, y): (f64, f64), (dx, dy): (f64, f64), menu: &graphene::Rect) -> bool {
    if dy <= 0.0 {
        return false;
    }
    let (left, top) = (f64::from(menu.x()), f64::from(menu.y()));
    let right = left + f64::from(menu.width());
    let hit = x + dx * (top - y).max(0.0) / dy;
    (left - AIM_SLACK_PX..=right + AIM_SLACK_PX).contains(&hit)
}

/// The module with a menu under the pointer.
fn menu_owner_at(window: &gtk4::Window, x: f64, y: f64) -> Option<gtk4::Widget> {
    let mut widget = window.pick(x, y, gtk4::PickFlags::DEFAULT);
    while let Some(current) = widget {
        if current.has_css_class("has-menu") {
            return Some(current);
        }
        widget = current.parent();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_into_menu_below() {
        let menu = graphene::Rect::new(100.0, 40.0, 200.0, 300.0);
        // Straight down onto the menu.
        assert!(heads_into((150.0, 10.0), (0.0, 6.0), &menu));
        // Diagonally towards it from a module on the right.
        assert!(heads_into((330.0, 10.0), (-8.0, 4.0), &menu));
        // Along the bar: switching should be instant.
        assert!(!heads_into((400.0, 10.0), (-8.0, 0.0), &menu));
        // Away from the menu.
        assert!(!heads_into((400.0, 10.0), (8.0, 4.0), &menu));
    }
}

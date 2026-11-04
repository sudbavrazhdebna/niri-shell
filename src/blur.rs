//! Which parts of the shell ask niri to blur what is behind them.
//!
//! GTK (4.24+) speaks the blur protocol itself (`ext-background-effect-v1`): it hands
//! niri the area of every widget whose CSS has `backdrop-filter: blur()` of 20px or more,
//! rounded corners, animations and all. style.css puts `var(--blur)` on every card; a
//! part that config turns off gets the window class `no-blur`, which sets `--blur: none`.
//! The protocol allows one blur object per surface and GTK owns it, so the shell must
//! never make its own: a second one is a protocol error that kills the connection.

use std::cell::RefCell;

use gtk4::glib;
use gtk4::prelude::*;

/// The parts of the shell that blur what is behind them; config's `blur { off … }` names
/// the ones that shouldn't.
pub const PARTS: &[&str] = &["bar", "menus", "notifications", "dock", "osd", "launcher"];

thread_local! {
    static OFF: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// Every window that is a part, to apply a new `OFF` to the ones that stay open.
    static LIVE: RefCell<Vec<(glib::WeakRef<gtk4::Window>, &'static str)>> = const { RefCell::new(Vec::new()) };
}

/// Sets which parts don't blur; windows already open follow at once.
pub fn configure(off: &[String]) {
    if OFF.with(|o| o.borrow().as_slice() == off) {
        return;
    }
    OFF.with(|o| o.replace(off.to_vec()));
    let live = LIVE.with(|l| {
        let mut l = l.borrow_mut();
        l.retain(|(window, _)| window.upgrade().is_some());
        l.clone()
    });
    for (window, part) in live {
        if let Some(window) = window.upgrade() {
            mark(&window, part);
        }
    }
}

/// Makes `window` one of [`PARTS`]: it gets the `no-blur` class while config turns that
/// part's blur off.
pub fn part(window: &impl IsA<gtk4::Window>, part: &'static str) {
    let window = window.upcast_ref::<gtk4::Window>();
    mark(window, part);
    LIVE.with(|l| {
        let mut l = l.borrow_mut();
        // Menus come and go: without this the list would grow with every one opened.
        l.retain(|(window, _)| window.upgrade().is_some());
        l.push((window.downgrade(), part));
    });
}

fn mark(window: &gtk4::Window, part: &str) {
    if OFF.with(|o| o.borrow().iter().any(|p| p == part)) {
        window.add_css_class("no-blur");
    } else {
        window.remove_css_class("no-blur");
    }
}

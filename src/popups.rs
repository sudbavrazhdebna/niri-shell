//! Notification popups: a column of cards top-right under the bar on the focused
//! monitor, newest on top. Each card leaves after its time; hovering keeps it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use gtk4::prelude::*;
use gtk4::glib;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::blur;
use crate::notification_card::card;
use crate::services::notifications::Notification;
use crate::services::Services;

struct Card {
    widget: gtk4::Box,
    /// What the card shows; a notification replaced under the same id gets a new card.
    shows: Rc<Notification>,
    hide: Option<glib::SourceId>,
}

thread_local! {
    /// The bar is at the bottom: popups show bottom-right, the newest lowest.
    static AT_BOTTOM: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Follows the bar's position (from the config). Takes effect with the next popup window.
pub fn set_bar_at_bottom(bottom: bool) {
    AT_BOTTOM.set(bottom);
}

/// Corner radius of `.notification` in style.css, for the blur shape.
const CARD_RADIUS: i32 = 12;

pub struct Popups {
    services: Rc<Services>,
    window: RefCell<Option<(gtk4::Window, gtk4::Box)>>,
    /// Sets the blur behind the cards again (while one slides).
    refresh_blur: RefCell<Option<Rc<dyn Fn()>>>,
    /// Cards on screen by notification id.
    cards: RefCell<HashMap<u32, Card>>,
}

impl Popups {
    pub fn start(services: Rc<Services>) -> Rc<Popups> {
        let popups = Rc::new(Popups {
            services: services.clone(),
            window: RefCell::new(None),
            refresh_blur: RefCell::new(None),
            cards: RefCell::new(HashMap::new()),
        });
        let weak = Rc::downgrade(&popups);
        services.notifications().popups.subscribe(move |ids| {
            let Some(popups) = weak.upgrade() else { return false };
            let ids = ids.clone();
            // Building cards touches the service again; do it outside its notification.
            glib::idle_add_local_once(move || popups.sync(&ids));
            true
        });
        popups
    }

    fn sync(self: &Rc<Self>, ids: &[u32]) {
        let server = self.services.notifications().clone();
        let list = server.list.get().clone();
        // Cards whose popup ended, or whose notification was replaced. Taken out first:
        // removing a widget under the pointer runs its leave handler right away, which
        // needs `cards` again.
        let gone: Vec<Card> = {
            let mut cards = self.cards.borrow_mut();
            let stale: Vec<u32> = cards
                .iter()
                .filter(|(id, card)| {
                    let current = list.iter().find(|n| n.id == **id);
                    !ids.contains(id) || !current.is_some_and(|n| Rc::ptr_eq(n, &card.shows))
                })
                .map(|(id, _)| *id)
                .collect();
            stale.iter().filter_map(|id| cards.remove(id)).collect()
        };
        for mut card in gone {
            if let Some(timer) = card.hide.take() {
                timer.remove();
            }
            self.leave(&card.widget);
        }
        if ids.is_empty() {
            // The window goes once the last card has slid out (`leave`).
            self.close_if_empty();
            return;
        }
        let Some(column) = self.column() else { return };

        // Each moved to the top in turn: oldest last, so the newest ends up first; at the
        // bottom, newest last, so it ends up last (next to the bar).
        let order: Vec<u32> = if AT_BOTTOM.get() { ids.to_vec() } else { ids.iter().rev().copied().collect() };
        for id in order {
            let Some(n) = list.iter().find(|n| n.id == id) else { continue };
            let existing = self.cards.borrow().get(&id).map(|c| c.widget.clone());
            let widget = match existing {
                Some(widget) => widget,
                None => {
                    // Slides in from the screen edge (style.css); the blur follows it.
                    let widget = card(n, &server, Rc::new(|| {}), false);
                    self.follow(&widget);
                    self.watch_hover(&widget, id, n.timeout_ms);
                    let hide = Some(self.hide_later(id, n.timeout_ms));
                    self.cards
                        .borrow_mut()
                        .insert(id, Card { widget: widget.clone(), shows: n.clone(), hide });
                    widget
                }
            };
            if widget.parent().is_some() {
                column.reorder_child_after(&widget, None::<&gtk4::Widget>);
            } else {
                column.prepend(&widget);
            }
        }
    }

    /// Slides a card out to the screen edge, then takes it away.
    fn leave(self: &Rc<Self>, widget: &gtk4::Box) {
        widget.add_css_class("leaving");
        widget.set_can_target(false);
        self.follow(widget);
        let (weak, widget) = (Rc::downgrade(self), widget.clone());
        glib::timeout_add_local_once(crate::ui::animation(0), move || {
            if let Some(parent) = widget.parent().and_downcast::<gtk4::Box>() {
                parent.remove(&widget);
            }
            let Some(popups) = weak.upgrade() else { return };
            if let Some((window, _)) = &*popups.window.borrow() {
                crate::ui::shrink_to_fit(window);
            }
            popups.close_if_empty();
        });
    }

    /// No cards left (none showing, none sliding out): the window goes.
    fn close_if_empty(&self) {
        let empty = self.cards.borrow().is_empty()
            && self.window.borrow().as_ref().is_none_or(|(_, column)| column.first_child().is_none());
        if empty && let Some((window, _)) = self.window.take() {
            self.refresh_blur.take();
            window.destroy();
        }
    }

    fn follow(&self, widget: &gtk4::Box) {
        if let Some(refresh) = &*self.refresh_blur.borrow() {
            crate::blur::follow(widget, refresh.clone(), crate::ui::ANIMATION_MS + 50);
        }
    }

    fn hide_later(self: &Rc<Self>, id: u32, ms: u32) -> glib::SourceId {
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(Duration::from_millis(u64::from(ms)), move || {
            if let Some(popups) = weak.upgrade() {
                if let Some(card) = popups.cards.borrow_mut().get_mut(&id) {
                    card.hide = None;
                }
                popups.services.notifications().hide_popup(id);
            }
        })
    }

    /// The pointer on a card stops its timer; leaving restarts it. Only while the card
    /// is still the one shown for `id`: a card sliding out after its notification was
    /// replaced (same id) must not touch the new card's timer.
    fn watch_hover(self: &Rc<Self>, card: &gtk4::Box, id: u32, ms: u32) {
        let motion = gtk4::EventControllerMotion::new();
        let weak = Rc::downgrade(self);
        let me = card.downgrade();
        motion.connect_enter(move |_, _, _| {
            if let Some(popups) = weak.upgrade()
                && let Some(card) = popups.cards.borrow_mut().get_mut(&id)
                && me.upgrade().is_some_and(|me| me == card.widget)
                && let Some(timer) = card.hide.take()
            {
                timer.remove();
            }
        });
        let weak = Rc::downgrade(self);
        let me = card.downgrade();
        motion.connect_leave(move |_| {
            let Some(popups) = weak.upgrade() else { return };
            let mut cards = popups.cards.borrow_mut();
            let Some(card) = cards.get_mut(&id).filter(|c| me.upgrade().is_some_and(|me| me == c.widget)) else {
                return;
            };
            if let Some(old) = card.hide.take() {
                old.remove();
            }
            drop(cards);
            let timer = popups.hide_later(id, ms.min(3000));
            match popups.cards.borrow_mut().get_mut(&id) {
                Some(card) => card.hide = Some(timer),
                None => timer.remove(),
            }
        });
        card.add_controller(motion);
    }

    /// The popup window, made on the focused monitor when the first card arrives.
    fn column(&self) -> Option<gtk4::Box> {
        if let Some((_, column)) = &*self.window.borrow() {
            return Some(column.clone());
        }
        let window = gtk4::Window::new();
        window.add_css_class("nshell-popups");
        window.init_layer_shell();
        window.set_namespace(Some("nshell-notifications"));
        window.set_layer(Layer::Overlay);
        window.set_monitor(self.services.niri.focused_monitor().as_ref());
        window.set_keyboard_mode(KeyboardMode::None);
        // Under a bar at the top; above one at the bottom (as Windows does).
        let bottom = AT_BOTTOM.get();
        window.set_anchor(if bottom { Edge::Bottom } else { Edge::Top }, true);
        if bottom {
            // style.css drops the shadow's room under the column, or it would add to
            // the gap above the bar.
            window.add_css_class("bottom");
        }
        window.set_anchor(Edge::Right, true);
        // Zero exclusive zone: clear of the bar, not over it.
        window.set_exclusive_zone(0);
        let column = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        column.add_css_class("popup-column");
        // Blur behind each card, following cards as they come and go.
        let column_weak = column.downgrade();
        let (wrapped, refresh_blur) = blur::behind_with_refresh(
            &window,
            "notifications",
            &column,
            move || {
                let mut cards = Vec::new();
                let mut child = column_weak.upgrade().and_then(|c| c.first_child());
                while let Some(card) = child {
                    child = card.next_sibling();
                    cards.push(card);
                }
                cards
            },
            CARD_RADIUS,
            || {},
        );
        window.set_child(Some(&wrapped));
        window.set_visible(true);
        self.refresh_blur.replace(Some(refresh_blur));
        self.window.replace(Some((window, column.clone())));
        Some(column)
    }
}

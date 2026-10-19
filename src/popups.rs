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

/// Corner radius of `.notification` in style.css, for the blur shape.
const CARD_RADIUS: i32 = 12;

pub struct Popups {
    services: Rc<Services>,
    window: RefCell<Option<(gtk4::Window, gtk4::Box)>>,
    /// Cards on screen by notification id.
    cards: RefCell<HashMap<u32, Card>>,
}

impl Popups {
    pub fn start(services: Rc<Services>) -> Rc<Popups> {
        let popups = Rc::new(Popups {
            services: services.clone(),
            window: RefCell::new(None),
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
            if let Some(parent) = card.widget.parent().and_downcast::<gtk4::Box>() {
                parent.remove(&card.widget);
            }
        }
        if let Some((window, _)) = &*self.window.borrow() {
            crate::ui::shrink_to_fit(window);
        }
        if ids.is_empty() {
            if let Some((window, _)) = self.window.take() {
                window.destroy();
            }
            return;
        }
        let Some(column) = self.column() else { return };

        // Oldest first, each moved to the top: the newest ends up first.
        for &id in ids.iter().rev() {
            let Some(n) = list.iter().find(|n| n.id == id) else { continue };
            let existing = self.cards.borrow().get(&id).map(|c| c.widget.clone());
            let widget = match existing {
                Some(widget) => widget,
                None => {
                    let widget = card(n, &server, Rc::new(|| {}), false);
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

    /// The pointer on a card stops its timer; leaving restarts it.
    fn watch_hover(self: &Rc<Self>, card: &gtk4::Box, id: u32, ms: u32) {
        let motion = gtk4::EventControllerMotion::new();
        let weak = Rc::downgrade(self);
        motion.connect_enter(move |_, _, _| {
            if let Some(popups) = weak.upgrade()
                && let Some(card) = popups.cards.borrow_mut().get_mut(&id)
                && let Some(timer) = card.hide.take()
            {
                timer.remove();
            }
        });
        let weak = Rc::downgrade(self);
        motion.connect_leave(move |_| {
            let Some(popups) = weak.upgrade() else { return };
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
        window.set_anchor(Edge::Top, true);
        window.set_anchor(Edge::Right, true);
        // Below the bar.
        window.set_exclusive_zone(0);
        let column = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        column.add_css_class("popup-column");
        // Blur behind each card, following cards as they come and go.
        let column_weak = column.downgrade();
        let wrapped = blur::behind(
            &window,
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
        self.window.replace(Some((window, column.clone())));
        Some(column)
    }
}

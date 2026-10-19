//! A value that widgets can follow: they get called now and on every change.

use std::cell::{Ref, RefCell};
use std::rc::Rc;

use gtk4::glib;
use gtk4::prelude::*;

struct Listener<T> {
    call: Box<dyn Fn(&T) -> bool>,
    /// For `bind`: the widget it updates. Once that is gone the listener is dropped at
    /// the next subscription, not only at the next change: a value that rarely changes
    /// (nothing playing, "stay awake") kept every closed menu's listeners and widgets.
    owner: Option<glib::WeakRef<glib::Object>>,
}

impl<T> Listener<T> {
    fn is_dead(&self) -> bool {
        self.owner.as_ref().is_some_and(|w| w.upgrade().is_none())
    }
}

pub struct Watch<T>(Rc<Inner<T>>);

struct Inner<T> {
    value: RefCell<T>,
    listeners: RefCell<Vec<Listener<T>>>,
}

impl<T> Clone for Watch<T> {
    fn clone(&self) -> Self {
        Watch(self.0.clone())
    }
}

impl<T: 'static> Watch<T> {
    pub fn new(value: T) -> Self {
        Watch(Rc::new(Inner {
            value: RefCell::new(value),
            listeners: RefCell::new(Vec::new()),
        }))
    }

    pub fn get(&self) -> Ref<'_, T> {
        self.0.value.borrow()
    }

    /// Calls `listener` now and after every change, for as long as it returns true.
    pub fn subscribe(&self, listener: impl Fn(&T) -> bool + 'static) {
        self.add(Box::new(listener), None);
    }

    fn add(&self, call: Box<dyn Fn(&T) -> bool>, owner: Option<glib::WeakRef<glib::Object>>) {
        if call(&self.get()) {
            let mut listeners = self.0.listeners.borrow_mut();
            listeners.retain(|l| !l.is_dead());
            listeners.push(Listener { call, owner });
        }
    }

    /// Like `subscribe`, for a widget: stops by itself once the widget is gone.
    pub fn bind<W: IsA<glib::Object>>(&self, widget: &W, update: impl Fn(&W, &T) + 'static) {
        let weak = widget.downgrade();
        let owner = widget.upcast_ref::<glib::Object>().downgrade();
        let call = move |value: &T| {
            let Some(widget) = weak.upgrade() else {
                return false;
            };
            update(&widget, value);
            true
        };
        self.add(Box::new(call), Some(owner));
    }

    /// Changes the value; listeners run only if `change` returns true.
    pub fn update(&self, change: impl FnOnce(&mut T) -> bool) {
        if change(&mut self.0.value.borrow_mut()) {
            self.notify();
        }
    }

    fn notify(&self) {
        let value = self.0.value.borrow();
        // A listener may subscribe others while running; they land in the emptied list.
        let listeners = std::mem::take(&mut *self.0.listeners.borrow_mut());
        let mut alive: Vec<_> = listeners.into_iter().filter(|l| (l.call)(&value)).collect();
        let mut listeners = self.0.listeners.borrow_mut();
        alive.append(&mut listeners);
        *listeners = alive;
    }
}

impl<T: PartialEq + 'static> Watch<T> {
    /// Replaces the value; listeners run only if it actually changed.
    pub fn set(&self, value: T) {
        self.update(|current| {
            if *current == value {
                return false;
            }
            *current = value;
            true
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bound_listeners_go_with_their_widget() {
        let watch = Watch::new(0);
        let owner = glib::Object::new::<glib::Object>();
        watch.bind(&owner, |_, _| {});
        assert_eq!(watch.0.listeners.borrow().len(), 1);
        drop(owner);
        // No change happened, yet a new subscription clears the dead one.
        watch.subscribe(|_| true);
        assert_eq!(watch.0.listeners.borrow().len(), 1);
    }
}

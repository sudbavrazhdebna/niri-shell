//! A value that widgets can follow: they get called now and on every change.

use std::cell::{Ref, RefCell};
use std::rc::Rc;

use gtk4::glib;
use gtk4::prelude::*;

type Listener<T> = Box<dyn Fn(&T) -> bool>;

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
        if listener(&self.get()) {
            self.0.listeners.borrow_mut().push(Box::new(listener));
        }
    }

    /// Like `subscribe`, for a widget: stops by itself once the widget is gone.
    pub fn bind<W: IsA<glib::Object>>(&self, widget: &W, update: impl Fn(&W, &T) + 'static) {
        let weak = widget.downgrade();
        self.subscribe(move |value| {
            let Some(widget) = weak.upgrade() else {
                return false;
            };
            update(&widget, value);
            true
        });
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
        let mut alive: Vec<_> = listeners.into_iter().filter(|l| l(&value)).collect();
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

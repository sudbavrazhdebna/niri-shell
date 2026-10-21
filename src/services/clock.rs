//! One clock for all bars: wakes up on the minute (or every second if a format shows
//! seconds) and tells every clock label to redraw.

use std::cell::Cell;
use std::rc::{Rc, Weak};
use std::time::Duration;

use gtk4::glib;

use crate::watch::Watch;

pub struct Clock {
    /// Bumped on every tick.
    pub tick: Watch<u64>,
    per_second: Cell<bool>,
    /// The timer of the next tick.
    next: Cell<Option<glib::SourceId>>,
}

impl Clock {
    pub fn start() -> Rc<Clock> {
        let clock = Rc::new(Clock { tick: Watch::new(0), per_second: Cell::new(false), next: Cell::new(None) });
        schedule(Rc::downgrade(&clock), false);
        clock
    }

    /// Ticks now and starts counting again: glib timers stop during suspend, so after
    /// waking up the clock lagged up to a minute.
    pub fn resync(self: &Rc<Self>) {
        if let Some(source) = self.next.take() {
            source.remove();
        }
        self.tick.update(|t| {
            *t += 1;
            true
        });
        schedule(Rc::downgrade(self), self.per_second.get());
    }

    /// Ticks every second from now on if `on` (a format with seconds), else every minute.
    pub fn set_per_second(self: &Rc<Self>, on: bool) {
        if self.per_second.replace(on) == on {
            return;
        }
        // Now, not after the minute already waited for: seconds stood still till then.
        if let Some(source) = self.next.take() {
            source.remove();
        }
        schedule(Rc::downgrade(self), on);
    }
}

fn schedule(clock: Weak<Clock>, per_second: bool) {
    let Some(this) = clock.upgrade() else { return };
    let Ok(now) = glib::DateTime::now_local() else { return };
    // Sleep until the next boundary, plus a little so we never land just before it.
    let ms_into_second = u64::from(now.microsecond().unsigned_abs() / 1000);
    let ms_left = if per_second {
        1000 - ms_into_second
    } else {
        (60 - u64::from(now.second().unsigned_abs())) * 1000 - ms_into_second
    };
    let source = glib::timeout_add_local_once(Duration::from_millis(ms_left + 20), move || {
        let Some(this) = clock.upgrade() else { return };
        this.next.set(None);
        this.tick.update(|t| {
            *t += 1;
            true
        });
        schedule(clock, this.per_second.get());
    });
    this.next.set(Some(source));
}

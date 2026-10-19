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
}

impl Clock {
    pub fn start() -> Rc<Clock> {
        let clock = Rc::new(Clock { tick: Watch::new(0), per_second: Cell::new(false) });
        schedule(Rc::downgrade(&clock), false);
        clock
    }

    /// Ticks every second from now on if `on` (a format with seconds), else every minute.
    pub fn set_per_second(&self, on: bool) {
        self.per_second.set(on);
    }
}

fn schedule(clock: Weak<Clock>, per_second: bool) {
    let Ok(now) = glib::DateTime::now_local() else { return };
    // Sleep until the next boundary, plus a little so we never land just before it.
    let ms_into_second = u64::from(now.microsecond().unsigned_abs() / 1000);
    let ms_left = if per_second {
        1000 - ms_into_second
    } else {
        (60 - u64::from(now.second().unsigned_abs())) * 1000 - ms_into_second
    };
    // TODO: glib timers stop during suspend, so after waking up the time can lag up to
    // a minute. Tick on logind PrepareForSleep(false) once logind is wired (idle step).
    glib::timeout_add_local_once(Duration::from_millis(ms_left + 20), move || {
        let Some(this) = clock.upgrade() else { return };
        this.tick.update(|t| {
            *t += 1;
            true
        });
        schedule(clock, this.per_second.get());
    });
}

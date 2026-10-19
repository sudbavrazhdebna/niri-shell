//! Clock: wakes up once a minute, on the minute (or every second if the format shows
//! seconds). Left click: calendar.

use std::time::Duration;

use gtk4::glib;
use gtk4::prelude::*;

use super::Context;
use crate::{menu, ui};

pub fn build(ctx: &Context) -> gtk4::Widget {
    let label = gtk4::Label::new(None);
    label.add_css_class("module");
    label.add_css_class("clock");

    let format = ctx.config.clock.format.clone();
    let per_second = format.contains("%S") || format.contains("%T") || format.contains("%r");
    tick(&label.downgrade(), format, per_second);

    menu::attach(&label, |anchor| {
        let column = ui::column();
        let today = glib::DateTime::now_local()
            .ok()
            .and_then(|now| now.format("%A, %e %B").ok())
            .map(String::from)
            .unwrap_or_default();
        column.append(&ui::title(today.trim()));
        let calendar = gtk4::Calendar::new();
        calendar.add_css_class("menu-calendar");
        column.append(&calendar);
        menu::show(anchor, &column);
    });
    label.upcast()
}

fn tick(label: &glib::WeakRef<gtk4::Label>, format: String, per_second: bool) {
    let Some(widget) = label.upgrade() else {
        return; // the bar is gone, stop ticking
    };
    let Ok(now) = glib::DateTime::now_local() else {
        return;
    };
    if let Ok(text) = now.format(&format) {
        widget.set_label(&text);
    }

    // Sleep until the next boundary, plus a little so we never land just before it.
    let ms_into_second = u64::from(now.microsecond().unsigned_abs() / 1000);
    let ms_left = if per_second {
        1000 - ms_into_second
    } else {
        (60 - u64::from(now.second().unsigned_abs())) * 1000 - ms_into_second
    };
    // TODO: glib timers stop during suspend, so after waking up the time can lag up to
    // a minute. Refresh on logind PrepareForSleep(false) once logind is wired (idle step).
    let label = label.clone();
    glib::timeout_add_local_once(Duration::from_millis(ms_left + 20), move || {
        tick(&label, format, per_second);
    });
}

//! Bar modules. Each one builds its widget for one bar (one monitor) and keeps it updated.

mod clock;
mod window_title;

use std::rc::Rc;

use crate::config::Config;
use crate::niri::Niri;

/// What a module gets when it is built.
pub struct Context {
    pub config: Rc<Config>,
    pub niri: Niri,
    /// niri output name of the bar's monitor (e.g. "eDP-1").
    pub output: String,
}

pub fn build(name: &str, ctx: &Context) -> Option<gtk4::Widget> {
    match name {
        "clock" => Some(clock::build(ctx)),
        "window-title" => Some(window_title::build(ctx)),
        // Accepted by the config, not written yet.
        _ => None,
    }
}

//! Displays as niri sees them (connected ones, on or off), and the rule that the laptop's
//! own screen goes off while an external monitor is on and back on when the last one
//! goes (`displays { laptop-screen "auto" }`). The rule follows GDK's monitor list, which
//! changes whenever niri enables, disables or loses an output; it acts only when an
//! external monitor comes or goes, so turning a screen on or off by hand sticks.

use std::cell::Cell;

use gtk4::gdk;
use gtk4::prelude::*;
use niri_ipc::{OutputAction, Request, Response};

thread_local! {
    static AUTO: Cell<bool> = const { Cell::new(false) };
    static FOLLOWING: Cell<bool> = const { Cell::new(false) };
    /// Whether an external monitor was on when the rule last looked.
    static EXTERNAL_ON: Cell<Option<bool>> = const { Cell::new(None) };
}

/// A connected display.
#[derive(Clone, Debug)]
pub struct Output {
    /// Connector, e.g. "eDP-1".
    pub name: String,
    /// "Built-in display", or maker and model ("Samsung LF24T450F").
    pub label: String,
    pub on: bool,
    pub internal: bool,
}

/// Every connected display, on or off, built-in ones first.
pub async fn outputs() -> Vec<Output> {
    let outputs = match crate::niri::request(Request::Outputs).await {
        Ok(Response::Outputs(outputs)) => outputs,
        Ok(_) => return Vec::new(),
        Err(err) => {
            tracing::warn!("displays: {err}");
            return Vec::new();
        }
    };
    let mut list: Vec<Output> = outputs
        .into_values()
        .map(|o| {
            let internal = is_internal(&o.name);
            let maker = o.make.split_whitespace().next().unwrap_or_default();
            let label = if internal {
                "Built-in display".to_string()
            } else {
                Some(format!("{maker} {}", o.model).trim().to_string()).filter(|l| !l.is_empty()).unwrap_or_else(|| o.name.clone())
            };
            Output { on: o.logical.is_some(), internal, label, name: o.name }
        })
        .collect();
    list.sort_by(|a, b| b.internal.cmp(&a.internal).then(a.name.cmp(&b.name)));
    list
}

/// Turns a display on or off.
pub async fn set_on(name: &str, on: bool) {
    let action = if on { OutputAction::On } else { OutputAction::Off };
    if let Err(err) = crate::niri::request(Request::Output { output: name.to_string(), action }).await {
        tracing::warn!("displays: {name}: {err}");
    }
}

/// Turns the rule on or off (at start and on config reload).
pub fn configure(auto: bool) {
    AUTO.set(auto);
    if auto
        && !FOLLOWING.replace(true)
        && let Some(display) = gdk::Display::default()
    {
        display.monitors().connect_items_changed(|_, _, _, _| {
            gtk4::glib::spawn_future_local(apply());
        });
    }
    if auto {
        gtk4::glib::spawn_future_local(apply());
    }
}

/// Built-in panels by connector name.
pub fn is_internal(name: &str) -> bool {
    ["eDP", "LVDS", "DSI"].iter().any(|prefix| name.starts_with(prefix))
}

/// What to switch: (output, on). Internal screens are on exactly when no external one
/// is on; external ones are never touched.
fn plan(outputs: &[(String, bool)]) -> Vec<(String, bool)> {
    let external_on = outputs.iter().any(|(name, on)| *on && !is_internal(name));
    outputs
        .iter()
        .filter(|(name, on)| is_internal(name) && *on == external_on)
        .map(|(name, _)| (name.clone(), !external_on))
        .collect()
}

async fn apply() {
    if !AUTO.get() {
        return;
    }
    let outputs = match crate::niri::request(Request::Outputs).await {
        Ok(Response::Outputs(outputs)) => outputs,
        Ok(_) => return,
        Err(err) => {
            tracing::warn!("displays: {err}");
            return;
        }
    };
    let state: Vec<(String, bool)> = outputs.values().map(|o| (o.name.clone(), o.logical.is_some())).collect();
    // Only when an external monitor came or went: a screen switched by hand stays so.
    let external_on = state.iter().any(|(name, on)| *on && !is_internal(name));
    if EXTERNAL_ON.replace(Some(external_on)) == Some(external_on) {
        return;
    }
    for (name, on) in plan(&state) {
        tracing::info!(output = name, on, "displays: laptop screen");
        set_on(&name, on).await;
    }
}

#[cfg(test)]
mod tests {
    use super::plan;

    fn outs(list: &[(&str, bool)]) -> Vec<(String, bool)> {
        list.iter().map(|(n, on)| (n.to_string(), *on)).collect()
    }

    #[test]
    fn laptop_screen_follows_external_ones() {
        // External plugged in: the panel goes off.
        assert_eq!(plan(&outs(&[("eDP-1", true), ("HDMI-A-1", true)])), outs(&[("eDP-1", false)]));
        // Already off: nothing to do.
        assert!(plan(&outs(&[("eDP-1", false), ("HDMI-A-1", true)])).is_empty());
        // External gone (or turned off): the panel comes back.
        assert_eq!(plan(&outs(&[("eDP-1", false)])), outs(&[("eDP-1", true)]));
        assert_eq!(plan(&outs(&[("eDP-1", false), ("DP-2", false)])), outs(&[("eDP-1", true)]));
        // No panel at all (a desktop): nothing.
        assert!(plan(&outs(&[("DP-1", true), ("HDMI-A-1", true)])).is_empty());
    }
}

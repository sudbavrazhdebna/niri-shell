//! Displays as niri sees them (connected ones, on or off), and the rule that the laptop's
//! own screen goes off while an external monitor is on and back on when the last one
//! goes (`displays { laptop-screen "auto" }`). The rule follows GDK's monitor list, which
//! changes whenever niri enables, disables or loses an output; it acts only when an
//! external monitor comes or goes, so turning a screen on or off by hand sticks.

use std::cell::{Cell, OnceCell};
use std::fs::File;
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd};
use std::time::Duration;

use gtk4::prelude::*;
use gtk4::{gdk, glib};
use niri_ipc::{OutputAction, Request, Response};

use crate::watch::Watch;

thread_local! {
    static AUTO: Cell<bool> = const { Cell::new(false) };
    static FOLLOWING: Cell<bool> = const { Cell::new(false) };
    /// Whether an external monitor was on when the rule last looked.
    static EXTERNAL_ON: Cell<Option<bool>> = const { Cell::new(None) };
    static MULTIPLE: OnceCell<Watch<bool>> = const { OnceCell::new() };
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
    MULTIPLE.with(|multiple| {
        if let Some(multiple) = multiple.get() {
            multiple.set(outputs.len() > 1);
        }
    });
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

/// Whether niri lists multiple connected displays, including disabled ones.
pub fn multiple() -> Watch<bool> {
    MULTIPLE.with(|state| {
        state.get_or_init(|| {
            if let Some(display) = gdk::Display::default() {
                display.monitors().connect_items_changed(|_, _, _, _| refresh_count());
            }
            if let Err(err) = watch_hotplug() {
                tracing::debug!(%err, "displays: DRM hotplug unavailable");
            }
            refresh_count();
            Watch::new(false)
        }).clone()
    })
}

fn refresh_count() {
    glib::spawn_future_local(async { outputs().await });
}

/// GDK only sees enabled outputs. A disabled monitor can be unplugged without a
/// change to its monitor list, so also listen for DRM hotplug events from Linux.
fn watch_hotplug() -> std::io::Result<()> {
    // SAFETY: socket creates a new descriptor, owned by File on success.
    let fd = unsafe {
        libc::socket(libc::AF_NETLINK, libc::SOCK_DGRAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC, libc::NETLINK_KOBJECT_UEVENT)
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: the descriptor is valid and has no other owner.
    let mut socket = unsafe { File::from_raw_fd(fd) };
    // SAFETY: zero describes an unspecified netlink address, including its padding.
    let mut address: libc::sockaddr_nl = unsafe { std::mem::zeroed() };
    address.nl_family = libc::AF_NETLINK as libc::sa_family_t;
    address.nl_groups = 1; // Kernel uevents.
    // SAFETY: address is initialized and the length matches its type.
    let bound = unsafe {
        libc::bind(socket.as_raw_fd(), (&raw const address).cast(), size_of_val(&address) as libc::socklen_t)
    };
    if bound < 0 {
        return Err(std::io::Error::last_os_error());
    }
    glib_unix::unix_fd_add_local(fd, glib::IOCondition::IN, move |_, condition| {
        if condition.intersects(glib::IOCondition::HUP | glib::IOCondition::NVAL) {
            return glib::ControlFlow::Break;
        }
        let mut buffer = [0; 4096];
        let mut changed = false;
        while let Ok(length) = socket.read(&mut buffer) {
            if length == 0 {
                break;
            }
            let fields = &buffer[..length];
            let has = |key: &[u8]| fields.split(|b| *b == 0).any(|field| field == key);
            changed |= has(b"SUBSYSTEM=drm") && has(b"HOTPLUG=1");
        }
        if changed {
            // Let niri process the same hotplug before asking for its output list.
            glib::timeout_add_local_once(Duration::from_millis(150), refresh_count);
        }
        glib::ControlFlow::Continue
    });
    Ok(())
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

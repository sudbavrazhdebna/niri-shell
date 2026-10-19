//! Starting apps and commands so that they outlive the shell.
//!
//! When the shell runs as a systemd user service, everything it spawns sits in the
//! service's cgroup, and stopping or restarting the service kills it all (ironbar took
//! its apps down this way). Each started process is moved into its own transient scope
//! under app.slice, as GNOME does.

use gio_unix::DesktopAppInfo;
use gio_unix::prelude::*;
use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};

/// Launches a desktop entry (optionally with files or URLs).
pub fn app(info: &DesktopAppInfo) {
    let context = gdk::Display::default().map(|d| d.app_launch_context());
    let mut pids = Vec::new();
    let result = info.launch_uris_as_manager_with_fds(
        &[],
        context.as_ref(),
        glib::SpawnFlags::SEARCH_PATH,
        None,
        Some(&mut |_: &DesktopAppInfo, pid: glib::Pid| pids.push(pid)),
        None::<std::os::fd::BorrowedFd>,
        None::<std::os::fd::BorrowedFd>,
        None::<std::os::fd::BorrowedFd>,
    );
    let name = info.id().map(String::from).unwrap_or_default();
    match result {
        Ok(()) => {
            for pid in pids {
                own_scope(&name, pid.0);
            }
        }
        Err(err) => tracing::warn!("can't start {name}: {err}"),
    }
}

/// Runs a command line (no shell), e.g. a settings app or the lock command.
pub fn command(command: &str) {
    let argv = match glib::shell_parse_argv(command) {
        Ok(argv) => argv,
        Err(err) => {
            tracing::warn!("bad command {command:?}: {err}");
            return;
        }
    };
    let Some((program, args)) = argv.split_first() else { return };
    match std::process::Command::new(program).args(args).spawn() {
        Ok(child) => {
            let pid = i32::try_from(child.id()).unwrap_or(0);
            // Reap it when it exits, or it lingers as a zombie.
            glib::child_watch_add_local(glib::Pid(pid), |_, _| {});
            own_scope(&program.to_string_lossy(), pid);
        }
        Err(err) => tracing::warn!("can't start {command:?}: {err}"),
    }
}

/// Moves `pid` into a new transient scope through the systemd user manager.
fn own_scope(name: &str, pid: i32) {
    let Ok(pid) = u32::try_from(pid) else { return };
    let safe: String = name
        .trim_end_matches(".desktop")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' })
        .collect();
    let unit = format!("app-nshell-{safe}-{pid}.scope");
    let props = vec![
        ("PIDs".to_string(), vec![pid].to_variant()),
        ("Slice".to_string(), "app.slice".to_variant()),
        ("CollectMode".to_string(), "inactive-or-failed".to_variant()),
    ];
    let aux: Vec<(String, Vec<(String, glib::Variant)>)> = Vec::new();
    let args = (unit.as_str(), "fail", props, aux).to_variant();
    glib::spawn_future_local(async move {
        let Ok(bus) = gio::bus_get_future(gio::BusType::Session).await else { return };
        let result = bus
            .call_future(
                Some("org.freedesktop.systemd1"),
                "/org/freedesktop/systemd1",
                "org.freedesktop.systemd1.Manager",
                "StartTransientUnit",
                Some(&args),
                None,
                gio::DBusCallFlags::NONE,
                -1,
            )
            .await;
        // Without a systemd user manager (e.g. the sandbox's own bus) the app just stays
        // where it is.
        if let Err(err) = result {
            tracing::debug!("no scope for {unit}: {err}");
        }
    });
}

//! Starting apps and commands so that they outlive the shell.
//!
//! When the shell runs as a systemd user service, everything it spawns sits in the
//! service's cgroup, and stopping or restarting the service kills it all (ironbar took
//! its apps down this way). Each started process is moved into its own transient scope
//! under app.slice, as GNOME does.

use std::cell::RefCell;

use gio_unix::DesktopAppInfo;
use gio_unix::prelude::*;
use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};

thread_local! {
    /// Environment variables the shell set for itself (renderer tweaks). Apps it starts
    /// must not get them: GSK_DEBUG=full-redraw would make every GTK app redraw its
    /// whole window on each frame.
    static SHELL_ONLY_ENV: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
}

/// Marks a variable the shell set for itself; started apps won't inherit it.
pub fn shell_only_env(name: &'static str) {
    SHELL_ONLY_ENV.with_borrow_mut(|names| names.push(name));
}

/// A launch context on the shell's display, without the shell-only variables.
fn launch_context() -> Option<gdk::AppLaunchContext> {
    let context = gdk::Display::default()?.app_launch_context();
    SHELL_ONLY_ENV.with_borrow(|names| {
        for name in names {
            context.unsetenv(name);
        }
    });
    Some(context)
}

/// Launches a desktop entry.
pub fn app(info: &DesktopAppInfo) {
    app_with(info, &[]);
}

/// Opens a file in the app set for its type, started like any other app.
pub fn open_file(path: &std::path::Path) {
    let file = gio::File::for_path(path);
    // A folder's type can't be told from its name.
    let content_type = if path.is_dir() {
        glib::GString::from("inode/directory")
    } else {
        gio::content_type_guess(Some(path), None::<&[u8]>).0
    };
    let app = gio::AppInfo::default_for_type(&content_type, false)
        .and_then(|info| info.downcast::<DesktopAppInfo>().ok());
    match app {
        Some(app) => app_with(&app, &[file.uri().as_str()]),
        None => tracing::warn!("no app opens {}", path.display()),
    }
}

/// Launches a desktop entry with files or URLs.
fn app_with(info: &DesktopAppInfo, uris: &[&str]) {
    let context = launch_context();
    let mut pids = Vec::new();
    let result = info.launch_uris_as_manager_with_fds(
        uris,
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

/// Opens a new window of an app. Many apps run one process and answer a plain launch by
/// raising the window they have (Nautilus, Sublime Text): their desktop entry has a
/// "new window" action for this, which GNOME uses too.
pub fn new_window(info: &DesktopAppInfo) {
    let actions = info.list_actions();
    let new_window = actions
        .iter()
        .find(|a| ["new-window", "NewWindow", "new_window", "window-new"].contains(&a.as_str()));
    match new_window {
        Some(name) => action(info, name),
        None => app(info),
    }
}

/// Runs one of the actions a desktop entry lists ("New Private Window" and the like).
/// The process gets its own scope like any other launch; GIO tells its pid only through
/// the context's "launched" signal (an app activated over D-Bus starts none).
pub fn action(info: &DesktopAppInfo, name: &str) {
    let context = launch_context();
    if let Some(context) = &context {
        let id = info.id().map(String::from).unwrap_or_default();
        context.connect_launched(move |_, _, platform_data| {
            let pid = crate::services::dbus::lookup(platform_data, "pid").and_then(|v| v.get::<i32>());
            if let Some(pid) = pid {
                own_scope(&id, pid);
            }
        });
    }
    info.launch_action(name, context.as_ref());
}

/// Runs a command line (no shell), e.g. a settings app or the lock command.
pub fn command(command: &str) {
    command_then(command, || {});
}

/// Like [`command`], and runs `done` when it exits (or at once if it can't start).
pub fn command_then(command: &str, done: impl FnOnce() + 'static) {
    let argv = match glib::shell_parse_argv(command) {
        Ok(argv) => argv,
        Err(err) => {
            tracing::warn!("bad command {command:?}: {err}");
            done();
            return;
        }
    };
    let Some((program, args)) = argv.split_first() else {
        done();
        return;
    };
    let mut child = std::process::Command::new(program);
    child.args(args);
    SHELL_ONLY_ENV.with_borrow(|names| {
        for name in names {
            child.env_remove(name);
        }
    });
    match child.spawn() {
        Ok(child) => {
            let pid = i32::try_from(child.id()).unwrap_or(0);
            // Reap it when it exits, or it lingers as a zombie.
            let done = std::cell::Cell::new(Some(done));
            glib::child_watch_add_local(glib::Pid(pid), move |_, _| {
                if let Some(done) = done.take() {
                    done();
                }
            });
            own_scope(&program.to_string_lossy(), pid);
        }
        Err(err) => {
            tracing::warn!("can't start {command:?}: {err}");
            done();
        }
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

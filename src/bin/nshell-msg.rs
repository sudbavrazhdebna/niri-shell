//! Commands for the running shell, to bind to keys in niri:
//!
//!     nshell-msg launcher
//!     nshell-msg show-dock [seconds]
//!
//! The same as `nshell msg …`, as a binary of its own: it loads only GLib and GIO, not
//! GTK and everything the shell links, so a key press reaches the shell in ~10 ms
//! instead of ~90.

use gio::prelude::*;

const NAME: &str = "dev.nshell.Shell";
const PATH: &str = "/dev/nshell/Shell";
const USAGE: &str = "usage: nshell-msg launcher | nshell-msg show-dock [seconds]";

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let call = match args.as_slice() {
        [cmd] if cmd == "launcher" => ("ToggleLauncher", ().to_variant()),
        [cmd] if cmd == "show-dock" => ("ShowDock", (3u32,).to_variant()),
        [cmd, secs] if cmd == "show-dock" => match secs.parse::<u32>() {
            Ok(secs) => ("ShowDock", (secs,).to_variant()),
            Err(_) => {
                eprintln!("{USAGE}");
                return std::process::ExitCode::FAILURE;
            }
        },
        _ => {
            eprintln!("{USAGE}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let result = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).and_then(|bus| {
        bus.call_sync(Some(NAME), PATH, NAME, call.0, Some(&call.1), None, gio::DBusCallFlags::NONE, 2000, gio::Cancellable::NONE)
    });
    match result {
        Ok(_) => std::process::ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("nshell isn't answering: {err}");
            std::process::ExitCode::FAILURE
        }
    }
}

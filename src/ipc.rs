//! `nshell msg <command>`: commands for the running shell over D-Bus, to bind to keys in
//! niri (e.g. `Mod+A { spawn "nshell" "msg" "show-dock"; }`, `Mod+D { spawn "nshell" "msg"
//! "launcher"; }`).

use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gio, glib};

use crate::services::Services;

const NAME: &str = "dev.nshell.Shell";
const PATH: &str = "/dev/nshell/Shell";
const XML: &str = r#"
<node>
  <interface name="dev.nshell.Shell">
    <method name="ShowDock"><arg name="seconds" type="u" direction="in"/></method>
    <method name="ToggleLauncher"/>
  </interface>
</node>"#;

/// Answers commands from `nshell msg` in the running shell.
pub fn serve(services: Rc<Services>) {
    let _owner = gio::bus_own_name(
        gio::BusType::Session,
        NAME,
        gio::BusNameOwnerFlags::NONE,
        move |conn, _| {
            let info = gio::DBusNodeInfo::for_xml(XML).ok().and_then(|n| n.lookup_interface(NAME));
            let Some(info) = info else { return };
            let services = services.clone();
            let result = conn
                .register_object(PATH, &info)
                .method_call(move |_, _, _, _, method, params, invocation| match method {
                    "ShowDock" => {
                        let seconds = params.get::<(u32,)>().map_or(3, |(s,)| s);
                        crate::dock::peek(&services.niri, seconds);
                        invocation.return_value(None);
                    }
                    "ToggleLauncher" => {
                        crate::launcher::toggle(&services);
                        invocation.return_value(None);
                    }
                    _ => invocation.return_error(gio::IOErrorEnum::NotSupported, "unknown command"),
                })
                .build();
            if let Err(err) = result {
                tracing::error!("ipc: {err}");
            }
        },
        |_, _| {},
        |_, _| tracing::warn!("ipc: another nshell already answers {NAME}"),
    );
}

/// The client side: `nshell msg show-dock [seconds]`, `nshell msg launcher`. Returns the
/// process exit code.
pub fn client(args: &[String]) -> glib::ExitCode {
    let usage = "usage: nshell msg show-dock [seconds] | nshell msg launcher";
    let call = match args {
        [cmd] if cmd == "launcher" => ("ToggleLauncher", ().to_variant()),
        [cmd] if cmd == "show-dock" => ("ShowDock", (3u32,).to_variant()),
        [cmd, secs] if cmd == "show-dock" => match secs.parse::<u32>() {
            Ok(secs) => ("ShowDock", (secs,).to_variant()),
            Err(_) => {
                eprintln!("{usage}");
                return glib::ExitCode::FAILURE;
            }
        },
        _ => {
            eprintln!("{usage}");
            return glib::ExitCode::FAILURE;
        }
    };
    let result = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).and_then(|bus| {
        bus.call_sync(
            Some(NAME),
            PATH,
            NAME,
            call.0,
            Some(&call.1),
            None,
            gio::DBusCallFlags::NONE,
            2000,
            gio::Cancellable::NONE,
        )
    });
    match result {
        Ok(_) => glib::ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("nshell isn't answering: {err}");
            glib::ExitCode::FAILURE
        }
    }
}

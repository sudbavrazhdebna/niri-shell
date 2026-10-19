mod apps;
mod bar;
mod config;
mod modules;
mod niri;

use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};
use tracing_subscriber::EnvFilter;

const APP_ID: &str = "dev.nshell.Shell";

fn main() -> glib::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("NSHELL_LOG").unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let app = gtk4::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_startup(|_| load_css());
    app.connect_activate(|app| {
        // Without this the app quits as soon as the last bar goes away
        // (e.g. all monitors unplugged).
        std::mem::forget(app.hold());
        let config = Rc::new(config::Config::default());
        let niri = niri::Niri::connect();
        bar::Bars::start(app, config, niri);
    });

    app.run_with_args::<&str>(&[])
}

fn load_css() {
    let Some(display) = gdk::Display::default() else {
        tracing::error!("no display");
        return;
    };
    let provider = gtk4::CssProvider::new();
    provider.load_from_string(include_str!("style.css"));
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

mod apps;
mod bar;
mod blur;
mod config;
mod menu;
mod modules;
mod niri;
mod services;
mod settings;
mod ui;
mod watch;

use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};
use tracing_subscriber::EnvFilter;

const APP_ID: &str = "dev.nshell.Shell";

fn main() -> glib::ExitCode {
    if std::env::var_os("GSK_RENDERER").is_none() {
        // GL: least CPU when text changes often (measured in the wayle fork), and GTK's
        // default Vulkan renderer was seen stalling for minutes while opening a bar window.
        // SAFETY: nothing else runs yet, so no thread can read the environment concurrently.
        unsafe { std::env::set_var("GSK_RENDERER", "gl") };
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("NSHELL_LOG").unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let app = gtk4::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_startup(|_| {
        load_css();
        load_icons();
    });
    app.connect_activate(|app| {
        // Without this the app quits as soon as the last bar goes away
        // (e.g. all monitors unplugged).
        std::mem::forget(app.hold());
        let (config, problem) = settings::Settings::initial_config();
        let services = services::Services::new(niri::Niri::connect());
        let bars = bar::Bars::start(app, config.clone(), services);
        // Lives as long as the app: it holds the file watcher.
        std::mem::forget(settings::Settings::start(bars, &config, problem));
    });

    app.run_with_args::<&str>(&[])
}

fn load_css() {
    let Some(display) = gdk::Display::default() else {
        tracing::error!("no display");
        return;
    };
    // The shell looks the same whatever GTK theme apps use: built-in Adwaita, dark.
    let settings = gtk4::Settings::for_display(&display);
    settings.set_gtk_theme_name(Some("Adwaita"));
    settings.set_gtk_application_prefer_dark_theme(true);
    let provider = gtk4::CssProvider::new();
    provider.load_from_string(include_str!("style.css"));
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

fn load_icons() {
    gio::resources_register_include!("icons.gresource").unwrap_or_else(|err| {
        tracing::error!("bundled icons: {err}");
    });
    if let Some(display) = gdk::Display::default() {
        gtk4::IconTheme::for_display(&display).add_resource_path("/dev/nshell/icons");
    }
}

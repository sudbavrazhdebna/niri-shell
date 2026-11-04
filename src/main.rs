mod apps;
mod bar;
mod blur;
mod config;
mod displays;
mod dock;
mod images;
mod ipc;
mod launch;
mod launcher;
mod menu;
mod modules;
mod niri;
mod notification_card;
mod osd;
mod popups;
mod services;
mod settings;
mod ui;
mod wallpaper;
mod watch;
mod wayland;

use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};
use tracing_subscriber::EnvFilter;

const APP_ID: &str = "dev.nshell.Shell";

fn main() -> glib::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "msg") {
        return ipc::client(&args[1..]);
    }
    if std::env::var_os("GSK_RENDERER").is_none() {
        // GL: least CPU when text changes often (measured in the wayle fork), and GTK's
        // default Vulkan renderer was seen stalling for minutes while opening a bar window.
        // SAFETY: nothing else runs yet, so no thread can read the environment concurrently.
        unsafe { std::env::set_var("GSK_RENDERER", "gl") };
        launch::shell_only_env("GSK_RENDERER");
    }
    if std::env::var_os("GSK_DEBUG").is_none() {
        // GTK 4.22's partial redraws leave traces of the old frame where something shrinks
        // or moves (the OSD bar, changing numbers); redrawing whole surfaces fixes it.
        // SAFETY: as above, still single-threaded.
        unsafe { std::env::set_var("GSK_DEBUG", "full-redraw") };
        launch::shell_only_env("GSK_DEBUG");
    }
    if std::env::var_os("mesa_glthread").is_none() {
        // Mesa's GL worker thread buys nothing for a few small surfaces; two threads less.
        // SAFETY: as above, still single-threaded.
        unsafe { std::env::set_var("mesa_glthread", "false") };
        launch::shell_only_env("mesa_glthread");
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
        menu::close_on_focus_change(&services.niri);
        services.idle().configure(&config.idle);
        displays::configure(config.laptop_screen_auto);
        wallpaper::configure(config.wallpaper.clone());
        wallpaper::follow_overview(&services.niri);
        osd::set_layout_rename(&config.layout_rename);
        launcher::configure(&config.launcher_paths);
        popups::set_bar_at_bottom(config.bar.bottom);
        blur::configure(&config.blur_off);
        let bars = bar::Bars::start(app, config.clone(), services.clone());
        // Lives as long as the app: the services hold its subscriptions.
        std::mem::forget(osd::Osd::start(services.clone()));
        std::mem::forget(popups::Popups::start(services.clone()));
        ipc::serve(services);
        // Bind the Wayland extras now rather than on the first menu (a noticeable pause).
        glib::idle_add_local_once(|| {
            wayland::with(|_| Some(()));
        });
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

//! Loads config.kdl and style.css from ~/.config/nshell and applies changes on save.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};

use crate::bar::Bars;
use crate::config::{self, Config};

/// Editors save in several steps (write, rename, chmod); wait for them to finish.
const SETTLE: Duration = Duration::from_millis(150);

pub struct Settings {
    bars: Rc<Bars>,
    user_css: gtk4::CssProvider,
    config_problem: RefCell<Option<String>>,
    style_problem: RefCell<Option<String>>,
    reload_pending: Cell<bool>,
    /// Kept alive: dropping the monitor stops watching.
    _monitor: Option<gio::FileMonitor>,
}

impl Settings {
    /// Loads the config (falling back to defaults on errors) before the bars exist.
    pub fn initial_config() -> (Rc<Config>, Option<String>) {
        match Config::load() {
            Ok(config) => (Rc::new(config), None),
            Err(err) => {
                tracing::error!("config: {err}");
                (Rc::new(Config::default()), Some(err))
            }
        }
    }

    pub fn start(bars: Rc<Bars>, config: &Config, config_problem: Option<String>) -> Rc<Settings> {
        apply_icon_theme(config);

        let user_css = gtk4::CssProvider::new();
        if let Some(display) = gdk::Display::default() {
            gtk4::style_context_add_provider_for_display(
                &display,
                &user_css,
                gtk4::STYLE_PROVIDER_PRIORITY_USER,
            );
        }

        let dir = gio::File::for_path(config::dir());
        let monitor = dir
            .monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
            .inspect_err(|err| tracing::warn!("can't watch {}: {err}", config::dir().display()))
            .ok();

        let settings = Rc::new(Settings {
            bars,
            user_css,
            config_problem: RefCell::new(config_problem),
            style_problem: RefCell::new(None),
            reload_pending: Cell::new(false),
            _monitor: monitor.clone(),
        });

        let this = settings.clone();
        settings.user_css.connect_parsing_error(move |_, section, err| {
            let line = section.start_location().lines() + 1;
            this.style_problem
                .replace(Some(format!("style.css, строка {line}: {err}")));
        });

        settings.load_style();
        settings.show_problems();

        if let Some(monitor) = monitor {
            let weak = Rc::downgrade(&settings);
            monitor.connect_changed(move |_, file, other, _| {
                let touches = |f: Option<&gio::File>| {
                    f.and_then(|f| f.basename())
                        .is_some_and(|name| name.as_os_str() == "config.kdl" || name.as_os_str() == "style.css")
                };
                if (touches(Some(file)) || touches(other))
                    && let Some(this) = weak.upgrade()
                {
                    this.schedule_reload();
                }
            });
        }
        settings
    }

    fn schedule_reload(self: &Rc<Self>) {
        if self.reload_pending.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(SETTLE, move || {
            if let Some(this) = weak.upgrade() {
                this.reload_pending.set(false);
                this.reload();
            }
        });
    }

    fn reload(&self) {
        self.load_style();
        match Config::load() {
            Ok(config) => {
                tracing::info!("config reloaded");
                self.config_problem.replace(None);
                apply_icon_theme(&config);
                self.bars.set_config(Rc::new(config), self.problems());
            }
            Err(err) => {
                tracing::error!("config: {err}");
                self.config_problem.replace(Some(err));
                self.show_problems();
            }
        }
    }

    fn load_style(&self) {
        self.style_problem.replace(None);
        let path = config::dir().join("style.css");
        let css = std::fs::read_to_string(&path).unwrap_or_default();
        // Parsing errors arrive through connect_parsing_error during this call.
        self.user_css.load_from_string(&css);
        if let Some(err) = &*self.style_problem.borrow() {
            tracing::error!("{err}");
        }
    }

    fn problems(&self) -> Vec<String> {
        let config = self
            .config_problem
            .borrow()
            .as_ref()
            .map(|e| format!("config.kdl, {e}\nПрименены прошлые настройки."));
        config
            .into_iter()
            .chain(self.style_problem.borrow().clone())
            .collect()
    }

    fn show_problems(&self) {
        self.bars.set_problems(self.problems());
    }
}

fn apply_icon_theme(config: &Config) {
    let Some(settings) = gtk4::Settings::default() else {
        return;
    };
    let current = settings.gtk_icon_theme_name().map(String::from);
    match &config.icon_theme {
        Some(name) if current.as_ref() != Some(name) => {
            settings.set_gtk_icon_theme_name(Some(name));
            crate::apps::clear_cache();
        }
        Some(_) => {}
        None => {
            // Back to whatever the system says.
            settings.reset_property("gtk-icon-theme-name");
            if settings.gtk_icon_theme_name().map(String::from) != current {
                crate::apps::clear_cache();
            }
        }
    }
}

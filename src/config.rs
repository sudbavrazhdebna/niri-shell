//! Settings. Built-in defaults for now; step 2 reads them from ~/.config/nshell/config.kdl.

use regex::Regex;

pub struct Config {
    pub bar: BarConfig,
    pub clock: ClockConfig,
    pub window_title: WindowTitleConfig,
}

pub struct BarConfig {
    /// Height in pixels; content is centered vertically.
    pub height: i32,
    pub left: Vec<String>,
    pub center: Vec<String>,
    pub right: Vec<String>,
}

pub struct ClockConfig {
    /// strftime-like format, see `glib::DateTime::format`.
    pub format: String,
}

pub struct WindowTitleConfig {
    /// Applied in order; the first matching pattern wins.
    pub rewrite: Vec<(Regex, String)>,
}

impl Default for Config {
    fn default() -> Self {
        let names = |list: &[&str]| list.iter().map(|s| s.to_string()).collect();
        let rewrite = [
            r"^(.*) - YouTube — Mozilla Firefox$",
            r"^(.*) — Mozilla Firefox$",
            r"^(.*) - Text Editor$",
            r"^(.*) - Discord$",
            r"^(.*) - Sublime Text( \(UNREGISTERED\))?$",
            r"^(.*) – GIMP$",
        ]
        .iter()
        .filter_map(|pattern| Regex::new(pattern).ok())
        .map(|re| (re, "$1".to_string()))
        .collect();

        Config {
            bar: BarConfig {
                height: 34,
                left: names(&["window-title"]),
                center: names(&["clock"]),
                right: names(&[]),
            },
            clock: ClockConfig {
                format: "%d %b  %H:%M".into(),
            },
            window_title: WindowTitleConfig { rewrite },
        }
    }
}

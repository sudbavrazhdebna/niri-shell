//! Settings from ~/.config/nshell/config.kdl. Anything left out falls back to a default.

use std::path::PathBuf;

use gtk4::{gio, prelude::*};
use kdl::{KdlDocument, KdlNode, KdlValue};
use regex::Regex;

/// Written to the user's config dir on first start, as a commented example.
pub const DEFAULT_KDL: &str = include_str!("../config/default.kdl");

/// Modules that exist in the code. Names from `PLANNED` are accepted and skipped,
/// so a config written today keeps working as modules land.
pub const MODULES: &[&str] = &[
    "clock", "window-title", "keyboard", "volume", "battery", "network", "bluetooth", "tray",
    "control", "media", "display", "taskbar",
];
pub const PLANNED: &[&str] = &["docker"];

/// Top-level sections of later steps, accepted and ignored for now.
const LATER: &[&str] = &["osd", "notifications"];

pub struct Config {
    pub icon_theme: Option<String>,
    pub bar: BarConfig,
    pub clock: ClockConfig,
    pub window_title: WindowTitleConfig,
    pub volume: VolumeConfig,
    pub idle: IdleConfig,
    pub dock: DockConfig,
    /// The laptop's own screen goes off while an external monitor is on.
    pub laptop_screen_auto: bool,
    /// The image on every monitor, filling it; None: no wallpaper from the shell.
    pub wallpaper: Option<PathBuf>,
    /// How strongly the wallpaper blurs in niri's overview, in pixels; 0: not at all.
    pub overview_blur: u32,
    /// How the OSD names keyboard layouts: (pattern, replacement), all applied in order.
    pub layout_rename: Vec<(Regex, String)>,
    /// Folders whose files the launcher finds.
    pub launcher_paths: Vec<PathBuf>,
    /// The media module shows the track's title next to its play button.
    pub media_title: bool,
    /// Parts of the shell that don't blur what's behind them (`blur::PARTS`).
    pub blur_off: Vec<String>,
}

#[derive(PartialEq, Eq)]
pub struct BarConfig {
    /// Height in pixels; content is centered vertically.
    pub height: i32,
    /// Along the bottom edge instead of the top (`position "bottom"`).
    pub bottom: bool,
    pub left: Vec<String>,
    pub center: Vec<String>,
    pub right: Vec<String>,
}

#[derive(PartialEq, Eq)]
pub struct ClockConfig {
    /// strftime-like format, see `glib::DateTime::format`.
    pub format: String,
    /// The calendar's weeks start on Monday (else: as the locale says).
    pub monday_first: bool,
}

pub struct DockConfig {
    pub enabled: bool,
    /// Hidden until the pointer touches the bottom edge (or `nshell msg show-dock`).
    pub autohide: bool,
    /// Keep an autohiding dock visible in niri's overview.
    pub show_in_overview: bool,
    /// Show the overview button and its separator after the apps.
    pub overview: bool,
    /// App ids (as niri reports them) or desktop entry ids, in dock order.
    pub pinned: Vec<String>,
}

#[derive(Clone)]
pub struct IdleConfig {
    /// Runs to lock the screen: from the control center, the lock stage, and before
    /// sleep (where the shell waits for it to exit, so it should wait for the lock).
    pub lock_command: String,
    /// Windows (app id, whole-name regex) that keep the session awake while open.
    pub inhibit_apps: Vec<Regex>,
    /// What happens when, on AC and on battery.
    pub on_ac: Vec<Stage>,
    pub on_battery: Vec<Stage>,
}

/// One step of going idle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IdleAction {
    /// Screen to 5%, keyboard light off; back on activity.
    Dim,
    Lock,
    /// Monitors off; on again on activity.
    Dpms,
    Suspend,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Stage {
    pub action: IdleAction,
    /// Counted from the start of idle, not from the previous stage.
    pub after: std::time::Duration,
}

impl IdleConfig {
    fn stages(list: &[(IdleAction, u64)]) -> Vec<Stage> {
        list.iter()
            .map(|&(action, secs)| Stage { action, after: std::time::Duration::from_secs(secs) })
            .collect()
    }
}

#[derive(PartialEq, Eq)]
pub struct VolumeConfig {
    /// Percent per wheel step.
    pub scroll_step: i32,
}

pub struct WindowTitleConfig {
    pub max_length: i32,
    /// Applied in order; the first matching pattern wins.
    pub rewrite: Vec<(Regex, String)>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            icon_theme: None,
            bar: BarConfig {
                height: 34,
                bottom: false,
                left: vec!["window-title".into()],
                center: vec!["clock".into()],
                right: Vec::new(),
            },
            clock: ClockConfig {
                format: "%d %b  %H:%M".into(),
                monday_first: true,
            },
            window_title: WindowTitleConfig {
                max_length: 80,
                rewrite: Vec::new(),
            },
            volume: VolumeConfig { scroll_step: 1 },
            idle: IdleConfig {
                lock_command: "loginctl lock-session".into(),
                inhibit_apps: ["vlc", "mpv", "steam_app_.*"].iter().filter_map(|p| whole(p).ok()).collect(),
                // The stasis setup's times, counted from the start of idle and rounded.
                on_ac: IdleConfig::stages(&[
                    (IdleAction::Dim, 120),
                    (IdleAction::Lock, 300),
                    (IdleAction::Dpms, 900),
                    (IdleAction::Suspend, 1800),
                ]),
                on_battery: IdleConfig::stages(&[
                    (IdleAction::Dim, 60),
                    (IdleAction::Dpms, 180),
                    (IdleAction::Lock, 360),
                    (IdleAction::Suspend, 600),
                ]),
            },
            laptop_screen_auto: true,
            layout_rename: Vec::new(),
            launcher_paths: Vec::new(),
            media_title: true,
            blur_off: Vec::new(),
            wallpaper: Some(gtk4::glib::home_dir().join(".config/wallpapers/m.jpg")),
            overview_blur: 24,
            dock: DockConfig {
                enabled: true,
                autohide: false,
                show_in_overview: true,
                overview: true,
                pinned: Vec::new(),
            },
        }
    }
}

pub fn dir() -> PathBuf {
    gtk4::glib::user_config_dir().join("nshell")
}

pub fn path() -> PathBuf {
    dir().join("config.kdl")
}

impl Config {
    /// Pin order is applied to the existing app rows. Other settings rebuild the bar.
    pub fn same_except_pinned(&self, other: &Self) -> bool {
        let Self {
            icon_theme, bar, clock, window_title, volume, idle, dock, laptop_screen_auto,
            wallpaper, overview_blur, layout_rename, launcher_paths, media_title, blur_off,
        } = self;
        let DockConfig { enabled, autohide, show_in_overview, overview, pinned: _ } = dock;
        let IdleConfig { lock_command, inhibit_apps, on_ac, on_battery } = idle;
        let WindowTitleConfig { max_length, rewrite } = window_title;
        let patterns = |a: &[Regex], b: &[Regex]| a.iter().map(Regex::as_str).eq(b.iter().map(Regex::as_str));

        icon_theme == &other.icon_theme
            && bar == &other.bar
            && clock == &other.clock
            && max_length == &other.window_title.max_length
            && same_rewrites(rewrite, &other.window_title.rewrite)
            && volume == &other.volume
            && lock_command == &other.idle.lock_command
            && patterns(inhibit_apps, &other.idle.inhibit_apps)
            && on_ac == &other.idle.on_ac
            && on_battery == &other.idle.on_battery
            && enabled == &other.dock.enabled
            && autohide == &other.dock.autohide
            && show_in_overview == &other.dock.show_in_overview
            && overview == &other.dock.overview
            && laptop_screen_auto == &other.laptop_screen_auto
            && wallpaper == &other.wallpaper
            && overview_blur == &other.overview_blur
            && same_rewrites(layout_rename, &other.layout_rename)
            && launcher_paths == &other.launcher_paths
            && media_title == &other.media_title
            && blur_off == &other.blur_off
    }

    /// Reads the user's config, creating it from the example if there is none.
    pub fn load() -> Result<Config, String> {
        let path = path();
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                if let Err(err) = std::fs::create_dir_all(dir())
                    .and_then(|()| std::fs::write(&path, DEFAULT_KDL))
                {
                    tracing::warn!("can't write {}: {err}", path.display());
                }
                DEFAULT_KDL.to_string()
            }
            Err(err) => return Err(format!("{}: {err}", path.display())),
        };
        Config::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Config, String> {
        let doc = KdlDocument::parse(text).map_err(|err| {
            let diag = err.diagnostics.first();
            let line = diag.map_or(1, |d| line_of(text, d.span.offset()));
            let what = diag
                .and_then(|d| d.message.clone().or_else(|| d.label.clone()))
                .unwrap_or_else(|| "syntax error".into());
            format!("line {line}: {what}")
        })?;

        let mut config = Config::default();
        let at = |node: &KdlNode, msg: String| {
            format!("line {}: {msg}", line_of(text, node.span().offset()))
        };

        for node in doc.nodes() {
            match node.name().value() {
                "icon-theme" => config.icon_theme = Some(string_arg(node).map_err(|e| at(node, e))?),
                "bar" => parse_bar(&mut config.bar, node, &at)?,
                "clock" => {
                    for child in children(node) {
                        match child.name().value() {
                            "format" => {
                                config.clock.format = string_arg(child).map_err(|e| at(child, e))?
                            }
                            "week-starts" => {
                                config.clock.monday_first = match string_arg(child).map_err(|e| at(child, e))?.as_str() {
                                    "monday" => true,
                                    "locale" => false,
                                    _ => return Err(at(child, "needs \"monday\" or \"locale\"".into())),
                                }
                            }
                            other => return Err(at(child, unknown(other))),
                        }
                    }
                }
                "window-title" => {
                    let wt = &mut config.window_title;
                    for child in children(node) {
                        match child.name().value() {
                            "max-length" => {
                                wt.max_length = int_arg(child, 1..=1000).map_err(|e| at(child, e))?
                            }
                            "rewrite" => {
                                let args = strings(child).map_err(|e| at(child, e))?;
                                let [pattern, replace] = args.as_slice() else {
                                    return Err(at(child, "needs two values: \"pattern\" \"replacement\"".into()));
                                };
                                let re = Regex::new(pattern)
                                    .map_err(|e| at(child, format!("bad pattern: {e}")))?;
                                wt.rewrite.push((re, replace.clone()));
                            }
                            other => return Err(at(child, unknown(other))),
                        }
                    }
                }
                "volume" => {
                    for child in children(node) {
                        match child.name().value() {
                            "scroll-step" => {
                                config.volume.scroll_step =
                                    int_arg(child, 1..=50).map_err(|e| at(child, e))?
                            }
                            other => return Err(at(child, unknown(other))),
                        }
                    }
                }
                "media" => {
                    for child in children(node) {
                        match child.name().value() {
                            "title" => {
                                config.media_title = match string_arg(child).map_err(|e| at(child, e))?.as_str() {
                                    "on" => true,
                                    "off" => false,
                                    _ => return Err(at(child, "needs \"on\" or \"off\"".into())),
                                }
                            }
                            other => return Err(at(child, unknown(other))),
                        }
                    }
                }
                "launcher" => {
                    for child in children(node) {
                        match child.name().value() {
                            "search-paths" => {
                                config.launcher_paths = strings(child)
                                    .map_err(|e| at(child, e))?
                                    .iter()
                                    .map(|p| match p.strip_prefix("~/") {
                                        Some(rest) => gtk4::glib::home_dir().join(rest),
                                        None if p == "~" => gtk4::glib::home_dir(),
                                        None => PathBuf::from(p),
                                    })
                                    .collect();
                            }
                            other => return Err(at(child, unknown(other))),
                        }
                    }
                }
                "keyboard" => {
                    for child in children(node) {
                        match child.name().value() {
                            "rename" => {
                                let args = strings(child).map_err(|e| at(child, e))?;
                                let [pattern, replace] = args.as_slice() else {
                                    return Err(at(child, "needs two values: \"pattern\" \"replacement\"".into()));
                                };
                                let re = Regex::new(pattern)
                                    .map_err(|e| at(child, format!("bad pattern: {e}")))?;
                                config.layout_rename.push((re, replace.clone()));
                            }
                            other => return Err(at(child, unknown(other))),
                        }
                    }
                }
                "wallpaper" => {
                    for child in children(node) {
                        match child.name().value() {
                            "image" => {
                                let path = string_arg(child).map_err(|e| at(child, e))?;
                                config.wallpaper = Some(match path.strip_prefix("~/") {
                                    Some(rest) => gtk4::glib::home_dir().join(rest),
                                    None => PathBuf::from(path),
                                });
                            }
                            "off" => config.wallpaper = None,
                            "overview-blur" => {
                                config.overview_blur =
                                    int_arg(child, 0..=200).map_err(|e| at(child, e))?.unsigned_abs();
                            }
                            other => return Err(at(child, unknown(other))),
                        }
                    }
                }
                "displays" => {
                    for child in children(node) {
                        match child.name().value() {
                            "laptop-screen" => {
                                config.laptop_screen_auto = match string_arg(child).map_err(|e| at(child, e))?.as_str() {
                                    "auto" => true,
                                    "on" => false,
                                    _ => return Err(at(child, "needs \"auto\" or \"on\"".into())),
                                }
                            }
                            other => return Err(at(child, unknown(other))),
                        }
                    }
                }
                "dock" => {
                    for child in children(node) {
                        match child.name().value() {
                            "off" => config.dock.enabled = false,
                            "autohide" => config.dock.autohide = true,
                            "show-in-overview" => {
                                config.dock.show_in_overview = match string_arg(child).map_err(|e| at(child, e))?.as_str() {
                                    "on" => true,
                                    "off" => false,
                                    _ => return Err(at(child, "needs \"on\" or \"off\"".into())),
                                }
                            }
                            "no-overview" => config.dock.overview = false,
                            "pinned" => {
                                config.dock.pinned = strings(child).map_err(|e| at(child, e))?
                            }
                            other => return Err(at(child, unknown(other))),
                        }
                    }
                }
                "blur" => {
                    for child in children(node) {
                        match child.name().value() {
                            "off" => {
                                let parts = strings(child).map_err(|e| at(child, e))?;
                                if let Some(bad) = parts.iter().find(|p| !crate::blur::PARTS.contains(&p.as_str())) {
                                    let all = crate::blur::PARTS.join(", ");
                                    return Err(at(child, format!("no part \"{bad}\", available: {all}")));
                                }
                                config.blur_off.extend(parts);
                            }
                            other => return Err(at(child, unknown(other))),
                        }
                    }
                }
                "idle" => parse_idle(&mut config.idle, node, &at)?,
                name if LATER.contains(&name) => {}
                other => return Err(at(node, unknown(other))),
            }
        }
        Ok(config)
    }
}

fn same_rewrites(a: &[(Regex, String)], b: &[(Regex, String)]) -> bool {
    a.iter().map(|(pattern, replacement)| (pattern.as_str(), replacement))
        .eq(b.iter().map(|(pattern, replacement)| (pattern.as_str(), replacement)))
}

/// Updates the latest pin list in the file while preserving its comments and layout.
pub fn set_pinned(key: &str, pin: bool) -> Result<(), String> {
    let path = path();
    let file = gio::File::for_path(&path);
    let (contents, etag) = file.load_contents(gio::Cancellable::NONE)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let text = std::str::from_utf8(&contents).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut pinned = Config::parse(text)?.dock.pinned;
    if pinned.iter().any(|app| app == key) == pin {
        return Ok(());
    }
    if pin {
        pinned.push(key.to_string());
    } else {
        pinned.retain(|app| app != key);
    }
    let updated = with_pinned(text, &pinned)?;
    file.replace_contents(
        updated.as_bytes(),
        etag.as_deref(),
        false,
        gio::FileCreateFlags::NONE,
        gio::Cancellable::NONE,
    )
    .map(|_| ())
    .map_err(|e| format!("{}: {e}", path.display()))
}

fn with_pinned(text: &str, pinned: &[String]) -> Result<String, String> {
    let mut doc = KdlDocument::parse(text).map_err(|_| "the config doesn't parse".to_string())?;
    let nodes = doc.nodes_mut();
    if !nodes.iter().any(|n| n.name().value() == "dock") {
        nodes.push(KdlNode::new("dock"));
    }
    let Some(dock) = nodes.iter_mut().rev().find(|n| n.name().value() == "dock") else {
        return Err("no dock section".into());
    };
    let children = dock.ensure_children().nodes_mut();
    if !children.iter().any(|n| n.name().value() == "pinned") {
        children.push(KdlNode::new("pinned"));
    }
    if let Some(node) = children.iter_mut().rev().find(|n| n.name().value() == "pinned") {
        let entries = node.entries_mut();
        entries.clear();
        entries.extend(pinned.iter().map(|app| kdl::KdlEntry::new(app.as_str())));
    }
    Ok(doc.to_string())
}

fn parse_bar(
    bar: &mut BarConfig,
    node: &KdlNode,
    at: &impl Fn(&KdlNode, String) -> String,
) -> Result<(), String> {
    for child in children(node) {
        let zone = match child.name().value() {
            "height" => {
                bar.height = int_arg(child, 16..=200).map_err(|e| at(child, e))?;
                continue;
            }
            "position" => {
                bar.bottom = match string_arg(child).map_err(|e| at(child, e))?.as_str() {
                    "top" => false,
                    "bottom" => true,
                    _ => return Err(at(child, "needs \"top\" or \"bottom\"".into())),
                };
                continue;
            }
            "left" => &mut bar.left,
            "center" => &mut bar.center,
            "right" => &mut bar.right,
            other => return Err(at(child, unknown(other))),
        };
        let names = strings(child).map_err(|e| at(child, e))?;
        if let Some(bad) = names
            .iter()
            .find(|n| !MODULES.contains(&n.as_str()) && !PLANNED.contains(&n.as_str()))
        {
            let all: Vec<_> = MODULES.iter().chain(PLANNED).copied().collect();
            return Err(at(
                child,
                format!("no module \"{bad}\", available: {}", all.join(", ")),
            ));
        }
        *zone = names;
    }
    Ok(())
}

fn parse_idle(
    idle: &mut IdleConfig,
    node: &KdlNode,
    at: &impl Fn(&KdlNode, String) -> String,
) -> Result<(), String> {
    for child in children(node) {
        match child.name().value() {
            "lock-command" => idle.lock_command = string_arg(child).map_err(|e| at(child, e))?,
            "inhibit-apps" => {
                idle.inhibit_apps = strings(child)
                    .map_err(|e| at(child, e))?
                    .iter()
                    .map(|p| whole(p).map_err(|e| at(child, format!("bad pattern: {e}"))))
                    .collect::<Result<_, _>>()?;
            }
            profile @ ("on-ac" | "on-battery") => {
                let mut stages = Vec::new();
                for stage in children(child) {
                    let action = match stage.name().value() {
                        "dim" => IdleAction::Dim,
                        "lock" => IdleAction::Lock,
                        "dpms" => IdleAction::Dpms,
                        "suspend" => IdleAction::Suspend,
                        other => return Err(at(stage, format!("no stage \"{other}\", there are dim, lock, dpms, suspend"))),
                    };
                    if stages.iter().any(|s: &Stage| s.action == action) {
                        return Err(at(stage, "this stage is already set".into()));
                    }
                    let after = duration_arg(stage).map_err(|e| at(stage, e))?;
                    stages.push(Stage { action, after });
                }
                stages.sort_by_key(|s| s.after);
                if profile == "on-ac" {
                    idle.on_ac = stages;
                } else {
                    idle.on_battery = stages;
                }
            }
            other => return Err(at(child, unknown(other))),
        }
    }
    Ok(())
}

/// A regex that must match the whole text.
fn whole(pattern: &str) -> Result<Regex, regex::Error> {
    Regex::new(&format!("^(?:{pattern})$"))
}

/// "90s", "2m", "1h", or a number of seconds; more than zero.
fn duration_arg(node: &KdlNode) -> Result<std::time::Duration, String> {
    let bad = || "needs a time like \"30s\", \"2m\" or \"1h\"".to_string();
    let secs = match node.entries() {
        [entry] if entry.name().is_none() => match entry.value() {
            KdlValue::Integer(n) => u64::try_from(*n).ok(),
            KdlValue::String(text) => {
                let text = text.trim();
                let (number, unit) = text.split_at(text.find(|c: char| !c.is_ascii_digit()).unwrap_or(text.len()));
                let n: u64 = number.parse().map_err(|_| bad())?;
                match unit.trim() {
                    "" | "s" => Some(n),
                    "m" => n.checked_mul(60),
                    "h" => n.checked_mul(3600),
                    _ => None,
                }
            }
            _ => None,
        },
        _ => None,
    };
    secs.filter(|s| *s > 0).map(std::time::Duration::from_secs).ok_or_else(bad)
}

fn children(node: &KdlNode) -> &[KdlNode] {
    node.children().map_or(&[], |doc| doc.nodes())
}

fn unknown(name: &str) -> String {
    format!("unknown setting \"{name}\"")
}

/// All arguments of a node as strings.
fn strings(node: &KdlNode) -> Result<Vec<String>, String> {
    node.entries()
        .iter()
        .filter(|e| e.name().is_none())
        .map(|e| match e.value() {
            KdlValue::String(s) => Ok(s.clone()),
            other => Err(format!("expected a quoted string, not {other}")),
        })
        .collect()
}

fn string_arg(node: &KdlNode) -> Result<String, String> {
    match strings(node)?.as_slice() {
        [one] => Ok(one.clone()),
        _ => Err("needs one quoted value".into()),
    }
}

fn int_arg(node: &KdlNode, range: std::ops::RangeInclusive<i32>) -> Result<i32, String> {
    let value = match node.entries() {
        [entry] if entry.name().is_none() => entry.value().as_integer(),
        _ => None,
    };
    value
        .and_then(|v| i32::try_from(v).ok())
        .filter(|v| range.contains(v))
        .ok_or_else(|| {
            format!(
                "needs one number from {} to {}",
                range.start(),
                range.end()
            )
        })
}

fn line_of(text: &str, offset: usize) -> usize {
    text.get(..offset).map_or(1, |head| head.matches('\n').count() + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_parses() {
        let config = Config::parse(DEFAULT_KDL).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(config.bar.height, 34);
        assert_eq!(config.window_title.rewrite.len(), 6);
        assert_eq!(config.bar.right.len(), 8);
    }

    #[test]
    fn layout_names_lose_their_variant() {
        let config = Config::parse(DEFAULT_KDL).unwrap_or_else(|e| panic!("{e}"));
        let rename = |name: &str| {
            config.layout_rename.iter().fold(name.to_string(), |n, (re, to)| re.replace_all(&n, to.as_str()).into_owned())
        };
        assert_eq!(rename("English (US)"), "English");
        assert_eq!(rename("Russian"), "Russian");
    }

    #[test]
    fn rewrite_drops_suffix() {
        let config = Config::parse(DEFAULT_KDL).unwrap_or_else(|e| panic!("{e}"));
        let (re, rep) = &config.window_title.rewrite[1];
        assert_eq!(re.replace("Docs — Mozilla Firefox", rep.as_str()), "Docs");
    }

    #[test]
    fn errors_point_at_line() {
        let err = Config::parse("bar {\n    height 5000\n}").err().unwrap_or_default();
        assert!(err.starts_with("line 2:"), "{err}");
        let err = Config::parse("clock {\n  format \"x\"\n}\nbar { left \"nope\" }")
            .err()
            .unwrap_or_default();
        assert!(err.starts_with("line 4:") && err.contains("nope"), "{err}");
        let err = Config::parse("bar {").err().unwrap_or_default();
        assert!(err.starts_with("line"), "{err}");
    }

    #[test]
    fn pinned_list_rewrites_only_itself() {
        let text = "// mine\nbar { height 30; }\ndock {\n    pinned \"a\" \"b\"\n}\n";
        let out = with_pinned(text, &["b".into(), "c".into()]).unwrap_or_else(|e| panic!("{e}"));
        assert!(out.contains("// mine"), "{out}");
        let config = Config::parse(&out).unwrap_or_else(|e| panic!("{e}: {out}"));
        assert_eq!(config.dock.pinned, ["b", "c"]);
        assert_eq!(config.bar.height, 30);
        let added = with_pinned("bar { height 30; }\n", &["x".into()]).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(Config::parse(&added).map(|c| c.dock.pinned).unwrap_or_default(), ["x"]);

        let repeated = "dock { pinned \"a\"; pinned \"b\"; }\ndock { autohide; }\n";
        let out = with_pinned(repeated, &["c".into()]).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(Config::parse(&out).map(|c| c.dock.pinned).unwrap_or_default(), ["c"]);
    }

    #[test]
    fn pin_changes_keep_other_settings_equivalent() {
        let parse = |text: &str| Config::parse(text).unwrap_or_else(|e| panic!("{e}"));
        let original = parse(DEFAULT_KDL);
        let edited = with_pinned(DEFAULT_KDL, &["firefox".into()]).unwrap_or_else(|e| panic!("{e}"));
        assert!(original.same_except_pinned(&parse(&edited)));
        assert!(!original.same_except_pinned(&parse(&edited.replace("height 34", "height 40"))));
        assert!(!original.same_except_pinned(&parse(&edited.replace("    autohide\n", ""))));
    }

    #[test]
    fn dock_overview_setting_survives_pin_changes() {
        assert!(Config::default().dock.overview);
        let text = "dock { no-overview; show-in-overview \"off\"; pinned \"a\"; }\n";
        let out = with_pinned(text, &["b".into()]).unwrap_or_else(|e| panic!("{e}"));
        let config = Config::parse(&out).unwrap_or_else(|e| panic!("{e}"));
        assert!(!config.dock.overview);
        assert!(!config.dock.show_in_overview);
        assert_eq!(config.dock.pinned, ["b"]);
    }

    #[test]
    fn showing_the_dock_in_overview_is_independent_of_its_button() {
        assert!(Config::default().dock.show_in_overview);
        let parse = |text: &str| Config::parse(text).unwrap_or_else(|e| panic!("{e}"));
        let on = parse("dock { no-overview; show-in-overview \"on\"; }");
        let off = parse("dock { no-overview; show-in-overview \"off\"; }");
        assert!(!on.dock.overview && on.dock.show_in_overview);
        assert!(!off.dock.overview && !off.dock.show_in_overview);
        assert!(!on.same_except_pinned(&off));
        assert!(Config::parse("dock { show-in-overview \"no\"; }").is_err());
    }

    #[test]
    fn idle_stages() {
        let text = "idle {\n  inhibit-apps \"mpv\" \"steam_app_.*\"\n  on-ac { lock \"5m\"; dim \"90s\"; suspend 1800; }\n}";
        let config = Config::parse(text).unwrap_or_else(|e| panic!("{e}"));
        let ac: Vec<_> = config.idle.on_ac.iter().map(|s| (s.action, s.after.as_secs())).collect();
        assert_eq!(ac, [(IdleAction::Dim, 90), (IdleAction::Lock, 300), (IdleAction::Suspend, 1800)]);
        // The battery profile stays as built in.
        assert_eq!(config.idle.on_battery.len(), 4);
        assert!(config.idle.inhibit_apps.iter().any(|r| r.is_match("steam_app_123")));
        assert!(!config.idle.inhibit_apps.iter().any(|r| r.is_match("mpvpaper")));
        let err = Config::parse("idle {\n  on-ac { nap \"5m\"; }\n}").err().unwrap_or_default();
        assert!(err.starts_with("line 2:") && err.contains("nap"), "{err}");
        let err = Config::parse("idle {\n  on-ac {\n    dim \"soon\"\n  }\n}").err().unwrap_or_default();
        assert!(err.starts_with("line 3:"), "{err}");
    }

    #[test]
    fn kdl_v2_syntax_works_too() {
        assert!(Config::parse("bar { height 30; left \"clock\" }").is_ok());
    }
}

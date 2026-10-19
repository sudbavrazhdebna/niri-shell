//! Settings from ~/.config/nshell/config.kdl. Anything left out falls back to a default.

use std::path::PathBuf;

use kdl::{KdlDocument, KdlNode, KdlValue};
use regex::Regex;

/// Written to the user's config dir on first start, as a commented example.
pub const DEFAULT_KDL: &str = include_str!("../config/default.kdl");

/// Modules that exist in the code. Names from `PLANNED` are accepted and skipped,
/// so a config written today keeps working as modules land.
pub const MODULES: &[&str] = &["clock", "window-title"];
pub const PLANNED: &[&str] = &[
    "tray", "keyboard", "network", "bluetooth", "volume", "battery", "control",
];

/// Top-level sections of later steps, accepted and ignored for now.
const LATER: &[&str] = &["idle", "dock", "wallpaper", "osd", "notifications"];

pub struct Config {
    pub icon_theme: Option<String>,
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
                left: vec!["window-title".into()],
                center: vec!["clock".into()],
                right: Vec::new(),
            },
            clock: ClockConfig {
                format: "%d %b  %H:%M".into(),
            },
            window_title: WindowTitleConfig {
                max_length: 80,
                rewrite: Vec::new(),
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
                .unwrap_or_else(|| "синтаксическая ошибка".into());
            format!("строка {line}: {what}")
        })?;

        let mut config = Config::default();
        let at = |node: &KdlNode, msg: String| {
            format!("строка {}: {msg}", line_of(text, node.span().offset()))
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
                                    return Err(at(child, "нужно два значения: \"шаблон\" \"замена\"".into()));
                                };
                                let re = Regex::new(pattern)
                                    .map_err(|e| at(child, format!("ошибка в шаблоне: {e}")))?;
                                wt.rewrite.push((re, replace.clone()));
                            }
                            other => return Err(at(child, unknown(other))),
                        }
                    }
                }
                name if LATER.contains(&name) => {}
                other => return Err(at(node, unknown(other))),
            }
        }
        Ok(config)
    }
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
                format!("нет модуля \"{bad}\", есть: {}", all.join(", ")),
            ));
        }
        *zone = names;
    }
    Ok(())
}

fn children(node: &KdlNode) -> &[KdlNode] {
    node.children().map_or(&[], |doc| doc.nodes())
}

fn unknown(name: &str) -> String {
    format!("неизвестная настройка \"{name}\"")
}

/// All arguments of a node as strings.
fn strings(node: &KdlNode) -> Result<Vec<String>, String> {
    node.entries()
        .iter()
        .filter(|e| e.name().is_none())
        .map(|e| match e.value() {
            KdlValue::String(s) => Ok(s.clone()),
            other => Err(format!("ожидалась строка в кавычках, а не {other}")),
        })
        .collect()
}

fn string_arg(node: &KdlNode) -> Result<String, String> {
    match strings(node)?.as_slice() {
        [one] => Ok(one.clone()),
        _ => Err("нужно одно значение в кавычках".into()),
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
                "нужно одно число от {} до {}",
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
        assert_eq!(config.bar.right.len(), 7);
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
        assert!(err.starts_with("строка 2:"), "{err}");
        let err = Config::parse("clock {\n  format \"x\"\n}\nbar { left \"nope\" }")
            .err()
            .unwrap_or_default();
        assert!(err.starts_with("строка 4:") && err.contains("nope"), "{err}");
        let err = Config::parse("bar {").err().unwrap_or_default();
        assert!(err.starts_with("строка"), "{err}");
    }

    #[test]
    fn kdl_v2_syntax_works_too() {
        assert!(Config::parse("bar { height 30; left \"clock\" }").is_ok());
    }
}

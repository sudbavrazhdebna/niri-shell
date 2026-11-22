//! Recently used apps, from niri's window list and focus, shared by all docks.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use gtk4::{glib, prelude::*};

use crate::niri::{Niri, State};

const VISIBLE: usize = 3;
// Some recent apps are still pinned or running, so keep more than we display.
const REMEMBERED: usize = 32;

#[derive(Default)]
pub struct RecentApps {
    history: RefCell<Option<History>>,
}

impl RecentApps {
    pub fn track(niri: &Niri, enabled: bool) -> Rc<Self> {
        let recent = Rc::new(Self::default());
        recent.configure(enabled, &niri.state.get());
        // Subscribe before the docks do so each render sees the new history and
        // the corresponding window list together.
        let weak = Rc::downgrade(&recent);
        niri.state.subscribe(move |state| {
            let Some(recent) = weak.upgrade() else { return false };
            if let Some(history) = recent.history.borrow_mut().as_mut() {
                history.observe(state);
            }
            true
        });
        recent
    }

    pub fn configure(&self, enabled: bool, state: &State) {
        let mut history = self.history.borrow_mut();
        if !enabled {
            history.take();
        } else if history.is_none() {
            let mut tracking = History::load();
            tracking.observe(state);
            *history = Some(tracking);
        }
    }

    pub(super) fn visible(&self, include: impl Fn(&str) -> bool) -> Vec<String> {
        let history = self.history.borrow();
        let Some(history) = history.as_ref() else { return Vec::new() };
        history.entries.iter()
            .filter(|id| include(id) && crate::apps::entry(id).is_some_and(|app| app.should_show()))
            .take(VISIBLE)
            .cloned()
            .collect()
    }
}

#[derive(Default)]
struct History {
    /// Desktop IDs, most recently used first.
    entries: Vec<String>,
    open_windows: HashMap<u64, String>,
    last_focused: Option<u64>,
    serial: Option<u64>,
}

impl History {
    fn load() -> Self {
        let text = std::fs::read_to_string(path()).unwrap_or_default();
        let mut entries = Vec::new();
        for line in text.lines().filter(|line| line.ends_with(".desktop")) {
            if !entries.iter().any(|id| id == line) {
                entries.push(line.to_string());
                if entries.len() == REMEMBERED {
                    break;
                }
            }
        }
        Self { entries, ..Self::default() }
    }

    fn observe(&mut self, state: &State) {
        if self.serial.replace(state.layout_serial) == Some(state.layout_serial) {
            return;
        }
        let open_windows: HashMap<_, _> = state.windows.values()
            .filter_map(|window| Some((window.id, window.app_id.clone()?)))
            .collect();
        let mut opened: Vec<_> = state.windows.values()
            .filter(|window| open_windows.get(&window.id) != self.open_windows.get(&window.id))
            .collect();
        // The initial snapshot can contain several already-open windows.
        opened.sort_by_key(|window| (window.focus_timestamp.map(|t| (t.secs, t.nanos)), window.id));
        self.open_windows = open_windows;

        let focused = state.windows.values().find(|window| window.is_focused)
            .filter(|window| self.last_focused.replace(window.id) != Some(window.id));
        let mut changed = false;
        for window in opened.into_iter().chain(focused) {
            let Some(id) = window.app_id.as_deref() else { continue };
            if let Some(app) = crate::apps::entry(id).filter(|app| app.should_show())
                && let Some(id) = app.id()
            {
                changed |= remember(&mut self.entries, &id);
            }
        }
        if changed {
            self.save();
        }
    }

    fn save(&self) {
        let file = path();
        if let Some(parent) = file.parent()
            && let Err(err) = std::fs::create_dir_all(parent)
        {
            tracing::warn!("dock history: {err}");
            return;
        }
        if let Err(err) = glib::file_set_contents(&file, self.entries.join("\n").as_bytes()) {
            tracing::warn!("dock history: {err}");
        }
    }
}

fn path() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| glib::home_dir().join(".local/state"))
        .join("nshell/dock-recents")
}

fn remember(entries: &mut Vec<String>, id: &str) -> bool {
    if entries.first().is_some_and(|first| first == id) {
        return false;
    }
    entries.retain(|entry| entry != id);
    entries.insert(0, id.to_string());
    entries.truncate(REMEMBERED);
    true
}

#[cfg(test)]
mod tests {
    use super::{REMEMBERED, remember};

    #[test]
    fn reuse_moves_an_app_to_the_front_without_a_duplicate() {
        let mut entries = vec!["b.desktop".into(), "a.desktop".into()];
        assert!(remember(&mut entries, "a.desktop"));
        assert!(!remember(&mut entries, "a.desktop"));
        assert_eq!(entries, ["a.desktop", "b.desktop"]);
        for index in 0..REMEMBERED {
            remember(&mut entries, &format!("app-{index}.desktop"));
        }
        assert_eq!(entries.len(), REMEMBERED);
        assert_eq!(entries.last().map(String::as_str), Some("app-0.desktop"));
    }
}

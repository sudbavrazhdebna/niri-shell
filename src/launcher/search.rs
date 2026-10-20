//! What the launcher finds: apps (by name, keywords, description, command) ranked by how
//! well they match and how often they were started, and files under the configured
//! folders (an index made off the main thread when the launcher opens).

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime};

use gio_unix::DesktopAppInfo;
use gio_unix::prelude::*;
use gtk4::{gio, glib};

/// An app that can be started.
pub struct App {
    pub info: DesktopAppInfo,
    /// Desktop id, e.g. "org.gnome.Nautilus.desktop".
    pub id: String,
    pub name: String,
    /// "Web Browser", or the comment: shown under the name.
    pub detail: String,
    /// Lowercased name, then the other things it can be found by.
    name_lower: String,
    others: Vec<String>,
}

thread_local! {
    /// The app list, read once: reading every desktop file took ~20 ms per opening.
    static APPS: RefCell<Option<Rc<Vec<Rc<App>>>>> = const { RefCell::new(None) };
    /// Tells when apps are installed or removed; kept alive here.
    static APP_MONITOR: RefCell<Option<gio::AppInfoMonitor>> = const { RefCell::new(None) };
}

/// Every app the menus would show, once per desktop id; read again only after apps
/// were installed or removed.
pub fn apps() -> Rc<Vec<Rc<App>>> {
    if let Some(apps) = APPS.with_borrow(Clone::clone) {
        return apps;
    }
    let apps = Rc::new(read_apps());
    APPS.set(Some(apps.clone()));
    // The monitor reports changes only after the list has been read, as just now.
    APP_MONITOR.with_borrow_mut(|monitor| {
        monitor.get_or_insert_with(|| {
            let monitor = gio::AppInfoMonitor::get();
            monitor.connect_changed(|_| APPS.set(None));
            monitor
        });
    });
    apps
}

fn read_apps() -> Vec<Rc<App>> {
    let mut seen = std::collections::HashSet::new();
    gio::AppInfo::all()
        .into_iter()
        .filter(|info| info.should_show())
        .filter_map(|info| info.downcast::<DesktopAppInfo>().ok())
        .filter_map(|info| {
            let id = info.id()?.to_string();
            if !seen.insert(id.clone()) {
                return None;
            }
            let name = info.name().to_string();
            let generic = info.generic_name().map(String::from).unwrap_or_default();
            let comment = info.description().map(String::from).unwrap_or_default();
            let mut others: Vec<String> = info.keywords().iter().map(|k| k.to_lowercase()).collect();
            others.push(generic.to_lowercase());
            let command = info.executable();
            if let Some(binary) = command.file_name() {
                others.push(binary.to_string_lossy().to_lowercase());
            }
            others.retain(|o| !o.is_empty());
            Some(Rc::new(App {
                name_lower: name.to_lowercase(),
                detail: if generic.is_empty() { comment } else { generic },
                info,
                id,
                name,
                others,
            }))
        })
        .collect()
}

/// How well `query` (lowercased) matches `text` (lowercased); None: not at all.
pub fn score(query: &str, text: &str) -> Option<u32> {
    if query.is_empty() || text.is_empty() {
        return None;
    }
    if text == query {
        return Some(1000);
    }
    if text.starts_with(query) {
        return Some(800);
    }
    // At the start of a word: "code" in "visual studio code".
    let word_start = text
        .match_indices(query)
        .any(|(i, _)| text[..i].ends_with([' ', '-', '_', '.']));
    if word_start {
        return Some(600);
    }
    if text.contains(query) {
        return Some(400);
    }
    // The first letters of the words: "vsc" → visual studio code.
    let initials: String = text
        .split([' ', '-', '_', '.'])
        .filter_map(|word| word.chars().next())
        .collect();
    if query.chars().count() >= 2 && initials.starts_with(query) {
        return Some(500);
    }
    // The letters in order ("ffx" → firefox), the fewer gaps the better: from the start
    // of a word, and not scattered over the text ("fire" isn't LibreOffice Impress).
    let first = query.chars().next()?;
    let start = text
        .char_indices()
        .find(|&(i, c)| c == first && (i == 0 || text[..i].ends_with([' ', '-', '_', '.'])))
        .map(|(i, _)| i)?;
    let mut gaps = 0u32;
    let mut chars = text[start..].chars();
    for wanted in query.chars() {
        let mut skipped = 0u32;
        loop {
            match chars.next() {
                Some(c) if c == wanted => break,
                Some(_) => skipped += 1,
                None => return None,
            }
        }
        gaps += skipped;
    }
    let allowed = u32::try_from(query.chars().count()).unwrap_or(u32::MAX).saturating_mul(2);
    (gaps <= allowed).then(|| 200u32.saturating_sub(gaps * 10).max(50))
}

/// Apps for `query`, best first; with no query, the most used.
pub fn rank(apps: &[Rc<App>], query: &str, history: &History) -> Vec<Rc<App>> {
    let query = query.trim().to_lowercase();
    let mut scored: Vec<(u32, &Rc<App>)> = apps
        .iter()
        .filter_map(|app| {
            let used = history.bonus(&app.id);
            if query.is_empty() {
                return (used > 0).then_some((used, app));
            }
            let by_name = score(&query, &app.name_lower);
            // Keywords and the like count for less than the name.
            let by_other = app.others.iter().filter_map(|o| score(&query, o)).max().map(|s| s * 3 / 5);
            let best = by_name.max(by_other)?;
            Some((best + used, app))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name_lower.cmp(&b.1.name_lower)));
    scored.into_iter().map(|(_, app)| app.clone()).collect()
}

/// How often and how lately each app was started from the launcher.
#[derive(Default)]
pub struct History {
    /// Desktop id → (times, last as Unix seconds).
    entries: HashMap<String, (u32, u64)>,
}

fn history_file() -> PathBuf {
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| glib::home_dir().join(".local/state"));
    state.join("nshell").join("launcher-history")
}

fn now() -> u64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl History {
    /// Lines of "id<TAB>times<TAB>last".
    pub fn load() -> History {
        let text = std::fs::read_to_string(history_file()).unwrap_or_default();
        let entries = text
            .lines()
            .filter_map(|line| {
                let mut parts = line.split('\t');
                let id = parts.next()?.to_string();
                let times = parts.next()?.parse().ok()?;
                let last = parts.next()?.parse().ok()?;
                Some((id, (times, last)))
            })
            .collect();
        History { entries }
    }

    pub fn record(&mut self, id: &str) {
        let entry = self.entries.entry(id.to_string()).or_insert((0, 0));
        entry.0 = entry.0.saturating_add(1);
        entry.1 = now();
        let text: String = self.entries.iter().map(|(id, (t, l))| format!("{id}\t{t}\t{l}\n")).collect();
        let file = history_file();
        let result = file.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(&file, text));
        if let Err(err) = result {
            tracing::warn!("launcher history: {err}");
        }
    }

    /// Extra score: often and lately used apps first (at most 150, less than a better match).
    fn bonus(&self, id: &str) -> u32 {
        let Some(&(times, last)) = self.entries.get(id) else { return 0 };
        let days = now().saturating_sub(last) / 86_400;
        let recent = 50u32.saturating_sub(u32::try_from(days).unwrap_or(u32::MAX).saturating_mul(5));
        (times.min(20) * 5 + recent).min(150)
    }
}

/// A file or folder found under the search folders.
pub struct FileHit {
    pub path: PathBuf,
    name_lower: String,
    pub is_dir: bool,
}

/// Folders never looked into: hidden ones and build or package caches.
const SKIP: &[&str] = &["node_modules", "target", "__pycache__", "venv", "site-packages", "build-dir"];
/// The index stops at this many entries and this depth below a search folder.
const MAX_FILES: usize = 100_000;
const MAX_DEPTH: usize = 8;
/// An index younger than this is reused when the launcher opens again.
const FRESH: Duration = Duration::from_secs(60);

thread_local! {
    static INDEX: RefCell<Option<(Instant, Rc<Vec<FileHit>>)>> = const { RefCell::new(None) };
}

/// The file index if there is one; starts making a new one when it is missing or old,
/// and calls `ready` once that is done.
pub fn files(roots: &[PathBuf], ready: impl FnOnce() + 'static) -> Option<Rc<Vec<FileHit>>> {
    let current = INDEX.with_borrow(|index| index.clone());
    if roots.is_empty() {
        return None;
    }
    if current.as_ref().is_none_or(|(made, _)| made.elapsed() > FRESH) {
        let roots = roots.to_vec();
        glib::spawn_future_local(async move {
            let Ok(found) = gio::spawn_blocking(move || walk(&roots)).await else { return };
            let found: Vec<FileHit> = found
                .into_iter()
                .map(|(path, is_dir)| FileHit {
                    name_lower: path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default(),
                    path,
                    is_dir,
                })
                .collect();
            INDEX.set(Some((Instant::now(), Rc::new(found))));
            ready();
        });
    }
    current.map(|(_, files)| files)
}

/// Files matching `query` by name, best first.
pub fn rank_files<'a>(files: &'a [FileHit], query: &str, limit: usize) -> Vec<&'a FileHit> {
    let query = query.trim().to_lowercase();
    if query.chars().count() < 2 {
        return Vec::new();
    }
    let mut scored: Vec<(u32, &FileHit)> = files
        .iter()
        .filter_map(|f| {
            // Only a real match of the name: letters scattered over a long path are noise.
            let s = score(&query, &f.name_lower).filter(|s| *s >= 400)?;
            Some((s, f))
        })
        .collect();
    // Shorter paths (closer to the search folder) first among equals.
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.path.as_os_str().len().cmp(&b.1.path.as_os_str().len())));
    scored.into_iter().take(limit).map(|(_, f)| f).collect()
}

/// (path, is a folder) under `roots`, breadth first. Runs off the main thread.
fn walk(roots: &[PathBuf]) -> Vec<(PathBuf, bool)> {
    let mut found = Vec::new();
    let mut queue: std::collections::VecDeque<(PathBuf, usize)> = roots.iter().map(|r| (r.clone(), 0)).collect();
    while let Some((dir, depth)) = queue.pop_front() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || SKIP.contains(&name.as_ref()) {
                continue;
            }
            // Not following links: they lead in circles or out of the folders.
            let Ok(kind) = entry.file_type() else { continue };
            let path = entry.path();
            if kind.is_dir() && depth < MAX_DEPTH {
                queue.push_back((path.clone(), depth + 1));
            }
            found.push((path, kind.is_dir()));
            if found.len() >= MAX_FILES {
                return found;
            }
        }
    }
    found
}

/// The path to show: "~/Documents/report.pdf".
pub fn short_path(path: &Path) -> String {
    let home = glib::home_dir();
    match path.strip_prefix(&home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::score;

    #[test]
    fn match_quality_orders() {
        let s = |q: &str, t: &str| score(q, t).unwrap_or(0);
        assert!(s("firefox", "firefox") > s("fire", "firefox"));
        assert!(s("fire", "firefox") > s("code", "visual studio code"));
        assert!(s("code", "visual studio code") > s("ode", "visual studio code"));
        // Initials say more than letters from the middle of a word.
        assert!(s("vsc", "visual studio code") > s("ode", "visual studio code"));
        assert!(score("ffx", "firefox").is_some());
        assert!(score("fire", "libreoffice impress").is_none());
        assert!(score("fire", "fedora media writer").is_none());
        assert!(score("vsc", "visual studio code").is_some());
        assert!(score("xyz", "firefox").is_none());
        assert!(score("", "firefox").is_none());
    }
}

//! Finding an app's desktop entry and full-color icon by its niri app id.

use std::cell::RefCell;
use std::collections::HashMap;

use gtk4::prelude::*;
use gio_unix::DesktopAppInfo;
use gtk4::{gdk, gio};

/// Size hint for picking the icon file; GTK scales it to the widget.
const LOOKUP_SIZE: i32 = 64;

thread_local! {
    /// Scanning desktop entries is slow, so each app id is looked up once.
    static ICONS: RefCell<HashMap<String, Option<gio::Icon>>> = RefCell::new(HashMap::new());
    static ENTRIES: RefCell<HashMap<String, Option<DesktopAppInfo>>> = RefCell::new(HashMap::new());
}

/// The desktop entry for an app id or desktop id, looked up once.
pub fn entry(app_id: &str) -> Option<DesktopAppInfo> {
    ENTRIES.with_borrow_mut(|cache| {
        cache
            .entry(app_id.to_string())
            .or_insert_with(|| desktop_entry(app_id))
            .clone()
    })
}

/// Full-color icon for an app id, or None if the app has no usable icon.
pub fn icon(app_id: &str) -> Option<gio::Icon> {
    ICONS.with_borrow_mut(|cache| {
        cache
            .entry(app_id.to_string())
            .or_insert_with(|| lookup(app_id))
            .clone()
    })
}

/// Forgets looked-up icons, e.g. after the icon theme changed.
pub fn clear_cache() {
    ICONS.with_borrow_mut(|cache| cache.clear());
    ENTRIES.with_borrow_mut(|cache| cache.clear());
}

fn lookup(app_id: &str) -> Option<gio::Icon> {
    let icon = entry(app_id)?.icon()?;

    // Some entries point straight at an image file.
    if let Some(file) = icon.downcast_ref::<gio::FileIcon>() {
        return Some(gio::FileIcon::new(&file.file()).upcast());
    }

    let names = icon.downcast_ref::<gio::ThemedIcon>()?.names();
    let name = names.first()?;
    resolve_color(name).map(|file| gio::FileIcon::new(&file).upcast())
}

/// The file GTK would load for a themed icon, insisting on the color variant: otherwise a
/// theme with only a `-symbolic` version of the icon shadows the color one from hicolor.
fn resolve_color(name: &str) -> Option<gio::File> {
    let display = gdk::Display::default()?;
    let theme = gtk4::IconTheme::for_display(&display);
    if !theme.has_icon(name) {
        return None;
    }
    let paintable = theme.lookup_icon(
        name,
        &[],
        LOOKUP_SIZE,
        1,
        gtk4::TextDirection::None,
        gtk4::IconLookupFlags::FORCE_REGULAR,
    );
    let file = paintable.file()?;
    let path = file.path()?;
    (!path.to_string_lossy().contains("-symbolic")).then_some(file)
}

/// App ids usually match the desktop file; Pulse/notifications may supply a display name.
fn desktop_entry(app_id: &str) -> Option<DesktopAppInfo> {
    let lower = app_id.to_lowercase();
    // A full desktop id, or an app id (which may itself end in ".desktop", like
    // Telegram's "org.telegram.desktop").
    for id in [app_id.to_string(), format!("{app_id}.desktop"), format!("{lower}.desktop")] {
        if !id.ends_with(".desktop") {
            continue;
        }
        if let Some(info) = DesktopAppInfo::new(&id) {
            return Some(info);
        }
    }
    gio::AppInfo::all()
        .into_iter()
        .filter_map(|info| info.downcast::<DesktopAppInfo>().ok())
        .find(|info| {
            info.startup_wm_class()
                .is_some_and(|class| class.eq_ignore_ascii_case(app_id))
                || info.name().eq_ignore_ascii_case(app_id)
        })
}

/// Whether a window's app id belongs to an app (a pinned name, a desktop id): equal, or
/// the same desktop entry.
pub fn same_app(key: &str, app_id: &str) -> bool {
    if key.eq_ignore_ascii_case(app_id) {
        return true;
    }
    let (Some(a), Some(b)) = (entry(key), entry(app_id)) else {
        return false;
    };
    a.id() == b.id()
}

//! One tray item: its icon, status and what clicking it does.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};

use crate::services::dbus::child;
use crate::watch::Watch;

const IFACE: &str = "org.kde.StatusNotifierItem";

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Status {
    /// Not important right now; hidden.
    Passive,
    #[default]
    Active,
    NeedsAttention,
}

#[derive(Clone, PartialEq, Debug, Default)]
pub enum TrayIcon {
    #[default]
    None,
    Name(String),
    File(PathBuf),
    /// ARGB32 pixels as sent by the app.
    Pixels { width: i32, height: i32, data: Vec<u8> },
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct ItemState {
    pub icon: TrayIcon,
    /// Bumped on every icon change signal: apps like clash-verge rewrite the same file.
    pub icon_serial: u64,
    pub status: Status,
    pub title: String,
    pub menu: Option<String>,
    /// The item has no action of its own: left click opens the menu.
    pub item_is_menu: bool,
}

pub struct Item {
    pub bus: String,
    pub path: String,
    pub state: Watch<ItemState>,
    conn: gio::DBusConnection,
    proxy: gio::DBusProxy,
    /// When the properties were last read, and whether a later read is already due.
    last_reload: Cell<Option<Instant>>,
    reload_due: Cell<bool>,
    /// An icon change waits in the due read.
    icon_changed: Cell<bool>,
}

/// Reads of an item's properties at most this often: some apps (a counter in Telegram,
/// Steam, Discord) announce a new icon many times a second.
const RELOAD_INTERVAL: Duration = Duration::from_millis(500);

impl Item {
    pub async fn connect(conn: &gio::DBusConnection, bus: &str, path: &str) -> Option<Rc<Item>> {
        // The proxy subscribes to signals before it reads the properties, so an icon
        // changed right after registration isn't missed.
        let proxy = gio::DBusProxy::new_future(
            conn,
            gio::DBusProxyFlags::DO_NOT_AUTO_START,
            None,
            Some(bus),
            path,
            IFACE,
        )
        .await
        .inspect_err(|err| tracing::warn!("tray: {bus}{path}: {err}"))
        .ok()?;

        let item = Rc::new(Item {
            bus: bus.to_string(),
            path: path.to_string(),
            state: Watch::new(ItemState::default()),
            conn: conn.clone(),
            proxy: proxy.clone(),
            last_reload: Cell::new(None),
            reload_due: Cell::new(false),
            icon_changed: Cell::new(false),
        });
        item.state.set(item.read(0));

        let weak = Rc::downgrade(&item);
        proxy.connect_local("g-signal", false, move |args| {
            let signal = args.get(2).and_then(|v| v.get::<String>().ok()).unwrap_or_default();
            if let Some(item) = weak.upgrade()
                && signal.starts_with("New")
            {
                item.request_reload(signal == "NewIcon" || signal == "NewAttentionIcon");
            }
            None
        });
        let weak = Rc::downgrade(&item);
        proxy.connect_local("g-properties-changed", false, move |_| {
            if let Some(item) = weak.upgrade() {
                let serial = item.state.get().icon_serial;
                item.state.set(item.read(serial));
            }
            None
        });
        Some(item)
    }

    /// The app is still on the bus.
    pub fn alive(&self) -> bool {
        self.proxy.name_owner().is_some()
    }

    /// Runs `gone` once the app leaves the bus.
    pub fn on_gone(&self, gone: impl Fn() + 'static) {
        let gone: Rc<dyn Fn()> = Rc::new(gone);
        self.proxy.connect_notify_local(Some("g-name-owner"), move |proxy, _| {
            if proxy.name_owner().is_none() {
                // Not from inside the proxy's own signal: removing the item drops the proxy.
                let gone = gone.clone();
                glib::idle_add_local_once(move || gone());
            }
        });
    }

    pub fn key(&self) -> String {
        format!("{}{}", self.bus, self.path)
    }

    pub fn activate(&self) -> impl Future<Output = Result<(), glib::Error>> + use<> {
        self.call("Activate", (0i32, 0i32).to_variant())
    }

    pub fn secondary_activate(&self) {
        glib::spawn_future_local(self.call("SecondaryActivate", (0i32, 0i32).to_variant()));
    }

    pub fn scroll(&self, delta: i32) {
        glib::spawn_future_local(self.call("Scroll", (delta, "vertical").to_variant()));
    }

    pub fn connection(&self) -> &gio::DBusConnection {
        &self.conn
    }

    fn call(&self, method: &str, args: glib::Variant) -> impl Future<Output = Result<(), glib::Error>> + use<> {
        let call = self.conn.call_future(
            Some(&self.bus),
            &self.path,
            IFACE,
            method,
            Some(&args),
            None,
            gio::DBusCallFlags::NONE,
            -1,
        );
        async move { call.await.map(|_| ()) }
    }

    /// Items announce changes with their own New* signals rather than PropertiesChanged,
    /// so the proxy's cache has to be refreshed by hand.
    /// Reads the properties again, at most every RELOAD_INTERVAL; signals in between fold
    /// into one read at the end of it, so the last change always shows.
    fn request_reload(self: &Rc<Self>, icon_changed: bool) {
        self.icon_changed.set(self.icon_changed.get() || icon_changed);
        if self.reload_due.replace(true) {
            return;
        }
        let wait = self
            .last_reload
            .get()
            .map_or(Duration::ZERO, |at| RELOAD_INTERVAL.saturating_sub(at.elapsed()));
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(wait, move || {
            if let Some(item) = weak.upgrade() {
                item.reload_due.set(false);
                item.last_reload.set(Some(Instant::now()));
                let icon_changed = item.icon_changed.replace(false);
                glib::spawn_future_local(item.reload(icon_changed));
            }
        });
    }

    async fn reload(self: Rc<Self>, icon_changed: bool) {
        let reply = self
            .conn
            .call_future(
                Some(&self.bus),
                &self.path,
                "org.freedesktop.DBus.Properties",
                "GetAll",
                Some(&(IFACE,).to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                -1,
            )
            .await;
        let Some(props) = reply.ok().and_then(|r| child(&r, 0)) else { return };
        for entry in props.iter() {
            let (Some(name), Some(value)) = (
                child(&entry, 0).and_then(|v| v.get::<String>()),
                child(&entry, 1).and_then(|v| v.as_variant()),
            ) else {
                continue;
            };
            self.proxy.set_cached_property(&name, Some(&value));
        }
        let serial = self.state.get().icon_serial + u64::from(icon_changed);
        self.state.set(self.read(serial));
    }

    fn read(&self, icon_serial: u64) -> ItemState {
        let string = |name: &str| {
            self.proxy
                .cached_property(name)
                .and_then(|v| v.get::<String>())
                .unwrap_or_default()
        };
        let status = match string("Status").as_str() {
            "Passive" => Status::Passive,
            "NeedsAttention" => Status::NeedsAttention,
            _ => Status::Active,
        };
        let theme_path = string("IconThemePath");
        let attention = status == Status::NeedsAttention;
        let icon = attention
            .then(|| self.icon_from("AttentionIconName", "AttentionIconPixmap", &theme_path))
            .filter(|icon| *icon != TrayIcon::None)
            .unwrap_or_else(|| self.icon_from("IconName", "IconPixmap", &theme_path));

        let title = self
            .proxy
            .cached_property("ToolTip")
            .and_then(|tip| child(&tip, 2))
            .and_then(|v| v.get::<String>())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| string("Title"));

        ItemState {
            icon,
            icon_serial,
            status,
            title,
            menu: self
                .proxy
                .cached_property("Menu")
                .and_then(|v| v.get::<glib::variant::ObjectPath>())
                .map(|p| p.as_str().to_string())
                .filter(|p| p != "/"),
            item_is_menu: self
                .proxy
                .cached_property("ItemIsMenu")
                .and_then(|v| v.get::<bool>())
                .unwrap_or(false),
        }
    }

    fn icon_from(&self, name_prop: &str, pixmap_prop: &str, theme_path: &str) -> TrayIcon {
        let name = self
            .proxy
            .cached_property(name_prop)
            .and_then(|v| v.get::<String>())
            .unwrap_or_default();
        if !name.is_empty() {
            if name.starts_with('/') && Path::new(&name).exists() {
                return TrayIcon::File(name.into());
            }
            if let Some(file) = find_in_theme_path(theme_path, &name) {
                return TrayIcon::File(file);
            }
            if has_themed_icon(&name) {
                return TrayIcon::Name(name);
            }
        }
        self.proxy
            .cached_property(pixmap_prop)
            .and_then(|v| v.get::<Vec<(i32, i32, Vec<u8>)>>())
            .and_then(|pixmaps| {
                // The biggest one; GTK scales it down.
                pixmaps
                    .into_iter()
                    .filter(|(w, h, data)| pixmap_size(*w, *h) == Some(data.len()))
                    .max_by_key(|(w, _, _)| *w)
            })
            .map_or(TrayIcon::None, |(width, height, data)| TrayIcon::Pixels { width, height, data })
    }
}

impl TrayIcon {
    pub fn apply(&self, image: &gtk4::Image) {
        match self {
            TrayIcon::None => image.set_icon_name(Some("image-missing")),
            TrayIcon::Name(name) => image.set_icon_name(Some(name)),
            TrayIcon::File(path) => image.set_from_file(Some(path)),
            TrayIcon::Pixels { width, height, data } => {
                image.set_paintable(Some(&pixmap_texture(*width, *height, data)));
            }
        }
    }
}

/// Bytes of a `width`×`height` ARGB32 pixmap; None for sizes that make no sense
/// (overflowing ones crashed the shell, a texture GDK refuses does too).
fn pixmap_size(width: i32, height: i32) -> Option<usize> {
    let (w, h) = (usize::try_from(width).ok()?, usize::try_from(height).ok()?);
    (w > 0 && h > 0).then(|| w.checked_mul(h)?.checked_mul(4)).flatten()
}

thread_local! {
    /// Textures of pixmap icons by their content: an icon that flips between a few
    /// states (a counter, a blinking dot) isn't made again each time.
    static TEXTURES: RefCell<HashMap<u64, gdk::Texture>> = RefCell::default();
}

fn pixmap_texture(width: i32, height: i32, data: &[u8]) -> gdk::Texture {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (width, height, data).hash(&mut hasher);
    let key = hasher.finish();
    if let Some(texture) = TEXTURES.with_borrow(|t| t.get(&key).cloned()) {
        return texture;
    }
    let texture: gdk::Texture = gdk::MemoryTexture::new(
        width,
        height,
        gdk::MemoryFormat::A8r8g8b8,
        &glib::Bytes::from(data),
        (width * 4) as usize,
    )
    .upcast();
    TEXTURES.with_borrow_mut(|t| {
        if t.len() >= 64 {
            t.clear();
        }
        t.insert(key, texture.clone());
    });
    texture
}

fn has_themed_icon(name: &str) -> bool {
    gdk::Display::default().is_some_and(|d| gtk4::IconTheme::for_display(&d).has_icon(name))
}

/// Apps with private icons point IconThemePath at a folder with them, either flat or
/// laid out like an icon theme.
fn find_in_theme_path(dir: &str, name: &str) -> Option<PathBuf> {
    if dir.is_empty() {
        return None;
    }
    let dir = Path::new(dir);
    let names = ["png", "svg", "xpm"].map(|ext| format!("{name}.{ext}"));
    let flat = names.iter().map(|n| dir.join(n)).find(|p| p.exists());
    flat.or_else(|| {
        let sizes = std::fs::read_dir(dir.join("hicolor")).ok()?;
        sizes
            .flatten()
            .flat_map(|size| names.iter().map(move |n| size.path().join("apps").join(n)))
            .find(|p| p.exists())
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn pixmap_sizes() {
        assert_eq!(super::pixmap_size(2, 3), Some(24));
        assert_eq!(super::pixmap_size(0, 3), None);
        assert_eq!(super::pixmap_size(-1, -1), None);
        assert_eq!(super::pixmap_size(i32::MAX, i32::MAX), Some(4 * (i32::MAX as usize).pow(2)));
    }
}

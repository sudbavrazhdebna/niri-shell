//! Notification server (`org.freedesktop.Notifications`): keeps the history for the
//! notification center and decides which notifications pop up.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gio, glib};

use super::dbus::{child, lookup};
use crate::watch::Watch;

const NAME: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";
/// Popups shown at once; newer ones push older ones into the center.
pub const MAX_POPUPS: usize = 3;

const XML: &str = r#"
<node>
  <interface name="org.freedesktop.Notifications">
    <method name="Notify">
      <arg type="s" direction="in"/><arg type="u" direction="in"/><arg type="s" direction="in"/>
      <arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="as" direction="in"/>
      <arg type="a{sv}" direction="in"/><arg type="i" direction="in"/>
      <arg type="u" direction="out"/>
    </method>
    <method name="CloseNotification"><arg type="u" direction="in"/></method>
    <method name="GetCapabilities"><arg type="as" direction="out"/></method>
    <method name="GetServerInformation">
      <arg type="s" direction="out"/><arg type="s" direction="out"/>
      <arg type="s" direction="out"/><arg type="s" direction="out"/>
    </method>
    <signal name="NotificationClosed"><arg type="u"/><arg type="u"/></signal>
    <signal name="ActionInvoked"><arg type="u"/><arg type="s"/></signal>
  </interface>
</node>"#;

/// Why a notification closed, as the spec numbers them.
#[derive(Clone, Copy)]
pub enum Reason {
    Expired = 1,
    Dismissed = 2,
    Closed = 3,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Image {
    /// RGB(A) pixels from the `image-data` hint.
    Pixels { width: i32, height: i32, stride: i32, alpha: bool, data: Vec<u8> },
    File(PathBuf),
    /// An icon theme name.
    Name(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Notification {
    pub id: u32,
    pub app_name: String,
    /// The app's desktop entry, if it said which.
    pub desktop_entry: Option<String>,
    pub summary: String,
    /// Pango markup, already checked to parse; plain text is escaped.
    pub body: String,
    /// (key, label); "default" is what a click on the card does.
    pub actions: Vec<(String, String)>,
    pub image: Option<Image>,
    pub critical: bool,
    /// How long the popup stays, in milliseconds.
    pub timeout_ms: u32,
    /// Stays after an action instead of closing.
    pub resident: bool,
    /// Never kept in the history.
    pub transient: bool,
    pub received: Option<glib::DateTime>,
}

impl Notification {
    pub fn default_action(&self) -> Option<&str> {
        self.actions.iter().find(|(key, _)| key == "default").map(|(key, _)| key.as_str())
    }
}

pub struct Notifications {
    /// Everything not closed yet, newest first.
    pub list: Watch<Vec<Rc<Notification>>>,
    /// Ids currently popped up, newest first.
    pub popups: Watch<Vec<u32>>,
    pub dnd: Watch<bool>,
    /// Something arrived since the center was last opened.
    pub unseen: Watch<bool>,
    conn: RefCell<Option<gio::DBusConnection>>,
    next_id: Cell<u32>,
}

impl Notifications {
    pub fn start() -> Rc<Notifications> {
        let notifications = Rc::new(Notifications {
            list: Watch::new(Vec::new()),
            popups: Watch::new(Vec::new()),
            dnd: Watch::new(load_dnd()),
            unseen: Watch::new(false),
            conn: RefCell::new(None),
            next_id: Cell::new(1),
        });
        let (on_bus, on_lost) = (Rc::downgrade(&notifications), Rc::downgrade(&notifications));
        let _owner = gio::bus_own_name(
            gio::BusType::Session,
            NAME,
            gio::BusNameOwnerFlags::NONE,
            move |conn, _| {
                if let Some(n) = on_bus.upgrade() {
                    n.register(&conn);
                }
            },
            |_, _| tracing::info!("notifications: serving"),
            move |_, _| {
                if on_lost.upgrade().is_some() {
                    tracing::warn!("notifications: another server (swaync?) owns the name");
                }
            },
        );
        notifications
    }

    pub fn set_dnd(&self, on: bool) {
        self.dnd.set(on);
        save_dnd(on);
        if on {
            // Like turning off a phone's screen: what's up now goes to the center.
            let critical: Vec<u32> = self
                .list
                .get()
                .iter()
                .filter(|n| n.critical)
                .map(|n| n.id)
                .collect();
            self.popups.update(|p| {
                let before = p.len();
                p.retain(|id| critical.contains(id));
                p.len() != before
            });
        }
    }

    pub fn mark_seen(&self) {
        self.unseen.set(false);
    }

    /// The popup's time is up: it stays in the center (unless transient).
    pub fn hide_popup(&self, id: u32) {
        self.popups.update(|p| {
            let before = p.len();
            p.retain(|&x| x != id);
            p.len() != before
        });
        let transient = self.list.get().iter().any(|n| n.id == id && n.transient);
        if transient {
            self.close(id, Reason::Expired);
        }
    }

    pub fn close(&self, id: u32, reason: Reason) {
        let mut found = false;
        self.list.update(|list| {
            let before = list.len();
            list.retain(|n| n.id != id);
            found = list.len() != before;
            found
        });
        self.popups.update(|p| {
            let before = p.len();
            p.retain(|&x| x != id);
            p.len() != before
        });
        if found {
            self.emit("NotificationClosed", &(id, reason as u32).to_variant());
        }
    }

    pub fn clear_all(&self) {
        let ids: Vec<u32> = self.list.get().iter().map(|n| n.id).collect();
        for id in ids {
            self.close(id, Reason::Dismissed);
        }
    }

    /// Runs one of the notification's actions in its app.
    pub fn invoke(&self, id: u32, action: &str) {
        let resident = self.list.get().iter().any(|n| n.id == id && n.resident);
        self.emit("ActionInvoked", &(id, action).to_variant());
        if !resident {
            self.close(id, Reason::Dismissed);
        }
    }

    fn emit(&self, signal: &str, args: &glib::Variant) {
        if let Some(conn) = &*self.conn.borrow()
            && let Err(err) = conn.emit_signal(None, PATH, NAME, signal, Some(args))
        {
            tracing::warn!("notifications: {signal}: {err}");
        }
    }

    fn register(self: &Rc<Self>, conn: &gio::DBusConnection) {
        self.conn.replace(Some(conn.clone()));
        let info = gio::DBusNodeInfo::for_xml(XML).ok().and_then(|n| n.lookup_interface(NAME));
        let Some(info) = info else {
            tracing::error!("notifications: bad interface description");
            return;
        };
        let weak = Rc::downgrade(self);
        let result = conn
            .register_object(PATH, &info)
            .method_call(move |_, _, _, _, method, params, invocation| {
                let Some(this) = weak.upgrade() else { return };
                match method {
                    "Notify" => {
                        let id = this.notify(&params);
                        invocation.return_value(Some(&(id,).to_variant()));
                    }
                    "CloseNotification" => {
                        if let Some((id,)) = params.get::<(u32,)>() {
                            this.close(id, Reason::Closed);
                        }
                        invocation.return_value(None);
                    }
                    "GetCapabilities" => {
                        let caps = vec!["actions", "body", "body-markup", "icon-static", "persistence"];
                        invocation.return_value(Some(&(caps,).to_variant()));
                    }
                    "GetServerInformation" => invocation.return_value(Some(
                        &("nshell", "nshell", env!("CARGO_PKG_VERSION"), "1.2").to_variant(),
                    )),
                    _ => invocation.return_error(gio::IOErrorEnum::NotSupported, "unknown method"),
                }
            })
            .build();
        if let Err(err) = result {
            tracing::error!("notifications: can't register: {err}");
        }
    }

    fn notify(&self, params: &glib::Variant) -> u32 {
        let text = |i: usize| child(params, i).and_then(|v| v.get::<String>()).unwrap_or_default();
        let replaces = child(params, 1).and_then(|v| v.get::<u32>()).unwrap_or(0);
        let app_icon = text(2);
        let actions: Vec<String> = child(params, 5).and_then(|v| v.get()).unwrap_or_default();
        let hints = child(params, 6).unwrap_or_else(|| glib::VariantDict::new(None).end());
        let expire = child(params, 7).and_then(|v| v.get::<i32>()).unwrap_or(-1);
        let hint = |key: &str| lookup(&hints, key);

        let id = if replaces != 0 && self.list.get().iter().any(|n| n.id == replaces) {
            replaces
        } else {
            let id = self.next_id.get();
            self.next_id.set(id.wrapping_add(1).max(1));
            id
        };
        let desktop_entry = hint("desktop-entry").and_then(|v| v.get::<String>()).filter(|s| !s.is_empty());
        let notification = Notification {
            id,
            app_name: text(0),
            summary: text(3),
            body: checked_markup(&text(4)),
            actions: actions
                .as_chunks::<2>().0.iter()
                .map(|pair| (pair[0].clone(), pair[1].clone()))
                .collect(),
            image: image(&hint("image-data").or_else(|| hint("image_data")), &hint("image-path").or_else(|| hint("image_path")))
                .or_else(|| icon_image(&app_icon))
                .or_else(|| hint("icon_data").and_then(|v| pixels(&v))),
            critical: hint("urgency").and_then(|v| v.get::<u8>()) == Some(2),
            // The user's swaync setup showed everything for 5 s; an app may ask for longer.
            timeout_ms: if expire > 0 { expire.max(5000) as u32 } else { 5000 },
            resident: hint("resident").and_then(|v| v.get::<bool>()).unwrap_or(false),
            transient: hint("transient").and_then(|v| v.get::<bool>()).unwrap_or(false),
            received: glib::DateTime::now_local().ok(),
            desktop_entry,
        };
        let critical = notification.critical;
        let transient = notification.transient;
        self.list.update(|list| {
            list.retain(|n| n.id != id);
            list.insert(0, Rc::new(notification));
            true
        });
        if !transient {
            self.unseen.set(true);
        }
        if !*self.dnd.get() || critical {
            self.popups.update(|p| {
                p.retain(|&x| x != id);
                p.insert(0, id);
                p.truncate(MAX_POPUPS);
                true
            });
        }
        id
    }
}

/// The body as Pango markup: kept if it parses, escaped otherwise (apps send stray `&`).
fn checked_markup(body: &str) -> String {
    if gtk4::pango::parse_markup(body, '\0').is_ok() {
        body.to_string()
    } else {
        glib::markup_escape_text(body).to_string()
    }
}

fn image(data: &Option<glib::Variant>, path: &Option<glib::Variant>) -> Option<Image> {
    data.as_ref()
        .and_then(pixels)
        .or_else(|| path.as_ref().and_then(|v| v.get::<String>()).and_then(|p| icon_image(&p)))
}

/// `(iiibiiay)`: width, height, rowstride, has alpha, bits per sample, channels, data.
/// Checked the way GDK checks it: a texture it refuses would take the shell down.
fn pixels(v: &glib::Variant) -> Option<Image> {
    let (width, height, stride, alpha, bits, channels, data) =
        v.get::<(i32, i32, i32, bool, i32, i32, Vec<u8>)>()?;
    if width <= 0 || height <= 0 || bits != 8 || channels != if alpha { 4 } else { 3 } {
        return None;
    }
    let row = usize::try_from(width).ok()?.checked_mul(usize::try_from(channels).ok()?)?;
    let stride_bytes = usize::try_from(stride).ok().filter(|s| *s >= row)?;
    let expected = stride_bytes.checked_mul(usize::try_from(height - 1).ok()?)?.checked_add(row)?;
    (data.len() >= expected).then_some(Image::Pixels { width, height, stride, alpha, data })
}

/// An icon given as a path, a file:// URI or a theme name.
fn icon_image(icon: &str) -> Option<Image> {
    if icon.is_empty() {
        return None;
    }
    if let Some(path) = icon.strip_prefix("file://") {
        return Some(Image::File(glib::Uri::unescape_string(path, None::<&str>).map_or_else(|| path.into(), |p| p.as_str().into())));
    }
    if icon.starts_with('/') {
        return Some(Image::File(icon.into()));
    }
    Some(Image::Name(icon.to_string()))
}

fn dnd_file() -> PathBuf {
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| glib::home_dir().join(".local/state"));
    state.join("nshell").join("dnd")
}

fn load_dnd() -> bool {
    std::fs::read_to_string(dnd_file()).is_ok_and(|s| s.trim() == "1")
}

fn save_dnd(on: bool) {
    let file = dnd_file();
    let result = file
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&file, if on { "1" } else { "0" }));
    if let Err(err) = result {
        tracing::warn!("can't save do-not-disturb to {}: {err}", file.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_is_kept_or_escaped() {
        assert_eq!(checked_markup("<b>hi</b>"), "<b>hi</b>");
        assert_eq!(checked_markup("Tom & Jerry"), "Tom &amp; Jerry");
    }

    #[test]
    fn pixel_data_is_checked() {
        let good = (2i32, 2i32, 8i32, true, 8i32, 4i32, vec![0u8; 16]).to_variant();
        assert!(matches!(pixels(&good), Some(Image::Pixels { .. })));
        let short = (2i32, 2i32, 8i32, true, 8i32, 4i32, vec![0u8; 4]).to_variant();
        assert!(pixels(&short).is_none());
        // Rows overlapping each other: GDK refuses that texture.
        let narrow = (2i32, 2i32, 0i32, true, 8i32, 4i32, vec![0u8; 16]).to_variant();
        assert!(pixels(&narrow).is_none());
        let huge = (i32::MAX, 2i32, i32::MAX, true, 8i32, 4i32, vec![0u8; 16]).to_variant();
        assert!(pixels(&huge).is_none());
    }
}

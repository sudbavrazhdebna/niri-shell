//! niri IPC on the GTK main loop: one event-stream connection that keeps a copy of the
//! compositor state, plus one-shot requests for actions. No threads: the socket is read
//! asynchronously by glib.

use std::collections::HashMap;

use gtk4::gio;
use gtk4::prelude::*;
use niri_ipc::{Action, Event, KeyboardLayouts, Reply, Request, Response, Window, Workspace};

use crate::watch::Watch;

/// What the shell knows about niri, updated from the event stream.
#[derive(Default)]
pub struct State {
    /// Bumped on every change except a window's title alone: titles change many times a
    /// second (terminal spinners), and most things don't show them.
    pub layout_serial: u64,
    pub windows: HashMap<u64, Window>,
    pub workspaces: HashMap<u64, Workspace>,
    pub keyboard_layouts: Option<KeyboardLayouts>,
    /// The overview is open: windows are shrunk away from the screen's edges.
    pub overview_open: bool,
}

impl State {
    /// Whether the output's shown window is maximized to the edges: its whole `width`,
    /// no gaps, right under the bar. The active window of the active workspace is always
    /// in view; columns fill the height, so the top tile of such a one touches the bar.
    pub fn covers_top(&self, output: &str, width: f64) -> bool {
        self.active_window_on(output).is_some_and(|w| {
            top_band(&w.layout).is_some() && w.layout.tile_size.0 >= width - 2.0 * EDGE_SLACK
        })
    }

    /// Like [`covers_top`](Self::covers_top) for a bar at the bottom: the shown window
    /// is tiled the whole `width`. Columns fill the height, so it reaches the bottom
    /// (its lowest tile does; which tile is lowest niri doesn't say, so any counts).
    pub fn covers_bottom(&self, output: &str, width: f64) -> bool {
        self.active_window_on(output).is_some_and(|w| {
            w.layout.pos_in_scrolling_layout.is_some() && w.layout.tile_size.0 >= width - 2.0 * EDGE_SLACK
        })
    }

    /// The window shown on an output: the active window of its active workspace.
    pub fn active_window_on(&self, output: &str) -> Option<&Window> {
        let workspace = self
            .workspaces
            .values()
            .find(|ws| ws.is_active && ws.output.as_deref() == Some(output))?;
        self.windows.get(&workspace.active_window_id?)
    }

    /// Applies one event. Returns false for events the shell doesn't care about,
    /// so listeners aren't woken for them.
    fn apply(&mut self, event: Event) -> bool {
        // Window positions change nothing that `layout_serial` stands for (who is open
        // where, focus); listeners that care look at `top_band` themselves.
        let positions_only = matches!(&event, Event::WindowLayoutsChanged { .. });
        let title_only = positions_only || matches!(&event, Event::WindowOpenedOrChanged { window }
            if self.windows.get(&window.id).is_some_and(|old| {
                old.app_id == window.app_id
                    && old.workspace_id == window.workspace_id
                    && old.is_focused == window.is_focused
                    && old.is_floating == window.is_floating
                    && old.is_urgent == window.is_urgent
            }));
        let changed = self.apply_event(event);
        if changed && !title_only {
            self.layout_serial += 1;
        }
        changed
    }

    fn apply_event(&mut self, event: Event) -> bool {
        match event {
            Event::WorkspacesChanged { workspaces } => {
                self.workspaces = workspaces.into_iter().map(|ws| (ws.id, ws)).collect();
            }
            Event::WorkspaceActivated { id, focused } => {
                let Some(output) = self.workspaces.get(&id).and_then(|ws| ws.output.clone())
                else {
                    return false;
                };
                for ws in self.workspaces.values_mut() {
                    if ws.output.as_ref() == Some(&output) {
                        ws.is_active = ws.id == id;
                    }
                    if focused {
                        ws.is_focused = ws.id == id;
                    }
                }
            }
            Event::WorkspaceActiveWindowChanged {
                workspace_id,
                active_window_id,
            } => {
                let Some(ws) = self.workspaces.get_mut(&workspace_id) else {
                    return false;
                };
                ws.active_window_id = active_window_id;
            }
            Event::WorkspaceUrgencyChanged { id, urgent } => {
                let Some(ws) = self.workspaces.get_mut(&id) else {
                    return false;
                };
                ws.is_urgent = urgent;
            }
            Event::WindowsChanged { windows } => {
                self.windows = windows.into_iter().map(|w| (w.id, w)).collect();
            }
            Event::WindowOpenedOrChanged { window } => {
                if window.is_focused {
                    self.set_focus(Some(window.id));
                }
                self.windows.insert(window.id, window);
            }
            // These come with every resize and animation frame: only a window reaching or
            // leaving the top band (or changing width while in it) wakes anyone.
            Event::WindowLayoutsChanged { changes } => {
                let mut noticed = false;
                for (id, layout) in changes {
                    let Some(window) = self.windows.get_mut(&id) else { continue };
                    let before = top_band(&window.layout);
                    window.layout = layout;
                    noticed |= top_band(&window.layout) != before;
                }
                if !noticed {
                    return false;
                }
            }
            Event::WindowClosed { id } => {
                self.windows.remove(&id);
            }
            Event::WindowFocusChanged { id } => self.set_focus(id),
            Event::WindowUrgencyChanged { id, urgent } => {
                let Some(window) = self.windows.get_mut(&id) else {
                    return false;
                };
                window.is_urgent = urgent;
            }
            Event::OverviewOpenedOrClosed { is_open } => {
                if self.overview_open == is_open {
                    return false;
                }
                self.overview_open = is_open;
            }
            Event::KeyboardLayoutsChanged { keyboard_layouts } => {
                self.keyboard_layouts = Some(keyboard_layouts);
            }
            Event::KeyboardLayoutSwitched { idx } => {
                let Some(layouts) = &mut self.keyboard_layouts else {
                    return false;
                };
                layouts.current_idx = idx;
            }
            _ => return false,
        }
        true
    }

    fn set_focus(&mut self, id: Option<u64>) {
        for window in self.windows.values_mut() {
            window.is_focused = Some(window.id) == id;
        }
    }
}

/// Slack when telling whether a tile spans the screen's whole width, in logical pixels:
/// none for gaps, only for rounding (a column maximized with Mod+F keeps the gaps).
pub const EDGE_SLACK: f64 = 1.0;

/// For the top tile of a column, its width in steps of 16 px: what the bar's look depends
/// on. (niri 26.04 leaves `tile_pos_in_workspace_view` empty, so position can't be used.)
fn top_band(layout: &niri_ipc::WindowLayout) -> Option<i64> {
    let (_, tile) = layout.pos_in_scrolling_layout?;
    (tile == 1).then(|| (layout.tile_size.0 / 16.0).round() as i64)
}

/// Handle to the niri connection. Cheap to clone.
#[derive(Clone)]
pub struct Niri {
    pub state: Watch<State>,
}

impl Niri {
    /// Connects to the event stream in the background. Until the first events arrive,
    /// the state is empty.
    pub fn connect() -> Niri {
        let niri = Niri {
            state: Watch::new(State::default()),
        };
        let this = niri.clone();
        gtk4::glib::spawn_future_local(async move {
            if let Err(err) = this.run_event_stream().await {
                tracing::error!("niri event stream: {err}");
            }
        });
        niri
    }

    /// The monitor of the focused workspace, or the first one: where popups and the OSD go.
    pub fn focused_monitor(&self) -> Option<gtk4::gdk::Monitor> {
        let output = {
            let state = self.state.get();
            state.workspaces.values().find(|ws| ws.is_focused).and_then(|ws| ws.output.clone())
        };
        let monitors = gtk4::gdk::Display::default()?.monitors();
        let all: Vec<gtk4::gdk::Monitor> = (0..monitors.n_items())
            .filter_map(|i| monitors.item(i).and_downcast::<gtk4::gdk::Monitor>())
            .collect();
        all.iter()
            .find(|m| m.connector().as_deref() == output.as_deref())
            .or(all.first())
            .cloned()
    }

    /// Sends an action to niri without waiting for the result.
    pub fn action(&self, action: Action) {
        gtk4::glib::spawn_future_local(async move {
            if let Err(err) = request(Request::Action(action)).await {
                tracing::warn!("niri action failed: {err}");
            }
        });
    }

    async fn run_event_stream(&self) -> Result<(), String> {
        let (_conn, mut lines) = open(&Request::EventStream).await?;
        match read_reply(&mut lines).await? {
            Ok(Response::Handled) => {}
            Ok(other) => return Err(format!("unexpected reply {other:?}")),
            Err(msg) => return Err(msg),
        }
        tracing::info!("connected to niri");

        loop {
            let line = read_line(&mut lines).await?;
            let event = match serde_json::from_str::<Event>(&line) {
                Ok(event) => event,
                // A newer niri may send events this build doesn't know yet.
                Err(err) => {
                    tracing::debug!("skipping niri event: {err}");
                    continue;
                }
            };
            self.state.update(|state| state.apply(event));
        }
    }
}

/// Sends one request and waits for niri's answer.
pub async fn request(request: Request) -> Result<Response, String> {
    let (_conn, mut lines) = open(&request).await?;
    read_reply(&mut lines).await?
}

/// Opens a connection to niri and sends one request. The connection must stay alive
/// while the stream is read.
async fn open(request: &Request) -> Result<(gio::SocketConnection, gio::DataInputStream), String> {
    let path = std::env::var("NIRI_SOCKET").map_err(|_| "NIRI_SOCKET is not set".to_string())?;
    let address = gio::UnixSocketAddress::new(std::path::Path::new(&path));
    let conn = gio::SocketClient::new()
        .connect_future(&address)
        .await
        .map_err(|e| format!("connect {path}: {e}"))?;

    let mut line = serde_json::to_string(request).map_err(|e| e.to_string())?;
    line.push('\n');
    conn.output_stream()
        .write_all_future(line.into_bytes(), gtk4::glib::Priority::DEFAULT)
        .await
        .map_err(|(_, e)| format!("write: {e}"))?;

    let lines = gio::DataInputStream::new(&conn.input_stream());
    Ok((conn, lines))
}

async fn read_line(lines: &mut gio::DataInputStream) -> Result<String, String> {
    match lines.read_line_utf8_future(gtk4::glib::Priority::DEFAULT).await {
        Ok(Some(line)) => Ok(line.into()),
        Ok(None) => Err("niri closed the connection".into()),
        Err(e) => Err(format!("read: {e}")),
    }
}

async fn read_reply(lines: &mut gio::DataInputStream) -> Result<Reply, String> {
    let line = read_line(lines).await?;
    serde_json::from_str(&line).map_err(|e| format!("bad reply: {e}"))
}

//! niri IPC on the GTK main loop: one event-stream connection that keeps a copy of the
//! compositor state, plus one-shot requests for actions. No threads: the socket is read
//! asynchronously by glib.

use std::cell::{Ref, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use gtk4::gio;
use gtk4::prelude::*;
use niri_ipc::{Action, Event, KeyboardLayouts, Reply, Request, Response, Window, Workspace};

/// What the shell knows about niri, updated from the event stream.
#[derive(Default)]
pub struct State {
    pub windows: HashMap<u64, Window>,
    pub workspaces: HashMap<u64, Workspace>,
    pub keyboard_layouts: Option<KeyboardLayouts>,
}

impl State {
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
            Event::KeyboardLayoutsChanged { keyboard_layouts } => {
                self.keyboard_layouts = Some(keyboard_layouts);
            }
            Event::KeyboardLayoutSwitched { idx } => {
                let Some(layouts) = &mut self.keyboard_layouts else {
                    return false;
                };
                layouts.current_idx = idx;
            }
            // Layout changes fire on every resize and animation frame; nothing in the
            // shell shows them.
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

type Listener = Box<dyn Fn(&State) -> bool>;

/// Shared handle to the niri connection. Cheap to clone.
#[derive(Clone)]
pub struct Niri(Rc<Inner>);

struct Inner {
    state: RefCell<State>,
    listeners: RefCell<Vec<Listener>>,
}

impl Niri {
    /// Connects to the event stream in the background. Until the first events arrive,
    /// the state is empty.
    pub fn connect() -> Niri {
        let niri = Niri(Rc::new(Inner {
            state: RefCell::new(State::default()),
            listeners: RefCell::new(Vec::new()),
        }));

        let this = niri.clone();
        gtk4::glib::spawn_future_local(async move {
            if let Err(err) = this.run_event_stream().await {
                tracing::error!("niri event stream: {err}");
            }
        });
        niri
    }

    pub fn state(&self) -> Ref<'_, State> {
        self.0.state.borrow()
    }

    /// Calls `listener` now and after every state change, for as long as it returns true.
    /// Widgets return false once they are gone.
    pub fn subscribe(&self, listener: impl Fn(&State) -> bool + 'static) {
        if listener(&self.state()) {
            self.0.listeners.borrow_mut().push(Box::new(listener));
        }
    }

    /// Sends an action to niri without waiting for the result.
    #[allow(dead_code)] // first user: the keyboard module (step 3)
    pub fn action(&self, action: Action) {
        gtk4::glib::spawn_future_local(async move {
            let result = async {
                let (_conn, mut lines) = open(&Request::Action(action)).await?;
                match read_reply(&mut lines).await? {
                    Ok(_) => Ok(()),
                    Err(msg) => Err(msg),
                }
            };
            if let Err(err) = result.await {
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
            if self.0.state.borrow_mut().apply(event) {
                self.notify();
            }
        }
    }

    fn notify(&self) {
        let state = self.0.state.borrow();
        // A listener may subscribe others while running; collect those separately.
        let listeners = std::mem::take(&mut *self.0.listeners.borrow_mut());
        let mut alive: Vec<Listener> = listeners.into_iter().filter(|l| l(&state)).collect();
        let mut listeners = self.0.listeners.borrow_mut();
        alive.append(&mut listeners);
        *listeners = alive;
    }
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

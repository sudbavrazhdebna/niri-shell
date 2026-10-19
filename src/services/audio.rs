//! Default output volume through PulseAudio (served by PipeWire). libpulse runs on the
//! GTK main loop, so no extra threads.

use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::time::Duration;

use gtk4::glib;
use libpulse_binding::callbacks::ListResult;
use libpulse_binding::context::subscribe::{Facility, InterestMaskSet};
use libpulse_binding::context::{Context, FlagSet, State};
use libpulse_binding::volume::{ChannelVolumes, Volume};
use libpulse_glib_binding::Mainloop;

use crate::watch::Watch;

/// Scrolling and the slider never go above this, like `max-volume 100` in waybar.
pub const MAX_PERCENT: u32 = 100;

#[derive(Clone, Default, PartialEq, Debug)]
pub struct AudioState {
    /// False until connected, and when there is no output device.
    pub available: bool,
    /// 100 = the device's normal level.
    pub percent: u32,
    pub muted: bool,
}

pub struct Audio {
    pub state: Watch<AudioState>,
    mainloop: Mainloop,
    context: RefCell<Option<Context>>,
    sink: RefCell<Option<Sink>>,
}

/// The default output as last seen.
struct Sink {
    name: String,
    volume: ChannelVolumes,
}

impl Audio {
    pub fn start() -> Option<Rc<Audio>> {
        let Some(mainloop) = Mainloop::new(None) else {
            tracing::error!("pulse: can't create a GLib main loop");
            return None;
        };
        let audio = Rc::new(Audio {
            state: Watch::new(AudioState::default()),
            mainloop,
            context: RefCell::new(None),
            sink: RefCell::new(None),
        });
        audio.connect();
        Some(audio)
    }

    /// Sets the output volume, keeping the balance between channels.
    pub fn set_percent(&self, percent: u32) {
        let percent = percent.min(MAX_PERCENT);
        let Some(context) = &*self.context.borrow() else {
            return;
        };
        let mut sink = self.sink.borrow_mut();
        let Some(sink) = sink.as_mut() else {
            return;
        };
        let target = Volume((f64::from(Volume::NORMAL.0) * f64::from(percent) / 100.0) as u32);
        if sink.volume.scale(target).is_none() {
            return;
        }
        context
            .introspect()
            .set_sink_volume_by_name(&sink.name, &sink.volume, None);
    }

    pub fn change_percent(&self, delta: i32) {
        let current = self.state.get().percent;
        let target = current.saturating_add_signed(delta);
        self.set_percent(target);
    }

    pub fn toggle_mute(&self) {
        let muted = self.state.get().muted;
        let Some(context) = &*self.context.borrow() else {
            return;
        };
        if let Some(sink) = &*self.sink.borrow() {
            context
                .introspect()
                .set_sink_mute_by_name(&sink.name, !muted, None);
        }
    }

    fn connect(self: &Rc<Self>) {
        let Some(mut context) = Context::new(&self.mainloop, "nshell") else {
            tracing::error!("pulse: can't create a context");
            return;
        };
        let weak = Rc::downgrade(self);
        context.set_state_callback(Some(Box::new(move || {
            if let Some(this) = weak.upgrade() {
                this.on_state();
            }
        })));
        if let Err(err) = context.connect(None, FlagSet::NOFAIL, None) {
            tracing::warn!("pulse: {err}");
        }
        self.context.replace(Some(context));
    }

    fn on_state(self: &Rc<Self>) {
        let state = match &*self.context.borrow() {
            Some(context) => context.get_state(),
            None => return,
        };
        match state {
            State::Ready => self.on_ready(),
            State::Failed | State::Terminated => {
                tracing::warn!("pulse: disconnected, retrying");
                self.state.set(AudioState::default());
                // The context can't be dropped from inside its own callback.
                let weak = Rc::downgrade(self);
                glib::timeout_add_local_once(Duration::from_secs(2), move || {
                    if let Some(this) = weak.upgrade() {
                        this.context.replace(None);
                        this.connect();
                    }
                });
            }
            _ => {}
        }
    }

    fn on_ready(self: &Rc<Self>) {
        tracing::info!("connected to pulse");
        {
            let mut context = self.context.borrow_mut();
            let Some(context) = context.as_mut() else {
                return;
            };
            let weak = Rc::downgrade(self);
            context.set_subscribe_callback(Some(Box::new(move |facility, _, _| {
                let Some(this) = weak.upgrade() else {
                    return;
                };
                match facility {
                    // The default device may have changed.
                    Some(Facility::Server) => this.refresh_default(),
                    Some(Facility::Sink) => this.refresh_sink(),
                    _ => {}
                }
            })));
            context.subscribe(InterestMaskSet::SINK | InterestMaskSet::SERVER, |_| {});
        }
        self.refresh_default();
    }

    fn refresh_default(self: &Rc<Self>) {
        let Some(context) = &*self.context.borrow() else {
            return;
        };
        let weak = Rc::downgrade(self);
        context.introspect().get_server_info(move |info| {
            let Some(this) = weak.upgrade() else {
                return;
            };
            match info.default_sink_name.as_deref() {
                Some(name) => {
                    let name = name.to_string();
                    this.query_sink(&name);
                }
                None => {
                    this.sink.replace(None);
                    this.state.set(AudioState::default());
                }
            }
        });
    }

    fn refresh_sink(self: &Rc<Self>) {
        let name = self.sink.borrow().as_ref().map(|s| s.name.clone());
        match name {
            Some(name) => self.query_sink(&name),
            None => self.refresh_default(),
        }
    }

    fn query_sink(self: &Rc<Self>, name: &str) {
        let Some(context) = &*self.context.borrow() else {
            return;
        };
        let weak: Weak<Audio> = Rc::downgrade(self);
        context
            .introspect()
            .get_sink_info_by_name(name, move |result| {
                let (Some(this), ListResult::Item(info)) = (weak.upgrade(), result) else {
                    return;
                };
                let Some(name) = info.name.as_deref() else {
                    return;
                };
                let percent = (f64::from(info.volume.avg().0) * 100.0
                    / f64::from(Volume::NORMAL.0))
                .round() as u32;
                this.sink.replace(Some(Sink {
                    name: name.to_string(),
                    volume: info.volume,
                }));
                this.state.set(AudioState {
                    available: true,
                    percent,
                    muted: info.mute,
                });
            });
    }
}

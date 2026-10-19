//! Default output volume through PulseAudio (served by PipeWire). libpulse runs on the
//! GTK main loop, so no extra threads.

use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::time::Duration;

use gtk4::glib;
use libpulse_binding::callbacks::ListResult;
use libpulse_binding::context::subscribe::{Facility, InterestMaskSet, Operation};
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
    /// All output devices; `default` is the one in use.
    pub outputs: Vec<Output>,
    pub default: String,
}

#[derive(Clone, Default, PartialEq, Debug)]
pub struct Output {
    pub name: String,
    pub description: String,
}

/// A program playing sound right now.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct AppStream {
    pub index: u32,
    pub name: String,
    /// Icon name the app gave, if any.
    pub icon: String,
    pub percent: u32,
    pub muted: bool,
}

/// The default input (microphone).
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Mic {
    pub percent: u32,
    pub muted: bool,
}

pub struct Audio {
    pub state: Watch<AudioState>,
    pub apps: Watch<Vec<AppStream>>,
    pub mic: Watch<Option<Mic>>,
    mainloop: Mainloop,
    context: RefCell<Option<Context>>,
    sink: RefCell<Option<Sink>>,
    /// The default input as last seen (same shape as the output).
    source: RefCell<Option<Sink>>,
    /// Channel volumes of the app streams, to change one keeping its balance.
    app_volumes: RefCell<std::collections::HashMap<u32, ChannelVolumes>>,
}

/// The default output as last seen.
struct Sink {
    index: u32,
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
            apps: Watch::new(Vec::new()),
            mic: Watch::new(None),
            mainloop,
            context: RefCell::new(None),
            sink: RefCell::new(None),
            source: RefCell::new(None),
            app_volumes: RefCell::default(),
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
        if sink.volume.scale(to_volume(percent)).is_none() {
            return;
        }
        context
            .introspect()
            .set_sink_volume_by_name(&sink.name, &sink.volume, None);
    }

    /// Sets one app's volume.
    pub fn set_app_percent(&self, index: u32, percent: u32) {
        let Some(context) = &*self.context.borrow() else { return };
        let mut volumes = self.app_volumes.borrow_mut();
        let Some(volume) = volumes.get_mut(&index) else { return };
        if volume.scale(to_volume(percent)).is_some() {
            context.introspect().set_sink_input_volume(index, volume, None);
        }
    }

    pub fn toggle_app_mute(&self, index: u32) {
        let muted = self.apps.get().iter().any(|a| a.index == index && a.muted);
        if let Some(context) = &*self.context.borrow() {
            context.introspect().set_sink_input_mute(index, !muted, None);
        }
    }

    pub fn set_mic_percent(&self, percent: u32) {
        let Some(context) = &*self.context.borrow() else { return };
        let mut source = self.source.borrow_mut();
        let Some(source) = source.as_mut() else { return };
        if source.volume.scale(to_volume(percent)).is_some() {
            context.introspect().set_source_volume_by_name(&source.name, &source.volume, None);
        }
    }

    pub fn toggle_mic_mute(&self) {
        let muted = self.mic.get().as_ref().is_some_and(|m| m.muted);
        let Some(context) = &*self.context.borrow() else { return };
        if let Some(source) = &*self.source.borrow() {
            context.introspect().set_source_mute_by_name(&source.name, !muted, None);
        }
    }

    pub fn change_percent(&self, delta: i32) {
        let current = self.state.get().percent;
        let target = current.saturating_add_signed(delta);
        self.set_percent(target);
    }

    /// Makes `name` the output in use.
    pub fn set_default(&self, name: &str) {
        if let Some(context) = &mut *self.context.borrow_mut() {
            context.set_default_sink(name, |_| {});
        }
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
            context.set_subscribe_callback(Some(Box::new(move |facility, operation, index| {
                let Some(this) = weak.upgrade() else {
                    return;
                };
                match (facility, operation) {
                    // The default device may have changed.
                    (Some(Facility::Server), _) => this.refresh_default(),
                    // Volume steps arrive here many times a second: only the output in
                    // use matters, and the device list stays the same.
                    (Some(Facility::Sink), Some(Operation::Changed)) => {
                        if this.sink.borrow().as_ref().is_some_and(|s| s.index == index) {
                            this.refresh_sink();
                        }
                    }
                    (Some(Facility::Sink), _) => this.refresh_outputs(),
                    (Some(Facility::SinkInput), _) => this.refresh_apps(),
                    (Some(Facility::Source), Some(Operation::Changed))
                        if this.source.borrow().as_ref().is_some_and(|s| s.index == index) => {
                            this.refresh_mic();
                        }
                    _ => {}
                }
            })));
            context.subscribe(
                InterestMaskSet::SINK
                    | InterestMaskSet::SERVER
                    | InterestMaskSet::SINK_INPUT
                    | InterestMaskSet::SOURCE,
                |_| {},
            );
        }
        self.refresh_default();
        self.refresh_outputs();
        self.refresh_apps();
    }

    /// Device list for the menu. Devices come and go rarely, so this re-reads all.
    fn refresh_outputs(self: &Rc<Self>) {
        let Some(context) = &*self.context.borrow() else {
            return;
        };
        let weak = Rc::downgrade(self);
        let found = Rc::new(RefCell::new(Vec::new()));
        context.introspect().get_sink_info_list(move |result| {
            let Some(this) = weak.upgrade() else { return };
            match result {
                ListResult::Item(info) => found.borrow_mut().push(Output {
                    name: info.name.as_deref().unwrap_or_default().to_string(),
                    description: info.description.as_deref().unwrap_or_default().to_string(),
                }),
                ListResult::End => {
                    let outputs = found.take();
                    this.state.update(|state| {
                        let changed = state.outputs != outputs;
                        state.outputs = outputs;
                        changed
                    });
                }
                ListResult::Error => {}
            }
        });
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
            match info.default_source_name.as_deref() {
                Some(name) => {
                    let name = name.to_string();
                    this.query_source(&name);
                }
                None => {
                    this.source.replace(None);
                    this.mic.set(None);
                }
            }
            match info.default_sink_name.as_deref() {
                Some(name) => {
                    let name = name.to_string();
                    this.query_sink(&name);
                }
                None => {
                    this.sink.replace(None);
                    this.state.update(|state| {
                        state.available = false;
                        true
                    });
                }
            }
        });
    }

    /// Programs playing sound. Their list changes as they start and stop.
    fn refresh_apps(self: &Rc<Self>) {
        let Some(context) = &*self.context.borrow() else { return };
        let weak = Rc::downgrade(self);
        let found = Rc::new(RefCell::new(Vec::new()));
        let volumes = Rc::new(RefCell::new(std::collections::HashMap::new()));
        context.introspect().get_sink_input_info_list(move |result| {
            let Some(this) = weak.upgrade() else { return };
            match result {
                ListResult::Item(info) => {
                    let prop = |key: &str| info.proplist.get_str(key).unwrap_or_default();
                    // Event sounds (notifications, clicks) aren't apps to adjust.
                    if prop("media.role") == "event" || info.corked {
                        return;
                    }
                    let name = Some(prop("application.name"))
                        .filter(|n| !n.is_empty())
                        .or_else(|| info.name.as_deref().map(String::from))
                        .unwrap_or_default();
                    volumes.borrow_mut().insert(info.index, info.volume);
                    found.borrow_mut().push(AppStream {
                        index: info.index,
                        name,
                        icon: prop("application.icon_name"),
                        percent: percent_of(&info.volume),
                        muted: info.mute,
                    });
                }
                ListResult::End => {
                    this.app_volumes.replace(volumes.take());
                    this.apps.set(found.take());
                }
                ListResult::Error => {}
            }
        });
    }

    fn refresh_mic(self: &Rc<Self>) {
        let name = self.source.borrow().as_ref().map(|s| s.name.clone());
        if let Some(name) = name {
            self.query_source(&name);
        }
    }

    fn query_source(self: &Rc<Self>, name: &str) {
        let Some(context) = &*self.context.borrow() else { return };
        let weak = Rc::downgrade(self);
        context.introspect().get_source_info_by_name(name, move |result| {
            let (Some(this), ListResult::Item(info)) = (weak.upgrade(), result) else { return };
            // A monitor of an output isn't a microphone.
            if info.monitor_of_sink.is_some() {
                this.mic.set(None);
                return;
            }
            this.source.replace(Some(Sink {
                index: info.index,
                name: info.name.as_deref().unwrap_or_default().to_string(),
                volume: info.volume,
            }));
            this.mic.set(Some(Mic { percent: percent_of(&info.volume), muted: info.mute }));
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
                let percent = percent_of(&info.volume);
                this.sink.replace(Some(Sink {
                    index: info.index,
                    name: name.to_string(),
                    volume: info.volume,
                }));
                let name = name.to_string();
                this.state.update(|state| {
                    let new = AudioState {
                        available: true,
                        percent,
                        muted: info.mute,
                        outputs: std::mem::take(&mut state.outputs),
                        default: name,
                    };
                    let changed = *state != new;
                    *state = new;
                    changed
                });
            });
    }
}

/// 100 = the device's normal level.
fn percent_of(volume: &ChannelVolumes) -> u32 {
    (f64::from(volume.avg().0) * 100.0 / f64::from(Volume::NORMAL.0)).round() as u32
}

fn to_volume(percent: u32) -> Volume {
    Volume((f64::from(Volume::NORMAL.0) * f64::from(percent.min(MAX_PERCENT)) / 100.0) as u32)
}

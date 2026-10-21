//! Connections to the system: each one starts on first use, so a service no module
//! needs costs nothing.

pub mod audio;
pub mod battery;
pub mod bluetooth;
pub mod brightness;
pub mod clock;
pub mod idle;
pub mod dbus;
pub mod media;
pub mod keys;
pub mod network;
pub mod notifications;
pub mod power;
pub mod session;
pub mod tray;
pub mod wifi;

use std::cell::OnceCell;
use std::rc::Rc;

use crate::niri::Niri;

pub struct Services {
    pub niri: Niri,
    battery: OnceCell<battery::Battery>,
    bluetooth: OnceCell<bluetooth::Bluetooth>,
    network: OnceCell<network::Network>,
    tray: OnceCell<Rc<tray::Tray>>,
    power: OnceCell<power::Power>,
    brightness: OnceCell<brightness::Brightness>,
    media: OnceCell<media::Media>,
    session: OnceCell<session::Session>,
    clock: OnceCell<Rc<clock::Clock>>,
    notifications: OnceCell<Rc<notifications::Notifications>>,
    caps_lock: OnceCell<crate::watch::Watch<Option<bool>>>,
    keyboard_light: OnceCell<keys::KeyboardLight>,
    audio: OnceCell<Option<Rc<audio::Audio>>>,
    idle: OnceCell<Rc<idle::Idle>>,
}

impl Services {
    pub fn new(niri: Niri) -> Rc<Services> {
        Rc::new(Services {
            niri,
            battery: OnceCell::new(),
            bluetooth: OnceCell::new(),
            network: OnceCell::new(),
            tray: OnceCell::new(),
            power: OnceCell::new(),
            brightness: OnceCell::new(),
            media: OnceCell::new(),
            session: OnceCell::new(),
            clock: OnceCell::new(),
            notifications: OnceCell::new(),
            caps_lock: OnceCell::new(),
            keyboard_light: OnceCell::new(),
            audio: OnceCell::new(),
            idle: OnceCell::new(),
        })
    }

    pub fn battery(&self) -> &battery::Battery {
        self.battery.get_or_init(battery::Battery::start)
    }

    pub fn bluetooth(&self) -> &bluetooth::Bluetooth {
        self.bluetooth.get_or_init(bluetooth::Bluetooth::start)
    }

    pub fn network(&self) -> &network::Network {
        self.network.get_or_init(network::Network::start)
    }

    pub fn tray(&self) -> &Rc<tray::Tray> {
        self.tray.get_or_init(tray::Tray::start)
    }

    pub fn power(&self) -> &power::Power {
        self.power.get_or_init(power::Power::start)
    }

    pub fn brightness(&self) -> &brightness::Brightness {
        self.brightness.get_or_init(brightness::Brightness::start)
    }

    pub fn media(&self) -> &media::Media {
        self.media.get_or_init(media::Media::start)
    }

    pub fn session(&self) -> &session::Session {
        self.session.get_or_init(session::Session::start)
    }

    pub fn caps_lock(&self) -> &crate::watch::Watch<Option<bool>> {
        self.caps_lock.get_or_init(keys::caps_lock)
    }

    pub fn keyboard_light(&self) -> &keys::KeyboardLight {
        self.keyboard_light.get_or_init(keys::KeyboardLight::start)
    }

    pub fn notifications(&self) -> &Rc<notifications::Notifications> {
        self.notifications.get_or_init(notifications::Notifications::start)
    }

    pub fn clock(&self) -> &Rc<clock::Clock> {
        self.clock.get_or_init(clock::Clock::start)
    }

    pub fn idle(self: &Rc<Self>) -> &Rc<idle::Idle> {
        self.idle.get_or_init(|| idle::Idle::start(self))
    }

    /// None only if libpulse can't run at all.
    pub fn audio(&self) -> Option<&Rc<audio::Audio>> {
        self.audio.get_or_init(audio::Audio::start).as_ref()
    }
}

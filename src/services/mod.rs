//! Connections to the system: each one starts on first use, so a service no module
//! needs costs nothing.

pub mod audio;
pub mod battery;

use std::cell::OnceCell;
use std::rc::Rc;

use crate::niri::Niri;

pub struct Services {
    pub niri: Niri,
    battery: OnceCell<battery::Battery>,
    audio: OnceCell<Option<Rc<audio::Audio>>>,
}

impl Services {
    pub fn new(niri: Niri) -> Rc<Services> {
        Rc::new(Services {
            niri,
            battery: OnceCell::new(),
            audio: OnceCell::new(),
        })
    }

    pub fn battery(&self) -> &battery::Battery {
        self.battery.get_or_init(battery::Battery::start)
    }

    /// None only if libpulse can't run at all.
    pub fn audio(&self) -> Option<&Rc<audio::Audio>> {
        self.audio.get_or_init(audio::Audio::start).as_ref()
    }
}

//! Screen brightness from sysfs, changed through logind (no root needed).

use std::path::PathBuf;

use gtk4::prelude::*;
use gtk4::{gio, glib};

use crate::watch::Watch;

pub struct Brightness {
    /// 0..=100, None without a backlight.
    pub percent: Watch<Option<u32>>,
    device: Option<(String, PathBuf, u32)>,
}

impl Brightness {
    pub fn start() -> Brightness {
        let device = std::fs::read_dir("/sys/class/backlight")
            .into_iter()
            .flatten()
            .flatten()
            .find_map(|entry| {
                let path = entry.path();
                let max = read_number(&path.join("max_brightness"))?;
                (max > 0).then(|| (entry.file_name().to_string_lossy().into_owned(), path, max))
            });
        let brightness = Brightness {
            percent: Watch::new(None),
            device,
        };
        brightness.refresh();
        brightness
    }

    /// Re-reads the level. Kernel changes don't notify sysfs watchers, so this runs when
    /// something shows the value (OSD and live updates come with the OSD step).
    pub fn refresh(&self) {
        let Some((_, path, max)) = &self.device else { return };
        let percent = read_number(&path.join("brightness"))
            .map(|value| (f64::from(value) * 100.0 / f64::from(*max)).round() as u32);
        self.percent.set(percent);
    }

    pub fn set_percent(&self, percent: u32) {
        let Some((name, _, max)) = self.device.clone() else { return };
        // Never fully dark from the slider.
        let percent = percent.clamp(1, 100);
        let value = (f64::from(max) * f64::from(percent) / 100.0).round() as u32;
        self.percent.set(Some(percent));
        glib::spawn_future_local(async move {
            let Ok(bus) = gio::bus_get_future(gio::BusType::System).await else { return };
            let result = bus
                .call_future(
                    Some("org.freedesktop.login1"),
                    "/org/freedesktop/login1/session/auto",
                    "org.freedesktop.login1.Session",
                    "SetBrightness",
                    Some(&("backlight", name.as_str(), value).to_variant()),
                    None,
                    gio::DBusCallFlags::NONE,
                    -1,
                )
                .await;
            if let Err(err) = result {
                tracing::warn!("brightness: {err}");
            }
        });
    }
}

fn read_number(path: &std::path::Path) -> Option<u32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

//! Screen brightness from sysfs, changed through logind (no root needed). Changes from
//! anywhere (keys, other tools) arrive as `POLLPRI` on `actual_brightness`: the kernel
//! notifies sysfs readers, so nothing is polled on a timer.

use std::fs::File;
use std::io::{Read, Seek};
use std::os::fd::AsRawFd;

use gtk4::prelude::*;
use gtk4::{gio, glib};

use crate::watch::Watch;

pub struct Brightness {
    /// 0..=100, None without a backlight.
    pub percent: Watch<Option<u32>>,
    /// Device name and max value.
    device: Option<(String, u32)>,
    /// The connector of the screen it lights ("eDP-1"), when sysfs says.
    pub connector: Option<String>,
}

impl Brightness {
    pub fn start() -> Brightness {
        let found = std::fs::read_dir("/sys/class/backlight")
            .into_iter()
            .flatten()
            .flatten()
            .find_map(|entry| {
                let path = entry.path();
                let max: u32 = std::fs::read_to_string(path.join("max_brightness")).ok()?.trim().parse().ok()?;
                let file = File::open(path.join("actual_brightness")).ok()?;
                (max > 0).then(|| (entry.file_name().to_string_lossy().into_owned(), max, file))
            });
        let Some((name, max, mut file)) = found else {
            return Brightness { percent: Watch::new(None), device: None, connector: None };
        };
        // A GPU's backlight sits under its connector: …/drm/card1/card1-eDP-1/amdgpu_bl1.
        let connector = std::fs::canonicalize(format!("/sys/class/backlight/{name}"))
            .ok()
            .and_then(|path| path.parent()?.file_name()?.to_str().map(String::from))
            .and_then(|dir| dir.split_once('-').map(|(_, connector)| connector.to_string()))
            .filter(|connector| !connector.is_empty());

        let percent = Watch::new(read_percent(&mut file, max));
        let watch = percent.clone();
        let fd = file.as_raw_fd();
        // The file stays open inside the handler for the life of the process.
        let device = name.clone();
        glib_unix::unix_fd_add_local(fd, glib::IOCondition::PRI | glib::IOCondition::ERR, move |_, _| {
            // Only a successful read re-arms the event; a device that went away (an
            // external GPU) would call this again at once, forever.
            let Some(percent) = read_percent(&mut file, max) else {
                tracing::warn!("brightness: {device} can't be read any more");
                return glib::ControlFlow::Break;
            };
            watch.set(Some(percent));
            glib::ControlFlow::Continue
        });
        Brightness { percent, device: Some((name, max)), connector }
    }

    pub fn set_percent(&self, percent: u32) {
        let Some((name, max)) = self.device.clone() else { return };
        // Never fully dark from the slider.
        let percent = percent.clamp(1, 100);
        let value = (f64::from(max) * f64::from(percent) / 100.0).round() as u32;
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

/// Reads the value from the start; reading also re-arms the kernel's notification.
fn read_percent(file: &mut File, max: u32) -> Option<u32> {
    let mut text = String::new();
    file.rewind().ok()?;
    file.read_to_string(&mut text).ok()?;
    let value: u32 = text.trim().parse().ok()?;
    Some((f64::from(value) * 100.0 / f64::from(max)).round() as u32)
}

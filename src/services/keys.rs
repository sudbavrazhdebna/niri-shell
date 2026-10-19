//! Hardware key events the OSD shows: Caps Lock (keyboard LEDs through evdev) and the
//! keyboard backlight (UPower).

use std::fs::File;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;

use gtk4::prelude::*;
use gtk4::{gio, glib};

use crate::watch::Watch;

const EV_KEY: u16 = 0x01;
const EV_MSC: u16 = 0x04;
const EV_LED: u16 = 0x11;
const LED_CAPSL: u16 = 0x01;
const KEY_CNT: usize = 0x300;
const MSC_CNT: usize = 0x08;
/// `struct input_event` on 64-bit: timeval (16) + type (2) + code (2) + value (4).
const EVENT_SIZE: usize = 24;

/// `_IOR('E', 0x19 + 0, char[8])`: LED state bits.
const EVIOCGLED_8: libc::c_ulong = (2 << 30) | (8 << 16) | (0x45 << 8) | 0x19;
/// `_IOW('E', 0x93, struct input_mask)`: which events this reader gets.
const EVIOCSMASK: libc::c_ulong = (1 << 30) | (16 << 16) | (0x45 << 8) | 0x93;

#[repr(C)]
struct InputMask {
    kind: u32,
    codes_size: u32,
    codes_ptr: u64,
}

/// Caps Lock state, None if no keyboard with a Caps LED could be read.
pub fn caps_lock() -> Watch<Option<bool>> {
    let watch = Watch::new(None);
    let Some(mut file) = open_caps_keyboard() else {
        tracing::info!("caps lock: no readable keyboard with a Caps LED");
        return watch;
    };
    watch.set(Some(led_state(&file)));
    let w = watch.clone();
    let fd = file.as_raw_fd();
    // The file stays open inside the handler for the life of the process.
    glib_unix::unix_fd_add_local(fd, glib::IOCondition::IN, move |_, condition| {
        if condition.contains(glib::IOCondition::ERR) || condition.contains(glib::IOCondition::HUP) {
            return glib::ControlFlow::Break; // keyboard unplugged
        }
        let mut buf = [0u8; EVENT_SIZE * 16];
        while let Ok(n) = file.read(&mut buf) {
            if n == 0 {
                break;
            }
            for event in buf[..n].as_chunks::<EVENT_SIZE>().0 {
                let kind = u16::from_ne_bytes([event[16], event[17]]);
                let code = u16::from_ne_bytes([event[18], event[19]]);
                let value = i32::from_ne_bytes([event[20], event[21], event[22], event[23]]);
                if kind == EV_LED && code == LED_CAPSL {
                    w.set(Some(value != 0));
                }
            }
        }
        glib::ControlFlow::Continue
    });
    watch
}

/// The first keyboard that has a Caps LED, opened so that only LED changes wake us.
fn open_caps_keyboard() -> Option<File> {
    let mut entries: Vec<_> = std::fs::read_dir("/sys/class/input").ok()?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    entries.into_iter().find_map(|entry| {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("event") {
            return None;
        }
        let leds = std::fs::read_to_string(entry.path().join("device/capabilities/led")).ok()?;
        // A hex bitmap; the lowest word holds LED_CAPSL (bit 1).
        let low = u64::from_str_radix(leds.split_whitespace().last()?, 16).ok()?;
        if low & (1 << LED_CAPSL) == 0 {
            return None;
        }
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(format!("/dev/input/{name}"))
            .ok()?;
        // Typing must not wake the shell: block key and scan events for this reader.
        // Empty SYN reports are dropped by the kernel, so only LED changes come through.
        for (kind, count) in [(EV_KEY, KEY_CNT), (EV_MSC, MSC_CNT)] {
            // The size must be a whole number of longs.
            let codes = vec![0u64; count.div_ceil(64)];
            let mask = InputMask {
                kind: u32::from(kind),
                codes_size: (codes.len() * 8) as u32,
                codes_ptr: codes.as_ptr() as u64,
            };
            // SAFETY: `mask` and `codes` outlive the call; the kernel only reads them.
            if unsafe { libc::ioctl(file.as_raw_fd(), EVIOCSMASK, &mask) } < 0 {
                tracing::warn!("caps lock: can't filter {name}, typing would wake the shell");
                return None;
            }
        }
        tracing::info!("caps lock: following {name}");
        Some(file)
    })
}

fn led_state(file: &File) -> bool {
    let mut bits = [0u8; 8];
    // SAFETY: the buffer is the 8 bytes the request asks for.
    let ok = unsafe { libc::ioctl(file.as_raw_fd(), EVIOCGLED_8, bits.as_mut_ptr()) } >= 0;
    ok && bits[0] & (1 << LED_CAPSL) != 0
}

/// Keyboard backlight level in percent, changed with the Fn key. Each change bumps
/// the counter, so the same level twice still counts as an event.
pub fn keyboard_backlight() -> Watch<Option<(u32, u64)>> {
    let watch = Watch::new(None);
    let w = watch.clone();
    glib::spawn_future_local(async move {
        let proxy = gio::DBusProxy::for_bus_future(
            gio::BusType::System,
            gio::DBusProxyFlags::DO_NOT_LOAD_PROPERTIES,
            None,
            "org.freedesktop.UPower",
            "/org/freedesktop/UPower/KbdBacklight",
            "org.freedesktop.UPower.KbdBacklight",
        )
        .await;
        let Ok(proxy) = proxy else { return };
        let max = proxy
            .call_future("GetMaxBrightness", None, gio::DBusCallFlags::NONE, -1)
            .await
            .ok()
            .and_then(|v| v.get::<(i32,)>())
            .map(|(m,)| m)
            .filter(|m| *m > 0);
        let Some(max) = max else { return };
        // The handler keeps the proxy alive for the life of the process.
        proxy.connect_local("g-signal", false, move |args| {
            let name = args.get(2).and_then(|v| v.get::<String>().ok()).unwrap_or_default();
            let params = args.get(3).and_then(|v| v.get::<glib::Variant>().ok());
            // "internal" is the Fn key; "external" are changes made by software.
            if name == "BrightnessChangedWithSource"
                && let Some((value, source)) = params.and_then(|p| p.get::<(i32, String)>())
                && source == "internal"
            {
                let percent = (f64::from(value) * 100.0 / f64::from(max)).round() as u32;
                w.update(|state| {
                    let serial = state.map_or(0, |(_, s)| s + 1);
                    *state = Some((percent, serial));
                    true
                });
            }
            None
        });
    });
    watch
}

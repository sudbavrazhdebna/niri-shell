# Decisions

Choices that weren't obvious, why they were made, and what was tried. Newest at the bottom of
each section. Read before changing any of these.

## Platform and stack

- **GLib's D-Bus (gio), not zbus.** Already inside GTK, runs on the main loop: no threads, no
  tokio. Remote data is read with `services::dbus::{child, lookup}` — `child_value` and
  `VariantDict::new` panic on unexpected types, and apps do send unexpected types.
- **libpulse on the GLib main loop** (`libpulse-glib-binding`) for sound: no threads, and it
  unifies with gtk-rs' glib 0.22.
- **GL renderer, not Vulkan.** The fork measured GL as cheaper with changing text, and GTK's
  Vulkan renderer was seen stalling for minutes in `wl_display_roundtrip` while opening a bar
  window. Set in `main` unless `GSK_RENDERER` is given.
- **Mesa glthread off** (`mesa_glthread=false` in `main`): two threads less, same CPU.
- **Link gtk4-layer-shell before libwayland-client** (`build.rs`): layer-shell intercepts
  libwayland calls; with the wrong order bars become plain windows.
- **wayland-client on GDK's connection** (`wayland.rs`) for protocols GTK lacks (blur, idle
  inhibit), instead of the fork's raw FFI.

## Menus

- **Menus are their own layer-shell windows, not GTK popovers.** niri closes a popover when its
  size changes after it's shown (confirmed: a calendar that grew 500 ms after opening closed).
  Our menus grow (scan results, devices). A transparent "catcher" window below the bar closes
  the menu on an outside click; it has exclusive zone 0 so the bar stays hoverable.
- Tray menus are drawn with our own widgets from the dbusmenu tree, not `PopoverMenu`, so they
  look like the rest and live in the same windows. Submenus are stack pages.
- Hover switching waits 250 ms when the pointer heads diagonally into the open menu (the fork's
  "safe triangle"), switches at once along the bar.

## Look

- All UI text is English. macOS is a loose reference only.
- The shell forces built-in Adwaita dark for its own widgets, so it looks the same whatever GTK
  theme apps use; font family is left to the system.
- CSS for buttons uses `background`, not `background-color`: the theme's background image wins
  otherwise (tiles stayed grey).
- Bar icons are the user's waybar SVGs, kept in their colours (not recoloured as symbolic, the
  volume icon is two-tone); opacity and shadow come from CSS.
- Blur needs `xray false` in a niri layer-rule for `^nshell-`, or it only blurs the wallpaper.

## Events without polling

- Window title: title-only changes shown at most 4×/s (terminal spinners redraw the bar).
- Brightness: `poll` for `POLLPRI` on `actual_brightness` (the kernel calls `sysfs_notify` on
  every change, keys or logind). Tested: nothing while idle, an event per change.
- Caps Lock: evdev of keyboards with a Caps LED, with `EVIOCSMASK` blocking `EV_KEY`/`EV_MSC`,
  so only LED changes wake us, not typing. Wayland would only tell the focused window, and the
  bar never has focus. `codes_size` must be a multiple of 8; `EV_REP` can't be masked.
- Keyboard backlight: UPower `BrightnessChangedWithSource`, only source "internal" (the Fn key).

## Code rules learned the hard way

- Never hold a `RefCell` borrow across a GTK call that can emit signals: removing or hiding a
  widget under the pointer runs its leave handler synchronously (a popup crashed this way).
  Take what you need out of the cell, drop the borrow, then touch widgets.

## Sandbox lessons

- One nested niri at a time: GNOME stops drawing a covered nested window, and that niri then
  hangs waiting for a frame (and ignores SIGTERM). `dev-nested.sh` stops the previous one.
- The sandbox replaces `XDG_DATA_HOME`; user icon themes are linked in, or icons go missing.
- Never `pkill -f` with a pattern that can match your own shell or the real session; use
  `scripts/dev-stop.sh`.

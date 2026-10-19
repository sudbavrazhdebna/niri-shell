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
- **`GSK_DEBUG=full-redraw`** (set in `main`): GTK 4.22's partial redraw left streaks of the
  old frame on the OSD bar (the fork saw the same on numbers). Whole-surface redraws fix it.
- **Mesa glthread off** (`mesa_glthread=false` in `main`): two threads less, same CPU.
- **Link gtk4-layer-shell before libwayland-client** (`build.rs`): layer-shell intercepts
  libwayland calls; with the wrong order bars become plain windows.
- **wayland-client on GDK's connection** (`wayland.rs`) for protocols GTK lacks (blur, idle
  inhibit), instead of the fork's raw FFI.

- **Runs as systemd user services** (`systemd/`, linked with `systemctl --user enable
  <path>`): `nshell-tray-watcher` first, then `nshell`, both part of `niri.service` and
  restarted a second after a crash. Run by hand from a terminal, the shell died with the
  terminal and stayed down after a crash. Apps it started live on (own scopes).

## Dock

- **A fully transparent GTK window never reaches the compositor**: with nothing to draw, GTK
  attaches no buffer, the layer surface stays unmapped and gets no pointer (seen with
  WAYLAND_DEBUG: no `wl_surface.attach`). The dock's hot edge and the menu catcher therefore
  have a nearly transparent background (alpha 0.004). This is why hovering the bottom edge did
  nothing and why a click beside a menu didn't close it.
- Size such windows explicitly: `set_default_size(-1, h)` made GTK ask for its default 200 px.
- The dock slides by its bottom margin on a 16 ms timer, not the frame clock, and never goes
  wholly off the screen while mapped (one pixel stays): niri sends no frames to a surface
  outside every output, GTK then never commits the next position, and the dock stayed blank.
  Its height is measured before it maps (a guessed height put it wholly off screen).
- Autohide: a hidden dock has no window at all (unmapped once it slid out); a separate 2 px
  transparent layer window along the whole bottom edge brings it back. The first version kept
  a strip inside the dock window, which had zero width until the dock had been shown once (a
  never-revealed revealer doesn't know its child's width), so hovering did nothing after a
  restart.

## Tray

- The StatusNotifierWatcher runs as its own small process (`nshell-tray-watcher`, no GTK,
  ~2 MB of its own memory, no CPU while idle), started before the shell; the shell reads
  the items from it. Qt apps register their icon once: when the watcher went away with a
  restarting shell, AmneziaVPN's icon never came back. Without that process the shell is
  the watcher itself, as before.
- Whoever becomes the watcher adopts the `org.kde.StatusNotifierItem-…` names already on
  the bus (Qt and KDE apps own one per icon), so their icons survive a handover.
- The watcher always answers "a host is registered", so apps put up their icon even while
  the shell restarts.

## Config

- The dock's pin/unpin writes the `pinned` line back into config.kdl through the `kdl` crate,
  which keeps comments and formatting; the file watcher then reloads the shell.

## Menus

- **Menus are their own layer-shell windows, not GTK popovers.** niri closes a popover when its
  size changes after it's shown (confirmed: a calendar that grew 500 ms after opening closed).
  Our menus grow (scan results, devices). A transparent "catcher" window below the bar closes
  the menu on an outside click; it has exclusive zone 0 so the bar stays hoverable.
- Tray menus are drawn with our own widgets from the dbusmenu tree, not `PopoverMenu`, so they
  look like the rest and live in the same windows. Submenus are stack pages.
- Menu windows use the *on-demand* keyboard mode. niri gives a mapped on-demand layer surface
  the keyboard (Escape works) and the app shows unfocused; a click on any app window gives it
  the focus back, which the shell sees as a niri focus change and closes the menu
  (`menu::close_on_focus_change`). "No window focused" (focus went to the menu) must not close
  it — the first version did and closed every menu right after opening.
- A second click on a module closes its menu: the press already closes it (focus goes back to
  the app), so `toggle` doesn't reopen a menu of the same anchor that closed under 300 ms ago.
- Switching menus (same monitor, same direction) keeps the menu window and swaps the card's
  content. Recreating the window handed the keyboard to the app for a moment, which closed the
  new menu (focus-change rule) and made the app's focus blink. A menu is placed at its measured
  width before it shows, so it doesn't jump sideways. The window is sized right when the
  content is swapped, not in an idle callback: that left one frame of the new menu in the
  old one's window (cut off, or with empty glass below), seen as a flash when moving along
  the bar. Measuring at once gives the same size as later (checked).
- The OSD stays quiet only for changes made with a slider in a menu (the slider shows them);
  keys always get it, menu open or not.
- Menus scroll as a whole once taller than the screen. The one inner scrolled list is the
  notification center's (asked for 2026-10-19): it shows the first 4 notifications and
  scrolls the rest. Its cap is the measured height of those 4 (cards differ), and no cap
  at all when everything fits. A first attempt with a fixed cap made the menu shrink to
  half a notification when a stack was expanded; the window refit below fixed that.
  The scrolled window's minimum doesn't follow its content, so the menu window is refit
  whenever the content's height changes (`notify::upper`); without it a player or device
  appearing in an open menu was cut off.
- Hover switching waits 250 ms when the pointer heads diagonally into the open menu (the fork's
  "safe triangle"), switches at once along the bar.

## Notifications

- The center keeps the newest 100 notifications; older ones close (reason "expired").
  Without a limit an app sending one a second, each with an image, grew memory forever.
- Bodies keep only the spec's markup (b, i, u; links as text). Pango's full markup let
  any app make a card cover the screen (`<span font="300">`).

## Apps the shell starts

- Every app or command the shell starts is moved into its own transient systemd scope under
  `app.slice` (`launch.rs`, StartTransientUnit with the pid, like GNOME). Otherwise it stays in
  the shell service's cgroup and dies when the service stops or restarts — ironbar did that.
  Verified: a test process moved from the terminal's scope into `app-nshell-test-*.scope`.
- Desktop ids may end in ".desktop" themselves (`org.telegram.desktop`): never strip it, try
  the id as is and with ".desktop" added.

## Look

- All UI text is English. macOS is a loose reference only.
- The shell forces built-in Adwaita dark for its own widgets, so it looks the same whatever GTK
  theme apps use; font family is left to the system.
- CSS for buttons uses `background`, not `background-color`: the theme's background image wins
  otherwise (tiles stayed grey).
- Bar icons are the user's waybar SVGs, kept in their colours (not recoloured as symbolic, the
  volume icon is two-tone); opacity and shadow come from CSS.
- Menus, OSD, popups and dock share one card look through CSS variables (`--glass`,
  `--glass-edge`, `--glass-shadow`, `--accent`, … at the top of style.css), so a user's
  `:root { … }` restyles all of them. The edge is an inset 1 px shadow, never a `border`:
  a border changes the card's size, which the blur shape is computed from. They had four
  different edges before (the OSD a dark border plus a light inset line).
- Shadows are drawn by the shell (CSS), not niri: niri shadows the whole layer surface,
  and the popup window holds several cards. Windows leave room for them (22 px below,
  16 px at the sides); the OSD had 10 px and its shadow was cut off.
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

- GTK windows grow with content but never shrink back. Layer windows (dock, menus, popup
  column) call `ui::shrink_to_fit` when content gets smaller — `ui::clear` does it for every
  rebuilt list — or they keep an empty, click-eating area (the dock showed a gap after an app
  closed).
- Never hold a `RefCell` borrow across a GTK call that can emit signals: removing or hiding a
  widget under the pointer runs its leave handler synchronously (a popup crashed this way).
  Take what you need out of the cell, drop the borrow, then touch widgets.

- Show layer-shell windows with `set_visible(true)`, not `present()`: `present` asks for focus
  through a blocking Wayland roundtrip (`gdk_wayland_toplevel_focus`), so a compositor that is
  slow to answer freezes the whole shell. The menu gets the keyboard from its layer-shell
  keyboard mode instead.

- A `Watch` listener must not change the same watch while it runs (the value is borrowed
  during notification): defer with `glib::idle_add_local_once`, as popups and the menu's
  focus rule do.
- `Watch::bind` drops its listener once the bound widget is gone, so the closure must not
  own that widget (or anything owning it): the sound menu bound its row and captured the
  row, and every opening left a listener for good. Nor may it hold the service that owns
  the watch (an `Rc` cycle: the Wi-Fi menu kept NetworkManager signals after closing).
  Dead bound listeners are cleared at the next subscription, not only the next change.
- Icons that follow a value go through `ui::set_icon`, which skips an unchanged name: GTK
  resolves a name again on every set (a stat of each icon theme folder) and redraws. The
  Wi-Fi icon did that on each signal-strength update from NetworkManager, every ~6 s while
  idle. Measured idle (sandbox, release): 0.00% CPU, 0 wakeups/s after; a frame every 6 s
  before.
- niri's `State::layout_serial` skips title-only changes; anything that doesn't show titles
  (the dock) should check it before recomputing.

## Sandbox lessons

- The sandbox's private bus outlives the shell (it used to end with it, taking test apps
  and the tray watcher along), so `scripts/dev-restart.sh` can restart just the shell.
- One nested niri at a time: GNOME stops drawing a covered nested window, and that niri then
  hangs waiting for a frame (and ignores SIGTERM). `dev-nested.sh` stops the previous one.
- The sandbox replaces `XDG_DATA_HOME`; user icon themes and app entries are linked in, or
  icons and apps go missing.
- `dev-shot.sh` used to return at once when the target file already existed (an old shot):
  several "the menu isn't drawn" scares came from that. It deletes the file first now.
- Never `pkill -f` with a pattern that can match your own shell or the real session; use
  `scripts/dev-stop.sh`.

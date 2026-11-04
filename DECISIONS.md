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

## Bar

- The bar turns to blurred glass (`window.bar.solid`) while the output's active window is
  maximized to the edges: the screen's whole width (no gaps, unlike Mod+F's column) as
  the top tile of its column. No line along its edge (seen as a stray border). Not by
  position: niri 26.04 leaves
  `tile_pos_in_workspace_view` empty even for a plainly visible window. niri's
  `WindowLayoutsChanged` comes on every resize and animation frame; it wakes listeners
  only when a top tile's width (in 16 px steps) changes, and never bumps `layout_serial`.
- The bar's zones are laid out by our own layout manager, not GTK's CenterBox: the
  CenterBox moved the clock aside when a side asked for more than half the bar (a long
  window title). Now the center stays in the middle and a side gets the room beside it;
  the title label shrinks to "…" there. `max-length` stays as a cap.

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

## Displays

- The laptop panel (eDP/LVDS/DSI) is turned off through niri IPC while an external output
  is on, and on again when none is (`displays { laptop-screen "auto" }`, the default).
  Triggered by GDK's monitor list changing; niri's `Outputs` tells which are connected and
  on (GDK only lists enabled ones). External outputs are never touched. It acts only when
  "an external monitor is on" changes, so a screen turned on or off by hand in the
  displays menu stays so; that menu lists niri's outputs (off ones too) and never lets
  the last one that is on be turned off.

## Wallpaper

- A background-layer window per monitor (namespace `nshell-wallpaper`; niri needs
  `place-within-backdrop true` for it to stay in the overview). The image is decoded on a
  `gio::spawn_blocking` thread at the size that covers the monitor: the user's 6000×4000
  JPEG would be ~96 MB as is, and decoding it on the main loop froze the shell. Decoded
  images are shared by monitors of the same size; config reloads keep them unless the
  image changed (style.css saves rebuild the bars, not the wallpaper).

## Launcher

- Replaces vicinae (4 processes, ~195 MB, Node.js, a root helper) for starting apps and
  opening files. A full-screen, nearly transparent overlay window with the card in it: a
  click beside the card closes it. Keyboard mode *exclusive* (typing must reach it), unlike
  the menus' on-demand.
- A running app comes to the front (its most recently focused window) instead of starting
  again; Ctrl+Enter starts another. Apps are matched to windows like the dock does
  (`apps::same_app`).
- Matching: whole name, its start, a word start, word initials ("vsc"), a substring, then
  letters in order only from a word start and with few gaps ("fire" matched LibreOffice
  Impress before). Launch counts (`~/.local/state/nshell/launcher-history`) add at most
  150, less than a better match.
- Files: names under `launcher { search-paths … }`, indexed on a blocking-pool thread when
  the launcher opens and the index is older than 60 s (no watching, no timer); hidden
  folders and build caches skipped, 100 000 entries at most.

## Idle

- Replaces stasis (measured idle: 39 wakeups/s, 0.30% CPU, 21 threads, a process of its
  own). niri counts idle time: one `ext-idle-notify` notification per stage, its time
  counted from the start of idle (not from the previous stage, as in stasis). No timers.
  The notifications live on a Wayland connection of the shell's own: on GDK's, only GDK's
  queue is dispatched and ours would sit unread.
- niri itself counts browsers playing video and other Wayland inhibitors. The shell adds
  its own reasons (local sound that isn't a system sound, speech or a game; windows like
  mpv, `steam_app_.*`) by holding one idle inhibitor while any reason lasts, released
  4 s after the last one ends (between tracks).
- The inhibitor's surface is a 1×1, almost transparent pixel on the **overlay** layer.
  niri counts an inhibitor only if its surface was presented in a frame; niri 26.04 draws
  the background layer through an offscreen, so a background surface never counts (the
  first version sat there: neither Stay awake nor any reason ever worked), and below the
  overlay layer a maximized or fullscreen window would cover it.
- Dim is built in (brightness to 5%, keyboard light off, both back on input, the OSD
  hushed); lock runs the lock command; dpms through niri; suspend through logind.
- Before any sleep (the lid too) the lock command runs and logind waits: a "delay"
  inhibitor is let go when the command exits, so it should wait for the lock
  (`veila lock --wait-ready`). After waking the clock catches up at once.
- In the sandbox (`NSHELL_SANDBOX`) stages are only logged: its niri gets no input, and
  brightness, the session lock and suspend are the real machine's.

## Images

- Wallpaper and cover art are decoded with the `image` crate on a blocking-pool thread,
  never with gdk-pixbuf: Fedora's gdk-pixbuf hands every image to glycin, which starts a
  sandboxed loader process (bwrap) each time — 366 ms just to read the wallpaper's size,
  418 ms to decode it, loader processes left running, and cover art froze the shell for
  that long since it loaded on the main loop. GTK's own texture loading (`set_from_file`,
  `Texture::from_filename`, used for tray and notification images) doesn't go through
  glycin. A DCT-scaled JPEG decoder saved only ~100 ms on the 6000×4000 wallpaper (340 →
  240 ms, off the main thread): not worth a dependency.

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
  notification center's (asked for 2026-10-19): past 340 px (about 4 cards) it scrolls, no
  cap when everything fits. First the cap was the measured height of the first 4 cards,
  but cards, closed stacks and stack headers differ, so opening a stack resized the menu
  (2026-11-04); a fixed height doesn't move. A first attempt with a fixed cap made the menu shrink to
  half a notification when a stack was expanded; the window refit below fixed that.
  The scrolled window's minimum doesn't follow its content, so the menu window is refit
  whenever the content's height changes (`notify::upper`); without it a player or device
  appearing in an open menu was cut off.
- Hover switching waits 250 ms when the pointer heads diagonally into the open menu (the fork's
  "safe triangle"), switches at once along the bar.
- Bluetooth off is an rfkill soft block of all Bluetooth radios (as GNOME does), not the
  adapter's `Powered`: the laptop's USB adapter is reset on every resume, and BlueZ
  (AutoEnable) powered the new one on. A block also covers adapters that appear later and
  systemd-rfkill keeps it over a reboot. On is an unblock (BlueZ powers it) plus `Powered`.

- The Wi-Fi list is rebuilt on every scan (every few seconds). An open password field, with
  what's typed and the focus (cursor at the end, nothing selected), moves to its network's
  new row; before, a scan threw it away mid-typing. One field at a time; it slides open
  and closed (150 ms).
- No hand cursor over buttons and rows: on the desktop (macOS, GNOME, KDE) that's for
  links only; the arrow stays.

## Notifications

- A notification's action buttons ("Mark as read", "Reply") slide out only while the
  pointer is on its card, in popups and in the center alike: most are only read. They
  float over the card's bottom-right corner (an overlay, covering some text), so hovering
  never changes the card's height; a summary-only card with buttons is a little taller.
- While the clock menu is open, a closed notification leaves its room empty (the list
  keeps its height) instead of the menu shrinking: it jumped with every one closed, and the
  next card now moves up under the pointer, as closing tabs in Chrome. The next opening is
  measured afresh. Folding a stack or "Clear all" do shrink it: those are asked for.
- The notification list scrolls without a scrollbar (policy External): GTK's floating one
  covered the cards' close buttons and edge. The fixed list height cuts through a card,
  which says there's more.
- Every stack of two or more has a header (app icon, name, an open/fold arrow), closed or
  open, as in GNOME: opening one doesn't move what's above it.
- Animations are CSS (150 ms, `ANIMATION_MS` in ui.rs): a menu fades in sliding 8 px away
  from the bar; popups slide in from the screen edge and back out (asked for 2026-11-04).
  niri blurs behind a region the shell gives it and can't fade that blur, so popups only
  slide (the blur follows them each frame, `blur::follow`); the menu's blur is there from
  its first frame, under a card still fading in. GTK only animates widgets on screen: the
  menu's animation class goes on after the window shows.
- Up to 5 popups at once (was 3, changed 2026-11-04).
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
- The blur stops 1 px inside a rounded card (its pixel-step corners looked jagged level
  with the smooth edge), but reaches the edge of the square bar: with the inset, the bar's
  outer pixels showed the window behind sharp, like a 1 px outline.

## Events without polling

- Window title: title-only changes shown at most 4×/s (terminal spinners redraw the bar),
  and a leading spinner glyph (braille dots, ◐◓◑◒, ✳✶✻…) is left out of the shown title,
  so a spinning one changes nothing on screen. With a title spinning 4×/s (sandbox,
  release): 0.87% CPU, 21 wakeups/s before; 0.13%, 4/s after. Per redraw the cost is the
  frame and layout, not pixels: without full-redraw it was 0.83%, so that decision stays.
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
- `dev-nested.sh` passes the session as one single-quoted `sh -c` string: an apostrophe
  in a comment there ("machine's") ended the string, and the sandbox niri came up empty
  with no error. That looked like "the window gets no frames" for two days.
- One nested niri at a time: GNOME stops drawing a covered nested window, and that niri then
  hangs waiting for a frame (and ignores SIGTERM). `dev-nested.sh` stops the previous one.
- The sandbox replaces `XDG_DATA_HOME`; user icon themes and app entries are linked in, or
  icons and apps go missing.
- `dev-shot.sh` used to return at once when the target file already existed (an old shot):
  several "the menu isn't drawn" scares came from that. It deletes the file first now.
- Never `pkill -f` with a pattern that can match your own shell or the real session; use
  `scripts/dev-stop.sh`.

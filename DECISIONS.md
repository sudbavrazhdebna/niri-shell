# Decisions

Choices that weren't obvious, why they were made, and what was tried. Newest at the bottom of
each section. Read before changing any of these.

## Platform and stack

- **GLib's D-Bus (gio), not zbus.** Already inside GTK, runs on the main loop: no threads, no
  tokio. Remote data is read with `services::dbus::{child, lookup}` — `child_value` and
  `VariantDict::new` panic on unexpected types, and apps do send unexpected types.
- **libpulse on the GLib main loop** (`libpulse-glib-binding`) for sound: no threads, and it
  unifies with gtk-rs' glib 0.22.
- The sound menu keeps playback controls in the popup. Each stream occupies one
  icon/slider/percentage row; names and mute actions stay in tooltips and accessibility
  labels. App icons survive muting (dimmed instead), using Pulse's icon name or a cached
  desktop-entry lookup by application id/executable/display name, then a generic app fallback.
  Row callbacks hold Audio weakly because the service itself owns their watch listeners.
  The popup's `.sound-menu` scope shares control-center white sliders and small thumbs;
  mute remains grey. Icon buttons explicitly clear GTK backgrounds, borders and shadows
  in favour of neutral hover/press plates and a visible white keyboard focus outline.
  Apps starts collapsed on each menu opening, behind one count/chevron disclosure row.
  Stream updates rebuild only its list and preserve expansion; an empty section hides
  entirely and resets closed. GTK's revealer animates the list and the menu's existing
  sizing follows it; no extra polling is introduced. Microphone controls are delegated to
  pavucontrol through the footer's Sound Settings item, which closes the popup before
  launching it, like Bluetooth Settings. The unused input tracking and source subscription
  are removed from Audio; this does not mute or disable any actual microphone.
- Percentage labels reserve 3 em in `.menu-value`, in addition to tabular digits.
  GtkLabel's `width-chars(4)` uses average character widths and was narrower than
  `100%`: going from 99 to 100 shortened adjacent volume/brightness sliders. The
  font-relative minimum keeps their geometry stable without padding the actual text.
- **GL renderer, not Vulkan.** The fork measured GL as cheaper with changing text, and GTK's
  Vulkan renderer was seen stalling for minutes in `wl_display_roundtrip` while opening a bar
  window. Set in `main` unless `GSK_RENDERER` is given.
- **`GSK_DEBUG=full-redraw`** (set in `main`): GTK 4.22's partial redraw left streaks of the
  old frame on the OSD bar (the fork saw the same on numbers). Whole-surface redraws fix it.
- **Mesa glthread off** (`mesa_glthread=false` in `main`): two threads less, same CPU.
- Root-window CSS (`surfaces.css`) is loaded at GTK's user priority + 1, and nshell's
  personal `style.css` at user + 2. Global `gtk-4.0/gtk.css` can apply a fill and
  `backdrop-filter` to every `window`: after restart that painted the fullscreen menu
  catcher grey and blurred rectangular margins around the dock and bar. Reset only root
  fills, blur and decorations above that global CSS; card styles retain their own blur
  and shadows. Never change the user's global GTK theme to fix shell surfaces. Reproduce
  global window rules in the sandbox too: its private XDG config otherwise hides them.
- **Link gtk4-layer-shell before libwayland-client** (`build.rs`): layer-shell intercepts
  libwayland calls; with the wrong order bars become plain windows.
- **wayland-client on GDK's connection** (`wayland.rs`) for protocols GTK lacks (idle
  inhibit), instead of the fork's raw FFI.
- **Blur is GTK's, asked for in CSS** (since GTK 4.24, Fedora 45, 2026-11-04). GTK now makes
  the surface's `ext-background-effect` object itself, and the protocol allows one per
  surface: the shell's own second one was a protocol error, niri dropped the connection and
  the shell didn't start. GTK hands niri the area of every `backdrop-filter: blur()` of 20 px
  or more (`--blur` in style.css), with rounded corners and following animations, so the
  shell's own region code (rounded strips, per-frame follow while sliding, corner radii in
  the code) went away. How strongly it blurs is niri's setting, not the CSS number. Older
  GTK just doesn't blur.

- **Runs as systemd user services** (`systemd/`, linked with `systemctl --user enable
  <path>`): `nshell-tray-watcher` first, then `nshell`, both part of `niri.service` and
  restarted a second after a crash. Run by hand from a terminal, the shell died with the
  terminal and stayed down after a crash. Apps it started live on (own scopes).

## Bar

- The bar turns to blurred glass (`window.bar.solid`) while the output's active window is
  maximized to the edges: the screen's whole width (no gaps, unlike Mod+F's column) as
  the top tile of its column. Fullscreen is excluded: its tile fills the output's height,
  ignoring the bar's reserved space. This keeps the bar clear before entering overview,
  too, rather than briefly exposing a dark strip during the transition. No line along its edge (seen as a stray border). Not by
  position: niri 26.04 leaves
  `tile_pos_in_workspace_view` empty even for a plainly visible window. In the overview
  the windows shrink away from the edges, so the bar is clear there (niri's
  `OverviewOpenedOrClosed`). niri's
  `WindowLayoutsChanged` comes on every resize and animation frame; it wakes listeners
  only when a tile's width (in 16 px steps), height or row changes, and never bumps
  `layout_serial`. Height changes matter even at the same width when entering fullscreen.
- The bar's zones are laid out by our own layout manager, not GTK's CenterBox: the
  CenterBox moved the clock aside when a side asked for more than half the bar (a long
  window title). Now the center stays in the middle and a side gets the room beside it;
  the title label shrinks to "…" there. `max-length` stays as a cap.
- Every bar zone has a 2 px inset above and below. Modules fill the zone so hover and
  open-menu backgrounds share the same height even when their icons and text differ.

## Dock

- App activation runs on primary press, so holding the button does not delay launching
  or focusing. Secondary press opens the context menu; while held, motion picks adjacent
  icons in dock-window coordinates (the implicit pointer grab still belongs to the first
  icon). Releasing leaves the last menu open. Ordinary hover does not switch dock menus.
- A menu taking keyboard focus must not rebuild dock icons just because niri reports no
  focused window: keep the last app highlight while that menu is open. Replacing the
  widgets destroyed the menu anchor and the pointer grab during a held right-click.
- A dock menu highlights its anchor until it closes. The outside-click catcher and the
  menu's transparent shadow margin leave the dock's input area reachable, so neighbouring
  icons still respond to hover after the right button is released.
- `dock { no-overview; }` hides the overview button together with its preceding
  separator. The button remains enabled by default and can be restored through hot
  reload; niri overview bindings and the pinned/running-app separator are independent.
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
- In the overview a blurred copy of the wallpaper fades in over it (asked for
  2026-11-04; niri's wiki suggests the same: niri can't blur its backdrop). The copy is
  made once with the decoded image, on the same thread, at 1/4 size (it's blurred, GTK
  scales it up smoothly; ~2 MB for 4K), three box blurs with the edge pixels repeated.
  First it was CSS `filter: blur()`: computed again on every opening, and its edges
  darkened (nothing beyond them to blur in); growing it 6% hid that but read as a zoom.

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
  folders and build caches skipped, 100 000 entries at most. Searches share an in-flight
  scan. Changing the search folders invalidates the index; a scan of the previous folders
  cannot publish its results afterward.

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

- Wi-Fi and Bluetooth share `ui::ConnectionCard`. The blue card marks the connection,
  and it exposes `Connected` as an accessible description. Its optional second line
  shows Wi-Fi IP/traffic or Bluetooth battery percentage. Disconnect is a right-aligned,
  vertically centered overlay that appears on hover or keyboard focus at the same
  card height. Nearby text fades through `ui::FadingContent`. Both menus have a 320 px minimum.
- Wi-Fi separates non-active saved profiles (`Known networks`) from unsaved scan results
  (`Other networks`). Saved profiles stay in the list when not detected: dimmed, labelled
  `Not in range`, and not clickable. The scan-result limit applies only to unsaved networks.
  Missing profiles have no AP object path, and the backend refuses to activate them too.
  A rescan restores their normal row when the AP returns, without duplicating the SSID.
- Wi-Fi Disconnect calls NetworkManager's device method, preserving the radio and saved
  profile and preventing immediate auto-reconnection. A manual network activation reconnects.
  The popup follows device state/IP changes while open; closing it drops those subscriptions.
- **Menus are their own layer-shell windows, not GTK popovers.** niri closes a popover when its
  size changes after it's shown (confirmed: a calendar that grew 500 ms after opening closed).
  Our menus grow (scan results, devices). A transparent "catcher" window below the bar closes
  the menu on an outside click; it has exclusive zone 0 so the bar stays hoverable.
- Tray menus are drawn with our own widgets from the dbusmenu tree, not `PopoverMenu`, so they
  look like the rest and live in the same windows. Submenus are stack pages.
- Omit the check column only from menus/pages with no selectable items. A running dock
  app's window choices reserve the column for its commands too; apps without windows
  use `ui::action_item`. Each tray page reserves it for all rows when any checkbox/radio
  item is present, including unchecked ones. Plain pages have no column. Removing it
  from individual command rows in mixed menus broke their shared text alignment.
- Menu windows use the *on-demand* keyboard mode. niri gives a mapped on-demand layer surface
  the keyboard (Escape works) and the app shows unfocused; a click on any app window gives it
  the focus back, which the shell sees as a niri focus change and closes the menu
  (`menu::close_on_focus_change`). "No window focused" (focus went to the menu) must not close
  it — the first version did and closed every menu right after opening.
- A second click on a module closes its menu: the press already closes it (focus goes back to
  the app), so `toggle` doesn't reopen a menu of the same anchor that closed under 300 ms ago.
- Switching menus on the same bar or dock keeps the window and swaps the card's
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
- The outer menu scroller uses `External` vertical policy: wheel/touch scrolling remains,
  but no overlay scrollbar appears. `Automatic` briefly showed one while the sound Apps
  revealer outgrew the previous allocation during its animation.
- Hover switching waits 250 ms when the pointer heads diagonally into the open menu (the fork's
  "safe triangle"), switches at once along the bar.
- Bluetooth off is an rfkill soft block of all Bluetooth radios (as GNOME does), not the
  adapter's `Powered`: the laptop's USB adapter is reset on every resume, and BlueZ
  (AutoEnable) powered the new one on. A block also covers adapters that appear later and
  systemd-rfkill keeps it over a reboot. On is an unblock (BlueZ powers it) plus `Powered`.

- The Wi-Fi list is rebuilt on every scan (every few seconds). An open password field, with
  what's typed and the focus (cursor at the end, nothing selected), moves to its network's
  new row; before, a scan threw it away mid-typing. One field at a time; it slides open
  (150 ms) and closed (75 ms). Moving from one field to another keeps the menu's height
  (measured flat per frame): the rows have no spacing (a closed field's spacing stayed),
  the closing field is twice as fast (each revealer rounds up, two together were 1 px
  taller), and the list holds its height while they slide.
- No hand cursor over buttons and rows: on the desktop (macOS, GNOME, KDE) that's for
  links only; the arrow stays.

- The bar can sit at the bottom (`bar { position "bottom" }`, asked for 2026-11-04 for a
  Windows-like single bar). Menus then open upward right above it (it reserves its room,
  so no extra margin, unlike the dock), notification popups show bottom-right above it
  with the newest lowest, and its glass turns solid when the shown window is tiled full
  width (niri doesn't say which tile of a column is lowest, so any tile counts).
- The single bar is put together from config (bar at the bottom, `taskbar` module, dock
  `off`), not a separate mode: each part is useful alone, and one switch would hide what
  it changes. The `taskbar` is the dock's own apps row (`dock::apps`), so pins, dots,
  clicks and menus stay one code; only its CSS is smaller to fit the bar.

- Selection rows keep the check mark's column on the left even when unchecked, so
  choices stay aligned. Settings commands in Sound, Bluetooth and Wi-Fi use the shared
  `ui::action_item` instead: it omits the check widget and keeps the usual row padding,
  aligning text with section headings. Both row types use one builder; no negative
  margins or module-specific position adjustments. Other menu rows retain their layout.
- The current-window title menu also uses action rows throughout. Floating invokes
  niri's toggle for the selected window without a state check mark; app actions,
  Maximize, Fullscreen and Close keep the same text alignment.

- No arrow on menus pointing at their module or dock icon (asked about 2026-11-04, the
  user declined): GTK's Popover would bring its own surface without our blur and menu
  switching, and a drawn arrow needs its own blur shape.

## Media

- The bar's play/pause button handles primary clicks itself. A primary click on the
  title or a secondary click anywhere on the module opens its menu. The play/pause
  icon updates in place in both the bar and the menu.
- Keep the last known MPRIS duration when an update omits it for the same player and
  track. A different track or D-Bus owner starts with its own duration.
- With `media { title "off"; }`, both horizontal paddings match so the play/pause
  icon sits in the center of the bar's hover plate.

## Notifications

- A notification's action buttons ("Mark as read", "Reply") slide out only while the
  pointer is on its card, in popups and in the center alike: most are only read. They
  float over the card's bottom-right corner (an overlay, covering some text), so hovering
  never changes the card's height. A summary-only card with buttons is a little taller.
  The buttons keep a thin inner outline and use no outer shadow.
- While actions show, only the body text fades approaching their actual allocated
  bounds (24 px horizontally, 10 px vertically). A shared GSK alpha mask removes text rather than
  painting a gradient matching a guessed card colour, so translucent/hover/stacked
  backgrounds keep their appearance. Header, icon, layout, markup and action clicks
  stay intact; hiding the actions restores the text. Weak references and revealer
  notifications invalidate the mask without an idle timer or continuous redraw loop.
- While the clock menu is open, a closed notification leaves its room empty (the list
  keeps its height) instead of the menu shrinking: it jumped with every one closed, and the
  next card now moves up under the pointer, as closing tabs in Chrome. The next opening is
  measured afresh. Folding a stack or "Clear all" do shrink it: those are asked for.
- The notification list scrolls without a scrollbar (policy External): GTK's floating one
  covered the cards' close buttons and edge. The fixed list height cuts through a card,
  which says there's more.
- Every stack of two or more has a header (app icon, name, an open/fold arrow), closed or
  open. Toggling replaces only that stack's cards and changes the arrow in place, so the
  same button keeps its hover and focus. Closed stacks use translucent fills over the
  menu glass. The sheets draw only their exposed edges so they stay out of the front card.
- Animations are CSS (150 ms, `ANIMATION_MS` in ui.rs): a menu fades in sliding 8 px away
  from the bar; popups slide in from the screen edge and back out (asked for 2026-11-04).
  niri can't fade a blur, so popups only slide (GTK moves the blur with them); the menu's
  blur is there from its first frame, under a card still fading in. GTK only animates
  widgets on screen: the menu's animation class goes on after the window shows.
- Up to 5 popups at once (was 3, changed 2026-11-04).
- Transient notifications appear only as popups, never in the center. If DND suppresses
  them, is enabled while they are visible, or a newer popup displaces them, they close
  with reason "expired" instead of waiting for a timer on a card that no longer exists.
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
- One rule takes the theme's frame (border, shadow) off every button, instead of one per
  button: the per-button way kept missing some (Mark as read, Clear all, the control
  center's tiles and power profiles, 2026-11-04). The power profiles are a segmented
  control of our own, not the theme's linked buttons.
  Battery and control center share the same component and surface styling. It shows only
  daemon-provided profiles, follows cached state, and hides when unavailable. Re-clicking
  the selected mode keeps it selected without writing the same profile again. Service
  references in widget callbacks stay weak so closed popups can release their listeners.
- Bar icons keep their colours (not recoloured as symbolic, the volume icon is two-tone);
  opacity and shadow come from CSS. Battery icons use the chosen Classic design: a rounded
  macOS-inspired outline (fully white, including the terminal), proportional fill in
  10% steps, and a white lightning bolt with
  a transparent gap on AC. Warning colours stay red through 20% and orange through 30%;
  above that, the fill is white on battery and green on AC. The 20 SVGs are generated by
  `scripts/generate-battery-icons.py`; their names and the existing level selection stay
  the same. Other bar icons remain the user's waybar SVGs, with intrinsic dimensions
  matched to their viewBoxes and unused editor attributes removed; their rendering is
  unchanged. The Wi-Fi cross means disconnected only: even a connected network below
  10% signal shows the weakest signal icon. The control-center button uses the chosen
  Soft downward chevron: a rounded white stroke, replacing the two horizontal switches.
  Its left padding is 5 px instead of 9: the narrow glyph has more internal whitespace
  than its neighbours, so the gap needs an optical correction. The menu and the button's
  behaviour stay the same. The chevron rotates upward in 150 ms while its menu is open,
  and returns downward when it closes or another menu takes over. Only the inner image
  rotates, keeping the asymmetric padding, hover plate and hit area stationary.
- Control center is being refined from the original layout. Wi-Fi, Bluetooth,
  keyboard light and Stay awake form one homogeneous row; the keyboard button is
  hidden without an empty slot on unsupported hardware. Each tile has its icon above
  the label/state; only the icon circle turns blue. Keyboard light shows percentages
  (Off / 50% / 100% on the two-level keyboard), and tooltips retain truncated names.
  Tile icons follow their service state: Wi-Fi disabled / disconnected / connected
  signal, Bluetooth disabled / on, and keyboard light off / low / high.
  Keyboard light off keeps the ordinary backlight glyph in `--text-faint`, without
  a slash; the active state restores the normal foreground on its blue circle.
  Wi-Fi's button only changes NetworkManager's WirelessEnabled, so it never claims
  airplane mode. Even the weakest connected Wi-Fi signal keeps a connected glyph.
  Sound uses muted / low / medium / high icons; brightness uses outline / half-filled /
  filled suns. The added brightness and keyboard SVGs extend the existing symbolic
  menu icons with filled geometry: GTK's symbolic recolouring can suppress SVG
  strokes. Bar assets stay independent. These updates use existing subscriptions.
  Sound and brightness each have their own surface: a white icon, title and numeric
  percentage above a white slider with a small thumb. Percentages follow the displayed
  thumb while dragging; existing system subscriptions supply external updates. Slider
  listeners are bound to the card, not the slider they capture, so closed menus can
  release them. Their external updates use the OSD's frame-clock easing: 30% of the
  remaining distance per frame, settling within 0.4% of the range. A new target
  retargets the active animation from its current position; animation frames only
  update the display and never send service writes. Initial/unmapped values and
  reduced-motion settings are immediate. Direct pointer/touch/wheel/key input
  cancels the animation and retains the slider's existing feedback grace period.
  The tick is removed at rest or on unmap. Smoothing is opt-in for these two sliders;
  other menus and media seeking keep their existing update behaviour.
  The original stats/player/power layout remains. Four
  round session buttons share one `--inner` surface with the stats/player padding
  and radius. All inner surfaces have a small soft shadow without a border or inset
  edge. The full grouped-popup study remains a reference only. The bar chevron's
  open/close animation stays enabled independently.
  The last control-center child has no bottom margin: the row's old 8 px margin
  stacked with the popup padding, making its bottom field wider than its other edges.
  Typography inherits the system family but uses only two sizes inside this panel:
  12 px regular for names, values and power profiles; 11 px regular for
  device/state lines, artists and session captions. CPU/Memory no longer inherit the
  larger system size or a bold weight. Secondary text is distinguished by colour.
  The playing track's title stays 12 px but uses semibold (600) to stand out above
  the artist without increasing the player card's text size.
  Player transport buttons explicitly style their normal, hover, pressed and disabled
  states: a transparent base, `--inner-hover` plate, stronger neutral pressed plate,
  and dimmed disabled glyphs. Pressed glyphs keep `--text` instead of a GTK theme colour.
  Play/pause updates a persistent child image instead of resetting the button's icon
  property, keeping its interactive child stable while player events arrive.
- Bar tooltips reuse the Waybar information: battery capacity/time/power, Wi-Fi
  SSID/exact signal/frequency, volume/default device, connected Bluetooth devices and
  their battery levels. Text is built on GTK's tooltip request from existing caches:
  exact Wi-Fi values do not replace the stepped bar state, and no polling is added.
  Missing readings show a dash. Remote names are plain text. The clock renders its
  calendar as one white text label: the full current date and weekday above the grid,
  today bold and underlined, and the configured week start.
  A nested GTK calendar lets global `tooltip label` rules draw a card around every day;
  one label keeps a single background even with a custom GTK stylesheet.
  Tooltips stay hidden while the module's menu is open.
- Menus, OSD, popups and dock share one card look through CSS variables (`--glass`,
  `--glass-edge`, `--glass-shadow`, `--accent`, … at the top of style.css), so a user's
  `:root { … }` restyles all of them. Outer card edges use a 1 px outline with offset
  -1 px, following the existing corner radius without changing layout. This replaces
  the inset shadow so the edge's offset can be adjusted independently of the outer
  shadow. Critical popups keep their danger-coloured edge; notification-center cards
  still have no own outline. Inner controls keep their separate focus/pressed styling.
- Shadows are drawn by the shell (CSS), not niri: niri shadows the whole layer surface,
  and the popup window holds several cards. Windows leave room for them (22 px below,
  16 px at the sides); the OSD had 10 px and its shadow was cut off.
- Blur needs `xray false` in a niri layer-rule for `^nshell-`, or it only blurs the wallpaper.
- Blur can be turned off per part (`blur { off "menus" … }`, asked for 2026-11-04), not
  only all at once: the user wanted plain menus with blurred notifications. Off, the part's
  window gets `.no-blur`, which sets `--blur: none` and makes the glass nearly opaque.
  Open windows follow a config change at once. The class goes on at creation and on a
  config change, never while GTK lays a window out: a style change there left the menu
  undrawn (invisible).

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
- The sandbox's config has short idle times (dim after 5 s, screen off soon after): shots
  taken a while after a restart come out dark or without the shell. Restart with
  `NSHELL_DEV_AWAKE=1`. Do-not-disturb (`.dev/state/nshell/dnd`) hides all but critical
  popups. A sandbox left running for hours (its window covered on the host) stopped
  finishing GTK animations: popups stayed at their slide-in start, invisible. A fresh
  `dev-nested.sh` fixed it.
- `NSHELL_DEV_MENU=<module>@secs`: the seconds are the delay before it opens, and the
  debug build needs several seconds to start; wait for the `nshell-menu` layer in
  `niri msg layers` rather than a fixed time.

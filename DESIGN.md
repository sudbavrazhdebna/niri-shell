# nshell — design document

Draft, 2026-10-19. Edit it right here: strike things out, add notes, put `?` where you disagree.
Items marked **[decide]** need your answer before the step they belong to.

The living spec for behaviour is the fork in `~/Projects/wayle` (plus its `NEW-SHELL.md`,
`ROADMAP.md`, `TODO.md`). We don't copy code from it; it's a reference for the tricky parts.

## 1. What it is

A shell for niri: one process instead of waybar + ironbar + swayosd + swaync + stasis + swaybg.
For one person and one machine: no "settings for every case", only what is actually used.

In: bar, module menus, control center, OSD, notifications, idle manager, wallpaper, dock.
Out: lock screen (stays `veila`; our own maybe later, as a separate binary), settings GUI, themes,
translations, other compositors. All text in the UI is English.

Name: **nshell** (decided 2026-10-19).

## 2. Look (overall)

- A top bar with clickable modules; each icon on the right is a module and a click opens its menu.
  While one menu is open, hovering a neighbour switches to its menu. macOS is a loose reference
  for the general feel, not something to copy exactly.
- Icons: white SVGs with some transparency, a soft shadow under icons and text. Our own icon set
  comes from `~/.config/waybar/icons` (battery 10..100 + charging, Wi-Fi, volume, Bluetooth).
- The bar is transparent; menus and popups are "glass": a translucent dark card + blur from niri,
  rounded corners, a thin border, a soft shadow.
- Hovering a module shows a plate behind it, like GNOME; a module with an open menu gets a denser one.
- Exact colours and sizes start from the fork and get tuned by eye.

## 3. Bar

On every monitor. Height is set in the config (default 34 px); icons and text stay vertically
centered at any height. The font is the system one (whatever GNOME/GTK is set to).

| Where | Module | Shows |
|---|---|---|
| left | window-title | app colour icon + focused window title (rewrite rules as in waybar); empty without a window |
| center | clock | time |
| right | tray | app icons (Telegram, clash-verge, …) |
| right | keyboard | EN / RU |
| right | network | Wi-Fi by signal / off |
| right | bluetooth | icon, only while the adapter is on |
| right | volume | volume level / mute |
| right | battery | level, charging |
| right | control | control center icon |

Docker comes at the very end, as its own module.

### Clicks and menus

| Module | Left | Right | Middle | Wheel | Menu |
|---|---|---|---|---|---|
| window-title | — | window menu? | — | — | **[decide]** needed at all: close, fullscreen, floating |
| clock | menu | — | — | — | calendar (+ notification center, step 8) |
| tray | app action, or its menu | app menu | app's secondary action | passed to app | the app's own menu |
| keyboard | menu | next layout | — | switch layout | layout list |
| network | menu | — | — | — | Wi-Fi on/off, networks, connect (password in the menu) |
| bluetooth | menu | — | — | — | on/off, paired devices, connect/disconnect, device battery |
| volume | menu | mute | mute | volume | slider, output device |
| battery | menu | — | — | — | percent, time to empty/full, power mode |
| control | control center | — | — | — | see below |

## 4. Control center

One menu with the everyday things:

1. Toggle tiles: Wi-Fi, Bluetooth, Stay awake (blocks idle), Keyboard light (Off → Low → High).
2. Sliders: volume, screen brightness.
3. Player (what's playing, previous / play-pause / next), only while something plays.
4. Power mode: saver / balanced / performance.
5. Buttons: lock, sleep, restart, shut down (the last two need a second click).

CPU and memory use, read every 2 s only while the menu is open (decided 2026-10-19).

## 5. OSD

Appears bottom-center (~84% of the height) on the focused monitor, fades after a second or two.
Glass, a white bar, no percent.

- volume (+ mute), screen brightness, keyboard backlight;
- Caps Lock;
- layout switch — big EN/RU (at the very end).

## 6. Notifications

Decided 2026-10-19: the look of the current swaync setup (close to GNOME), without the light
highlight along the top that the fork's cards have.

- **Card:** dark, nearly opaque, 400 px wide, ~10 px corners, a soft shadow. App icon (40 px) or
  the notification's image on the left; bold summary, body, relative time ("now", "5m"); a close
  button; action buttons under the text. Clicking the card runs its default action, if any.
- **Popups:** top-right under the bar on the focused monitor, 5 s (or the app's own timeout),
  paused while hovered. **Stacked:** newest on top, at most 5 at once; more go straight to the
  center.
- **Center:** in the clock menu, under the calendar: "Clear all", a Do not disturb switch.
  Notifications of one app form a **stack** (like iOS): the newest shows with up to two
  narrower, darker cards behind it, peeking out below; a click anywhere on it expands it,
  above every stack a header (app icon, name, a round arrow, as in GNOME) opens and folds it.
- **Bar:** a dot by the clock while there are unseen notifications; a bell-off icon in DND.
- **Do not disturb** survives a restart; critical notifications still show (like GNOME).
- History lives in memory (gone after a restart), the newest 100; older ones close.

## 7. Idle manager

Done here, replacing stasis. Behaviour as in your `stasis.rune`, logic from the analysis in
`wayle/ROADMAP.md`, phase 3. The main difference from stasis: no polling, niri tells us.

Stages: **dim** (screen to 5%, keyboard backlight off) → **lock** (`veila lock`) → **dpms**
(monitors off) → **suspend**. Any mouse move or key undoes the stages in reverse order
(brightness comes back, monitors turn on).

Profiles by power source: on AC and on battery.

Don't go idle while:
- a browser / mpv / Firefox plays video (niri handles this itself);
- local sound plays (not system sounds, not remote playback);
- a `vlc`, `mpv`, `steam_app_*` window is open;
- the Stay awake tile is on.

The screen is always locked before sleep (lid close included).

**Timings [decide].** In stasis each timeout counts from the *previous* stage, plus 4 s. What you
actually get today:

| | dim | lock | dpms | suspend |
|---|---|---|---|---|
| ac | 2:04 | 5:08 | 15:12 | 30:16 |
| battery | 1:04 | 6:12 (after dpms) | 3:08 | 9:36 |

Proposal: in the config, times count **from the start of idle** — clearer what happens when.
Round the numbers from the table (2m, 5m, 15m, 30m)? On battery, lock after dpms — intended?

## 8. Wallpaper

One image on all monitors (`~/.config/wallpapers/m.jpg`), "fill" mode. Reloaded on config change.

## 9. Dock

At the bottom, over windows, no reserved space. Pinned: ghostty, zed, firefox, telegram,
sublime_text, nautilus, clash-verge, AmneziaVPN + running apps. Dots under icons: open — grey,
focused — blue, needs attention — red. Clicks: left — focus/launch, middle — new window,
right — menu (its windows by title, new window, pin/unpin, close), wheel — through the app's
windows. An overview button. No previews. Pin/unpin rewrites the `pinned` line of config.kdl,
keeping the rest of the file as it is. A line between pinned and running apps.
**Autohide** (decided 2026-10-19): slides out of sight, back when the pointer touches the bottom
edge; `nshell msg show-dock` shows it for 3 s on the focused monitor (bind it, e.g. Mod+A). On an
empty workspace it stays up (nothing to cover) and goes once a window opens there.
Later: an integration with niri binds (dock order ↔ Mod+1..9 in binds.kdl).
**In the bar** (2026-11-04): the `taskbar` module puts the same apps row into the bar (left or
center), for one Windows-like bar at the bottom with the dock `off`.

## 10. Config

One file, `~/.config/nshell/config.kdl`, the same format as niri. Save it and the shell picks it
up. Roughly (it grows as steps land):

```kdl
bar {
    height 34
    left "window-title"
    center "clock"
    right "tray" "keyboard" "network" "bluetooth" "volume" "battery" "control"
}

clock {
    format "%d %b  %H:%M"
}

window-title {
    rewrite "^(.*) — Mozilla Firefox$" "$1"
    rewrite "^(.*) - Discord$" "$1"
}

idle {
    lock-command "veila lock"
    inhibit-apps "vlc" "mpv" "steam_app_.*"

    on-ac {
        dim "2m"
        lock "5m"
        dpms "15m"
        suspend "30m"
    }
    on-battery {
        dim "1m"
        dpms "3m"
        lock "6m"
        suspend "10m"
    }
}

wallpaper "~/.config/wallpapers/m.jpg"

dock {
    pinned "com.mitchellh.ghostty" "dev.zed.Zed" "firefox" "org.telegram.desktop"
}
```

Styles (colours, transparency, radii) live in `style.css` next to it, also applied on save.
A mistake in the config doesn't crash the shell: it keeps the previous settings and shows ⚠ on
the bar with the reason in its tooltip.

## 11. Technology (decisions)

Longer explanations are in `wayle/NEW-SHELL.md`. In short:

| What | Decision | Why |
|---|---|---|
| language | Rust | fast, little memory, the fork's experience |
| windows | GTK4 + gtk4-layer-shell | tested: blur, shadows, transparency |
| UI code | plain gtk-rs, no relm4 | simpler and clearer for a small project |
| async | the GTK main loop | no extra threads |
| D-Bus | GLib's D-Bus (gio) | already inside GTK, runs on the main loop: no threads, no extra crates (was: zbus) |
| niri | official `niri-ipc` | windows, layouts, outputs, dpms |
| sound | libpulse (PipeWire speaks it) on the GTK main loop | ready-made, no threads of its own |
| config | KDL | same as niri |
| rendering | GL | less CPU when text changes often |
| menus | own layer-shell windows, not GTK popovers | niri closes a popover when its size changes |
| blur | GTK's own (4.24+), from CSS `backdrop-filter` | GTK holds the surface's blur object |
| idle inhibit | Wayland protocol via wayland-client on GDK's connection | no raw FFI |

**Libraries vs our own.** Battery (UPower), brightness, layouts, window title, idle, wallpaper —
our own, little code. Sound, tray, notification server — prefer a ready crate where one fits.

## 12. Resource targets

Memory ~40–60 MB, under 10 threads, ~0% CPU when idle (the fork today: 94 MB, 36 threads, 0.25%).

- no timer polling, everything event-driven; the clock wakes once a minute;
- menus, OSD, popups are created when needed;
- one connection per service.

Measure after each big step, the same way as in the fork, for an honest before/after.

| Step | Memory (PSS) | Threads | Idle CPU | Note |
|---|---|---|---|---|
| 1, Vulkan | 24 MB | 10 | 0% | title spinner at 10/s: 0.8% |
| 3, GL | 38 MB | 17 | 0% (0 wakeups/s) | 13 threads are Mesa's GL driver, sleeping |
| 5, menu open | 50 MB | 24 | 0% | |

Nested niri, release build. We'll try trimming Mesa's threads with driver settings later.

## 13. How we work

- You're in GNOME right now. The bar can't run in GNOME itself (GNOME has no panel windows), so
  it's tested in a **nested niri** — niri opens as a window inside GNOME and the shell runs there.
  Your session isn't touched. I take the screenshots.
- Idle and locking can't be tested in a window — that step needs a real niri session.
- git from the start, one step — one commit. Something breaks — we roll back.
- After each step: what's done, a screenshot, what to look at and try.
- The wayle fork stays the main bar until nshell replaces it.

## 14. Steps

Each step ends with something you can see or try.

0. ✅ **Skeleton.** Project, nested niri sandbox, an empty transparent bar.
1. ✅ **Bar: clock and window title.** Shadows, app icon, rewrite rules.
2. ✅ **KDL config** reloaded on save + `style.css`.
3. ✅ **Simple modules:** battery, keyboard layout, volume.
4. ✅ **Harder modules:** network, Bluetooth, tray.
5. ✅ **Module menus** + switching on hover, blur.
6. ✅ **Control center.**
7. ✅ **OSD.**
8. ✅ **Notifications** (popups + center in the clock menu).
9. **Idle manager** — needs a real niri session. Then stasis goes.
10. ✅ **Wallpaper** — swaybg goes.
11. ✅ **Dock** (basic) — ironbar goes. Later: pin order synced with niri binds.
11b. ✅ **Launcher** — vicinae goes (apps, switching to running ones, files).
12. **Switch over:** nshell instead of the wayle fork, final measurement.
13. Later: docker module, our own lock screen. (Layout OSD: done.)

After steps 7 and 8 swayosd and swaync can go.

### Needed when switching over

- A rule in the niri config so menus blur the windows under them, not just the wallpaper (x-ray):
  `layer-rule { match namespace="^nshell-"; background-effect { xray false; }; }`
  (already in the sandbox, see `scripts/dev-nested.sh`).
- swaync must not run: only one program can be the notification server (nshell logs a warning
  and shows nothing if another one has the name). Same for swayosd (no conflict, just doubled OSD).

## 15. Open questions (summary)

- window-title menu;
- idle timings and the order on battery;

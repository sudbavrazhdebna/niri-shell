# TODO

Agreed, not started (or not finished). Newest wishes at the bottom of each part.

## Next

- **Switch over** — nshell alone: wayle, ironbar, stasis, swaybg out of autostart, final
  measurement against the numbers in DESIGN.md.

## Settings GUI

A window for graphical settings, writing the same `config.kdl` (through the `kdl` crate,
keeping comments) and the `:root { … }` block of the user's `style.css`; changes apply
at once. Page by page:

- Appearance: accent, glass colour and transparency, edge, shadow, with a live preview.
- Bar and dock: modules per zone (drag to order), pinned apps, autohide.
- Later: idle times, displays rule, notifications.

## Features

- **Night light**: warmer colours in the evening (wlr-gamma-control in niri), a tile in
  the control center, optionally on a schedule.
- **Screenshot and screen recording** tiles: area, screen or window; recording shows in
  the bar and stops on a click.
- **Battery charge limit** toggle in the battery menu (60% / full before a trip). The user
  thinks the BIOS limits charging too: check that first. Writing the limit needs a
  permission rule (udev/polkit).
- **External monitor brightness** over DDC/CI: needs `ddcutil` and access to `/dev/i2c-*`
  (the `i2c` group or a udev rule). Until then its slider is greyed out.
- **Media covers from https URLs** (only file:// ones load now).
- **Window menu checkmarks** for Maximize and Fullscreen: niri's IPC doesn't report those
  states; guessing from sizes would lie with zero gaps.
- From DESIGN.md: docker module, our own lock screen.
- **Shared app activation for dock and hotkeys**: move the behaviour of
  `~/.local/bin/app-toggle` into the shell's Rust code, reusing the existing niri state,
  app matching and launch helpers. Expose commands through `nshell-msg`, e.g.
  `activate-app <app-id>` and `activate-slot <1..9>`; slot numbers follow the current
  pinned-app order, so reordering the dock also updates Mod+1..9 targets without
  rewriting the bindings. Preserve deliberate differences such as `nautilus-toggle`
  only searching the current workspace. No immediate replacement needed.
- **Drag to reorder pinned dock icons**: start with pinned apps only, saving the final
  order through the existing KDL update helper after a successful drop. Distinguish
  clicks from dragging (activation currently runs on press), keep the dock open and
  preserve its widgets during the gesture; handle cancellation without saving changes.

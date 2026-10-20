# TODO

Agreed, not started (or not finished). Newest wishes at the bottom of each part.

## Next

- **Idle manager** — written on the branch `idle-wip`, not checked yet:
  1. sandbox run with short times (stages only log there: `NSHELL_SANDBOX`);
  2. real session with short times: brightness, keyboard light and monitors come back on
     input; lock before sleep (lid) with `lock-command "veila lock --wait-ready"`;
  3. merge, then stasis stays off for good.
  The user's needs: video and YouTube music in the browser keep the screen on; Steam
  games later (`steam_app_.*`). `lock-before-sleep.service` (hyprlock, unused) is still
  enabled: disable it with the user's OK.
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
- From DESIGN.md: docker module, our own lock screen, dock order ↔ Mod+1..9 in niri binds.

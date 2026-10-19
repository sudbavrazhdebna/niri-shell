# CLAUDE.md

nshell — a shell for niri (bar, menus, OSD, notifications, idle, wallpaper, dock). The user
doesn't write code and checks visually: after each step — a screenshot, what's done, what to try.
Everything in the project (UI text, error messages, docs, comments) is English. macOS is only a
loose reference for the look.

- [DECISIONS.md](DECISIONS.md) — non-obvious choices and why; read before changing them, add new ones.
- [STYLING.md](STYLING.md) — how styles work and the CSS classes; keep it in step with style.css.
- [DESIGN.md](DESIGN.md) — design document: what we build, decisions, step order. `[decide]`
  marks questions for the user; without an answer, take the document's proposal and say so.
- The living spec for behaviour and the reference for tricky parts is the fork in
  `~/Projects/wayle` (`NEW-SHELL.md`, `ROADMAP.md`, `TODO.md`). Don't copy code from it.

Code: plain gtk-rs without relm4, everything on the GTK main loop, no tokio. No timer polling —
events only. `unwrap`/`expect` are denied by clippy. Data from D-Bus/apps must never panic the
shell: use `services::dbus::{child, lookup}`, not `child_value`/`VariantDict` on remote data.
Buttons in CSS: use `background`, not `background-color` (the theme's image wins otherwise).

Checks: `scripts/dev-nested.sh` (niri in a window, works from GNOME too; log `.dev/nshell.log`;
stops the previous sandbox first), screenshot `scripts/dev-shot.sh <path>`, commands inside
`scripts/dev-run.sh <cmd>`, stop `scripts/dev-stop.sh` (sandbox processes only — never
`pkill -f` with a pattern that may match the real session or your own shell), measure
`scripts/dev-measure.sh 60`. Debug-build hooks: `NSHELL_DEV_MENU=<module>[@secs]` opens a
menu, `NSHELL_DEV_TRAY_MENU=1` opens tray menus, `NSHELL_DEV_AWAKE=1` turns on Stay awake.
Commands to the running shell: `nshell msg show-dock [secs]` (D-Bus `dev.nshell.Shell`).
Fake apps: `scripts/test-tray-item.py`, `scripts/test-mpris-player.py`, `scripts/test-backdrop.py` (stripes to judge blur). One step — one commit.

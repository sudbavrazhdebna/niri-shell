# nshell

A shell for the [niri](https://github.com/YaLTeR/niri) compositor: bar, menus, control
center, notifications, OSD, dock, launcher, wallpaper and idle handling, in one program
(Rust, GTK4, no libadwaita). It looks the same whatever theme apps use.

## Install

```sh
cargo build --release
```

It produces three programs in `target/release/`:

- `nshell`: the shell itself;
- `nshell-tray-watcher`: keeps tray icons registered while the shell restarts;
- `nshell-msg`: sends commands to the running shell (see below).

Run the first two as systemd user services started with niri (`WantedBy=niri.service`,
`PartOf=niri.service`), the shell with `Restart=on-failure`. After a rebuild, restart it:
`systemctl --user restart nshell`.

## What niri needs

In niri's config:

```kdl
// Blur what is really behind the bar and menus, not only the wallpaper.
layer-rule {
    match namespace="^nshell-"
    background-effect {
        xray false
    }
}

// The wallpaper stays in the overview (it blurs there).
layer-rule {
    match namespace="^nshell-wallpaper$"
    place-within-backdrop true
}

binds {
    Mod+D { spawn "nshell-msg" "launcher"; }
    Mod+A { spawn "nshell-msg" "show-dock"; }   // with the dock on autohide
}
```

Blur needs niri 26.04 or newer and GTK 4.24 or newer. GTK requests blur for each glass
card. The layer rule selects what to sample with `xray false`. Use niri's top-level
`blur` block to tune `saturation` and `noise` for blurred regions. On a broad
`^nshell-` layer rule, `blur true`, `saturation` and `noise` can affect the full-screen
click catcher behind menus.

Take out other bars, notification daemons, idle daemons and wallpaper tools from niri's
autostart: nshell does their jobs.

## Commands

`nshell-msg <command>` (fast: it doesn't load GTK; `nshell msg <command>` does the same):

- `launcher`: opens the launcher (apps, files, running windows);
- `show-dock [seconds]`: shows a hidden dock for a few seconds (3 by default).

## Settings

- **`~/.config/nshell/config.kdl`**: what is shown and how it behaves: bar modules and
  position, clock format, idle times, dock, wallpaper, blur. Written on first start from
  [`config/default.kdl`](config/default.kdl), which explains every option. Saved changes
  apply at once; a mistake shows ⚠ on the bar (hover it for the reason) and the previous
  settings stay.
- Adding `taskbar` to any bar zone hides the standalone dock. The taskbar still uses
  the `dock` section's pinned apps.
- In `control { … }`, `layout "grid"` gives the quick switches two columns (the default).
  Set `layout "row"` to put them in a single row.
- In `dock { … }`, `mode "reserved"` keeps the dock visible and reserves a bottom
  strip for it in the workspace. `mode "overlay"` is the default and allows `autohide`.
- Add `no-overview` inside `dock { … }` to hide its overview button and the separator
  before it. Remove that line to restore them. niri's overview itself is unaffected.
- With `autohide`, the dock stays visible in the overview and starts hiding as soon as
  the overview closes. Set `show-in-overview "off"` inside `dock { … }` to disable this.
- **`~/.config/nshell/style.css`**: the look. It goes on top of the built-in style;
  [STYLING.md](STYLING.md) explains how and lists every class.

## For development

- [DESIGN.md](DESIGN.md): what nshell is meant to be, the decided behaviour of each part.
- [DECISIONS.md](DECISIONS.md): choices that weren't obvious, and why.
- [TODO.md](TODO.md): agreed and not done yet.
- `scripts/dev-nested.sh` runs niri with the shell in a window (log in `.dev/nshell.log`);
  `scripts/dev-restart.sh`, `dev-shot.sh`, `dev-run.sh`, `dev-stop.sh` work with it. See
  [CLAUDE.md](CLAUDE.md) for the debug hooks.

#!/usr/bin/env bash
# Opens niri as a window (works inside GNOME or inside niri) and runs a dev nshell in it.
#
# Everything is kept apart from the real session: own D-Bus session, own XDG dirs in .dev/,
# host niri config without autostart. Inside the window Mod is Alt.
#
#   scripts/dev-nested.sh            build (debug) and run
#   NO_BUILD=1 scripts/dev-nested.sh run the last build
#   PROFILE=release ...              use the release build
#
# Log: .dev/nshell.log. Screenshot: scripts/dev-shot.sh. Commands inside: scripts/dev-run.sh.
# Stop: scripts/dev-stop.sh (also runs before every start).

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
DEV="$ROOT/.dev"
NIRI_HOST="${XDG_CONFIG_HOME:-$HOME/.config}/niri"
PROFILE="${PROFILE:-debug}"

# One sandbox at a time: GNOME doesn't draw a covered nested window, and niri stalls.
"$ROOT/scripts/dev-stop.sh"

mkdir -p "$DEV/config/nshell" "$DEV/data" "$DEV/state" "$DEV/shots"
# Icon themes installed for the user (e.g. MoreWaita) live in the real data dir.
if [[ -d "$HOME/.local/share/icons" && ! -e "$DEV/data/icons" ]]; then
    ln -s "$HOME/.local/share/icons" "$DEV/data/icons"
fi

if [[ "${NO_BUILD:-0}" != 1 ]]; then
    if [[ "$PROFILE" == release ]]; then
        cargo build --manifest-path "$ROOT/Cargo.toml" --release
    else
        cargo build --manifest-path "$ROOT/Cargo.toml"
    fi
fi

{
    if [[ -d "$NIRI_HOST" ]]; then
        for f in "$NIRI_HOST"/*.kdl; do
            name=$(basename "$f")
            [[ "$name" == config.kdl || "$name" == autostart.kdl ]] && continue
            echo "include \"$f\""
        done
        # Top-level settings of the host config.kdl, without its includes.
        grep -vE '^\s*(include|screenshot-path) ' "$NIRI_HOST/config.kdl" || true
    fi
    # Blur what's behind nshell's menus and popups, not just the wallpaper (x-ray).
    # The real session needs the same rule in the niri config.
    echo 'layer-rule { match namespace="^nshell-"; background-effect { xray false; }; }'
    echo "screenshot-path \"$DEV/shots/%Y%m%d-%H%M%S.png\""
    echo "hotkey-overlay { skip-at-startup; }"
} > "$DEV/niri.kdl"

BIN="$ROOT/target/$PROFILE/nshell"

exec niri -c "$DEV/niri.kdl" -- dbus-run-session -- sh -c '
    echo "$DBUS_SESSION_BUS_ADDRESS" > "$1/bus"
    echo "$NIRI_SOCKET" > "$1/niri-socket"
    echo "$WAYLAND_DISPLAY" > "$1/wayland-display"
    # No accessibility bus in the private D-Bus session.
    export GTK_A11Y=none
    export XDG_CONFIG_HOME="$1/config" XDG_DATA_HOME="$1/data" XDG_STATE_HOME="$1/state"
    exec "$2" 2>&1 | tee "$1/nshell.log"
' sh "$DEV" "$BIN"

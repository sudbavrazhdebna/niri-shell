#!/usr/bin/env bash
# Runs a command inside the nested niri session (its Wayland display, D-Bus and niri socket).
#
#   scripts/dev-run.sh niri msg windows
#   scripts/dev-run.sh notify-send hello

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
DEV="$ROOT/.dev"

export NIRI_SOCKET=$(cat "$DEV/niri-socket")
export DBUS_SESSION_BUS_ADDRESS=$(cat "$DEV/bus")
export WAYLAND_DISPLAY=$(cat "$DEV/wayland-display")
exec "$@"

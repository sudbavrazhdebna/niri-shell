#!/usr/bin/env bash
# Stops the sandbox: every process started inside the nested session (found by its private
# D-Bus address), then the nested niri. Never touches the real session.

set -uo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
DEV="$ROOT/.dev"
BUS=$(cat "$DEV/bus" 2>/dev/null || true)

if [[ -n "$BUS" ]]; then
    for dir in /proc/[0-9]*; do
        pid=${dir#/proc/}
        [[ "$pid" == "$$" ]] && continue
        { grep -qzxF "DBUS_SESSION_BUS_ADDRESS=$BUS" "$dir/environ" && kill "$pid"; } 2>/dev/null
    done
fi
# Its command line holds a multi-line script, so match only the start.
pkill -f "^niri -c $DEV/niri.kdl" 2>/dev/null
# A nested niri whose window GNOME stopped drawing (covered, minimized) hangs waiting
# for a frame and ignores SIGTERM.
for _ in $(seq 1 20); do
    pgrep -f "^niri -c $DEV/niri.kdl" >/dev/null || exit 0
    sleep 0.1
done
pkill -KILL -f "^niri -c $DEV/niri.kdl" 2>/dev/null
exit 0

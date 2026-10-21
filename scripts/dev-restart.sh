#!/usr/bin/env bash
# Restarts only the shell inside the sandbox: its bus, niri and test apps keep running,
# as in a real session when the shell crashes or is restarted. The log is appended to.
#
#   scripts/dev-restart.sh            the debug build
#   PROFILE=release ...               the release build

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
DEV="$ROOT/.dev"
BIN="$ROOT/target/${PROFILE:-debug}/nshell"
BUS=$(cat "$DEV/bus")

# The sandbox's shell only: the process with that binary and the sandbox's bus.
for pid in $(pgrep -x nshell); do
    if grep -qzxF "DBUS_SESSION_BUS_ADDRESS=$BUS" "/proc/$pid/environ" 2>/dev/null; then
        kill "$pid"
    fi
done
sleep 0.5

export XDG_CONFIG_HOME="$DEV/config" XDG_DATA_HOME="$DEV/data" XDG_STATE_HOME="$DEV/state"
export GTK_A11Y=none NSHELL_SANDBOX=1
setsid "$ROOT/scripts/dev-run.sh" "$BIN" >> "$DEV/nshell.log" 2>&1 < /dev/null &
echo "restarted $BIN"

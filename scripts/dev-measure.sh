#!/usr/bin/env bash
# Samples a running shell: memory, threads, CPU and wakeups over N seconds.
#
#   scripts/dev-measure.sh [seconds]          the newest running nshell
#   PID=1234 scripts/dev-measure.sh 60        any process (e.g. the wayle fork)

set -euo pipefail

SECS="${1:-60}"
PID="${PID:-$(pgrep -n -x nshell || true)}"
if [[ -z "$PID" ]]; then
    echo "nshell is not running" >&2
    exit 1
fi

HZ=$(getconf CLK_TCK)
cpu_ticks() { awk '{print $14 + $15}' "/proc/$PID/stat"; }
ctx() { awk '/voluntary_ctxt_switches/ {s += $2} END {print s}' "/proc/$PID/status"; }
status() { awk -v k="$1:" '$1 == k {print $2}' "/proc/$PID/status"; }

t0=$(cpu_ticks); c0=$(ctx)
sleep "$SECS"
t1=$(cpu_ticks); c1=$(ctx)

pss=$(awk '$1 == "Pss:" {print $2}' "/proc/$PID/smaps_rollup")
cpu=$(awk -v d=$((t1 - t0)) -v hz="$HZ" -v s="$SECS" 'BEGIN {printf "%.2f", d / hz / s * 100}')

printf 'pid=%s window=%ss\n' "$PID" "$SECS"
printf 'pss=%s MB  rss_anon=%s MB  threads=%s\n' \
    $((pss / 1024)) $(($(status RssAnon) / 1024)) "$(status Threads)"
printf 'cpu=%s%%  wakeups=%s/s\n' "$cpu" $(((c1 - c0) / SECS))

#!/usr/bin/env bash
# Screenshots the nested niri from dev-nested.sh and prints the file path.
#
#   scripts/dev-shot.sh [path]

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
DEV="$ROOT/.dev"
OUT="${1:-$DEV/shots/$(date +%Y%m%d-%H%M%S).png}"

NIRI_SOCKET=$(cat "$DEV/niri-socket") \
    niri msg action screenshot-screen --show-pointer false --path "$OUT" >/dev/null

for _ in $(seq 1 30); do
    [[ -s "$OUT" ]] && break
    sleep 0.1
done
echo "$OUT"

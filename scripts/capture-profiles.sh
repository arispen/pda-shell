#!/usr/bin/env bash
# Screenshots of Today and the Start menu in every density profile and both
# orientations, for comparing profiles (docs/screenshots.md).
#
#   ./scripts/capture-profiles.sh [OUT_DIR]     # default screenshots/profiles
set -uo pipefail
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="${1:-$REPO/screenshots/profiles}"
mkdir -p "$OUT"
(cd "$REPO" && cargo build) || exit 1
ui() { python3 "$REPO/scripts/lib/pda_ui.py" "$@" 2>/dev/null; }

for density in compact balanced readable; do
    for orient in portrait landscape; do
        home="$(mktemp -d)"; ready="$(mktemp)"; rm -f "$ready"
        "$REPO/scripts/dev-session.sh" --orientation "$orient" --density "$density" --headless --no-build \
            --home "$home" --reset-home --ready-file "$ready" > /dev/null 2>&1 &
        session=$!
        for _ in $(seq 300); do [ -s "$ready" ] && break; sleep 0.1; done
        # shellcheck disable=SC1090
        . "$ready"; export DISPLAY
        pid="$(xdotool search --classname pda-shell getwindowpid %1 2>/dev/null)"
        export DBUS_SESSION_BUS_ADDRESS
        DBUS_SESSION_BUS_ADDRESS="$(tr '\0' '\n' < "/proc/$pid/environ" | sed -n 's/^DBUS_SESSION_BUS_ADDRESS=//p')"
        sleep 1.5
        xdotool mousemove 300 5
        import -window root "$OUT/$density-$orient-today.png"
        ui tap pda-shell button "^Start$" --frame "status bar" --timeout 5 >/dev/null
        sleep 1; xdotool mousemove 300 5; sleep 0.3
        import -window root "$OUT/$density-$orient-start.png"
        kill "$session"; wait "$session" 2>/dev/null
        rm -rf "$home" "$ready"
        echo "$density $orient done"
    done
done

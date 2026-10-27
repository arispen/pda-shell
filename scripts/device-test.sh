#!/usr/bin/env bash
# Checks of the device session (scripts/device-session.sh) that need no
# device: it runs on Xvfb with --xserver Xvfb, so the X server is virtual but
# everything else (display plan, D-Bus, Openbox, shell supervision, End
# session) is the device code path. Physical-panel results are recorded
# separately (docs/device-checklist.md).
#
#   ./scripts/device-test.sh                    # 1024x600, 600x1024, 800x480
#   ./scripts/device-test.sh --screen 800x480   # one size
#   ./scripts/device-test.sh --no-build
#
# Per screen size: the session comes up with the shell laid out for that
# size (bars at the real edges, nothing hard-coded), startup times are
# logged, a crashed shell is restarted and finds the open program again,
# End session still lets a program ask about unsaved work, and nothing is
# left running. Once: a shell that keeps crashing ends the session with
# status 1 instead of looping.
set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCREENS=(1024x600 600x1024 800x480)
BUILD=1
SHOTS="$REPO/screenshots/device-test"
while [ $# -gt 0 ]; do
    case "$1" in
        --screen) SCREENS=("${2:?}"); shift 2 ;;
        --screenshots) SHOTS="${2:?}"; shift 2 ;;
        --no-build) BUILD=0; shift ;;
        -h|--help) sed -n '2,17p' "$0"; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done
for tool in xdotool xprop xwininfo import python3 Xvfb; do
    command -v "$tool" >/dev/null || { echo "missing $tool; run scripts/setup-debian.sh" >&2; exit 1; }
done
[ "$BUILD" -eq 1 ] && (cd "$REPO" && cargo build) || [ "$BUILD" -eq 0 ] || exit 1
TARGET="${CARGO_TARGET_DIR:-$REPO/target}/debug"
mkdir -p "$SHOTS"

PASS=0
FAIL=0
ok()   { PASS=$((PASS + 1)); printf '  ok    %s\n' "$*"; }
fail() { FAIL=$((FAIL + 1)); printf '  FAIL  %s\n' "$*"; }
check() { local what="$1"; shift; if "$@"; then ok "$what"; else fail "$what"; fi; }
note() { printf '  note  %s\n' "$*"; }
wait_for() { local t="${WAIT:-100}"; while [ "$t" -gt 0 ]; do "$@" && return 0; sleep 0.1; t=$((t - 1)); done; return 1; }
wait_n() { local WAIT="$1"; shift; wait_for "$@"; }

clients() { xprop -root _NET_CLIENT_LIST 2>/dev/null | sed -n 's/.*window id # //p' | tr -d ','; }
class_of() { xprop -id "$1" WM_CLASS 2>/dev/null | sed -n 's/.*", "\(.*\)"/\1/p'; }
type_of() { xprop -id "$1" _NET_WM_WINDOW_TYPE 2>/dev/null | sed -n 's/.*_NET_WM_WINDOW_TYPE_//p'; }
geom() { xwininfo -id "$1" 2>/dev/null | awk '/Absolute upper-left X/{x=$4} /Absolute upper-left Y/{y=$4} /Width:/{w=$2} /Height:/{h=$2} END{print x","y","w","h}'; }
window_of_class() { for w in $(clients); do [ "$(class_of "$w")" = "$1" ] && { echo "$w"; return 0; }; done; return 1; }
has_class() { window_of_class "$1" >/dev/null; }
no_class() { ! window_of_class "$1" >/dev/null; }
mapped_named() { xdotool search --onlyvisible --name "^$1\$" >/dev/null 2>&1; }
# Not every window of the class carries _NET_WM_PID (GTK's leader window).
shell_pid() { local w; for w in $(xdotool search --classname pda-shell 2>/dev/null); do xdotool getwindowpid "$w" 2>/dev/null && return 0; done; return 1; }
shell_windows() { local n=0 w; for w in $(clients); do [ "$(class_of "$w")" = pda-shell ] && n=$((n + 1)); done; [ "$n" -ge 3 ]; }
tap() { xdotool mousemove "$1" "$2"; sleep 0.15; xdotool click 1; }
ui() { python3 "$REPO/scripts/lib/pda_ui.py" "$@" 2>/dev/null; }
found() { ui find "$@" >/dev/null; }
no_leftovers() {
    local env
    for env in /proc/[0-9]*/environ; do
        if { tr '\0' '\n' < "$env"; } 2>/dev/null | grep -q "^PDA_SESSION_TAG=$1"; then
            echo "    left over: $(tr '\0' ' ' < "${env%environ}cmdline")"; return 1
        fi
    done
    return 0
}
gone() { ! kill -0 "$1" 2>/dev/null; }
restarted() { local p; p="$(shell_pid)" && [ "$p" != "$1" ]; }

DISPLAY_NUM=30
run_screen() {
    local screen="$1" W H
    W="${screen%x*}"; H="${screen#*x}"
    DISPLAY_NUM=$((DISPLAY_NUM + 1))
    echo "== device session on a ${W}x${H} virtual screen"
    local home ready
    home="$(mktemp -d "${TMPDIR:-/tmp}/pda-device-home.XXXXXX")"
    ready="$(mktemp)"; rm -f "$ready"
    # --orientation native: Xvfb cannot rotate, so every size is tested as
    # the panel would report it.
    HOME="$home" XDG_RUNTIME_DIR="" XDG_STATE_HOME="$home/.local/state" XDG_CONFIG_HOME="$home/.config" \
        DBUS_SESSION_BUS_ADDRESS="" PDA_BIN_DIR="$TARGET" \
        "$REPO/scripts/device-session.sh" --xserver Xvfb --screen "$screen" --display ":$DISPLAY_NUM" \
        --orientation native --seed-samples --ready-file "$ready" > "$SHOTS/$screen-session.out" 2>&1 &
    local session=$!
    if ! wait_n 300 test -s "$ready"; then
        fail "session became ready"; kill "$session" 2>/dev/null; wait "$session"; return
    fi
    # shellcheck disable=SC1090
    . "$ready"; export DISPLAY XAUTHORITY DBUS_SESSION_BUS_ADDRESS
    local tag="pda-device-$session-"

    check "started without Xephyr or a desktop (no nested X server)" bash -c "! pgrep -x Xephyr >/dev/null"
    check "startup times logged" grep -q "^shell=" "$LOG_DIR/startup.txt"
    note "startup: $(tr '\n' ' ' < "$LOG_DIR/startup.txt")"
    check "display plan logged" grep -q "display plan: output screen mode $screen" "$LOG_DIR/session.log"
    check "screen is ${W}x${H}" grep -q "screen ${W}x${H}" "$LOG_DIR/session.log"
    check "no touchscreen here: logged, no stylus daemon started" grep -q "no touchscreen (pda-device inputs" "$LOG_DIR/session.log"
    local wa TOP WA_H BOTTOM
    wa="$(xprop -root _NET_WORKAREA | sed 's/.*= //; s/ //g' | cut -d, -f1-4)"
    IFS=, read -r _ TOP _ WA_H <<< "$wa"
    BOTTOM=$((H - TOP - WA_H))
    check "bars reserve the top and bottom edges (work area $wa)" [ "$TOP" -gt 0 -a "$BOTTOM" -gt 0 -a "${wa%%,*}" = 0 ]
    local top="" bottom="" desk=""
    for w in $(clients); do
        case "$(type_of "$w"):$(geom "$w")" in
            DOCK:0,0,$W,$TOP) top="$w" ;;
            DOCK:0,$((H - BOTTOM)),$W,$BOTTOM) bottom="$w" ;;
            DESKTOP:0,$TOP,$W,$WA_H) desk="$w" ;;
        esac
    done
    check "status bar spans the top edge" [ -n "$top" ]
    check "command bar spans the bottom edge" [ -n "$bottom" ]
    check "Today fills the work area" [ -n "$desk" ]
    check "Today shows the seeded documents" wait_n 50 found pda-shell "list item" "^Budget 2026.xlsx$" --frame "^Today$"
    import -window root "$SHOTS/$screen-today.png"

    # --- a program, then a shell crash -------------------------------------------
    tap 30 $((TOP / 2)); wait_for mapped_named Start; sleep 0.5
    xdotool type --delay 40 mousepad; sleep 0.4; xdotool key Return
    check "Mousepad launched from the Start menu" wait_n 150 has_class Mousepad
    check "Mousepad fills the work area" wait_for bash -c "[ \"\$(xwininfo -id $(window_of_class Mousepad 2>/dev/null || echo 0) 2>/dev/null | awk '/Width:/{w=\$2} /Height:/{h=\$2} END{print w\"x\"h}')\" = '${W}x${WA_H}' ]"
    xdotool type --delay 30 "unsaved device text"
    local old; old="$(shell_pid)"
    check "shell process found (pid $old)" [ -n "$old" ]
    # SIGABRT: a panic=abort or failed assertion. (Rust swallows the first
    # SIGSEGV sent with kill: its stack-overflow handler expects a re-fault.)
    kill -ABRT "$old"
    check "crashed shell is restarted" wait_n 100 restarted "$old"
    check "restart is logged" grep -q "pda-shell exited with status .*; restarting (1/5)" "$LOG_DIR/session.log"
    check "restarted shell maps its bars again" wait_n 100 shell_windows
    check "Mousepad survived the shell crash" has_class Mousepad
    sleep 1
    check "the restarted shell shows Mousepad in its title" wait_n 50 found pda-shell label "Mousepad|Untitled" --frame "status bar"
    import -window root "$SHOTS/$screen-after-restart.png"

    # --- End session with unsaved work -----------------------------------------------
    ui tap pda-shell button "^Start$" --frame "status bar" >/dev/null; sleep 0.8
    ui tap pda-shell "list item" "^End session$" --frame "^Start$" >/dev/null
    check "Mousepad asks before discarding unsaved changes" wait_n 100 found mousepad button "^Don't Save$"
    check "session keeps running while the question is open" kill -0 "$session"
    import -window root "$SHOTS/$screen-end-session.png"
    ui tap mousepad button "^Don't Save$" >/dev/null
    check "session ends once Mousepad has closed" wait_n 300 gone "$session"
    wait "$session"; local status=$?
    check "session exited with status 0 (got $status)" [ "$status" -eq 0 ]
    check "no session processes left behind" no_leftovers "$tag"
    check "X server stopped" bash -c "! xdpyinfo -display :$DISPLAY_NUM >/dev/null 2>&1"
    cp "$LOG_DIR/session.log" "$SHOTS/$screen-session.log" 2>/dev/null
    rm -rf "$home" "$ready"
    unset DISPLAY XAUTHORITY DBUS_SESSION_BUS_ADDRESS
}

crash_loop() {
    echo "== a shell that keeps crashing"
    local home bin
    home="$(mktemp -d "${TMPDIR:-/tmp}/pda-device-home.XXXXXX")"
    bin="$home/bin"; mkdir -p "$bin"
    ln -s "$TARGET/pda-device" "$bin/pda-device"
    printf '#!/bin/sh\necho "simulated crash" >&2\nexit 3\n' > "$bin/pda-shell"; chmod +x "$bin/pda-shell"
    DISPLAY_NUM=$((DISPLAY_NUM + 1))
    local start; start="$(date +%s)"
    HOME="$home" XDG_RUNTIME_DIR="" XDG_STATE_HOME="$home/.local/state" XDG_CONFIG_HOME="$home/.config" \
        DBUS_SESSION_BUS_ADDRESS="" PDA_BIN_DIR="$bin" timeout 60 \
        "$REPO/scripts/device-session.sh" --xserver Xvfb --display ":$DISPLAY_NUM" > "$SHOTS/crash-loop.out" 2>&1
    local status=$?
    check "session gives up with status 1 (got $status) after $(($(date +%s) - start)) s" [ "$status" -eq 1 ]
    check "giving up is logged" grep -q "5 times within 60 s; giving up" "$home/.local/state/pda-shell/logs/latest/session.log"
    check "X server stopped" bash -c "! xdpyinfo -display :$DISPLAY_NUM >/dev/null 2>&1"
    rm -rf "$home"
}

calibration() {
    echo "== stylus calibration screen (pda-device calibrate)"
    local home; home="$(mktemp -d "${TMPDIR:-/tmp}/pda-device-home.XXXXXX")"
    DISPLAY_NUM=$((DISPLAY_NUM + 1))
    Xvfb ":$DISPLAY_NUM" -screen 0 1024x600x24 -nolisten tcp > /dev/null 2>&1 &
    local xvfb=$!
    export DISPLAY=":$DISPLAY_NUM"
    wait_for xdpyinfo >/dev/null 2>&1
    openbox > /dev/null 2>&1 &
    local wm=$!
    sleep 0.5
    local file="$home/.config/pda-shell/stylus-calibration"
    mkdir -p "$(dirname "$file")"; echo "1 0 0 0 1 0" > "$file"
    HOME="$home" XDG_CONFIG_HOME="" "$TARGET/pda-device" calibrate > "$home/out" 2>&1 &
    local cal=$!
    check "calibration screen opens full screen" wait_n 50 mapped_named "Calibrate stylus"
    sleep 0.5
    # A miscalibrated panel: every tap lands 20 px right of and 10 px below the cross.
    for t in "0.08 0.08" "0.92 0.08" "0.92 0.92" "0.08 0.92" "0.5 0.5"; do
        set -- $t
        tap $(awk -v a="$1" 'BEGIN { print int(a * 1024) + 20 }') $(awk -v a="$2" 'BEGIN { print int(a * 600) + 10 }')
        sleep 0.3
    done
    sleep 0.5
    import -window root "$SHOTS/calibrate-saved.png"
    local c; c="$(tr '\n' ' ' < "$file")"
    check "saved calibration moves taps 20 px left, 10 px up ($c)" awk -v c="$c" 'BEGIN { split(c, m, " "); exit !(m[3] < -0.017 && m[3] > -0.021 && m[6] < -0.015 && m[6] > -0.018 && m[1] > 0.99 && m[5] > 0.99) }'
    # Close: centred below the crosses (Again, Close), 100 px above the bottom.
    tap $((1024 / 2 + 6 + 60)) $((600 - 100 - 23))
    check "Close ends the calibration" wait_n 50 gone "$cal"
    # Cancel puts the previous calibration back.
    cp "$file" "$home/before"
    HOME="$home" XDG_CONFIG_HOME="" "$TARGET/pda-device" calibrate > "$home/out2" 2>&1 &
    cal=$!
    wait_n 50 mapped_named "Calibrate stylus"; sleep 0.5
    tap 100 50; sleep 0.3
    check "during calibration the saved file is the identity" grep -q "^1.000000 0.000000 0.000000" "$file"
    kill -TERM "$cal"
    wait_n 50 gone "$cal"
    check "stopping it before the end restores the previous calibration" cmp -s "$file" "$home/before"
    kill "$wm" "$xvfb" 2>/dev/null; wait "$wm" "$xvfb" 2>/dev/null
    unset DISPLAY
    rm -rf "$home"
}

for s in "${SCREENS[@]}"; do run_screen "$s"; done
crash_loop
calibration
echo
echo "device test: $PASS passed, $FAIL failed (screenshots in $SHOTS)"
[ "$FAIL" -eq 0 ]

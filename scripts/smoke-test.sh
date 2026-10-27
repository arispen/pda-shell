#!/usr/bin/env bash
# End-to-end smoke test of the nested PDA session, driven with synthetic
# pointer events (xdotool). Runs headless on Xvfb, so it needs no host
# display and does not touch the developer's mouse.
#
#   ./scripts/smoke-test.sh                       # both orientations
#   ./scripts/smoke-test.sh --orientation landscape
#   ./scripts/smoke-test.sh --screenshots DIR     # keep screenshots (default screenshots/smoke)
#   ./scripts/smoke-test.sh --no-build
#
# Exit status is non-zero if any check fails. Text typed into the Start menu
# search uses synthetic key events: this test checks shell behaviour, not the
# on-screen keyboard.
set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ORIENTATIONS=(portrait landscape)
SHOTS="$REPO/screenshots/smoke"
BUILD=1
while [ $# -gt 0 ]; do
    case "$1" in
        --orientation) ORIENTATIONS=("${2:?}"); shift 2 ;;
        --screenshots) SHOTS="${2:?}"; shift 2 ;;
        --no-build) BUILD=0; shift ;;
        -h|--help) sed -n '2,13p' "$0"; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done
for tool in xdotool xprop xwininfo import python3; do
    command -v "$tool" >/dev/null || { echo "missing $tool; run scripts/setup-debian.sh" >&2; exit 1; }
done
[ "$BUILD" -eq 1 ] && (cd "$REPO" && cargo build) || [ "$BUILD" -eq 0 ] || exit 1
mkdir -p "$SHOTS"

PASS=0
FAIL=0
ok()   { PASS=$((PASS + 1)); printf '  ok    %s\n' "$*"; }
fail() { FAIL=$((FAIL + 1)); printf '  FAIL  %s\n' "$*"; }
check() { local what="$1"; shift; if "$@"; then ok "$what"; else fail "$what"; fi; }

# Poll a command until it succeeds (default 10 s).
wait_long() { WAIT=150 wait_for "$@"; }
wait_for() { local t="${WAIT:-100}"; while [ "$t" -gt 0 ]; do "$@" && return 0; sleep 0.1; t=$((t - 1)); done; return 1; }

root_prop() { xprop -root "$1" 2>/dev/null | sed -n 's/^[^=]*= *//p; s/.*window id # //p' | head -1; }
active() { xprop -root _NET_ACTIVE_WINDOW 2>/dev/null | sed -n 's/.*window id # //p'; }
clients() { xprop -root _NET_CLIENT_LIST 2>/dev/null | sed -n 's/.*window id # //p' | tr -d ','; }
class_of() { xprop -id "$1" WM_CLASS 2>/dev/null | sed -n 's/.*", "\(.*\)"/\1/p'; }
type_of() { xprop -id "$1" _NET_WM_WINDOW_TYPE 2>/dev/null | sed -n 's/.*_NET_WM_WINDOW_TYPE_//p'; }
geom() { xwininfo -id "$1" 2>/dev/null | awk '/Absolute upper-left X/{x=$4} /Absolute upper-left Y/{y=$4} /Width:/{w=$2} /Height:/{h=$2} END{print x","y","w","h}'; }
window_of_class() { for w in $(clients); do [ "$(class_of "$w")" = "$1" ] && { echo "$w"; return 0; }; done; return 1; }
has_class() { window_of_class "$1" >/dev/null; }
no_class() { ! window_of_class "$1" >/dev/null; }
is_active_class() { [ "$(class_of "$(active)")" = "$1" ]; }
showing_desktop() { [ "$(xprop -root _NET_SHOWING_DESKTOP | sed 's/.*= //')" = "$1" ]; }
mapped_named() { xdotool search --onlyvisible --name "^$1\$" >/dev/null 2>&1; }
unmapped_named() { ! mapped_named "$1"; }
# Override-redirect popups (popovers, context menus) of the shell.
shell_popup() {
    local pid; pid="$(cat "$SESSION_SHELL_PID")"
    for w in $(xdotool search --onlyvisible --pid "$pid" 2>/dev/null); do
        xwininfo -id "$w" 2>/dev/null | grep -q 'Override Redirect State: yes' && { echo "$w"; return 0; }
    done
    return 1
}
has_popup() { shell_popup >/dev/null; }
no_popup() { ! shell_popup >/dev/null; }
# Processes still carrying the session tag (see dev-session.sh).
no_leftovers() {
    local env
    for env in /proc/[0-9]*/environ; do
        if { tr '\0' '\n' < "$env"; } 2>/dev/null | grep -q "^PDA_SESSION_TAG=$1"; then
            echo "    left over: $(tr '\0' ' ' < "${env%environ}cmdline")"; return 1
        fi
    done
    return 0
}
tap() { xdotool mousemove "$1" "$2"; sleep 0.15; xdotool click 1; }
# Finds shell widgets by their accessible name (scripts/lib/pda_ui.py).
ui() { python3 "$REPO/scripts/lib/pda_ui.py" "$@" 2>/dev/null; }
shot() { import -window root "$SHOTS/$ORIENT-$1.png"; }
found_shell() { ui find pda-shell "$@" >/dev/null; }

run_orientation() {
    ORIENT="$1"
    case "$ORIENT" in portrait) W=600; H=1024 ;; landscape) W=1024; H=600 ;; esac
    echo "== $ORIENT ${W}x${H}"

    local ready; ready="$(mktemp)"; rm -f "$ready"
    # A fresh HOME: leftovers of earlier sessions (e.g. Mousepad offering to
    # restore a killed session) must not change what the test sees.
    local home; home="$(mktemp -d "${TMPDIR:-/tmp}/pda-smoke-home.XXXXXX")"
    "$REPO/scripts/dev-session.sh" --orientation "$ORIENT" --headless --no-build --ready-file "$ready" \
        --home "$home" --reset-home \
        > "$SHOTS/$ORIENT-session.out" 2>&1 &
    local session=$!
    if ! WAIT=200 wait_for test -s "$ready"; then
        fail "session became ready"; kill "$session" 2>/dev/null; wait "$session"; return
    fi
    # shellcheck disable=SC1090
    . "$ready"; export DISPLAY
    SESSION_SHELL_PID="$(mktemp)"
    xdotool search --classname pda-shell getwindowpid %1 > "$SESSION_SHELL_PID" 2>/dev/null
    # The session's own bus, for finding shell widgets over AT-SPI.
    export DBUS_SESSION_BUS_ADDRESS
    DBUS_SESSION_BUS_ADDRESS="$(tr '\0' '\n' < "/proc/$(cat "$SESSION_SHELL_PID")/environ" | sed -n 's/^DBUS_SESSION_BUS_ADDRESS=//p')"
    sleep 1
    # Bar heights come from the density profile; read them from the work area.
    local wa_h
    IFS=, read -r _ TOP _ wa_h <<< "$(root_prop _NET_WORKAREA | tr -d ' ')"
    BOTTOM=$((H - TOP - wa_h))
    WORKAREA="0,$TOP,$W,$wa_h"

    # --- session and shell surfaces
    check "bars reserve the screen edges (top $TOP, bottom $BOTTOM)" [ "$TOP" -gt 0 -a "$BOTTOM" -gt 0 ]
    local top="" bottom="" desk=""
    for w in $(clients); do
        case "$(type_of "$w"):$(geom "$w")" in
            DOCK:0,0,$W,$TOP) top="$w" ;;
            DOCK:0,$((H - BOTTOM)),$W,$BOTTOM) bottom="$w" ;;
            DESKTOP:$WORKAREA) desk="$w" ;;
        esac
    done
    check "status bar is a dock at the top edge" [ -n "$top" ]
    check "command bar is a dock at the bottom edge" [ -n "$bottom" ]
    check "Today is a desktop window filling the work area" [ -n "$desk" ]
    shot today

    # --- Start menu, search, launch
    tap 30 12
    check "Start opens the Start menu" wait_for mapped_named Start
    check "Start menu has keyboard focus" wait_for is_active_class pda-shell
    shot start-menu
    xdotool type --delay 40 mousepad; sleep 0.4; xdotool key Return
    check "Start menu closes after launching" wait_for unmapped_named Start
    check "Mousepad window appears" wait_long has_class Mousepad
    local mp; mp="$(window_of_class Mousepad)"
    check "Mousepad is active" wait_for is_active_class Mousepad
    sleep 0.5
    check "Mousepad fills the work area ($WORKAREA)" [ "$(geom "$mp")" = "$WORKAREA" ]
    check "Mousepad has no title bar" bash -c "xprop -id $mp _NET_WM_STATE | grep -q _OB_WM_STATE_UNDECORATED"

    tap 30 12; wait_for mapped_named Start
    xdotool type --delay 40 thunar; sleep 0.4; xdotool key Return
    check "Thunar launched from search" wait_long has_class Thunar
    check "Thunar is active" wait_for is_active_class Thunar
    shot thunar
    # Thunar publishes no menus: the command bar offers generic ones.
    check "command bar shows generic Edit for Thunar" wait_for found_shell button "^Edit$" --frame "command bar"
    check "status bar stays above applications (bar still at top)" [ "$(geom "$top")" = "0,0,$W,$TOP" ]

    # --- Programs and Settings are shell screens on the desktop surface
    # The drop-down sizes itself to its content when it maps; let it settle.
    tap 30 12; wait_for mapped_named Start; sleep 0.5
    ui tap pda-shell "list item" "^Settings$" --frame "^Start$" >/dev/null
    check "Start > Settings shows the desktop" wait_for showing_desktop 1
    check "Settings shows About" wait_for found_shell button "^About$" --frame "^Today$"
    ui tap pda-shell button "^About$" --frame "^Today$" >/dev/null
    check "About shows the operating system" wait_for found_shell label "^Operating system$" --frame "^Today$"
    shot settings-about
    ui tap pda-shell button "^OK$" --frame "command bar" >/dev/null
    check "OK returns to Settings" wait_for found_shell button "^Screen$" --frame "^Today$"
    tap 30 12; wait_for mapped_named Start; sleep 0.5
    ui tap pda-shell "list item" "^Programs$" --frame "^Start$" >/dev/null
    check "Programs lists Linux tools" wait_for found_shell button "^Linux tools$" --frame "^Today$"
    shot programs
    ui tap pda-shell button "^Today$" --frame "command bar" >/dev/null
    check "Today button returns to the Today screen" wait_for found_shell "list item" "Budget 2026.xlsx" --frame "^Today$"

    # --- switcher
    tap 100 $((H - BOTTOM / 2))
    check "switcher popover opens" wait_for has_popup
    shot switcher
    ui tap pda-shell "list item" "Mousepad" --frame "command bar" >/dev/null
    check "switcher brings Mousepad back" wait_for is_active_class Mousepad
    check "switcher popover closes" wait_for no_popup

    # --- Today
    tap 30 $((H - BOTTOM / 2))
    check "Today button shows the desktop" wait_for showing_desktop 1
    sleep 0.3
    shot today-with-apps

    # --- long press on the first Today favorite (Files)
    local sx sy sw sh
    read -r sx sy sw sh <<< "$(ui find pda-shell button "^Files$" --frame "^Today$")"
    sx=$((sx + sw / 2)); sy=$((sy + sh / 2))
    xdotool mousemove $sx $sy; sleep 0.15; xdotool mousedown 1; sleep 0.4
    shot long-press-progress
    sleep 0.45
    xdotool mouseup 1
    check "long press opens a context menu" wait_for has_popup
    shot long-press-menu
    local launches; launches="$(grep -c 'launched pda-files.desktop' "$LOG_DIR/shell.log")"
    check "long press did not also launch the favorite (Files launches: $launches)" [ "$launches" = 0 ]
    tap $((W - 40)) $((H / 2))
    check "tapping elsewhere closes the context menu" wait_for no_popup

    # --- quick tap still launches (no accidental long press)
    tap 30 12; wait_for mapped_named Start; tap 30 12
    check "Start button toggles the menu closed" wait_for unmapped_named Start

    # --- close the active application
    tap 100 $((H - BOTTOM / 2)); wait_for has_popup
    ui tap pda-shell "list item" "Mousepad" --frame "command bar" >/dev/null
    wait_for is_active_class Mousepad
    ui tap pda-shell button "^Close$" --frame "command bar" >/dev/null
    check "Close in the command bar closes Mousepad" wait_for no_class Mousepad

    # --- end session: closes remaining apps gracefully, then exits
    tap 30 12; wait_for mapped_named Start; sleep 0.5
    ui tap pda-shell "list item" "^End session$" --frame "^Start$" >/dev/null
    check "End session closes Thunar" wait_long no_class Thunar
    local t=100; while kill -0 "$session" 2>/dev/null && [ $t -gt 0 ]; do sleep 0.1; t=$((t - 1)); done
    check "session script exits after End session" bash -c "! kill -0 $session 2>/dev/null"
    wait "$session"; local status=$?
    check "session exited with status 0 (got $status)" [ "$status" -eq 0 ]
    check "no session processes left behind" no_leftovers "pda-$ORIENT-$session-"
    cp "$LOG_DIR/shell.log" "$SHOTS/$ORIENT-shell.log" 2>/dev/null
    rm -rf "$ready" "$SESSION_SHELL_PID" "$home"
}

# [keyboard] show_on_focus: the built-in panel appears when a text field
# gets the focus; hiding it by hand keeps it hidden for that field; the
# keyboard button still works.
focus_session() {
    echo "== keyboard on text focus (landscape, show_on_focus = true)"
    ORIENT=focus; W=1024; H=600
    local home cfg ready; home="$(mktemp -d "${TMPDIR:-/tmp}/pda-smoke-home.XXXXXX")"
    cfg="$home/pda-shell.toml"
    sed 's/^panel = "onboard"/panel = "pocketpc"/; s/^show_on_focus = false/show_on_focus = true/' \
        "$REPO/config/pda-shell.toml" > "$cfg"
    ready="$(mktemp)"; rm -f "$ready"
    PDA_SHELL_CONFIG="$cfg" "$REPO/scripts/dev-session.sh" --orientation landscape --headless --no-build \
        --ready-file "$ready" --home "$home/h" --reset-home > "$SHOTS/focus-session.out" 2>&1 &
    local session=$!
    if ! WAIT=200 wait_for test -s "$ready"; then fail "session became ready"; kill "$session"; wait "$session"; return; fi
    # shellcheck disable=SC1090
    . "$ready"; export DISPLAY
    SESSION_SHELL_PID="$(mktemp)"
    for w in $(xdotool search --classname pda-shell); do xdotool getwindowpid "$w" > "$SESSION_SHELL_PID" 2>/dev/null && break; done
    export DBUS_SESSION_BUS_ADDRESS
    DBUS_SESSION_BUS_ADDRESS="$(tr '\0' '\n' < "/proc/$(cat "$SESSION_SHELL_PID")/environ" | sed -n 's/^DBUS_SESSION_BUS_ADDRESS=//p')"
    sleep 1
    panel_shown() { xdotool search --onlyvisible --name '^Input panel$' >/dev/null 2>&1; }
    panel_hidden() { ! panel_shown; }
    check "no panel before any text field has the focus" panel_hidden
    tap 30 12; wait_for mapped_named Start; sleep 0.5
    xdotool type --delay 40 mousepad; sleep 0.4; xdotool key Return
    wait_long has_class Mousepad
    check "with Mousepad's text field focused the panel is up" wait_for panel_shown
    ui tap pda-shell button "keyboard" --frame "command bar" >/dev/null
    check "the keyboard button hides it" wait_for panel_hidden
    tap 30 $((H - 25)); wait_for showing_desktop 1
    tap 100 $((H - 25)); wait_for has_popup
    ui tap pda-shell "list item" "Mousepad" --frame "command bar" >/dev/null
    wait_for is_active_class Mousepad; sleep 1
    check "back in the same field: stays hidden (hidden by hand)" panel_hidden
    xdotool key ctrl+f; sleep 1.5
    check "another text field (Mousepad's search) brings it up again" wait_for panel_shown
    shot focus-search-field
    xdotool key Escape; sleep 0.3
    ui tap pda-shell button "keyboard" --frame "command bar" >/dev/null; wait_for panel_hidden
    ui tap pda-shell button "keyboard" --frame "command bar" >/dev/null
    check "manual activation still works" wait_for panel_shown
    ui tap pda-shell button "keyboard" --frame "command bar" >/dev/null
    xdotool key ctrl+q; sleep 0.5
    for w in $(xdotool search --name "Save Changes" 2>/dev/null); do xdotool key --window "$w" alt+d; done
    wait_long no_class Mousepad
    tap 30 12; wait_for mapped_named Start; sleep 0.5
    ui tap pda-shell "list item" "^End session$" --frame "^Start$" >/dev/null
    local t=100; while kill -0 "$session" 2>/dev/null && [ $t -gt 0 ]; do sleep 0.1; t=$((t - 1)); done
    kill "$session" 2>/dev/null; wait "$session" 2>/dev/null
    rm -rf "$home" "$ready" "$SESSION_SHELL_PID"
}

for o in "${ORIENTATIONS[@]}"; do run_orientation "$o"; done
focus_session
echo
echo "smoke test: $PASS passed, $FAIL failed (screenshots in $SHOTS)"
[ "$FAIL" -eq 0 ]

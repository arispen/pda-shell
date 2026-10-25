#!/usr/bin/env bash
# Screenshots of the shell's own screens (Today, Start, Programs, Settings,
# About, Screen, keyboard over a program) and of the command bar, native
# programs and built-in input panel (P1/P2) in both orientations, for the
# POC 0.3 before/after comparison (docs/screenshots.md).
#
#   ./scripts/capture-screens.sh [OUT_DIR]      # default screenshots/pocketpc
#   PDA_SHELL_DENSITY=readable ./scripts/capture-screens.sh /tmp/x
set -uo pipefail
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="${1:-$REPO/screenshots/pocketpc}"
mkdir -p "$OUT"
(cd "$REPO" && cargo build) || exit 1
ui() { python3 "$REPO/scripts/lib/pda_ui.py" "$@" >/dev/null 2>&1; }

for orient in portrait landscape; do
    home="$(mktemp -d)"; ready="$(mktemp)"; rm -f "$ready"
    "$REPO/scripts/dev-session.sh" --orientation "$orient" --headless --no-build \
        --home "$home" --reset-home --ready-file "$ready" > /dev/null 2>&1 &
    session=$!
    for _ in $(seq 300); do [ -s "$ready" ] && break; sleep 0.1; done
    # shellcheck disable=SC1090
    . "$ready"; export DISPLAY
    pid="$(xdotool search --classname pda-shell getwindowpid %1 2>/dev/null)"
    export DBUS_SESSION_BUS_ADDRESS
    DBUS_SESSION_BUS_ADDRESS="$(tr '\0' '\n' < "/proc/$pid/environ" | sed -n 's/^DBUS_SESSION_BUS_ADDRESS=//p')"
    sleep 2
    # Park the pointer on the status bar title so no tooltip covers the shot.
    shot() { xdotool mousemove 300 10; sleep 0.6; import -window root "$OUT/shell-$orient-$1.png"; }
    start() { ui tap pda-shell button "^Start$" --frame "status bar" --timeout 5; sleep 0.8; }

    shot 01-today
    start; shot 02-start
    ui tap pda-shell "list item" "^Programs$" --frame "^Start$"; sleep 0.8; shot 03-programs
    ui tap pda-shell button "^Linux tools$" --frame "^Today$"; sleep 0.8; shot 04-linux-tools
    start; ui tap pda-shell "list item" "^Settings$" --frame "^Start$"; sleep 0.8; shot 05-settings
    ui tap pda-shell button "^About$" --frame "^Today$"; sleep 0.8; shot 06-about
    ui tap pda-shell button "^OK$" --frame "command bar"; sleep 0.5
    ui tap pda-shell button "^Screen$" --frame "^Today$"; sleep 0.8; shot 07-screen
    start; ui tap pda-shell "list item" "^Find$" --frame "^Start$"; sleep 0.3
    xdotool type --delay 60 "text"; sleep 0.8; shot 08-start-find
    xdotool key Escape; sleep 0.5
    ui tap pda-shell button "^Today$" --frame "command bar"; sleep 0.5
    # Mousepad is under Programs > Linux tools since POC 0.3 (P2).
    start; ui tap pda-shell "list item" "^Programs$" --frame "^Start$"; sleep 0.8
    ui tap pda-shell button "^Linux tools$" --frame "^Today$"; sleep 0.8
    ui tap pda-shell button "^Mousepad$" --frame "^Today$"; sleep 4
    ui tap pda-shell button "keyboard" --frame "command bar"; sleep 3; shot 09-mousepad-keyboard
    ui tap pda-shell button "keyboard" --frame "command bar"; sleep 1
    ui tap pda-shell button "^Today$" --frame "command bar"; sleep 1; shot 10-today-with-program

    # P1: the command bar. Mousepad publishes its menus; Gnumeric does not.
    bar_menu() { ui tap pda-shell button "$1" --frame "command bar"; sleep 0.8; }
    row() { ui tap pda-shell "list item" "$1" --frame "$2" --timeout 5; sleep 0.8; }
    ui tap pda-shell button "^Windows" --frame "command bar"; sleep 0.8
    ui tap pda-shell "list item" "Mousepad" --frame "command bar"; sleep 1
    bar_menu "^Edit$"; shot 11-mousepad-own-edit-menu
    xdotool key Escape; sleep 0.3
    ui tap pda-shell button "^Today$" --frame "command bar"; sleep 0.5
    bar_menu "^New$"; shot 12-today-new-menu
    row "^Spreadsheet$" "command bar"; sleep 5
    bar_menu "^Edit$"; shot 13-gnumeric-generic-edit-menu
    xdotool key Escape; sleep 0.3
    # P2: Notes with the built-in input panel, Files.
    ui tap pda-shell button "^Today$" --frame "command bar"; sleep 0.5
    bar_menu "^New$"; row "^Note$" "command bar"; sleep 4
    bar_menu "Input panel choice"; shot 14-input-panel-chooser
    row "^Pocket PC keyboard$" "command bar"
    ui tap pda-shell button "keyboard" --frame "command bar"; sleep 1
    for k in CAP s h o p p i n g CAP space l i s t; do
        ui tap pda-shell button "^($k|${k^^})$" --frame "^Input panel$" --timeout 3
    done
    sleep 1; shot 15-notes-builtin-panel
    ui tap pda-shell button "keyboard" --frame "command bar"; sleep 0.5
    ui tap pda-shell button "^Done$" --frame "command bar"; sleep 1; shot 16-notes-list
    bar_menu "Input panel choice"; row "^Full keyboard" "command bar"
    ui tap pda-shell button "^Today$" --frame "command bar"; sleep 0.5
    ui tap pda-shell button "^Files$" --frame "^Today$"; sleep 4; shot 17-files
    bar_menu "^View$"; shot 18-files-view-menu
    xdotool key Escape; sleep 0.3
    ui tap pda-shell button "^Windows" --frame "command bar"; sleep 0.8; shot 19-switcher-native-and-linux

    kill "$session"; wait "$session" 2>/dev/null
    rm -rf "$home" "$ready"
    echo "$orient done"
done

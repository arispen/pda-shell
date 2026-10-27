#!/usr/bin/env bash
# End-to-end test with real applications, driven by the pointer only.
#
# After the session starts, every interaction is a pointer tap (xdotool),
# exactly what a stylus produces: shell buttons, application toolbars and
# menus, and the keys of the on-screen keyboard (Onboard). No synthetic key
# events are sent. AT-SPI is used to *find* widgets and read results, and
# saved files are checked on disk.
#
# The flow (issue #1, Milestone 7): Today -> Thunar (Programs > Linux
# tools since POC 0.3) -> find the XLSX ->
# Gnumeric edit/save -> Mousepad edit/save -> switcher -> back to Gnumeric ->
# Atril PDF -> Firefox page and form -> full screen and back -> Today, then
# reopen both documents, context menus near the edges, the native Notes and
# Files with the built-in input panel (POC 0.3, P2), and End session with
# unsaved work (which must not be discarded silently).
#
#   ./scripts/app-workflow-test.sh                         # both orientations
#   ./scripts/app-workflow-test.sh --orientation landscape
#   ./scripts/app-workflow-test.sh --density readable
#   ./scripts/app-workflow-test.sh --screenshots DIR       # default screenshots/workflow
#   ./scripts/app-workflow-test.sh --no-build
#
# Exit status is non-zero if any check fails. Problems the shell detects
# (dialogs that do not fit, ...) are copied from the shell log into the
# results as "geometry:" lines.
set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ORIENTATIONS=(portrait landscape)
SHOTS="$REPO/screenshots/workflow"
BUILD=1
DENSITY=""
while [ $# -gt 0 ]; do
    case "$1" in
        --orientation) ORIENTATIONS=("${2:?}"); shift 2 ;;
        --screenshots) SHOTS="${2:?}"; shift 2 ;;
        --density) DENSITY="${2:?}"; shift 2 ;;
        --no-build) BUILD=0; shift ;;
        -h|--help) sed -n '2,24p' "$0"; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done
for tool in xdotool xprop xwininfo import ssconvert python3; do
    command -v "$tool" >/dev/null || { echo "missing $tool; run scripts/setup-debian.sh" >&2; exit 1; }
done
[ "$BUILD" -eq 1 ] && (cd "$REPO" && cargo build) || [ "$BUILD" -eq 0 ] || exit 1
mkdir -p "$SHOTS"

PASS=0
FAIL=0
FAILED=()
ok()   { PASS=$((PASS + 1)); printf '  ok    %s\n' "$*"; }
fail() { FAIL=$((FAIL + 1)); FAILED+=("$ORIENT: $*"); printf '  FAIL  %s\n' "$*"; }
check() { local what="$1"; shift; if "$@"; then ok "$what"; else fail "$what"; fi; }
note() { printf '  note  %s\n' "$*"; }

wait_for() { local t="${WAIT:-100}"; while [ "$t" -gt 0 ]; do "$@" && return 0; sleep 0.1; t=$((t - 1)); done; return 1; }
wait_n() { local WAIT="$1"; shift; wait_for "$@"; }
wait_long() { wait_n 300 "$@"; }

# Pointer-only driver (taps, on-screen keyboard) and AT-SPI lookups.
ui() { python3 "$REPO/scripts/lib/pda_ui.py" "$@" 2>/dev/null; }
osk() { python3 "$REPO/scripts/lib/pda_ui.py" osk-type "$1" 2>/dev/null; }
bar() { ui tap pda-shell button "$1" --frame "command bar" --timeout 15 >/dev/null || note "could not find $1 in the command bar"; }
# A row of a menu opened from the command bar.
bar_item() { ui tap pda-shell "list item" "$1" --frame "command bar" --timeout 5 >/dev/null || note "no menu row $1"; }
# A key of the built-in input panel (labels follow Shift).
sip() { local k; for k in "$@"; do ui tap pda-shell button "^($k|${k^^})\$" --frame "^Input panel$" --timeout 3 >/dev/null || note "no key $k"; done; }
tap() { xdotool mousemove "$1" "$2"; sleep 0.1; xdotool click 1; sleep 0.2; }

active() { xprop -root _NET_ACTIVE_WINDOW 2>/dev/null | sed -n 's/.*window id # //p'; }
clients() { xprop -root _NET_CLIENT_LIST 2>/dev/null | sed -n 's/.*window id # //p' | tr -d ','; }
class_of() { xprop -id "$1" WM_CLASS 2>/dev/null | sed -n 's/.*", "\(.*\)"/\1/p'; }
title_of() { xprop -id "$1" _NET_WM_NAME 2>/dev/null | sed -n 's/.* = "\(.*\)"/\1/p'; }
geom() { xwininfo -id "$1" 2>/dev/null | awk '/Absolute upper-left X/{x=$4} /Absolute upper-left Y/{y=$4} /Width:/{w=$2} /Height:/{h=$2} END{print x","y","w","h}'; }
workarea() { xprop -root _NET_WORKAREA | sed 's/.*= //; s/ //g' | cut -d, -f1-4; }
window_of_class() { for w in $(clients); do [ "$(class_of "$w")" = "$1" ] && { echo "$w"; return 0; }; done; return 1; }
has_class() { window_of_class "$1" >/dev/null; }
no_class() { ! window_of_class "$1" >/dev/null; }
is_active_class() { [ "$(class_of "$(active)")" = "$1" ]; }
active_title_matches() { title_of "$(active)" | grep -q "$1"; }
fills_workarea() { local w; w="$(window_of_class "$1")" && [ "$(geom "$w")" = "$(workarea)" ]; }
fullscreen() { xprop -id "$(window_of_class "$1")" _NET_WM_STATE | grep -q FULLSCREEN; }
not_fullscreen() { ! fullscreen "$1"; }
showing_desktop() { [ "$(xprop -root _NET_SHOWING_DESKTOP | sed 's/.*= //')" = "$1" ]; }
found() { ui find "$@" >/dev/null; }
button_on_screen() {
    local x y w h
    read -r x y w h <<< "$(ui find "$1" button "$2" --frame "$3")" || return 1
    [ "$x" -ge 0 ] && [ "$y" -ge "$TOP" ] && [ $((x + w)) -le "$W" ] && [ $((y + h)) -le $((H - BOTTOM)) ]
}
text_has() { ui text "$1" "$2" "$3" | grep -qF -- "$4"; }
# The file name field is the first row of a GTK file chooser at its left
# edge: it is on screen when the dialog's left and top edges are (X
# geometry; GTK 3's AT-SPI extents of the field were not reliable here).
dialog_name_row_on_screen() {
    local w g; w="$(xdotool search --onlyvisible --name "$1" | head -1)" || return 1
    IFS=, read -r dx dy _ _ <<< "$(geom "$w")"
    [ "$dx" -ge 0 ] && [ "$dy" -ge "$TOP" ]
}
dialog_name_row_off_screen() { ! dialog_name_row_on_screen "$@"; }
# The built-in panel lies inside the work area (floating) or the screen.
panel_inside() {
    local g; g="$(geom "$(xdotool search --onlyvisible --name '^Input panel$' | head -1)")"
    IFS=, read -r px py pw ph <<< "$g"
    [ "$px" -ge 0 ] && [ "$py" -ge "$TOP" ] && [ $((px + pw)) -le "$W" ] && [ $((py + ph)) -le $((H - BOTTOM)) ]
}
# Park the pointer on the status bar title (no tooltips there) so screenshots
# show the screen rather than a tooltip under the last tap; see
# docs/known-limitations.md about tooltips after stylus taps.
shot() { xdotool mousemove $((W / 2)) $((TOP / 2)); sleep 0.4; import -window root "$SHOTS/$ORIENT-$1.png"; }
shell_popup() {
    local w
    for w in $(xdotool search --onlyvisible --pid "$SHELL_PID" 2>/dev/null); do
        xwininfo -id "$w" 2>/dev/null | grep -q 'Override Redirect State: yes' || continue
        # The full-screen escape and the input panel are not popups.
        case "$(xprop -id "$w" WM_NAME 2>/dev/null)" in *'"Exit full screen"'|*'"Input panel"'|*'"Dialog arrows"') continue ;; esac
        echo "$w"; return 0
    done
    return 1
}
has_popup() { shell_popup >/dev/null; }
no_popup() { ! shell_popup >/dev/null; }
popup_on_screen() {
    local p; p="$(shell_popup)" || return 1
    IFS=, read -r px py pw ph <<< "$(geom "$p")"
    [ "$px" -ge 0 ] && [ "$py" -ge 0 ] && [ $((px + pw)) -le "$W" ] && [ $((py + ph)) -le "$H" ]
}
# Line N of a spreadsheet exported as CSV (ssconvert cannot write to stdout).
csv_line() {
    local out; out="$(mktemp --suffix .csv)"
    ssconvert --export-type=Gnumeric_stf:stf_csv "$1" "$out" >/dev/null 2>&1
    sed -n "$2p" "$out"; rm -f "$out"
}
csv_cell() { csv_line "$1" "$2" | cut -d, -f"$3" | tr -d '"'; }
no_leftovers() {
    local env
    for env in /proc/[0-9]*/environ; do
        if { tr '\0' '\n' < "$env"; } 2>/dev/null | grep -q "^PDA_SESSION_TAG=$1"; then
            echo "    left over: $(tr '\0' ' ' < "${env%environ}cmdline")"; return 1
        fi
    done
    return 0
}

run_orientation() {
    ORIENT="$1"
    case "$ORIENT" in portrait) W=600; H=1024 ;; landscape) W=1024; H=600 ;; esac
    echo "== $ORIENT ${W}x${H}${DENSITY:+ density=$DENSITY}"
    local home; home="$(mktemp -d "${TMPDIR:-/tmp}/pda-workflow-home.XXXXXX")"
    local docs="$home/Documents"
    local ready; ready="$(mktemp)"; rm -f "$ready"
    # Applications expose their widgets over AT-SPI only when accessibility
    # is requested (Firefox in particular).
    GNOME_ACCESSIBILITY=1 "$REPO/scripts/dev-session.sh" --orientation "$ORIENT" --headless --no-build \
        --home "$home" --reset-home ${DENSITY:+--density "$DENSITY"} --ready-file "$ready" \
        > "$SHOTS/$ORIENT-session.out" 2>&1 &
    local session=$!
    if ! wait_n 300 test -s "$ready"; then
        fail "session became ready"; kill "$session" 2>/dev/null; wait "$session"; return
    fi
    # shellcheck disable=SC1090
    . "$ready"; export DISPLAY
    SHELL_PID="$(xdotool search --classname pda-shell getwindowpid %1 2>/dev/null)"
    export DBUS_SESSION_BUS_ADDRESS
    DBUS_SESSION_BUS_ADDRESS="$(tr '\0' '\n' < "/proc/$SHELL_PID/environ" | sed -n 's/^DBUS_SESSION_BUS_ADDRESS=//p')"
    sleep 1
    IFS=, read -r _ TOP _ WA_H <<< "$(workarea)"
    BOTTOM=$((H - TOP - WA_H))
    local full_wa; full_wa="$(workarea)"

    # --- Today -------------------------------------------------------------
    check "Today lists the sample spreadsheet" wait_for found pda-shell "list item" "^Budget 2026.xlsx$" --frame "^Today$"
    check "Today has favorites (native Files)" found pda-shell button "^Files$" --frame "^Today$"
    shot 01-today

    # --- Thunar: find the spreadsheet ---------------------------------------
    # The Linux file manager is an advanced program now: Programs > Linux tools.
    ui tap pda-shell button "^Start$" --frame "status bar" >/dev/null; sleep 0.8
    ui tap pda-shell "list item" "^Programs$" --frame "^Start$" >/dev/null; sleep 0.8
    ui tap pda-shell button "^Linux tools$" --frame "^Today$" --timeout 5 >/dev/null; sleep 0.8
    ui tap pda-shell button "^Thunar" --frame "^Today$" --timeout 5 >/dev/null
    check "Thunar opens from Programs > Linux tools" wait_long has_class Thunar
    check "Thunar fills the work area" wait_for fills_workarea Thunar
    ui tap thunar "table cell" "^Documents$" --timeout 10 >/dev/null
    check "Documents folder opens with one tap" wait_n 100 found thunar "table cell" "^Budget 2026.xlsx$"
    shot 02-thunar-documents
    # Deeply nested folders and long file names.
    for d in Projects 2026 "Q4 planning" "Client deliverables" "Regional energy cooperative" Drafts; do
        ui tap thunar "table cell" "^$d\$" --timeout 5 >/dev/null; sleep 0.6
    done
    check "long file name reachable six folders deep" wait_n 50 found thunar "table cell" "^Quarterly report"
    shot 03-thunar-long-names
    ui tap thunar button "^Home$" >/dev/null; sleep 0.6
    ui tap thunar "table cell" "^Documents$" --timeout 5 >/dev/null; sleep 0.6

    # --- Gnumeric: edit the populated spreadsheet with the on-screen keyboard
    ui tap thunar "table cell" "^Budget 2026.xlsx$" --timeout 5 >/dev/null
    check "tapping the XLSX opens Gnumeric (file association)" wait_long has_class Gnumeric
    check "Gnumeric fills the work area" wait_for fills_workarea Gnumeric
    shot 04-gnumeric
    bar "keyboard"
    check "keyboard appears" wait_n 100 found onboard frame "^Onboard$"
    sleep 1
    local kb_wa; kb_wa="$(workarea)"
    check "work area shrinks above the keyboard ($full_wa -> $kb_wa)" [ "$kb_wa" != "$full_wa" ]
    check "Gnumeric resized to the smaller work area" wait_for fills_workarea Gnumeric
    check "focus stays in Gnumeric after showing the keyboard" is_active_class Gnumeric
    # Cell navigation through the Name box (tap it, Ctrl+A to replace the
    # current cell name), then numbers, a formula and text.
    ui tap gnumeric text "" --frame "Gnumeric" --pick leftmost >/dev/null
    osk "{Ctrl}aB2{Return}4321.5{Return}"
    ui tap gnumeric text "" --frame "Gnumeric" --pick leftmost >/dev/null
    osk "{Ctrl}aA18{Return}=SUM(B2:B13)*2{Return}Checked on the PDA, 22 Oct.{Return}"
    # Esc abandons an entry; arrow keys move between cells (now A20 -> B18).
    osk "discard me{Esc}{Up}{Up}{Right}5{Return}"
    shot 05-gnumeric-keyboard
    # Gnumeric publishes no menus: the command bar offers generic commands,
    # typed as shortcuts (File > Save is Ctrl+S).
    check "command bar shows generic menus for Gnumeric" found pda-shell button "^Edit$" --frame "command bar"
    bar "^File$"; sleep 0.5
    bar_item "^Save$"
    check "Gnumeric saved from the command bar (title without *)" wait_for active_title_matches "^Budget 2026.xlsx"
    sleep 1
    check "saved file: B2 = 4321.5" [ "$(csv_cell "$docs/Budget 2026.xlsx" 2 2)" = "4321.5" ]
    check "saved file: formula =SUM(B2:B13)*2 = 104643" [ "$(csv_cell "$docs/Budget 2026.xlsx" 18 1)" = "104643" ]
    check "saved file: Esc discarded the entry in A20" [ -z "$(csv_line "$docs/Budget 2026.xlsx" 20 | tr -d ',')" ]
    check "saved file: arrow keys reached B18 = 5" [ "$(csv_cell "$docs/Budget 2026.xlsx" 18 2)" = "5" ]
    check "saved file: text typed with punctuation (A19)" [ "$(csv_line "$docs/Budget 2026.xlsx" 19 | cut -d, -f1-2)" = '"Checked on the PDA, 22 Oct."' ]
    # Save As: a GTK file chooser, the largest dialog of the session.
    ui tap gnumeric menu "^File$" >/dev/null; sleep 0.5
    ui tap gnumeric "menu item" "^Save As" --timeout 3 >/dev/null
    local dlg="^Save the current workbook as"
    check "Save As dialog opens" wait_n 50 found gnumeric button "^Cancel$" --frame "$dlg"
    sleep 1
    check "Save As: Save button fully on screen" button_on_screen gnumeric "^Save$" "$dlg"
    check "Save As: Cancel button fully on screen" button_on_screen gnumeric "^Cancel$" "$dlg"
    shot 05b-save-as-dialog
    # Too wide (portrait) or, with the docked keyboard, too tall (landscape):
    # the shell offers arrows to slide the dialog to its cut-off part.
    if [ "$ORIENT" = portrait ]; then local show=left back=right; else local show=top back=bottom; fi
    check "Save As: the file name field is cut off at first" dialog_name_row_off_screen "$dlg"
    check "Save As: arrows offered for the oversized dialog" wait_n 30 found pda-shell button "^Show the $show" --frame "^Dialog arrows$"
    ui tap pda-shell button "^Show the $show" --frame "^Dialog arrows$" >/dev/null
    check "Save As: '$show' arrow brings the file name field on screen" wait_n 30 dialog_name_row_on_screen "$dlg"
    shot 05c-save-as-name-field
    ui tap pda-shell button "^Show the $back" --frame "^Dialog arrows$" >/dev/null
    check "Save As: '$back' arrow brings Save back" wait_n 30 button_on_screen gnumeric "^Save$" "$dlg"
    ui tap gnumeric button "^Cancel$" --frame "$dlg" >/dev/null
    check "Save As dialog closes" wait_n 50 bash -c "! python3 '$REPO/scripts/lib/pda_ui.py' find gnumeric button '^Cancel$' >/dev/null 2>&1"
    check "the arrows go away with the dialog" wait_n 30 bash -c "! xdotool search --onlyvisible --name '^Dialog arrows$' >/dev/null 2>&1"
    bar "keyboard"
    check "keyboard hides and the work area is restored" wait_for bash -c "[ \"\$(xprop -root _NET_WORKAREA | sed 's/.*= //; s/ //g' | cut -d, -f1-4)\" = '$full_wa' ]"
    check "focus stays in Gnumeric after hiding the keyboard" is_active_class Gnumeric

    # --- Mousepad via the switcher and Thunar --------------------------------
    bar "^Windows"
    check "switcher opens" wait_for has_popup
    shot 06-switcher
    ui tap pda-shell "list item" "Thunar" --frame "command bar" >/dev/null
    check "switcher brings Thunar back" wait_for is_active_class Thunar
    ui tap thunar "table cell" "^notes.txt$" --timeout 5 >/dev/null
    check "tapping notes.txt opens Mousepad (file association)" wait_long has_class Mousepad
    check "Mousepad fills the work area" wait_for fills_workarea Mousepad
    sleep 0.5
    # Tap below the last line: the cursor goes to the end of the text.
    local mp; mp="$(window_of_class Mousepad)"
    IFS=, read -r mx my mw mh <<< "$(geom "$mp")"
    tap $((mx + mw / 2)) $((my + mh - 40))
    bar "keyboard"
    wait_for found onboard frame "^Onboard$"; sleep 1
    check "Mousepad resized above the keyboard" wait_for fills_workarea Mousepad
    osk "{Return}Edited on the PDA: 3 * 4 = 12 (ok); path ~/Documents/notes.txt"
    shot 07-mousepad-keyboard
    # Mousepad publishes its menu bar (GtkApplication): the command bar shows
    # Mousepad's own menus, and Save runs Mousepad's own action over D-Bus.
    # Mousepad has six menus; what does not fit (portrait: 3 places) is under More.
    if [ "$ORIENT" = portrait ]; then
        check "command bar shows Mousepad's own menus (File ... More)" found pda-shell button "^More$" --frame "command bar"
    else
        check "command bar shows Mousepad's own Search menu" found pda-shell button "^Search$" --frame "command bar"
    fi
    bar "^File$"; sleep 0.8
    bar_item "^Save$"
    check "Mousepad saved through its published menu (title without *)" wait_for active_title_matches "^~/Documents/notes.txt"
    check "saved file contains the typed line" grep -qF "Edited on the PDA: 3 * 4 = 12 (ok); path ~/Documents/notes.txt" "$docs/notes.txt"
    bar "keyboard"
    sleep 0.5

    # --- back to Gnumeric through the switcher -------------------------------
    bar "^Windows"; wait_for has_popup
    ui tap pda-shell "list item" "Budget 2026.xlsx" --frame "command bar" >/dev/null
    check "switcher returns to Gnumeric" wait_for is_active_class Gnumeric
    # Close all is not one tap away: the first tap only arms it.
    bar "^Windows"; wait_for has_popup
    ui tap pda-shell "list item" "^Close all programs$" --frame "command bar" >/dev/null
    check "Close all asks for a second tap" wait_for found pda-shell label "^Tap again to close all" --frame "command bar"
    shot 08-switcher-close-all-armed
    check "nothing was closed by the first tap" has_class Gnumeric
    tap $((W / 2)) $((TOP + 200))
    wait_for no_popup

    # --- Atril with the multi-page PDF ---------------------------------------
    bar "^Windows"; wait_for has_popup
    ui tap pda-shell "list item" "Thunar" --frame "command bar" >/dev/null
    wait_for is_active_class Thunar
    ui tap thunar "table cell" "^Field guide.pdf$" --timeout 5 >/dev/null
    check "tapping the PDF opens Atril" wait_long has_class Atril
    check "Atril shows page 1 of 4" wait_n 100 found atril label "1 of 4"
    ui tap atril button "^Next$" >/dev/null
    check "Next goes to page 2 of 4" wait_n 50 found atril label "2 of 4"
    shot 09-atril

    # --- Firefox with the local page -----------------------------------------
    bar "^Windows"; wait_for has_popup
    ui tap pda-shell "list item" "Thunar" --frame "command bar" >/dev/null
    wait_for is_active_class Thunar
    ui tap thunar "table cell" "^Web$" --timeout 5 >/dev/null; sleep 0.6
    ui tap thunar "table cell" "^index.html$" --timeout 5 >/dev/null
    check "tapping index.html opens Firefox" wait_n 400 active_title_matches "^Field station"
    check "Firefox fills the work area" wait_for fills_workarea firefox-esr
    ui wait firefox heading "^Field station$" --timeout 20 >/dev/null
    # With program text at device scale the form is below the fold in
    # landscape (and an off-screen field is not found at all). Scroll it into
    # view with the wheel: a stand-in for a stylus scroll, which needs
    # system-wide stylus input (Milestone 4).
    local fx fy fw fh n=0
    read -r fx fy fw fh <<< "$(ui find firefox entry '^Observer$')"
    while { [ -z "$fy" ] || [ $((fy + fh)) -gt $((H - BOTTOM)) ]; } && [ "$n" -lt 15 ]; do
        xdotool mousemove $((W / 2)) $((H / 2)) click 5; sleep 0.3; n=$((n + 1))
        read -r fx fy fw fh <<< "$(ui find firefox entry '^Observer$')"
    done
    [ "$n" -gt 0 ] && note "scrolled the page $n wheel steps to reach the form"
    ui tap firefox entry "^Observer$" >/dev/null
    bar "keyboard"
    wait_for found onboard frame "^Onboard$"; sleep 1
    osk "Ana{Tab}Fog at 6:00, 7.4 C."
    check "text typed into the web form" wait_n 30 text_has firefox entry "^Observation$" "Fog at 6:00, 7.4 C."
    shot 10-firefox-form
    # F11 from the keyboard's Fn layer: the program covers both bars.
    osk "{F11}"
    check "F11 puts Firefox in full screen" wait_n 50 fullscreen firefox-esr
    check "Exit full screen button is offered" wait_n 30 found pda-shell button "Exit full screen"
    shot 11-fullscreen-escape
    ui tap pda-shell button "Exit full screen" >/dev/null
    check "Exit full screen restores the bars" wait_n 50 not_fullscreen firefox-esr
    check "Firefox back between the bars" wait_for fills_workarea firefox-esr
    bar "keyboard"
    sleep 0.5

    # --- Today, then reopen both documents -----------------------------------
    bar "^Today$"
    check "Today button shows the desktop" wait_for showing_desktop 1
    check "Today lists open programs" wait_for found pda-shell "list item" "Mousepad" --frame "^Today$"
    shot 12-today-with-apps
    bar "^Windows"; wait_for has_popup
    ui tap pda-shell "list item" "Budget 2026.xlsx" --frame "command bar" >/dev/null
    wait_for is_active_class Gnumeric
    bar "^Close$"
    check "Close in the command bar closes saved Gnumeric without asking" wait_long no_class Gnumeric
    bar "^Today$"; wait_for showing_desktop 1
    ui tap pda-shell "list item" "^Budget 2026.xlsx$" --frame "^Today$" --timeout 5 >/dev/null
    check "reopened from Today's recent documents" wait_long has_class Gnumeric
    check "reopened workbook is the saved file (no unsaved marker)" wait_n 100 found gnumeric frame "^Budget 2026.xlsx"
    shot 13-gnumeric-reopened
    bar "^Close$"; wait_long no_class Gnumeric
    bar "^Windows"; wait_for has_popup
    ui tap pda-shell "list item" "notes.txt" --frame "command bar" >/dev/null
    wait_for is_active_class Mousepad
    bar "^Close$"
    check "saved Mousepad closes without asking" wait_long no_class Mousepad
    bar "^Today$"; wait_for showing_desktop 1
    ui tap pda-shell "list item" "^notes.txt$" --frame "^Today$" --timeout 5 >/dev/null
    check "notes.txt reopened from Today" wait_long has_class Mousepad
    check "reopened text contains the edit" wait_n 50 text_has mousepad text "" "3 * 4 = 12 (ok)"

    # --- context menus near the screen edges ----------------------------------
    bar "^Today$"; wait_for showing_desktop 1
    local pos x y w h
    for target in "list item:^notes.txt$" "button:^Xfce Terminal$" "button:^Files$"; do
        pos="$(ui find pda-shell "${target%%:*}" "${target#*:}" --frame "^Today$")" || { fail "find $target"; continue; }
        read -r x y w h <<< "$pos"
        # Press near the element's outer edge (left / right side). A fifth
        # of the way in: GTK 4 reports AT-SPI positions without the CSS
        # padding of enclosing boxes (~16 px on Today), so "6 px inside" can
        # miss the element.
        case "$target" in *Terminal*) x=$((x + w - w / 5)) ;; *) x=$((x + w / 5)) ;; esac
        ui long-press "$x" $((y + h / 2)) 900
        check "long press near ($x,$((y + h / 2))) opens a context menu" wait_for has_popup
        check "context menu stays on screen" popup_on_screen
        [ "$target" = "button:^Xfce Terminal$" ] && shot 14-context-menu-edge
        tap $((W / 2)) $((TOP / 2)); wait_for no_popup
    done

    # --- native Notes and Files, built-in input panel (POC 0.3, P2) ----------
    bar "^Today$"; wait_for showing_desktop 1
    bar "^New$"; sleep 0.5
    bar_item "^Note$"
    check "New > Note opens Notes" wait_long has_class pda-notes
    check "Notes fills the work area" wait_for fills_workarea pda-notes
    check "command bar shows Notes' own menus (Done)" wait_n 50 found pda-shell button "^Done$" --frame "command bar"
    bar "Input panel choice"; sleep 0.5
    bar_item "^Pocket PC keyboard$"
    bar "keyboard"
    check "built-in input panel appears" wait_n 50 found pda-shell button "^space$" --frame "^Input panel$"
    sleep 0.5
    local kb_wa2; kb_wa2="$(workarea)"
    if [ "$ORIENT" = portrait ]; then
        check "portrait: the built-in panel is docked, the work area shrinks" [ "$kb_wa2" != "$full_wa" ]
        check "Notes resized above the built-in panel" wait_for fills_workarea pda-notes
    else
        check "landscape: the built-in panel floats, programs keep the work area ($kb_wa2)" [ "$kb_wa2" = "$full_wa" ]
        check "landscape: the floating panel lies inside the work area" panel_inside
        check "Notes keeps the whole work area" wait_for fills_workarea pda-notes
    fi
    sip CAP p d a CAP space n o t e Shift '1|!'
    check "focus stayed in Notes while typing" is_active_class pda-notes
    shot 16-notes-builtin-panel
    bar "^Edit$"; sleep 0.5
    shot 17-notes-edit-menu
    tap $((W / 2)) $((TOP + 100)); wait_for no_popup
    bar "keyboard"
    bar "^Done$"
    check "Done shows the list of notes" wait_n 50 found pda-notes "list item" "^PDA note!$"
    check "note written to ~/Documents/Notes" bash -c "grep -qx 'PDA note!' '$docs'/Notes/*.txt"
    # --- spreadsheet keys: the acceptance formula, built-in panel only ------
    bar "^Today$"; wait_for showing_desktop 1
    ui tap pda-shell "list item" "^Budget 2026.xlsx$" --frame "^Today$" --timeout 5 >/dev/null
    check "Gnumeric reopened for the spreadsheet keys" wait_long has_class Gnumeric
    sleep 1
    bar "keyboard"
    wait_n 50 found pda-shell button "^space$" --frame "^Input panel$"
    sip 123
    check "123 switches to the spreadsheet keys" wait_n 30 found pda-shell button "^SUM\($" --frame "^Input panel$"
    # Name box: tap at its right end, clear it, type the cell.
    local nx ny nw nh
    read -r nx ny nw nh <<< "$(ui find gnumeric text "" --frame "Gnumeric" --pick leftmost)"
    tap $((nx + nw - 12)) $((ny + nh / 2))
    sip ⌫ ⌫ ⌫ ⌫ ⌫ B 2 0 ↵
    sip = 'SUM\(' B 2 : B 1 3 '\)' '\*' 2 ↵
    if [ "$ORIENT" = landscape ]; then
        check "landscape: Gnumeric keeps the whole work area with the keys shown" fills_workarea Gnumeric
        check "landscape: the spreadsheet keys float inside the work area" panel_inside
    else
        check "portrait: Gnumeric resized above the docked keys" fills_workarea Gnumeric
    fi
    check "focus stayed in Gnumeric while typing" is_active_class Gnumeric
    shot 19-gnumeric-spreadsheet-keys
    bar "^File$"; sleep 0.5
    bar_item "^Save$"
    sleep 2
    check "saved file: =SUM(B2:B13)*2 typed with the spreadsheet keys (B20 = 104643)" [ "$(csv_cell "$docs/Budget 2026.xlsx" 20 2)" = "104643" ]
    sip abc
    check "abc returns to the full keyboard" wait_n 30 found pda-shell button "^space$" --frame "^Input panel$"
    bar "keyboard"
    bar "^Close$"
    check "Gnumeric closes without asking (saved)" wait_long no_class Gnumeric

    bar "Input panel choice"; sleep 0.5
    bar_item "^Full keyboard"
    bar "^Today$"; wait_for showing_desktop 1
    ui tap pda-shell button "^Files$" --frame "^Today$" >/dev/null
    check "Files opens from Today" wait_long has_class pda-files
    check "command bar shows Files' menus" wait_n 50 found pda-shell button "^Go$" --frame "command bar"
    ui tap pda-files "list item" "^Notes$" --timeout 5 >/dev/null
    check "Files shows the note in Documents/Notes" wait_n 50 found pda-files "list item" "^Note .*\.txt$"
    shot 18-files-notes-folder
    ui tap pda-files button "^Up$" >/dev/null
    check "Up returns to Documents" wait_n 50 found pda-files "list item" "^Budget 2026.xlsx$"
    bar "^Windows"; wait_for has_popup
    check "native and Linux programs share the switcher" found pda-shell "list item" "Notes" --frame "command bar"
    ui tap pda-shell "list item" "Notes" --frame "command bar" >/dev/null
    check "switcher brings Notes back" wait_for is_active_class pda-notes

    # --- unsaved work is never discarded silently ------------------------------
    bar "^Windows"; wait_for has_popup
    ui tap pda-shell "list item" "notes.txt" --frame "command bar" >/dev/null
    wait_for is_active_class Mousepad
    bar "keyboard"; wait_for found onboard frame "^Onboard$"; sleep 1
    osk "UNSAVED"
    bar "keyboard"
    check "Mousepad has unsaved changes" wait_for active_title_matches '^\*'
    ui tap pda-shell button "^Start$" --frame "status bar" >/dev/null
    sleep 0.5
    ui tap pda-shell "list item" "^End session$" --frame "^Start$" >/dev/null
    check "Mousepad asks before discarding unsaved changes" wait_n 100 found mousepad button "^Don't Save$"
    check "Notes and Files closed without asking (Notes saves by itself)" wait_n 100 bash -c "! xdotool search --classname 'pda-(notes|files)' >/dev/null 2>&1"
    check "session is still running while the question is open" kill -0 "$session"
    shot 15-end-session-unsaved
    ui tap mousepad button "^Don't Save$" >/dev/null
    check "unsaved text was not written" bash -c "! grep -q UNSAVED '$docs/notes.txt'"
    local t=300; while kill -0 "$session" 2>/dev/null && [ $t -gt 0 ]; do sleep 0.1; t=$((t - 1)); done
    check "End session finishes once every program has closed" bash -c "! kill -0 $session 2>/dev/null"
    wait "$session"; local status=$?
    check "session exited with status 0 (got $status)" [ "$status" -eq 0 ]
    check "no session processes left behind" no_leftovers "pda-$ORIENT-$session-"

    grep -h "geometry:" "$LOG_DIR/shell.log" | sed 's/^.*geometry: /  geometry: /' | sort -u
    cp "$LOG_DIR/shell.log" "$SHOTS/$ORIENT-shell.log" 2>/dev/null
    rm -rf "$home" "$ready"
}

for o in "${ORIENTATIONS[@]}"; do run_orientation "$o"; done
echo
echo "workflow test: $PASS passed, $FAIL failed (screenshots in $SHOTS)"
for f in "${FAILED[@]}"; do echo "  failed: $f"; done
[ "$FAIL" -eq 0 ]

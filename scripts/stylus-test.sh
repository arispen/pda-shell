#!/usr/bin/env bash
# Kernel-level test of the stylus daemon (pda-device stylus): a fake
# touchscreen made with uinput (scripts/lib/fake_input.py) plays stylus
# gestures, the real daemon grabs it and drives its virtual pointer, and the
# test reads what that pointer emits. Real evdev, EVIOCGRAB and uinput; no
# panel, no X server. Physical-pen results are recorded separately
# (docs/device-checklist.md).
#
#   ./scripts/container.sh uinput ./scripts/stylus-test.sh    # on macOS / in the container
#   sudo ./scripts/stylus-test.sh                             # on a Linux machine or the board
#
# Needs write access to /dev/uinput and read access to /dev/input (root).
# Build first (cargo build). Exits 77 (skipped) without /dev/uinput.
set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${PDA_BIN_DIR:-${CARGO_TARGET_DIR:-$REPO/target}/debug}"
[ -x "$BIN/pda-device" ] || { echo "build first: $BIN/pda-device missing" >&2; exit 1; }
if [ ! -w /dev/uinput ]; then
    echo "skipped: no write access to /dev/uinput (run as root, or ./scripts/container.sh uinput $0)" >&2
    exit 77
fi

PASS=0
FAIL=0
ok()   { PASS=$((PASS + 1)); printf '  ok    %s\n' "$*"; }
fail() { FAIL=$((FAIL + 1)); printf '  FAIL  %s\n' "$*"; }
check() { local what="$1"; shift; if "$@"; then ok "$what"; else fail "$what"; fi; }
note() { printf '  note  %s\n' "$*"; }

WORK="$(mktemp -d)"
FAKE=(python3 -I "$REPO/scripts/lib/fake_input.py")
TS="pda-test touchscreen"
VIRTUAL="pda-shell stylus"
# No per-user calibration; the shipped thresholds (600 ms hold, 8 px slop).
export HOME="$WORK" XDG_CONFIG_HOME="$WORK/.config" PDA_SHELL_CONFIG="$REPO/config/pda-shell.toml"
DAEMON=""
cleanup() { [ -n "$DAEMON" ] && kill "$DAEMON" 2>/dev/null; wait 2>/dev/null; rm -rf "$WORK"; }
trap cleanup EXIT

start_daemon() {
    RUST_LOG=info "$BIN/pda-device" stylus --device "$TS" --screen 1024x600 "$@" >> "$WORK/daemon.log" 2>&1 &
    DAEMON=$!
    for _ in $(seq 50); do grep -q "virtual pointer" "$WORK/daemon.log" && return 0; sleep 0.1; done
    return 1
}

# scenario NAME SCRIPT: play SCRIPT on a new fake touchscreen (which the
# daemon has to find and grab first) while recording the virtual pointer.
# Sets BUTTONS ("L1 L0 R1 R0" ...), XS (ABS_X values) and RAWSEEN (events
# another reader of the touchscreen got).
scenario() {
    local name="$1" script="$2"
    printf 'wait 1600\n%s\nwait 400\n' "$script" > "$WORK/$name.play"
    "${FAKE[@]}" record "$VIRTUAL" 8 > "$WORK/$name.out" 2>/dev/null &
    local rec=$!
    "${FAKE[@]}" touchscreen "$TS" "$WORK/$name.play" &
    local fake=$!
    # Another reader of the touchscreen: must see nothing while it is grabbed.
    "${FAKE[@]}" record "$TS" 4 > "$WORK/$name.raw" 2>/dev/null &
    local raw=$!
    wait "$fake"; sleep 0.3
    kill "$rec" "$raw" 2>/dev/null; wait "$rec" "$raw" 2>/dev/null
    BUTTONS="$(awk '$2 == 1 && $3 == 272 { printf "L%s ", $4 } $2 == 1 && $3 == 273 { printf "R%s ", $4 }' "$WORK/$name.out")"
    BUTTONS="${BUTTONS% }"
    XS="$(awk '$2 == 3 && $3 == 0 { printf "%s ", $4 }' "$WORK/$name.out")"
    RAWSEEN="$(grep -vc '^#' "$WORK/$name.raw")"
}
# Virtual units (0..32767) for a screen pixel on the 1024-wide screen.
vx() { echo $(( $1 * 32767 / 1023 )); }
near() { [ "$1" -ge $(( $2 - 160 )) ] && [ "$1" -le $(( $2 + 160 )) ]; }
last() { echo "${1% }" | awk '{ print $NF }'; }
first() { echo "$1" | awk '{ print $1 }'; }

echo "== stylus daemon against a uinput touchscreen"
check "daemon creates its virtual pointer" start_daemon
grep -q "waiting for the touchscreen" "$WORK/daemon.log" && note "daemon started before the touchscreen and waits for it"

scenario tap "down 100 200
wait 80
up"
check "tap: one left click (got: $BUTTONS)" [ "$BUTTONS" = "L1 L0" ]
check "tap: at the tapped position (x $(last "$XS") ~ $(vx 100))" near "$(last "$XS")" "$(vx 100)"
check "the grabbed touchscreen sends nothing to other readers ($RAWSEEN events)" [ "$RAWSEEN" -eq 0 ]
check "daemon logged the touchscreen it translates" grep -q "translating .*($TS, 0eef:0001)" "$WORK/daemon.log"

scenario hold "down 300 300
wait 900
up"
check "hold: a right click and no left click (got: $BUTTONS)" [ "$BUTTONS" = "R1 R0" ]

scenario jitter "down 300 300
wait 150
move 304 302
wait 150
move 298 299
wait 150
move 302 297
wait 400
up"
check "hold with jitter inside the slop is still a right click (got: $BUTTONS)" [ "$BUTTONS" = "R1 R0" ]

scenario quick-jitter "down 500 300
wait 40
move 505 303
wait 40
up"
check "short tap with jitter is a left click (got: $BUTTONS)" [ "$BUTTONS" = "L1 L0" ]

scenario drag "down 100 100
wait 60
move 120 100
wait 30
move 160 100
wait 30
move 200 120
wait 60
up"
check "drag: left button down, moves, up (got: $BUTTONS)" [ "$BUTTONS" = "L1 L0" ]
check "drag: pressed where the stylus went down ($(first "$XS") ~ $(vx 100))" near "$(first "$XS")" "$(vx 100)"
check "drag: ends where the stylus left ($(last "$XS") ~ $(vx 200))" near "$(last "$XS")" "$(vx 200)"

scenario drag-then-still "down 100 100
wait 40
move 140 100
wait 1500
up"
check "a drag held still never becomes a right click (got: $BUTTONS)" [ "$BUTTONS" = "L1 L0" ]

scenario double "down 400 400
wait 60
up
wait 120
down 401 401
wait 60
up"
check "double tap: two left clicks for the X server to pair (got: $BUTTONS)" [ "$BUTTONS" = "L1 L0 L1 L0" ]

scenario unplug "down 100 100
wait 40
move 160 100
wait 100
remove
wait 600"
check "unplugged during a drag: the left button is released (got: $BUTTONS)" [ "$BUTTONS" = "L1 L0" ]
check "unplugging is logged" grep -q "gone" "$WORK/daemon.log"

scenario replug "down 600 300
wait 70
up"
check "after replugging, taps work again (got: $BUTTONS)" [ "$BUTTONS" = "L1 L0" ]

echo "== devices the daemon must leave alone"
printf 'wait 2500\nrel 5 5\nclick\nwait 500\n' > "$WORK/mouse.play"
"${FAKE[@]}" record "pda-test mouse" 5 > "$WORK/mouse.raw" 2>/dev/null &
rec=$!
"${FAKE[@]}" mouse "pda-test mouse" "$WORK/mouse.play" &
mouse=$!
sleep 1.2
"$BIN/pda-device" inputs > "$WORK/inputs.txt" 2>&1
wait "$mouse"; kill "$rec" 2>/dev/null; wait "$rec" 2>/dev/null
check "a mouse is not grabbed: its events reach other readers" [ "$(grep -vc '^#' "$WORK/mouse.raw")" -gt 0 ]
check "inputs: the mouse is not taken" grep -q "pda-test mouse.*not taken" "$WORK/inputs.txt"
check "inputs: the daemon's own pointer is never taken" grep -q "$VIRTUAL.*own virtual pointer" "$WORK/inputs.txt"
sed 's/^/        /' "$WORK/inputs.txt"

echo "== stopping"
kill -TERM "$DAEMON"
for _ in $(seq 30); do kill -0 "$DAEMON" 2>/dev/null || break; sleep 0.1; done
check "daemon exits on SIGTERM" bash -c "! kill -0 $DAEMON 2>/dev/null"
wait "$DAEMON"; status=$?; DAEMON=""
check "daemon exit status 0 (got $status)" [ "$status" -eq 0 ]
check "the virtual pointer is gone" bash -c "! grep -q 'Name=\"$VIRTUAL\"' /proc/bus/input/devices"

scenario after-stop "down 100 200
wait 80
up" 2>/dev/null
check "without the daemon, the touchscreen reaches other readers again ($RAWSEEN events)" [ "$RAWSEEN" -gt 0 ]

echo "== right_click = false"
sed 's/^right_click = true/right_click = false/' "$REPO/config/pda-shell.toml" > "$WORK/no-right.toml"
PDA_SHELL_CONFIG="$WORK/no-right.toml" start_daemon
scenario passthrough "down 300 300
wait 900
up"
check "with right_click off, a hold is a long left press (got: $BUTTONS)" [ "$BUTTONS" = "L1 L0" ]

cp "$WORK/daemon.log" "${TMPDIR:-/tmp}/stylus-daemon.log" 2>/dev/null
echo
echo "stylus test: $PASS passed, $FAIL failed (daemon log: ${TMPDIR:-/tmp}/stylus-daemon.log)"
[ "$FAIL" -eq 0 ]

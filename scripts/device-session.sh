#!/usr/bin/env bash
# Start the PDA session directly on the device's own display: X server,
# D-Bus, Openbox and pda-shell, without Xephyr, Xvfb or a desktop
# environment. For development inside a desktop use dev-session.sh.
#
# Run it from a console login on the device, or let the systemd unit
# (config/device/pda-shell-session.service, installed by
# scripts/install-device.sh) start it on boot.
#
#   ./scripts/device-session.sh                          # [device] settings
#   ./scripts/device-session.sh --orientation portrait   # 600x1024 on a 1024x600 panel
#   ./scripts/device-session.sh --mode 800x480           # another panel mode
#
# Options:
#   --orientation landscape|portrait|native   override [device] orientation
#   --mode WxH          override [device] mode
#   --output NAME       override [device] output (e.g. HDMI-1)
#   --vt N              virtual terminal for the X server (default: $XDG_VTNR,
#                       else the current console)
#   --display :N        X display (default :0)
#   --ready-file PATH   write DISPLAY, LOG_DIR and HOME to PATH once the shell
#                       is up
#   --seed-samples      copy the sample documents into ~/Documents (the
#                       acceptance scenario); existing files are kept
#   --xserver Xvfb --screen WxH
#                       automated tests only: a virtual screen instead of the
#                       panel. Never needed on the device.
#
# Leaving: Start > End session closes every program the normal way (each may
# ask about unsaved work) and ends the session with status 0. If pda-shell
# crashes it is restarted (programs keep running and reappear in the
# switcher); after 5 crashes within 60 s the session ends with status 1.
# The stylus daemon (hold to right-click, pda-device stylus) is restarted the
# same way; if it keeps failing the X server keeps the plain touchscreen.
# Stopping the script (SIGTERM, Ctrl+C on the console) ends the X server and
# every program of the session. Logs: ~/.local/state/pda-shell/logs/latest/.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
T0="$(date +%s.%N)"
ORIENTATION=""
MODE=""
OUTPUT_NAME=""
VT="${XDG_VTNR:-}"
DISPLAY_NUM=0
READY_FILE=""
SEED_SAMPLES=0
XSERVER="Xorg"
SCREEN="1024x600"
MAX_CRASHES=5
CRASH_WINDOW=60

usage() { sed -n '2,33p' "$0"; }

while [ $# -gt 0 ]; do
    case "$1" in
        --orientation) ORIENTATION="${2:?}"; shift 2 ;;
        --mode) MODE="${2:?}"; shift 2 ;;
        --output) OUTPUT_NAME="${2:?}"; shift 2 ;;
        --vt) VT="${2:?}"; shift 2 ;;
        --display) DISPLAY_NUM="${2:?}"; DISPLAY_NUM="${DISPLAY_NUM#:}"; shift 2 ;;
        --ready-file) READY_FILE="${2:?}"; shift 2 ;;
        --seed-samples) SEED_SAMPLES=1; shift ;;
        --xserver) XSERVER="${2:?}"; shift 2 ;;
        --screen) SCREEN="${2:?}"; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
    esac
done
case "$ORIENTATION" in ""|landscape|portrait|native) ;; *) echo "orientation must be landscape, portrait or native" >&2; exit 2 ;; esac
case "$XSERVER" in Xorg|Xvfb) ;; *) echo "--xserver must be Xorg or Xvfb" >&2; exit 2 ;; esac

for tool in "$XSERVER" openbox xdpyinfo xprop xrandr dbus-daemon; do
    command -v "$tool" >/dev/null || { echo "missing $tool; run scripts/setup-debian.sh --device" >&2; exit 1; }
done

# --- programs and configuration ----------------------------------------------
# Installed layout (scripts/install-device.sh): $ROOT/bin. A checkout: the
# release build, else the debug build.
BIN_DIR="${PDA_BIN_DIR:-}"
if [ -z "$BIN_DIR" ]; then
    for d in "$ROOT/bin" "${CARGO_TARGET_DIR:-$ROOT/target}/release" "${CARGO_TARGET_DIR:-$ROOT/target}/debug"; do
        if [ -x "$d/pda-shell" ] && [ -x "$d/pda-device" ]; then BIN_DIR="$d"; break; fi
    done
fi
[ -n "$BIN_DIR" ] || { echo "pda-shell and pda-device not found; build with: cargo build --release" >&2; exit 1; }
SHELL_BIN="$BIN_DIR/pda-shell"

# The shell reads PDA_SHELL_CONFIG, then ~/.config/pda-shell, then
# /etc/pda-shell; the shipped file is the last resort.
if [ -z "${PDA_SHELL_CONFIG:-}" ] && [ ! -e "${XDG_CONFIG_HOME:-$HOME/.config}/pda-shell/pda-shell.toml" ] \
    && [ ! -e /etc/pda-shell/pda-shell.toml ]; then
    export PDA_SHELL_CONFIG="$ROOT/config/pda-shell.toml"
fi

# --- logs --------------------------------------------------------------------
STATE_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/pda-shell"
STAMP="$(date +%Y%m%d-%H%M%S)"
LOG_DIR="$STATE_DIR/logs/$STAMP"
mkdir -p "$LOG_DIR"
ln -sfn "$LOG_DIR" "$STATE_DIR/logs/latest"
# Keep the ten most recent sessions.
ls -1d "$STATE_DIR"/logs/2* 2>/dev/null | head -n -10 | while read -r old; do rm -rf -- "$old"; done
log() { printf '%s [device-session] %s\n' "$(date +%H:%M:%S.%3N)" "$*" | tee -a "$LOG_DIR/session.log" >&2; }
since() { awk -v a="$1" -v b="$(date +%s.%N)" 'BEGIN { printf "%.2f", b - a }'; }

# --- session HOME defaults ---------------------------------------------------
# File associations, Thunar list view, GTK 3 tooltip style (config/home).
# Files the user already has are never replaced.
SEED_VERSION=1
SEED_MARK="${XDG_CONFIG_HOME:-$HOME/.config}/pda-shell/device-seed-version"
if [ "$(cat "$SEED_MARK" 2>/dev/null)" != "$SEED_VERSION" ]; then
    mkdir -p "$HOME/Documents" "$(dirname "$SEED_MARK")"
    cp -Rn "$ROOT/config/home/." "$HOME/" 2>/dev/null || true
    echo "$SEED_VERSION" > "$SEED_MARK"
    log "seeded $HOME with the session defaults"
fi
if [ "$SEED_SAMPLES" -eq 1 ]; then
    mkdir -p "$HOME/Documents"
    cp -Rn "$ROOT/assets/sample-files/." "$HOME/Documents/" 2>/dev/null || true
fi

# --- runtime directory -------------------------------------------------------
# logind provides /run/user/UID for a login session; otherwise use a private one.
OWN_RUNTIME=0
if [ -z "${XDG_RUNTIME_DIR:-}" ] || [ ! -w "${XDG_RUNTIME_DIR}" ]; then
    XDG_RUNTIME_DIR="$(mktemp -d "${TMPDIR:-/tmp}/pda-shell-run.XXXXXX")"
    OWN_RUNTIME=1
fi
export XDG_RUNTIME_DIR

# Every process started here inherits this tag, so cleanup finds programs
# that re-parented themselves (double fork, D-Bus activation).
export PDA_SESSION_TAG="pda-device-$$-$STAMP"
NESTED=":$DISPLAY_NUM"
XAUTH_FILE=""
PIDS=()
CLEANED=0
cleanup() {
    [ "$CLEANED" -eq 1 ] && return
    CLEANED=1
    log "stopping session $NESTED"
    tagged=()
    for env in /proc/[0-9]*/environ; do
        pid="${env#/proc/}"; pid="${pid%/environ}"
        [ "$pid" = "$$" ] || [ "$pid" = "${XSERVER_PID:-}" ] && continue
        if { tr '\0' '\n' < "$env"; } 2>/dev/null | grep -qx "PDA_SESSION_TAG=$PDA_SESSION_TAG"; then
            tagged+=("$pid")
        fi
    done
    # Programs first, the X server last, so nothing loses its display while
    # still shutting down.
    for group in "${tagged[*]:-} ${SHELL_PID:-} ${STYLUS_PID:-} ${OPENBOX_PID:-} ${DBUS_PID:-}" "${XSERVER_PID:-}"; do
        # shellcheck disable=SC2086
        set -- $group
        [ $# -eq 0 ] && continue
        kill -TERM "$@" 2>/dev/null || true
        for _ in $(seq 30); do
            alive=0
            for pid in "$@"; do kill -0 "$pid" 2>/dev/null && alive=1; done
            [ "$alive" -eq 0 ] && break
            sleep 0.1
        done
        kill -KILL "$@" 2>/dev/null || true
    done
    [ -n "$XAUTH_FILE" ] && rm -f "$XAUTH_FILE"
    [ "$OWN_RUNTIME" -eq 1 ] && rm -rf "$XDG_RUNTIME_DIR"
    log "session stopped; logs in $LOG_DIR"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM HUP

# --- X server ------------------------------------------------------------------
[ ! -e "/tmp/.X11-unix/X$DISPLAY_NUM" ] || { log "display $NESTED is in use"; exit 1; }
server_args=("$NESTED" -nolisten tcp -noreset)
if command -v xauth >/dev/null && command -v mcookie >/dev/null; then
    XAUTH_FILE="$XDG_RUNTIME_DIR/pda-shell.xauth"
    : > "$XAUTH_FILE"; chmod 600 "$XAUTH_FILE"
    xauth -q -f "$XAUTH_FILE" add "$NESTED" . "$(mcookie)"
    export XAUTHORITY="$XAUTH_FILE"
    server_args+=(-auth "$XAUTH_FILE")
else
    log "xauth/mcookie missing: the X server runs without an access cookie"
fi
if [ "$XSERVER" = Xorg ]; then
    if [ -z "$VT" ]; then
        tty_name="$(tty 2>/dev/null || true)"
        case "$tty_name" in /dev/tty[0-9]*) VT="${tty_name#/dev/tty}" ;; esac
    fi
    [ -n "$VT" ] || { log "no virtual terminal: start from a console login, the systemd unit, or pass --vt N"; exit 1; }
    # -keeptty: stay on the logind session's VT (rootless Xorg); -novtswitch:
    # leaving the session must not jump to another console.
    server_args+=("vt$VT" -keeptty -novtswitch)
else
    server_args+=(-screen 0 "${SCREEN}x24" -dpi 96)
fi
log "starting $XSERVER ${server_args[*]} (bin $BIN_DIR, config ${PDA_SHELL_CONFIG:-default search})"
# Not tagged: cleanup stops the X server last, after the programs.
env -u PDA_SESSION_TAG "$XSERVER" "${server_args[@]}" > "$LOG_DIR/xserver.log" 2>&1 &
XSERVER_PID=$!
export DISPLAY="$NESTED"
for _ in $(seq 150); do
    xdpyinfo >/dev/null 2>&1 && break
    kill -0 "$XSERVER_PID" 2>/dev/null || { log "X server exited; see $LOG_DIR/xserver.log"; exit 1; }
    sleep 0.1
done
xdpyinfo >/dev/null 2>&1 || { log "X server did not come up within 15 s"; exit 1; }
T_X="$(since "$T0")"

# --- display: output, mode, rotation ---------------------------------------------
plan_args=()
[ -n "$ORIENTATION" ] && plan_args+=(--orientation "$ORIENTATION")
[ -n "$MODE" ] && plan_args+=(--mode "$MODE")
[ -n "$OUTPUT_NAME" ] && plan_args+=(--output "$OUTPUT_NAME")
xrandr --query > "$LOG_DIR/xrandr-before.txt" 2>&1 || true
if plan="$("$BIN_DIR/pda-device" display --xrandr "$LOG_DIR/xrandr-before.txt" "${plan_args[@]}" 2>>"$LOG_DIR/session.log")"; then
    eval "$plan"
    log "display plan: output $OUTPUT mode $MODE rotation $ROTATION -> ${WIDTH}x${HEIGHT} $ORIENTATION (panel ${SIZE_MM} mm, ${PPI} ppi)"
    if xrandr --output "$OUTPUT" --mode "$MODE" --rotate "$ROTATION" >> "$LOG_DIR/session.log" 2>&1; then
        SCREEN_ROTATION="$ROTATION"
    else
        log "xrandr could not set $MODE $ROTATION on $OUTPUT; keeping the current mode"
    fi
else
    log "no display plan (see above); keeping the X server's choice"
fi
SCREEN_ROTATION="${SCREEN_ROTATION:-normal}"
read -r SCREEN_W SCREEN_H < <(xdpyinfo | awk '/dimensions:/ { split($2, d, "x"); print d[1], d[2] }')
if [ "$SCREEN_H" -gt "$SCREEN_W" ]; then SCREEN_ORIENTATION=portrait; else SCREEN_ORIENTATION=landscape; fi
log "screen ${SCREEN_W}x${SCREEN_H} $SCREEN_ORIENTATION"
[ -n "${WIDTH:-}" ] && [ "$WIDTH" != "$SCREEN_W" -o "$HEIGHT" != "$SCREEN_H" ] \
    && log "screen differs from the plan (${WIDTH}x${HEIGHT}); the shell lays out the real size"

# Blank the screen when idle (any tap wakes it); 0 disables blanking.
xset s off 2>/dev/null || true
if [ "${SCREEN_OFF_MINUTES:-0}" -gt 0 ]; then
    xset dpms 0 0 $((SCREEN_OFF_MINUTES * 60)) 2>/dev/null || log "no DPMS on this display; the screen is not blanked"
else
    xset -dpms 2>/dev/null || true
fi

# --- environment for everything inside the session ---------------------------
unset WAYLAND_DISPLAY SESSION_MANAGER
export GDK_BACKEND=x11
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=pda-shell
export PDA_SHELL_ORIENTATION="$SCREEN_ORIENTATION"
# The touchscreen follows the picture (pda-device stylus, touch, calibrate).
export PDA_DEVICE_ROTATION="$SCREEN_ROTATION"
export PDA_SHELL_ICONS="$ROOT/assets/icons"
export PATH="$ROOT/scripts/bin:$BIN_DIR:$PATH"
export XDG_DATA_DIRS="$ROOT/config/share:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
export RUST_LOG="${RUST_LOG:-pda_shell=info,warn}"

# D-Bus: the login session's bus (systemd user session) if there is one,
# else a private one for this session.
if [ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ] && [ -S "$XDG_RUNTIME_DIR/bus" ]; then
    export DBUS_SESSION_BUS_ADDRESS="unix:path=$XDG_RUNTIME_DIR/bus"
fi
if [ -n "${DBUS_SESSION_BUS_ADDRESS:-}" ] && dbus-send --session --dest=org.freedesktop.DBus --print-reply \
    /org/freedesktop/DBus org.freedesktop.DBus.GetId >/dev/null 2>&1; then
    log "using the login session bus $DBUS_SESSION_BUS_ADDRESS"
    # Programs that bus or systemd --user start (D-Bus activation) need the display.
    dbus-update-activation-environment --systemd DISPLAY XAUTHORITY GDK_BACKEND XDG_SESSION_TYPE \
        XDG_CURRENT_DESKTOP PATH XDG_DATA_DIRS PDA_SESSION_TAG >/dev/null 2>&1 || true
else
    dbus-daemon --session --fork --address="unix:path=$XDG_RUNTIME_DIR/pda-bus" \
        --print-address=3 --print-pid=4 3>"$XDG_RUNTIME_DIR/pda-bus.address" 4>"$XDG_RUNTIME_DIR/pda-bus.pid"
    DBUS_SESSION_BUS_ADDRESS="$(head -n1 "$XDG_RUNTIME_DIR/pda-bus.address")"
    DBUS_PID="$(head -n1 "$XDG_RUNTIME_DIR/pda-bus.pid")"
    export DBUS_SESSION_BUS_ADDRESS
    log "private session bus $DBUS_SESSION_BUS_ADDRESS (pid $DBUS_PID)"
fi

# --- window manager ------------------------------------------------------------
wm_ready() { xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null | grep -q 'window id'; }
start_openbox() {
    local attempt
    for attempt in 1 2 3 4 5; do
        openbox --config-file "$ROOT/config/openbox/rc.xml" >> "$LOG_DIR/openbox.log" 2>&1 &
        OPENBOX_PID=$!
        for _ in $(seq 50); do
            wm_ready && kill -0 "$OPENBOX_PID" 2>/dev/null && return 0
            kill -0 "$OPENBOX_PID" 2>/dev/null || break
            sleep 0.1
        done
        kill "$OPENBOX_PID" 2>/dev/null || true
        log "openbox did not start (attempt $attempt); retrying"
        sleep 0.5
    done
    return 1
}
start_openbox || { log "openbox is not running; see $LOG_DIR/openbox.log"; exit 1; }
T_WM="$(since "$T0")"
log "openbox running (pid $OPENBOX_PID)"

# --- stylus ------------------------------------------------------------------------
# The X server first gets the touchscreen itself, rotated and calibrated
# (pda-device touch): plain taps and drags work even if the daemon cannot
# run. The daemon then grabs it and adds hold-to-right-click for every
# program through its virtual pointer; if it stops, the grab ends and X has
# the touchscreen again.
STYLUS_PID=""
STYLUS_FAILS=()
if touch_info="$("$BIN_DIR/pda-device" touch 2>>"$LOG_DIR/session.log")"; then
    eval "$touch_info"
    log "touchscreen $TOUCH_DEVICE ($TOUCH_NAME, $TOUCH_ID), rotation $SCREEN_ROTATION"
    if command -v xinput >/dev/null; then
        xinput set-prop "pointer:$TOUCH_NAME" "Coordinate Transformation Matrix" $TOUCH_MATRIX \
            >> "$LOG_DIR/session.log" 2>&1 || log "xinput cannot map $TOUCH_NAME; the X server keeps its default"
    fi
    HAVE_TOUCH=1
else
    log "no touchscreen (pda-device inputs lists the devices); no stylus translation"
    HAVE_TOUCH=0
fi
start_stylus() {
    RUST_LOG="${RUST_LOG_STYLUS:-info}" "$BIN_DIR/pda-device" stylus --screen "${SCREEN_W}x${SCREEN_H}" \
        --rotation "$SCREEN_ROTATION" >> "$LOG_DIR/stylus.log" 2>&1 &
    STYLUS_PID=$!
    log "stylus daemon pid $STYLUS_PID (log stylus.log)"
}
[ "$HAVE_TOUCH" -eq 1 ] && start_stylus

# --- the shell, restarted if it crashes ------------------------------------------
shell_windows() {
    local n=0 w
    for w in $(xprop -root _NET_CLIENT_LIST 2>/dev/null | sed -n 's/.*window id # //p' | tr -d ','); do
        xprop -id "$w" WM_CLASS 2>/dev/null | grep -q '"pda-shell"' && n=$((n + 1))
    done
    echo "$n"
}
start_shell() {
    "$SHELL_BIN" >> "$LOG_DIR/shell.log" 2>&1 &
    SHELL_PID=$!
    log "pda-shell pid $SHELL_PID"
}
# Helpers of a crashed shell (the on-screen keyboard) would be started twice.
stop_shell_helpers() {
    local env pid cmd
    for env in /proc/[0-9]*/environ; do
        pid="${env#/proc/}"; pid="${pid%/environ}"
        { tr '\0' '\n' < "$env"; } 2>/dev/null | grep -qx "PDA_SESSION_TAG=$PDA_SESSION_TAG" || continue
        cmd="$(tr '\0' ' ' < "/proc/$pid/cmdline" 2>/dev/null)"
        case "$cmd" in *onboard*) kill "$pid" 2>/dev/null && log "stopped leftover keyboard $pid" ;; esac
    done
}

start_shell
for _ in $(seq 300); do
    [ "$(shell_windows)" -ge 3 ] && break
    kill -0 "$SHELL_PID" 2>/dev/null || break
    sleep 0.05
done
T_SHELL="$(since "$T0")"
log "startup: X server ${T_X} s, window manager $(awk -v a="$T_X" -v b="$T_WM" 'BEGIN{printf "%.2f", b-a}') s," \
    "shell $(awk -v a="$T_WM" -v b="$T_SHELL" 'BEGIN{printf "%.2f", b-a}') s, total ${T_SHELL} s"
printf 'x_server=%s\nwindow_manager=%s\nshell=%s\nscreen=%sx%s\n' "$T_X" "$T_WM" "$T_SHELL" "$SCREEN_W" "$SCREEN_H" \
    > "$LOG_DIR/startup.txt"
if [ -n "$READY_FILE" ]; then
    printf 'DISPLAY=%s\nLOG_DIR=%s\nHOME=%s\nXAUTHORITY=%s\nDBUS_SESSION_BUS_ADDRESS=%s\n' \
        "$NESTED" "$LOG_DIR" "$HOME" "${XAUTHORITY:-}" "$DBUS_SESSION_BUS_ADDRESS" > "$READY_FILE"
fi

crashes=()
status=0
while :; do
    status=0
    wait -n "$SHELL_PID" "$XSERVER_PID" "$OPENBOX_PID" ${STYLUS_PID:+"$STYLUS_PID"} || status=$?
    if ! kill -0 "$XSERVER_PID" 2>/dev/null; then
        log "X server exited; see $LOG_DIR/xserver.log"
        status=1
        break
    fi
    if ! kill -0 "$OPENBOX_PID" 2>/dev/null; then
        log "openbox exited; restarting it (programs keep their windows)"
        start_openbox || { log "openbox cannot be restarted"; status=1; break; }
        continue
    fi
    if [ -n "$STYLUS_PID" ] && ! kill -0 "$STYLUS_PID" 2>/dev/null; then
        # The X server has the raw touchscreen again (taps, no right click).
        now="$(date +%s)"
        recent=()
        for t in "${STYLUS_FAILS[@]}" "$now"; do [ $((now - t)) -lt "$CRASH_WINDOW" ] && recent+=("$t"); done
        STYLUS_FAILS=("${recent[@]}")
        if [ "${#STYLUS_FAILS[@]}" -ge "$MAX_CRASHES" ]; then
            log "stylus daemon exited with status $status, ${#STYLUS_FAILS[@]} times within ${CRASH_WINDOW} s;" \
                "leaving the touchscreen to the X server (no hold-to-right-click)"
            STYLUS_PID=""
        else
            log "stylus daemon exited with status $status; restarting it"
            sleep 1
            start_stylus
        fi
        continue
    fi
    if kill -0 "$SHELL_PID" 2>/dev/null; then continue; fi
    if [ "$status" -eq 0 ]; then
        log "session ended from the shell (End session)"
        break
    fi
    now="$(date +%s)"
    recent=()
    for t in "${crashes[@]}" "$now"; do [ $((now - t)) -lt "$CRASH_WINDOW" ] && recent+=("$t"); done
    crashes=("${recent[@]}")
    if [ "${#crashes[@]}" -ge "$MAX_CRASHES" ]; then
        log "pda-shell exited with status $status, ${#crashes[@]} times within ${CRASH_WINDOW} s; giving up"
        status=1
        break
    fi
    log "pda-shell exited with status $status; restarting (${#crashes[@]}/$MAX_CRASHES)"
    stop_shell_helpers
    sleep 1
    start_shell
done
exit "$status"

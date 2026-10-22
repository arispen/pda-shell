#!/usr/bin/env bash
# Start a nested PDA desktop (X server + Openbox + pda-shell) inside the
# developer's normal desktop session.
#
#   ./scripts/dev-session.sh --orientation portrait     # 600x1024
#   ./scripts/dev-session.sh --orientation landscape    # 1024x600
#
# Options:
#   --orientation portrait|landscape   (default: portrait)
#   --headless          use Xvfb instead of a Xephyr window (CI, screenshots)
#   --display :N        use this display number instead of the first free one
#   --no-build          do not run `cargo build` first
#   --release           build and run the release binary
#   --no-shell          start X + Openbox only (debugging)
#   --ready-file PATH   write "DISPLAY=:N" to PATH once the shell is mapped
#
# Everything started here is isolated from the host desktop: its own X
# display, its own D-Bus session bus, and its own HOME/XDG directories under
# .dev-session/. Leaving the session (closing the Xephyr window, Ctrl+C, or
# "End session" in the Start menu) stops every process the session started.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ORIENTATION="portrait"
HEADLESS=0
DISPLAY_NUM=""
BUILD=1
PROFILE="debug"
RUN_SHELL=1
READY_FILE=""

usage() { sed -n '2,22p' "$0"; }

while [ $# -gt 0 ]; do
    case "$1" in
        --orientation) ORIENTATION="${2:?}"; shift 2 ;;
        --orientation=*) ORIENTATION="${1#*=}"; shift ;;
        --headless) HEADLESS=1; shift ;;
        --display) DISPLAY_NUM="${2:?}"; DISPLAY_NUM="${DISPLAY_NUM#:}"; shift 2 ;;
        --no-build) BUILD=0; shift ;;
        --release) PROFILE="release"; shift ;;
        --no-shell) RUN_SHELL=0; shift ;;
        --ready-file) READY_FILE="${2:?}"; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
    esac
done

case "$ORIENTATION" in
    portrait) WIDTH=600; HEIGHT=1024 ;;
    landscape) WIDTH=1024; HEIGHT=600 ;;
    *) echo "orientation must be portrait or landscape" >&2; exit 2 ;;
esac

for tool in openbox xdpyinfo xprop; do
    command -v "$tool" >/dev/null || { echo "missing $tool; run scripts/setup-debian.sh" >&2; exit 1; }
done
if [ "$HEADLESS" -eq 1 ]; then XSERVER=Xvfb; else XSERVER=Xephyr; fi
command -v "$XSERVER" >/dev/null || { echo "missing $XSERVER; run scripts/setup-debian.sh" >&2; exit 1; }
if [ "$HEADLESS" -eq 0 ] && [ -z "${DISPLAY:-}" ]; then
    echo "Xephyr needs a host X display (DISPLAY is unset); use --headless" >&2
    exit 1
fi

# --- build -------------------------------------------------------------------
TARGET_DIR="${CARGO_TARGET_DIR:-$REPO/target}"
SHELL_BIN="$TARGET_DIR/$PROFILE/pda-shell"
if [ "$RUN_SHELL" -eq 1 ]; then
    if [ "$BUILD" -eq 1 ]; then
        flag=""; [ "$PROFILE" = "release" ] && flag="--release"
        (cd "$REPO" && cargo build $flag)
    fi
    [ -x "$SHELL_BIN" ] || { echo "shell binary not found: $SHELL_BIN" >&2; exit 1; }
fi

# --- session directories -----------------------------------------------------
SESSION_ROOT="$REPO/.dev-session"
STAMP="$(date +%Y%m%d-%H%M%S)"
LOG_DIR="$SESSION_ROOT/logs/$STAMP-$ORIENTATION"
SESSION_HOME="$SESSION_ROOT/home"
# Sockets need a local file system with Unix permissions, so the runtime
# directory lives in /tmp rather than in the (possibly shared) checkout.
RUNTIME_DIR="$(mktemp -d "${TMPDIR:-/tmp}/pda-shell-run.XXXXXX")"
mkdir -p "$LOG_DIR" "$SESSION_HOME"
ln -sfn "$LOG_DIR" "$SESSION_ROOT/logs/latest"

if [ ! -e "$SESSION_HOME/.seeded" ]; then
    mkdir -p "$SESSION_HOME/Documents" "$SESSION_HOME/.config" "$SESSION_HOME/.local/share" "$SESSION_HOME/.cache"
    cp -R "$REPO/assets/sample-files/." "$SESSION_HOME/Documents/" 2>/dev/null || true
    touch "$SESSION_HOME/.seeded"
fi

log() { printf '%s [dev-session] %s\n' "$(date +%H:%M:%S)" "$*" | tee -a "$LOG_DIR/session.log" >&2; }

# --- display -----------------------------------------------------------------
display_free() { [ ! -e "/tmp/.X$1-lock" ] && [ ! -e "/tmp/.X11-unix/X$1" ]; }
if [ -z "$DISPLAY_NUM" ]; then
    DISPLAY_NUM=10
    while ! display_free "$DISPLAY_NUM"; do DISPLAY_NUM=$((DISPLAY_NUM + 1)); done
elif ! display_free "$DISPLAY_NUM"; then
    echo "display :$DISPLAY_NUM is in use" >&2; exit 1
fi
NESTED=":$DISPLAY_NUM"

# Every process started by this session inherits this tag, so cleanup can find
# them even if they re-parent themselves (double fork, D-Bus activation).
export PDA_SESSION_TAG="pda-$ORIENTATION-$$-$STAMP"

PIDS=()
CLEANED=0
cleanup() {
    [ "$CLEANED" -eq 1 ] && return
    CLEANED=1
    log "stopping session $NESTED"
    tagged=()
    for env in /proc/[0-9]*/environ; do
        pid="${env#/proc/}"; pid="${pid%/environ}"
        [ "$pid" = "$$" ] && continue
        if { tr '\0' '\n' < "$env"; } 2>/dev/null | grep -qx "PDA_SESSION_TAG=$PDA_SESSION_TAG"; then
            tagged+=("$pid")
        fi
    done
    all=("${PIDS[@]}" "${tagged[@]}")
    if [ "${#all[@]}" -gt 0 ]; then
        kill -TERM "${all[@]}" 2>/dev/null || true
        for _ in $(seq 30); do
            alive=0
            for pid in "${all[@]}"; do kill -0 "$pid" 2>/dev/null && alive=1; done
            [ "$alive" -eq 0 ] && break
            sleep 0.1
        done
        kill -KILL "${all[@]}" 2>/dev/null || true
    fi
    rm -rf "$RUNTIME_DIR"
    log "session stopped; logs in $LOG_DIR"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM HUP

log "orientation=$ORIENTATION ${WIDTH}x${HEIGHT} display=$NESTED server=$XSERVER"
if [ "$HEADLESS" -eq 1 ]; then
    Xvfb "$NESTED" -screen 0 "${WIDTH}x${HEIGHT}x24" -nolisten tcp -dpi 96 \
        > "$LOG_DIR/xserver.log" 2>&1 &
else
    Xephyr "$NESTED" -screen "${WIDTH}x${HEIGHT}" -title "pda-shell $ORIENTATION ${WIDTH}x${HEIGHT}" \
        -no-host-grab -nolisten tcp -dpi 96 \
        > "$LOG_DIR/xserver.log" 2>&1 &
fi
XSERVER_PID=$!
PIDS+=("$XSERVER_PID")

for _ in $(seq 100); do
    xdpyinfo -display "$NESTED" >/dev/null 2>&1 && break
    kill -0 "$XSERVER_PID" 2>/dev/null || { log "X server exited; see $LOG_DIR/xserver.log"; exit 1; }
    sleep 0.1
done
xdpyinfo -display "$NESTED" >/dev/null 2>&1 || { log "X server did not come up"; exit 1; }

# --- environment for everything inside the session ---------------------------
export DISPLAY="$NESTED"
unset WAYLAND_DISPLAY SESSION_MANAGER DBUS_SESSION_BUS_ADDRESS XDG_SESSION_TYPE
export GDK_BACKEND=x11
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=pda-shell
export HOME="$SESSION_HOME"
export XDG_CONFIG_HOME="$SESSION_HOME/.config"
export XDG_DATA_HOME="$SESSION_HOME/.local/share"
export XDG_CACHE_HOME="$SESSION_HOME/.cache"
export XDG_STATE_HOME="$SESSION_HOME/.local/state"
export XDG_RUNTIME_DIR="$RUNTIME_DIR"
export PDA_SHELL_CONFIG="${PDA_SHELL_CONFIG:-$REPO/config/pda-shell.toml}"
export PDA_SHELL_ORIENTATION="$ORIENTATION"
export RUST_LOG="${RUST_LOG:-pda_shell=debug,info}"

dbus-daemon --session --fork --address="unix:path=$RUNTIME_DIR/bus" \
    --print-address=3 --print-pid=4 3>"$RUNTIME_DIR/bus.address" 4>"$RUNTIME_DIR/bus.pid"
DBUS_SESSION_BUS_ADDRESS="$(head -n1 "$RUNTIME_DIR/bus.address")"
DBUS_PID="$(head -n1 "$RUNTIME_DIR/bus.pid")"
export DBUS_SESSION_BUS_ADDRESS
PIDS+=("$DBUS_PID")
log "session bus $DBUS_SESSION_BUS_ADDRESS (pid $DBUS_PID)"

openbox --config-file "$REPO/config/openbox/rc.xml" > "$LOG_DIR/openbox.log" 2>&1 &
PIDS+=("$!")
for _ in $(seq 50); do
    xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null | grep -q 'window id' && break
    sleep 0.1
done
log "openbox running"

if [ "$RUN_SHELL" -eq 0 ]; then
    log "no shell requested; waiting for the X server to exit"
    wait "$XSERVER_PID" || true
    exit 0
fi

"$SHELL_BIN" > "$LOG_DIR/shell.log" 2>&1 &
SHELL_PID=$!
PIDS+=("$SHELL_PID")
log "pda-shell pid $SHELL_PID, log $LOG_DIR/shell.log"

if [ -n "$READY_FILE" ]; then
    for _ in $(seq 100); do
        if xprop -root _NET_WORKAREA 2>/dev/null | grep -q '='; then
            if [ "$(xdotool search --classname pda-shell 2>/dev/null | wc -l)" -ge 3 ]; then break; fi
        fi
        kill -0 "$SHELL_PID" 2>/dev/null || break
        sleep 0.1
    done
    printf 'DISPLAY=%s\nLOG_DIR=%s\nHOME=%s\n' "$NESTED" "$LOG_DIR" "$SESSION_HOME" > "$READY_FILE"
fi

# Leave when either the shell or the X server goes away.
status=0
wait -n "$SHELL_PID" "$XSERVER_PID" || status=$?
if kill -0 "$SHELL_PID" 2>/dev/null; then
    log "X server exited"
else
    log "pda-shell exited with status $status"
fi
exit "$status"

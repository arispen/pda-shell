#!/usr/bin/env bash
# Measure the session: startup, idle and active memory and CPU, and how long
# Gnumeric and Firefox take to open a document. Prints a Markdown report for
# docs/test-results.md (and saves it next to the session logs).
#
# On the device, inside the running session (e.g. over ssh as the session
# user):
#   ./scripts/device-bench.sh --attach                 # DISPLAY :0
# Anywhere else, in a session this script starts itself (Xvfb, labelled as
# simulated in the report):
#   ./scripts/device-bench.sh --simulate [--screen 1024x600]
#
# Options: --runs N (launches per program, default 3), --idle S (seconds of
# each CPU sample, default 10), --display :N (with --attach).
#
# Record next to the numbers: board and RAM, Debian and kernel (the report
# starts with `pda-device info`), panel and resolution, power mode/governor,
# and how the board was powered. Power draw is measured externally (USB
# power meter); this script does not estimate it.
set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODE=""
SCREEN=1024x600
RUNS=3
IDLE=10
ATTACH_DISPLAY=":0"
while [ $# -gt 0 ]; do
    case "$1" in
        --attach) MODE=attach; shift ;;
        --simulate) MODE=simulate; shift ;;
        --screen) SCREEN="${2:?}"; shift 2 ;;
        --runs) RUNS="${2:?}"; shift 2 ;;
        --idle) IDLE="${2:?}"; shift 2 ;;
        --display) ATTACH_DISPLAY="${2:?}"; shift 2 ;;
        -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done
[ -n "$MODE" ] || { echo "choose --attach (on the device) or --simulate" >&2; exit 2; }

find_bin() {
    local d
    for d in "${PDA_BIN_DIR:-}" "$REPO/bin" "${CARGO_TARGET_DIR:-$REPO/target}/release" "${CARGO_TARGET_DIR:-$REPO/target}/debug"; do
        [ -n "$d" ] && [ -x "$d/pda-device" ] && { echo "$d"; return 0; }
    done
    return 1
}
BIN_DIR="$(find_bin)" || { echo "pda-device not found; build first" >&2; exit 1; }
DOCS="${PDA_BENCH_DOCS:-$REPO/assets/sample-files}"
SESSION=""
cleanup() {
    if [ -n "$SESSION" ] && kill -0 "$SESSION" 2>/dev/null; then
        kill "$SESSION" 2>/dev/null; wait "$SESSION" 2>/dev/null
    fi
    [ -n "${BENCH_HOME:-}" ] && rm -rf "$BENCH_HOME"
}
trap cleanup EXIT

if [ "$MODE" = simulate ]; then
    BENCH_HOME="$(mktemp -d "${TMPDIR:-/tmp}/pda-bench-home.XXXXXX")"
    ready="$BENCH_HOME/ready"
    HOME="$BENCH_HOME" XDG_RUNTIME_DIR="" XDG_STATE_HOME="$BENCH_HOME/.local/state" \
        XDG_CONFIG_HOME="$BENCH_HOME/.config" DBUS_SESSION_BUS_ADDRESS="" PDA_BIN_DIR="$BIN_DIR" \
        "$REPO/scripts/device-session.sh" --xserver Xvfb --screen "$SCREEN" --display :40 --orientation native \
        --seed-samples --ready-file "$ready" > "$BENCH_HOME/session.out" 2>&1 &
    SESSION=$!
    for _ in $(seq 300); do [ -s "$ready" ] && break; sleep 0.1; done
    [ -s "$ready" ] || { echo "session did not start; see $BENCH_HOME/session.out" >&2; cat "$BENCH_HOME/session.out" >&2; exit 1; }
    # shellcheck disable=SC1090
    . "$ready"; export DISPLAY XAUTHORITY DBUS_SESSION_BUS_ADDRESS
    DOCS="$BENCH_HOME/Documents"
else
    export DISPLAY="$ATTACH_DISPLAY"
    export XAUTHORITY="${XAUTHORITY:-${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/pda-shell.xauth}"
    LOG_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/pda-shell/logs/latest"
    [ -d "$HOME/Documents" ] && [ -e "$HOME/Documents/Budget 2026.xlsx" ] && DOCS="$HOME/Documents"
fi
xprop -root _NET_SUPPORTING_WM_CHECK >/dev/null 2>&1 || { echo "no session on $DISPLAY" >&2; exit 1; }

# --- process sampling ----------------------------------------------------------
HZ="$(getconf CLK_TCK)"
shell_pid() { pgrep -n -x pda-shell; }
session_pids() {
    # The shell's session: every process carrying its PDA_SESSION_TAG, plus the X server.
    local tag env pid
    tag="$(tr '\0' '\n' < "/proc/$(shell_pid)/environ" 2>/dev/null | sed -n 's/^PDA_SESSION_TAG=//p')"
    for env in /proc/[0-9]*/environ; do
        pid="${env#/proc/}"; pid="${pid%/environ}"
        { tr '\0' '\n' < "$env"; } 2>/dev/null | grep -qx "PDA_SESSION_TAG=$tag" && echo "$pid"
    done
    pgrep -x Xorg; pgrep -x Xvfb
}
proc_rss() { local p="$1"; awk '/^VmRSS/{print $2}' "/proc/$p/status" 2>/dev/null; }
cpu_ticks() { local p t=0; for p in "$@"; do t=$((t + $(awk '{print $14 + $15}' "/proc/$p/stat" 2>/dev/null || echo 0))); done; echo "$t"; }
# CPU use of the session over IDLE seconds, in % of one core.
cpu_pct() {
    local pids; pids=($(session_pids))
    local a b; a="$(cpu_ticks "${pids[@]}")"; sleep "$IDLE"; b="$(cpu_ticks "${pids[@]}")"
    awk -v d=$((b - a)) -v hz="$HZ" -v s="$IDLE" 'BEGIN { printf "%.1f", 100 * d / hz / s }'
}
mem_used_mib() { awk '/^MemTotal/{t=$2} /^MemAvailable/{a=$2} END{printf "%d", (t - a) / 1024}' /proc/meminfo; }
session_rss_mib() { local pids; pids=($(session_pids)); local s=0 p r; for p in "${pids[@]}"; do r="$(proc_rss "$p")"; s=$((s + ${r:-0})); done; echo $((s / 1024)); }
named_rss_mib() { local s=0 p r; for p in $(pgrep -x "$1"); do r="$(proc_rss "$p")"; s=$((s + ${r:-0})); done; echo $((s / 1024)); }

clients() { xprop -root _NET_CLIENT_LIST 2>/dev/null | sed -n 's/.*window id # //p' | tr -d ','; }
has_window() { local w; for w in $(clients); do xprop -id "$w" WM_CLASS 2>/dev/null | grep -qi "\"$1\"" && return 0; done; return 1; }
now_ms() { date +%s%3N; }
# Seconds from starting a program with a document until its window is mapped.
launch_latency() {
    local class="$1"; shift
    local t0 t1 pid n=0
    t0="$(now_ms)"
    "$@" >/dev/null 2>&1 &
    pid=$!
    while ! has_window "$class"; do
        sleep 0.05; n=$((n + 1))
        [ "$n" -gt 1200 ] && { echo "timeout"; kill "$pid" 2>/dev/null; return 1; }
    done
    t1="$(now_ms)"
    awk -v d=$((t1 - t0)) 'BEGIN { printf "%.2f", d / 1000 }'
}
close_all() {
    local w
    for w in $(clients); do
        xprop -id "$w" WM_CLASS 2>/dev/null | grep -qiE '"(gnumeric|firefox-esr|firefox)"' || continue
        xdotool windowclose "$w" 2>/dev/null || wmctrl -i -c "$w" 2>/dev/null || true
    done
    pkill -x gnumeric 2>/dev/null; pkill -f firefox-esr 2>/dev/null
    sleep 2
}

# --- report --------------------------------------------------------------------
REPORT="$(mktemp)"
out() { printf '%s\n' "$*" | tee -a "$REPORT"; }
SCREEN_SIZE="$(xdpyinfo | awk '/dimensions:/{print $2}')"
out "## Session measurements, $(date '+%Y-%m-%d %H:%M')"
out ""
if [ "$MODE" = simulate ]; then
    where="this machine"; [ -e /.dockerenv ] && where="a container"
    out "**Simulated**: Xvfb at $SCREEN_SIZE in $where ($(uname -m)); not the device. The numbers only compare runs on the same host."
else
    out "**On the device**: the session on $DISPLAY at $SCREEN_SIZE."
fi
out ""
out '```text'
"$BIN_DIR/pda-device" info | tee -a "$REPORT"
out "screen: $SCREEN_SIZE"
out '```'
out ""
out "| Measurement | Value |"
out "|---|---|"
if [ -f "${LOG_DIR:-/nonexistent}/startup.txt" ]; then
    # shellcheck disable=SC1090
    . "$LOG_DIR/startup.txt"
    out "| Startup to X server ready | ${x_server} s |"
    out "| Startup to window manager ready | ${window_manager} s |"
    out "| Startup to shell bars mapped | ${shell} s |"
fi
sleep 3
out "| Idle: memory used (system) | $(mem_used_mib) MiB |"
out "| Idle: session RSS (shell, X, WM, helpers) | $(session_rss_mib) MiB |"
out "| Idle: pda-shell RSS | $(( $(proc_rss "$(shell_pid)") / 1024 )) MiB |"
out "| Idle: session CPU over ${IDLE} s | $(cpu_pct) % of one core |"

g=(); f=()
for i in $(seq "$RUNS"); do
    g+=("$(launch_latency gnumeric gnumeric "$DOCS/Budget 2026.xlsx")")
    sleep 3
    [ "$i" -lt "$RUNS" ] && close_all
done
out "| Gnumeric opening Budget 2026.xlsx, window mapped (${RUNS} runs) | ${g[*]} s |"
for i in $(seq "$RUNS"); do
    f+=("$(launch_latency firefox-esr firefox-esr "file://$DOCS/Web/index.html")")
    sleep 5
    [ "$i" -lt "$RUNS" ] && pkill -f firefox-esr 2>/dev/null && sleep 2
done
out "| Firefox ESR opening Web/index.html, window mapped (${RUNS} runs) | ${f[*]} s |"
sleep 3
out "| Active (Gnumeric + Firefox open): memory used (system) | $(mem_used_mib) MiB |"
out "| Active: Gnumeric RSS | $(named_rss_mib gnumeric) MiB |"
out "| Active: Firefox RSS (all processes) | $(( $(for p in $(pgrep -f firefox-esr); do proc_rss "$p"; done | awk '{s+=$1} END{print s+0}') / 1024 )) MiB |"
out "| Active: session CPU over ${IDLE} s, both programs idle | $(cpu_pct) % of one core |"
close_all
out ""
out "Window mapped is when the program's window appears, not when the document has finished rendering."
if [ "$MODE" = simulate ]; then
    dest="$REPO/screenshots/device-test/bench-simulated-$(date +%Y%m%d-%H%M%S).md"; mkdir -p "$(dirname "$dest")"
else
    dest="${LOG_DIR:-.}/bench-$(date +%Y%m%d-%H%M%S).md"
fi
cp "$REPORT" "$dest" 2>/dev/null && echo "(saved to $dest)" >&2
rm -f "$REPORT"

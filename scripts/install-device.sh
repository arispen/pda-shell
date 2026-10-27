#!/usr/bin/env bash
# Install pda-shell on the device (Orange Pi or any Debian 13 board) so the
# session starts on the panel at boot. Run from a checkout, as root:
#
#   sudo ./scripts/setup-debian.sh --device     # once: packages
#   sudo ./scripts/install-device.sh --enable   # build, install, start at boot
#
# Options:
#   --user NAME     account the session runs as (default: the user who ran
#                   sudo, else "pda"; created if missing)
#   --prefix DIR    where programs, scripts, config and icons go
#                   (default /opt/pda-shell)
#   --no-build      install the existing release build (target/release)
#   --enable        start the session at boot (and now): enables the
#                   systemd unit and makes graphical.target the default
#   --samples       copy the sample documents into the user's ~/Documents
#
# Rerunning updates the installation. /etc/pda-shell/pda-shell.toml is
# created once from config/pda-shell.toml and then left alone: device
# settings ([device] orientation, mode, ...) belong there.
# Details and troubleshooting: docs/device-setup.md.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
USER_NAME="${SUDO_USER:-}"
[ "$USER_NAME" = root ] && USER_NAME=""
USER_NAME="${USER_NAME:-pda}"
PREFIX=/opt/pda-shell
BUILD=1
ENABLE=0
SAMPLES=0
while [ $# -gt 0 ]; do
    case "$1" in
        --user) USER_NAME="${2:?}"; shift 2 ;;
        --prefix) PREFIX="${2:?}"; shift 2 ;;
        --no-build) BUILD=0; shift ;;
        --enable) ENABLE=1; shift ;;
        --samples) SAMPLES=1; shift ;;
        -h|--help) sed -n '2,22p' "$0"; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done
[ "$(id -u)" -eq 0 ] || { echo "run as root (sudo $0)" >&2; exit 1; }
case "$PREFIX" in /*) ;; *) echo "--prefix must be absolute" >&2; exit 2 ;; esac

say() { printf '==> %s\n' "$*"; }

# --- build (as the owner of the checkout, never as root) ------------------------
TARGET_DIR="${CARGO_TARGET_DIR:-$REPO/target}"
if [ "$BUILD" -eq 1 ]; then
    owner="$(stat -c %U "$REPO")"
    say "building the release binaries as $owner (a first build on a 2 GB board takes a while)"
    # One job: linking four GTK programs in parallel needs more RAM than
    # small boards have.
    runuser -u "$owner" -- bash -c "cd '$REPO' && CARGO_BUILD_JOBS=\${CARGO_BUILD_JOBS:-1} cargo build --release"
fi
for b in pda-shell pda-files pda-notes pda-calendar pda-tasks pda-device; do
    [ -x "$TARGET_DIR/release/$b" ] || { echo "missing $TARGET_DIR/release/$b; build first or drop --no-build" >&2; exit 1; }
done

# --- user ------------------------------------------------------------------------
if ! id "$USER_NAME" >/dev/null 2>&1; then
    say "creating user $USER_NAME"
    useradd -m -s /bin/bash "$USER_NAME"
fi
# video/render: the display; input: the touchscreen and /dev/uinput for the
# stylus daemon (see config/device/70-pda-touchscreen.rules).
for g in video render input audio; do
    getent group "$g" >/dev/null && usermod -aG "$g" "$USER_NAME"
done
USER_HOME="$(getent passwd "$USER_NAME" | cut -d: -f6)"

# --- files ------------------------------------------------------------------------
say "installing into $PREFIX"
install -d "$PREFIX/bin" "$PREFIX/scripts/bin" "$PREFIX/config" "$PREFIX/assets"
for b in pda-shell pda-files pda-notes pda-calendar pda-tasks pda-device; do
    install -m 755 "$TARGET_DIR/release/$b" "$PREFIX/bin/$b"
done
install -m 755 "$REPO/scripts/device-session.sh" "$PREFIX/scripts/device-session.sh"
install -m 755 "$REPO"/scripts/bin/* "$PREFIX/scripts/bin/"
rm -rf "$PREFIX/config" "$PREFIX/assets"
cp -R "$REPO/config" "$PREFIX/config"
cp -R "$REPO/assets" "$PREFIX/assets"
if [ ! -e /etc/pda-shell/pda-shell.toml ]; then
    install -d /etc/pda-shell
    install -m 644 "$REPO/config/pda-shell.toml" /etc/pda-shell/pda-shell.toml
    say "created /etc/pda-shell/pda-shell.toml (device settings go here)"
fi
if [ "$SAMPLES" -eq 1 ]; then
    runuser -u "$USER_NAME" -- bash -c "mkdir -p '$USER_HOME/Documents' && cp -Rn '$PREFIX/assets/sample-files/.' '$USER_HOME/Documents/'"
    say "copied the sample documents to $USER_HOME/Documents"
fi

# --- stylus input ------------------------------------------------------------------
# /dev/uinput for the session user (input group), loaded at boot, and the
# /dev/input/pda-touchscreen symlink.
install -m 644 "$REPO/config/device/70-pda-touchscreen.rules" /etc/udev/rules.d/70-pda-touchscreen.rules
install -m 644 "$REPO/config/device/uinput.conf" /etc/modules-load.d/pda-shell-uinput.conf
modprobe uinput 2>/dev/null || say "could not load the uinput module now (it loads at the next boot)"
udevadm control --reload-rules && udevadm trigger --subsystem-match=input --subsystem-match=misc
say "installed udev rules for the touchscreen and /dev/uinput"

# --- systemd -------------------------------------------------------------------------
UNIT=/etc/systemd/system/pda-shell-session.service
sed -e "s|@USER@|$USER_NAME|g" -e "s|@PREFIX@|$PREFIX|g" "$REPO/config/device/pda-shell-session.service" > "$UNIT"
chmod 644 "$UNIT"
systemctl daemon-reload
say "installed $UNIT"
if [ "$ENABLE" -eq 1 ]; then
    systemctl set-default graphical.target
    systemctl enable pda-shell-session.service
    systemctl restart pda-shell-session.service
    say "the session starts at boot and is starting now on tty7"
else
    say "start it with: systemctl start pda-shell-session (at boot: rerun with --enable)"
fi
say "logs: journalctl -u pda-shell-session and $USER_HOME/.local/state/pda-shell/logs/latest/"

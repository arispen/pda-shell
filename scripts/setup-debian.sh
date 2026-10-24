#!/usr/bin/env bash
# Install everything needed to build and run the pda-shell development session
# on Debian 13 (trixie). Works on amd64 and arm64.
#
#   ./scripts/setup-debian.sh            # build deps + session + POC apps
#   ./scripts/setup-debian.sh --minimal  # build deps + session only (no apps)
set -euo pipefail

MINIMAL=0
for arg in "$@"; do
    case "$arg" in
        --minimal) MINIMAL=1 ;;
        -h|--help) sed -n '2,7p' "$0"; exit 0 ;;
        *) echo "unknown argument: $arg" >&2; exit 2 ;;
    esac
done

if [ -r /etc/os-release ]; then
    . /etc/os-release
    if [ "${ID:-}" != "debian" ] || [ "${VERSION_ID:-}" != "13" ]; then
        echo "warning: expected Debian 13, found ${PRETTY_NAME:-unknown}; continuing" >&2
    fi
fi

SUDO=""
if [ "$(id -u)" -ne 0 ]; then SUDO="sudo"; fi

BUILD_PKGS=(
    build-essential pkg-config ca-certificates curl git
    rustc cargo
    libgtk-4-dev
)

SESSION_PKGS=(
    xserver-xephyr xvfb openbox
    x11-utils x11-xserver-utils x11-apps xdotool wmctrl
    imagemagick dbus-x11 dbus-user-session at-spi2-core
    adwaita-icon-theme fonts-dejavu-core fonts-wine librsvg2-common
    desktop-file-utils shared-mime-info xdg-utils
    onboard gir1.2-atspi-2.0
)

APP_PKGS=(
    thunar gnumeric mousepad atril firefox-esr xfce4-terminal
)

PKGS=("${BUILD_PKGS[@]}" "${SESSION_PKGS[@]}")
if [ "$MINIMAL" -eq 0 ]; then PKGS+=("${APP_PKGS[@]}"); fi

export DEBIAN_FRONTEND=noninteractive
$SUDO apt-get update
$SUDO apt-get install -y --no-install-recommends "${PKGS[@]}"

# Wine's Tahoma clone (LGPL) is the shell's and the programs' default font.
# fonts-wine keeps its files outside the fontconfig path; register only
# Tahoma, not Wine's bitmap and symbol fonts.
$SUDO mkdir -p /usr/local/share/fonts/pda-shell
$SUDO ln -sf /usr/share/wine/fonts/tahoma.ttf /usr/share/wine/fonts/tahomabd.ttf /usr/local/share/fonts/pda-shell/
$SUDO fc-cache -f /usr/local/share/fonts/pda-shell >/dev/null

# Firefox scaling exception (see the file); only when Firefox is installed.
if [ -d /etc/firefox-esr ]; then
    $SUDO install -m 644 "$(dirname "$0")/../config/firefox/pda-shell.js" /etc/firefox-esr/pda-shell.js
fi

echo
echo "Installed. Toolchain: $(rustc --version), $(cargo --version)"
echo "GTK: $(pkg-config --modversion gtk4)"

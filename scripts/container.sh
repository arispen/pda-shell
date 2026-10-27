#!/usr/bin/env bash
# Run a command inside the Debian 13 development container. For developers
# whose host is not Debian (e.g. macOS). The repository is mounted at /src.
#
#   ./scripts/container.sh build                 # build the image
#   ./scripts/container.sh cargo test
#   ./scripts/container.sh ./scripts/smoke-test.sh
#   ./scripts/container.sh vnc portrait          # session viewable on vnc://localhost:5900
#                                                # (password "pda", or $PDA_VNC_PASSWORD)
#   ./scripts/container.sh uinput ./scripts/stylus-test.sh
#                                                # as root with /dev/uinput and /dev/input
#                                                # (the Docker VM's kernel), for the stylus test
#   ./scripts/container.sh                       # interactive shell
set -euo pipefail
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE="${PDA_IMAGE:-pda-shell-dev}"

if [ "${1:-}" = "build" ]; then
    exec docker build -t "$IMAGE" -f "$REPO/container/Dockerfile" "$REPO"
fi

run=(docker run --rm -i -v "$REPO:/src" -v pda-shell-cargo:/cargo -v pda-shell-target:/target --shm-size 512m)
[ -t 0 ] && run+=(-t)

if [ "${1:-}" = "vnc" ]; then
    orientation="${2:-portrait}"
    # The port is published on the host loopback only (127.0.0.1).
    # Xvfb plays the role of the developer's desktop; the nested Xephyr
    # session runs inside it exactly as it would on a Debian workstation.
    # macOS Screen Sharing refuses servers without a password, hence one.
    exec "${run[@]}" -p 127.0.0.1:5900:5900 -e PDA_VNC_PASSWORD="${PDA_VNC_PASSWORD:-pda}" "$IMAGE" bash -c "
        Xvfb :1 -screen 0 1280x1100x24 -nolisten tcp & sleep 1
        mkdir -p /tmp/vnc && x11vnc -storepasswd \"\$PDA_VNC_PASSWORD\" /tmp/vnc/passwd >/dev/null 2>&1
        x11vnc -display :1 -forever -rfbauth /tmp/vnc/passwd -quiet -rfbport 5900 -o /tmp/x11vnc.log &
        DISPLAY=:1 ./scripts/dev-session.sh --orientation $orientation"
fi

if [ "${1:-}" = "uinput" ]; then
    shift
    # Input devices created with uinput appear in the VM's /dev/input;
    # character major 13 is the input subsystem.
    exec "${run[@]}" -u root --device /dev/uinput -v /dev/input:/dev/input --device-cgroup-rule 'c 13:* rmw' \
        "$IMAGE" "$@"
fi

if [ $# -eq 0 ]; then set -- bash; fi
exec "${run[@]}" "$IMAGE" "$@"

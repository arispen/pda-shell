#!/usr/bin/env bash
# Run a command inside the Debian 13 development container. For developers
# whose host is not Debian (e.g. macOS). The repository is mounted at /src.
#
#   ./scripts/container.sh build                 # build the image
#   ./scripts/container.sh cargo test
#   ./scripts/container.sh ./scripts/smoke-test.sh
#   ./scripts/container.sh vnc portrait          # session viewable on vnc://localhost:5900
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
    exec "${run[@]}" -p 127.0.0.1:5900:5900 "$IMAGE" bash -c "
        Xvfb :1 -screen 0 1280x1100x24 -nolisten tcp & sleep 1
        x11vnc -display :1 -forever -nopw -quiet -rfbport 5900 -o /tmp/x11vnc.log &
        DISPLAY=:1 ./scripts/dev-session.sh --orientation $orientation"
fi

if [ $# -eq 0 ]; then set -- bash; fi
exec "${run[@]}" "$IMAGE" "$@"

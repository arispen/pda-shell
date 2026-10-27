# Hardware

## First prototype display

| | |
|---|---|
| Panel | SpotPear / Waveshare 5DP-CAPLCD-G, 5", 1024×600 (≈237 ppi) |
| Touch | Resistive, single point, passive stylus, USB |
| Video | HDMI |
| Orientation | landscape (1024×600) is primary since POC 0.4; portrait (600×1024) stays supported |
| Keyboard | none |
| Computer | not selected; bring-up boards: Orange Pi 3 LTS, Orange Pi Zero 2W (see [device-setup.md](device-setup.md)) |

The final device may use a ~5.5" panel (< 6"). Nothing in the software assumes
this panel: the screen size is read at start-up and every measurement lives in
`config/pda-shell.toml`.

## What is hardware-dependent and not validated yet

- **Readability and target sizes.** Four density profiles (compact,
  balanced, readable, and `pocketpc`, the default since POC 0.3) are ready to
  compare on the real 237 ppi panel with a stylus, together with the
  programs' session-wide scale (`app_dpi`); physical sizes are in
  [ergonomics.md](ergonomics.md) and live in Settings → System → Screen.
- **Touch input path** (POC 0.4: implemented, not yet tried with the
  panel). The USB resistive controller should appear as an evdev absolute
  device:
  1. `pda-device inputs` (or `libinput list-devices`, `evtest`) lists it
     and says whether the daemon would take it; note vendor:product.
  2. `config/device/70-pda-touchscreen.rules` gives it the stable name
     `/dev/input/pda-touchscreen` (installed by `install-device.sh`); with
     more than one touchscreen put its ids in the rule and set
     `[stylus] device`.
  3. The session maps it to the screen with the xrandr rotation (portrait
     turns it too) and starts `pda-device stylus`, which adds
     hold-to-right-click for every program.
  4. Settings → Stylus (`pda-device calibrate`): tap five crosses near the
     corners and the centre; the result is saved per user and the daemon
     reloads it.
  5. Repeat the checklist with the stylus ([device-checklist.md](device-checklist.md)).
- **System-wide long press** (POC 0.4): `pda-device stylus`, see
  `architecture.md`. It grabs only the touchscreen; tested against a fake
  uinput touchscreen and mouse on a real kernel (`scripts/stylus-test.sh`),
  not with a pen.
- **Rotation at runtime** (if the device gets an orientation sensor): needs
  RandR rotation plus touch matrix updates without restarting the session.
- **Power, battery, suspend:** out of scope for POC 0.1. The status bar shows
  a battery only if `/sys/class/power_supply` reports one.

## Device session (POC 0.4)

`scripts/device-session.sh` runs the session directly on the board's
display (Xorg, logind, no desktop environment), started at boot by
`config/device/pda-shell-session.service`. Output, mode and rotation come
from `xrandr` and `[device]` in the config (`pda-device display`); no size
is hard-coded, so other panels (e.g. 800×480) need only a config line.
Installation, boot configuration and troubleshooting:
[device-setup.md](device-setup.md); the checklist for the physical tests:
[device-checklist.md](device-checklist.md).

Evidence so far (keep these apart):

| Kind | Status |
|---|---|
| Simulated (Xvfb, Debian 13 arm64 container) | device session at 1024×600, 600×1024, 800×480: start, crash recovery, End session (`scripts/device-test.sh`) |
| Real kernel, synthetic device (uinput in the Docker VM) | stylus daemon: tap, hold → right click, jitter, drag, double tap, unplug/replug, grab exclusivity, mouse left alone (`scripts/stylus-test.sh`) |
| arm64 on the real board | **not yet** |
| Panel with stylus | **not yet** |

## Development substitute

`scripts/dev-session.sh` simulates the panel with a nested X server at
exactly 1024×600 or 600×1024, and the mouse stands in for the stylus.

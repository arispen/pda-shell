# Hardware

## First prototype display

| | |
|---|---|
| Panel | SpotPear / Waveshare 5DP-CAPLCD-G, 5", 1024×600 (≈237 ppi) |
| Touch | Resistive, single point, passive stylus, USB |
| Video | HDMI |
| Orientation | undecided: both portrait (600×1024) and landscape (1024×600) are supported |
| Keyboard | none |
| Computer | not selected (x86-64 and ARM64 both kept possible) |

The final device may use a ~5.5" panel (< 6"). Nothing in the software assumes
this panel: the screen size is read at start-up and every measurement lives in
`config/pda-shell.toml`.

## What is hardware-dependent and not validated yet

- **Readability and target sizes.** Three density profiles (compact,
  balanced, readable) are ready to compare on the real 237 ppi panel with a
  stylus; physical sizes are in [ergonomics.md](ergonomics.md).
- **Touch input path.** The USB resistive controller should appear as an
  evdev absolute pointer. Expected steps when it arrives:
  1. Identify it with `libinput list-devices` / `evtest`.
  2. Calibrate (libinput calibration matrix via a udev rule or
     `xinput set-prop … "libinput Calibration Matrix"`), including the
     rotation matrix for the chosen orientation.
  3. Map it to the HDMI output (`xinput map-to-output`).
  4. Repeat the manual usability test (docs: Milestone 7) with the stylus.
- **System-wide long press.** Requires the separate translation daemon
  described in `architecture.md` (evdev in → recognizer → uinput out). It
  should grab only the touchscreen device.
- **Rotation at runtime** (if the device gets an orientation sensor): needs
  RandR rotation plus touch matrix updates without restarting the session.
- **Power, battery, suspend:** out of scope for POC 0.1. The status bar shows
  a battery only if `/sys/class/power_supply` reports one.

## Development substitute

Until the panel arrives, `scripts/dev-session.sh` simulates it with a nested
X server at exactly 1024×600 or 600×1024, and the mouse stands in for the
stylus.

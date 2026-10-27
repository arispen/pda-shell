# Physical device checklist (POC 0.4)

Fill this in on the real board with the resistive panel and a passive
stylus, **landscape first**, then portrait. Only results from that setup
count here; container and Xvfb runs are in [test-results.md](test-results.md).
Mark each row **pass** / **fail** / **n/a** and write what you saw.
Screenshots: `import -window root shot.png` over ssh (with the session's
`DISPLAY=:0` and `XAUTHORITY=/run/user/$(id -u)/pda-shell.xauth`), or a
photo of the panel when the problem is touch or readability.

Board: ____  RAM: ____  Image / kernel: ____ (`pda-device info`)
Panel / resolution: ____  Stylus: ____  Date / tester: ____

## Bring-up

| # | Check | Result | Observations |
|---|---|---|---|
| B1 | Board boots Debian 13; console visible on the HDMI panel | | |
| B2 | `sudo ./scripts/install-device.sh --enable --samples` completes; build time noted | | |
| B3 | After a reboot the session starts on its own into Today, no desktop environment, no login prompt | | |
| B4 | `session.log`: display plan shows the panel's output and 1024×600; ppi ≈ 237 | | |
| B5 | Startup times (`startup.txt`) and time from power-on to Today (stopwatch) | | |
| B6 | `[device] orientation = "portrait"`, `systemctl restart pda-shell-session`: 600×1024, bars at the real edges | | |
| B7 | `[device] mode = "800x480"` (or another mode the panel offers): layout follows | | |
| B8 | End session: programs asked about unsaved work; a fresh session appears | | |
| B9 | `kill -ABRT $(pgrep -x pda-shell)` over ssh: the shell comes back, programs still listed | | |
| B10 | Screen blanks after `screen_off_minutes`; a tap wakes it | | |

## Stylus

| # | Check | Result | Observations |
|---|---|---|---|
| S1 | `libinput list-devices` / `evtest`: the USB controller is listed; vendor:product noted | | |
| S2 | Taps in all four corners and on the status and command bars hit their targets (after calibration) | | |
| S3 | Tap opens, double tap works where programs expect it (Thunar, Gnumeric cells) | | |
| S4 | Hold in Gnumeric's sheet, Firefox's page, Files and Thunar: a real context menu, no selection or click before it | | |
| S5 | After a hold: dragging (text selection, scroll bars) and scrolling still work | | |
| S6 | Unplug and replug the touchscreen during the session: input recovers, no stuck button | | |
| S7 | A USB mouse plugged in at the same time still works normally (not grabbed, no right-click on hold) | | |

## Landscape use (acceptance scenario)

| # | Check | Result | Observations |
|---|---|---|---|
| A1 | Today → Start → Programs; Files opens | | |
| A2 | Open `Budget 2026.xlsx` from Files in Gnumeric | | |
| A3 | Show the input panel; enter a number and `=SUM(B2:B13)*2`; sheet and entry stay usable | | |
| A4 | Hold in Gnumeric's sheet: context menu; then drag-select cells and scroll | | |
| A5 | Save; switch programs; reopen the spreadsheet through Files; values persisted | | |
| A6 | Save As dialog: name field, folder and Save reachable with the stylus only | | |
| A7 | Firefox: F11, then back with Exit full screen, stylus only | | |
| A8 | End session with an unsaved edit: the program asks | | |

## Readability and targets

| # | Check | Result | Observations |
|---|---|---|---|
| R1 | Shell text (22 px) readable at arm's length | | |
| R2 | 46 px shell targets hit reliably; 43 px input-panel keys in portrait | | |
| R3 | Programs' text at 144 dpi readable; toolbar icons (not scaled) usable | | |
| R4 | Tooltips after taps: how often, how disruptive | | |

## Performance and power

Run `./scripts/device-bench.sh --attach` and paste its report into
[test-results.md](test-results.md). Power (USB meter on the supply):

| State | Power (W) | Notes |
|---|---|---|
| Today, screen on, idle | | |
| Gnumeric editing | | |
| Firefox page loaded | | |
| Screen blanked (DPMS) | | |

# Device setup (POC 0.4)

How to put the session on a real board with the 5" HDMI panel, start it at
boot without a desktop environment, and get logs and measurements back.
Status: the procedure and every script were run in the Debian 13 arm64
development container (Xvfb instead of the panel; see
[test-results.md](test-results.md)). **Not yet run on an Orange Pi.** Steps
that only the board can confirm are marked *(board)*; record what happens
in [device-checklist.md](device-checklist.md).

## 1. Choose the bring-up board

| | Orange Pi 3 LTS | Orange Pi Zero 2W |
|---|---|---|
| SoC | Allwinner H6, 4× Cortex-A53 | Allwinner H618, 4× Cortex-A53 |
| RAM | 2 GB | 1, 1.5, 2 or 4 GB |
| Video out | full-size HDMI | mini-HDMI |
| USB for the touch controller | USB-A host ports | USB-C (OTG) / expansion board |
| Mainline Linux display support | longer established (H6) | recent; check the image's kernel |

Start with the **Orange Pi 3 LTS**: the panel's HDMI and USB cables plug in
without adapters, and H6 display support has been in mainline Linux for
longer. The Zero 2W is closer to the target size; repeat the bring-up there
once the 3 LTS works, preferably the 2 GB or 4 GB model (1 GB is too little
for Firefox next to Gnumeric). Neither is the production SoC. Record the
exact board, RAM, image and kernel with every result (`pda-device info`
prints them).

## 2. Debian 13 on the board

Use a Debian 13 (trixie) arm64 image for the board, e.g. Armbian's
minimal/server trixie image *(board: confirm the image boots and HDMI shows
the console)*. Then, on the board with network access:

```bash
sudo apt-get install -y git
git clone https://github.com/arispen/pda-shell.git && cd pda-shell
sudo ./scripts/setup-debian.sh --device   # Xorg, libinput, Openbox, the programs, build tools
```

`--device` installs the real X server (`xserver-xorg-core`, modesetting
driver, `xserver-xorg-input-libinput`), `xauth`, and touchscreen tools
(`xinput`, `evtest`, `libinput-tools`) instead of Xephyr/Xvfb.

**Memory for the build.** A release build of the four GTK programs needs
more than 1 GB while linking. `install-device.sh` builds with one job; on a
1–2 GB board add swap first:

```bash
sudo fallocate -l 2G /swapfile && sudo chmod 600 /swapfile
sudo mkswap /swapfile && sudo swapon /swapfile
```

## 3. Install and start at boot

```bash
sudo ./scripts/install-device.sh --enable --samples
```

This builds the release binaries (as the checkout's owner), installs them
with the session scripts, config and icons under `/opt/pda-shell`, creates
`/etc/pda-shell/pda-shell.toml` (once), and installs
`/etc/systemd/system/pda-shell-session.service` for the user who ran sudo
(or `--user NAME`). `--enable` makes `graphical.target` the default and
starts the session on tty7 now and at every boot. `--samples` copies the
sample documents (the acceptance scenario's spreadsheet) to `~/Documents`.

Without systemd, from a console login on the board (with a USB keyboard):

```bash
./scripts/device-session.sh                       # landscape, as configured
./scripts/device-session.sh --orientation portrait
```

`dev-session.sh` stays the development session (Xephyr window or Xvfb in a
desktop); the device session never uses either.

### How the session starts

`device-session.sh` starts, in order: Xorg on the session's virtual
terminal (rootless: the systemd unit opens a logind session with
`PAMName=login` on tty7, so logind hands Xorg the display and input
devices), an access cookie (`xauth`), the display plan (below), screen
blanking, the login session's D-Bus bus (or a private one), Openbox and
pda-shell. It logs each step with timings to
`~/.local/state/pda-shell/logs/latest/` (`session.log`, `shell.log`,
`openbox.log`, `xserver.log`, `startup.txt`, the xrandr output it saw); the
ten most recent sessions are kept.

| Event | What happens |
|---|---|
| Start > End session | Programs are closed the normal way, each may ask about unsaved work; when all are gone the session ends with status 0, the X server stops and systemd starts a fresh session after 2 s. |
| pda-shell crashes | Restarted after 1 s; programs keep running and reappear in the switcher and title. After 5 crashes within 60 s the session ends with status 1. |
| Openbox exits | Restarted; windows stay. |
| X server exits | The session ends with status 1; systemd restarts it (at most 5 times in 2 minutes, then it stays down and the text consoles remain usable). |
| `systemctl stop`, shutdown | Every program of the session is terminated (no save prompts), then the X server. |

## 4. Display: size, orientation, other panels

Nothing is hard-coded to 1024×600. At start, `pda-device display` reads
`xrandr --query` and the `[device]` section of the config and picks:

- **output**: `[device] output`, else the primary, else the first connected
  output;
- **mode**: `[device] mode` (e.g. `800x480`) if the output offers it, else
  the panel's preferred mode;
- **rotation**: none if `[device] orientation` matches the mode's own shape;
  otherwise `rotate` (`left` or `right`). `native` keeps the panel as it is.

The shell then lays out whatever size the screen really has. Per start,
`--orientation`, `--mode` and `--output` override the config. The plan, the
EDID size and the resulting pixels per inch are in `session.log`.

Panels that report no or a wrong EDID mode *(board)*: force the mode for
the kernel console and X with the DRM `video=` parameter, e.g. in Armbian's
`/boot/armbianEnv.txt`: `extraargs=video=HDMI-A-1:1024x600@60D`, then set
`[device] mode = "1024x600"`.

Screen blanking: `[device] screen_off_minutes` (DPMS; any tap wakes the
screen). The panel's backlight is not software-controlled over HDMI;
`pda-device info` lists `/sys/class/backlight` devices if the board has any.

## 5. Power, backlight, suspend: what exists

`pda-device info` reports, read-only: board, RAM, OS and kernel, CPU
governor, `/sys/class/backlight`, the sleep states the kernel offers
(`/sys/power/state`, `mem_sleep`), power supplies (batteries) and input
devices with a power key. What the session does with them in POC 0.4:

| Function | Status |
|---|---|
| Screen off when idle | DPMS via `xset`, configurable; a tap wakes it. |
| Power key | Handled by systemd-logind (`HandlePowerKey`, default poweroff) if the board has one; the shell adds nothing. |
| Backlight | Not available over HDMI on the prototype panel; nothing to drive. |
| Suspend/resume | **Not claimed.** List the board's sleep states with `pda-device info` and try `systemctl suspend` by hand *(board)*; USB touch wake-up and HDMI after resume are the expected blockers. |
| Battery | None on the prototype (external USB power). The status bar shows a battery only if `/sys/class/power_supply` has one. |

## 6. Measuring

```bash
./scripts/device-bench.sh --attach     # on the board, inside the running session (as its user)
```

Prints a Markdown report: `pda-device info`, startup times of the current
session, idle memory and CPU, Gnumeric and Firefox launch latency with the
sample documents, active memory. Add it to [test-results.md](test-results.md)
with the panel, resolution, power mode/governor and how the board was
powered. Power draw: an inline USB power meter on the board's supply, read
with the screen on and idle (Today), while editing in Gnumeric, and with
the screen blanked; keep external measurements apart from any battery-life
estimate. `--simulate` runs the same report in a virtual session (e.g. the
container) to compare changes, never as device evidence.

## 7. Troubleshooting

| Symptom | Check |
|---|---|
| Black screen or console only | `journalctl -u pda-shell-session -b`; `~/.local/state/pda-shell/logs/latest/session.log` and `xserver.log`. |
| "Only console users are allowed to run the X server" | Xorg was started outside a logind session on its VT (e.g. over ssh). Use the systemd unit, or log in on the console. |
| "no screens found" in `xserver.log` | `ls /dev/dri` must show `card0`; the image's kernel lacks the display driver for this SoC (see section 1). |
| Wrong resolution | `xrandr-before.txt` in the log directory shows what the panel offered; set `[device] mode`, or the `video=` parameter above. |
| Session restarts in a loop | `session.log`: "giving up" after 5 shell crashes; `shell.log` has the reason. systemd stops after 5 restarts in 2 minutes. |
| Get out of the session with a keyboard | Ctrl+Alt+F1 for a text console; `sudo systemctl disable --now pda-shell-session` to stop starting it at boot. |
| Touchscreen does nothing | `sudo libinput list-devices` and `sudo evtest` must show the USB controller; see [hardware.md](hardware.md). |

# Known limitations and status

Last updated: 2026-10-22 (end of Milestones 1–2). Everything marked **done** was
exercised in the running implementation (nested Xephyr/Xvfb + Openbox on
Debian 13 arm64, in the dev container) and is covered by
`scripts/smoke-test.sh` unless noted otherwise. Nothing here has been tried on
the physical 5" resistive display yet.

## Status by milestone

| Milestone | Item | Status |
|---|---|---|
| 1 | `setup-debian.sh` installs build + session + POC apps | **done** (used to build the container image) |
| 1 | `dev-session.sh --orientation portrait/landscape` (Xephyr) | **done** |
| 1 | Headless variant (`--headless`, Xvfb) for tests | **done** |
| 1 | Isolated HOME/XDG dirs, own D-Bus bus, own display | **done** |
| 1 | Clean shutdown of every session process | **done** (tagged-process sweep, checked by smoke test) |
| 1 | Logs per session in `.dev-session/logs/<stamp>-<orientation>/` | **done** |
| 2 | Status bar: Start, active title, clock | **done** |
| 2 | Status bar: network / battery indicators | **done** for real sysfs data; hidden when there is none (container has wired network, no battery) |
| 2 | Today: date/time, shortcuts, recent documents, system info | **done**; recent documents list only fills once apps record files (empty in a fresh session) |
| 2 | Start menu: installed apps, search, launch, system actions | **done** |
| 2 | Command bar: Today, switcher, context actions, keyboard toggle | **done** |
| 2 | Apps maximize between the bars | **done** (Openbox rule + struts; verified geometry = work area) |
| 2 | Window tracking, switching, return to Today, close active | **done** |
| 2 | End session closes apps gracefully, then exits | **done** |
| 4 | Gesture recognizer (tap / double tap / drag / long press, thresholds) | **done** as a tested library; only long press is wired into shell widgets |
| 4 | Long press → context menu with progress ring, in the shell | **done** (Today shortcuts, recent documents, Start menu entries) |
| 4 | System-wide long press for third-party apps | **not started** (separate component, needs real touch device) |
| 5 | Onboard show/hide from the command bar, positioned above the bar, work area shrinks | **done** (manual check, screenshots); typing into apps with it **not yet verified** |
| 3, 5–7 | Real-app document round trip, keyboard editing, visual pass, manual usability test | **planned** |

## Known issues and limitations

### Window management / X11
- **GTK 4.18 deprecates its X11 backend** (removal expected in GTK 5). The shell
  uses `gdk4-x11` only to get X window IDs (`src/platform/x11/gtk_glue.rs`).
  This is fine for GTK 4.x but is the main long-term risk of the Rust + GTK4 +
  X11 stack. Possible future paths: a Wayland compositor with
  layer-shell (e.g. labwc + `gtk4-layer-shell`), keeping the platform layer
  boundary as it is.
- **Openbox must not have unmodified `Frame`-context mouse bindings**: they grab
  every click on every window and the dock bars then never receive presses.
  `config/openbox/rc.xml` documents this.
- GTK 4 sets WM_CLASS to `pda-shell`/`pda-shell` (lowercase class). Openbox
  rules must match `class="pda-shell"`.
- Openbox's focus-stealing prevention keeps focus in the app under the Start
  menu; the shell explicitly activates the menu with `_NET_ACTIVE_WINDOW`
  (source = pager) when it maps.
- Popovers are drawn flat (no shadow) because there is no compositor; the
  shadow area would otherwise render black.
- Fullscreen applications (e.g. F11 in Firefox) cover the bars; there is no
  way back without the app's own controls. Not addressed yet.
- Dialogs keep Openbox decorations so they can be moved; they are not
  constrained to the work area beyond what Openbox does.
- Orientation is fixed per session. Xephyr cannot rotate at runtime; the
  shell computes its layout once at start. Runtime rotation (RandR) is future
  work and must not restart the session.

### Shell
- The keyboard toggle state can go stale if the user hides Onboard with its
  own close key; the shell does not yet read Onboard's `Visible` property back.
- Onboard's default layout has no Esc key; layout choice is Milestone 5 work.
  Onboard's `at-spi` features need `gir1.2-atspi-2.0` (now in the setup
  script; auto-show is not used).
- Window icons in the switcher are matched from WM_CLASS to desktop entries
  heuristically; unknown classes get a generic icon.
- The "Refresh" context action re-reads applications and recent documents;
  application installs are not watched automatically.
- Typed search in the Start menu was tested with synthetic key events, not
  with Onboard.
- No double-tap or drag behaviour is attached to shell widgets yet (GTK's own
  handling applies; lists scroll with scrollbars, not by dragging).

### Development environment
- On non-Debian hosts (this POC was developed on macOS) the session runs in
  the Debian 13 container (`scripts/container.sh`); viewing it interactively
  needs a VNC client (`scripts/container.sh vnc portrait`). The Xephyr window
  path itself was exercised inside the container on an Xvfb "host" display.
- Readability at 1024×600 on a 27" monitor proves nothing about a 5" panel
  (~237 ppi). All sizes are in `config/pda-shell.toml` for tuning on the device.

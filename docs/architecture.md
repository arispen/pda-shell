# Architecture

pda-shell is a set of shell surfaces (two bars, the Today screen, the Start
menu) running on top of an unmodified X11 server and Openbox. It does not
manage windows itself; it reserves screen edges and asks the window manager
for things through standard EWMH messages.

```text
            ┌───────────────────────── X server (Xephyr / Xvfb / real) ─┐
            │  Openbox ── manages, maximizes, focuses, stacks windows   │
            │     ▲  _NET_WORKAREA, _NET_CLIENT_LIST, _NET_ACTIVE_WINDOW│
            │     │  ▼ _NET_ACTIVE_WINDOW / _NET_CLOSE_WINDOW /         │
            │     │    _NET_SHOWING_DESKTOP client messages             │
 pda-shell ─┼─ platform::x11 (x11rb) ── watcher thread ── WindowSnapshot │
   (GTK 4)  │                                                           │
            │  Thunar, Gnumeric, Mousepad, … (unmodified)   Onboard     │
            └───────────────────────────────────────────────────────────┘
```

## Modules

| Module | Responsibility | Depends on GTK? | Depends on X11? |
|---|---|---|---|
| `config` | TOML design tokens, input thresholds, Today contents | no | no |
| `shell::layout` | Orientation and every rectangle (bars, work area, menu, keyboard) | no | no |
| `shell::state` | State machine: `Event` in, `Effect` out | no | no |
| `shell` (mod) | Controller: runs effects, refreshes UI | yes | via `platform` |
| `apps` | Desktop entries via GIO, search ranking, launching | GIO only for loading | no |
| `input::gesture` | Stylus gesture recognizer (tap, double tap, drag, long press) | no | no |
| `input` (mod) | Feeds the recognizer from GTK, draws long-press progress | yes | no |
| `keyboard` | Onboard process, GSettings placement, D-Bus show/hide | GIO | no |
| `platform` | Neutral types (`WindowId`, `AppWindow`, `WindowSnapshot`, `SurfaceRole`) and the `WindowSystem` trait | no | no |
| `platform::x11` | EWMH via `x11rb`: roles, struts, activation, window watcher; XSETTINGS manager and `Xft.dpi` for programs (`settings.rs`) | `gtk_glue.rs` only | yes |
| `ui::*` | Status bar, command bar, Today, Start menu, Programs and Settings screens (hosted in a stack on the Today desktop window), shell icons | yes | no |

All X11 code is below `src/platform/x11/`. The single GTK↔X11 contact point
is `gtk_glue::xid()`, which returns the X window ID of a realized GTK window.

## Event flow

1. A widget or the window watcher produces a `Command` (UI-level) or an
   `Event` (state-machine level).
2. `Shell::dispatch` feeds events to `ShellState::handle`, which returns
   `Effect`s such as `Activate(id)`, `ShowDesktop(true)`, `Launch(id)`,
   `ShowPage(page)` (Today, Programs, Settings, About, …; `Event::Back` is
   the command bar's OK).
3. `Shell::run` executes each effect against `X11`, `apps` or `Keyboard`.
4. The window manager reacts; the watcher thread sees property changes and
   sends a new `WindowSnapshot`, which comes back as `Event::Windows`.
5. `Shell::refresh_ui` redraws the title, switcher, context actions.

Keeping decisions in `ShellState` means the behaviour (what Today means, when a
new window is brought forward, when ending the session may quit) is unit
tested without a display; `tests/usability_flow.rs` replays the manual test
script against it, and `scripts/app-workflow-test.sh` runs it end to end with
real programs.

## Shell surfaces and the window manager

GTK 4 has no API for window types, struts or positions, so the shell sets
them on the X window itself in the `realize` handler, before the window is
mapped (`ShellWindow::new`, `X11::apply_role`):

| Surface | `_NET_WM_WINDOW_TYPE` | Extras |
|---|---|---|
| Status bar | `DOCK` | `_NET_WM_STRUT(_PARTIAL)` top |
| Command bar | `DOCK` | strut bottom; grows by the keyboard height while Onboard is shown |
| Today | `DESKTOP` | sized to the work area, below all apps |
| Start menu | `UTILITY` | `_NET_WM_STATE_ABOVE`, activated explicitly when shown; shrinks with the keyboard |
| Exit full screen | none (override-redirect) | not managed, so it can stay above full-screen programs; re-raised every 400 ms while shown |

Every surface is then moved to its layout rectangle with `ConfigureWindow`
(once before map, again on map in case the WM placed it).

Openbox computes `_NET_WORKAREA` from the struts. The shell checks it against
its own layout at start and logs a warning if they differ. Application windows
are maximized into the work area by the Openbox rule in
`config/openbox/rc.xml` (`type="normal"` → maximized, undecorated); the shell
does not resize other programs' windows.

"Today" is EWMH *show desktop* (`_NET_SHOWING_DESKTOP`): Openbox hides the
application windows, the desktop-type Today window is what remains, and
activating any application leaves the mode again. Applications keep running
and keep their state.

Full screen: the watcher reports `_NET_WM_STATE_FULLSCREEN` per window. With
`[windows] fullscreen = "escape"` the shell shows the "Exit full screen"
surface while the program in front is full screen and removes the state with
a `_NET_WM_STATE` client message when it is tapped; with `"prevent"` it
removes the state as soon as a window enters it.

Dialogs: the watcher reads each dialog's frame (`_NET_FRAME_EXTENTS`) and
`WM_NORMAL_HINTS` minimum size and, if it does not lie inside
`_NET_WORKAREA`, sends `_NET_MOVERESIZE_WINDOW` with the result of
`layout::fit_within` (shrink, move inside; a dialog that cannot shrink keeps
its bottom-right corner, where GTK puts the buttons, on screen). This
happens when a dialog appears, when it resizes itself and when the work area
changes, never after the user moves it. Windows that cannot fit are logged
with a `geometry:` prefix.

On-screen keyboard: the command bar's strut grows by the keyboard height
while Onboard is shown, so Openbox shrinks maximized programs; Today and the
Start menu are resized by the shell. Onboard's `Visible` property
(`PropertiesChanged` on its D-Bus object) and its bus name are watched, so
hiding it with its own key or Onboard exiting releases the space and resets
the toggle. Onboard is started through `scripts/bin/pda-onboard`, which works
around Onboard 1.4.1's broken symbol keys on Python 3.12+.

Closing uses `_NET_CLOSE_WINDOW`, so each application can ask to save. "End
session" closes every application this way and only quits when all windows
have gone; if some remain after 15 s (e.g. a save prompt) the session stays
and the status bar says "Some programs are still open".

## Stylus input

Two layers, deliberately separate:

1. **Inside the shell** (implemented): `input::attach_long_press` feeds GTK
   pointer events into `gesture::Recognizer`, draws the Pocket PC style ring
   of dots after 150 ms, fires the context menu at 600 ms (configurable), and
   claims the event sequence so the widget's own click does not also fire.
   Mouse right-click opens the same menu.
2. **System-wide** (future, hardware milestone): a small daemon reading the
   touchscreen's evdev device, running the same `gesture::Recognizer`, and
   emitting left/right clicks through `uinput`. The recognizer has no GTK or
   X11 dependency for this reason. During development it must only grab the
   touchscreen device, never the developer's mouse.

## Development session

`scripts/dev-session.sh` starts, in order: an X server (Xephyr window, or Xvfb
with `--headless`), a private D-Bus session bus, Openbox with the repository's
`rc.xml`, and the shell. HOME and the XDG directories point into
`.dev-session/home`, so nothing is read from or written to the developer's
real configuration. Every process inherits `PDA_SESSION_TAG`; on exit the
script terminates all processes carrying that tag (including re-parented and
D-Bus-activated ones), then kills stragglers after 3 s.

## Feasibility findings (Milestone 1)

Verified in the running system before building the UI:

- GTK 4.18 windows on X11 accept EWMH type/state/strut properties set at
  realize time; Openbox 3.6.1 honours them: `_NET_WORKAREA` became exactly
  `0,24,600,974` (portrait) and `0,24,1024,550` (landscape).
- Openbox maximizes ordinary applications into that area and removes their
  decorations through per-application rules.
- GTK popovers (switcher, context menus) open outside the 24–26 px bars as
  override-redirect windows.
- Blocker found and fixed: an unmodified `Frame` mouse binding in `rc.xml`
  swallowed all clicks on dock windows.

No framework change was needed. The main risk is GTK's X11 backend
deprecation, recorded in `known-limitations.md`.

## Portability

Nothing is tied to a SoC or display: screen size comes from the GDK monitor,
all measurements from the config file. The POC was built and tested on
arm64 (Debian 13 container on Apple silicon); the same scripts target amd64.
Dependencies are pure Rust (`x11rb`) or Debian-packaged libraries.

# Known limitations and status

Last updated: 2026-10-25 (POC 0.3: Pocket PC 2003 SE UX, P0, P1 and P2 of
issue #4; POC 0.2 sections below unchanged unless noted). Nothing here has
been tried on the physical 5" resistive display yet; everything ran in the
nested X11 session (Xvfb + Openbox, Debian 13 arm64 in the dev container).

Status words used below:

- **automated**: exercised by `scripts/app-workflow-test.sh` (real programs,
  pointer and on-screen keyboard only) or `scripts/smoke-test.sh`, in both
  orientations, on every run.
- **demonstrated**: done once in the running session and captured in a
  screenshot, not re-checked automatically.
- **unit-tested**: decision logic covered by `cargo test`, not by a running
  session.
- **unverified**: not tried, or only possible on the real hardware.

There was no human at a physical screen. The "manual pointer-only test" of
the issue was replayed by the workflow test with synthetic pointer taps; see
[test-results.md](test-results.md).

## POC 0.3 status (issue #4)

| Area | Item | Status |
|---|---|---|
| P0 scaling | `pocketpc` profile (22 px text, 46 px targets, 48/50 px bars), shipped default | **automated** (smoke and workflow tests run with it), **unit-tested** (token ranges) |
| P0 scaling | Programs scaled session-wide: XSETTINGS `Xft/DPI` 144 and `Gtk/FontName` Tahoma 10, `Xft.dpi` resource | **demonstrated** (screenshots), **unit-tested** (XSETTINGS encoding) |
| P0 scaling | Firefox exception `ui.textScaleFactor = 115` so it fits 600 px | **automated** (fills the work area in portrait) |
| P0 Today | Date, Calendar / Tasks / Messages empty states, Documents, Open programs, Favorites; system info moved to Settings → About | **automated** (documents, open programs, favorites), **demonstrated** (layout) |
| P0 Start | Drop-down with recent strip, Today, favorites, Programs, Settings, Find, End session | **automated** (launch by typing, Programs, Settings, End session) |
| P0 Programs / Settings | Icon grids, Linux tools, About, Screen, Network, Linux settings, OK navigation | **automated** (Settings → About → OK, Programs → Linux tools), **unit-tested** (page navigation) |
| P0 theme | Blue status bar, silver menu bar, Tahoma, original icons | **demonstrated** |
| P0 screenshots | Before/after at 600×1024 and 1024×600 | done: [screenshots.md](screenshots.md) |
| P0 | Assessment on the physical 5" panel | **not done** (no hardware; see below) |
| P1 | Program-specific commands in the command bar: published GtkApplication menu bars ([command-bar-api.md](command-bar-api.md)), generic File/Edit for other X11 programs, New on Today | **automated** (Mousepad saved through its own published File menu, Gnumeric through the generic File > Save, Notes and Files menus), **unit-tested** (menu model, overflow, shortcuts, key mapping) |
| P1 | Compatibility defaults: Mousepad wraps lines and does not offer session restore; Firefox without the Privacy Notice tab and tab-bar window buttons | **demonstrated**; installed by `setup-debian.sh` (dconf, `/etc/firefox-esr`) |
| P2 | Native Files and Notes (`pda-files`, `pda-notes`), Thunar and Mousepad under Linux tools | **automated** (New > Note, typing, Done, note on disk, Files lists and navigates, switcher with native and Linux programs, End session closes them) |
| P2 | Built-in Pocket PC input panel, Onboard kept as the default | **automated** (switch panels, type with Shift/Caps, work area shrinks, focus stays in the program), **unit-tested** (modifiers, layout) |

### What scaling made worse (reported, not silently accepted)

Programs' text is 1.5× larger, so dialogs laid out around text grow:

- **Gnumeric Save As now needs at least 876×428 px** (POC 0.2: 665×427).
  In portrait (600 px wide) it is aligned to keep Save and Cancel visible,
  as before, but now **278 px** of its left side are off screen (POC 0.2:
  67 px): the "Name" label, the places sidebar and the start of the name
  field. Saving with the current name works; typing a new name in portrait
  is not practical. In landscape it fits the width; with the keyboard shown
  (1024×302 work area) it is aligned to its bottom edge and the name field
  is under the status bar, as in POC 0.2.
- **Mousepad "Save Changes"** is 602 px wide: 2 px wider than portrait; it is
  shifted 2 px left, both buttons visible.
- **Landscape with the keyboard** leaves 1024×302 px (POC 0.2: 1024×319 with
  the compact bars). The landscape keyboard was reduced from 42 % to 40 % of
  the work area (200 px, four rows of 50 px keys) so Gnumeric (minimum
  height 294 px at 144 dpi) still fits above it.
- **Firefox** at the session's 150 % needed 760 px; the 115 % exception
  above fixes the portrait fit at the cost of smaller text than other
  programs.

Possible next steps: a per-program DPI (GTK 3 honours `GDK_DPI_SCALE`, but
programs opened from Thunar inherit Thunar's environment, not the shell's),
or the native Files frontend (P2) so desktop file choosers are needed less.

### Physical panel

Nothing in POC 0.3 was judged on the 5" resistive panel. The sizes in mm
are computed (Settings → System → Screen shows them live); readability,
stylus accuracy with 46 px targets and the 1.5× program text must be
checked on the device before the tokens are final. Record findings in
[test-results.md](test-results.md).

### P1 / P2 limitations

- The command bar shows a program's own menus only if it publishes a
  GtkApplication menu bar. Thunar and Gnumeric do not, so they get the
  generic File/Edit commands; their own menus stay in their windows.
- Generic commands are keyboard shortcuts: a program that does not use
  Ctrl+S for Save (rare) does something else or nothing.
- Mousepad shows its menu bar both in its window and in the command bar.
  Hiding it in the window is a Mousepad setting the shell does not change.
- GTK 4's own popover menus expose their items to accessibility without
  any text; the shell therefore draws command-bar menus itself. Other GTK 4
  programs' popover menus are affected the same way (GTK issue, not ours).
- Files: no copy/move between folders, no multiple selection, no search;
  Rename and Delete are on the long press. Opening documents uses the
  default programs (Gnumeric, Atril, Mousepad, Firefox).
- Notes: plain text only (no ink, no formatting), no recording; notes are
  `.txt` files in `~/Documents/Notes`, titled by their first line.
- Built-in input panel: one US layout, no word suggestions, no
  handwriting; non-ASCII characters are typed through a spare keycode
  (works in X11, not tested with every program). Its keys are 43 px wide
  in portrait (14 per row on 600 px), below the 46 px target; Onboard
  remains the default until it is judged on the panel.
- Neither input panel appears by itself when a text field gets the focus.

### Smaller POC 0.3 limitations

- The Start menu leaves out the favorites rows (they stay in its recent
  strip) when the full menu would not fit, e.g. landscape (502 px work
  area) or with the keyboard shown, so End session never scrolls out of
  sight.
- Calendar, Tasks and Messages have no providers; they show what is
  missing, not sample data.
- Toolbar icons inside programs keep their 16/24 px size; only text scales.
- GTK 4 reports AT-SPI positions without the CSS padding of enclosing
  boxes (about 16 px on Today items); the workflow test presses a fifth of
  the way into an element instead of 6 px from its edge.
- `setup-debian.sh` must be rerun (or the container image rebuilt) after
  updating: it now installs `fonts-wine` (Tahoma) and the Firefox
  preference file.
- The shell takes over the XSETTINGS selection; inside a full desktop
  session (not the nested dev session) that would override the desktop's
  settings daemon while the shell runs.

## Status by milestone

| Milestone | Item | Status |
|---|---|---|
| 1 | One-command session on Debian 13 (`dev-session.sh`), isolated HOME/bus/display, clean exit | **automated** |
| 2 | Bars, Today, Start menu, switcher, maximized apps, End session | **automated** |
| 3 | Realistic sample files: `.xlsx` with formulas and formatting, Markdown, 4-page PDF, local web page, folders six levels deep with long and non-ASCII names | **automated** (generated by `scripts/make-sample-files.py`) |
| 3 | Thunar, Gnumeric, Mousepad, Atril, Firefox ESR start as real processes and fill the work area | **automated** |
| 3 | File associations (`config/home/.config/mimeapps.list`): tapping a file in Thunar opens the right program | **automated** for xlsx, txt, pdf, html |
| 3 | Gnumeric edit → save → close → reopen round trip with a populated `.xlsx` | **automated**; saved values checked on disk with `ssconvert` |
| 3 | Mousepad edit → save → close → reopen round trip | **automated**; file checked on disk, reopened text read back |
| 3 | Atril multi-page PDF, page navigation | **automated** |
| 3 | Firefox with a real local page, typing into a form | **automated** |
| 3 | Screenshots of every program in 600×1024 and 1024×600 | **demonstrated** (captured by the workflow test) |
| 3 | Dialog geometry: dialogs moved/shrunk into the work area; windows whose minimum size exceeds it are logged | **automated** (Save As), **unit-tested** (`fit_within`) |
| 3 | Full-screen recovery: "Exit full screen" button above full-screen programs (or `fullscreen = "prevent"`) | **automated** (Firefox F11 from the on-screen keyboard and back), **unit-tested** (prevent mode) |
| 5 | Typing through Onboard into Gnumeric, Mousepad, Firefox and the Start menu search | **automated** (numbers, formulas, punctuation, file paths, Shift) |
| 5 | Esc, Tab, Ctrl, Alt, arrows, F-keys on the keyboard (Onboard "Small" layout) | **automated** (Tab, Ctrl+A, F11, Return); Esc and arrows **demonstrated** |
| 5 | Keyboard shrinks the work area; apps, Today and the Start menu resize above it | **automated** (both orientations) |
| 5 | Shell's keyboard toggle follows Onboard's own hide key / Onboard exiting | **unit-tested**; D-Bus signal **demonstrated** in the log |
| 5 | Focus stays in the application when the keyboard is shown or hidden | **automated** (Gnumeric) |
| 5 | Automatic keyboard on text focus | **not done** (manual by design for the POC) |
| 6 | Density profiles compact / balanced / readable in `config/pda-shell.toml` | **automated** in compact; readable and balanced **demonstrated** (see [screenshots.md](screenshots.md)) |
| 6 | Physical size table for the 237 ppi panel | done: [ergonomics.md](ergonomics.md) |
| 6 | Today: date first, recent documents (incl. newest files in ~/Documents), open programs, then shortcuts | **demonstrated**, list contents **automated** |
| 6 | Start menu hides technical utilities and settings; search and "Other programs" still reach them | **unit-tested**, **demonstrated** |
| 6 | Switcher: larger rows with program name and "current" marker; "Close all" separated and needs a second tap | **automated** |
| 6 | Context menus near screen edges stay on screen | **automated** (left edge, right edge, low row) |
| 7 | Unsaved work: End session lets the program ask; nothing is discarded silently | **automated** (Mousepad "Don't Save" path; file unchanged) |
| 7 | End session exits with status 0 and leaves no processes | **automated** |
| 4 | System-wide long press for third-party apps, touchscreen calibration | **not started** (needs the panel; out of scope) |

## Where the desktop-application approach breaks down

These are the problems found with real programs. Each is reproducible with
`scripts/app-workflow-test.sh` (screenshots in `screenshots/workflow/`) unless
noted.

### Dialog and window geometry
- **Gnumeric's Save As needs at least 665×427 px** (GTK file chooser plus
  Gnumeric's format row). In portrait (600 px wide) it cannot fit. The shell
  logs it and aligns the dialog so its bottom-right corner (Cancel, Save)
  stays visible; the left 67 px (the "Name:" label and the start of the
  places sidebar) are cut off.
- **Landscape with the keyboard open leaves a 1024×319 work area.** The same
  dialog is then aligned to its bottom edge: Save and Cancel stay reachable,
  but **the file name field is under the status bar**. Typing a new file
  name in landscape needs the keyboard hidden first (which gives 1024×550 and
  the whole dialog), then shown again: an awkward but working sequence.
  This is the clearest case where a 600 px screen dimension plus an
  on-screen keyboard does not fit desktop dialogs.
- Dialogs keep their Openbox title bar so they can be dragged. The shell
  fits a dialog once when it appears, when it resizes itself, and when the
  keyboard changes the work area; it does not move a dialog back after the
  user drags it.
- Free-standing dialogs that set skip-taskbar (e.g. Mousepad's "restore the
  previous session?" question) float above Today without becoming the
  current program in the switcher. They are visible and usable.

### Applications
- **Onboard 1.4.1 on Debian 13 cannot type symbol keys.** Every key defined
  by a character (`.` `=` `*` `(` `:` `@` …) raised `SystemError: bad format
  char` in Onboard's C extension under Python 3.13; letters and digits
  worked. `scripts/bin/pda-onboard` starts the unmodified package with a
  small fix (character → X keysym → Onboard's working keysym path). Without
  it a spreadsheet formula cannot be entered at all.
- Symbols need a layer switch on the "Small" layout ("12?" then the key);
  a formula like `=SUM(B2:B13)*2` costs about 25 taps.
- Mousepad does not wrap long lines by default; in portrait a long line
  scrolls sideways (see `screenshots/apps/portrait-15-end-session-unsaved.png`).
- Gnumeric shows formulas with function names in the case they were typed
  (`=sum(…)`); cosmetic.
- Gnumeric's toolbar shows more controls in landscape (a second text combo
  appears next to the chart button); tests must not assume one text field.
- Firefox ESR opens a "Privacy Notice" tab on first start, draws its own
  minimise/maximise/close buttons in the tab bar (they work, but are
  redundant with the command bar), and logs a content-sandbox warning in the
  container (no user namespaces); pages render normally.
- Firefox only exposes its page to AT-SPI when accessibility is requested
  (`GNOME_ACCESSIBILITY=1`); the workflow test sets it. No effect on users.
- Mousepad offers to restore the previous session after it was killed
  (e.g. the dev session was stopped with Ctrl+C instead of End session).
- Thunar logs that no thumbnailer service exists; harmless.

### Stylus interaction
- **Tooltips appear after taps.** With a stylus the pointer stays where the
  last tap was, so tooltips pop up over toolbars ("Go to the home folder").
  Since POC 0.3 the shell and its native programs set no tooltips at all,
  and GTK's own tooltips are drawn as a flat light box: without a
  compositor GTK's translucent tooltip rendered as a black box, blank when
  it inherited a bar's text colour. GTK 3 programs get the same look from
  `config/home/.config/gtk-3.0/gtk.css` (session HOME default). Programs'
  tooltips still appear; GTK 3 has no switch to turn them off. The workflow
  test parks the pointer on the status bar before each screenshot.
- The "Exit full screen" button covers 152×24 px of the full-screen
  program's top-right corner while shown.
- In full screen the keyboard stays on top of the program but the program
  does not shrink (full-screen windows ignore reserved space).
- Long press works inside the shell only; third-party programs get no right
  click from the stylus yet (out of scope until the panel arrives).

### Window management / X11
- **GTK 4.18 deprecates its X11 backend** (removal expected in GTK 5). The
  shell uses `gdk4-x11` only to get X window IDs (`src/platform/x11/gtk_glue.rs`).
  Possible future path: a Wayland compositor with layer-shell (labwc +
  `gtk4-layer-shell`), keeping the platform layer boundary.
- GTK 4 reports widget positions to AT-SPI relative to the window only; the
  test driver adds the window position from X.
- **Openbox must not have unmodified `Frame`-context mouse bindings**: they
  grab every click on every window and the bars never receive presses.
- GTK 4 sets WM_CLASS to `pda-shell`/`pda-shell` (lowercase class); Openbox
  rules must match `class="pda-shell"`.
- Openbox's focus-stealing prevention keeps focus in the app under the Start
  menu; the shell activates the menu explicitly with `_NET_ACTIVE_WINDOW`.
- Popovers are flat (no shadow) because there is no compositor.
- Orientation is fixed per session; runtime rotation (RandR) is future work.

### Shell
- Window icons in the switcher are matched from WM_CLASS heuristically.
- (POC 0.2) Application installs were not watched. Since POC 0.3 the shell
  follows installs and removals (GIO app-info monitor); Refresh is gone.
- No double-tap or drag behaviour is attached to shell widgets (GTK
  defaults; lists scroll with scrollbars).
- (POC 0.2) The density profile changed the shell only. Since POC 0.3
  programs are scaled through `app_dpi` (see [ergonomics.md](ergonomics.md)).

### Development environment
- On non-Debian hosts the session runs in the Debian 13 container
  (`scripts/container.sh`); interactive viewing needs a VNC client.
- Readability on a desktop monitor proves nothing about the 5" panel; see
  [ergonomics.md](ergonomics.md) for how to judge screenshots.

## Needs the real hardware

- Final readability and target-size judgement (profiles are candidates).
- Resistive touch accuracy, especially near the edges and corners where the
  bars are; calibration matrix per orientation.
- Whether 38–40 px targets are needed with a stylus or 24–32 px suffice.
- Performance of Firefox and Gnumeric on the chosen SoC.
- Tooltip behaviour with a real stylus pointer.

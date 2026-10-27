# POC 0.4 test results: system-wide stylus input (issue #7, Milestone 4)

Date: 2026-10-27. Same container as the device session below. **No pen and
no panel**: the daemon ran on the Docker VM's real Linux kernel (6.8,
arm64) against touchscreens and mice created with uinput.

| Test | Command | Result |
|---|---|---|
| Unit and integration tests | `cargo test` | 149 + 1 passed |
| Stylus daemon, real kernel, synthetic devices | `./scripts/container.sh uinput ./scripts/stylus-test.sh` | 24 passed, 0 failed (two runs) |
| Device session incl. calibration screen | `./scripts/device-test.sh` | 80 passed, 0 failed |
| Shell smoke test (regression) | `./scripts/smoke-test.sh` | 68 passed, 0 failed |

`stylus-test.sh` plays gestures on a fake 4096×4096 resistive touchscreen
(`scripts/lib/fake_input.py`) and reads what the daemon's virtual pointer
emits: a tap is one left click at the tapped position; a 900 ms hold is a
right click and no left click, also with ±4 px jitter; a short tap with
jitter stays a left click; a drag presses where the stylus went down and
releases where it left; a drag held still for 1.5 s never becomes a right
click; a double tap is two left clicks; unplugging the touchscreen mid-drag
releases the left button, and taps work again after replugging; while the
daemon runs another reader of the touchscreen gets nothing (EVIOCGRAB); a
fake USB mouse keeps working and is never taken; `pda-device inputs`
refuses the daemon's own pointer and the VM's "Apple Virtual USB
Digitizer" (absolute and relative axes); SIGTERM exits with 0, the virtual
pointer disappears and the touchscreen is readable again; with
`right_click = false` a hold is a long left press.

The calibration screen (`pda-device calibrate`) was driven with taps 20 px
right of and 10 px below each cross: the saved matrix shifts by −0.0186
and −0.0167 (20/1024 and 10/600), residual 0.0 %; Cancel or SIGTERM
restores the previous file.

Found and fixed while testing:

- An empty `XDG_CONFIG_HOME` was taken as a relative directory: the
  calibration landed in the current directory. Empty now means unset.
- Closing the calibration window from outside (SIGTERM) left the identity
  calibration in place; it now quits normally and restores the previous one.

Not shown by these tests: whether X delivers the virtual pointer's right
click to Gnumeric, Firefox, Thunar and Files as a context menu (Xvfb has
no evdev input), calibration accuracy of the real resistive panel, and the
hold timing that feels right with a pen. Those are rows S2–S7 of
[device-checklist.md](device-checklist.md).

# POC 0.4 test results: device session (issue #7)

Date: 2026-10-27. Environment: the Debian 13 arm64 development container
on an Apple-silicon Mac (Docker VM with 2 CPUs and 1.9 GiB RAM, shared
with other containers), Xvfb, Openbox, GTK 4.18. **Simulated only**: no
Orange Pi and no panel were used. Board and panel results go in
[device-checklist.md](device-checklist.md) and below once they exist.

| Test | Command | Result |
|---|---|---|
| Unit and integration tests | `cargo test` | 129 + 1 passed |
| Device session at 1024×600, 600×1024, 800×480, crash recovery, crash loop | `./scripts/device-test.sh` | 72 passed, 0 failed |
| Shell smoke test, both orientations (regression) | `./scripts/smoke-test.sh` | 68 passed, 0 failed |
| Workflow test, both orientations (regression) | `./scripts/app-workflow-test.sh` | 161 passed, 5 failed: Firefox's page process crashed ("Tab crash reporter") in both orientations, see below |

`device-test.sh` runs the unchanged `device-session.sh` with
`--xserver Xvfb` (the only difference from the device: no Xorg, no VT, no
rotation). Per size it checks: the display plan and screen size in the log,
startup times written, bars at the real screen edges and Today filling the
work area (no fixed positions), Mousepad launched from Start filling the
work area; then the shell is killed with SIGABRT: it is restarted, maps its
bars again and shows Mousepad (with its unsaved text) as the current
program; End session makes Mousepad ask, the session waits, Don't Save
ends it with status 0, nothing tagged is left and the X server is gone. A
stand-in shell that always exits 3 makes the session give up after 5
crashes with status 1.

Startup in the container (debug build, Xvfb): X server 0.05–0.13 s, window
manager +0.13 s, shell bars mapped +0.12 s, about 0.3–0.45 s in total.
These are container numbers, not the board's.

Simulated measurement (`./scripts/device-bench.sh --simulate --runs 2
--idle 5`, debug build, the VM had about 0.5 GiB free): shell RSS 213 MiB,
whole session 313 MiB, idle CPU 0.0 % over 5 s, Gnumeric window after
0.14–0.18 s, Firefox after 0.6–0.9 s, Firefox 413–521 MiB with the page
open. Under that memory pressure the kernel reclaims pages, so Gnumeric's
RSS read 13–22 MiB (99 MiB when measured alone). The numbers show the
script works; they say nothing about the Orange Pi.

Workflow regression failures: the five failed checks are the Firefox web
form and full-screen steps. The shell log shows Firefox's window titled
"Tab crash reporter": its content process died while the VM had about
0.5 GiB free (two older development containers held 0.85 GiB of the 1.9
GiB). This slice changes no shell or Firefox code path; the same steps
passed in POC 0.3 (166/166). To be rerun with more memory.

Found and fixed while testing:

- Rust's runtime ignores the first SIGSEGV sent with `kill` (its
  stack-overflow handler expects the fault to repeat), so a "crash" by
  SIGSEGV did not end the shell; the test simulates crashes with SIGABRT.
- Linking four GTK programs in parallel was killed by the OOM killer in a
  1.9 GiB VM; `install-device.sh` builds with one job (small boards have
  the same limit).

# POC 0.3 P1 / P2 test results

Date: 2026-10-25. Same environment as below; the container image adds
`dconf-cli` and the program defaults (`config/dconf`, `config/firefox`).

| Test | Command | Result |
|---|---|---|
| Unit and integration tests | `cargo test` | 115 + 1 passed |
| Shell smoke test, both orientations | `./scripts/smoke-test.sh` | 68 passed, 0 failed |
| Workflow test, both orientations | `./scripts/app-workflow-test.sh --screenshots screenshots/pocketpc` | 166 passed, 0 failed |
| Captures | `./scripts/capture-screens.sh` | 19 screens per orientation |

What the workflow test now also checks, pointer only:

- Gnumeric (no published menus) saved through the command bar's generic
  **File › Save** (Ctrl+S); the saved values are checked on disk.
- Mousepad's own menus in the command bar (published menu bar; under
  **More** in portrait) and Mousepad saved through **its own File › Save**
  action over D-Bus; the file is checked on disk.
- Today **New › Note** opens Notes; Notes' own menus (Done, Edit) in the
  bar; switching to the built-in **Pocket PC keyboard** with the chooser;
  typing "PDA note!" with Caps and Shift; the work area shrinks and focus
  stays in Notes; **Done** lists the note; the file is on disk.
- **Files** opens from the favorites, shows its menus, lists the note in
  Documents/Notes, **Up** returns; the switcher lists native and Linux
  programs together; End session closes Notes and Files without asking
  while Mousepad still asks about unsaved text.
- Thunar is opened from **Programs › Linux tools**.

Found and fixed while testing:

- GTK 4 popover menus expose their items without any accessible text →
  command-bar menus are drawn by the shell as lists (also better targets).
- A program's menus load over D-Bus piece by piece (submenus, sections) →
  open menus re-render as parts arrive.
- Mousepad pads menu labels ("Save      ") → labels are trimmed.
- Mousepad's 14-row File menu did not fit landscape (502 px) → menus
  scroll and are capped at the space between the bars.
- The built-in panel re-raised itself over open menus → it is raised only
  when the window list changes.
- After a command-bar menu the keyboard focus could stay in the shell →
  the program in front is activated before typing.
- The Start menu, activated right after mapping, could be seen as an
  unknown active window, making the shell think Today was showing →
  unknown active windows keep the previous program (unit-tested).
- `dev-session.sh` started Openbox against a display that was not ready
  (twice in a row of runs): it now checks that its own X server answers
  and starts Openbox again (up to 5 times) until it manages the screen.

Still not done: any judgement on the physical panel.

# POC 0.3 test results

Date: 2026-10-24. Same environment as POC 0.2 below (Debian 13 arm64 dev
container, Xvfb at 600×1024 / 1024×600, Openbox, GTK 4.18), plus
`fonts-wine` (Tahoma) and the Firefox preference file. Profile: `pocketpc`
(the new default).

| Test | Command | Result |
|---|---|---|
| Unit and integration tests | `cargo test` | 97 + 1 passed |
| Shell smoke test, both orientations | `./scripts/smoke-test.sh` | 66 passed, 0 failed (one earlier run failed 2 checks on a Start-menu tap during the drop-down's first layout; a settle wait was added, then 2 clean runs) |
| Workflow test (POC 0.2 pointer-only flow), both orientations | `./scripts/app-workflow-test.sh --screenshots screenshots/pocketpc` | 130 passed, 0 failed |
| Shell screen captures | `./scripts/capture-screens.sh` | `screenshots/pocketpc/shell-*` |

No regression in the #2 pointer-only workflow: open/edit/save/reopen
documents, switch programs, dismiss the keyboard, unsaved edits are asked
about, End session exits cleanly with nothing left behind.

Changes the tests needed, all from the new UI rather than lowered checks:
bar sizes are read from the work area instead of assumed 24/26 px; Close
and End session are found by name (Close moved next to Windows, End session
is a Start-menu row); the smoke test now uses a fresh HOME (a stale Mousepad
"restore session" dialog in the shared one broke it); context-menu edge
presses go a fifth into the element (GTK 4 AT-SPI positions ignore CSS
padding); in landscape the web form is below the fold and is scrolled into
view with the wheel.

Found and fixed while testing:

- Firefox at 150 % text needed 760 px in portrait → exception
  `ui.textScaleFactor = 115` ([ergonomics.md](ergonomics.md)).
- Gnumeric at 144 dpi needs 294 px height; landscape with the keyboard left
  292 px → landscape keyboard 40 % instead of 42 % of the work area.
- The Start drop-down did not fit landscape (End session scrolled away) → it
  drops the favorites rows when it would not fit.
- Landscape Today wasted a third of the width on a separator → fixed.

Reported, not fixed: Gnumeric's Save As is 876 px wide at 144 dpi (was
665 px); in portrait 278 px of it are off the left edge. See
[known-limitations.md](known-limitations.md#what-scaling-made-worse-reported-not-silently-accepted).

Seen once, pre-existing: Openbox failed to open the display at session
start (`dev-session.sh` does not check that Openbox is alive after its
wait); the run's checks all failed and the next run passed.

**Not done: assessment on the physical 5" panel.** All of the above ran in
Xvfb. Text sizes, target sizes and the 1.5× program text are computed in
millimetres, not judged by a person with a stylus. Physical usability
findings: none yet; record them here when the panel is available.

# POC 0.2 test results and recommendation

Date: 2026-10-22. Environment: Debian 13 (trixie) arm64 dev container on an
Apple-silicon Mac, Xvfb at 600×1024 / 1024×600, Openbox 3.6.1, GTK 4.18,
Onboard 1.4.1, Thunar, Gnumeric, Mousepad, Atril, Firefox ESR from Debian.

## What was run

| Test | Command | Result |
|---|---|---|
| Unit and integration tests | `cargo test` | 83 + 1 passed |
| Shell smoke test, both orientations | `./scripts/smoke-test.sh` | 54 passed, 0 failed |
| Workflow test, compact, both orientations | `./scripts/app-workflow-test.sh` | 130 passed, 0 failed |
| Workflow test, readable, both orientations | `./scripts/app-workflow-test.sh --density readable` | 130 passed, 0 failed |
| Workflow test, balanced, portrait | `./scripts/app-workflow-test.sh --density balanced --orientation portrait` | 65 passed, 0 failed |

### How "pointer only" is guaranteed

After the session starts, the workflow test sends nothing but pointer
moves and left clicks (`xdotool mousemove`/`click`). Text is entered by
tapping keys of the real Onboard window; key positions come from Onboard's
own layout code fitted to the window size (`scripts/lib/pda_ui.py`).
AT-SPI is used only to locate widgets and read results; files are checked on
disk with `ssconvert` and `grep`.

What this does **not** replace: a person holding a stylus on the real panel.
The issue's manual pointer-only test was replayed by this automation in
both orientations; there was no physical screen to test with by hand.

### The replayed manual flow

Today → Thunar (shortcut) → Documents → six folders deep and back → tap
`Budget 2026.xlsx` → Gnumeric → keyboard on → Name box, Ctrl+A, `B2`,
`4321.5`, `A18`, `=SUM(B2:B13)*2`, a sentence with punctuation, Esc on a
discarded entry, arrow keys to B18, `5` → toolbar Save → Save As opened and
cancelled → keyboard off → switcher → Thunar → `notes.txt` → Mousepad →
keyboard on → typed line with `* = ( ) ; ~ /` → File ▸ Save → switcher →
Gnumeric → "Close all" armed but not confirmed → Thunar → PDF → Atril
page 2 of 4 → Thunar → `Web/index.html` → Firefox → form typed with Tab →
F11 from the Fn layer → "Exit full screen" → Today → close and reopen both
documents from Today (values checked) → long presses at the left edge, right
edge and a low row → unsaved Mousepad edit → End session → Mousepad asks →
Don't Save → session ends, status 0, nothing left running.

## Results by category

**Interaction** — passes. Every step above works with taps only. Single-tap
opening in Thunar (seeded setting) avoids double taps. Long press opens
context menus near both side edges and lower rows, and they stay on screen.
"Close all" needs a confirming second tap. One point of friction: symbols on
the on-screen keyboard need a layer tap each (≈25 taps for a short
formula).

**Readability** — not assessable without the panel. Compact text is
1.3 mm (12 px) on the 5" panel, about half the physical size of Pocket PC
text; readable is 1.7 mm. Applications draw 1.4 mm text regardless of the
shell profile. See [ergonomics.md](ergonomics.md).

**Dialog / window geometry** — partly fails, logged. All main windows fill
the work area in both orientations and resize when the keyboard opens.
Gnumeric's Save As (min 665×427) does not fit portrait width (left 67 px
cut, buttons kept visible) and, with the keyboard open in landscape (work
area 1024×319), its file-name field is pushed under the status bar. Mousepad
does not wrap long lines by default, so they scroll sideways in portrait.

**Input focus** — passes. Focus stays in the program when the keyboard is
shown or hidden; Firefox scrolls the focused field above the keyboard;
Gnumeric's edit row stays visible. The shell's keyboard toggle follows
Onboard's own hide key.

**Data editing** — passes. Spreadsheet values, formula result, text with
punctuation, Esc and arrow navigation verified in the saved `.xlsx`; the
Mousepad edit verified in the saved file and after reopening; unsaved work
is never written or dropped silently.

**Application compatibility** — passes with one workaround. All five
programs work unmodified. Onboard 1.4.1 on Debian 13 cannot type symbol keys
without `scripts/bin/pda-onboard` (Python 3.12+ incompatibility in its C
extension). Firefox needs nothing special; its first-run privacy tab and its
own window buttons are cosmetic.

## Recommendation

**Orientation: portrait first, keep landscape working.** Portrait gives
the programs 974 px of height (682 px with the keyboard), enough for
Gnumeric's toolbars plus 24 spreadsheet rows, and a keyboard with 42 px wide
keys. Landscape gives wider documents but only 550 px of height and 319 px
with the keyboard: about five spreadsheet rows, and desktop dialogs no
longer fit. Landscape is better for reading (PDF, web) without typing.

**Density: start device testing with `balanced`, compare with `readable`.**
Compact is denser than the applications next to it and its 1.3 mm text is
very likely too small at 237 ppi. Readable (16 px text, 38 px targets)
matches the applications' own toolbar targets and costs little space in
portrait; balanced (14 px, 32 px) is the denser candidate. Pick on the
panel; the profiles are one line of config.

**Adapt next:**
1. Application text size: a session-wide setting (`Xft.dpi` or GTK
   settings) tied to the density profile, since the shell cannot fix
   1.4 mm application text alone.
2. Dialogs vs. keyboard in landscape: shrink or float the keyboard while a
   dialog is open, or a keyboard height setting per orientation.
3. Keyboard: a custom Onboard layout with the symbols a spreadsheet needs
   (`= ( ) : * / + -`) on the main layer, and an "auto-hide on Return in
   dialogs" behaviour.
4. Seed application defaults for small screens: Mousepad word wrap,
   Firefox without the first-run tab and with its own window buttons off.
5. Tooltips after stylus taps.

**Needs the real hardware:** readability and target size, resistive touch
accuracy near the edges (the bars live there), calibration per orientation,
system-wide long press, performance of Firefox and Gnumeric on the final
SoC, and how tooltips behave with a real stylus pointer.

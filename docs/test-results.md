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

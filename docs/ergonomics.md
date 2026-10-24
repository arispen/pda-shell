# Ergonomics: physical sizes, density profiles and app scaling

Milestone 6 calibration notes, extended for POC 0.3 (the `pocketpc` profile
and session-wide scaling of programs). Every measurement below is computed for the
target panel, not for the monitor the screenshots are viewed on.

## The target panel

5" diagonal, 1024×600: √(1024² + 600²) = 1186.8 px over 5.0 in = **237 ppi**,
so **1 px ≈ 0.107 mm**.

A screenshot shown 1:1 on a typical 24–27" desktop monitor (92–110 ppi) is
**2.2–2.6× larger** than the same pixels on the device. To judge real size on
a desktop monitor, view screenshots at about 40–45 % zoom, or better, hold a
printout scaled to 237 ppi at reading distance. Readability can only be
signed off on the real panel (see [hardware.md](hardware.md)).

## Reference: Pocket PC

Pocket PC 2003 (3.5" QVGA, 240×320, ~114 ppi) drew 8 pt Tahoma at about
11 px, i.e. **≈ 2.4 mm**, and menu rows of about 16–18 px, i.e. **3.6–4 mm**.
That is the physical density the UX principles refer to, not its pixel
counts: on a 237 ppi panel the same physical sizes need roughly twice the
pixels.

## Shell tokens per profile

`config/pda-shell.toml` defines four profiles (`[density.*]`), selected with
`design.density` (the file selects **`pocketpc`**), `dev-session.sh --density
NAME` or `PDA_SHELL_DENSITY`. Each token stays independent; a profile only
overrides what it lists. Settings → System → Screen shows the active values
in pixels and millimetres.

| Token | compact | balanced | readable | **pocketpc** | compact mm | readable mm | **pocketpc mm** |
|---|---:|---:|---:|---:|---:|---:|---:|
| `font_px` (body text, em size) | 12 | 14 | 16 | **22** | 1.28 | 1.71 | **2.35** |
| `small_font_px` (paths, dates) | 10 | 11 | 13 | **18** | 1.07 | 1.39 | **1.93** |
| `title_font_px` (status bar) | 12 | 14 | 16 | **22** | 1.28 | 1.71 | **2.35** |
| `date_font_px` (Today date) | 16 | 19 | 22 | **26** | 1.71 | 2.35 | **2.78** |
| `top_bar_px` | 24 | 30 | 36 | **48** | 2.57 | 3.85 | **5.14** |
| `bottom_bar_px` | 26 | 32 | 40 | **50** | 2.78 | 4.28 | **5.35** |
| `icon_px` (bars, lists, Start menu) | 16 | 18 | 22 | **28** | 1.71 | 2.35 | **3.00** |
| `today_icon_px` (Today, Programs, Settings) | 24 | 28 | 32 | **44** | 2.57 | 3.42 | **4.71** |
| `row_px` (list rows) | 20 | 28 | 34 | **44** | 2.14 | 3.64 | **4.71** |
| `target_px` (bar buttons, switcher, menus) | 24 | 32 | 38 | **46** | 2.57 | 4.07 | **4.92** |
| `start_menu_width_px` | 300 | 340 | 380 | **400** | | | |
| `app_dpi` (programs, see below) | 96 | 96 | 96 | **144** | | | |

The `pocketpc` values are the starting hypotheses of issue #4 (body ~22 px,
secondary ~18 px, bars ~48/50 px, rows ~44 px, targets ~46 px, Today icons
~44 px). They match Pocket PC 2003's *physical* sizes: 8 pt Tahoma at
≈ 2.4 mm and menu rows of 3.6–4 mm on a 3.5" QVGA screen. They are to be
tuned with the stylus on the real panel; nothing here has been judged there.

Font sizes are em sizes; Tahoma's x-height is about 0.55 em, so 22 px text
has lowercase letters about 1.3 mm tall on the panel.

For comparison, print: 12 px on the panel is the em size of 3.6 pt type,
22 px of 6.7 pt. A newspaper uses 8–9 pt.

Derived layout rules (`shell::layout`):

- Icon grids (Programs, Settings, Favorites) use cells of
  `today_icon_px + 4 × font_px` (at least 90 px): **4 columns** in portrait
  and 7 in landscape with `pocketpc`; favorites on landscape Today use half
  the width.
- The Start drop-down is `start_menu_width_px` wide and as tall as its
  content, at most the work area (less while the keyboard is shown); longer
  content scrolls.
- Bars are capped at a quarter of the screen height.

## Applications: session-wide scaling

The shell's tokens are pixels in its own stylesheet; programs are scaled
separately, for the whole session, by `[design] app_dpi` and `app_font`:

- The shell owns the **XSETTINGS** selection and publishes `Xft/DPI`
  (`app_dpi`) and `Gtk/FontName` (`app_font`, "Tahoma 10"). GTK 3 programs
  (Thunar, Mousepad, Gnumeric, Atril, Onboard), GTK 4 programs and Firefox
  (through GTK) read them when they start.
- It also sets the `Xft.dpi` X resource for toolkits that only read
  resources (Xlib, Qt).
- Nothing is written to the user's configuration; the settings end with the
  shell. On a desktop that already runs an XSETTINGS manager the shell takes
  the selection over and logs it.

At 144 dpi a program's default 10 pt text is 20 px (2.1 mm), close to the
shell's 22 px body text. Measured in the session (`screenshots/pocketpc/`):

| Program | What scales | What does not | Notes |
|---|---|---|---|
| GTK 3 programs (Thunar, Mousepad, Gnumeric, Atril) | all text, and every widget sized by its text (menus, buttons, rows, tabs) | icons (16/24 px toolbar icons), fixed pixel geometry, scrollbar width | the scale is a font resolution, not a zoom: dialog layouts grow only as much as their text |
| Firefox ESR | text of the UI and pages (Firefox turns the DPI into a text scale; it keeps 1 device pixel per CSS px) | icons, page layout widths | **exception:** held at 115 % instead of 150 %, see below |
| Onboard | key labels | key geometry (follows the window) | |
| The shell | nothing: it uses pixel tokens | | `app_dpi` never changes the shell |

**Why not a blind 2× zoom** (`GDK_SCALE=2`): it halves the logical screen.
A 600×1024 portrait panel would become 300×512 logical pixels, and desktop
dialogs that already do not fit 600 px (Gnumeric's Save As needs ~665 px)
would need twice the screen. A font resolution grows text and text-sized
controls only, keeping icons and fixed geometry at their size.

**Per-program exceptions:**

- **Firefox ESR: `ui.textScaleFactor = 115`** (`config/firefox/pda-shell.js`,
  installed into `/etc/firefox-esr/` by `setup-debian.sh`; a default the user
  can change). At the session's 150 % its toolbars need at least 760 px, more
  than the 600 px portrait width, so the window hung off the right edge.
  Measured minimum widths: 760 px at 150 %, 625 px at 125 %, fits 600 px at
  115 %. `layout.css.devPixelsPerPx` is the wrong knob here: 1.15 made the
  minimum *larger* (867 px), because it multiplies the already scaled text.
  Page text at 115 % is 18 px (1.9 mm).
- Programs that hard-code pixel fonts ignore the DPI and stay small.

**Minimum usable viewport.** The work area is 600×926 (portrait) and
1024×502 (landscape) with `pocketpc` bars; with the keyboard shown it is
600×649 and 1024×302 (measured; landscape is the hard case). Dialogs that need more
than the work area are fitted (moved, shrunk to their minimum size, aligned
so their bottom-right buttons stay visible) and logged as `geometry:` lines;
see [known-limitations.md](known-limitations.md) for the measured cases.

## Results

See [screenshots.md](screenshots.md) for the same screens in each profile
and [test-results.md](test-results.md) for the recommendation.

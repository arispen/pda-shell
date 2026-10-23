# Ergonomics: physical sizes and density profiles

Milestone 6 calibration notes. Every measurement below is computed for the
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

`config/pda-shell.toml` defines three profiles (`[density.*]`), selected with
`design.density`, `dev-session.sh --density NAME` or `PDA_SHELL_DENSITY`.
Each token stays independent; a profile only overrides what it lists.

| Token | compact | balanced | readable | compact mm | balanced mm | readable mm |
|---|---:|---:|---:|---:|---:|---:|
| `font_px` (body text, em size) | 12 | 14 | 16 | 1.28 | 1.50 | 1.71 |
| `small_font_px` (paths, dates) | 10 | 11 | 13 | 1.07 | 1.18 | 1.39 |
| `title_font_px` (status bar) | 12 | 14 | 16 | 1.28 | 1.50 | 1.71 |
| `date_font_px` (Today date) | 16 | 19 | 22 | 1.71 | 2.03 | 2.35 |
| `top_bar_px` | 24 | 30 | 36 | 2.57 | 3.21 | 3.85 |
| `bottom_bar_px` | 26 | 32 | 40 | 2.78 | 3.42 | 4.28 |
| `icon_px` (bars, lists) | 16 | 18 | 22 | 1.71 | 1.93 | 2.35 |
| `today_icon_px` | 24 | 28 | 32 | 2.57 | 3.00 | 3.42 |
| `row_px` (list rows) | 20 | 28 | 34 | 2.14 | 3.00 | 3.64 |
| `target_px` (bar buttons, switcher, menus) | 24 | 32 | 38 | 2.57 | 3.42 | 4.07 |

Font sizes are em sizes; the x-height of DejaVu Sans is about 0.55 em, so
12 px text has lowercase letters about 0.7 mm tall on the panel.

For comparison, print: 12 px on the panel is the em size of 3.6 pt type,
16 px of 4.9 pt, 18 px of 5.5 pt. A newspaper uses 8–9 pt.

## Applications

The desktop applications do not use the shell's tokens. GTK 3 programs
(Thunar, Gnumeric, Mousepad, Atril) render their default 10 pt font at 96 dpi
as 13.3 px, **1.4 mm** on the panel, and their toolbar buttons are 35–37 px
(**3.7–4 mm**), comparable to the readable profile's targets. Firefox renders
16 px page text (1.7 mm) and 32 px toolbar buttons.

So in the compact profile the shell is denser than the applications next to
it; in the readable profile it roughly matches their targets while its text
is slightly larger than theirs. Making the applications' text larger is a
session-wide setting (`Xft.dpi`, GTK settings), not a shell setting, and was
not changed for this POC.

## Results

See [screenshots.md](screenshots.md) for the same screens in each profile
and [test-results.md](test-results.md) for the recommendation.

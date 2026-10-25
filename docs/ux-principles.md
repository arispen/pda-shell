# UX principles

Pocket PC 2003 SE (iPAQ era) adapted to a 5" 1024×600 resistive panel and a
stylus. Since POC 0.3 it is the reference for the **look** as well as the
interaction model: a blue status bar with Start, a silver menu bar with the
input panel button, a Today screen of glanceable items, a Start drop-down
leading to Programs and Settings. Windows Mobile 5 is used only where one
specific idea is clearly better; the two navigation conventions are not
mixed. Linux stays under the hood; ordinary Linux programs remain available
as the advanced compatibility path.

The experience is recreated with original assets: our own icons
(`assets/icons`, see its README), Wine's LGPL Tahoma clone, our own colour
values. No Microsoft artwork, logos or the Windows flag.

## Rules

1. **Stylus-first.** Controls are sized for a stylus tip on the real panel
   (physical millimetres, see [ergonomics.md](ergonomics.md)), not a finger.
2. **High information density, Pocket PC style.** Thin separators, colored
   icons, bold item titles with dim secondary text. No cards, no floating
   buttons, no large finger padding.
3. **One primary application at a time**, filling the space between the bars.
4. **Persistent navigation.** The status bar (Start, title, clock) and the
   command bar (Today, Windows, Close/OK, input panel) are always reachable,
   whatever program is in front.
5. **Desktop compatibility.** Real Linux applications, unmodified; their
   text is scaled session-wide, their technical utilities live in
   Programs → Linux tools.
6. **Portrait and landscape** layouts, decided by `shell::layout`.
7. **Few steps** for common actions.
8. **No physical keyboard** needed for anything essential.
9. **No invented data.** Items without a data source (calendar, tasks, mail)
   say so; system details are real or left out.

## Screen anatomy

```text
┌──────────────────────────────────────────┐
│ (●) Start  Title of active screen   ⎓ 15:24│  status bar, blue (48 px)
├──────────────────────────────────────────┤
│ Saturday, 24 October 2026                │  Today: date header
│ ▦ Calendar    No calendar connected      │  items: icon, title, details,
│ ☑ Tasks       No task list connected     │  thin separators
│ ✉ Messages    No mail account set up     │
│ ▤ Documents   Budget 2026.xlsx …         │
│ ▣ Open programs …                        │
│ ★ Favorites   [icon] [icon] [icon] …     │
├──────────────────────────────────────────┤
│ ☀ ▣2▴ │ File▴ Edit▴ Tools▴   ✕ │ ⌨ ▴ │  command bar, silver (50 px)
└──────────────────────────────────────────┘
```

| Element | Pocket PC 2003 origin | pda-shell |
|---|---|---|
| Start button, top left | Start flag + "Start" | original green stylus emblem + "Start"; opens the Start drop-down |
| Title in the status bar | active program title | screen name (Today, Programs, Settings, About, …) or the live `_NET_WM_NAME` of the active window |
| Clock, top right | clock | tapping it returns to Today |
| Start drop-down | recent-program icons, Today, pinned programs, Programs, Settings, Find, Help | recent-program icons, Today, favorites, Programs, Settings, Find…, then End session in a quieter style |
| Today | date, owner, Calendar, Tasks, Messages | date, Calendar / Tasks / Messages (empty states until providers exist), Documents, Open programs, Favorites |
| Programs | Programs folder, icon grid | icon grid of user programs plus a **Linux tools** folder with technical utilities and settings dialogs |
| Settings | Personal / System / Connections tabs | the same three groups; System → About (device, OS, kernel, memory), Screen (sizes in mm), Linux settings, End session; Connections → Network |
| Menu bar | program menus left, SIP button right | Today and switcher icons, then **the program's own menus** (published GtkApplication menu bar) or generic File/Edit commands for other X11 programs, New on Today, OK on shell screens; Close and the input panel button at the right. See [command-bar-api.md](command-bar-api.md) |
| SIP button and arrow | input panel; arrow chooses Keyboard, Letter Recognizer… | keyboard button shows the input panel; the arrow chooses the **Pocket PC keyboard** (built in) or the **full keyboard** (Onboard, default) |
| File Explorer, Notes | built-in programs | native **Files** and **Notes** (`pda-files`, `pda-notes`); Thunar and Mousepad stay as advanced Linux tools |
| "ok" | closes a Settings applet | **OK** in the command bar goes up one level (About → Settings → Today) |
| Tap-and-hold ring of dots | tap-and-hold | same idea: dots fill in for 600 ms, then the context menu |

Portrait shows Today as one column. Landscape shows two: Calendar, Tasks,
Messages and Favorites on the left, Documents and Open programs on the right.
The Start drop-down is anchored under the Start button in both orientations
(`start_menu_width_px`, as tall as its content), so the program behind it
stays visible.

Search is kept but no longer the first impression: **Find…** in the Start
menu, or simply typing while the menu is open, turns it into a search over
every installed program.

## Navigation rules

- **Today** (command bar, clock, Start → Today) always lands on the Today
  screen, from any program or shell screen.
- **Programs** and **Settings** are shell screens on the desktop surface
  below the programs; opening one from a program shows the desktop.
- **OK** goes up one level among the shell screens
  (Linux tools → Programs → Today, About/Screen/Network/Linux settings →
  Settings → Today). It is shown only on those screens.
- **Close** (✕) closes the program in front the normal way (it may ask to
  save). **Windows** switches programs; "Close all" needs a second tap.
- **New ▴** on Today creates a note, a spreadsheet or a folder.
- The program's menus are in the command bar whatever program is in front;
  a program's own menu bar inside its window is optional.
- **Exit full screen** stays above full-screen programs.
- **End session** is in the Start menu (last, quieter row) and in
  Settings → System; it closes programs the normal way first.

## Interaction model

| Stylus | Means | Status |
|---|---|---|
| Tap | left click | shell: GTK default |
| Double tap | double click | recognizer done, shell uses GTK default |
| Tap and drag | drag / select | recognizer done; shell lists use scrollbars |
| Long press (600 ms, cancelled by > 8 px movement) | right click / context menu | shell and native programs: done (favorites, Programs, Start rows, documents, Files: Rename/Delete, Notes: Delete); system-wide: future |

All thresholds are in `[input]` of the config file.

## Visual language

One set of tokens (`[design]`, `[colors]` in `config/pda-shell.toml`) drives
the generated stylesheet (`ui::stylesheet`) and, through XSETTINGS, the
programs' font and resolution. Token reference and per-program scaling
exceptions: [ergonomics.md](ergonomics.md).

- **Font:** Tahoma (Wine's LGPL clone, `fonts-wine`), DejaVu Sans as
  fallback. Two text sizes (body, secondary) plus the Today date.
- **Status bar:** blue vertical gradient (`bar_bg` → `bar_bg_end`), white
  bold text.
- **Command bar:** silver gradient (`menu_bg` → `menu_bg_end`) with a thin
  darker top line, black text-only commands separated by thin rules; the
  pressed command turns into the blue selection.
- **Surfaces:** white, with `surface_alt` for the Today header and the
  Start menu's recent strip; thin `separator` lines between items.
- **Selection:** solid blue (`accent`) with white text, sharp corners.
- **Icons:** the shell's own colored set (`assets/icons`, 48 px grid,
  dark outline, vertical gradient) at `today_icon_px` on Today, Programs and
  Settings and `icon_px` in lists and bars; programs keep their own colored
  icons. Symbolic icons only for status (network, battery).
- **Popovers and menus:** flat, square, 1 px outline; no shadows (there is
  no compositor) and no animations. Command bar menus are lists of
  `target_px` rows with a ✓ column, drawn by the shell.
- **Native programs:** the same tokens and stylesheet as the shell: header
  band, lined lists with document icons, no toolbars; questions (new folder
  name, delete?) appear as a strip above the content instead of dialogs.
- **Built-in input panel:** five rows (numbers, QWERTY, CAP/Shift, Ctl/Alt
  and arrows), silver keys with a dark outline, the blue selection for
  pressed and latched keys.

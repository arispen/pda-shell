# Command bar API

The command bar at the bottom of the screen shows the commands of what is in
front, like the menu bar of a Pocket PC 2003 program:

```text
┌──────────────────────────────────────────┐
│ ☀ ▣2▴ │ File▴ Edit▴ Tools▴        ✕ │ ⌨ ▴ │
└──────────────────────────────────────────┘
  Today, Windows   the program's menus    Close, input panel and its chooser
```

Today, the switcher, Close and the input panel are always there, whatever
program is in front. The middle part changes:

| In front | Middle of the command bar |
|---|---|
| Today | **New ▴**: Note, Event, Task, Spreadsheet, Folder (only what is installed) |
| Programs, Settings, About, … | **OK** (up one level) |
| Start menu open | **Cancel** |
| A program that publishes its menus | **its own menus** |
| Any other X11 program | generic **File ▴** and **Edit ▴** |

## For programs: publish a GtkApplication menu bar

There is no pda-shell specific API. A program gets its commands into the bar
by doing what GTK already supports for global menus:

1. Be a `GtkApplication` (GTK 3 or 4) registered on the session bus.
2. Set a menu bar: `gtk_application_set_menubar()` with a `GMenuModel`.
3. Optionally hide its own: `gtk_application_window_set_show_menubar(FALSE)`.

GTK then exports the menu over D-Bus (`org.gtk.Menus`) together with the
application's and the window's actions (`org.gtk.Actions`), and announces
them in properties on the X window:

| Window property | Meaning |
|---|---|
| `_GTK_UNIQUE_BUS_NAME` | the program's session-bus name |
| `_GTK_MENUBAR_OBJECT_PATH` | the menu bar (`org.gtk.Menus`) |
| `_GTK_APPLICATION_OBJECT_PATH` | `app.*` actions |
| `_GTK_WINDOW_OBJECT_PATH` | `win.*` actions of this window |

The shell's window watcher reads them (`platform::AppMenus`), and the command
bar loads the menu with `GDBusMenuModel` and the actions with
`GDBusActionGroup`. Tapping a command runs the program's own action in the
program; nothing is simulated.

How the model is shown:

- A top-level item with a submenu becomes a **"Label ▴"** button that opens
  the submenu upwards. A top-level item with an action becomes a plain
  button (Notes uses this for **New** and **Done**).
- Sections become separator lines; nested submenus open in place with a
  **Back** row.
- Disabled actions are greyed out; boolean actions and choices
  (`target` = state) show a **✓**.
- What does not fit (3 buttons in portrait, about 7 in landscape with the
  `pocketpc` tokens) goes into a final **More ▴**.
- The program may change its menu bar at any time (e.g. different commands
  in a list and in an editor); the bar follows `items-changed`.
- Mnemonic underscores are removed ("_File" → "File").

Programs that already do this, unmodified: **Mousepad** (File, Edit, Search,
View, Document, Help) and the shell's native **Files**, **Notes**,
**Calendar** and **Tasks**. GNOME
programs based on `GtkApplication` menu bars are expected to work the same
way; Thunar and Gnumeric build their menus without it and get the fallback.

## Fallback for other X11 programs

Programs without a published menu bar get two generic menus whose commands
are typed into the program as keyboard shortcuts (XTEST), after the shell
has given the program the keyboard focus again:

| Menu | Command | Keys |
|---|---|---|
| File | Save | Ctrl+S |
| File | Open… | Ctrl+O |
| File | Find… | Ctrl+F |
| File | Program menu | F10 (opens the program's own menu bar) |
| Edit | Undo, Cut, Copy, Paste, Select all | Ctrl+Z, X, C, V, A |

They are the conventions of almost every desktop program, and harmless where
a program does not use one.

## Native programs

`pda-files`, `pda-notes`, `pda-calendar` and `pda-tasks` (`src/native/`,
`src/bin/`) are separate
programs, not part of the shell process: the window manager maximizes them,
they appear in the switcher, Close sends them the normal close request. They
share the shell's stylesheet and tokens (`Config::load`, `ui::install_stylesheet`)
and use the API above for all their commands.

Desktop actions (`[Desktop Action new-note]`, `new-folder`, `new-event`,
`new-task`) are how Today's New menu starts them: `apps::launch_action`.
Today opens a single event, task or note by passing its file
(`Exec=pda-calendar %F`): `apps::launch_with`.

Each program changes its menu bar with what is on screen, and every
command is a window action shared by the menu and the hold-the-stylus menu:

| Program | List / folder | Form or mode |
|---|---|---|
| Files | File (New folder, Find…, Properties, Open in Linux file manager), Edit (Select, Select all, Paste), Go (Up, Home, Documents, Show hidden files, sort) | selection: **Done**, Edit (Select all, Cut, Copy, Rename…, Delete, Properties); Find, Properties: **Done** |
| Notes | **New**, View (Sort by date/title, Notes folder) | editor: **Done**, Edit, Tools |
| Calendar | **New**, View (Day, Agenda, Go to today, Export to Documents) | form: **Done**, **Cancel**, **Delete** |
| Tasks | **New**, View (Active tasks, Completed tasks, Export to Documents) | form: **Done**, **Cancel**, **Delete** |

# UX principles

A modern evolution of Pocket PC (Windows Mobile 2003): its compact layout,
persistent navigation and stylus interaction model, not its look.

## Rules

1. **Stylus-first.** Controls are sized for a stylus tip, not a finger.
2. **High information density.** No cards, no large padding, no floating
   buttons. Lists are single-line where possible.
3. **One primary application at a time**, filling the space between the bars.
4. **Persistent navigation.** The top and bottom bars are always reachable.
5. **Desktop compatibility.** Real Linux applications, unmodified.
6. **Portrait and landscape** layouts, decided by `shell::layout`.
7. **Few steps** for common actions.
8. **No physical keyboard** needed for anything essential.

## Screen anatomy

```text
┌──────────────────────────────────────────┐
│ ⠿ Start  Title of active program   ⎓ 15:24│  status bar (24 px)
├──────────────────────────────────────────┤
│                                          │
│   active application, maximized          │  work area
│   (or Today, or the Start menu on top)   │
│                                          │
├──────────────────────────────────────────┤
│ ⌂ Today │ ☰ Windows (2)   ✕ Close │ ⌨    │  command bar (26 px)
└──────────────────────────────────────────┘
```

| Element | Pocket PC origin | pda-shell |
|---|---|---|
| Start button, top left | Start flag | opens a drop-down menu: search, programs, Today / Keyboard / End session |
| Title in the top bar | active program title | live `_NET_WM_NAME` of the active window |
| Clock, top right | clock | tapping it returns to Today |
| Today screen | Today | date first, then recent documents, open programs, program shortcuts, system info |
| Bottom command bar | menu bar + SIP button | Today, running-program switcher, shell context actions, keyboard toggle |
| Tap-and-hold ring of dots | tap-and-hold | same idea: dots fill in for 600 ms, then the context menu |

Portrait: Start menu uses the full width; Today stacks Recent documents,
Open programs, Programs, System. Landscape: Start menu is a 300 px column so
the program stays visible beside it; Today puts documents and open programs
on the left, shortcuts and system information on the right. The Start menu
lists user programs; settings dialogs and technical utilities are behind
"Other programs and settings" and in search results.

## Interaction model

| Stylus | Means | Status |
|---|---|---|
| Tap | left click | shell: GTK default |
| Double tap | double click | recognizer done, shell uses GTK default |
| Tap and drag | drag / select | recognizer done; shell lists use scrollbars |
| Long press (600 ms, cancelled by > 8 px movement) | right click / context menu | shell: done; system-wide: future |

All thresholds are in `[input]` of the config file.

## Visual language

One set of tokens (`[design]`, `[colors]` in `config/pda-shell.toml`) drives
the generated stylesheet (`ui::stylesheet`): one font family, two text sizes,
16 px icons in bars and lists, 24 px icons on Today, 2–4 px spacing, 2 px
radius, dark blue bars with white text, white surfaces, a blue accent for
selection and an orange focus/feedback colour. No animations. Density
profiles (compact, balanced, readable) scale text, rows, targets, icons and
bars independently; see [ergonomics.md](ergonomics.md).

Windows Mobile provides inspiration only: no proprietary assets, logos or
colours are used.

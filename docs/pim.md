# Calendar, Tasks and Notes data (POC 0.5)

The native Calendar, Tasks and Notes programs keep everything on the device,
in open formats that other programs read. Nothing needs an account or a
server. The model and storage code is in `src/pim/` (no GTK); the programs
are `src/native/calendar.rs`, `tasks.rs` and `notes.rs`.

## Where things are stored

| Data | Location | Format |
|---|---|---|
| Events | `$XDG_DATA_HOME/pda-shell/calendar/*.ics` | iCalendar (RFC 5545), one `VCALENDAR` with one `VEVENT` per file |
| Tasks | `$XDG_DATA_HOME/pda-shell/tasks/*.ics` | iCalendar, one `VTODO` per file |
| Notes | `~/Documents/Notes/*.txt` | plain UTF-8 text; the first line is the title |
| Pinned notes | `~/Documents/Notes/.pinned` | one note file name per line |

`$XDG_DATA_HOME` defaults to `~/.local/share`. The event and task folders
use the *vdir* layout (one item per file, named after its UID), the same as
khal, todoman and vdirsyncer, so those tools can read the folders directly
or sync them later. Calendar and Tasks also have **View > Export to
Documents**, which writes all events (or tasks) into one `Calendar.ics`
(`Tasks.ics`) in `~/Documents` for backup or import into another calendar.

### Writing safely

- Every write goes to a temporary file in the same folder, is flushed
  (`fsync`), then renamed over the old file: an interrupted write leaves the
  old version, never a half-written one.
- Properties the PDA does not edit (RRULE, VALARM, ATTENDEE, X-…, time
  zone definitions) are kept when a file from another program is saved
  again.
- A file that cannot be read (malformed iCalendar, not UTF-8, an event
  without a start) is listed under the day's events or tasks with the
  reason, and never rewritten or deleted.
- Deleting an event or task moves its file to the trash.
- Notes: if another program changed a note while it was open in Notes, the
  edits are saved as "… (edited here).txt" next to it instead of
  overwriting the other version. A failed write is shown, and the note
  stays open with its text.

## Fields

**Events**: title (`SUMMARY`), start and end (`DTSTART`, `DTEND`; `DURATION`
is read), all-day flag (dates instead of date-times; `DTEND` is the day
after the last day, as RFC 5545 defines), location, notes (`DESCRIPTION`).

**Tasks**: title, completed (`STATUS:COMPLETED` and the `COMPLETED` time),
optional due date (`DUE` as a date), priority (`PRIORITY` 1 = high, none =
normal, 9 = low; 1–4 and 6–9 from other programs are read as high and low),
notes. The active list is sorted overdue → due date → undated, then high
priority first, then by title. Completed tasks: most recently completed
first.

## Time zones

- Events created or edited on the PDA are stored in **floating local time**
  (no `TZID`): 09:00 stays 09:00 when the device's time zone changes, like a
  paper diary. All-day events are dates and have no time zone at all.
- Events from other programs keep their times exactly as written (UTC
  `…Z` or `TZID=…`). They are shown converted to the device's current
  local time (GLib's time zone database, so DST is applied for the date of
  the event). Saving such an event without touching its times writes them
  back unchanged. Only if its start, end or all-day flag is edited do the
  new times become floating local times.
- An unknown `TZID` is shown as written, without conversion.
- Recurring events (`RRULE`) show their first occurrence only, marked
  "repeats"; the rule is kept. Overridden occurrences (`RECURRENCE-ID`) are
  not shown separately.

## Today

Today's Calendar and Tasks items read the same folders, read-only:

- **Calendar**: the rest of today's events (all-day ones all day), up to
  three, then "N more today"; "No events today" or "No more events today"
  when there are none.
- **Tasks**: "N due today · M overdue · K active" (or "No active tasks"),
  then up to three overdue or due-today tasks.

The interface between Today and the programs is the storage format, not the
programs: Today needs neither program to be running. It keeps a read-only
snapshot and reloads it when a **file monitor** on either folder reports a
change (no polling); the rows are redrawn once a minute so events that are
over leave the list. Tapping an event or task opens that file in the
program (`pda-calendar FILE`, `pda-tasks FILE`); tapping a summary line opens
the program. If a program is not installed, Today still shows the data it
finds, and says "Calendar is not installed" when there is none.

Today's Documents list includes recent notes; tapping one opens it in Notes
rather than in a text editor. Messages stays a placeholder until there is a
mail or messaging provider.

## Command line

```text
pda-calendar [--new] [EVENT.ics]
pda-tasks    [--new] [TASK.ics]
pda-notes    [--new] [NOTE.txt]
pda-files    [FOLDER] [--new-folder]
```

`--new` is also the desktop action behind Today's **New** menu (Event, Task,
Note).

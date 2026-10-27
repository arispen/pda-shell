//! Calendar events (VEVENT).
//!
//! Time zones: an event created or edited on the PDA is stored in floating
//! local time (no TZID), all-day events as dates. Imported events keep
//! their UTC or TZID times as written; they are shown converted to the
//! device's local time, and written back unchanged unless their times are
//! edited (then they become floating local times). Recurring events (RRULE)
//! keep their rule but only the first occurrence is shown.

use super::date::{Date, DateTime};
use super::ical::{Component, Property, When, Zones};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub uid: String,
    pub summary: String,
    pub start: When,
    /// For all-day events the day after the last day (exclusive), as in
    /// iCalendar.
    pub end: Option<When>,
    pub location: String,
    pub notes: String,
    /// Where it is stored.
    pub path: PathBuf,
    /// The VEVENT and VCALENDAR as read, for properties the PDA does not edit.
    pub source: Option<(Component, Component)>,
}

impl Event {
    pub fn new(uid: String, summary: String, start: When, end: Option<When>, dir: &Path) -> Self {
        let path = dir.join(super::file_name_for(&uid));
        Self { uid, summary, start, end, location: String::new(), notes: String::new(), path, source: None }
    }

    pub fn all_day(&self) -> bool {
        matches!(self.start, When::Date(_))
    }

    pub fn repeats(&self) -> bool {
        self.source.as_ref().is_some_and(|(ev, _)| ev.prop("RRULE").is_some())
    }

    /// Local start and end. Without DTEND: a timed event lasts no time, an
    /// all-day event one day.
    pub fn span(&self, zones: &dyn Zones) -> (DateTime, DateTime) {
        let start = self.start.local(zones);
        let end = match (&self.end, &self.start) {
            (Some(e), _) => e.local(zones),
            (None, When::Date(d)) => d.add_days(1).at(0, 0),
            (None, _) => start,
        };
        (start, end.max(start))
    }

    /// Whether the event takes place (partly) on `day`.
    pub fn on(&self, day: Date, zones: &dyn Zones) -> bool {
        let (s, e) = self.span(zones);
        let (d0, d1) = (day.at(0, 0), day.add_days(1).at(0, 0));
        if s == e {
            return s >= d0 && s < d1;
        }
        s < d1 && e > d0
    }

    /// "All day", "09:00–10:30", "09:00 (until 29 Oct)".
    pub fn time_label(&self, day: Date, zones: &dyn Zones) -> String {
        if self.all_day() {
            return "All day".into();
        }
        let (s, e) = self.span(zones);
        let from = if s.date < day { "…".to_string() } else { s.hm() };
        if s == e {
            from
        } else if e.date == s.date || (e.date == s.date.add_days(1) && e.hour == 0 && e.minute == 0) {
            format!("{from}–{}", e.hm())
        } else {
            format!("{from} (until {} {})", e.date.short(), e.hm())
        }
    }

    pub fn from_calendar(path: &Path, cal: &Component) -> Result<Vec<Event>, String> {
        let mut out = Vec::new();
        for ev in cal.children.iter().filter(|c| c.kind == "VEVENT") {
            // An overridden occurrence of a recurring event (RECURRENCE-ID)
            // is not shown separately.
            if ev.prop("RECURRENCE-ID").is_some() {
                continue;
            }
            let start = ev
                .prop("DTSTART")
                .and_then(When::from_property)
                .ok_or_else(|| "an event without a readable start".to_string())?;
            let end = match ev.prop("DTEND").and_then(When::from_property) {
                Some(e) => Some(e),
                None => ev.prop("DURATION").and_then(|d| duration_minutes(&d.value)).map(|m| match &start {
                    When::Date(d) => When::Date(d.add_days((m / 1440).max(1))),
                    When::Floating(t) => When::Floating(t.add_minutes(m)),
                    When::Utc(t) => When::Utc(t.add_minutes(m)),
                    When::Zoned(t, z) => When::Zoned(t.add_minutes(m), z.clone()),
                }),
            };
            out.push(Event {
                uid: ev.text("UID").unwrap_or_else(|| path.file_stem().unwrap_or_default().to_string_lossy().into_owned()),
                summary: ev.text("SUMMARY").unwrap_or_default(),
                start,
                end,
                location: ev.text("LOCATION").unwrap_or_default(),
                notes: ev.text("DESCRIPTION").unwrap_or_default(),
                path: path.to_path_buf(),
                source: Some((ev.clone(), cal.clone())),
            });
        }
        Ok(out)
    }

    /// The VCALENDAR to store; `stamp` is the current UTC time.
    pub fn to_calendar(&self, stamp: DateTime) -> Component {
        let (mut ev, cal) = match &self.source {
            Some((ev, cal)) => (ev.clone(), Some(cal)),
            None => (Component::new("VEVENT"), None),
        };
        ev.set("UID", Some(Property::text("UID", &self.uid)));
        let stamp = Property::new("DTSTAMP", format!("{}Z", stamp.ical()));
        ev.set("DTSTAMP", Some(stamp.clone()));
        ev.set("LAST-MODIFIED", Some(Property { name: "LAST-MODIFIED".into(), ..stamp }));
        ev.set_text("SUMMARY", &self.summary);
        ev.set("DTSTART", Some(self.start.to_property("DTSTART")));
        ev.set("DURATION", None);
        ev.set("DTEND", self.end.as_ref().map(|e| e.to_property("DTEND")));
        ev.set_text("LOCATION", &self.location);
        ev.set_text("DESCRIPTION", &self.notes);
        super::wrap(ev, cal)
    }
}

/// "PT1H30M", "P1D", "-PT15M" in minutes (weeks, days, hours, minutes).
pub fn duration_minutes(s: &str) -> Option<i64> {
    let s = s.trim();
    let (sign, s) = match s.strip_prefix('-') {
        Some(r) => (-1, r),
        None => (1, s.strip_prefix('+').unwrap_or(s)),
    };
    let s = s.strip_prefix('P')?;
    let (mut total, mut num, mut in_time) = (0i64, String::new(), false);
    for c in s.chars() {
        match c {
            '0'..='9' => num.push(c),
            'T' => in_time = true,
            _ => {
                let n: i64 = num.parse().ok()?;
                num.clear();
                total += n * match (c, in_time) {
                    ('W', false) => 7 * 1440,
                    ('D', false) => 1440,
                    ('H', true) => 60,
                    ('M', true) => 1,
                    ('S', true) => 0,
                    _ => return None,
                };
            }
        }
    }
    num.is_empty().then_some(sign * total)
}

/// Read every event of a collection folder.
pub fn load(dir: &Path) -> (Vec<Event>, Vec<super::Unreadable>) {
    super::load_dir(dir, |path, text| {
        let cals = super::ical::parse(text)?;
        let mut out = Vec::new();
        for cal in cals.iter().filter(|c| c.kind == "VCALENDAR") {
            out.extend(Event::from_calendar(path, cal)?);
        }
        Ok(out)
    })
}

/// Events on `day`, all-day ones first, then by start time and title.
pub fn on_day<'a>(events: &'a [Event], day: Date, zones: &dyn Zones) -> Vec<&'a Event> {
    let mut v: Vec<&Event> = events.iter().filter(|e| e.on(day, zones)).collect();
    v.sort_by(|a, b| {
        b.all_day()
            .cmp(&a.all_day())
            .then_with(|| a.span(zones).0.cmp(&b.span(zones).0))
            .then_with(|| a.summary.to_lowercase().cmp(&b.summary.to_lowercase()))
    });
    v
}

/// What Today shows: today's events that have not ended yet (all-day ones
/// all day), in order.
pub fn upcoming_today<'a>(events: &'a [Event], zones: &dyn Zones) -> Vec<&'a Event> {
    let now = zones.now();
    on_day(events, now.date, zones)
        .into_iter()
        .filter(|e| {
            let (s, end) = e.span(zones);
            e.all_day() || end > now || (s == end && s >= now)
        })
        .collect()
}

/// Every event as one iCalendar file (export, backup).
pub fn export(events: &[Event], stamp: DateTime) -> String {
    let mut cal = super::wrap(Component::new("VEVENT"), None);
    cal.children.clear();
    for e in events {
        let c = e.to_calendar(stamp);
        // Keep the time zone definitions the events refer to, once.
        for child in c.children {
            if child.kind == "VEVENT" || !cal.children.contains(&child) {
                cal.children.push(child);
            }
        }
    }
    super::ical::write(&cal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pim::{ical, FixedZones};

    fn zones() -> FixedZones {
        // Local time is UTC+1; now is 27 Oct 2026 12:00 local.
        FixedZones { offset_minutes: 60, now: Date::new(2026, 10, 27).unwrap().at(12, 0) }
    }

    fn day(d: u32) -> Date {
        Date::new(2026, 10, d).unwrap()
    }

    fn ev(summary: &str, start: When, end: Option<When>) -> Event {
        Event::new(crate::pim::new_uid(), summary.into(), start, end, Path::new("/tmp"))
    }

    #[test]
    fn days_and_labels() {
        let z = zones();
        let meeting = ev("Meeting", When::Floating(day(27).at(9, 0)), Some(When::Floating(day(27).at(10, 30))));
        let trip = ev("Trip", When::Date(day(27)), Some(When::Date(day(29))));
        let night = ev("Night", When::Floating(day(27).at(22, 0)), Some(When::Floating(day(28).at(2, 0))));
        assert!(meeting.on(day(27), &z) && !meeting.on(day(28), &z));
        assert!(trip.on(day(28), &z) && !trip.on(day(29), &z), "DTEND of a date is exclusive");
        assert!(night.on(day(28), &z));
        assert_eq!(meeting.time_label(day(27), &z), "09:00–10:30");
        assert_eq!(trip.time_label(day(27), &z), "All day");
        assert_eq!(night.time_label(day(27), &z), "22:00 (until 28 Oct 02:00)");
        assert_eq!(night.time_label(day(28), &z), "… (until 28 Oct 02:00)");
        let all = vec![meeting, trip, night];
        let titles: Vec<_> = on_day(&all, day(27), &z).iter().map(|e| e.summary.as_str()).collect();
        assert_eq!(titles, ["Trip", "Meeting", "Night"]);
        // At noon the morning meeting is over.
        let titles: Vec<_> = upcoming_today(&all, &z).iter().map(|e| e.summary.as_str()).collect();
        assert_eq!(titles, ["Trip", "Night"]);
    }

    #[test]
    fn utc_times_are_shown_local_and_saved_unchanged() {
        let text = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:x1\r\nSUMMARY:Call\r\nDTSTART:20261027T230000Z\r\nDURATION:PT30M\r\nRRULE:FREQ=WEEKLY\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let cal = &ical::parse(text).unwrap()[0];
        let e = &Event::from_calendar(Path::new("/tmp/x1.ics"), cal).unwrap()[0];
        let z = zones();
        // 23:00 UTC is midnight local: the next day.
        assert!(e.on(day(28), &z) && !e.on(day(27), &z));
        assert_eq!(e.time_label(day(28), &z), "00:00–00:30");
        assert!(e.repeats());
        let again = ical::write(&e.to_calendar(z.now_utc()));
        assert!(again.contains("DTSTART:20261027T230000Z"), "{again}");
        assert!(again.contains("DTEND:20261027T233000Z"));
        assert!(again.contains("RRULE:FREQ=WEEKLY"), "unknown properties are kept");
    }

    #[test]
    fn round_trip_through_files() {
        let dir = std::env::temp_dir().join(format!("pda-events-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let z = zones();
        let mut e = ev("Dentist, Dr. Ü; room 2", When::Floating(day(30).at(8, 15)), Some(When::Floating(day(30).at(9, 0))));
        e.path = dir.join(crate::pim::file_name_for(&e.uid));
        e.location = "Main street 1".into();
        e.notes = "Bring card\nand coffee".into();
        crate::pim::write_atomic(&e.path, &ical::write(&e.to_calendar(z.now_utc()))).unwrap();
        std::fs::write(dir.join("broken.ics"), "BEGIN:VCALENDAR\nBEGIN:VEVENT\nSUMMARY:x\nEND:VEVENT\nEND:VCALENDAR\n").unwrap();
        let (events, bad) = load(&dir);
        assert_eq!(bad.len(), 1, "an event without a start is reported");
        assert_eq!(events.len(), 1);
        let back = &events[0];
        assert_eq!((&back.summary, &back.location, &back.notes), (&e.summary, &e.location, &e.notes));
        assert_eq!((&back.start, &back.end, &back.uid), (&e.start, &e.end, &e.uid));
        let exported = export(&events, z.now_utc());
        assert_eq!(ical::parse(&exported).unwrap()[0].children.len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn durations() {
        assert_eq!(duration_minutes("PT1H30M"), Some(90));
        assert_eq!(duration_minutes("P1W2D"), Some(9 * 1440));
        assert_eq!(duration_minutes("-PT15M"), Some(-15));
        assert_eq!(duration_minutes("1H"), None);
        assert_eq!(duration_minutes("PT1X"), None);
    }
}

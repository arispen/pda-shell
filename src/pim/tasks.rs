//! Tasks (VTODO).
//!
//! Fields: title (SUMMARY), completed (STATUS:COMPLETED plus COMPLETED
//! time), optional due date (DUE as a date; a due date-time from another
//! program is shown by its local date and kept unless the due date is
//! edited), notes (DESCRIPTION) and priority (PRIORITY: 1–4 high, 5 or
//! none normal, 6–9 low).
//!
//! Order of the active list: overdue and dated tasks by due date, then
//! undated ones; within a day high priority first, then by title.
//! Completed tasks: most recently completed first.

use super::date::{Date, DateTime};
use super::ical::{Component, Property, When, Zones};
use std::cmp::Ordering;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Priority {
    High,
    Normal,
    Low,
}

impl Priority {
    pub fn from_ical(v: Option<&str>) -> Self {
        match v.and_then(|v| v.trim().parse::<u8>().ok()) {
            Some(1..=4) => Priority::High,
            Some(6..=9) => Priority::Low,
            _ => Priority::Normal,
        }
    }

    pub fn ical(self) -> Option<&'static str> {
        match self {
            Priority::High => Some("1"),
            Priority::Normal => None,
            Priority::Low => Some("9"),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Priority::High => "High",
            Priority::Normal => "Normal",
            Priority::Low => "Low",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub uid: String,
    pub summary: String,
    /// Completion time (UTC), if completed.
    pub completed: Option<DateTime>,
    pub done: bool,
    pub due: Option<When>,
    pub priority: Priority,
    pub notes: String,
    pub path: PathBuf,
    pub source: Option<(Component, Component)>,
}

impl Task {
    pub fn new(uid: String, summary: String, dir: &Path) -> Self {
        let path = dir.join(super::file_name_for(&uid));
        Self {
            uid,
            summary,
            completed: None,
            done: false,
            due: None,
            priority: Priority::Normal,
            notes: String::new(),
            path,
            source: None,
        }
    }

    pub fn due_date(&self, zones: &dyn Zones) -> Option<Date> {
        self.due.as_ref().map(|d| d.local(zones).date)
    }

    pub fn overdue(&self, today: Date, zones: &dyn Zones) -> bool {
        !self.done && self.due_date(zones).is_some_and(|d| d < today)
    }

    pub fn due_today(&self, today: Date, zones: &dyn Zones) -> bool {
        !self.done && self.due_date(zones) == Some(today)
    }

    /// "Due today", "Overdue since 25 Oct", "Due 30 Oct", "" (undated).
    pub fn due_label(&self, today: Date, zones: &dyn Zones) -> String {
        match self.due_date(zones) {
            None => String::new(),
            Some(d) if self.done => format!("Was due {}", d.short()),
            Some(d) if d < today => format!("Overdue since {}", d.short()),
            Some(d) if d == today => "Due today".into(),
            Some(d) if d == today.add_days(1) => "Due tomorrow".into(),
            Some(d) => format!("Due {}", d.short()),
        }
    }

    pub fn set_done(&mut self, done: bool, now_utc: DateTime) {
        self.done = done;
        self.completed = done.then_some(now_utc);
    }

    pub fn from_calendar(path: &Path, cal: &Component) -> Result<Vec<Task>, String> {
        Ok(cal
            .children
            .iter()
            .filter(|c| c.kind == "VTODO")
            .map(|t| {
                let status = t.prop("STATUS").map(|s| s.value.trim().to_ascii_uppercase());
                let completed = t.prop("COMPLETED").and_then(|p| DateTime::parse_ical(p.value.trim()));
                Task {
                    uid: t.text("UID").unwrap_or_else(|| path.file_stem().unwrap_or_default().to_string_lossy().into_owned()),
                    summary: t.text("SUMMARY").unwrap_or_default(),
                    done: status.as_deref() == Some("COMPLETED") || completed.is_some(),
                    completed,
                    due: t.prop("DUE").and_then(When::from_property),
                    priority: Priority::from_ical(t.prop("PRIORITY").map(|p| p.value.as_str())),
                    notes: t.text("DESCRIPTION").unwrap_or_default(),
                    path: path.to_path_buf(),
                    source: Some((t.clone(), cal.clone())),
                }
            })
            .collect())
    }

    pub fn to_calendar(&self, stamp: DateTime) -> Component {
        let (mut t, cal) = match &self.source {
            Some((t, cal)) => (t.clone(), Some(cal)),
            None => (Component::new("VTODO"), None),
        };
        t.set("UID", Some(Property::text("UID", &self.uid)));
        let stamp_p = Property::new("DTSTAMP", format!("{}Z", stamp.ical()));
        t.set("DTSTAMP", Some(stamp_p.clone()));
        t.set("LAST-MODIFIED", Some(Property { name: "LAST-MODIFIED".into(), ..stamp_p }));
        t.set_text("SUMMARY", &self.summary);
        t.set("DUE", self.due.as_ref().map(|d| d.to_property("DUE")));
        t.set("PRIORITY", self.priority.ical().map(|p| Property::new("PRIORITY", p)));
        t.set_text("DESCRIPTION", &self.notes);
        if self.done {
            t.set("STATUS", Some(Property::new("STATUS", "COMPLETED")));
            let at = self.completed.unwrap_or(stamp);
            t.set("COMPLETED", Some(Property::new("COMPLETED", format!("{}Z", at.ical()))));
            t.set("PERCENT-COMPLETE", Some(Property::new("PERCENT-COMPLETE", "100")));
        } else {
            t.set("STATUS", Some(Property::new("STATUS", "NEEDS-ACTION")));
            t.set("COMPLETED", None);
            t.set("PERCENT-COMPLETE", None);
        }
        super::wrap(t, cal)
    }
}

/// Active-list order (see the module documentation).
pub fn active_order(a: &Task, b: &Task, zones: &dyn Zones) -> Ordering {
    let key = |t: &Task| (t.due_date(zones).is_none(), t.due_date(zones), t.priority);
    key(a).cmp(&key(b)).then_with(|| a.summary.to_lowercase().cmp(&b.summary.to_lowercase()))
}

/// Tasks of a view, in order.
pub fn sorted<'a>(tasks: &'a [Task], done: bool, zones: &dyn Zones) -> Vec<&'a Task> {
    let mut v: Vec<&Task> = tasks.iter().filter(|t| t.done == done).collect();
    if done {
        v.sort_by(|a, b| b.completed.cmp(&a.completed).then_with(|| a.summary.cmp(&b.summary)));
    } else {
        v.sort_by(|a, b| active_order(a, b, zones));
    }
    v
}

/// What Today shows about tasks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary<'a> {
    pub active: usize,
    pub due_today: usize,
    pub overdue: usize,
    /// Overdue and due-today tasks, in list order.
    pub urgent: Vec<&'a Task>,
}

pub fn summary<'a>(tasks: &'a [Task], zones: &dyn Zones) -> Summary<'a> {
    let today = zones.now().date;
    let active = sorted(tasks, false, zones);
    Summary {
        active: active.len(),
        due_today: active.iter().filter(|t| t.due_today(today, zones)).count(),
        overdue: active.iter().filter(|t| t.overdue(today, zones)).count(),
        urgent: active.into_iter().filter(|t| t.overdue(today, zones) || t.due_today(today, zones)).collect(),
    }
}

/// "1 due today · 2 overdue · 5 active", "No active tasks".
pub fn summary_line(s: &Summary) -> String {
    if s.active == 0 {
        return "No active tasks".into();
    }
    let mut parts = Vec::new();
    if s.due_today > 0 {
        parts.push(format!("{} due today", s.due_today));
    }
    if s.overdue > 0 {
        parts.push(format!("{} overdue", s.overdue));
    }
    parts.push(format!("{} active", s.active));
    parts.join(" · ")
}

pub fn load(dir: &Path) -> (Vec<Task>, Vec<super::Unreadable>) {
    super::load_dir(dir, |path, text| {
        let cals = super::ical::parse(text)?;
        let mut out = Vec::new();
        for cal in cals.iter().filter(|c| c.kind == "VCALENDAR") {
            out.extend(Task::from_calendar(path, cal)?);
        }
        if out.is_empty() {
            return Err("no task in this file".into());
        }
        Ok(out)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pim::{ical, FixedZones};

    fn zones() -> FixedZones {
        FixedZones { offset_minutes: 60, now: Date::new(2026, 10, 27).unwrap().at(12, 0) }
    }

    fn task(s: &str, due: Option<u32>, p: Priority) -> Task {
        let mut t = Task::new(crate::pim::new_uid(), s.into(), Path::new("/tmp"));
        t.due = due.map(|d| When::Date(Date::new(2026, 10, d).unwrap()));
        t.priority = p;
        t
    }

    #[test]
    fn order_summary_and_labels() {
        let z = zones();
        let mut tasks = vec![
            task("undated", None, Priority::High),
            task("b today", Some(27), Priority::Normal),
            task("a today", Some(27), Priority::Normal),
            task("urgent today", Some(27), Priority::High),
            task("overdue", Some(25), Priority::Low),
            task("later", Some(30), Priority::Normal),
            task("done", Some(20), Priority::Normal),
        ];
        tasks[6].set_done(true, z.now_utc());
        let titles: Vec<_> = sorted(&tasks, false, &z).iter().map(|t| t.summary.as_str()).collect();
        assert_eq!(titles, ["overdue", "urgent today", "a today", "b today", "later", "undated"]);
        let s = summary(&tasks, &z);
        assert_eq!((s.active, s.due_today, s.overdue, s.urgent.len()), (6, 3, 1, 4));
        assert_eq!(summary_line(&s), "3 due today · 1 overdue · 6 active");
        assert_eq!(summary_line(&summary(&[], &z)), "No active tasks");
        let today = z.now().date;
        assert_eq!(tasks[4].due_label(today, &z), "Overdue since 25 Oct");
        assert_eq!(tasks[1].due_label(today, &z), "Due today");
        assert_eq!(tasks[5].due_label(today, &z), "Due 30 Oct");
        assert_eq!(tasks[0].due_label(today, &z), "");
        assert!(!tasks[6].overdue(today, &z), "completed tasks are never overdue");
    }

    #[test]
    fn completing_round_trips_through_ical() {
        let z = zones();
        let mut t = task("Pay rent; call, landlord", Some(28), Priority::High);
        t.notes = "IBAN in notes".into();
        let cal = t.to_calendar(z.now_utc());
        let text = ical::write(&cal);
        assert!(text.contains("STATUS:NEEDS-ACTION") && text.contains("PRIORITY:1") && text.contains("DUE;VALUE=DATE:20261028"));
        let mut back = Task::from_calendar(&t.path, &ical::parse(&text).unwrap()[0]).unwrap().remove(0);
        assert_eq!((&back.summary, &back.notes, back.priority, back.done), (&t.summary, &t.notes, Priority::High, false));
        back.set_done(true, z.now_utc());
        let text = ical::write(&back.to_calendar(z.now_utc()));
        assert!(text.contains("STATUS:COMPLETED") && text.contains("COMPLETED:20261027T110000Z"), "{text}");
        let again = Task::from_calendar(&t.path, &ical::parse(&text).unwrap()[0]).unwrap().remove(0);
        assert!(again.done);
        back.set_done(false, z.now_utc());
        let text = ical::write(&back.to_calendar(z.now_utc()));
        assert!(!text.contains("COMPLETED:") && text.contains("NEEDS-ACTION"));
    }

    #[test]
    fn foreign_priorities_and_due_times() {
        let text = "BEGIN:VCALENDAR\nBEGIN:VTODO\nUID:t\nSUMMARY:x\nPRIORITY:7\nDUE:20261027T230000Z\nX-KEEP:1\nEND:VTODO\nEND:VCALENDAR\n";
        let t = Task::from_calendar(Path::new("t.ics"), &ical::parse(text).unwrap()[0]).unwrap().remove(0);
        let z = zones();
        assert_eq!(t.priority, Priority::Low);
        // 23:00 UTC is tomorrow at 00:00 local.
        assert_eq!(t.due_date(&z), Date::new(2026, 10, 28));
        let out = ical::write(&t.to_calendar(z.now_utc()));
        assert!(out.contains("DUE:20261027T230000Z") && out.contains("X-KEEP:1") && out.contains("PRIORITY:9"));
    }
}

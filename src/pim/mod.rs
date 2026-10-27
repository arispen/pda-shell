//! Personal information (POC 0.5, issue #8): calendar events and tasks,
//! stored locally as iCalendar, independent of GTK.
//!
//! Storage is a "vdir" folder per collection, one `.ics` file per event or
//! task (the layout khal, todoman and vdirsyncer use):
//!
//! ```text
//! $XDG_DATA_HOME/pda-shell/calendar/<UID>.ics   VCALENDAR with one VEVENT
//! $XDG_DATA_HOME/pda-shell/tasks/<UID>.ics      VCALENDAR with one VTODO
//! ```
//!
//! Files are written atomically (temporary file, fsync, rename). A file that
//! cannot be read is skipped and reported, never rewritten. The shell's
//! Today screen reads the same folders (read-only) and watches them for
//! changes; see docs/pim.md.

pub mod date;
pub mod events;
pub mod ical;
pub mod tasks;

use std::path::{Path, PathBuf};

/// `$XDG_DATA_HOME/pda-shell/<name>` (default `~/.local/share/...`).
pub fn collection_dir(name: &str) -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
        home.join(".local/share")
    });
    base.join("pda-shell").join(name)
}

pub fn calendar_dir() -> PathBuf {
    collection_dir("calendar")
}

pub fn tasks_dir() -> PathBuf {
    collection_dir("tasks")
}

/// A new unique identifier.
pub fn new_uid() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}-{:09}-{}-{n}@pda-shell", t.as_secs(), t.subsec_nanos(), std::process::id())
}

/// A file name for `uid`: characters outside [A-Za-z0-9@._-] become '_'.
pub fn file_name_for(uid: &str) -> String {
    let safe: String =
        uid.chars().map(|c| if c.is_ascii_alphanumeric() || "@._-".contains(c) { c } else { '_' }).take(120).collect();
    format!("{safe}.ics")
}

/// Write `text` to `path` atomically.
pub fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path.parent().ok_or_else(|| std::io::Error::other("no folder"))?;
    std::fs::create_dir_all(dir)?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let result = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// A file of a collection that could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unreadable {
    pub path: PathBuf,
    pub why: String,
}

/// Every `.ics` file of `dir`, parsed by `read`, plus the files that could
/// not be read. A missing folder is an empty collection.
pub fn load_dir<T>(dir: &Path, read: impl Fn(&Path, &str) -> Result<Vec<T>, String>) -> (Vec<T>, Vec<Unreadable>) {
    let (mut items, mut bad) = (Vec::new(), Vec::new());
    let Ok(rd) = std::fs::read_dir(dir) else { return (items, bad) };
    let mut paths: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("ics")))
        .filter(|p| !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')))
        .collect();
    paths.sort();
    for path in paths {
        let parsed = std::fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|b| String::from_utf8(b).map_err(|_| "not UTF-8 text".to_string()))
            .and_then(|text| read(&path, &text));
        match parsed {
            Ok(mut v) => items.append(&mut v),
            Err(why) => bad.push(Unreadable { path, why }),
        }
    }
    (items, bad)
}

/// A VCALENDAR around `child`, keeping the other content of `original`
/// (time zone definitions, X- properties).
pub fn wrap(child: ical::Component, original: Option<&ical::Component>) -> ical::Component {
    let mut cal = original.cloned().unwrap_or_else(|| {
        let mut c = ical::Component::new("VCALENDAR");
        c.props.push(ical::Property::new("VERSION", "2.0"));
        c.props.push(ical::Property::new("PRODID", "-//pda-shell//POC 0.5//EN"));
        c
    });
    cal.children.retain(|c| c.kind != child.kind);
    cal.children.push(child);
    cal
}

/// Zones for tests: UTC is `offset_minutes` behind local time; named zones
/// are treated like UTC.
#[cfg(test)]
pub struct FixedZones {
    pub offset_minutes: i64,
    pub now: date::DateTime,
}

#[cfg(test)]
impl ical::Zones for FixedZones {
    fn to_local(&self, t: date::DateTime, _tz: Option<&str>) -> date::DateTime {
        t.add_minutes(self.offset_minutes)
    }
    fn now(&self) -> date::DateTime {
        self.now
    }
    fn now_utc(&self) -> date::DateTime {
        self.now.add_minutes(-self.offset_minutes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uids_are_unique_and_file_names_safe() {
        let (a, b) = (new_uid(), new_uid());
        assert_ne!(a, b);
        assert_eq!(file_name_for("a/b c:d@x.y"), "a_b_c_d@x.y.ics");
    }

    #[test]
    fn unreadable_files_are_reported_not_fatal() {
        let dir = std::env::temp_dir().join(format!("pda-pim-load-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        write_atomic(&dir.join("good.ics"), "BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n").unwrap();
        std::fs::write(dir.join("bad.ics"), "BEGIN:VCALENDAR\r\n").unwrap();
        std::fs::write(dir.join("binary.ics"), [0xff, 0xfe, 0]).unwrap();
        std::fs::write(dir.join("other.txt"), "x").unwrap();
        let (items, bad) = load_dir(&dir, |_, t| ical::parse(t));
        assert_eq!(items.len(), 1);
        assert_eq!(bad.len(), 2);
        assert!(std::fs::read_dir(&dir).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().ends_with(".tmp")));
        assert!(load_dir(&dir.join("missing"), |_, t| ical::parse(t)).0.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

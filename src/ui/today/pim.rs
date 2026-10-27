//! Today's Calendar and Tasks entries. Each row's widget name is the file
//! of the event or task it shows (empty: just open the program), so a tap
//! opens that item in its program.

use crate::native::forms::LocalZones;
use crate::pim::events::{self, Event};
use crate::pim::ical::Zones;
use crate::pim::tasks::{self, Task};
use crate::shell::Command;
use gtk::prelude::*;

/// Rows shown at most per item; the rest is counted.
const MAX_ROWS: usize = 3;

pub fn activate(app: &str, row: &gtk::ListBoxRow) -> Command {
    Command::OpenWith(app.to_string(), row.widget_name().to_string())
}

fn row(path: &str, title: &str, detail: &str, accessible: &str) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_widget_name(path);
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    if !detail.is_empty() {
        let d = gtk::Label::new(Some(detail));
        d.add_css_class("dim");
        d.add_css_class("event-time");
        d.set_xalign(0.0);
        b.append(&d);
    }
    let t = gtk::Label::new(Some(title));
    t.set_xalign(0.0);
    t.set_hexpand(true);
    t.set_ellipsize(gtk::pango::EllipsizeMode::End);
    b.append(&t);
    row.set_child(Some(&b));
    row.update_property(&[gtk::accessible::Property::Label(accessible)]);
    row
}

/// A dim line; tapping it opens the program (when it is installed).
fn summary(text: &str, installed: bool) -> gtk::ListBoxRow {
    let r = gtk::ListBoxRow::new();
    r.set_widget_name("");
    r.set_activatable(installed);
    let l = super::line(text);
    r.set_child(Some(&l));
    r.update_property(&[gtk::accessible::Property::Label(text)]);
    r
}

fn title(s: &str) -> String {
    if s.trim().is_empty() {
        "(no title)".into()
    } else {
        s.to_string()
    }
}

pub fn fill_agenda(list: &gtk::ListBox, all: &[Event], installed: bool, _px: u32) {
    list.remove_all();
    if !installed && all.is_empty() {
        list.append(&summary("Calendar is not installed", false));
        return;
    }
    let today = LocalZones.now().date;
    let upcoming = events::upcoming_today(all, &LocalZones);
    for e in upcoming.iter().take(MAX_ROWS) {
        let when = e.time_label(today, &LocalZones);
        let t = title(&e.summary);
        list.append(&row(&e.path.to_string_lossy(), &t, &when, &format!("{t}, {when}")));
    }
    let text = match upcoming.len() {
        0 if events::on_day(all, today, &LocalZones).is_empty() => "No events today".to_string(),
        0 => "No more events today".to_string(),
        n if n > MAX_ROWS => format!("{} more today", n - MAX_ROWS),
        _ => String::new(),
    };
    if !text.is_empty() {
        list.append(&summary(&text, installed));
    }
}

pub fn fill_tasks(list: &gtk::ListBox, all: &[Task], installed: bool, _px: u32) {
    list.remove_all();
    if !installed && all.is_empty() {
        list.append(&summary("Tasks is not installed", false));
        return;
    }
    let today = LocalZones.now().date;
    let s = tasks::summary(all, &LocalZones);
    list.append(&summary(&tasks::summary_line(&s), installed));
    for t in s.urgent.iter().take(MAX_ROWS) {
        let due = if t.overdue(today, &LocalZones) { "Overdue" } else { "Today" };
        let name = title(&t.summary);
        list.append(&row(&t.path.to_string_lossy(), &name, due, &format!("{name}, {}", t.due_label(today, &LocalZones))));
    }
}

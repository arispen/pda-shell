//! Edit-form pieces shared by Calendar and Tasks: labelled rows, a date
//! field that opens a month calendar in place, an hour/minute field, and
//! the device's time zones through GLib.

use crate::pim::date::{Date, DateTime};
use crate::pim::ical::Zones;
use gtk::prelude::*;
use std::cell::Cell;
use std::rc::Rc;

/// The device's local time zone (GLib's zone database).
pub struct LocalZones;

fn from_glib(t: &glib::DateTime) -> DateTime {
    let date = Date::new(t.year(), t.month() as u32, t.day_of_month() as u32).unwrap_or(Date { year: 1970, month: 1, day: 1 });
    DateTime { date, hour: t.hour() as u32, minute: t.minute() as u32, second: t.second() as u32 }
}

impl Zones for LocalZones {
    fn to_local(&self, t: DateTime, tz: Option<&str>) -> DateTime {
        let zone = match tz {
            None => glib::TimeZone::utc(),
            // An unknown zone name: the time is shown as written.
            Some(id) => {
                // GLib falls back to UTC for names it does not know.
                #[allow(deprecated)]
                let z = glib::TimeZone::new(Some(id));
                if z.identifier() != id {
                    return t;
                }
                z
            }
        };
        glib::DateTime::new(&zone, t.date.year, t.date.month as i32, t.date.day as i32, t.hour as i32, t.minute as i32, t.second as f64)
            .and_then(|d| d.to_local())
            .map(|d| from_glib(&d))
            .unwrap_or(t)
    }

    fn now(&self) -> DateTime {
        glib::DateTime::now_local().map(|d| from_glib(&d)).unwrap_or_else(|_| self.now_utc())
    }

    fn now_utc(&self) -> DateTime {
        let s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
        DateTime::from_seconds(s)
    }
}

/// A form row: dim label above the field.
pub fn field(label: &str, widget: &impl IsA<gtk::Widget>) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 2);
    b.add_css_class("form-row");
    let l = gtk::Label::new(Some(label));
    l.add_css_class("kv-key");
    l.set_xalign(0.0);
    b.append(&l);
    b.append(widget);
    widget.as_ref().update_property(&[gtk::accessible::Property::Label(label)]);
    b
}

/// A button showing a date; tapping it shows a month calendar below it to
/// pick another day.
#[derive(Clone)]
pub struct DateField {
    pub widget: gtk::Box,
    button: gtk::Button,
    calendar: gtk::Calendar,
    date: Rc<Cell<Date>>,
    on_change: Rc<std::cell::RefCell<Option<Box<dyn Fn(Date)>>>>,
}

impl DateField {
    pub fn new(name: &str) -> Self {
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 2);
        let button = gtk::Button::new();
        button.update_property(&[gtk::accessible::Property::Label(name)]);
        button.set_halign(gtk::Align::Start);
        let calendar = gtk::Calendar::new();
        calendar.set_visible(false);
        calendar.set_halign(gtk::Align::Start);
        widget.append(&button);
        widget.append(&calendar);
        let f = Self {
            widget,
            button,
            calendar,
            date: Rc::new(Cell::new(Date { year: 2026, month: 1, day: 1 })),
            on_change: Default::default(),
        };
        let c = f.calendar.clone();
        f.button.connect_clicked(move |_| c.set_visible(!c.is_visible()));
        let me = f.clone();
        f.calendar.connect_day_selected(move |cal| {
            let d = cal.date();
            if let Some(date) = Date::new(d.year(), d.month() as u32, d.day_of_month() as u32) {
                if date != me.date.get() {
                    me.set_inner(date, false);
                    cal.set_visible(false);
                    if let Some(f) = me.on_change.borrow().as_ref() {
                        f(date);
                    }
                }
            }
        });
        f
    }

    fn set_inner(&self, d: Date, calendar: bool) {
        self.date.set(d);
        self.button.set_label(&d.long());
        if calendar {
            if let Ok(g) = glib::DateTime::from_local(d.year, d.month as i32, d.day as i32, 12, 0, 0.0) {
                self.calendar.select_day(&g);
            }
        }
    }

    pub fn set(&self, d: Date) {
        self.set_inner(d, true);
        self.calendar.set_visible(false);
    }

    pub fn get(&self) -> Date {
        self.date.get()
    }

    pub fn connect_changed(&self, f: impl Fn(Date) + 'static) {
        *self.on_change.borrow_mut() = Some(Box::new(f));
    }
}

/// Hour and minute as two spin buttons (minutes in steps of 5).
#[derive(Clone)]
pub struct TimeField {
    pub widget: gtk::Box,
    hour: gtk::SpinButton,
    minute: gtk::SpinButton,
}

impl TimeField {
    pub fn new(name: &str) -> Self {
        let widget = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let hour = gtk::SpinButton::with_range(0.0, 23.0, 1.0);
        hour.set_wrap(true);
        hour.update_property(&[gtk::accessible::Property::Label(&format!("{name} hour"))]);
        let minute = gtk::SpinButton::with_range(0.0, 59.0, 5.0);
        minute.set_wrap(true);
        minute.update_property(&[gtk::accessible::Property::Label(&format!("{name} minute"))]);
        for s in [&hour, &minute] {
            s.set_numeric(true);
            // Two digits: "09".
            s.connect_output(|s| {
                s.set_text(&format!("{:02}", s.value() as i32));
                glib::Propagation::Stop
            });
        }
        widget.append(&hour);
        widget.append(&gtk::Label::new(Some(":")));
        widget.append(&minute);
        Self { widget, hour, minute }
    }

    pub fn set(&self, hour: u32, minute: u32) {
        self.hour.set_value(hour as f64);
        self.minute.set_value(minute as f64);
    }

    pub fn get(&self) -> (u32, u32) {
        (self.hour.value() as u32, self.minute.value() as u32)
    }

    pub fn connect_changed(&self, f: impl Fn() + 'static) {
        let f = Rc::new(f);
        for s in [&self.hour, &self.minute] {
            let f = f.clone();
            s.connect_value_changed(move |_| f());
        }
    }
}

/// A multi-line text field.
pub fn notes_view() -> gtk::TextView {
    let view = gtk::TextView::new();
    view.set_wrap_mode(gtk::WrapMode::WordChar);
    view.add_css_class("notes-field");
    view.set_size_request(-1, 96);
    view
}

pub fn text_of(view: &gtk::TextView) -> String {
    let b = view.buffer();
    b.text(&b.start_iter(), &b.end_iter(), false).to_string()
}

/// The page of an edit form: fields in a column that scrolls.
pub fn form_page(rows: &[&gtk::Widget]) -> (gtk::ScrolledWindow, gtk::Label) {
    let col = gtk::Box::new(gtk::Orientation::Vertical, 6);
    col.add_css_class("form");
    let error = gtk::Label::new(None);
    error.add_css_class("form-error");
    error.set_xalign(0.0);
    error.set_wrap(true);
    error.set_visible(false);
    col.append(&error);
    for r in rows {
        col.append(*r);
    }
    let scroll =
        gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).vexpand(true).child(&col).build();
    (scroll, error)
}

//! Calendar: local events in the Pocket PC 2003 style. A day view (events
//! of one day, all-day ones first) with previous/next day, an agenda of the
//! coming two weeks, and an edit form. Events are iCalendar files in
//! `pim::calendar_dir()` (see `pim::events` for the time zone rules).
//!
//! Like Notes there is no separate Save: Done keeps the event, Cancel
//! drops the changes. Closing the program with an open form keeps a valid
//! event and says so in the log; an event without a title is not kept.
//!
//! Command bar: day/agenda: New, View (Day, Agenda, Today, Export to
//! Documents); form: Done, Cancel, Delete.
//!
//! `pda-calendar [--new] [EVENT.ics]`: open an event (Today does this when
//! an event is tapped) or start a new one.

use super::forms::{self, DateField, LocalZones, TimeField};
use super::{action, AskBar, NativeWindow};
use crate::config::Config;
use crate::pim::date::{Date, DateTime};
use crate::pim::events::{self, Event};
use crate::pim::ical::{When, Zones};
use crate::pim::{self, Unreadable};
use crate::ui::MenuItem;
use gtk::gio;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

pub const APP_ID: &str = "io.github.pda_shell.Calendar";
/// Days the agenda shows.
const AGENDA_DAYS: i64 = 14;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Day,
    Agenda,
    Edit,
}

/// The event being edited: the stored one (None for new) and its local
/// times when the form opened, to keep zoned times that were not changed.
struct Editing {
    /// The event with the fields the form does not show (uid, path, source).
    event: Event,
    original: Option<Event>,
    opened: (DateTime, DateTime, bool),
}

struct Calendar {
    win: NativeWindow,
    cfg: Config,
    dir: PathBuf,
    events: RefCell<Vec<Event>>,
    unreadable: RefCell<Vec<Unreadable>>,
    day: Cell<Date>,
    view: Cell<View>,
    /// The view to return to from the form.
    list_view: Cell<View>,
    menubar: gio::Menu,
    stack: gtk::Stack,
    header: gtk::Box,
    title: gtk::Label,
    sub: gtk::Label,
    list: gtk::ListBox,
    note: gtk::Label,
    ask: AskBar,
    monitor: RefCell<Option<gio::FileMonitor>>,
    // The form.
    editing: RefCell<Option<Editing>>,
    dirty: Cell<bool>,
    f_title: gtk::Entry,
    f_location: gtk::Entry,
    f_all_day: gtk::CheckButton,
    f_start_date: DateField,
    f_start_time: TimeField,
    f_end_date: DateField,
    f_end_time: TimeField,
    f_notes: gtk::TextView,
    f_error: gtk::Label,
    f_repeats: gtk::Label,
    edit_actions: Vec<gio::SimpleAction>,
}

thread_local! {
    static CALENDAR: RefCell<Option<Rc<Calendar>>> = const { RefCell::new(None) };
}

fn this() -> Option<Rc<Calendar>> {
    CALENDAR.with(|c| c.borrow().clone())
}

fn with(f: impl Fn(&Calendar) + 'static) -> impl Fn() + 'static {
    move || {
        if let Some(c) = this() {
            f(&c);
        }
    }
}

/// Entry point of `pda-calendar`.
pub fn main() -> glib::ExitCode {
    super::run(APP_ID, "pda-calendar", |app, cfg, args| {
        let cal = this().unwrap_or_else(|| {
            let c = Calendar::new(app, cfg);
            CALENDAR.with(|cell| *cell.borrow_mut() = Some(c.clone()));
            c.reload();
            c.show_list(View::Day);
            c
        });
        let file = args
            .iter()
            .find(|a| !a.starts_with("--"))
            .map(|a| if a.contains("://") { gio::File::for_uri(a) } else { gio::File::for_commandline_arg(a) })
            .and_then(|f| f.path());
        if args.iter().any(|a| a == "--new") {
            cal.new_event();
        } else if let Some(path) = file {
            cal.open_file(&path);
        }
        cal.win.window.present();
    })
}

fn nav_button(label: &str, name: &str, f: impl Fn() + 'static) -> gtk::Button {
    let b = gtk::Button::with_label(label);
    b.update_property(&[gtk::accessible::Property::Label(name)]);
    b.connect_clicked(move |_| f());
    b
}

impl Calendar {
    fn new(app: &gtk::Application, cfg: &Config) -> Rc<Self> {
        let win = NativeWindow::new(app, "Calendar", "pda-app-calendar", cfg);
        let pad = cfg.design.padding_px as i32;
        let dir = pim::calendar_dir();

        let header = gtk::Box::new(gtk::Orientation::Horizontal, pad);
        header.add_css_class("today-header");
        header.append(&crate::ui::named_icon("pda-calendar", cfg.design.icon_px));
        let names = gtk::Box::new(gtk::Orientation::Vertical, 0);
        names.set_hexpand(true);
        let title = gtk::Label::new(None);
        title.add_css_class("item-title");
        title.set_xalign(0.0);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let sub = gtk::Label::new(None);
        sub.add_css_class("dim");
        sub.set_xalign(0.0);
        names.append(&title);
        names.append(&sub);
        header.append(&names);
        header.append(&nav_button("◀", "Previous day", with(|c| c.move_day(-1))));
        header.append(&nav_button("Today", "Today", with(|c| c.go_today())));
        header.append(&nav_button("▶", "Next day", with(|c| c.move_day(1))));

        let list = gtk::ListBox::new();
        list.add_css_class("compact");
        list.add_css_class("targets");
        list.add_css_class("lined");
        list.set_selection_mode(gtk::SelectionMode::None);
        list.set_activate_on_single_click(true);
        let note = gtk::Label::new(None);
        note.add_css_class("dim");
        note.add_css_class("page-note");
        note.set_xalign(0.0);
        note.set_wrap(true);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&list);
        content.append(&note);
        let list_scroll =
            gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).vexpand(true).child(&content).build();

        let f_title = gtk::Entry::new();
        let f_location = gtk::Entry::new();
        let f_all_day = gtk::CheckButton::with_label("All day");
        let f_start_date = DateField::new("Start date");
        let f_start_time = TimeField::new("Start");
        let f_end_date = DateField::new("End date");
        let f_end_time = TimeField::new("End");
        let f_notes = forms::notes_view();
        let f_repeats = gtk::Label::new(Some(
            "This event repeats in the program it came from. Only its first date is shown and edited here.",
        ));
        f_repeats.add_css_class("dim");
        f_repeats.set_wrap(true);
        f_repeats.set_xalign(0.0);
        let starts = gtk::Box::new(gtk::Orientation::Horizontal, pad * 2);
        starts.append(&f_start_date.widget);
        starts.append(&f_start_time.widget);
        let ends = gtk::Box::new(gtk::Orientation::Horizontal, pad * 2);
        ends.append(&f_end_date.widget);
        ends.append(&f_end_time.widget);
        let (form, f_error) = forms::form_page(&[
            forms::field("Title", &f_title).upcast_ref(),
            forms::field("Location", &f_location).upcast_ref(),
            f_all_day.upcast_ref(),
            forms::field("Starts", &starts).upcast_ref(),
            forms::field("Ends", &ends).upcast_ref(),
            forms::field("Notes", &f_notes).upcast_ref(),
            f_repeats.upcast_ref(),
        ]);

        let stack = gtk::Stack::new();
        stack.add_named(&list_scroll, Some("list"));
        stack.add_named(&form, Some("edit"));
        let ask = AskBar::new();
        let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        page.append(&header);
        page.append(&ask.widget);
        page.append(&stack);
        win.overlay.set_child(Some(&page));

        let menubar = gio::Menu::new();
        app.set_menubar(Some(&menubar));
        action(app, "new", with(Calendar::new_event));
        let w = &win.window;
        action(w, "day", with(|c| c.show_list(View::Day)));
        action(w, "agenda", with(|c| c.show_list(View::Agenda)));
        action(w, "today", with(Calendar::go_today));
        action(w, "export", with(Calendar::export));
        let edit_actions = vec![
            action(w, "done", with(Calendar::done)),
            action(w, "cancel", with(|c| c.leave_form())),
            action(w, "delete", with(|c| {
                let ev = c.editing.borrow().as_ref().and_then(|e| e.original.clone());
                match ev {
                    Some(ev) => c.ask_delete(ev),
                    None => c.leave_form(),
                }
            })),
        ];

        let cal = Rc::new(Self {
            win,
            cfg: cfg.clone(),
            dir,
            events: Default::default(),
            unreadable: Default::default(),
            day: Cell::new(LocalZones.now().date),
            view: Cell::new(View::Day),
            list_view: Cell::new(View::Day),
            menubar,
            stack,
            header,
            title,
            sub,
            list,
            note,
            ask,
            monitor: RefCell::new(None),
            editing: RefCell::new(None),
            dirty: Cell::new(false),
            f_title,
            f_location,
            f_all_day,
            f_start_date,
            f_start_time,
            f_end_date,
            f_end_time,
            f_notes,
            f_error,
            f_repeats,
            edit_actions,
        });

        cal.list.connect_row_activated(|_, row| {
            if let Some(c) = this() {
                c.open_uid(row.widget_name().as_str());
            }
        });
        let dirty = with(|c| c.dirty.set(true));
        let dirty = Rc::new(dirty);
        for e in [&cal.f_title, &cal.f_location] {
            let d = dirty.clone();
            e.connect_changed(move |_| d());
        }
        let d = dirty.clone();
        cal.f_notes.buffer().connect_changed(move |_| d());
        cal.f_all_day.connect_toggled(|b| {
            if let Some(c) = this() {
                c.dirty.set(true);
                c.f_start_time.widget.set_sensitive(!b.is_active());
                c.f_end_time.widget.set_sensitive(!b.is_active());
            }
        });
        // Moving the start keeps the length; an end before the start moves along.
        let d = dirty.clone();
        cal.f_start_date.connect_changed(move |new| {
            d();
            if let Some(c) = this() {
                let end = c.f_end_date.get();
                if end < new {
                    c.f_end_date.set(new);
                }
            }
        });
        let d = dirty.clone();
        cal.f_end_date.connect_changed(move |_| d());
        let d = dirty.clone();
        cal.f_start_time.connect_changed(move || d());
        let d = dirty;
        cal.f_end_time.connect_changed(move || d());

        cal.win.window.connect_close_request(|_| {
            if let Some(c) = this() {
                c.close_form_on_exit();
            }
            glib::Propagation::Proceed
        });
        // Follow changes made by other programs (and by Today's taps).
        let _ = std::fs::create_dir_all(&cal.dir);
        *cal.monitor.borrow_mut() = gio::File::for_path(&cal.dir)
            .monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
            .ok()
            .inspect(|m| {
                m.connect_changed(|_, _, _, _| {
                    if let Some(c) = this() {
                        c.reload();
                        if c.view.get() != View::Edit {
                            c.refresh();
                        }
                    }
                });
            });
        cal
    }

    fn reload(&self) {
        let (mut evs, bad) = events::load(&self.dir);
        // The same UID twice (a copied file): show it once.
        let mut seen = std::collections::HashSet::new();
        evs.retain(|e| seen.insert(e.uid.clone()));
        for b in &bad {
            log::warn!("cannot read {}: {}", b.path.display(), b.why);
        }
        *self.events.borrow_mut() = evs;
        *self.unreadable.borrow_mut() = bad;
    }

    fn set_menus(&self) {
        self.menubar.remove_all();
        let editing = self.view.get() == View::Edit;
        if editing {
            self.menubar.append(Some("Done"), Some("win.done"));
            self.menubar.append(Some("Cancel"), Some("win.cancel"));
            self.menubar.append(Some("Delete"), Some("win.delete"));
        } else {
            self.menubar.append(Some("New"), Some("app.new"));
            let view = gio::Menu::new();
            let views = super::menu(&[("Day", "win.day"), ("Agenda", "win.agenda"), ("Go to today", "win.today")]);
            view.append_section(None, &views);
            view.append_section(None, &super::menu(&[("Export to Documents", "win.export")]));
            self.menubar.append_submenu(Some("View"), &view);
        }
        for a in &self.edit_actions {
            a.set_enabled(editing);
        }
    }

    fn show_list(&self, view: View) {
        self.view.set(view);
        self.list_view.set(view);
        self.header.set_visible(true);
        self.stack.set_visible_child_name("list");
        self.set_menus();
        self.refresh();
    }

    fn move_day(&self, by: i64) {
        self.day.set(self.day.get().add_days(by));
        if self.view.get() != View::Edit {
            self.show_list(View::Day);
        }
    }

    fn go_today(&self) {
        self.day.set(LocalZones.now().date);
        if self.view.get() != View::Edit {
            self.show_list(View::Day);
        }
    }

    fn event_row(&self, e: &Event, day: Date) -> gtk::ListBoxRow {
        let row = gtk::ListBoxRow::new();
        row.set_widget_name(&e.uid);
        let title = if e.summary.trim().is_empty() { "(no title)".to_string() } else { e.summary.clone() };
        row.update_property(&[gtk::accessible::Property::Label(&title)]);
        let b = gtk::Box::new(gtk::Orientation::Horizontal, self.cfg.design.padding_px as i32 * 2);
        let when = gtk::Label::new(Some(&e.time_label(day, &LocalZones)));
        when.add_css_class("dim");
        when.add_css_class("event-time");
        when.set_xalign(0.0);
        when.set_valign(gtk::Align::Start);
        let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
        text.set_hexpand(true);
        let t = gtk::Label::new(Some(&title));
        t.set_xalign(0.0);
        t.set_ellipsize(gtk::pango::EllipsizeMode::End);
        text.append(&t);
        let mut details = Vec::new();
        if !e.location.is_empty() {
            details.push(e.location.clone());
        }
        if e.repeats() {
            details.push("repeats".into());
        }
        if !details.is_empty() {
            let d = gtk::Label::new(Some(&details.join(" · ")));
            d.add_css_class("dim");
            d.set_xalign(0.0);
            d.set_ellipsize(gtk::pango::EllipsizeMode::End);
            text.append(&d);
        }
        b.append(&when);
        b.append(&text);
        row.set_child(Some(&b));
        let uid = e.uid.clone();
        crate::ui::context_menu(&row, &self.win.feedback, &self.cfg, move || {
            let (u1, u2) = (uid.clone(), uid.clone());
            vec![
                MenuItem::new("Open", move || {
                    if let Some(c) = this() {
                        c.open_uid(&u1);
                    }
                }),
                MenuItem::new("Delete", move || {
                    if let Some(c) = this() {
                        let ev = c.events.borrow().iter().find(|e| e.uid == u2).cloned();
                        if let Some(ev) = ev {
                            c.ask_delete(ev);
                        }
                    }
                }),
            ]
        });
        row
    }

    fn day_heading(&self, d: Date) -> gtk::ListBoxRow {
        let row = gtk::ListBoxRow::new();
        row.set_activatable(false);
        row.add_css_class("day-heading");
        let today = LocalZones.now().date;
        let text = match d.days() - today.days() {
            0 => format!("Today, {}", d.long()),
            1 => format!("Tomorrow, {}", d.long()),
            _ => d.long(),
        };
        let l = gtk::Label::new(Some(&text));
        l.add_css_class("item-title");
        l.set_xalign(0.0);
        row.set_child(Some(&l));
        row
    }

    fn refresh(&self) {
        self.list.remove_all();
        let evs = self.events.borrow();
        let day = self.day.get();
        let today = LocalZones.now().date;
        let count;
        match self.view.get() {
            View::Agenda => {
                self.title.set_text("Agenda");
                self.win.window.set_title(Some("Agenda - Calendar"));
                let mut n = 0;
                for i in 0..AGENDA_DAYS {
                    let d = today.add_days(i);
                    let on = events::on_day(&evs, d, &LocalZones);
                    if on.is_empty() {
                        continue;
                    }
                    self.list.append(&self.day_heading(d));
                    for e in on {
                        self.list.append(&self.event_row(e, d));
                        n += 1;
                    }
                }
                count = n;
                self.sub.set_text(&format!("Next {AGENDA_DAYS} days · {}", plural(n, "event")));
            }
            _ => {
                let label = if day == today { format!("Today, {}", day.long()) } else { day.long() };
                self.title.set_text(&label);
                self.win.window.set_title(Some(&format!("{} - Calendar", day.long())));
                let on = events::on_day(&evs, day, &LocalZones);
                count = on.len();
                for e in on {
                    self.list.append(&self.event_row(e, day));
                }
                self.sub.set_text(&plural(count, "event"));
            }
        }
        let mut note = String::new();
        if count == 0 {
            note.push_str(if self.view.get() == View::Agenda {
                "No events in the next two weeks. Tap New to add one."
            } else {
                "No events on this day. Tap New to add one."
            });
        }
        let bad = self.unreadable.borrow();
        if !bad.is_empty() {
            let names: Vec<String> =
                bad.iter().map(|b| format!("{} ({})", b.path.file_name().unwrap_or_default().to_string_lossy(), b.why)).collect();
            note.push_str(&format!(
                "\n{} could not be read and {} left as they are: {}",
                plural(bad.len(), "calendar file"),
                if bad.len() == 1 { "is" } else { "are" },
                names.join("; ")
            ));
        }
        let note = note.trim().to_string();
        self.note.set_text(&note);
        self.note.set_visible(!note.is_empty());
    }

    /// Before the form shows another event (a second launch reuses this
    /// program): keep the event being edited if it changed, like closing
    /// does. False, and the form stays with the reason, if it cannot be kept.
    fn settle_form(&self) -> bool {
        if self.view.get() != View::Edit || !self.dirty.get() {
            return true;
        }
        match self.form_event().and_then(|x| self.save(&x)) {
            Ok(()) => {
                self.dirty.set(false);
                self.reload();
                true
            }
            Err(why) => {
                self.f_error.set_text(&why);
                self.f_error.set_visible(true);
                self.ask.tell("Finish or cancel the event being edited first.");
                false
            }
        }
    }

    fn open_uid(&self, uid: &str) {
        if !self.settle_form() {
            return;
        }
        let ev = self.events.borrow().iter().find(|e| e.uid == uid).cloned();
        match ev {
            Some(ev) => self.edit(Some(ev)),
            None => self.ask.tell("This event no longer exists."),
        }
    }

    fn open_file(&self, path: &std::path::Path) {
        if !self.settle_form() {
            return;
        }
        self.reload();
        let ev = self.events.borrow().iter().find(|e| e.path == path).cloned();
        match ev {
            Some(ev) => {
                self.day.set(ev.start.local(&LocalZones).date);
                self.edit(Some(ev));
            }
            None => self.ask.tell(&format!("“{}” is not an event in this calendar.", path.display())),
        }
    }

    fn new_event(&self) {
        if !self.settle_form() {
            return;
        }
        let now = LocalZones.now();
        let start = if self.day.get() == now.date {
            // The next full hour.
            DateTime::from_seconds((now.seconds() / 3600 + 1) * 3600)
        } else {
            self.day.get().at(9, 0)
        };
        let ev = Event::new(pim::new_uid(), String::new(), When::Floating(start), Some(When::Floating(start.add_minutes(60))), &self.dir);
        self.edit_inner(ev, None);
    }

    fn edit(&self, ev: Option<Event>) {
        if let Some(ev) = ev {
            self.edit_inner(ev.clone(), Some(ev));
        }
    }

    fn edit_inner(&self, ev: Event, original: Option<Event>) {
        self.ask.hide();
        let (s, e) = ev.span(&LocalZones);
        let all_day = ev.all_day();
        // All-day ends are exclusive: show the last day.
        let end_date = if all_day { e.date.add_days(-1).max(s.date) } else { e.date };
        self.f_title.set_text(&ev.summary);
        self.f_location.set_text(&ev.location);
        self.f_all_day.set_active(all_day);
        self.f_start_date.set(s.date);
        self.f_start_time.set(s.hour, s.minute);
        self.f_end_date.set(end_date);
        self.f_end_time.set(e.hour, e.minute);
        self.f_notes.buffer().set_text(&ev.notes);
        self.f_start_time.widget.set_sensitive(!all_day);
        self.f_end_time.widget.set_sensitive(!all_day);
        self.f_repeats.set_visible(ev.repeats());
        self.f_error.set_visible(false);
        let opened = self.form_times();
        let is_new = original.is_none();
        *self.editing.borrow_mut() = Some(Editing { event: ev, original, opened });
        self.dirty.set(false);
        self.view.set(View::Edit);
        self.header.set_visible(false);
        self.stack.set_visible_child_name("edit");
        self.win.window.set_title(Some(if is_new { "New event" } else { "Edit event" }));
        self.set_menus();
        self.f_title.grab_focus();
    }

    /// Start, end (inclusive last day for all-day events) and the flag, as
    /// the form shows them.
    fn form_times(&self) -> (DateTime, DateTime, bool) {
        let (sh, sm) = self.f_start_time.get();
        let (eh, em) = self.f_end_time.get();
        (self.f_start_date.get().at(sh, sm), self.f_end_date.get().at(eh, em), self.f_all_day.is_active())
    }

    /// The event as the form describes it, or what is wrong with it.
    fn form_event(&self) -> Result<Event, String> {
        let mut ev = self.editing.borrow().as_ref().map(|e| e.event.clone()).ok_or("no event")?;
        let title = self.f_title.text().trim().to_string();
        if title.is_empty() {
            return Err("Give the event a title.".into());
        }
        let (start, end, all_day) = self.form_times();
        let opened = self.editing.borrow().as_ref().map(|e| e.opened);
        if opened != Some((start, end, all_day)) {
            if all_day {
                if end.date < start.date {
                    return Err("The event ends before it starts.".into());
                }
                ev.start = When::Date(start.date);
                ev.end = Some(When::Date(end.date.add_days(1)));
            } else {
                if end < start {
                    return Err("The event ends before it starts.".into());
                }
                ev.start = When::Floating(start);
                ev.end = Some(When::Floating(end));
            }
        }
        ev.summary = title;
        ev.location = self.f_location.text().trim().to_string();
        ev.notes = forms::text_of(&self.f_notes).trim_end().to_string();
        Ok(ev)
    }

    fn save(&self, ev: &Event) -> Result<(), String> {
        let text = crate::pim::ical::write(&ev.to_calendar(LocalZones.now_utc()));
        pim::write_atomic(&ev.path, &text).map_err(|e| format!("Cannot save the event: {e}"))?;
        log::info!("saved event {} ({})", ev.uid, ev.path.display());
        Ok(())
    }

    fn done(&self) {
        let ev = match self.form_event() {
            Ok(ev) => ev,
            Err(why) => {
                self.f_error.set_text(&why);
                self.f_error.set_visible(true);
                return;
            }
        };
        if self.dirty.get() || self.editing.borrow().as_ref().is_some_and(|e| e.original.is_none()) {
            if let Err(why) = self.save(&ev) {
                self.f_error.set_text(&why);
                self.f_error.set_visible(true);
                return;
            }
        }
        self.day.set(ev.start.local(&LocalZones).date);
        self.reload();
        self.leave_form();
    }

    fn leave_form(&self) {
        self.editing.borrow_mut().take();
        self.dirty.set(false);
        self.show_list(self.list_view.get());
    }

    /// Closing with the form open: keep a valid event, drop an empty one.
    fn close_form_on_exit(&self) {
        if self.view.get() != View::Edit || !self.dirty.get() {
            return;
        }
        match self.form_event() {
            Ok(ev) => {
                if let Err(e) = self.save(&ev) {
                    log::error!("{e}");
                }
            }
            Err(why) => log::info!("closing: event not kept ({why})"),
        }
    }

    fn ask_delete(&self, ev: Event) {
        let title = if ev.summary.is_empty() { "this event".to_string() } else { format!("“{}”", ev.summary) };
        self.ask.confirm(&format!("Delete {title}?"), "Delete", move || {
            let Some(c) = this() else { return };
            if let Err(e) = gio::File::for_path(&ev.path).trash(gio::Cancellable::NONE) {
                log::error!("cannot delete {}: {e}", ev.path.display());
                c.ask.tell(&format!("Cannot delete the event: {e}"));
                return;
            }
            c.reload();
            if c.view.get() == View::Edit {
                c.leave_form();
            } else {
                c.refresh();
            }
        });
    }

    fn export(&self) {
        let docs = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join("Documents");
        let path = docs.join(super::fileops::free_name(&docs, "Calendar.ics"));
        let text = events::export(&self.events.borrow(), LocalZones.now_utc());
        match pim::write_atomic(&path, &text) {
            Ok(()) => self.ask.tell(&format!(
                "Exported {} to {}.",
                plural(self.events.borrow().len(), "event"),
                crate::ui::settings::info::tilde(&path)
            )),
            Err(e) => self.ask.tell(&format!("Cannot export: {e}")),
        }
    }
}

/// "1 event", "3 events", "no events".
pub fn plural(n: usize, what: &str) -> String {
    match n {
        0 => format!("no {what}s"),
        1 => format!("1 {what}"),
        n => format!("{n} {what}s"),
    }
}

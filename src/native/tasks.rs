//! Tasks: a local to-do list in the Pocket PC 2003 style. One list (active
//! or completed tasks) with a check box per task, and an edit form. Tasks
//! are iCalendar VTODO files in `pim::tasks_dir()` (see `pim::tasks` for
//! the fields and the sort order).
//!
//! Tapping a check box completes (or reopens) a task at once; tapping the
//! task opens it. Done keeps the edits, Cancel drops them; closing the
//! program with an open form keeps a task that has a title.
//!
//! Command bar: list: New, View (Active, Completed, Export to Documents);
//! form: Done, Cancel, Delete.
//!
//! `pda-tasks [--new] [TASK.ics]`.

use super::calendar::plural;
use super::forms::{self, DateField, LocalZones};
use super::{action, AskBar, NativeWindow};
use crate::config::Config;
use crate::pim::ical::{When, Zones};
use crate::pim::tasks::{self, Priority, Task};
use crate::pim::{self, Unreadable};
use crate::ui::MenuItem;
use gtk::gio;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub const APP_ID: &str = "io.github.pda_shell.Tasks";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Active,
    Completed,
    Edit,
}

struct Editing {
    task: Task,
    is_new: bool,
    /// The due date when the form opened, to keep a foreign due time.
    opened_due: Option<crate::pim::date::Date>,
}

struct Tasks {
    win: NativeWindow,
    cfg: Config,
    dir: PathBuf,
    tasks: RefCell<Vec<Task>>,
    unreadable: RefCell<Vec<Unreadable>>,
    view: Cell<View>,
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
    editing: RefCell<Option<Editing>>,
    dirty: Cell<bool>,
    f_title: gtk::Entry,
    f_priority: Vec<(Priority, gtk::ToggleButton)>,
    f_has_due: gtk::CheckButton,
    f_due: DateField,
    f_done: gtk::CheckButton,
    f_notes: gtk::TextView,
    f_error: gtk::Label,
    edit_actions: Vec<gio::SimpleAction>,
}

thread_local! {
    static TASKS: RefCell<Option<Rc<Tasks>>> = const { RefCell::new(None) };
}

fn this() -> Option<Rc<Tasks>> {
    TASKS.with(|t| t.borrow().clone())
}

fn with(f: impl Fn(&Tasks) + 'static) -> impl Fn() + 'static {
    move || {
        if let Some(t) = this() {
            f(&t);
        }
    }
}

/// Entry point of `pda-tasks`.
pub fn main() -> glib::ExitCode {
    super::run(APP_ID, "pda-tasks", |app, cfg, args| {
        let t = this().unwrap_or_else(|| {
            let t = Tasks::new(app, cfg);
            TASKS.with(|cell| *cell.borrow_mut() = Some(t.clone()));
            t.reload();
            t.show_list(View::Active);
            t
        });
        let file = args
            .iter()
            .find(|a| !a.starts_with("--"))
            .map(|a| if a.contains("://") { gio::File::for_uri(a) } else { gio::File::for_commandline_arg(a) })
            .and_then(|f| f.path());
        if args.iter().any(|a| a == "--new") {
            t.new_task();
        } else if let Some(path) = file {
            t.open_file(&path);
        }
        t.win.window.present();
    })
}

impl Tasks {
    fn new(app: &gtk::Application, cfg: &Config) -> Rc<Self> {
        let win = NativeWindow::new(app, "Tasks", "pda-app-tasks", cfg);
        let pad = cfg.design.padding_px as i32;
        let dir = pim::tasks_dir();

        let header = gtk::Box::new(gtk::Orientation::Horizontal, pad);
        header.add_css_class("today-header");
        header.append(&crate::ui::named_icon("pda-tasks", cfg.design.icon_px));
        let names = gtk::Box::new(gtk::Orientation::Vertical, 0);
        names.set_hexpand(true);
        let title = gtk::Label::new(None);
        title.add_css_class("item-title");
        title.set_xalign(0.0);
        let sub = gtk::Label::new(None);
        sub.add_css_class("dim");
        sub.set_xalign(0.0);
        sub.set_ellipsize(gtk::pango::EllipsizeMode::End);
        names.append(&title);
        names.append(&sub);
        header.append(&names);

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
        let prio_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        prio_box.add_css_class("linked");
        let mut f_priority: Vec<(Priority, gtk::ToggleButton)> = Vec::new();
        for p in [Priority::High, Priority::Normal, Priority::Low] {
            let b = gtk::ToggleButton::with_label(p.label());
            if let Some((_, first)) = f_priority.first() {
                b.set_group(Some(first));
            }
            prio_box.append(&b);
            f_priority.push((p, b));
        }
        let f_has_due = gtk::CheckButton::with_label("Due date");
        let f_due = DateField::new("Due date");
        let due_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
        due_box.append(&f_has_due);
        due_box.append(&f_due.widget);
        let f_done = gtk::CheckButton::with_label("Completed");
        let f_notes = forms::notes_view();
        let (form, f_error) = forms::form_page(&[
            forms::field("Title", &f_title).upcast_ref(),
            forms::field("Priority", &prio_box).upcast_ref(),
            due_box.upcast_ref(),
            f_done.upcast_ref(),
            forms::field("Notes", &f_notes).upcast_ref(),
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
        action(app, "new", with(Tasks::new_task));
        let w = &win.window;
        action(w, "active", with(|t| t.show_list(View::Active)));
        action(w, "completed", with(|t| t.show_list(View::Completed)));
        action(w, "export", with(Tasks::export));
        let edit_actions = vec![
            action(w, "done", with(Tasks::done)),
            action(w, "cancel", with(Tasks::leave_form)),
            action(w, "delete", with(|t| {
                let task = t.editing.borrow().as_ref().filter(|e| !e.is_new).map(|e| e.task.clone());
                match task {
                    Some(task) => t.ask_delete(task),
                    None => t.leave_form(),
                }
            })),
        ];

        let t = Rc::new(Self {
            win,
            cfg: cfg.clone(),
            dir,
            tasks: Default::default(),
            unreadable: Default::default(),
            view: Cell::new(View::Active),
            list_view: Cell::new(View::Active),
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
            f_priority,
            f_has_due,
            f_due,
            f_done,
            f_notes,
            f_error,
            edit_actions,
        });

        t.list.connect_row_activated(|_, row| {
            if let Some(t) = this() {
                t.open_uid(row.widget_name().as_str());
            }
        });
        let dirty = Rc::new(with(|t| t.dirty.set(true)));
        let d = dirty.clone();
        t.f_title.connect_changed(move |_| d());
        let d = dirty.clone();
        t.f_notes.buffer().connect_changed(move |_| d());
        for (_, b) in &t.f_priority {
            let d = dirty.clone();
            b.connect_toggled(move |_| d());
        }
        let d = dirty.clone();
        t.f_done.connect_toggled(move |_| d());
        t.f_has_due.connect_toggled(|b| {
            if let Some(t) = this() {
                t.dirty.set(true);
                t.f_due.widget.set_sensitive(b.is_active());
            }
        });
        let d = dirty;
        t.f_due.connect_changed(move |_| d());
        t.win.window.connect_close_request(|_| {
            if let Some(t) = this() {
                t.close_form_on_exit();
            }
            glib::Propagation::Proceed
        });
        let _ = std::fs::create_dir_all(&t.dir);
        *t.monitor.borrow_mut() = gio::File::for_path(&t.dir)
            .monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
            .ok()
            .inspect(|m| {
                m.connect_changed(|_, _, _, _| {
                    if let Some(t) = this() {
                        t.reload();
                        if t.view.get() != View::Edit {
                            t.refresh();
                        }
                    }
                });
            });
        t
    }

    fn reload(&self) {
        let (mut ts, bad) = tasks::load(&self.dir);
        let mut seen = std::collections::HashSet::new();
        ts.retain(|t| seen.insert(t.uid.clone()));
        for b in &bad {
            log::warn!("cannot read {}: {}", b.path.display(), b.why);
        }
        *self.tasks.borrow_mut() = ts;
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
            view.append_section(None, &super::menu(&[("Active tasks", "win.active"), ("Completed tasks", "win.completed")]));
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

    fn task_row(&self, t: &Task) -> gtk::ListBoxRow {
        let today = LocalZones.now().date;
        let row = gtk::ListBoxRow::new();
        row.set_widget_name(&t.uid);
        let title = if t.summary.trim().is_empty() { "(no title)".to_string() } else { t.summary.clone() };
        row.update_property(&[gtk::accessible::Property::Label(&title)]);
        let b = gtk::Box::new(gtk::Orientation::Horizontal, self.cfg.design.padding_px as i32);
        let check = gtk::CheckButton::new();
        check.set_active(t.done);
        check.set_valign(gtk::Align::Center);
        check.add_css_class("task-check");
        let verb = if t.done { "Reopen" } else { "Complete" };
        check.update_property(&[gtk::accessible::Property::Label(&format!("{verb} {title}"))]);
        let uid = t.uid.clone();
        check.connect_toggled(move |c| {
            if let Some(t) = this() {
                t.set_done(&uid, c.is_active());
            }
        });
        let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
        text.set_hexpand(true);
        text.set_valign(gtk::Align::Center);
        let l = gtk::Label::new(Some(&title));
        l.set_xalign(0.0);
        l.set_ellipsize(gtk::pango::EllipsizeMode::End);
        if t.done {
            l.add_css_class("task-done");
        }
        text.append(&l);
        let mut details = Vec::new();
        let due = t.due_label(today, &LocalZones);
        if !due.is_empty() {
            details.push(due);
        }
        if t.priority != Priority::Normal {
            details.push(format!("{} priority", t.priority.label()));
        }
        if !details.is_empty() {
            let d = gtk::Label::new(Some(&details.join(" · ")));
            d.add_css_class("dim");
            if t.overdue(today, &LocalZones) {
                d.add_css_class("overdue");
            }
            d.set_xalign(0.0);
            text.append(&d);
        }
        b.append(&check);
        b.append(&text);
        if t.priority == Priority::High && !t.done {
            let mark = gtk::Label::new(Some("!"));
            mark.add_css_class("priority-mark");
            b.append(&mark);
        }
        row.set_child(Some(&b));
        let (uid, done) = (t.uid.clone(), t.done);
        crate::ui::context_menu(&row, &self.win.feedback, &self.cfg, move || {
            let (u1, u2, u3) = (uid.clone(), uid.clone(), uid.clone());
            vec![
                MenuItem::new("Open", move || {
                    if let Some(t) = this() {
                        t.open_uid(&u1);
                    }
                }),
                MenuItem::new(if done { "Mark as not done" } else { "Complete" }, move || {
                    if let Some(t) = this() {
                        t.set_done(&u2, !done);
                    }
                }),
                MenuItem::new("Delete", move || {
                    if let Some(t) = this() {
                        let task = t.tasks.borrow().iter().find(|x| x.uid == u3).cloned();
                        if let Some(task) = task {
                            t.ask_delete(task);
                        }
                    }
                }),
            ]
        });
        row
    }

    fn refresh(&self) {
        self.list.remove_all();
        let all = self.tasks.borrow();
        let completed = self.view.get() == View::Completed;
        let shown = tasks::sorted(&all, completed, &LocalZones);
        for t in &shown {
            self.list.append(&self.task_row(t));
        }
        let s = tasks::summary(&all, &LocalZones);
        if completed {
            self.title.set_text("Completed tasks");
            self.sub.set_text(&plural(shown.len(), "completed task"));
            self.win.window.set_title(Some("Completed - Tasks"));
        } else {
            self.title.set_text("Tasks");
            self.sub.set_text(&tasks::summary_line(&s));
            self.win.window.set_title(Some("Tasks"));
        }
        let mut note = String::new();
        if shown.is_empty() {
            note.push_str(if completed { "No completed tasks." } else { "Nothing to do. Tap New to add a task." });
        }
        let bad = self.unreadable.borrow();
        if !bad.is_empty() {
            let names: Vec<String> =
                bad.iter().map(|b| format!("{} ({})", b.path.file_name().unwrap_or_default().to_string_lossy(), b.why)).collect();
            note.push_str(&format!("\n{} could not be read and left as they are: {}", plural(bad.len(), "task file"), names.join("; ")));
        }
        let note = note.trim().to_string();
        self.note.set_text(&note);
        self.note.set_visible(!note.is_empty());
    }

    fn save(&self, task: &Task) -> Result<(), String> {
        let text = crate::pim::ical::write(&task.to_calendar(LocalZones.now_utc()));
        pim::write_atomic(&task.path, &text).map_err(|e| format!("Cannot save the task: {e}"))?;
        log::info!("saved task {} ({})", task.uid, task.path.display());
        Ok(())
    }

    /// Complete or reopen a task from the list (or Today, via the file).
    fn set_done(&self, uid: &str, done: bool) {
        let task = self.tasks.borrow().iter().find(|t| t.uid == uid).cloned();
        let Some(mut task) = task else { return };
        if task.done == done {
            return;
        }
        task.set_done(done, LocalZones.now_utc());
        if let Err(e) = self.save(&task) {
            self.ask.tell(&e);
        }
        self.reload();
        // Leave the check box's signal handler before rebuilding the rows.
        glib::idle_add_local_once(|| {
            if let Some(t) = this() {
                if t.view.get() != View::Edit {
                    t.refresh();
                }
            }
        });
    }

    fn open_uid(&self, uid: &str) {
        let task = self.tasks.borrow().iter().find(|t| t.uid == uid).cloned();
        match task {
            Some(task) => self.edit(task, false),
            None => self.ask.tell("This task no longer exists."),
        }
    }

    fn open_file(&self, path: &Path) {
        self.reload();
        let task = self.tasks.borrow().iter().find(|t| t.path == path).cloned();
        match task {
            Some(task) => self.edit(task, false),
            None => self.ask.tell(&format!("“{}” is not a task in this list.", path.display())),
        }
    }

    fn new_task(&self) {
        let task = Task::new(pim::new_uid(), String::new(), &self.dir);
        self.edit(task, true);
    }

    fn edit(&self, task: Task, is_new: bool) {
        self.ask.hide();
        self.f_title.set_text(&task.summary);
        for (p, b) in &self.f_priority {
            b.set_active(*p == task.priority);
        }
        let due = task.due_date(&LocalZones);
        self.f_has_due.set_active(due.is_some());
        self.f_due.set(due.unwrap_or_else(|| LocalZones.now().date));
        self.f_due.widget.set_sensitive(due.is_some());
        self.f_done.set_active(task.done);
        self.f_notes.buffer().set_text(&task.notes);
        self.f_error.set_visible(false);
        *self.editing.borrow_mut() = Some(Editing { task, is_new, opened_due: due });
        self.dirty.set(false);
        self.view.set(View::Edit);
        self.header.set_visible(false);
        self.stack.set_visible_child_name("edit");
        self.win.window.set_title(Some(if is_new { "New task" } else { "Edit task" }));
        self.set_menus();
        self.f_title.grab_focus();
    }

    fn form_task(&self) -> Result<Task, String> {
        let editing = self.editing.borrow();
        let e = editing.as_ref().ok_or("no task")?;
        let mut task = e.task.clone();
        let title = self.f_title.text().trim().to_string();
        if title.is_empty() {
            return Err("Give the task a title.".into());
        }
        task.summary = title;
        task.priority = self.f_priority.iter().find(|(_, b)| b.is_active()).map_or(Priority::Normal, |(p, _)| *p);
        let due = self.f_has_due.is_active().then(|| self.f_due.get());
        if due != e.opened_due {
            task.due = due.map(When::Date);
        }
        let done = self.f_done.is_active();
        if done != task.done {
            task.set_done(done, LocalZones.now_utc());
        }
        task.notes = forms::text_of(&self.f_notes).trim_end().to_string();
        Ok(task)
    }

    fn done(&self) {
        let task = match self.form_task() {
            Ok(t) => t,
            Err(why) => {
                self.f_error.set_text(&why);
                self.f_error.set_visible(true);
                return;
            }
        };
        let is_new = self.editing.borrow().as_ref().is_some_and(|e| e.is_new);
        if self.dirty.get() || is_new {
            if let Err(why) = self.save(&task) {
                self.f_error.set_text(&why);
                self.f_error.set_visible(true);
                return;
            }
        }
        self.reload();
        self.leave_form();
    }

    fn leave_form(&self) {
        self.editing.borrow_mut().take();
        self.dirty.set(false);
        self.show_list(self.list_view.get());
    }

    fn close_form_on_exit(&self) {
        if self.view.get() != View::Edit || !self.dirty.get() {
            return;
        }
        match self.form_task() {
            Ok(task) => {
                if let Err(e) = self.save(&task) {
                    log::error!("{e}");
                }
            }
            Err(why) => log::info!("closing: task not kept ({why})"),
        }
    }

    fn ask_delete(&self, task: Task) {
        self.ask.confirm(&format!("Delete the task “{}”?", task.summary), "Delete", move || {
            let Some(t) = this() else { return };
            if let Err(e) = gio::File::for_path(&task.path).trash(gio::Cancellable::NONE) {
                log::error!("cannot delete {}: {e}", task.path.display());
                t.ask.tell(&format!("Cannot delete the task: {e}"));
                return;
            }
            t.reload();
            if t.view.get() == View::Edit {
                t.leave_form();
            } else {
                t.refresh();
            }
        });
    }

    fn export(&self) {
        let docs = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join("Documents");
        let path = docs.join(super::fileops::free_name(&docs, "Tasks.ics"));
        let mut cal = pim::wrap(crate::pim::ical::Component::new("VTODO"), None);
        cal.children.clear();
        let stamp = LocalZones.now_utc();
        for t in self.tasks.borrow().iter() {
            cal.children.extend(t.to_calendar(stamp).children.into_iter().filter(|c| c.kind == "VTODO"));
        }
        match pim::write_atomic(&path, &crate::pim::ical::write(&cal)) {
            Ok(()) => self.ask.tell(&format!(
                "Exported {} to {}.",
                plural(self.tasks.borrow().len(), "task"),
                crate::ui::settings::info::tilde(&path)
            )),
            Err(e) => self.ask.tell(&format!("Cannot export: {e}")),
        }
    }
}

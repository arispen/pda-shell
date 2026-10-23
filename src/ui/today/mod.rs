//! Today screen: the date first, then what the user is working on (recent
//! documents, open programs), then program shortcuts and a little real
//! system information. It is the desktop surface below all application
//! windows.

pub mod info;
pub mod recent;

use super::command_bar::{switcher_row, SwitcherEntry};
use super::{MenuItem, ShellWindow};
use crate::apps::{self, AppEntry};
use crate::config::Config;
use crate::platform::x11::X11;
use crate::platform::SurfaceRole;
use crate::shell::layout::Layout;
use crate::shell::state::Event;
use crate::shell::{Command, Dispatch};
use gtk::gio;
use gtk::prelude::*;
use std::rc::Rc;

pub struct Today {
    pub win: ShellWindow,
    cfg: Config,
    layout: Layout,
    dispatch: Dispatch,
    date: gtk::Label,
    time: gtk::Label,
    summary: gtk::Label,
    shortcuts: gtk::Grid,
    recent: gtk::ListBox,
    running_col: gtk::Box,
    running: gtk::ListBox,
    running_count: std::cell::Cell<usize>,
    /// Documents removed from the list in this session.
    forgotten: std::cell::RefCell<std::collections::HashSet<std::path::PathBuf>>,
    info: gtk::Label,
}

fn section(title: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(title));
    l.add_css_class("section-title");
    l.set_xalign(0.0);
    l
}

impl Today {
    pub fn new(app: &gtk::Application, layout: &Layout, x11: &Rc<X11>, cfg: &Config, dispatch: Dispatch) -> Rc<Self> {
        let win = ShellWindow::new(app, "Today", "today", SurfaceRole::Desktop, layout.workarea, x11, cfg);
        let pad = cfg.design.padding_px as i32;

        let header = gtk::Box::new(gtk::Orientation::Horizontal, pad);
        header.add_css_class("today-header");
        let heading = gtk::Box::new(gtk::Orientation::Vertical, 0);
        heading.set_hexpand(true);
        let date = gtk::Label::new(None);
        date.add_css_class("date");
        date.set_xalign(0.0);
        let summary = gtk::Label::new(None);
        summary.add_css_class("summary");
        summary.set_xalign(0.0);
        heading.append(&date);
        heading.append(&summary);
        let time = gtk::Label::new(None);
        time.add_css_class("time");
        time.set_valign(gtk::Align::Start);
        header.append(&heading);
        header.append(&time);

        let shortcuts = gtk::Grid::new();
        shortcuts.set_column_homogeneous(true);
        shortcuts.set_row_spacing(cfg.design.spacing_px);
        shortcuts.set_column_spacing(cfg.design.spacing_px);
        shortcuts.set_margin_start(pad);
        shortcuts.set_margin_end(pad);
        shortcuts.set_margin_top(pad);

        let recent = gtk::ListBox::new();
        recent.add_css_class("compact");
        recent.set_selection_mode(gtk::SelectionMode::None);
        recent.set_activate_on_single_click(true);
        let d = dispatch.clone();
        recent.connect_row_activated(move |_, row| d(Command::OpenUri(row.widget_name().to_string())));

        let running = gtk::ListBox::new();
        running.add_css_class("compact");
        running.set_selection_mode(gtk::SelectionMode::None);
        running.set_activate_on_single_click(true);
        let d = dispatch.clone();
        running.connect_row_activated(move |_, row| {
            if let Some(id) = row.widget_name().strip_prefix("win-").and_then(|s| s.parse().ok()) {
                d(Command::Shell(Event::Switch(crate::platform::WindowId(id))));
            }
        });

        let info = gtk::Label::new(None);
        info.add_css_class("dim");
        info.set_xalign(0.0);
        info.set_wrap(true);
        info.set_margin_start(pad);
        info.set_margin_end(pad);
        info.set_margin_top(pad);
        info.set_selectable(false);

        let programs_col = gtk::Box::new(gtk::Orientation::Vertical, 0);
        programs_col.append(&section("Programs"));
        programs_col.append(&shortcuts);
        let recent_col = gtk::Box::new(gtk::Orientation::Vertical, 0);
        recent_col.append(&section("Recent documents"));
        recent_col.append(&recent);
        let running_col = gtk::Box::new(gtk::Orientation::Vertical, 0);
        running_col.append(&section("Open programs"));
        running_col.append(&running);
        running_col.set_visible(false);
        let info_col = gtk::Box::new(gtk::Orientation::Vertical, 0);
        info_col.append(&section("System"));
        info_col.append(&info);

        // What the user is working on comes first: documents and open
        // programs, then shortcuts, then system details.
        let body = if layout.today_side_by_side {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, pad);
            row.set_homogeneous(true);
            let left = gtk::Box::new(gtk::Orientation::Vertical, pad);
            left.append(&recent_col);
            left.append(&running_col);
            let right = gtk::Box::new(gtk::Orientation::Vertical, pad);
            right.append(&programs_col);
            right.append(&info_col);
            row.append(&left);
            row.append(&right);
            row
        } else {
            let col = gtk::Box::new(gtk::Orientation::Vertical, pad);
            col.append(&recent_col);
            col.append(&running_col);
            col.append(&programs_col);
            col.append(&info_col);
            col
        };
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&body)
            .build();
        let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        page.append(&header);
        page.append(&scroller);
        win.set_content(&page);

        let today = Rc::new(Self {
            win,
            cfg: cfg.clone(),
            layout: layout.clone(),
            dispatch,
            date,
            time,
            summary,
            shortcuts,
            recent,
            running_col,
            running,
            running_count: std::cell::Cell::new(0),
            forgotten: Default::default(),
            info,
        });
        today.tick();
        let t = Rc::downgrade(&today);
        glib::timeout_add_seconds_local(1, move || match t.upgrade() {
            Some(t) => {
                t.tick();
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });
        let t = Rc::downgrade(&today);
        gtk::RecentManager::default().connect_changed(move |_| {
            if let Some(t) = t.upgrade() {
                t.refresh_recent();
            }
        });
        today
    }

    fn tick(&self) {
        let Ok(now) = glib::DateTime::now_local() else { return };
        if let Ok(s) = now.format("%A, %e %B %Y") {
            let s = s.replace("  ", " ");
            if self.date.text() != s {
                self.date.set_text(&s);
            }
        }
        if let Ok(s) = now.format("%H:%M") {
            if self.time.text() != s {
                self.time.set_text(&s);
            }
        }
        self.update_summary(&now);
    }

    fn update_summary(&self, now: &glib::DateTime) {
        let mut parts = vec![format!("Week {}", now.week_of_year())];
        match self.running_count.get() {
            0 => parts.push("no programs open".into()),
            1 => parts.push("1 program open".into()),
            n => parts.push(format!("{n} programs open")),
        }
        let s = parts.join(" · ");
        if self.summary.text() != s {
            self.summary.set_text(&s);
        }
    }

    /// Open programs, most recently used first; tapping one switches to it.
    pub fn set_running(&self, entries: &[SwitcherEntry]) {
        self.running.remove_all();
        for e in entries {
            self.running.append(&switcher_row(&SwitcherEntry { active: false, ..e.clone() }, self.cfg.design.icon_px));
        }
        self.running_col.set_visible(!entries.is_empty());
        self.running_count.set(entries.len());
        if let Ok(now) = glib::DateTime::now_local() {
            self.update_summary(&now);
        }
    }

    pub fn set_shortcuts(&self, entries: &[&AppEntry]) {
        while let Some(c) = self.shortcuts.first_child() {
            self.shortcuts.remove(&c);
        }
        let cols = self.layout.today_columns as i32;
        let d = &self.cfg.design;
        for (i, entry) in entries.iter().enumerate() {
            let button = gtk::Button::new();
            button.add_css_class("shortcut");
            let content = gtk::Box::new(gtk::Orientation::Vertical, 1);
            content.append(&super::icon_image(
                apps::gicon(entry).as_ref(),
                "application-x-executable",
                d.today_icon_px,
            ));
            let label = gtk::Label::new(Some(&entry.name));
            label.set_wrap(true);
            label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            label.set_lines(2);
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            label.set_justify(gtk::Justification::Center);
            label.set_max_width_chars(10);
            content.append(&label);
            button.set_child(Some(&content));
            button.set_tooltip_text(Some(&entry.name));

            let id = entry.id.clone();
            let dsp = self.dispatch.clone();
            button.connect_clicked(move |_| dsp(Command::Shell(Event::Launch(id.clone()))));

            let (id, dsp) = (entry.id.clone(), self.dispatch.clone());
            super::context_menu(&button, &self.win.feedback, &self.cfg, move || {
                let (a, b) = (dsp.clone(), dsp.clone());
                let (ia, ib) = (id.clone(), id.clone());
                vec![
                    MenuItem::new("Open", move || a(Command::Shell(Event::Launch(ia.clone())))),
                    MenuItem::new("Remove from Today", move || b(Command::RemoveShortcut(ib.clone()))),
                ]
            });
            self.shortcuts.attach(&button, i as i32 % cols, i as i32 / cols, 1, 1);
        }
        if entries.is_empty() {
            let l = gtk::Label::new(Some("No shortcuts. Long-press a program in Start to add one."));
            l.add_css_class("dim");
            l.set_wrap(true);
            self.shortcuts.attach(&l, 0, 0, cols, 1);
        }
    }

    /// Leave a document out of the list for the rest of the session.
    pub fn forget(&self, uri: &str) {
        if let Some(path) = gio::File::for_uri(uri).path() {
            self.forgotten.borrow_mut().insert(path);
        }
        self.refresh_recent();
    }

    pub fn refresh_recent(&self) {
        self.recent.remove_all();
        let mut docs: Vec<recent::Doc> = gtk::RecentManager::default()
            .items()
            .into_iter()
            .filter(|i| i.exists())
            .filter_map(|i| {
                let path = gio::File::for_uri(&i.uri()).path()?;
                Some(recent::Doc { path, modified: i.modified().to_unix() })
            })
            .collect();
        if let Some(home) = std::env::var_os("HOME") {
            docs.extend(recent::scan(&std::path::Path::new(&home).join("Documents"), 4));
        }
        docs.retain(|d| !self.forgotten.borrow().contains(&d.path));
        let docs = recent::merge(docs, self.cfg.today.recent_documents);
        let px = self.cfg.design.icon_px;
        for doc in &docs {
            let file = gio::File::for_path(&doc.path);
            let uri = file.uri().to_string();
            let row = gtk::ListBoxRow::new();
            row.set_widget_name(&uri);
            let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            let name = doc.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let (content_type, _) = gio::content_type_guess(Some(&doc.path), None::<&[u8]>);
            b.append(&super::icon_image(Some(&gio::content_type_get_icon(&content_type)), "text-x-generic", px));
            let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
            text.set_valign(gtk::Align::Center);
            let title = gtk::Label::new(Some(&name));
            title.set_xalign(0.0);
            title.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
            let folder = doc.path.parent().map(info::tilde).unwrap_or_default();
            let when = glib::DateTime::from_unix_local(doc.modified)
                .ok()
                .and_then(|t| t.format("%d %b %H:%M").ok())
                .map(|s| s.to_string())
                .unwrap_or_default();
            let sub = gtk::Label::new(Some(&format!("{when} · {folder}")));
            sub.add_css_class("dim");
            sub.set_xalign(0.0);
            sub.set_ellipsize(gtk::pango::EllipsizeMode::Start);
            text.append(&title);
            text.append(&sub);
            b.append(&text);
            row.set_child(Some(&b));
            row.set_tooltip_text(Some(&doc.path.display().to_string()));
            row.update_property(&[gtk::accessible::Property::Label(&name)]);

            let dsp = self.dispatch.clone();
            let u = uri.clone();
            super::context_menu(&row, &self.win.feedback, &self.cfg, move || {
                let (a, b, c) = (dsp.clone(), dsp.clone(), dsp.clone());
                let (ua, ub, uc) = (u.clone(), u.clone(), u.clone());
                vec![
                    MenuItem::new("Open", move || a(Command::OpenUri(ua.clone()))),
                    MenuItem::new("Open containing folder", move || b(Command::OpenFolder(ub.clone()))),
                    MenuItem::new("Remove from list", move || c(Command::ForgetRecent(uc.clone()))),
                ]
            });
            self.recent.append(&row);
        }
        if docs.is_empty() {
            let row = gtk::ListBoxRow::new();
            row.set_activatable(false);
            let l = gtk::Label::new(Some("No documents yet."));
            l.add_css_class("dim");
            l.set_xalign(0.0);
            row.set_child(Some(&l));
            self.recent.append(&row);
        }
    }

    pub fn refresh_info(&self) {
        let l = &self.layout;
        let mut lines = info::system_lines();
        lines.push(format!("Screen {}×{} {}", l.screen.width, l.screen.height, l.orientation.name()));
        self.info.set_text(&lines.join("\n"));
    }
}

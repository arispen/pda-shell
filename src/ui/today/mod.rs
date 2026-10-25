//! Today: the date first, then glanceable items in the Pocket PC 2003
//! order: Calendar, Tasks, Messages, then the documents and programs the
//! user is working on and their favorite programs. Each item is a row with a
//! large icon at the left, separated by thin lines.
//!
//! Calendar, Tasks and Messages have no data source yet; they say so instead
//! of showing invented entries. System details are in Settings → About.
//!
//! The Today window is the desktop surface below all application windows,
//! so it also hosts the other shell screens (Programs, Settings) in a stack.

pub mod recent;

use super::command_bar::{switcher_row, SwitcherEntry};
use super::programs::Programs;
use super::settings::{info, Settings};
use super::{MenuItem, ShellWindow};
use crate::apps::{self, AppEntry};
use crate::config::Config;
use crate::platform::x11::X11;
use crate::platform::SurfaceRole;
use crate::shell::layout::Layout;
use crate::shell::state::{Event, Page};
use crate::shell::{Command, Dispatch};
use gtk::gio;
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

pub struct Today {
    pub win: ShellWindow,
    cfg: Config,
    layout: Layout,
    dispatch: Dispatch,
    stack: gtk::Stack,
    programs: Programs,
    settings: Settings,
    date: gtk::Label,
    summary: gtk::Label,
    shortcuts: gtk::Box,
    recent: gtk::ListBox,
    running_item: gtk::Box,
    running: gtk::ListBox,
    running_count: std::cell::Cell<usize>,
    /// Documents removed from the list in this session.
    forgotten: std::cell::RefCell<std::collections::HashSet<std::path::PathBuf>>,
}

/// A Today item: large icon at the left, bold title, then `content`.
fn item(cfg: &Config, icon: &str, title: &str, content: &impl IsA<gtk::Widget>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, cfg.design.padding_px as i32 * 2);
    row.add_css_class("today-item");
    let image = super::named_icon(icon, cfg.design.today_icon_px);
    image.set_valign(gtk::Align::Start);
    row.append(&image);
    let col = gtk::Box::new(gtk::Orientation::Vertical, 0);
    col.set_hexpand(true);
    col.set_valign(gtk::Align::Center);
    let t = gtk::Label::new(Some(title));
    t.add_css_class("item-title");
    t.set_xalign(0.0);
    col.append(&t);
    col.append(content);
    row.append(&col);
    row
}

/// Dim one-line text inside an item.
fn line(text: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.add_css_class("dim");
    l.set_xalign(0.0);
    l.set_wrap(true);
    l
}

fn list() -> gtk::ListBox {
    let l = gtk::ListBox::new();
    l.add_css_class("compact");
    l.add_css_class("targets");
    l.set_selection_mode(gtk::SelectionMode::None);
    l.set_activate_on_single_click(true);
    l
}

/// Icon for a document: one of the shell's document icons by content type.
fn document_icon(content_type: &str) -> &'static str {
    let t = content_type.to_lowercase();
    if t.contains("spreadsheet") || t.contains("excel") || t.contains("csv") {
        "pda-doc-spreadsheet"
    } else if t.contains("pdf") {
        "pda-doc-pdf"
    } else if t.contains("html") {
        "pda-doc-web"
    } else if t.starts_with("text/") || t.contains("document") || t.contains("word") {
        "pda-doc-text"
    } else {
        "pda-doc"
    }
}

impl Today {
    pub fn new(
        app: &gtk::Application,
        layout: &Layout,
        x11: &Rc<X11>,
        cfg: &Config,
        dispatch: Dispatch,
        favorites: Rc<RefCell<Vec<String>>>,
    ) -> Rc<Self> {
        let win = ShellWindow::new(app, "Today", "today", SurfaceRole::Desktop, layout.workarea, x11, cfg);

        let header = gtk::Box::new(gtk::Orientation::Vertical, 0);
        header.add_css_class("today-header");
        let date = gtk::Label::new(None);
        date.add_css_class("date");
        date.set_xalign(0.0);
        let summary = gtk::Label::new(None);
        summary.add_css_class("summary");
        summary.set_xalign(0.0);
        header.append(&date);
        header.append(&summary);

        let recent = list();
        let d = dispatch.clone();
        recent.connect_row_activated(move |_, row| d(Command::OpenUri(row.widget_name().to_string())));

        let running = list();
        let d = dispatch.clone();
        running.connect_row_activated(move |_, row| {
            if let Some(id) = row.widget_name().strip_prefix("win-").and_then(|s| s.parse().ok()) {
                d(Command::Shell(Event::Switch(crate::platform::WindowId(id))));
            }
        });

        let shortcuts = gtk::Box::new(gtk::Orientation::Vertical, 0);

        let calendar = item(cfg, "pda-calendar", "Calendar", &line("No calendar connected"));
        let tasks = item(cfg, "pda-tasks", "Tasks", &line("No task list connected"));
        let messages = item(cfg, "pda-messages", "Messages", &line("No mail account set up"));
        let documents = item(cfg, "pda-documents", "Documents", &recent);
        let running_item = item(cfg, "pda-windows", "Open programs", &running);
        running_item.set_visible(false);
        let favorites_item = item(cfg, "pda-favorites", "Favorites", &shortcuts);

        let body = if layout.today_side_by_side {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            row.set_homogeneous(true);
            let left = gtk::Box::new(gtk::Orientation::Vertical, 0);
            left.append(&calendar);
            left.append(&tasks);
            left.append(&messages);
            left.append(&favorites_item);
            let right = gtk::Box::new(gtk::Orientation::Vertical, 0);
            right.add_css_class("today-column");
            right.append(&documents);
            right.append(&running_item);
            row.append(&left);
            row.append(&right);
            row
        } else {
            let col = gtk::Box::new(gtk::Orientation::Vertical, 0);
            for w in [&calendar, &tasks, &messages, &documents, &running_item, &favorites_item] {
                col.append(w);
            }
            col
        };
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&body)
            .build();
        let today_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        today_page.append(&header);
        today_page.append(&scroller);

        let programs = Programs::new(layout, cfg, dispatch.clone(), win.feedback.clone(), favorites);
        let settings = Settings::new(layout, cfg, dispatch.clone());
        let stack = gtk::Stack::new();
        stack.set_transition_type(gtk::StackTransitionType::None);
        for page in Page::ALL {
            let child: gtk::Widget = match page {
                Page::Today => today_page.clone().upcast(),
                Page::Programs => programs.programs.clone().upcast(),
                Page::LinuxTools => programs.tools.clone().upcast(),
                Page::Settings => settings.settings.clone().upcast(),
                Page::About => settings.about.clone().upcast(),
                Page::Screen => settings.screen.clone().upcast(),
                Page::Network => settings.network.clone().upcast(),
                Page::LinuxSettings => settings.linux_settings.clone().upcast(),
            };
            stack.add_named(&child, Some(page.title()));
        }
        stack.set_visible_child_name(Page::Today.title());
        win.set_content(&stack);

        let today = Rc::new(Self {
            win,
            cfg: cfg.clone(),
            layout: layout.clone(),
            dispatch,
            stack,
            programs,
            settings,
            date,
            summary,
            shortcuts,
            recent,
            running_item,
            running,
            running_count: std::cell::Cell::new(0),
            forgotten: Default::default(),
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

    /// Switch to one of the shell screens.
    pub fn show_page(&self, page: Page) {
        self.settings.refresh(page);
        self.stack.set_visible_child_name(page.title());
    }

    pub fn set_apps(&self, apps: &[AppEntry]) {
        self.programs.set_apps(apps);
        self.settings.set_apps(apps);
    }

    fn tick(&self) {
        let Ok(now) = glib::DateTime::now_local() else { return };
        if let Ok(s) = now.format("%A, %e %B %Y") {
            let s = s.replace("  ", " ");
            if self.date.text() != s {
                self.date.set_text(&s);
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
        self.running_item.set_visible(!entries.is_empty());
        self.running_count.set(entries.len());
        if let Ok(now) = glib::DateTime::now_local() {
            self.update_summary(&now);
        }
    }

    /// Favorites: a single grid of program icons.
    pub fn set_shortcuts(&self, entries: &[&AppEntry]) {
        super::programs::clear(&self.shortcuts);
        let d = &self.cfg.design;
        let buttons: Vec<gtk::Button> = entries
            .iter()
            .map(|entry| {
                let icon = super::icon_image(apps::gicon(entry).as_ref(), "application-x-executable", d.today_icon_px);
                let button = super::grid_button(icon, &entry.name);
                button.update_property(&[gtk::accessible::Property::Label(&entry.name)]);
                let id = entry.id.clone();
                let dsp = self.dispatch.clone();
                button.connect_clicked(move |_| dsp(Command::Shell(Event::Launch(id.clone()))));

                let (id, dsp) = (entry.id.clone(), self.dispatch.clone());
                super::context_menu(&button, &self.win.feedback, &self.cfg, move || {
                    let (a, b) = (dsp.clone(), dsp.clone());
                    let (ia, ib) = (id.clone(), id.clone());
                    vec![
                        MenuItem::new("Open", move || a(Command::Shell(Event::Launch(ia.clone())))),
                        MenuItem::new("Remove from favorites", move || b(Command::RemoveShortcut(ib.clone()))),
                    ]
                });
                button
            })
            .collect();
        if buttons.is_empty() {
            self.shortcuts.append(&line("Hold the stylus on a program in Programs to add it here."));
        } else {
            self.shortcuts.append(&super::icon_grid(buttons, self.layout.today_columns, d.spacing_px));
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
            let b = gtk::Box::new(gtk::Orientation::Horizontal, self.cfg.design.padding_px as i32);
            let name = doc.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let (content_type, _) = gio::content_type_guess(Some(&doc.path), None::<&[u8]>);
            b.append(&super::named_icon(document_icon(&content_type), px));
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
            row.set_child(Some(&line("No documents yet")));
            self.recent.append(&row);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::document_icon;

    #[test]
    fn documents_get_a_matching_icon() {
        assert_eq!(document_icon("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"), "pda-doc-spreadsheet");
        assert_eq!(document_icon("application/pdf"), "pda-doc-pdf");
        assert_eq!(document_icon("text/html"), "pda-doc-web");
        assert_eq!(document_icon("text/markdown"), "pda-doc-text");
        assert_eq!(document_icon("image/png"), "pda-doc");
    }
}

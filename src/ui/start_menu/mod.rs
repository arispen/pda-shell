//! Start menu: searchable list of installed programs plus system actions.
//! Drops down from the top-left corner like the Pocket PC Start menu.

use super::{MenuItem, ShellWindow};
use crate::apps::{self, AppEntry};
use crate::config::Config;
use crate::platform::x11::X11;
use crate::platform::SurfaceRole;
use crate::shell::layout::Layout;
use crate::shell::state::Event;
use crate::shell::{Command, Dispatch};
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

pub struct StartMenu {
    pub win: ShellWindow,
    cfg: Config,
    dispatch: Dispatch,
    search: gtk::SearchEntry,
    list: gtk::ListBox,
    apps: RefCell<Vec<AppEntry>>,
    shortcuts: Rc<RefCell<Vec<String>>>,
}

impl StartMenu {
    pub fn new(
        app: &gtk::Application,
        layout: &Layout,
        x11: &Rc<X11>,
        cfg: &Config,
        dispatch: Dispatch,
        shortcuts: Rc<RefCell<Vec<String>>>,
    ) -> Rc<Self> {
        let win = ShellWindow::new(app, "Start", "start-menu", SurfaceRole::Overlay, layout.start_menu, x11, cfg);
        win.window.set_hide_on_close(true);

        let search = gtk::SearchEntry::new();
        search.set_placeholder_text(Some("Search programs"));

        let list = gtk::ListBox::new();
        list.add_css_class("compact");
        list.set_activate_on_single_click(true);
        let d = dispatch.clone();
        list.connect_row_activated(move |_, row| d(Command::Shell(Event::Launch(row.widget_name().to_string()))));
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&list)
            .build();

        let actions = gtk::Box::new(gtk::Orientation::Horizontal, cfg.design.spacing_px as i32);
        actions.add_css_class("system-actions");
        actions.set_homogeneous(true);
        let px = cfg.design.icon_px;
        for (icon, label, cmd) in [
            ("go-home-symbolic", "Today", Command::Shell(Event::Today)),
            ("input-keyboard-symbolic", "Keyboard", Command::Shell(Event::ToggleKeyboard)),
            ("system-log-out-symbolic", "End session", Command::Shell(Event::EndSession)),
        ] {
            let b = gtk::Button::new();
            let content = gtk::Box::new(gtk::Orientation::Horizontal, 3);
            content.set_halign(gtk::Align::Center);
            content.append(&super::icon_image(None, icon, px));
            content.append(&gtk::Label::new(Some(label)));
            b.set_child(Some(&content));
            let d = dispatch.clone();
            b.connect_clicked(move |_| d(cmd.clone()));
            actions.append(&b);
        }

        let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        page.append(&search);
        let title = gtk::Label::new(Some("Programs"));
        title.add_css_class("section-title");
        title.set_xalign(0.0);
        page.append(&title);
        page.append(&scroller);
        page.append(&actions);
        win.set_content(&page);

        let menu = Rc::new(Self {
            win,
            cfg: cfg.clone(),
            dispatch: dispatch.clone(),
            search,
            list,
            apps: RefCell::new(Vec::new()),
            shortcuts,
        });

        let m = Rc::downgrade(&menu);
        menu.search.connect_search_changed(move |_| {
            if let Some(m) = m.upgrade() {
                m.rebuild();
            }
        });
        let m = Rc::downgrade(&menu);
        menu.search.connect_activate(move |_| {
            if let Some(m) = m.upgrade() {
                // Enter can arrive before the entry's search-changed delay
                // has elapsed; filter now so the visible first match is used.
                m.rebuild();
                if let Some(row) = m.list.row_at_index(0).filter(|r| r.is_activatable()) {
                    m.list.emit_by_name::<()>("row-activated", &[&row]);
                }
            }
        });
        let d = dispatch.clone();
        menu.search.connect_stop_search(move |_| d(Command::Shell(Event::DismissStartMenu)));

        let d = dispatch;
        menu.win.window.connect_is_active_notify(move |w| {
            if !w.is_active() && w.is_visible() {
                d(Command::StartMenuFocusLost);
            }
        });
        menu
    }

    pub fn set_apps(&self, apps: Vec<AppEntry>) {
        *self.apps.borrow_mut() = apps;
        self.rebuild();
    }

    fn rebuild(&self) {
        self.list.remove_all();
        let apps = self.apps.borrow();
        let query = self.search.text();
        let px = self.cfg.design.icon_px;
        for entry in apps::search(&apps, &query) {
            let row = gtk::ListBoxRow::new();
            row.set_widget_name(&entry.id);
            let b = gtk::Box::new(gtk::Orientation::Horizontal, 4);
            b.append(&super::icon_image(apps::gicon(entry).as_ref(), "application-x-executable", px));
            let name = gtk::Label::new(Some(&entry.name));
            name.set_xalign(0.0);
            name.set_ellipsize(gtk::pango::EllipsizeMode::End);
            b.append(&name);
            if !entry.generic_name.is_empty() && entry.generic_name != entry.name {
                let g = gtk::Label::new(Some(&entry.generic_name));
                g.add_css_class("dim");
                g.set_hexpand(true);
                g.set_xalign(1.0);
                g.set_ellipsize(gtk::pango::EllipsizeMode::End);
                b.append(&g);
            }
            row.set_child(Some(&b));

            let (id, dsp, shortcuts) = (entry.id.clone(), self.dispatch.clone(), self.shortcuts.clone());
            super::context_menu(&row, &self.win.feedback, &self.cfg, move || {
                let (a, b) = (dsp.clone(), dsp.clone());
                let (ia, ib) = (id.clone(), id.clone());
                let pinned = shortcuts.borrow().contains(&id);
                vec![
                    MenuItem::new("Open", move || a(Command::Shell(Event::Launch(ia.clone())))),
                    if pinned {
                        MenuItem::new("Remove from Today", move || b(Command::RemoveShortcut(ib.clone())))
                    } else {
                        MenuItem::new("Add to Today", move || b(Command::AddShortcut(ib.clone())))
                    },
                ]
            });
            self.list.append(&row);
        }
        if self.list.row_at_index(0).is_none() {
            let row = gtk::ListBoxRow::new();
            row.set_activatable(false);
            let l = gtk::Label::new(Some("No matching programs"));
            l.add_css_class("dim");
            row.set_child(Some(&l));
            self.list.append(&row);
        }
    }

    pub fn show(&self) {
        self.search.set_text("");
        self.win.window.present();
        self.search.grab_focus();
    }

    pub fn hide(&self) {
        self.win.window.set_visible(false);
    }
}

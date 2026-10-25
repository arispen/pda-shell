//! Start menu: a compact drop-down under the Start button, laid out like the
//! Pocket PC 2003 Start menu: a strip of recently started programs, Today,
//! the favorites, then Programs, Settings and Find. End session sits at the
//! bottom in a quieter style.
//!
//! Search is still there but not the first impression: "Find" (or simply
//! typing while the menu is open) turns the menu into a search over every
//! installed program, technical utilities included.

use super::{MenuItem, ShellWindow};
use crate::apps::{self, AppEntry};
use crate::config::Config;
use crate::platform::x11::X11;
use crate::platform::SurfaceRole;
use crate::shell::layout::{Layout, Rect};
use crate::shell::state::{Event, Page};
use crate::shell::{Command, Dispatch};
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const ROW_TODAY: &str = "__today__";
const ROW_PROGRAMS: &str = "__programs__";
const ROW_SETTINGS: &str = "__settings__";
const ROW_FIND: &str = "__find__";
const ROW_END: &str = "__end_session__";

pub struct StartMenu {
    pub win: ShellWindow,
    cfg: Config,
    dispatch: Dispatch,
    recent: gtk::Box,
    search: gtk::SearchEntry,
    scroller: gtk::ScrolledWindow,
    list: gtk::ListBox,
    apps: RefCell<Vec<AppEntry>>,
    shortcuts: Rc<RefCell<Vec<String>>>,
    recent_apps: Rc<RefCell<Vec<String>>>,
    searching: Cell<bool>,
    /// Favorites are left out of the list because the menu would not fit
    /// (landscape, keyboard shown); the recent strip still has them.
    without_favorites: Cell<bool>,
    /// The space the menu may use (shrinks while the keyboard is shown).
    available: Cell<Rect>,
}

impl StartMenu {
    pub fn new(
        app: &gtk::Application,
        layout: &Layout,
        x11: &Rc<X11>,
        cfg: &Config,
        dispatch: Dispatch,
        shortcuts: Rc<RefCell<Vec<String>>>,
        recent_apps: Rc<RefCell<Vec<String>>>,
    ) -> Rc<Self> {
        let win = ShellWindow::new(app, "Start", "start-menu", SurfaceRole::Overlay, layout.start_menu, x11, cfg);
        win.window.set_hide_on_close(true);

        let recent = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        recent.add_css_class("start-recent");

        let search = gtk::SearchEntry::new();
        search.set_placeholder_text(Some("Find a program"));
        search.set_visible(false);

        let list = gtk::ListBox::new();
        list.add_css_class("compact");
        list.add_css_class("targets");
        list.set_selection_mode(gtk::SelectionMode::None);
        list.set_activate_on_single_click(true);
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .child(&list)
            .build();

        let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        page.append(&recent);
        page.append(&search);
        page.append(&scroller);
        win.set_content(&page);

        let menu = Rc::new(Self {
            win,
            cfg: cfg.clone(),
            dispatch: dispatch.clone(),
            recent,
            search,
            scroller,
            list,
            apps: RefCell::new(Vec::new()),
            shortcuts,
            recent_apps,
            searching: Cell::new(false),
            without_favorites: Cell::new(false),
            available: Cell::new(layout.start_menu),
        });

        let m = Rc::downgrade(&menu);
        let d = dispatch.clone();
        menu.list.connect_row_activated(move |_, row| {
            let name = row.widget_name();
            let cmd = match name.as_str() {
                ROW_TODAY => Command::Shell(Event::Today),
                ROW_PROGRAMS => Command::Shell(Event::ShowPage(Page::Programs)),
                ROW_SETTINGS => Command::Shell(Event::ShowPage(Page::Settings)),
                ROW_END => Command::Shell(Event::EndSession),
                ROW_FIND => {
                    if let Some(m) = m.upgrade() {
                        m.start_search("");
                    }
                    return;
                }
                id => Command::Shell(Event::Launch(id.to_string())),
            };
            d(cmd);
        });

        let m = Rc::downgrade(&menu);
        menu.search.connect_search_changed(move |_| {
            if let Some(m) = m.upgrade() {
                if m.searching.get() {
                    m.rebuild();
                }
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

        // Typing while the menu is open starts a search with that text.
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let m = Rc::downgrade(&menu);
        keys.connect_key_pressed(move |_, key, _, state| {
            let Some(m) = m.upgrade() else { return glib::Propagation::Proceed };
            let plain = !state.intersects(gtk::gdk::ModifierType::CONTROL_MASK | gtk::gdk::ModifierType::ALT_MASK);
            match key.to_unicode().filter(|c| plain && !c.is_control()) {
                Some(c) if !m.searching.get() => {
                    m.start_search(&c.to_string());
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        menu.win.window.add_controller(keys);

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
        self.refresh();
    }

    /// Rebuild after favorites, recent programs or installs changed.
    pub fn refresh(&self) {
        self.rebuild_recent();
        self.rebuild();
    }

    fn rebuild_recent(&self) {
        while let Some(c) = self.recent.first_child() {
            self.recent.remove(&c);
        }
        let apps = self.apps.borrow();
        // Recently started programs; the favorites until there are any.
        let mut ids = self.recent_apps.borrow().clone();
        for id in self.shortcuts.borrow().iter() {
            if ids.len() < crate::shell::shortcuts::RECENT_APPS && !ids.contains(id) {
                ids.push(id.clone());
            }
        }
        for entry in apps::resolve_shortcuts(&apps, &ids) {
            let b = gtk::Button::new();
            b.set_child(Some(&super::icon_image(apps::gicon(entry).as_ref(), "application-x-executable", self.cfg.design.icon_px)));
            b.update_property(&[gtk::accessible::Property::Label(&entry.name)]);
            let (d, id) = (self.dispatch.clone(), entry.id.clone());
            b.connect_clicked(move |_| d(Command::Shell(Event::Launch(id.clone()))));
            self.recent.append(&b);
        }
        self.recent.set_visible(self.recent.first_child().is_some());
    }

    fn rebuild(&self) {
        self.without_favorites.set(false);
        self.fill();
        // Every command must be visible without scrolling; drop the
        // favorites rather than hide End session below the edge.
        if !self.searching.get() && self.natural_height() > self.available.get().height {
            self.without_favorites.set(true);
            self.fill();
        }
        self.fit();
    }

    fn fill(&self) {
        self.list.remove_all();
        if self.searching.get() {
            self.rebuild_search();
        } else {
            self.rebuild_menu();
        }
    }

    fn rebuild_menu(&self) {
        let apps = self.apps.borrow();
        self.append_item(ROW_TODAY, "pda-today", "Today");
        if !self.without_favorites.get() {
            for entry in apps::resolve_shortcuts(&apps, &self.shortcuts.borrow()) {
                self.append_entry(entry);
            }
        }
        self.append_item(ROW_PROGRAMS, "pda-programs", "Programs").set_header(Some(&separator()));
        self.append_item(ROW_SETTINGS, "pda-settings", "Settings");
        self.append_item(ROW_FIND, "pda-find", "Find…");
        let end = self.append_item(ROW_END, "pda-power", "End session");
        end.add_css_class("minor");
        end.set_header(Some(&separator()));
    }

    fn rebuild_search(&self) {
        let apps = self.apps.borrow();
        for entry in apps::search(&apps, &self.search.text()) {
            self.append_entry(entry);
        }
        if self.list.row_at_index(0).is_none() {
            let row = gtk::ListBoxRow::new();
            row.set_activatable(false);
            let l = gtk::Label::new(Some("No matching programs"));
            l.add_css_class("dim");
            l.set_xalign(0.0);
            row.set_child(Some(&l));
            self.list.append(&row);
        }
    }

    fn append_item(&self, name: &str, icon: &str, label: &str) -> gtk::ListBoxRow {
        let row = gtk::ListBoxRow::new();
        row.set_widget_name(name);
        row.update_property(&[gtk::accessible::Property::Label(label.trim_end_matches('…'))]);
        let b = gtk::Box::new(gtk::Orientation::Horizontal, self.cfg.design.padding_px as i32);
        b.append(&super::named_icon(icon, self.cfg.design.icon_px));
        let l = gtk::Label::new(Some(label));
        l.set_xalign(0.0);
        b.append(&l);
        row.set_child(Some(&b));
        self.list.append(&row);
        row
    }

    fn append_entry(&self, entry: &AppEntry) {
        let px = self.cfg.design.icon_px;
        let row = gtk::ListBoxRow::new();
        row.set_widget_name(&entry.id);
        row.update_property(&[gtk::accessible::Property::Label(&entry.name)]);
        let b = gtk::Box::new(gtk::Orientation::Horizontal, self.cfg.design.padding_px as i32);
        b.append(&super::icon_image(apps::gicon(entry).as_ref(), "application-x-executable", px));
        let name = gtk::Label::new(Some(&entry.name));
        name.set_xalign(0.0);
        name.set_hexpand(true);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        b.append(&name);
        row.set_child(Some(&b));

        let (id, dsp, shortcuts) = (entry.id.clone(), self.dispatch.clone(), self.shortcuts.clone());
        super::context_menu(&row, &self.win.feedback, &self.cfg, move || {
            let (a, b) = (dsp.clone(), dsp.clone());
            let (ia, ib) = (id.clone(), id.clone());
            let pinned = shortcuts.borrow().contains(&id);
            vec![
                MenuItem::new("Open", move || a(Command::Shell(Event::Launch(ia.clone())))),
                if pinned {
                    MenuItem::new("Remove from favorites", move || b(Command::RemoveShortcut(ib.clone())))
                } else {
                    MenuItem::new("Add to favorites", move || b(Command::AddShortcut(ib.clone())))
                },
            ]
        });
        self.list.append(&row);
    }

    fn start_search(&self, text: &str) {
        self.searching.set(true);
        self.recent.set_visible(false);
        self.search.set_visible(true);
        self.search.set_text(text);
        self.search.grab_focus();
        self.search.set_position(-1);
        self.rebuild();
    }

    /// As tall as the content, at most the available space; a search uses
    /// all of it so results do not make the menu jump.
    fn fit(&self) {
        let area = self.available.get();
        self.scroller.set_max_content_height(area.height as i32);
        let height = if self.searching.get() { area.height } else { self.natural_height().min(area.height) };
        self.win.set_rect(Rect { height, ..area });
    }

    /// Height the menu content wants, with the drop-down's border.
    fn natural_height(&self) -> u32 {
        let page = self.scroller.parent().expect("menu page");
        let (_, natural, _, _) = page.measure(gtk::Orientation::Vertical, self.available.get().width as i32);
        natural.max(0) as u32 + 2
    }

    /// The space the menu may use, e.g. less while the keyboard is shown.
    pub fn set_available(&self, area: Rect) {
        self.available.set(area);
        self.fit();
    }

    pub fn show(&self) {
        self.searching.set(false);
        self.search.set_visible(false);
        self.search.set_text("");
        self.refresh();
        self.win.window.present();
        self.list.grab_focus();
    }

    pub fn hide(&self) {
        self.win.window.set_visible(false);
    }
}

fn separator() -> gtk::Separator {
    gtk::Separator::new(gtk::Orientation::Horizontal)
}

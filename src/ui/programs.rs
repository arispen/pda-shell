//! Programs: the Pocket PC "Programs" folder. Ordinary programs as a grid of
//! large icons; technical utilities and settings dialogs of the Linux system
//! sit one level down in "Linux tools", the advanced compatibility path.

use super::MenuItem;
use crate::apps::{self, AppEntry};
use crate::config::Config;
use crate::input::StylusFeedback;
use crate::shell::layout::Layout;
use crate::shell::state::{Event, Page};
use crate::shell::{Command, Dispatch};
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

pub struct Programs {
    /// The Programs screen.
    pub programs: gtk::ScrolledWindow,
    /// The Linux tools screen.
    pub tools: gtk::ScrolledWindow,
    programs_box: gtk::Box,
    tools_box: gtk::Box,
    cfg: Config,
    columns: u32,
    dispatch: Dispatch,
    feedback: StylusFeedback,
    shortcuts: Rc<RefCell<Vec<String>>>,
}

fn screen(content: &gtk::Box) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(content)
        .build()
}

/// A dim explanatory line under a grid.
pub fn note(text: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.add_css_class("dim");
    l.add_css_class("page-note");
    l.set_wrap(true);
    l.set_xalign(0.0);
    l
}

impl Programs {
    pub fn new(
        layout: &Layout,
        cfg: &Config,
        dispatch: Dispatch,
        feedback: StylusFeedback,
        shortcuts: Rc<RefCell<Vec<String>>>,
    ) -> Self {
        let pad = cfg.design.padding_px as i32;
        let boxed = || {
            let b = gtk::Box::new(gtk::Orientation::Vertical, pad);
            b.set_margin_top(pad);
            b.set_margin_start(pad);
            b.set_margin_end(pad);
            b
        };
        let (programs_box, tools_box) = (boxed(), boxed());
        Self {
            programs: screen(&programs_box),
            tools: screen(&tools_box),
            programs_box,
            tools_box,
            cfg: cfg.clone(),
            columns: layout.grid_columns,
            dispatch,
            feedback,
            shortcuts,
        }
    }

    pub fn set_apps(&self, all: &[AppEntry]) {
        let hidden = &self.cfg.start_menu.hidden;
        let (technical, programs): (Vec<&AppEntry>, Vec<&AppEntry>) =
            all.iter().partition(|a| apps::is_technical(a, hidden));

        clear(&self.programs_box);
        let mut buttons: Vec<gtk::Button> = programs.iter().map(|e| self.app_button(e)).collect();
        let tools = super::grid_button(super::named_icon("pda-tools", self.cfg.design.today_icon_px), "Linux tools");
        let d = self.dispatch.clone();
        tools.connect_clicked(move |_| d(Command::Shell(Event::ShowPage(Page::LinuxTools))));
        buttons.push(tools);
        let spacing = self.cfg.design.spacing_px;
        self.programs_box.append(&super::icon_grid(buttons, self.columns, spacing));
        self.programs_box.append(&note("Hold the stylus on a program to add it to your favorites."));

        clear(&self.tools_box);
        self.tools_box.append(&note(
            "Utilities and settings dialogs of the Linux system. They are desktop programs and may need the keyboard or a larger screen.",
        ));
        let buttons = technical.iter().map(|e| self.app_button(e)).collect();
        self.tools_box.append(&super::icon_grid(buttons, self.columns, spacing));
    }

    fn app_button(&self, entry: &AppEntry) -> gtk::Button {
        let icon = super::icon_image(apps::gicon(entry).as_ref(), "application-x-executable", self.cfg.design.today_icon_px);
        let b = super::grid_button(icon, &entry.name);
        b.update_property(&[gtk::accessible::Property::Label(&entry.name)]);
        let (id, d) = (entry.id.clone(), self.dispatch.clone());
        b.connect_clicked(move |_| d(Command::Shell(Event::Launch(id.clone()))));
        let (id, dsp, shortcuts) = (entry.id.clone(), self.dispatch.clone(), self.shortcuts.clone());
        super::context_menu(&b, &self.feedback, &self.cfg, move || {
            let (a, b) = (dsp.clone(), dsp.clone());
            let (ia, ib) = (id.clone(), id.clone());
            vec![
                MenuItem::new("Open", move || a(Command::Shell(Event::Launch(ia.clone())))),
                if shortcuts.borrow().contains(&id) {
                    MenuItem::new("Remove from favorites", move || b(Command::RemoveShortcut(ib.clone())))
                } else {
                    MenuItem::new("Add to favorites", move || b(Command::AddShortcut(ib.clone())))
                },
            ]
        });
        b
    }
}

pub fn clear(b: &gtk::Box) {
    while let Some(c) = b.first_child() {
        b.remove(&c);
    }
}

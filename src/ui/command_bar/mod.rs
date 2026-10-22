//! Bottom command bar: Today, application switcher, shell context actions,
//! keyboard toggle.

use super::ShellWindow;
use crate::config::Config;
use crate::platform::x11::X11;
use crate::platform::{SurfaceRole, WindowId};
use crate::shell::layout::Layout;
use crate::shell::state::{ContextAction, Event};
use crate::shell::{Command, Dispatch};
use gtk::gio;
use gtk::prelude::*;
use std::rc::Rc;

/// One row of the application switcher.
pub struct SwitcherEntry {
    pub id: WindowId,
    pub title: String,
    pub icon: Option<gio::Icon>,
    pub active: bool,
}

pub struct CommandBar {
    pub win: ShellWindow,
    switcher: gtk::MenuButton,
    switcher_label: gtk::Label,
    switcher_list: gtk::ListBox,
    context: gtk::Box,
    keyboard: gtk::Button,
    dispatch: Dispatch,
    icon_px: u32,
}

fn bar_button(icon: &str, label: Option<&str>, px: u32) -> gtk::Button {
    let b = gtk::Button::new();
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 3);
    content.append(&super::icon_image(None, icon, px));
    if let Some(l) = label {
        content.append(&gtk::Label::new(Some(l)));
    }
    b.set_child(Some(&content));
    b
}

impl CommandBar {
    pub fn new(app: &gtk::Application, layout: &Layout, x11: &Rc<X11>, cfg: &Config, dispatch: Dispatch) -> Rc<Self> {
        let px = cfg.design.icon_px;
        let win =
            ShellWindow::new(app, "pda-shell command bar", "bar", SurfaceRole::BottomPanel, layout.bottom_bar, x11, cfg);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, cfg.design.spacing_px as i32);

        let today = bar_button("go-home-symbolic", Some("Today"), px);
        today.set_tooltip_text(Some("Show Today"));
        let d = dispatch.clone();
        today.connect_clicked(move |_| d(Command::Shell(Event::Today)));

        let switcher_label = gtk::Label::new(Some("Windows"));
        let switcher_content = gtk::Box::new(gtk::Orientation::Horizontal, 3);
        switcher_content.append(&super::icon_image(None, "view-list-symbolic", px));
        switcher_content.append(&switcher_label);
        let switcher = gtk::MenuButton::new();
        switcher.set_child(Some(&switcher_content));
        switcher.set_direction(gtk::ArrowType::Up);
        switcher.set_tooltip_text(Some("Running programs"));
        let switcher_list = gtk::ListBox::new();
        switcher_list.add_css_class("compact");
        switcher_list.set_activate_on_single_click(true);
        let popover = gtk::Popover::new();
        popover.add_css_class("pda");
        popover.set_has_arrow(false);
        popover.set_child(Some(&switcher_list));
        switcher.set_popover(Some(&popover));
        let (d, p) = (dispatch.clone(), popover.clone());
        switcher_list.connect_row_activated(move |_, row| {
            p.popdown();
            let name = row.widget_name();
            if let Some(id) = name.strip_prefix("win-").and_then(|s| s.parse().ok()) {
                d(Command::Shell(Event::Switch(WindowId(id))));
            } else if name == "close-all" {
                d(Command::Shell(Event::CloseAll));
            }
        });

        let context = gtk::Box::new(gtk::Orientation::Horizontal, cfg.design.spacing_px as i32);
        context.set_hexpand(true);
        context.set_halign(gtk::Align::End);

        let keyboard = bar_button("input-keyboard-symbolic", None, px);
        keyboard.set_tooltip_text(Some("Show or hide the keyboard"));
        let d = dispatch.clone();
        keyboard.connect_clicked(move |_| d(Command::Shell(Event::ToggleKeyboard)));

        row.append(&today);
        row.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        row.append(&switcher);
        row.append(&context);
        row.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        row.append(&keyboard);
        win.set_content(&row);

        Rc::new(Self { win, switcher, switcher_label, switcher_list, context, keyboard, dispatch, icon_px: px })
    }

    pub fn set_windows(&self, entries: Vec<SwitcherEntry>) {
        self.switcher_label.set_text(&match entries.len() {
            0 => "Windows".to_string(),
            n => format!("Windows ({n})"),
        });
        self.switcher.set_sensitive(!entries.is_empty());
        self.switcher_list.remove_all();
        let count = entries.len();
        for e in entries {
            let row = gtk::ListBoxRow::new();
            row.set_widget_name(&format!("win-{}", e.id.0));
            let b = gtk::Box::new(gtk::Orientation::Horizontal, 4);
            b.append(&super::icon_image(e.icon.as_ref(), "application-x-executable-symbolic", self.icon_px));
            let l = gtk::Label::new(Some(&e.title));
            l.set_xalign(0.0);
            l.set_ellipsize(gtk::pango::EllipsizeMode::End);
            l.set_max_width_chars(40);
            if e.active {
                l.add_css_class("heading");
            }
            b.append(&l);
            row.set_child(Some(&b));
            self.switcher_list.append(&row);
        }
        if count > 1 {
            let row = gtk::ListBoxRow::new();
            row.set_widget_name("close-all");
            let l = gtk::Label::new(Some("Close all programs"));
            l.set_xalign(0.0);
            row.set_child(Some(&l));
            self.switcher_list.append(&row);
        }
    }

    pub fn set_context_actions(&self, actions: &[ContextAction]) {
        while let Some(c) = self.context.first_child() {
            self.context.remove(&c);
        }
        for a in actions {
            let (icon, label, cmd) = match a {
                ContextAction::Refresh => ("view-refresh-symbolic", "Refresh", Command::Refresh),
                ContextAction::CloseApp => ("window-close-symbolic", "Close", Command::Shell(Event::CloseActive)),
                ContextAction::CancelMenu => ("go-previous-symbolic", "Cancel", Command::Shell(Event::DismissStartMenu)),
            };
            let b = bar_button(icon, Some(label), self.icon_px);
            let d = self.dispatch.clone();
            b.connect_clicked(move |_| d(cmd.clone()));
            self.context.append(&b);
        }
    }

    pub fn set_keyboard_visible(&self, visible: bool) {
        if visible {
            self.keyboard.add_css_class("open");
        } else {
            self.keyboard.remove_css_class("open");
        }
    }
}

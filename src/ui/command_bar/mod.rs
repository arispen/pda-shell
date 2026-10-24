//! Bottom command bar, a Pocket PC 2003 style menu bar: text commands on the
//! left (Today, the running-program switcher, then commands for the current
//! screen: Close, OK, Cancel) and the input panel (keyboard) button at the
//! right edge.

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
#[derive(Clone)]
pub struct SwitcherEntry {
    pub id: WindowId,
    pub title: String,
    /// Program name, shown under the title (titles are often file names).
    pub program: String,
    pub icon: Option<gio::Icon>,
    pub active: bool,
}

/// Label of the "Close all" row: a second tap is needed to confirm.
pub fn close_all_label(count: usize, armed: bool) -> String {
    if armed {
        format!("Tap again to close all {count} programs")
    } else {
        "Close all programs…".to_string()
    }
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
    target_px: u32,
}

/// A menu-bar command: text only, like the menus of a Pocket PC program.
fn bar_button(label: &str) -> gtk::Button {
    let b = gtk::Button::with_label(label);
    b.update_property(&[gtk::accessible::Property::Label(label)]);
    b
}

/// The small "opens upwards" arrow next to menus and the input panel.
fn arrow() -> gtk::Label {
    let a = gtk::Label::new(Some("▴"));
    a.add_css_class("arrow");
    a
}

impl CommandBar {
    pub fn new(app: &gtk::Application, layout: &Layout, x11: &Rc<X11>, cfg: &Config, dispatch: Dispatch) -> Rc<Self> {
        let px = cfg.design.icon_px;
        let win =
            ShellWindow::new(app, "pda-shell command bar", "bar", SurfaceRole::BottomPanel, layout.bottom_bar, x11, cfg);
        win.window.add_css_class("menu-bar");
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);

        let today = bar_button("Today");
        today.set_tooltip_text(Some("Show Today"));
        let d = dispatch.clone();
        today.connect_clicked(move |_| d(Command::Shell(Event::Today)));

        let switcher_label = gtk::Label::new(Some("Windows"));
        let switcher_content = gtk::Box::new(gtk::Orientation::Horizontal, 3);
        switcher_content.append(&switcher_label);
        switcher_content.append(&arrow());
        let switcher = gtk::MenuButton::new();
        switcher.set_child(Some(&switcher_content));
        switcher.set_direction(gtk::ArrowType::Up);
        switcher.set_tooltip_text(Some("Running programs"));
        let switcher_list = gtk::ListBox::new();
        switcher_list.add_css_class("compact");
        switcher_list.add_css_class("targets");
        switcher_list.set_activate_on_single_click(true);
        // The program in front is marked "current"; a selection highlight
        // on top of that would only be a second, competing marker.
        switcher_list.set_selection_mode(gtk::SelectionMode::None);
        // Wide enough for "file name - Program" titles, never wider than
        // the screen.
        switcher_list.set_size_request((layout.screen.width.saturating_sub(16)).min(440) as i32, -1);
        let popover = gtk::Popover::new();
        popover.add_css_class("pda");
        popover.set_has_arrow(false);
        popover.set_child(Some(&switcher_list));
        switcher.set_popover(Some(&popover));
        let (d, p) = (dispatch.clone(), popover.clone());
        switcher_list.connect_row_activated(move |list, row| {
            let name = row.widget_name();
            if let Some(id) = name.strip_prefix("win-").and_then(|s| s.parse().ok()) {
                p.popdown();
                d(Command::Shell(Event::Switch(WindowId(id))));
            } else if name == "close-all" {
                // Closing every program is not a casual tap next to the
                // program list: the first tap arms, the second confirms.
                if row.has_css_class("armed") {
                    p.popdown();
                    d(Command::Shell(Event::CloseAll));
                } else {
                    row.add_css_class("armed");
                    let count = (0..)
                        .map_while(|i| list.row_at_index(i))
                        .filter(|r| r.widget_name().starts_with("win-"))
                        .count();
                    if let Some(l) = row.child().and_then(|b| b.last_child()).and_downcast::<gtk::Label>() {
                        l.set_text(&close_all_label(count, true));
                    }
                }
            }
        });
        let l = switcher_list.clone();
        popover.connect_closed(move |_| {
            if let Some(row) = l.last_child().and_downcast::<gtk::ListBoxRow>().filter(|r| r.has_css_class("armed")) {
                row.remove_css_class("armed");
                if let Some(label) = row.child().and_then(|b| b.last_child()).and_downcast::<gtk::Label>() {
                    label.set_text(&close_all_label(0, false));
                }
            }
        });

        // Commands for the current screen follow the switcher, on the left.
        let context = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        context.set_hexpand(true);
        context.set_halign(gtk::Align::Start);

        // The input panel button: the keyboard icon with an arrow, at the
        // right edge like the Pocket PC SIP button.
        let keyboard = gtk::Button::new();
        keyboard.add_css_class("sip");
        let sip = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        sip.append(&super::named_icon("pda-keyboard", (cfg.design.bottom_bar_px * 2 / 3).max(px)));
        sip.append(&arrow());
        keyboard.set_child(Some(&sip));
        keyboard.set_tooltip_text(Some("Show or hide the keyboard"));
        keyboard.update_property(&[gtk::accessible::Property::Label("Input panel (keyboard)")]);
        let d = dispatch.clone();
        keyboard.connect_clicked(move |_| d(Command::Shell(Event::ToggleKeyboard)));

        row.append(&today);
        row.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        row.append(&switcher);
        row.append(&context);
        row.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        row.append(&keyboard);
        win.set_content(&row);

        Rc::new(Self {
            win,
            switcher,
            switcher_label,
            switcher_list,
            context,
            keyboard,
            dispatch,
            icon_px: px,
            target_px: cfg.design.target_px,
        })
    }

    pub fn set_windows(&self, entries: Vec<SwitcherEntry>) {
        let label = match entries.len() {
            0 => "Windows".to_string(),
            n => format!("Windows ({n})"),
        };
        self.switcher.update_property(&[gtk::accessible::Property::Label(&label)]);
        self.switcher_label.set_text(&label);
        self.switcher.set_sensitive(!entries.is_empty());
        self.switcher_list.remove_all();
        let count = entries.len();
        for e in entries {
            self.switcher_list.append(&switcher_row(&e, self.icon_px));
        }
        if count > 1 {
            let row = gtk::ListBoxRow::new();
            row.set_header(Some(&gtk::Separator::new(gtk::Orientation::Horizontal)));
            row.set_widget_name("close-all");
            row.add_css_class("destructive");
            row.update_property(&[gtk::accessible::Property::Label("Close all programs")]);
            let b = gtk::Box::new(gtk::Orientation::Horizontal, 4);
            b.append(&super::icon_image(None, "window-close-symbolic", self.icon_px));
            let l = gtk::Label::new(Some(&close_all_label(count, false)));
            l.set_xalign(0.0);
            b.append(&l);
            row.set_child(Some(&b));
            row.set_size_request(-1, self.target_px as i32);
            self.switcher_list.append(&row);
        }
    }

    pub fn set_context_actions(&self, actions: &[ContextAction]) {
        while let Some(c) = self.context.first_child() {
            self.context.remove(&c);
        }
        for a in actions {
            let (label, cmd) = match a {
                ContextAction::CloseApp => ("Close", Command::Shell(Event::CloseActive)),
                ContextAction::CancelMenu => ("Cancel", Command::Shell(Event::DismissStartMenu)),
                ContextAction::Ok => ("OK", Command::Shell(Event::Back)),
            };
            self.context.append(&gtk::Separator::new(gtk::Orientation::Vertical));
            let b = bar_button(label);
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

/// A switcher row: icon, window title, program name, and a marker on the
/// program that is in front.
pub fn switcher_row(e: &SwitcherEntry, icon_px: u32) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_widget_name(&format!("win-{}", e.id.0));
    row.update_property(&[gtk::accessible::Property::Label(&e.title)]);
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    b.append(&super::icon_image(e.icon.as_ref(), "application-x-executable-symbolic", icon_px));
    let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);
    let l = gtk::Label::new(Some(&e.title));
    l.set_xalign(0.0);
    l.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    if e.active {
        l.add_css_class("heading");
    }
    text.append(&l);
    if !e.program.is_empty() && !e.title.contains(&e.program) {
        let p = gtk::Label::new(Some(&e.program));
        p.add_css_class("dim");
        p.set_xalign(0.0);
        text.append(&p);
    }
    b.append(&text);
    if e.active {
        let m = gtk::Label::new(Some("current"));
        m.add_css_class("active-marker");
        b.append(&m);
    }
    row.set_child(Some(&b));
    row
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_all_needs_confirmation_text() {
        assert_eq!(close_all_label(3, false), "Close all programs…");
        assert_eq!(close_all_label(3, true), "Tap again to close all 3 programs");
    }
}

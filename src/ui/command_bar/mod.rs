//! Bottom command bar, a Pocket PC 2003 style menu bar. Left: Today and the
//! running-program switcher (icons), then the commands of what is in front:
//! the program's own menus when it publishes them (GtkApplication menu bar
//! over D-Bus: the shell's native programs, Mousepad, ...), otherwise
//! generic File / Edit commands typed as keyboard shortcuts; New on Today;
//! OK on shell screens; Cancel while the Start menu is open. Right: Close
//! for programs, and the input panel button with its chooser.

pub mod menus;

use super::ShellWindow;
use crate::config::Config;
use crate::platform::x11::X11;
use crate::platform::{AppMenus, Key, KeyChord, SurfaceRole, WindowId};
use crate::shell::layout::Layout;
use crate::shell::state::{ContextAction, Event};
use crate::shell::{Command, Dispatch};
use gtk::gio;
use gtk::prelude::*;
use std::cell::RefCell;
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
    /// The program's menus (or the generic ones); kept while it is in front.
    app_menus: gtk::Box,
    /// Which window's menus `app_menus` shows, and the D-Bus model behind it.
    shown_menus: RefCell<Option<(WindowId, Option<AppMenus>, Option<menus::Remote>)>>,
    /// The actions the bar shows now; rebuilt only when they change, so an
    /// open menu survives title updates.
    shown_actions: RefCell<Option<Vec<ContextAction>>>,
    new_menu: gio::Menu,
    new_actions: gio::SimpleActionGroup,
    keys: gio::SimpleActionGroup,
    close: gtk::Button,
    keyboard: gtk::Button,
    panels: gio::SimpleAction,
    dispatch: Dispatch,
    icon_px: u32,
    target_px: u32,
    /// Menu buttons that fit between the fixed buttons.
    max_menus: usize,
}

/// Generic commands for programs that publish no menus: common keyboard
/// shortcuts, typed into the program in front.
pub fn generic_menus() -> Vec<(&'static str, Vec<(&'static str, KeyChord)>)> {
    vec![
        (
            "File",
            vec![
                ("Save", KeyChord::ctrl('s')),
                ("Open…", KeyChord::ctrl('o')),
                ("Find…", KeyChord::ctrl('f')),
                ("Program menu", KeyChord::plain(Key::F(10))),
            ],
        ),
        (
            "Edit",
            vec![
                ("Undo", KeyChord::ctrl('z')),
                ("Cut", KeyChord::ctrl('x')),
                ("Copy", KeyChord::ctrl('c')),
                ("Paste", KeyChord::ctrl('v')),
                ("Select all", KeyChord::ctrl('a')),
            ],
        ),
    ]
}

fn remote_groups(r: &menus::Remote) -> menus::Groups {
    let mut g = Vec::new();
    if let Some(a) = &r.app {
        g.push(("app".to_string(), a.clone()));
    }
    if let Some(w) = &r.window {
        g.push(("win".to_string(), w.clone()));
    }
    menus::Groups(g)
}

/// How many menu buttons fit beside the fixed ones (Today, Windows, Close,
/// input panel and its chooser).
pub fn max_menus(width: u32, target_px: u32, font_px: u32, padding_px: u32) -> usize {
    let fixed = 5 * target_px + 2 * font_px;
    let per_menu = font_px * 4 + 2 * padding_px;
    (width.saturating_sub(fixed) / per_menu.max(1)).max(1) as usize
}

/// A menu-bar command: text only, like the menus of a Pocket PC program.
fn bar_button(label: &str) -> gtk::Button {
    let b = gtk::Button::with_label(label);
    b.update_property(&[gtk::accessible::Property::Label(label)]);
    b
}

/// A bar button that is an icon only; `label` is its accessible name.
fn icon_button(icon: &str, label: &str, px: u32) -> gtk::Button {
    let b = gtk::Button::new();
    b.set_child(Some(&super::named_icon(icon, px)));
    b.set_tooltip_text(Some(label));
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
        menus::set_max_height(layout.workarea.height);
        let win =
            ShellWindow::new(app, "pda-shell command bar", "bar", SurfaceRole::BottomPanel, layout.bottom_bar, x11, cfg);
        win.window.add_css_class("menu-bar");
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);

        let today = icon_button("pda-today", "Today", px);
        let d = dispatch.clone();
        today.connect_clicked(move |_| d(Command::Shell(Event::Today)));

        let switcher_label = gtk::Label::new(None);
        let switcher_content = gtk::Box::new(gtk::Orientation::Horizontal, 3);
        switcher_content.append(&super::named_icon("pda-windows", px));
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

        // Commands for what is in front follow the switcher, on the left.
        let context = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        context.set_hexpand(true);
        context.set_halign(gtk::Align::Start);
        let app_menus = gtk::Box::new(gtk::Orientation::Horizontal, 0);

        // Generic commands: actions "keys.N" type a shortcut.
        let keys = gio::SimpleActionGroup::new();
        for (i, chord) in generic_menus().into_iter().flat_map(|(_, items)| items).map(|(_, c)| c).enumerate() {
            let a = gio::SimpleAction::new(&format!("k{i}"), None);
            let d = dispatch.clone();
            a.connect_activate(move |_, _| d(Command::SendKey(chord)));
            keys.add_action(&a);
        }
        app_menus.insert_action_group("keys", Some(&keys));
        let new_actions = gio::SimpleActionGroup::new();
        context.insert_action_group("new", Some(&new_actions));

        let close = icon_button("pda-close", "Close", px);
        close.set_visible(false);
        let d = dispatch.clone();
        close.connect_clicked(move |_| d(Command::Shell(Event::CloseActive)));

        // The input panel button: the keyboard icon at the right edge like
        // the Pocket PC SIP button, and the arrow beside it to choose the
        // input panel.
        let keyboard = gtk::Button::new();
        keyboard.add_css_class("sip");
        keyboard.set_child(Some(&super::named_icon("pda-keyboard", (cfg.design.bottom_bar_px * 2 / 3).max(px))));
        keyboard.set_tooltip_text(Some("Show or hide the keyboard"));
        keyboard.update_property(&[gtk::accessible::Property::Label("Input panel (keyboard)")]);
        let d = dispatch.clone();
        keyboard.connect_clicked(move |_| d(Command::Shell(Event::ToggleKeyboard)));

        let panels = gio::SimpleAction::new_stateful("panel", Some(glib::VariantTy::STRING), &"".to_variant());
        let d = dispatch.clone();
        panels.connect_change_state(move |a, value| {
            if let Some(name) = value.and_then(|v| v.get::<String>()) {
                a.set_state(&name.to_variant());
                d(Command::SetInputPanel(name));
            }
        });
        let sip_actions = gio::SimpleActionGroup::new();
        sip_actions.add_action(&panels);
        let chooser_menu = gio::Menu::new();
        for (label, name) in crate::keyboard::PANELS {
            let item = gio::MenuItem::new(Some(label), None);
            item.set_action_and_target_value(Some("sip.panel"), Some(&name.to_variant()));
            chooser_menu.append_item(&item);
        }
        let chooser = gtk::MenuButton::new();
        chooser.set_child(Some(&arrow()));
        chooser.add_css_class("sip-chooser");
        chooser.set_direction(gtk::ArrowType::Up);
        chooser.update_property(&[gtk::accessible::Property::Label("Input panel choice")]);
        chooser.insert_action_group("sip", Some(&sip_actions));
        let sip_groups = menus::Groups(vec![("sip".into(), sip_actions.clone().upcast())]);
        menus::attach_menu(&chooser, &chooser_menu.clone().upcast(), &sip_groups);

        row.append(&today);
        row.append(&switcher);
        row.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        row.append(&context);
        row.append(&close);
        row.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        row.append(&keyboard);
        row.append(&chooser);
        win.set_content(&row);

        let d = &cfg.design;
        Rc::new(Self {
            win,
            switcher,
            switcher_label,
            switcher_list,
            context,
            app_menus,
            shown_menus: RefCell::new(None),
            shown_actions: RefCell::new(None),
            new_menu: gio::Menu::new(),
            new_actions,
            keys,
            close,
            keyboard,
            panels,
            dispatch,
            icon_px: px,
            target_px: d.target_px,
            max_menus: max_menus(layout.screen.width, d.target_px, d.font_px, d.padding_px),
        })
    }

    /// Items of the New menu on Today (label, command).
    pub fn set_new_items(&self, items: Vec<(String, Command)>) {
        self.new_menu.remove_all();
        for name in self.new_actions.list_actions() {
            self.new_actions.remove_action(&name);
        }
        // The New button is built from the menu when shown.
        self.shown_actions.borrow_mut().take();
        for (i, (label, cmd)) in items.into_iter().enumerate() {
            let a = gio::SimpleAction::new(&format!("n{i}"), None);
            let d = self.dispatch.clone();
            a.connect_activate(move |_, _| d(cmd.clone()));
            self.new_actions.add_action(&a);
            self.new_menu.append(Some(&label), Some(&format!("new.n{i}")));
        }
    }

    /// The input panel the chooser shows as selected.
    pub fn set_input_panel(&self, name: &str) {
        self.panels.set_state(&name.to_variant());
    }

    /// Show the menus of the program in front: its published menu bar, or
    /// the generic commands.
    pub fn set_app(self: &Rc<Self>, id: WindowId, published: Option<&AppMenus>) {
        if let Some((shown, menus, _)) = &*self.shown_menus.borrow() {
            if *shown == id && menus.as_ref() == published {
                return;
            }
        }
        for g in ["app", "win"] {
            self.app_menus.insert_action_group(g, None::<&gio::ActionGroup>);
        }
        let remote = published.and_then(|m| match menus::remote(m) {
            Ok(r) => Some(r),
            Err(e) => {
                log::warn!("cannot read the menus of {}: {e}", m.bus_name);
                None
            }
        });
        match &remote {
            Some(r) => {
                self.app_menus.insert_action_group("app", r.app.as_ref());
                self.app_menus.insert_action_group("win", r.window.as_ref());
                let bar = Rc::downgrade(self);
                // The model fills in asynchronously, and changes with the
                // program's state.
                let groups = remote_groups(r);
                let g = groups.clone();
                r.model.connect_items_changed(move |model, _, _, _| {
                    if let Some(bar) = bar.upgrade() {
                        menus::fill(&bar.app_menus, &menus::entries(model), bar.max_menus, &g);
                    }
                });
                menus::fill(&self.app_menus, &menus::entries(&r.model), self.max_menus, &groups);
            }
            None => {
                let mut k = 0;
                let entries: Vec<menus::Entry> = generic_menus()
                    .into_iter()
                    .map(|(label, items)| {
                        let m = gio::Menu::new();
                        for (item, _) in items {
                            m.append(Some(item), Some(&format!("keys.k{k}")));
                            k += 1;
                        }
                        menus::Entry { label: label.into(), submenu: Some(m.upcast()), action: None }
                    })
                    .collect();
                let groups = menus::Groups(vec![("keys".into(), self.keys.clone().upcast())]);
                menus::fill(&self.app_menus, &entries, self.max_menus, &groups);
            }
        }
        *self.shown_menus.borrow_mut() = Some((id, published.cloned(), remote));
    }

    pub fn set_windows(&self, entries: Vec<SwitcherEntry>) {
        let label = match entries.len() {
            0 => "Windows".to_string(),
            n => format!("Windows ({n})"),
        };
        self.switcher.update_property(&[gtk::accessible::Property::Label(&label)]);
        self.switcher.set_tooltip_text(Some(&label));
        self.switcher_label.set_text(&match entries.len() {
            0 => String::new(),
            n => n.to_string(),
        });
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
        if self.shown_actions.borrow().as_deref() == Some(actions) {
            return;
        }
        *self.shown_actions.borrow_mut() = Some(actions.to_vec());
        while let Some(c) = self.context.first_child() {
            self.context.remove(&c);
        }
        self.close.set_visible(actions.contains(&ContextAction::CloseApp));
        for a in actions {
            let (label, cmd) = match a {
                ContextAction::CloseApp => continue,
                ContextAction::AppMenus => {
                    self.context.append(&self.app_menus);
                    continue;
                }
                ContextAction::New => {
                    if self.new_menu.n_items() > 0 {
                        let entry = menus::Entry {
                            label: "New".into(),
                            submenu: Some(self.new_menu.clone().upcast()),
                            action: None,
                        };
                        let holder = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                        let groups = menus::Groups(vec![("new".into(), self.new_actions.clone().upcast())]);
                        menus::fill(&holder, &[entry], 1, &groups);
                        self.context.append(&holder);
                    }
                    continue;
                }
                ContextAction::CancelMenu => ("Cancel", Command::Shell(Event::DismissStartMenu)),
                ContextAction::Ok => ("OK", Command::Shell(Event::Back)),
            };
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
    fn menus_that_fit_the_bar() {
        // pocketpc tokens: 46 px targets, 22 px text, 8 px padding.
        assert_eq!(max_menus(600, 46, 22, 8), 3);
        assert!(max_menus(1024, 46, 22, 8) >= 6);
        assert_eq!(max_menus(100, 46, 22, 8), 1, "always at least one");
    }

    #[test]
    fn generic_commands_are_common_shortcuts() {
        let all: Vec<(&str, KeyChord)> = generic_menus().into_iter().flat_map(|(_, i)| i).collect();
        let get = |n: &str| all.iter().find(|(l, _)| *l == n).map(|(_, c)| *c).unwrap();
        assert_eq!(get("Save"), KeyChord::ctrl('s'));
        assert_eq!(get("Paste"), KeyChord::ctrl('v'));
        assert_eq!(get("Program menu").key, Key::F(10));
    }

    #[test]
    fn close_all_needs_confirmation_text() {
        assert_eq!(close_all_label(3, false), "Close all programs…");
        assert_eq!(close_all_label(3, true), "Tap again to close all 3 programs");
    }
}

//! Menu-bar commands built from a `gio::MenuModel`: a program's own menu
//! bar (published over D-Bus by GtkApplication, see [`remote`]) or one the
//! shell builds itself (the generic commands for ordinary X11 programs, New
//! on Today).
//!
//! Top-level items with a submenu become "Name ▴" buttons that open the
//! submenu upwards; items with an action become plain buttons. What does not
//! fit goes into a final "More ▴" menu.
//!
//! Submenus are drawn by the shell as a list (not GtkPopoverMenu): rows are
//! stylus-sized, toggles and choices show their state, nested submenus open
//! in place with a Back row, and every row has an accessible name (GTK 4's
//! popover menus expose their items without any text).

use crate::platform::AppMenus;
use gtk::gio;
use gtk::prelude::*;

/// One top-level entry of a menu bar.
#[derive(Debug, Clone)]
pub struct Entry {
    pub label: String,
    pub submenu: Option<gio::MenuModel>,
    pub action: Option<(String, Option<glib::Variant>)>,
}

/// "_File" → "File" ("__" stays a literal underscore); padding some
/// programs add for alignment (Mousepad: "Save      ") is trimmed.
pub fn strip_mnemonic(label: &str) -> String {
    let mut out = String::new();
    let mut chars = label.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '_' {
            if chars.peek() == Some(&'_') {
                out.push('_');
                chars.next();
            }
        } else {
            out.push(c);
        }
    }
    out.trim().to_string()
}

/// How many of `n` entries get their own button when `max` fit; the rest
/// share the "More" menu (which takes one of the places itself).
pub fn visible_count(n: usize, max: usize) -> usize {
    if n <= max {
        n
    } else {
        max.saturating_sub(1)
    }
}

/// The top-level entries, sections flattened.
pub fn entries(model: &gio::MenuModel) -> Vec<Entry> {
    let mut out = Vec::new();
    for i in 0..model.n_items() {
        if let Some(section) = model.item_link(i, gio::MENU_LINK_SECTION) {
            out.extend(entries(&section));
            continue;
        }
        let label = model
            .item_attribute_value(i, gio::MENU_ATTRIBUTE_LABEL, Some(glib::VariantTy::STRING))
            .and_then(|v| v.get::<String>())
            .map(|s| strip_mnemonic(&s))
            .unwrap_or_default();
        let submenu = model.item_link(i, gio::MENU_LINK_SUBMENU);
        let action = model
            .item_attribute_value(i, gio::MENU_ATTRIBUTE_ACTION, Some(glib::VariantTy::STRING))
            .and_then(|v| v.get::<String>())
            .map(|a| (a, model.item_attribute_value(i, gio::MENU_ATTRIBUTE_TARGET, None)));
        if !label.is_empty() && (submenu.is_some() || action.is_some()) {
            out.push(Entry { label, submenu, action });
        }
    }
    out
}

fn arrow_label(text: &str) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 3);
    b.append(&gtk::Label::new(Some(text)));
    let a = gtk::Label::new(Some("▴"));
    a.add_css_class("arrow");
    b.append(&a);
    b
}

/// Action groups by prefix ("app", "win", ...), to show whether an item is
/// enabled and its state.
#[derive(Clone, Default)]
pub struct Groups(pub Vec<(String, gio::ActionGroup)>);

impl Groups {
    fn lookup(&self, action: &str) -> Option<(&gio::ActionGroup, String)> {
        let (prefix, name) = action.split_once('.')?;
        self.0.iter().find(|(p, _)| p == prefix).map(|(_, g)| (g, name.to_string()))
    }

    fn enabled(&self, action: &str) -> bool {
        self.lookup(action).is_none_or(|(g, n)| !g.has_action(&n) || g.is_action_enabled(&n))
    }

    /// "✓" for a set toggle or the chosen value of a choice.
    fn mark(&self, action: &str, target: Option<&glib::Variant>) -> &'static str {
        let Some(state) = self.lookup(action).and_then(|(g, n)| g.action_state(&n)) else { return "" };
        let on = match target {
            Some(t) => &state == t,
            None => state.get::<bool>() == Some(true),
        };
        if on {
            "✓"
        } else {
            ""
        }
    }
}

/// A row of a menu: the label, and what tapping it does.
enum RowKind {
    Action(String, Option<glib::Variant>),
    Submenu(gio::MenuModel),
    Back,
}

/// A menu and its sections: a program's menus arrive over D-Bus piece by
/// piece, each of these may still fill in.
fn models_of(model: &gio::MenuModel) -> Vec<gio::MenuModel> {
    let mut out = vec![model.clone()];
    for i in 0..model.n_items() {
        if let Some(section) = model.item_link(i, gio::MENU_LINK_SECTION) {
            out.extend(models_of(&section));
        }
    }
    out
}

/// Signal handlers on the menu being shown, dropped when it changes.
type Watches = std::rc::Rc<std::cell::RefCell<Vec<(gio::MenuModel, glib::SignalHandlerId)>>>;

fn unwatch(watches: &Watches) {
    for (m, id) in watches.borrow_mut().drain(..) {
        m.disconnect(id);
    }
}

/// Rows of a menu model; sections are separated by a line.
fn menu_rows(model: &gio::MenuModel) -> Vec<(String, RowKind, bool)> {
    let mut rows = Vec::new();
    for i in 0..model.n_items() {
        if let Some(section) = model.item_link(i, gio::MENU_LINK_SECTION) {
            let mut inner = menu_rows(&section);
            if let (Some(first), false) = (inner.first_mut(), rows.is_empty()) {
                first.2 = true;
            }
            rows.extend(inner);
            continue;
        }
        let label = model
            .item_attribute_value(i, gio::MENU_ATTRIBUTE_LABEL, Some(glib::VariantTy::STRING))
            .and_then(|v| v.get::<String>())
            .map(|s| strip_mnemonic(&s))
            .unwrap_or_default();
        if let Some(sub) = model.item_link(i, gio::MENU_LINK_SUBMENU) {
            rows.push((label, RowKind::Submenu(sub), false));
        } else if let Some(action) = model
            .item_attribute_value(i, gio::MENU_ATTRIBUTE_ACTION, Some(glib::VariantTy::STRING))
            .and_then(|v| v.get::<String>())
        {
            let target = model.item_attribute_value(i, gio::MENU_ATTRIBUTE_TARGET, None);
            rows.push((label, RowKind::Action(action, target), false));
        }
    }
    rows
}

/// Fill `list` with `model`'s items; `back` is the menu to return to.
fn show_menu(
    list: &gtk::ListBox,
    popover: &gtk::Popover,
    model: &gio::MenuModel,
    back: Vec<gio::MenuModel>,
    groups: &Groups,
    watches: &Watches,
) {
    list.remove_all();
    // Show what arrives later, while the menu is open.
    unwatch(watches);
    for m in models_of(model) {
        let (l, p, top, b, g, w) = (list.clone(), popover.clone(), model.clone(), back.clone(), groups.clone(), watches.clone());
        let id = m.connect_items_changed(move |_, _, _, _| {
            if p.is_visible() {
                // Re-rendering replaces these handlers; let this one return first.
                let (l, p, top, b, g, w) = (l.clone(), p.clone(), top.clone(), b.clone(), g.clone(), w.clone());
                glib::idle_add_local_once(move || {
                    if p.is_visible() {
                        show_menu(&l, &p, &top, b, &g, &w);
                    }
                });
            }
        });
        watches.borrow_mut().push((m, id));
    }
    let mut rows = menu_rows(model);
    if !back.is_empty() {
        rows.insert(0, ("Back".into(), RowKind::Back, false));
    }
    for (label, kind, separated) in rows {
        let row = gtk::ListBoxRow::new();
        let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let mark = gtk::Label::new(None);
        mark.add_css_class("menu-mark");
        let text = gtk::Label::new(Some(&label));
        text.set_xalign(0.0);
        text.set_hexpand(true);
        b.append(&mark);
        b.append(&text);
        match &kind {
            RowKind::Action(action, target) => {
                mark.set_text(groups.mark(action, target.as_ref()));
                row.set_sensitive(groups.enabled(action));
            }
            RowKind::Submenu(_) => b.append(&gtk::Label::new(Some("▸"))),
            RowKind::Back => mark.set_text("◂"),
        }
        row.set_child(Some(&b));
        row.update_property(&[gtk::accessible::Property::Label(&label)]);
        if separated {
            row.set_header(Some(&gtk::Separator::new(gtk::Orientation::Horizontal)));
        }
        let (l, p, m, bk, g, w) =
            (list.clone(), popover.clone(), model.clone(), back.clone(), groups.clone(), watches.clone());
        let kind = std::rc::Rc::new(kind);
        let gesture = gtk::GestureClick::new();
        gesture.connect_released(move |_, _, _, _| match &*kind {
            RowKind::Action(action, target) => {
                p.popdown();
                if let Some(parent) = p.parent() {
                    if let Err(e) = parent.activate_action(action, target.as_ref()) {
                        log::warn!("menu action {action}: {e}");
                    }
                }
            }
            RowKind::Submenu(sub) => {
                let mut stack = bk.clone();
                stack.push(m.clone());
                show_menu(&l, &p, sub, stack, &g, &w);
            }
            RowKind::Back => {
                let mut stack = bk.clone();
                if let Some(prev) = stack.pop() {
                    show_menu(&l, &p, &prev, stack, &g, &w);
                }
            }
        });
        row.add_controller(gesture);
        list.append(&row);
    }
}

/// A "Label ▴" button that opens `model` upwards as a list menu.
pub fn menu_button(label: &str, model: &gio::MenuModel, groups: &Groups) -> gtk::MenuButton {
    let b = gtk::MenuButton::new();
    b.set_child(Some(&arrow_label(label)));
    b.update_property(&[gtk::accessible::Property::Label(label)]);
    attach_menu(&b, model, groups);
    b
}

thread_local! {
    /// Tallest a menu may be: the space between the bars.
    static MAX_HEIGHT: std::cell::Cell<i32> = const { std::cell::Cell::new(400) };
}

/// Long menus (a program's File menu) scroll instead of leaving the screen.
pub fn set_max_height(px: u32) {
    MAX_HEIGHT.with(|h| h.set(px as i32));
}

/// Make `b` open `model` upwards as a list menu.
pub fn attach_menu(b: &gtk::MenuButton, model: &gio::MenuModel, groups: &Groups) {
    b.set_direction(gtk::ArrowType::Up);
    let list = gtk::ListBox::new();
    list.add_css_class("compact");
    list.add_css_class("targets");
    list.add_css_class("menu-list");
    list.set_selection_mode(gtk::SelectionMode::None);
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .propagate_natural_width(true)
        .max_content_height(MAX_HEIGHT.with(|h| h.get()))
        .child(&list)
        .build();
    let popover = gtk::Popover::new();
    popover.add_css_class("pda");
    popover.set_has_arrow(false);
    popover.set_child(Some(&scroller));
    b.set_popover(Some(&popover));
    // Built when opened: the program's menus and action states change.
    let watches: Watches = Default::default();
    let (m, g, l, w) = (model.clone(), groups.clone(), list.clone(), watches.clone());
    popover.connect_show(move |p| show_menu(&l, p, &m, Vec::new(), &g, &w));
    popover.connect_closed(move |_| unwatch(&watches));
}

/// Replace the buttons in `bar` with `entries`, at most `max` buttons.
pub fn fill(bar: &gtk::Box, entries: &[Entry], max: usize, groups: &Groups) {
    while let Some(c) = bar.first_child() {
        bar.remove(&c);
    }
    let shown = visible_count(entries.len(), max);
    for e in &entries[..shown] {
        if let Some(sub) = &e.submenu {
            bar.append(&menu_button(&e.label, sub, groups));
        } else if let Some((action, target)) = &e.action {
            let b = gtk::Button::with_label(&e.label);
            b.update_property(&[gtk::accessible::Property::Label(&e.label)]);
            let (action, target) = (action.clone(), target.clone());
            b.connect_clicked(move |w| {
                if let Err(err) = w.activate_action(&action, target.as_ref()) {
                    log::warn!("menu action {action}: {err}");
                }
            });
            bar.append(&b);
        }
    }
    if shown < entries.len() {
        let more = gio::Menu::new();
        for e in &entries[shown..] {
            if let Some(sub) = &e.submenu {
                more.append_submenu(Some(&e.label), sub);
            } else if let Some((action, target)) = &e.action {
                let item = gio::MenuItem::new(Some(&e.label), None);
                item.set_action_and_target_value(Some(action), target.as_ref());
                more.append_item(&item);
            }
        }
        bar.append(&menu_button("More", &more.upcast(), groups));
    }
}

/// A program's published menu bar and the action groups its items use.
pub struct Remote {
    pub model: gio::MenuModel,
    pub app: Option<gio::ActionGroup>,
    pub window: Option<gio::ActionGroup>,
}

/// Connect to a program's menus on the session bus. The model loads
/// asynchronously: watch `items-changed`.
pub fn remote(menus: &AppMenus) -> Result<Remote, glib::Error> {
    let conn = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)?;
    let bus = Some(menus.bus_name.as_str());
    let group = |path: &Option<String>| {
        path.as_deref().map(|p| gio::DBusActionGroup::get(&conn, bus, p).upcast::<gio::ActionGroup>())
    };
    Ok(Remote {
        model: gio::DBusMenuModel::get(&conn, bus, &menus.menubar_path).upcast(),
        app: group(&menus.app_path),
        window: group(&menus.window_path),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mnemonics_are_removed() {
        assert_eq!(strip_mnemonic("_File"), "File");
        assert_eq!(strip_mnemonic("Save _As…"), "Save As…");
        assert_eq!(strip_mnemonic("a__b"), "a_b");
        assert_eq!(strip_mnemonic("_Save      "), "Save");
    }

    #[test]
    fn overflow_keeps_one_place_for_more() {
        assert_eq!(visible_count(3, 3), 3);
        assert_eq!(visible_count(6, 3), 2, "two menus and More");
        assert_eq!(visible_count(2, 0), 0);
    }

    #[test]
    fn entries_flatten_sections_and_skip_empty_items() {
        let file = gio::Menu::new();
        file.append(Some("_Save"), Some("win.save"));
        let section = gio::Menu::new();
        section.append_submenu(Some("_File"), &file);
        let bar = gio::Menu::new();
        bar.append_section(None, &section);
        bar.append(Some("_New"), Some("app.new"));
        bar.append(Some("No action"), None);
        let e = entries(&bar.upcast());
        assert_eq!(e.iter().map(|e| e.label.as_str()).collect::<Vec<_>>(), vec!["File", "New"]);
        assert!(e[0].submenu.is_some());
        assert_eq!(e[1].action.as_ref().map(|(a, _)| a.as_str()), Some("app.new"));
    }
}

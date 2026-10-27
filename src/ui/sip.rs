//! The built-in input panel: a Pocket PC style keyboard drawn by the shell.
//!
//! Two key sets: the full keyboard (five rows with a number row, Shift and
//! Caps, Ctrl and Alt as one-shot modifiers, arrows and Esc) and spreadsheet
//! keys (digits, operators, `=` `SUM(` `(` `)` `:`, column letters A-H and
//! navigation), switched with its 123 / abc key. Taps are typed into the
//! program in front with XTEST. The window is unmanaged (override-redirect),
//! so the window manager never gives it the keyboard focus: the program
//! keeps it, as with a hardware keyboard.
//!
//! Docked, it sits in the space the command bar reserves for keyboards, like
//! Onboard, and programs shrink above it. Floating (landscape by default),
//! it reserves nothing: programs keep their full height and the panel sits
//! in a corner of the work area; dragging its title strip moves it, a tap
//! on the strip sends it to the next corner.

use super::ShellWindow;
use crate::config::Config;
use crate::platform::x11::{gtk_glue, X11};
use crate::platform::{Key, KeyChord, SurfaceRole};
use crate::shell::layout::{Orientation, Rect};
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// The panel's key sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keys {
    Text,
    Sheet,
}

impl Keys {
    /// Grid columns per row: every row's key spans add up to this.
    pub fn columns(self) -> u32 {
        match self {
            Keys::Text => 28,
            Keys::Sheet => 20,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Keys::Text => "text",
            Keys::Sheet => "sheet",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// A character key: (plain, shifted).
    Char(char, char),
    Key(Key),
    /// Several characters at once (`SUM(`).
    Text(&'static str),
    /// Switch to the other key set.
    Switch(Keys),
    Shift,
    Caps,
    Ctrl,
    Alt,
}

/// (label, action, span in columns)
pub type KeyDef = (&'static str, Action, u32);

fn chars(s: &'static str, shifted: &'static str) -> Vec<KeyDef> {
    // Labels are the plain characters themselves; leak-free static slices.
    s.char_indices()
        .zip(shifted.chars())
        .map(|((i, c), u)| (&s[i..i + c.len_utf8()], Action::Char(c, u), 2))
        .collect()
}

/// The rows of a key set.
pub fn rows(keys: Keys) -> Vec<Vec<KeyDef>> {
    match keys {
        Keys::Text => text_rows(),
        Keys::Sheet => sheet_rows(),
    }
}

/// Spreadsheet keys: formula symbols and the first column letters on the
/// left, a number pad on the right, navigation at the bottom. Everything a
/// formula like `=SUM(B2:B13)*2` needs is one tap away.
fn sheet_rows() -> Vec<Vec<KeyDef>> {
    let key = |label: &'static str, action| (label, action, 2);
    let c = |label: &'static str| {
        let ch = label.chars().next().unwrap();
        (label, Action::Char(ch, ch), 2)
    };
    vec![
        vec![c("="), key("SUM(", Action::Text("SUM(")), c("("), c(")"), c(":"), c("7"), c("8"), c("9"), c("/"),
            key("⌫", Action::Key(Key::BackSpace))],
        vec![c("A"), c("B"), c("C"), c("D"), c("E"), c("4"), c("5"), c("6"), c("*"), key("Tab", Action::Key(Key::Tab))],
        vec![c("F"), c("G"), c("H"), c("%"), c("$"), c("1"), c("2"), c("3"), c("-"),
            key("↵", Action::Key(Key::Return))],
        vec![key("abc", Action::Switch(Keys::Text)), key("←", Action::Key(Key::Left)), key("↑", Action::Key(Key::Up)),
            key("↓", Action::Key(Key::Down)), key("→", Action::Key(Key::Right)), c("0"), c("."), c(","), c("+"),
            key("Esc", Action::Key(Key::Escape))],
    ]
}

/// The five rows of the full keyboard.
fn text_rows() -> Vec<Vec<KeyDef>> {
    let mut r1 = vec![("Esc", Action::Key(Key::Escape), 2)];
    r1.extend(chars("1234567890-=", "!@#$%^&*()_+"));
    r1.push(("⌫", Action::Key(Key::BackSpace), 2));

    let mut r2 = vec![("Tab", Action::Key(Key::Tab), 2)];
    r2.extend(chars("qwertyuiop[]\\", "QWERTYUIOP{}|"));

    let mut r3 = vec![("CAP", Action::Caps, 3)];
    r3.extend(chars("asdfghjkl;'", "ASDFGHJKL:\""));
    r3.push(("↵", Action::Key(Key::Return), 3));

    let mut r4 = vec![("Shift", Action::Shift, 4)];
    r4.extend(chars("zxcvbnm,./", "ZXCVBNM<>?"));
    r4.push(("↑", Action::Key(Key::Up), 2));
    r4.push(("Del", Action::Key(Key::Delete), 2));

    let mut r5 = vec![("Ctl", Action::Ctrl, 3), ("Alt", Action::Alt, 3)];
    r5.push(("123", Action::Switch(Keys::Sheet), 2));
    r5.push(("space", Action::Char(' ', ' '), 14));
    r5.push(("←", Action::Key(Key::Left), 2));
    r5.push(("↓", Action::Key(Key::Down), 2));
    r5.push(("→", Action::Key(Key::Right), 2));
    vec![r1, r2, r3, r4, r5]
}

/// Modifier state of the panel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    pub caps: bool,
    pub ctrl: bool,
    pub alt: bool,
}

impl Mods {
    /// The chord a key produces, and the state after it: Shift, Ctrl and Alt
    /// apply to one key only; Caps stays until tapped again. `None` for
    /// modifier keys, which only change the state, and for the keys the
    /// panel handles itself (several characters, switching key sets).
    pub fn press(self, action: Action) -> (Option<KeyChord>, Mods) {
        let mut next = self;
        let chord = match action {
            Action::Text(_) | Action::Switch(_) => return (None, self),
            Action::Shift => {
                next.shift = !self.shift;
                None
            }
            Action::Caps => {
                next.caps = !self.caps;
                None
            }
            Action::Ctrl => {
                next.ctrl = !self.ctrl;
                None
            }
            Action::Alt => {
                next.alt = !self.alt;
                None
            }
            Action::Char(plain, shifted) => {
                let upper = if plain.is_alphabetic() { self.shift != self.caps } else { self.shift };
                // With Ctrl, programs expect the plain letter (Ctrl+S, not Ctrl+Shift+S).
                let c = if upper && !self.ctrl { shifted } else { plain };
                Some(KeyChord { key: Key::Char(c), ctrl: self.ctrl, alt: self.alt, shift: false })
            }
            Action::Key(k) => Some(KeyChord { key: k, ctrl: self.ctrl, alt: self.alt, shift: self.shift }),
        };
        if chord.is_some() {
            next.shift = false;
            next.ctrl = false;
            next.alt = false;
        }
        (chord, next)
    }

    /// What a key shows in this state.
    pub fn label(self, def: &KeyDef) -> String {
        match def.1 {
            Action::Char(plain, shifted) if plain != ' ' => {
                let upper = if plain.is_alphabetic() { self.shift != self.caps } else { self.shift };
                if upper { shifted } else { plain }.to_string()
            }
            _ => def.0.to_string(),
        }
    }
}

/// Where the panel goes: docked above the command bar (programs shrink) or
/// floating over programs in a corner of the work area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    Docked,
    Floating,
}

impl Placement {
    /// From `[keyboard] placement`: "docked", "floating", or "auto"
    /// (floating in landscape, where height is short; docked in portrait).
    pub fn from_config(value: &str, orientation: Orientation) -> Self {
        match value {
            "docked" => Placement::Docked,
            "floating" => Placement::Floating,
            "auto" => match orientation {
                Orientation::Landscape => Placement::Floating,
                Orientation::Portrait => Placement::Docked,
            },
            other => {
                log::warn!("unknown keyboard placement {other:?}; docked");
                Placement::Docked
            }
        }
    }
}

/// Corners of the work area a floating panel can sit in, in the order a
/// tap on its strip cycles through them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner {
    BottomRight,
    TopRight,
    TopLeft,
    BottomLeft,
}

impl Corner {
    pub fn next(self) -> Self {
        match self {
            Corner::BottomRight => Corner::TopRight,
            Corner::TopRight => Corner::TopLeft,
            Corner::TopLeft => Corner::BottomLeft,
            Corner::BottomLeft => Corner::BottomRight,
        }
    }
}

/// Size of a floating panel: keys of the shell's target size, plus the
/// strip to move it by; never larger than the work area.
pub fn floating_size(keys: Keys, target_px: u32, area: Rect) -> (u32, u32) {
    let rows = rows(keys).len() as u32;
    let strip = strip_height(target_px);
    let w = (keys.columns() / 2 * target_px).min(area.width);
    let h = (rows * target_px + strip).min(area.height);
    (w, h)
}

fn strip_height(target_px: u32) -> u32 {
    (target_px * 3 / 5).max(20)
}

/// A floating panel of `size` in `corner` of `area`.
pub fn floating_rect(size: (u32, u32), corner: Corner, area: Rect) -> Rect {
    let (w, h) = (size.0.min(area.width), size.1.min(area.height));
    let right = area.right() - w as i32;
    let bottom = area.bottom() - h as i32;
    let (x, y) = match corner {
        Corner::BottomRight => (right, bottom),
        Corner::TopRight => (right, area.y),
        Corner::TopLeft => (area.x, area.y),
        Corner::BottomLeft => (area.x, bottom),
    };
    Rect::new(x, y, w, h)
}

/// `rect` moved by (dx, dy), kept inside `area`.
pub fn dragged(rect: Rect, dx: f64, dy: f64, area: Rect) -> Rect {
    let x = (rect.x + dx.round() as i32).clamp(area.x, area.right() - rect.width as i32);
    let y = (rect.y + dy.round() as i32).clamp(area.y, area.bottom() - rect.height as i32);
    Rect { x, y, ..rect }
}

pub struct InputPanel {
    pub win: ShellWindow,
    x11: Rc<X11>,
    keys: Vec<(gtk::Button, KeyDef)>,
    mods: Cell<Mods>,
    stack: gtk::Stack,
    set: Cell<Keys>,
    placement: Placement,
    /// Docked: the reserved keyboard space. Floating: the work area the
    /// panel moves in.
    area: Rect,
    target_px: u32,
    corner: Cell<Corner>,
    /// A floating panel the user dragged stays where it was put.
    dragged_to: Cell<Option<Rect>>,
    /// The panel's rectangle and the pointer (root coordinates) when a
    /// drag began. Root coordinates, because the strip moves with the
    /// window during the drag.
    drag_start: RefCell<Option<(Rect, (i32, i32))>>,
}

impl InputPanel {
    /// `docked` is the space reserved for keyboards; `workarea` is where a
    /// floating panel moves.
    pub fn new(
        app: &gtk::Application,
        docked: Rect,
        workarea: Rect,
        placement: Placement,
        x11: &Rc<X11>,
        cfg: &Config,
    ) -> Rc<Self> {
        let (area, rect) = match placement {
            Placement::Docked => (docked, docked),
            Placement::Floating => {
                (workarea, floating_rect(floating_size(Keys::Text, cfg.design.target_px, workarea), Corner::BottomRight, workarea))
            }
        };
        let win = ShellWindow::new(app, "Input panel", "sip-panel", SurfaceRole::Escape, rect, x11, cfg);
        let stack = gtk::Stack::new();
        stack.set_vexpand(true);
        // Sized by the key set shown, not the larger of the two.
        stack.set_hhomogeneous(false);
        stack.set_vhomogeneous(false);
        let mut keys = Vec::new();
        for set in [Keys::Text, Keys::Sheet] {
            let grid = gtk::Grid::new();
            grid.set_column_homogeneous(true);
            grid.set_row_homogeneous(true);
            for (r, row) in rows(set).into_iter().enumerate() {
                let mut col = 0;
                for def in row {
                    let b = gtk::Button::with_label(def.0);
                    b.add_css_class("sip-key");
                    // Words (Shift, SUM(, abc) in smaller text, so small keys
                    // fit them.
                    if def.0.chars().count() > 1 {
                        b.add_css_class("word");
                    }
                    b.set_focusable(false);
                    b.set_focus_on_click(false);
                    b.update_property(&[gtk::accessible::Property::Label(def.0)]);
                    grid.attach(&b, col as i32, r as i32, def.2 as i32, 1);
                    col += def.2;
                    keys.push((b, def));
                }
                debug_assert_eq!(col, set.columns());
            }
            stack.add_named(&grid, Some(set.name()));
        }
        // Floating only: a strip to drag the panel by (tap: next corner).
        let strip = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        strip.add_css_class("sip-strip");
        let grip = gtk::Label::new(Some("⠿ Move"));
        grip.set_hexpand(true);
        grip.set_halign(gtk::Align::Start);
        strip.append(&grip);
        strip.set_size_request(-1, strip_height(cfg.design.target_px) as i32);
        strip.set_visible(placement == Placement::Floating);
        strip.update_property(&[gtk::accessible::Property::Label("Move input panel")]);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&strip);
        content.append(&stack);
        win.set_content(&content);
        if placement == Placement::Floating {
            win.window.add_css_class("floating");
        }
        let panel = Rc::new(Self {
            win,
            x11: x11.clone(),
            keys,
            mods: Cell::new(Mods::default()),
            stack,
            set: Cell::new(Keys::Text),
            placement,
            area,
            target_px: cfg.design.target_px,
            corner: Cell::new(Corner::BottomRight),
            dragged_to: Cell::new(None),
            drag_start: RefCell::new(None),
        });
        for (b, def) in &panel.keys {
            let (p, action) = (Rc::downgrade(&panel), def.1);
            b.connect_clicked(move |_| {
                if let Some(p) = p.upgrade() {
                    p.press(action);
                }
            });
        }
        let drag = gtk::GestureDrag::new();
        let p = Rc::downgrade(&panel);
        drag.connect_drag_begin(move |_, _, _| {
            if let Some(p) = p.upgrade() {
                if let Ok(pointer) = p.x11.pointer() {
                    *p.drag_start.borrow_mut() = Some((p.current_rect(), pointer));
                }
            }
        });
        let p = Rc::downgrade(&panel);
        drag.connect_drag_update(move |_, _, _| {
            let Some(p) = p.upgrade() else { return };
            let Some((start, (x0, y0))) = *p.drag_start.borrow() else { return };
            let Ok((x, y)) = p.x11.pointer() else { return };
            let moved = dragged(start, (x - x0) as f64, (y - y0) as f64, p.area);
            if moved != p.current_rect() {
                p.dragged_to.set(Some(moved));
                p.win.set_rect(moved);
            }
        });
        let p = Rc::downgrade(&panel);
        drag.connect_drag_end(move |_, dx, dy| {
            let Some(p) = p.upgrade() else { return };
            p.drag_start.borrow_mut().take();
            if dx.abs() < 8.0 && dy.abs() < 8.0 {
                // A tap: the next corner.
                p.dragged_to.set(None);
                p.corner.set(p.corner.get().next());
                p.place();
            }
        });
        strip.add_controller(drag);
        // The first rectangle was computed before the keys could be measured.
        if placement == Placement::Floating {
            panel.win.set_rect(panel.current_rect());
        }
        panel
    }

    pub fn placement(&self) -> Placement {
        self.placement
    }

    fn current_rect(&self) -> Rect {
        match self.placement {
            Placement::Docked => self.area,
            Placement::Floating => self.dragged_to.get().unwrap_or_else(|| {
                // Never smaller than the keys need, or the window would grow
                // past the screen edge after being placed.
                let (w, h) = floating_size(self.set.get(), self.target_px, self.area);
                // The window, so its CSS border counts too.
                let (min_w, ..) = self.win.window.measure(gtk::Orientation::Horizontal, -1);
                let (min_h, ..) = self.win.window.measure(gtk::Orientation::Vertical, -1);
                let size = (w.max(min_w.max(0) as u32), h.max(min_h.max(0) as u32));
                floating_rect(size, self.corner.get(), self.area)
            }),
        }
    }

    fn place(&self) {
        self.win.set_rect(self.current_rect());
        self.raise();
    }

    /// Show the full keyboard or the spreadsheet keys.
    pub fn set_keys(&self, set: Keys) {
        if self.set.replace(set) == set {
            return;
        }
        self.stack.set_visible_child_name(set.name());
        if self.placement == Placement::Floating {
            // Keep a dragged panel's position, with the new size.
            if let Some(r) = self.dragged_to.get() {
                self.dragged_to.set(None);
                let size = self.current_rect();
                self.dragged_to.set(Some(dragged(Rect { width: size.width, height: size.height, ..r }, 0.0, 0.0, self.area)));
            }
            self.place();
        }
    }

    pub fn keys(&self) -> Keys {
        self.set.get()
    }

    fn press(&self, action: Action) {
        match action {
            Action::Switch(set) => return self.set_keys(set),
            Action::Text(text) => {
                for c in text.chars() {
                    let chord = KeyChord { key: Key::Char(c), ctrl: false, alt: false, shift: false };
                    if let Err(e) = self.x11.send_key(chord) {
                        log::warn!("input panel: cannot type {chord:?}: {e}");
                    }
                }
                return;
            }
            _ => {}
        }
        let (chord, next) = self.mods.get().press(action);
        if let Some(chord) = chord {
            if let Err(e) = self.x11.send_key(chord) {
                log::warn!("input panel: cannot type {chord:?}: {e}");
            }
        }
        if next != self.mods.get() {
            self.mods.set(next);
            self.relabel();
        }
    }

    fn relabel(&self) {
        let m = self.mods.get();
        for (b, def) in &self.keys {
            b.set_label(&m.label(def));
            let latched = match def.1 {
                Action::Shift => m.shift,
                Action::Caps => m.caps,
                Action::Ctrl => m.ctrl,
                Action::Alt => m.alt,
                _ => false,
            };
            if latched {
                b.add_css_class("latched");
            } else {
                b.remove_css_class("latched");
            }
        }
    }

    pub fn is_visible(&self) -> bool {
        self.win.window.is_visible()
    }

    pub fn set_visible(&self, visible: bool) {
        if visible == self.is_visible() {
            return;
        }
        // Not present(): that would ask the window manager to activate it.
        self.win.window.set_visible(visible);
        if visible {
            self.raise();
        }
    }

    /// Put the panel back above programs. The shell calls this when windows
    /// appear or change; not on a timer, which would also cover the command
    /// bar's menus opening over the panel.
    pub fn raise(&self) {
        if let Some(xid) = gtk_glue::xid(self.win.window.upcast_ref()).filter(|_| self.is_visible()) {
            let _ = self.x11.raise(xid);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_fills_the_width() {
        for set in [Keys::Text, Keys::Sheet] {
            for (i, row) in rows(set).iter().enumerate() {
                assert_eq!(row.iter().map(|k| k.2).sum::<u32>(), set.columns(), "{set:?} row {i}");
            }
        }
    }

    #[test]
    fn spreadsheet_keys_are_one_tap_away() {
        let sheet = rows(Keys::Sheet);
        let has_char = |c: char| sheet.iter().flatten().any(|k| k.1 == Action::Char(c, c));
        for c in "0123456789=+-*/():.,".chars() {
            assert!(has_char(c), "missing {c}");
        }
        let has = |a: Action| sheet.iter().flatten().any(|k| k.1 == a);
        for k in [Key::Left, Key::Right, Key::Up, Key::Down, Key::Return, Key::Tab, Key::BackSpace, Key::Escape] {
            assert!(has(Action::Key(k)), "missing {k:?}");
        }
        assert!(has(Action::Text("SUM(")));
        assert!(has(Action::Switch(Keys::Text)));
        assert!(rows(Keys::Text).iter().flatten().any(|k| k.1 == Action::Switch(Keys::Sheet)));
        // Switching and multi-character keys leave the modifiers alone.
        let shifted = Mods { shift: true, ..Default::default() };
        assert_eq!(shifted.press(Action::Text("SUM(")), (None, shifted));
    }

    #[test]
    fn placement_from_config() {
        assert_eq!(Placement::from_config("auto", Orientation::Landscape), Placement::Floating);
        assert_eq!(Placement::from_config("auto", Orientation::Portrait), Placement::Docked);
        assert_eq!(Placement::from_config("floating", Orientation::Portrait), Placement::Floating);
        assert_eq!(Placement::from_config("docked", Orientation::Landscape), Placement::Docked);
        assert_eq!(Placement::from_config("sideways", Orientation::Landscape), Placement::Docked);
    }

    #[test]
    fn floating_panel_leaves_the_application_area_larger() {
        // Landscape 1024x600, pocketpc bars: work area 1024x502 at y 48.
        let wa = Rect::new(0, 48, 1024, 502);
        let (w, h) = floating_size(Keys::Sheet, 46, wa);
        assert_eq!((w, h), (460, 4 * 46 + 27));
        let r = floating_rect((w, h), Corner::BottomRight, wa);
        assert_eq!((r.right(), r.bottom()), (wa.right(), wa.bottom()));
        // Docked, programs get 1024x302; floating, they keep 1024x502 and
        // the spreadsheet keys cover under a fifth of it.
        assert!(((w * h) as f64) < 0.2 * (wa.width * wa.height) as f64);
        let text = floating_size(Keys::Text, 46, wa);
        assert_eq!(text.0, 14 * 46);
        // Corners cycle and stay inside.
        let mut c = Corner::BottomRight;
        for _ in 0..4 {
            assert!(wa.contains(&floating_rect(text, c, wa)), "{c:?}");
            c = c.next();
        }
        assert_eq!(c, Corner::BottomRight);
        assert_eq!(floating_rect(text, Corner::TopLeft, wa), Rect::new(0, 48, 644, text.1));
        // Never larger than a small screen's work area (800x480: 800x382).
        let small = Rect::new(0, 48, 800, 382);
        let s = floating_size(Keys::Text, 46, small);
        assert!(s.0 <= 800 && s.1 <= 382);
    }

    #[test]
    fn dragging_stays_inside_the_work_area() {
        let wa = Rect::new(0, 48, 1024, 502);
        let r = Rect::new(500, 300, 460, 211);
        assert_eq!(dragged(r, -100.0, -50.0, wa), Rect::new(400, 250, 460, 211));
        assert_eq!(dragged(r, -900.0, -900.0, wa), Rect::new(0, 48, 460, 211));
        assert_eq!(dragged(r, 900.0, 900.0, wa), Rect::new(564, 339, 460, 211));
    }

    #[test]
    fn shift_applies_to_one_key_caps_stays() {
        let a = Action::Char('a', 'A');
        let m = Mods::default();
        let (none, m) = m.press(Action::Shift);
        assert!(none.is_none() && m.shift);
        let (c, m) = m.press(a);
        assert_eq!(c.unwrap().key, Key::Char('A'));
        let (c, m) = m.press(a);
        assert_eq!(c.unwrap().key, Key::Char('a'), "shift was one-shot");
        let (_, m) = m.press(Action::Caps);
        let (c, m) = m.press(a);
        assert_eq!(c.unwrap().key, Key::Char('A'));
        assert!(m.caps, "caps lock stays");
        // Caps does not shift digits.
        let (c, _) = m.press(Action::Char('1', '!'));
        assert_eq!(c.unwrap().key, Key::Char('1'));
    }

    #[test]
    fn ctrl_types_the_plain_letter() {
        let (_, m) = Mods::default().press(Action::Ctrl);
        let (c, m) = m.press(Action::Char('s', 'S'));
        assert_eq!(c, Some(KeyChord::ctrl('s')));
        assert!(!m.ctrl);
    }

    #[test]
    fn labels_follow_the_state() {
        let q: KeyDef = ("q", Action::Char('q', 'Q'), 2);
        let one: KeyDef = ("1", Action::Char('1', '!'), 2);
        let shifted = Mods { shift: true, ..Default::default() };
        assert_eq!(shifted.label(&q), "Q");
        assert_eq!(shifted.label(&one), "!");
        assert_eq!(Mods { caps: true, ..Default::default() }.label(&one), "1");
    }
}

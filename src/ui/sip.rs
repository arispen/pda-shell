//! The built-in input panel: a Pocket PC style keyboard drawn by the shell.
//!
//! Five rows with a number row, Shift and Caps, Ctrl and Alt as one-shot
//! modifiers, arrows and Esc. Taps are typed into the program in front with
//! XTEST. The window is unmanaged (override-redirect), so the window manager
//! never gives it the keyboard focus: the program keeps it, as with a
//! hardware keyboard. It sits in the space the command bar reserves for
//! keyboards, like Onboard.

use super::ShellWindow;
use crate::config::Config;
use crate::platform::x11::{gtk_glue, X11};
use crate::platform::{Key, KeyChord, SurfaceRole};
use crate::shell::layout::Rect;
use gtk::prelude::*;
use std::cell::Cell;
use std::rc::Rc;

/// Grid columns per row: every row's key spans add up to this.
pub const COLUMNS: u32 = 28;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// A character key: (plain, shifted).
    Char(char, char),
    Key(Key),
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

/// The five rows of the keyboard.
pub fn rows() -> Vec<Vec<KeyDef>> {
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
    r5.extend(chars("`", "~"));
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
    /// modifier keys, which only change the state.
    pub fn press(self, action: Action) -> (Option<KeyChord>, Mods) {
        let mut next = self;
        let chord = match action {
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

pub struct InputPanel {
    pub win: ShellWindow,
    x11: Rc<X11>,
    keys: Vec<(gtk::Button, KeyDef)>,
    mods: Cell<Mods>,
}

impl InputPanel {
    pub fn new(app: &gtk::Application, rect: Rect, x11: &Rc<X11>, cfg: &Config) -> Rc<Self> {
        let win = ShellWindow::new(app, "Input panel", "sip-panel", SurfaceRole::Escape, rect, x11, cfg);
        let grid = gtk::Grid::new();
        grid.set_column_homogeneous(true);
        grid.set_row_homogeneous(true);
        let mut keys = Vec::new();
        for (r, row) in rows().into_iter().enumerate() {
            let mut col = 0;
            for def in row {
                let b = gtk::Button::with_label(def.0);
                b.add_css_class("sip-key");
                b.set_focusable(false);
                b.set_focus_on_click(false);
                b.update_property(&[gtk::accessible::Property::Label(def.0)]);
                grid.attach(&b, col as i32, r as i32, def.2 as i32, 1);
                col += def.2;
                keys.push((b, def));
            }
            debug_assert_eq!(col, COLUMNS);
        }
        win.set_content(&grid);
        let panel = Rc::new(Self {
            win,
            x11: x11.clone(),
            keys,
            mods: Cell::new(Mods::default()),
        });
        for (b, def) in &panel.keys {
            let (p, action) = (Rc::downgrade(&panel), def.1);
            b.connect_clicked(move |_| {
                if let Some(p) = p.upgrade() {
                    p.press(action);
                }
            });
        }
        panel
    }

    fn press(&self, action: Action) {
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
        for (i, row) in rows().iter().enumerate() {
            assert_eq!(row.iter().map(|k| k.2).sum::<u32>(), COLUMNS, "row {i}");
        }
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

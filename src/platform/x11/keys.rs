//! Typing into the program in front with XTEST: key symbols, the server's
//! keyboard mapping, and where a symbol is on it.

use crate::platform::Key;

pub const SHIFT_L: u32 = 0xffe1;
pub const CONTROL_L: u32 = 0xffe3;
pub const ALT_L: u32 = 0xffe9;

/// The X keysym of a key.
pub fn keysym(key: Key) -> u32 {
    match key {
        Key::Char(c) => char_keysym(c),
        Key::Return => 0xff0d,
        Key::BackSpace => 0xff08,
        Key::Tab => 0xff09,
        Key::Escape => 0xff1b,
        Key::Delete => 0xffff,
        Key::Left => 0xff51,
        Key::Up => 0xff52,
        Key::Right => 0xff53,
        Key::Down => 0xff54,
        Key::F(n) => 0xffbe + n.clamp(1, 12) as u32 - 1,
    }
}

/// Latin-1 characters are their own keysym; others use the Unicode range.
pub fn char_keysym(c: char) -> u32 {
    match c as u32 {
        cp @ (0x20..=0x7e | 0xa0..=0xff) => cp,
        cp => 0x0100_0000 + cp,
    }
}

/// The server's keycode → keysyms table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap {
    pub min_keycode: u8,
    pub per_keycode: usize,
    pub syms: Vec<u32>,
}

impl Keymap {
    /// Keycode and level (0 plain, 1 shifted) that produce `sym`.
    pub fn find(&self, sym: u32) -> Option<(u8, u8)> {
        let per = self.per_keycode.max(1);
        for (i, row) in self.syms.chunks(per).enumerate() {
            for (level, s) in row.iter().take(2).enumerate() {
                if *s == sym {
                    return Some((self.min_keycode + i as u8, level as u8));
                }
            }
        }
        None
    }

    /// A keycode with no symbols, to map a symbol the keyboard lacks.
    pub fn spare(&self) -> Option<u8> {
        let per = self.per_keycode.max(1);
        // From the top: low keycodes are the ones real keyboards use.
        self.syms
            .chunks(per)
            .enumerate()
            .rev()
            .find(|(_, row)| row.iter().all(|s| *s == 0))
            .map(|(i, _)| self.min_keycode + i as u8)
    }

    pub fn set(&mut self, keycode: u8, sym: u32) {
        let per = self.per_keycode.max(1);
        let start = (keycode - self.min_keycode) as usize * per;
        for s in &mut self.syms[start..start + per] {
            *s = sym;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> Keymap {
        // keycode 8: a A, 9: 1 !, 10: Return, 11: empty
        Keymap { min_keycode: 8, per_keycode: 2, syms: vec![0x61, 0x41, 0x31, 0x21, 0xff0d, 0, 0, 0] }
    }

    #[test]
    fn finds_plain_and_shifted_symbols() {
        let m = map();
        assert_eq!(m.find(char_keysym('a')), Some((8, 0)));
        assert_eq!(m.find(char_keysym('A')), Some((8, 1)));
        assert_eq!(m.find(char_keysym('!')), Some((9, 1)));
        assert_eq!(m.find(keysym(Key::Return)), Some((10, 0)));
        assert_eq!(m.find(char_keysym('€')), None);
    }

    #[test]
    fn missing_symbols_go_to_a_spare_keycode() {
        let mut m = map();
        assert_eq!(m.spare(), Some(11));
        m.set(11, char_keysym('€'));
        assert_eq!(m.find(char_keysym('€')), Some((11, 0)));
        assert_eq!(m.spare(), None);
    }

    #[test]
    fn keysyms() {
        assert_eq!(char_keysym('ä'), 0xe4);
        assert_eq!(char_keysym('€'), 0x0100_20ac);
        assert_eq!(keysym(Key::F(11)), 0xffc8);
        assert_eq!(keysym(Key::Char(' ')), 0x20);
    }
}

//! Arrows for dialogs larger than the work area (Gnumeric's Save As needs
//! 876 px at 144 dpi; portrait has 600, landscape with a docked keyboard
//! 302 px of height). The shell fits such a dialog so its buttons stay
//! visible; these arrows, over the status bar, slide it to show the part
//! that is cut off (the file name field) and back. Unmanaged, like the
//! full-screen escape, so it stays above the dialog.

use super::ShellWindow;
use crate::config::Config;
use crate::platform::x11::{gtk_glue, X11};
use crate::platform::{DialogOverflow, SurfaceRole};
use crate::shell::layout::{Layout, Rect};
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

/// The four directions, as (label, accessible name).
const ARROWS: [(&str, &str); 4] = [
    ("◀", "Show the left part of the dialog"),
    ("▶", "Show the right part of the dialog"),
    ("▲", "Show the top of the dialog"),
    ("▼", "Show the bottom of the dialog"),
];

/// Where the dialog's outer frame goes for each arrow: (x, y); `None`
/// keeps that coordinate.
pub fn pan_target(o: &DialogOverflow, arrow: usize) -> (Option<i32>, Option<i32>) {
    let a = o.area;
    match arrow {
        0 => (Some(a.x), None),
        1 => (Some(a.right() - o.size.0 as i32), None),
        2 => (None, Some(a.y)),
        _ => (None, Some(a.bottom() - o.size.1 as i32)),
    }
}

/// Which arrows a dialog needs.
pub fn arrows(o: &DialogOverflow) -> [bool; 4] {
    let wide = o.size.0 > o.area.width;
    let tall = o.size.1 > o.area.height;
    [wide, wide, tall, tall]
}

/// Top-right corner over the status bar, as wide as the arrows shown.
pub fn pan_rect(layout: &Layout, cfg: &Config, count: u32) -> Rect {
    let h = layout.top_bar.height.max(cfg.design.target_px);
    let w = (cfg.design.target_px * count.max(1)).min(layout.screen.width);
    Rect::new((layout.screen.width - w) as i32, 0, w, h)
}

pub struct DialogPan {
    pub win: ShellWindow,
    x11: Rc<X11>,
    layout: Layout,
    target_px: u32,
    top_px: u32,
    buttons: Vec<gtk::Button>,
    current: Rc<RefCell<Option<DialogOverflow>>>,
}

impl DialogPan {
    pub fn new(app: &gtk::Application, layout: &Layout, x11: &Rc<X11>, cfg: &Config) -> Rc<Self> {
        let win = ShellWindow::new(app, "Dialog arrows", "dialog-pan", SurfaceRole::Escape, pan_rect(layout, cfg, 2), x11, cfg);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        row.set_homogeneous(true);
        let current: Rc<RefCell<Option<DialogOverflow>>> = Rc::default();
        let mut buttons = Vec::new();
        for (i, (label, name)) in ARROWS.iter().enumerate() {
            // The arrow glyph is decoration; the button's name says what it does.
            let glyph = gtk::Label::builder().label(*label).accessible_role(gtk::AccessibleRole::Presentation).build();
            let b = gtk::Button::new();
            b.set_child(Some(&glyph));
            b.set_focusable(false);
            b.set_focus_on_click(false);
            b.update_property(&[gtk::accessible::Property::Label(name)]);
            let (x, cur) = (x11.clone(), current.clone());
            b.connect_clicked(move |_| {
                if let Some(o) = cur.borrow().as_ref() {
                    let (tx, ty) = pan_target(o, i);
                    if let Err(e) = x.move_window(o.window, tx, ty) {
                        log::warn!("cannot move dialog: {e}");
                    }
                }
            });
            row.append(&b);
            buttons.push(b);
        }
        win.set_content(&row);
        Rc::new(Self {
            win,
            x11: x11.clone(),
            layout: layout.clone(),
            target_px: cfg.design.target_px,
            top_px: layout.top_bar.height.max(cfg.design.target_px),
            buttons,
            current,
        })
    }

    /// Show the arrows for this dialog, or hide them.
    pub fn set(&self, overflow: Option<DialogOverflow>) {
        if *self.current.borrow() == overflow {
            return;
        }
        match &overflow {
            Some(o) => {
                let shown = arrows(o);
                for (b, on) in self.buttons.iter().zip(shown) {
                    b.set_visible(on);
                }
                let count = shown.iter().filter(|s| **s).count() as u32;
                let w = (self.target_px * count.max(1)).min(self.layout.screen.width);
                self.win.set_rect(Rect::new((self.layout.screen.width - w) as i32, 0, w, self.top_px));
                log::info!("geometry: offering arrows for dialog {:?} ({}x{} in {:?})", o.window, o.size.0, o.size.1, o.area);
                self.win.window.set_visible(true);
                self.raise();
            }
            None => self.win.window.set_visible(false),
        }
        *self.current.borrow_mut() = overflow;
    }

    pub fn raise(&self) {
        if let Some(xid) = gtk_glue::xid(self.win.window.upcast_ref()).filter(|_| self.win.window.is_visible()) {
            let _ = self.x11.raise(xid);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::WindowId;

    #[test]
    fn arrows_follow_the_overflow() {
        // Gnumeric's Save As in portrait: wider than the work area.
        let wide = DialogOverflow { window: WindowId(1), size: (878, 460), area: Rect::new(0, 48, 600, 926) };
        assert_eq!(arrows(&wide), [true, true, false, false]);
        assert_eq!(pan_target(&wide, 0), (Some(0), None), "left edge on screen: the Name field");
        assert_eq!(pan_target(&wide, 1), (Some(600 - 878), None), "right edge: Save and Cancel");
        // Landscape with a docked keyboard: taller than the work area.
        let tall = DialogOverflow { window: WindowId(1), size: (878, 460), area: Rect::new(0, 48, 1024, 302) };
        assert_eq!(arrows(&tall), [false, false, true, true]);
        assert_eq!(pan_target(&tall, 2), (None, Some(48)), "top: the Name field under the status bar");
        assert_eq!(pan_target(&tall, 3), (None, Some(48 + 302 - 460)), "bottom: the buttons");
    }
}

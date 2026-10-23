//! "Exit full screen" button. A program in full screen covers both bars, so
//! this small unmanaged window stays on top of it and gives the stylus a way
//! back without the program's own (often keyboard-only) controls.

use super::ShellWindow;
use crate::config::Config;
use crate::platform::x11::{gtk_glue, X11};
use crate::platform::SurfaceRole;
use crate::shell::layout::{Layout, Rect};
use crate::shell::state::Event;
use crate::shell::{Command, Dispatch};
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

/// Top-right corner, the size of a status bar button.
pub fn escape_rect(layout: &Layout, cfg: &Config) -> Rect {
    let h = layout.top_bar.height.max(cfg.design.target_px);
    let w = (cfg.design.font_px * 10 + cfg.design.icon_px + 4 * cfg.design.padding_px).min(layout.screen.width);
    Rect::new((layout.screen.width - w) as i32, 0, w, h)
}

pub struct FullscreenEscape {
    pub win: ShellWindow,
    x11: Rc<X11>,
    raise_timer: RefCell<Option<glib::SourceId>>,
}

impl FullscreenEscape {
    pub fn new(app: &gtk::Application, layout: &Layout, x11: &Rc<X11>, cfg: &Config, dispatch: Dispatch) -> Rc<Self> {
        let rect = escape_rect(layout, cfg);
        let win = ShellWindow::new(app, "Exit full screen", "escape", SurfaceRole::Escape, rect, x11, cfg);
        let b = gtk::Button::new();
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 3);
        content.set_halign(gtk::Align::Center);
        content.append(&super::icon_image(None, "view-restore-symbolic", cfg.design.icon_px));
        content.append(&gtk::Label::new(Some("Exit full screen")));
        b.set_child(Some(&content));
        b.connect_clicked(move |_| dispatch(Command::Shell(Event::LeaveFullscreen)));
        win.set_content(&b);
        Rc::new(Self { win, x11: x11.clone(), raise_timer: RefCell::new(None) })
    }

    pub fn set_visible(self: &Rc<Self>, visible: bool) {
        if visible == self.win.window.is_visible() {
            return;
        }
        if !visible {
            if let Some(t) = self.raise_timer.take() {
                t.remove();
            }
            self.win.window.set_visible(false);
            return;
        }
        self.win.window.present();
        // The full-screen program is raised again whenever it is tapped; keep
        // the button above it.
        let w = Rc::downgrade(self);
        let id = glib::timeout_add_local(Duration::from_millis(400), move || match w.upgrade() {
            Some(e) => {
                if let Some(xid) = gtk_glue::xid(e.win.window.upcast_ref()) {
                    let _ = e.x11.raise(xid);
                }
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });
        *self.raise_timer.borrow_mut() = Some(id);
    }
}

//! The one place where GTK and X11 meet: getting the X window of a GTK window.

use gtk::prelude::*;

/// X window ID of a realized GTK window, if it runs on the X11 backend.
#[allow(deprecated)] // GTK 4.18 deprecates the X11 backend API; see docs/known-limitations.md
pub fn xid(window: &gtk::Window) -> Option<u32> {
    let surface = window.surface()?;
    let x11 = surface.downcast_ref::<gdk4_x11::X11Surface>()?;
    Some(x11.xid() as u32)
}

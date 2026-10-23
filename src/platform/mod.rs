//! Platform layer. Everything that talks to the window system lives below
//! this module; the rest of the shell only sees the neutral types here.

pub mod x11;

/// Opaque identifier of a top-level application window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WindowId(pub u32);

/// An application window the shell can switch to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppWindow {
    pub id: WindowId,
    pub title: String,
    /// WM_CLASS class part, e.g. "Mousepad". Used for icons and fallbacks.
    pub app_class: String,
    pub pid: Option<u32>,
    /// `_NET_WM_STATE_FULLSCREEN`: the window covers the bars.
    pub fullscreen: bool,
}

impl AppWindow {
    pub fn display_title(&self) -> &str {
        if self.title.trim().is_empty() {
            &self.app_class
        } else {
            &self.title
        }
    }
}

/// What the window manager currently reports.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WindowSnapshot {
    /// Application windows in the window manager's mapping order.
    pub windows: Vec<AppWindow>,
    /// The active window, if it is one of `windows`.
    pub active: Option<WindowId>,
    pub showing_desktop: bool,
}

/// Requests the shell makes of the window manager.
pub trait WindowSystem {
    fn activate(&self, id: WindowId);
    fn close(&self, id: WindowId);
    fn show_desktop(&self, on: bool);
    fn set_fullscreen(&self, id: WindowId, on: bool);
}

/// How one of the shell's own windows should be treated by the window manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceRole {
    /// Persistent bar that reserves a screen edge.
    TopPanel,
    BottomPanel,
    /// Background surface below all applications (the Today screen).
    Desktop,
    /// Transient shell surface above applications (the Start menu).
    Overlay,
    /// Unmanaged surface above everything, even full-screen windows (the
    /// "Exit full screen" button).
    Escape,
}

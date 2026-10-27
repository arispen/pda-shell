//! Platform layer. Everything that talks to the window system lives below
//! this module; the rest of the shell only sees the neutral types here.

pub mod x11;

/// Opaque identifier of a top-level application window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WindowId(pub u32);

/// Where a program publishes its menus (GtkApplication's D-Bus export,
/// announced in `_GTK_*` window properties). The command bar shows them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppMenus {
    /// Unique session-bus name of the program, e.g. ":1.42".
    pub bus_name: String,
    /// `org.gtk.Menus` object of the menu bar.
    pub menubar_path: String,
    /// `org.gtk.Actions` objects for "app." and "win." actions.
    pub app_path: Option<String>,
    pub window_path: Option<String>,
}

impl AppMenus {
    /// From the raw property values; `None` unless both the bus name and a
    /// menu bar are announced.
    pub fn from_properties(bus: &str, menubar: &str, app: &str, window: &str) -> Option<Self> {
        let some = |s: &str| Some(s.to_string()).filter(|s| !s.is_empty());
        Some(Self {
            bus_name: some(bus)?,
            menubar_path: some(menubar)?,
            app_path: some(app),
            window_path: some(window),
        })
    }
}

/// An application window the shell can switch to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppWindow {
    pub id: WindowId,
    pub title: String,
    /// WM_CLASS class part, e.g. "Mousepad". Used for icons and fallbacks.
    pub app_class: String,
    pub pid: Option<u32>,
    /// `_NET_WM_STATE_FULLSCREEN`: the window covers the bars.
    pub fullscreen: bool,
    /// Menus the program publishes; `None` for ordinary X11 programs.
    pub menus: Option<AppMenus>,
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

#[cfg(test)]
mod tests {
    use super::AppMenus;

    #[test]
    fn menus_need_a_bus_name_and_a_menu_bar() {
        let m = AppMenus::from_properties(":1.3", "/org/x/menus/menubar", "/org/x", "").unwrap();
        assert_eq!(m.app_path.as_deref(), Some("/org/x"));
        assert_eq!(m.window_path, None);
        assert!(AppMenus::from_properties("", "/m", "", "").is_none());
        assert!(AppMenus::from_properties(":1.3", "", "/a", "/w").is_none());
    }
}

/// A key the shell types into the program in front (input panel, generic
/// commands such as Save = Ctrl+S).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Return,
    BackSpace,
    Tab,
    Escape,
    Delete,
    Left,
    Right,
    Up,
    Down,
    F(u8),
}

/// A key with modifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyChord {
    pub key: Key,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl KeyChord {
    pub fn plain(key: Key) -> Self {
        Self { key, ctrl: false, alt: false, shift: false }
    }

    pub fn ctrl(c: char) -> Self {
        Self { ctrl: true, ..Self::plain(Key::Char(c)) }
    }
}

/// A dialog that does not fit the work area even after shrinking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogOverflow {
    pub window: WindowId,
    /// Outer (frame) size.
    pub size: (u32, u32),
    /// The work area it was fitted into.
    pub area: crate::shell::layout::Rect,
}

/// What the window manager currently reports.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WindowSnapshot {
    /// Application windows in the window manager's mapping order.
    pub windows: Vec<AppWindow>,
    /// The active window, if it is one of `windows`.
    pub active: Option<WindowId>,
    pub showing_desktop: bool,
    /// The most recent dialog larger than the work area, if any.
    pub overflow: Option<DialogOverflow>,
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

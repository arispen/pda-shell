//! Shell state machine. UI and window-manager notifications come in as
//! [`Event`]s; the resulting work comes out as [`Effect`]s that the GTK side
//! executes. Nothing here touches GTK or X11, so it is fully unit-tested.

use crate::config::FullscreenMode;
use crate::platform::{AppWindow, WindowId, WindowSnapshot};

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Windows(WindowSnapshot),
    StartButton,
    DismissStartMenu,
    Today,
    Switch(WindowId),
    CloseActive,
    CloseAll,
    ToggleKeyboard,
    /// The keyboard reported its real visibility (it can also be hidden
    /// with its own close key, or exit).
    KeyboardVisible(bool),
    /// The "Exit full screen" button.
    LeaveFullscreen,
    Launch(String),
    EndSession,
    /// Applications did not all close in time; stay in the session.
    EndSessionTimeout,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    ShowDesktop(bool),
    Activate(WindowId),
    Close(WindowId),
    ShowStartMenu(bool),
    Launch(String),
    /// Show or hide the on-screen keyboard.
    SetKeyboard(bool),
    /// Reserve (or release) screen space for the keyboard, so applications
    /// shrink instead of being covered. Reserved only once the keyboard has
    /// reported that it is visible, so a keyboard that fails to start never
    /// leaves the applications shrunk.
    ReserveKeyboard(bool),
    SetFullscreen(WindowId, bool),
    /// Arm the end-session timeout.
    StartEndSessionTimer,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Today,
    App(WindowId),
}

/// Shell-specific commands offered in the command bar for the current view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextAction {
    Refresh,
    CloseApp,
    CancelMenu,
}

#[derive(Debug)]
pub struct ShellState {
    fullscreen_mode: FullscreenMode,
    windows: Vec<AppWindow>,
    /// Most recently used first.
    mru: Vec<WindowId>,
    active: Option<WindowId>,
    today: bool,
    start_menu_open: bool,
    /// The keyboard is shown, or was asked to show (drives the toggle).
    keyboard_visible: bool,
    /// Screen space is currently reserved for the keyboard.
    keyboard_reserved: bool,
    ending_session: bool,
    /// Shown in the status bar until the user does something else.
    notice: Option<String>,
}

impl Default for ShellState {
    fn default() -> Self {
        Self::new()
    }
}

impl ShellState {
    pub fn new() -> Self {
        Self::with_fullscreen_mode(FullscreenMode::Escape)
    }

    pub fn with_fullscreen_mode(fullscreen_mode: FullscreenMode) -> Self {
        Self {
            fullscreen_mode,
            windows: Vec::new(),
            mru: Vec::new(),
            active: None,
            today: true,
            start_menu_open: false,
            keyboard_visible: false,
            keyboard_reserved: false,
            ending_session: false,
            notice: None,
        }
    }

    pub fn view(&self) -> View {
        match self.active {
            Some(id) if !self.today => View::App(id),
            _ => View::Today,
        }
    }

    pub fn start_menu_open(&self) -> bool {
        self.start_menu_open
    }

    pub fn keyboard_visible(&self) -> bool {
        self.keyboard_visible
    }

    pub fn ending_session(&self) -> bool {
        self.ending_session
    }

    /// The active application covers the bars; offer the way back.
    pub fn fullscreen_escape(&self) -> bool {
        match self.view() {
            View::App(id) => self.window(id).is_some_and(|w| w.fullscreen),
            View::Today => false,
        }
    }

    /// Windows for the switcher, most recently used first.
    pub fn switcher_entries(&self) -> Vec<&AppWindow> {
        self.mru.iter().filter_map(|id| self.window(*id)).collect()
    }

    pub fn window(&self, id: WindowId) -> Option<&AppWindow> {
        self.windows.iter().find(|w| w.id == id)
    }

    pub fn title(&self) -> String {
        if self.start_menu_open {
            return "Start".into();
        }
        if let Some(n) = &self.notice {
            return n.clone();
        }
        match self.view() {
            View::Today => "Today".into(),
            View::App(id) => self
                .window(id)
                .map(|w| w.display_title().to_string())
                .unwrap_or_default(),
        }
    }

    pub fn context_actions(&self) -> Vec<ContextAction> {
        if self.start_menu_open {
            return vec![ContextAction::CancelMenu];
        }
        match self.view() {
            View::Today => vec![ContextAction::Refresh],
            View::App(_) => vec![ContextAction::CloseApp],
        }
    }

    pub fn handle(&mut self, event: Event) -> Vec<Effect> {
        let mut fx = Vec::new();
        if !matches!(event, Event::Windows(_) | Event::KeyboardVisible(_) | Event::EndSessionTimeout) {
            self.notice = None;
        }
        match event {
            Event::Windows(snap) => self.apply_snapshot(snap, &mut fx),
            Event::StartButton => {
                self.start_menu_open = !self.start_menu_open;
                fx.push(Effect::ShowStartMenu(self.start_menu_open));
            }
            Event::DismissStartMenu => self.close_menu(&mut fx),
            Event::Today => {
                self.close_menu(&mut fx);
                if !self.today {
                    self.today = true;
                    fx.push(Effect::ShowDesktop(true));
                }
            }
            Event::Switch(id) => {
                self.close_menu(&mut fx);
                if self.window(id).is_some() {
                    self.today = false;
                    self.active = Some(id);
                    self.touch(id);
                    fx.push(Effect::Activate(id));
                }
            }
            Event::CloseActive => {
                if let View::App(id) = self.view() {
                    fx.push(Effect::Close(id));
                }
            }
            Event::CloseAll => {
                fx.extend(self.windows.iter().map(|w| Effect::Close(w.id)));
            }
            Event::ToggleKeyboard => {
                self.keyboard_visible = !self.keyboard_visible;
                fx.push(Effect::SetKeyboard(self.keyboard_visible));
                // Space is given back at once; it is taken only when the
                // keyboard confirms it is visible (KeyboardVisible).
                if !self.keyboard_visible {
                    self.release_keyboard(&mut fx);
                }
            }
            Event::KeyboardVisible(v) => {
                self.keyboard_visible = v;
                if v && !self.keyboard_reserved {
                    self.keyboard_reserved = true;
                    fx.push(Effect::ReserveKeyboard(true));
                } else if !v {
                    self.release_keyboard(&mut fx);
                }
            }
            Event::LeaveFullscreen => {
                if let (true, View::App(id)) = (self.fullscreen_escape(), self.view()) {
                    fx.push(Effect::SetFullscreen(id, false));
                }
            }
            Event::Launch(app) => {
                self.close_menu(&mut fx);
                fx.push(Effect::Launch(app));
            }
            Event::EndSession => {
                self.close_menu(&mut fx);
                if self.windows.is_empty() {
                    fx.push(Effect::Quit);
                } else {
                    // Ask every application to close the normal way, so each
                    // one can offer to save. Only quit once they are all gone.
                    self.ending_session = true;
                    fx.extend(self.windows.iter().map(|w| Effect::Close(w.id)));
                    fx.push(Effect::StartEndSessionTimer);
                }
            }
            Event::EndSessionTimeout => {
                self.ending_session = false;
                if !self.windows.is_empty() {
                    self.notice = Some("Some programs are still open".into());
                }
            }
        }
        fx
    }

    fn release_keyboard(&mut self, fx: &mut Vec<Effect>) {
        if self.keyboard_reserved {
            self.keyboard_reserved = false;
            fx.push(Effect::ReserveKeyboard(false));
        }
    }

    fn close_menu(&mut self, fx: &mut Vec<Effect>) {
        if self.start_menu_open {
            self.start_menu_open = false;
            fx.push(Effect::ShowStartMenu(false));
        }
    }

    fn touch(&mut self, id: WindowId) {
        self.mru.retain(|w| *w != id);
        self.mru.insert(0, id);
    }

    fn apply_snapshot(&mut self, snap: WindowSnapshot, fx: &mut Vec<Effect>) {
        let new_windows: Vec<WindowId> = snap
            .windows
            .iter()
            .filter(|w| self.window(w.id).is_none())
            .map(|w| w.id)
            .collect();
        if self.fullscreen_mode == FullscreenMode::Prevent {
            for w in snap.windows.iter().filter(|w| w.fullscreen) {
                if !self.window(w.id).is_some_and(|old| old.fullscreen) {
                    fx.push(Effect::SetFullscreen(w.id, false));
                }
            }
        }

        self.windows = snap.windows;
        let alive = |id: &WindowId| self.windows.iter().any(|w| w.id == *id);
        let mut mru: Vec<WindowId> = self.mru.iter().copied().filter(alive).collect();
        for w in &self.windows {
            if !mru.contains(&w.id) {
                mru.push(w.id);
            }
        }
        self.mru = mru;

        self.active = snap.active.filter(alive);
        if let Some(id) = self.active {
            self.touch(id);
        }
        self.today = snap.showing_desktop || self.active.is_none();

        // A window that appears while Today is showing was just opened by
        // the user (from the shell or another app): bring it to the front.
        if let Some(&newest) = new_windows.last() {
            if self.today {
                self.today = false;
                self.active = Some(newest);
                self.touch(newest);
                fx.push(Effect::Activate(newest));
            }
        }

        if self.ending_session && self.windows.is_empty() {
            fx.push(Effect::Quit);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(id: u32, title: &str) -> AppWindow {
        AppWindow { id: WindowId(id), title: title.into(), app_class: "App".into(), pid: None, fullscreen: false }
    }

    fn fullscreen(id: u32) -> AppWindow {
        AppWindow { fullscreen: true, ..win(id, "video") }
    }

    fn snap(windows: Vec<AppWindow>, active: Option<u32>, showing_desktop: bool) -> Event {
        Event::Windows(WindowSnapshot { windows, active: active.map(WindowId), showing_desktop })
    }

    #[test]
    fn starts_on_today() {
        let s = ShellState::new();
        assert_eq!(s.view(), View::Today);
        assert_eq!(s.title(), "Today");
        assert_eq!(s.context_actions(), vec![ContextAction::Refresh]);
    }

    #[test]
    fn start_button_toggles_menu() {
        let mut s = ShellState::new();
        assert_eq!(s.handle(Event::StartButton), vec![Effect::ShowStartMenu(true)]);
        assert!(s.start_menu_open());
        assert_eq!(s.title(), "Start");
        assert_eq!(s.context_actions(), vec![ContextAction::CancelMenu]);
        assert_eq!(s.handle(Event::StartButton), vec![Effect::ShowStartMenu(false)]);
        assert!(!s.start_menu_open());
    }

    #[test]
    fn dismissing_a_closed_menu_does_nothing() {
        let mut s = ShellState::new();
        assert!(s.handle(Event::DismissStartMenu).is_empty());
    }

    #[test]
    fn launch_closes_menu_and_launches() {
        let mut s = ShellState::new();
        s.handle(Event::StartButton);
        assert_eq!(
            s.handle(Event::Launch("mousepad.desktop".into())),
            vec![Effect::ShowStartMenu(false), Effect::Launch("mousepad.desktop".into())]
        );
    }

    #[test]
    fn active_window_becomes_the_view() {
        let mut s = ShellState::new();
        s.handle(snap(vec![win(1, "notes.txt - Mousepad")], Some(1), false));
        assert_eq!(s.view(), View::App(WindowId(1)));
        assert_eq!(s.title(), "notes.txt - Mousepad");
        assert_eq!(s.context_actions(), vec![ContextAction::CloseApp]);
    }

    #[test]
    fn untitled_window_falls_back_to_class() {
        let mut s = ShellState::new();
        s.handle(snap(vec![win(1, " ")], Some(1), false));
        assert_eq!(s.title(), "App");
    }

    #[test]
    fn new_window_while_on_today_is_activated() {
        let mut s = ShellState::new();
        let fx = s.handle(snap(vec![win(7, "Thunar")], None, true));
        assert_eq!(fx, vec![Effect::Activate(WindowId(7))]);
        assert_eq!(s.view(), View::App(WindowId(7)));
    }

    #[test]
    fn existing_windows_do_not_steal_today() {
        let mut s = ShellState::new();
        s.handle(snap(vec![win(1, "a")], Some(1), false));
        s.handle(Event::Today);
        let fx = s.handle(snap(vec![win(1, "a")], None, true));
        assert!(fx.is_empty());
        assert_eq!(s.view(), View::Today);
    }

    #[test]
    fn today_shows_desktop_once() {
        let mut s = ShellState::new();
        s.handle(snap(vec![win(1, "a")], Some(1), false));
        assert_eq!(s.handle(Event::Today), vec![Effect::ShowDesktop(true)]);
        assert!(s.handle(Event::Today).is_empty());
    }

    #[test]
    fn switch_activates_known_window_only() {
        let mut s = ShellState::new();
        s.handle(snap(vec![win(1, "a"), win(2, "b")], Some(2), false));
        assert_eq!(s.handle(Event::Switch(WindowId(1))), vec![Effect::Activate(WindowId(1))]);
        assert_eq!(s.view(), View::App(WindowId(1)));
        assert!(s.handle(Event::Switch(WindowId(99))).is_empty());
    }

    #[test]
    fn switcher_is_most_recently_used_first() {
        let mut s = ShellState::new();
        s.handle(snap(vec![win(1, "a"), win(2, "b"), win(3, "c")], Some(1), false));
        s.handle(snap(vec![win(1, "a"), win(2, "b"), win(3, "c")], Some(3), false));
        let ids: Vec<u32> = s.switcher_entries().iter().map(|w| w.id.0).collect();
        assert_eq!(ids, vec![3, 1, 2]);
    }

    #[test]
    fn closed_windows_leave_the_switcher() {
        let mut s = ShellState::new();
        s.handle(snap(vec![win(1, "a"), win(2, "b")], Some(2), false));
        s.handle(snap(vec![win(1, "a")], Some(1), false));
        let ids: Vec<u32> = s.switcher_entries().iter().map(|w| w.id.0).collect();
        assert_eq!(ids, vec![1]);
    }

    #[test]
    fn last_window_closing_returns_to_today() {
        let mut s = ShellState::new();
        s.handle(snap(vec![win(1, "a")], Some(1), false));
        s.handle(snap(vec![], None, false));
        assert_eq!(s.view(), View::Today);
    }

    #[test]
    fn close_active_only_in_app_view() {
        let mut s = ShellState::new();
        assert!(s.handle(Event::CloseActive).is_empty());
        s.handle(snap(vec![win(4, "x")], Some(4), false));
        assert_eq!(s.handle(Event::CloseActive), vec![Effect::Close(WindowId(4))]);
    }

    #[test]
    fn keyboard_space_is_reserved_once_the_keyboard_is_visible() {
        let mut s = ShellState::new();
        assert_eq!(s.handle(Event::ToggleKeyboard), vec![Effect::SetKeyboard(true)]);
        assert!(s.keyboard_visible(), "the toggle shows the request at once");
        assert_eq!(s.handle(Event::KeyboardVisible(true)), vec![Effect::ReserveKeyboard(true)]);
        // Repeated reports change nothing.
        assert!(s.handle(Event::KeyboardVisible(true)).is_empty());
        assert_eq!(s.handle(Event::ToggleKeyboard), vec![Effect::SetKeyboard(false), Effect::ReserveKeyboard(false)]);
        assert!(s.handle(Event::KeyboardVisible(false)).is_empty());
    }

    #[test]
    fn keyboard_that_fails_to_start_reserves_nothing() {
        let mut s = ShellState::new();
        s.handle(Event::ToggleKeyboard);
        // Launch failed or the program exited before showing itself.
        assert!(s.handle(Event::KeyboardVisible(false)).is_empty(), "no space was taken, none to give back");
        assert!(!s.keyboard_visible(), "the toggle rolls back");
        assert_eq!(s.handle(Event::ToggleKeyboard), vec![Effect::SetKeyboard(true)]);
    }

    #[test]
    fn keyboard_hidden_by_its_own_close_key_releases_space() {
        let mut s = ShellState::new();
        s.handle(Event::ToggleKeyboard);
        s.handle(Event::KeyboardVisible(true));
        // Onboard reports it was hidden: the toggle follows and the
        // application gets its space back.
        assert_eq!(s.handle(Event::KeyboardVisible(false)), vec![Effect::ReserveKeyboard(false)]);
        assert!(!s.keyboard_visible());
        // The next tap shows it again instead of being a no-op hide.
        assert_eq!(s.handle(Event::ToggleKeyboard), vec![Effect::SetKeyboard(true)]);
    }

    #[test]
    fn fullscreen_app_gets_an_escape() {
        let mut s = ShellState::new();
        s.handle(snap(vec![win(1, "a")], Some(1), false));
        assert!(!s.fullscreen_escape());
        assert!(s.handle(Event::LeaveFullscreen).is_empty());
        let fx = s.handle(snap(vec![fullscreen(1)], Some(1), false));
        assert!(fx.is_empty(), "escape mode allows full screen");
        assert!(s.fullscreen_escape());
        assert_eq!(s.handle(Event::LeaveFullscreen), vec![Effect::SetFullscreen(WindowId(1), false)]);
        // Today hides the app, so there is nothing to escape from.
        s.handle(snap(vec![fullscreen(1)], None, true));
        assert!(!s.fullscreen_escape());
    }

    #[test]
    fn prevent_mode_takes_windows_out_of_fullscreen_once() {
        let mut s = ShellState::with_fullscreen_mode(FullscreenMode::Prevent);
        s.handle(snap(vec![win(1, "a")], Some(1), false));
        assert_eq!(s.handle(snap(vec![fullscreen(1)], Some(1), false)), vec![Effect::SetFullscreen(WindowId(1), false)]);
        // Not repeated for every snapshot while the request is in flight.
        assert!(s.handle(snap(vec![fullscreen(1)], Some(1), false)).is_empty());
    }

    #[test]
    fn end_session_without_apps_quits() {
        let mut s = ShellState::new();
        assert_eq!(s.handle(Event::EndSession), vec![Effect::Quit]);
    }

    #[test]
    fn end_session_closes_apps_then_quits() {
        let mut s = ShellState::new();
        s.handle(snap(vec![win(1, "a"), win(2, "b")], Some(2), false));
        assert_eq!(
            s.handle(Event::EndSession),
            vec![
                Effect::Close(WindowId(1)),
                Effect::Close(WindowId(2)),
                Effect::StartEndSessionTimer
            ]
        );
        assert!(s.handle(snap(vec![win(1, "a")], Some(1), false)).is_empty());
        assert_eq!(s.handle(snap(vec![], None, false)), vec![Effect::Quit]);
    }

    #[test]
    fn end_session_timeout_keeps_the_session() {
        let mut s = ShellState::new();
        s.handle(snap(vec![win(1, "unsaved")], Some(1), false));
        s.handle(Event::EndSession);
        s.handle(Event::EndSessionTimeout);
        assert!(!s.ending_session());
        assert_eq!(s.title(), "Some programs are still open");
        s.handle(Event::Switch(WindowId(1)));
        assert_eq!(s.title(), "unsaved", "the notice goes away with the next action");
        // Closing the app later is not mistaken for the end of the session.
        assert!(s.handle(snap(vec![], None, false)).is_empty());
    }
}

//! The shell controller: owns the state machine and the UI components, and
//! executes effects against the window system, launcher and keyboard.

pub mod layout;
pub mod shortcuts;
pub mod state;

use crate::apps::{self, AppEntry};
use crate::config::Config;
use crate::keyboard::focus::{self, Focus, FocusWatch};
use crate::keyboard::Keyboard;
use crate::platform::x11::{self, gtk_glue, X11};
use crate::platform::{SurfaceRole, WindowId, WindowSystem};
use crate::ui::command_bar::{CommandBar, SwitcherEntry};
use crate::ui::dialog_pan::DialogPan;
use crate::ui::escape::FullscreenEscape;
use crate::ui::sip::{InputPanel, Placement};
use crate::ui::start_menu::StartMenu;
use crate::ui::status_bar::StatusBar;
use crate::ui::today::Today;
use gtk::gio;
use gtk::prelude::*;
use layout::Layout;
use state::{Effect, Event, ShellState, View};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

/// Everything the UI can ask the shell to do.
#[derive(Debug, Clone)]
pub enum Command {
    Shell(Event),
    StartButton,
    StartMenuFocusLost,
    OpenUri(String),
    OpenFolder(String),
    AddShortcut(String),
    RemoveShortcut(String),
    ForgetRecent(String),
    /// Type a shortcut into the program in front (generic menu commands).
    SendKey(crate::platform::KeyChord),
    /// Switch the input panel ("pocketpc" or "onboard").
    SetInputPanel(String),
    /// Onboard reported its visibility.
    OnboardVisible(bool),
    /// A desktop action of an application (e.g. Notes' new note).
    LaunchAction(String, String),
    /// Open a file with a given program (an event in Calendar, a task in
    /// Tasks, a note in Notes); an empty path just starts the program.
    OpenWith(String, String),
    /// An object in some program got the focus (AT-SPI).
    TextFocus(Focus),
}

pub type Dispatch = Rc<dyn Fn(Command)>;

/// A Start button press right after the menu closed because it lost focus
/// is the same tap; it must not reopen the menu.
const MENU_REOPEN_GUARD: Duration = Duration::from_millis(300);
const END_SESSION_TIMEOUT: Duration = Duration::from_secs(15);
/// A generic command's keys are typed once its menu has closed and the
/// program has the focus again.
const KEY_DELAY: Duration = Duration::from_millis(250);

/// Ids of the shell's native programs (desktop entries).
pub const NOTES_APP: &str = "pda-notes.desktop";
pub const FILES_APP: &str = "pda-files.desktop";
use crate::ui::today::{CALENDAR_APP, TASKS_APP};
const SPREADSHEET_APP: &str = "org.gnumeric.gnumeric.desktop";

/// The New menu on Today: what can be created with what is installed.
fn new_items(apps: &[AppEntry]) -> Vec<(String, Command)> {
    let has = |id: &str| apps.iter().any(|a| a.id == id);
    let mut items = Vec::new();
    if has(NOTES_APP) {
        items.push(("Note".to_string(), Command::LaunchAction(NOTES_APP.into(), "new-note".into())));
    }
    if has(CALENDAR_APP) {
        items.push(("Event".to_string(), Command::LaunchAction(CALENDAR_APP.into(), "new-event".into())));
    }
    if has(TASKS_APP) {
        items.push(("Task".to_string(), Command::LaunchAction(TASKS_APP.into(), "new-task".into())));
    }
    if has(SPREADSHEET_APP) {
        items.push(("Spreadsheet".to_string(), Command::Shell(Event::Launch(SPREADSHEET_APP.into()))));
    }
    if has(FILES_APP) {
        items.push(("Folder".to_string(), Command::LaunchAction(FILES_APP.into(), "new-folder".into())));
    }
    items
}

pub struct Shell {
    app: gtk::Application,
    layout: Layout,
    x11: Rc<X11>,
    state: RefCell<ShellState>,
    apps: RefCell<Vec<AppEntry>>,
    shortcuts: Rc<RefCell<Vec<String>>>,
    /// Programs started in this session, most recent first (Start menu).
    recent_apps: Rc<RefCell<Vec<String>>>,
    status: Rc<StatusBar>,
    command: Rc<CommandBar>,
    today: Rc<Today>,
    start: Rc<StartMenu>,
    escape: Rc<FullscreenEscape>,
    keyboard: RefCell<Keyboard>,
    /// The built-in input panel.
    sip: Rc<InputPanel>,
    /// Which input panel the keyboard button shows.
    panel: RefCell<String>,
    /// Keyboard on text focus ([keyboard] show_on_focus): the watch, the
    /// object that has the focus, and the field the user hid the panel in.
    focus_watch: RefCell<Option<FocusWatch>>,
    focus: RefCell<Option<Focus>>,
    dismissed_in: RefCell<Option<(String, String)>>,
    /// The panel is being shown because a field got the focus: the
    /// program already has it, so it is not activated again.
    auto_shown: Cell<bool>,
    /// Arrows for dialogs larger than the work area.
    pan: Rc<DialogPan>,
    menu_closed_at: Cell<Option<Instant>>,
    /// Kept alive so installs and removals keep being reported.
    app_monitor: RefCell<Option<gio::AppInfoMonitor>>,
    dispatch: Dispatch,
}

impl Shell {
    pub fn start(app: &gtk::Application, cfg: Config) -> Result<Rc<Self>, String> {
        let display = gtk::gdk::Display::default().ok_or("no display")?;
        let monitor = display
            .monitors()
            .item(0)
            .and_downcast::<gtk::gdk::Monitor>()
            .ok_or("no monitor")?;
        let geo = monitor.geometry();
        let layout = Layout::compute(geo.width() as u32, geo.height() as u32, &cfg.design);
        log::info!(
            "screen {}x{} {}; workarea {:?}",
            geo.width(),
            geo.height(),
            layout.orientation.name(),
            layout.workarea
        );

        let x11 = Rc::new(X11::connect().map_err(|e| format!("X11: {e}"))?);
        if let Err(e) = x11.publish_settings(cfg.design.app_dpi, &cfg.design.app_font) {
            log::error!("cannot publish application settings: {e}");
        }
        crate::ui::install_stylesheet(&cfg);

        let shortcuts = Rc::new(RefCell::new(shortcuts::load(&cfg.today.shortcuts)));
        let weak: Rc<RefCell<Weak<Shell>>> = Rc::new(RefCell::new(Weak::new()));
        let w = weak.clone();
        let dispatch: Dispatch = Rc::new(move |cmd| {
            if let Some(shell) = w.borrow().upgrade() {
                shell.dispatch(cmd);
            }
        });

        let status = StatusBar::new(app, &layout, &x11, &cfg, dispatch.clone());
        let command = CommandBar::new(app, &layout, &x11, &cfg, dispatch.clone());
        let today = Today::new(app, &layout, &x11, &cfg, dispatch.clone(), shortcuts.clone());
        let recent_apps: Rc<RefCell<Vec<String>>> = Rc::default();
        let start =
            StartMenu::new(app, &layout, &x11, &cfg, dispatch.clone(), shortcuts.clone(), recent_apps.clone());
        let escape = FullscreenEscape::new(app, &layout, &x11, &cfg, dispatch.clone());
        let d = dispatch.clone();
        let on_keyboard_visible: Rc<dyn Fn(bool)> = Rc::new(move |v| d(Command::OnboardVisible(v)));
        let keyboard = RefCell::new(Keyboard::new(cfg.keyboard.clone(), &layout, on_keyboard_visible));
        let placement = Placement::from_config(&cfg.keyboard.placement, layout.orientation);
        let sip = InputPanel::new(app, keyboard.borrow().rect(), layout.workarea, placement, &x11, &cfg);
        let pan = DialogPan::new(app, &layout, &x11, &cfg);
        let panel = match cfg.keyboard.panel.as_str() {
            p @ ("pocketpc" | "onboard") => p.to_string(),
            other => {
                log::warn!("unknown input panel {other:?}; using onboard");
                "onboard".into()
            }
        };
        command.set_input_panel(&panel);

        let shell = Rc::new(Self {
            app: app.clone(),
            layout,
            x11,
            state: RefCell::new(ShellState::with_fullscreen_mode(cfg.windows.fullscreen)),
            apps: RefCell::new(Vec::new()),
            shortcuts,
            recent_apps,
            status,
            command,
            today,
            start,
            escape,
            keyboard,
            sip,
            panel: RefCell::new(panel),
            focus_watch: RefCell::new(None),
            focus: RefCell::new(None),
            dismissed_in: RefCell::new(None),
            auto_shown: Cell::new(false),
            pan,
            menu_closed_at: Cell::new(None),
            app_monitor: RefCell::new(None),
            dispatch,
        });
        *weak.borrow_mut() = Rc::downgrade(&shell);
        if cfg.keyboard.show_on_focus {
            let d = shell.dispatch.clone();
            *shell.focus_watch.borrow_mut() = focus::watch(Rc::new(move |f| d(Command::TextFocus(f))));
        }

        // Openbox's focus-stealing prevention would leave keyboard focus in
        // the application under the Start menu; the user did ask for the
        // menu, so activate it explicitly (as a pager would).
        let w = Rc::downgrade(&shell);
        shell.start.win.window.connect_map(move |win| {
            if let (Some(shell), Some(xid)) = (w.upgrade(), gtk_glue::xid(win.upcast_ref())) {
                shell.x11.activate(WindowId(xid));
            }
        });

        shell.reload_apps();
        // Programs installed or removed while the shell runs.
        let w = Rc::downgrade(&shell);
        let monitor = gio::AppInfoMonitor::get();
        monitor.connect_changed(move |_| {
            if let Some(shell) = w.upgrade() {
                shell.reload_apps();
            }
        });
        *shell.app_monitor.borrow_mut() = Some(monitor);
        shell.today.refresh_recent();
        shell.today.win.window.present();
        shell.status.win.window.present();
        shell.command.win.window.present();
        shell.refresh_ui();

        let rx = x11::spawn_watcher(std::process::id(), cfg.windows.fit_dialogs)
            .map_err(|e| format!("window watcher: {e}"))?;
        let w = Rc::downgrade(&shell);
        glib::spawn_future_local(async move {
            while let Ok(snap) = rx.recv().await {
                match w.upgrade() {
                    Some(shell) => shell.dispatch(Command::Shell(Event::Windows(snap))),
                    None => break,
                }
            }
            log::warn!("window watcher stopped");
        });

        let w = Rc::downgrade(&shell);
        glib::timeout_add_local_once(Duration::from_millis(500), move || {
            if let Some(shell) = w.upgrade() {
                match shell.x11.workarea() {
                    Ok(Some(wa)) if wa == shell.layout.workarea => log::info!("window manager work area {wa:?} matches layout"),
                    Ok(Some(wa)) => log::warn!("window manager work area {wa:?} differs from layout {:?}", shell.layout.workarea),
                    Ok(None) => log::warn!("window manager does not publish _NET_WORKAREA"),
                    Err(e) => log::warn!("cannot read work area: {e}"),
                }
            }
        });
        Ok(shell)
    }

    fn reload_apps(&self) {
        let apps = apps::load_installed();
        log::info!("{} applications installed", apps.len());
        self.start.set_apps(apps.clone());
        self.today.set_apps(&apps);
        self.command.set_new_items(new_items(&apps));
        *self.apps.borrow_mut() = apps;
        self.refresh_shortcuts();
    }

    fn refresh_shortcuts(&self) {
        let apps = self.apps.borrow();
        let ids = self.shortcuts.borrow();
        self.today.set_shortcuts(&apps::resolve_shortcuts(&apps, &ids));
        self.start.refresh();
    }

    pub fn dispatch(&self, cmd: Command) {
        log::debug!("command {cmd:?}");
        match cmd {
            Command::Shell(Event::EndSessionTimeout) if self.state.borrow().ending_session() => {
                log::warn!("some programs did not close; staying in the session");
                self.handle(Event::EndSessionTimeout);
            }
            Command::Shell(Event::ToggleKeyboard) => {
                // Hidden by hand in a text field: not shown again for it.
                *self.dismissed_in.borrow_mut() = if self.state.borrow().keyboard_visible() {
                    self.focus.borrow().as_ref().filter(|f| f.editable).map(|f| f.object.clone())
                } else {
                    None
                };
                self.handle(Event::ToggleKeyboard);
            }
            Command::Shell(Event::Windows(snap)) => {
                self.pan.set(snap.overflow);
                self.handle(Event::Windows(snap));
            }
            Command::Shell(ev) => self.handle(ev),
            Command::TextFocus(f) => {
                let visible = self.state.borrow().keyboard_visible();
                let show = focus::decide(&f, visible, self.dismissed_in.borrow().as_ref());
                log::debug!("text focus {f:?}: show panel {show}");
                *self.focus.borrow_mut() = Some(f);
                if show {
                    self.auto_shown.set(true);
                    self.handle(Event::ToggleKeyboard);
                    self.auto_shown.set(false);
                }
            }
            Command::StartButton => {
                let guarded = !self.state.borrow().start_menu_open()
                    && self.menu_closed_at.get().is_some_and(|t| t.elapsed() < MENU_REOPEN_GUARD);
                if !guarded {
                    self.handle(Event::StartButton);
                }
            }
            Command::StartMenuFocusLost => {
                if self.state.borrow().start_menu_open() {
                    self.handle(Event::DismissStartMenu);
                }
            }
            Command::OpenUri(uri) => {
                // Notes open in Notes, which saves them by itself.
                let note = gio::File::for_uri(&uri)
                    .path()
                    .filter(|p| p.parent() == Some(crate::native::notes::notes_dir().as_path()))
                    .filter(|p| p.extension().is_some_and(|x| x == "txt"))
                    .filter(|_| self.apps.borrow().iter().any(|a| a.id == NOTES_APP));
                if let Some(path) = note {
                    self.dispatch(Command::OpenWith(NOTES_APP.into(), path.to_string_lossy().into_owned()));
                } else if let Err(e) = apps::open_uri(&uri) {
                    log::error!("cannot open {uri}: {e}");
                }
            }
            Command::OpenFolder(uri) => {
                if let Some(parent) = gio::File::for_uri(&uri).parent() {
                    if let Err(e) = apps::open_uri(&parent.uri()) {
                        log::error!("cannot open folder of {uri}: {e}");
                    }
                }
            }
            Command::SendKey(chord) => {
                // The program in front gets the keys; make sure it has the
                // focus once the menu that sent them has closed.
                self.refocus_app();
                let x11 = self.x11.clone();
                glib::timeout_add_local_once(KEY_DELAY, move || {
                    if let Err(e) = x11.send_key(chord) {
                        log::warn!("cannot type {chord:?}: {e}");
                    }
                });
            }
            Command::SetInputPanel(name) => self.set_input_panel(name),
            Command::OnboardVisible(v) => {
                // A report from Onboard after switching to the built-in panel
                // must not hide the panel now in use.
                if *self.panel.borrow() == "onboard" {
                    self.handle(Event::KeyboardVisible(v));
                }
            }
            Command::OpenWith(id, path) => {
                let result = if path.is_empty() {
                    apps::launch(&id)
                } else {
                    apps::launch_with(&id, std::path::Path::new(&path))
                };
                if let Err(e) = result {
                    log::error!("cannot open {path:?} with {id}: {e}");
                }
            }
            Command::LaunchAction(id, action) => {
                if let Err(e) = apps::launch_action(&id, &action) {
                    log::error!("cannot launch {id} {action}: {e}");
                }
            }
            Command::AddShortcut(id) => self.edit_shortcuts(|s| shortcuts::add(s, &id)),
            Command::RemoveShortcut(id) => self.edit_shortcuts(|s| shortcuts::remove(s, &id)),
            Command::ForgetRecent(uri) => {
                // Documents found in ~/Documents are not in the recent list.
                if let Err(e) = gtk::RecentManager::default().remove_item(&uri) {
                    log::debug!("{uri} is not a recent document: {e}");
                }
                self.today.forget(&uri);
            }
        }
    }

    /// Give the keyboard focus back to the program in front: a tap in the
    /// command bar's menus can leave it in the shell, and the input panel
    /// types into whatever has the focus.
    fn refocus_app(&self) {
        if let View::App(id) = self.state.borrow().view() {
            self.x11.activate(id);
        }
    }

    fn show_panel(&self, visible: bool) {
        if visible && !self.auto_shown.get() {
            self.refocus_app();
        }
        if *self.panel.borrow() == "pocketpc" {
            self.sip.set_visible(visible);
            // Shown at once, unlike Onboard which reports it over D-Bus.
            let d = self.dispatch.clone();
            glib::idle_add_local_once(move || d(Command::Shell(Event::KeyboardVisible(visible))));
        } else {
            self.keyboard.borrow_mut().set_visible(visible);
        }
    }

    fn set_input_panel(&self, name: String) {
        if *self.panel.borrow() == name || !crate::keyboard::PANELS.iter().any(|(_, n)| *n == name) {
            return;
        }
        let visible = self.state.borrow().keyboard_visible();
        if visible {
            self.show_panel(false);
        }
        log::info!("input panel: {name}");
        *self.panel.borrow_mut() = name.clone();
        self.command.set_input_panel(&name);
        // Both panels use the same reserved space; the new one takes over.
        // A floating panel reserves none, Onboard always does.
        if visible {
            self.show_panel(true);
            self.reserve_keyboard(true);
        } else {
            self.refocus_app();
        }
    }

    /// Keep programs (and Today, the Start menu) clear of a docked
    /// keyboard; a floating panel covers them instead.
    fn reserve_keyboard(&self, visible: bool) {
        let floating = *self.panel.borrow() == "pocketpc" && self.sip.placement() == Placement::Floating;
        let extra = if visible && !floating { self.keyboard.borrow().rect().height } else { 0 };
        let shrink = |r: layout::Rect| layout::Rect { height: r.height.saturating_sub(extra), ..r };
        self.today.win.set_rect(shrink(self.layout.workarea));
        self.start.set_available(shrink(self.layout.start_menu));
        if let Some(xid) = gtk_glue::xid(self.command.win.window.upcast_ref()) {
            if let Err(e) = self.x11.reserve(xid, SurfaceRole::BottomPanel, self.layout.bottom_bar, extra) {
                log::warn!("cannot reserve keyboard space: {e}");
            }
        }
    }

    fn edit_shortcuts(&self, f: impl FnOnce(&mut Vec<String>)) {
        f(&mut self.shortcuts.borrow_mut());
        if let Err(e) = shortcuts::save(&self.shortcuts.borrow()) {
            log::error!("cannot save Today shortcuts: {e}");
        }
        self.refresh_shortcuts();
    }

    fn handle(&self, ev: Event) {
        let was_today = self.state.borrow().view() == View::Today;
        let effects = self.state.borrow_mut().handle(ev);
        for fx in effects {
            log::debug!("effect {fx:?}");
            self.run(fx);
        }
        // Documents saved while a program was in front show up on return.
        if !was_today && self.state.borrow().view() == View::Today {
            self.today.refresh_recent();
        }
        self.refresh_ui();
    }

    fn run(&self, fx: Effect) {
        match fx {
            Effect::ShowDesktop(on) => self.x11.show_desktop(on),
            Effect::Activate(id) => self.x11.activate(id),
            Effect::Close(id) => self.x11.close(id),
            Effect::SetFullscreen(id, on) => self.x11.set_fullscreen(id, on),
            Effect::ShowStartMenu(true) => self.start.show(),
            Effect::ShowStartMenu(false) => {
                self.menu_closed_at.set(Some(Instant::now()));
                self.start.hide();
            }
            Effect::Launch(id) => match apps::launch(&id) {
                Ok(()) => shortcuts::add_recent(&mut self.recent_apps.borrow_mut(), &id),
                Err(e) => log::error!("cannot launch {id}: {e}"),
            },
            Effect::ShowPage(page) => self.today.show_page(page),
            Effect::SetKeyboard(visible) => self.show_panel(visible),
            // Today and the Start menu shrink like applications do, so a
            // docked keyboard never covers their lower part (End session).
            Effect::ReserveKeyboard(visible) => self.reserve_keyboard(visible),
            Effect::StartEndSessionTimer => {
                let d = self.dispatch.clone();
                glib::timeout_add_local_once(END_SESSION_TIMEOUT, move || {
                    d(Command::Shell(Event::EndSessionTimeout));
                });
            }
            Effect::Quit => {
                log::info!("all programs closed; ending session");
                self.keyboard.borrow_mut().stop();
                self.sip.set_visible(false);
                self.app.quit();
            }
        }
    }

    fn refresh_ui(&self) {
        // A window that appeared or was raised must not cover the panel
        // or the dialog arrows.
        self.sip.raise();
        self.pan.raise();
        let st = self.state.borrow();
        self.status.set_title(&st.title());
        self.status.set_start_open(st.start_menu_open());
        self.command.set_context_actions(&st.context_actions());
        self.command.set_keyboard_visible(st.keyboard_visible());
        self.escape.set_visible(st.fullscreen_escape());
        let active = match st.view() {
            View::App(id) => Some(id),
            View::Today => None,
        };
        if let Some(w) = active.and_then(|id| st.window(id)) {
            self.command.set_app(w.id, w.menus.as_ref());
        }
        let apps = self.apps.borrow();
        let entries: Vec<SwitcherEntry> = st
            .switcher_entries()
            .into_iter()
            .map(|w| {
                let entry = apps::for_window_class(&apps, &w.app_class);
                SwitcherEntry {
                    id: w.id,
                    title: w.display_title().to_string(),
                    program: entry.map_or_else(|| w.app_class.clone(), |e| e.name.clone()),
                    icon: entry.and_then(apps::gicon),
                    active: Some(w.id) == active,
                }
            })
            .collect();
        self.today.set_running(&entries);
        self.command.set_windows(entries);
    }
}

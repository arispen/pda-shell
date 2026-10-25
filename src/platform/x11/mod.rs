//! X11 / EWMH implementation of the platform layer.
//!
//! Two connections are used: one owned by the GTK main thread for requests
//! (window roles, activation, closing) and one owned by a watcher thread that
//! blocks on events and publishes [`WindowSnapshot`]s. The watcher also keeps
//! dialogs inside the work area and logs windows that cannot fit it
//! (`geometry:` lines in the shell log).

pub mod gtk_glue;
pub mod keys;
pub mod settings;

use super::{AppWindow, KeyChord, SurfaceRole, WindowId, WindowSnapshot, WindowSystem};
use crate::shell::layout::{fit_within, Rect};
use std::collections::{HashMap, HashSet};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    AtomEnum, ChangeWindowAttributesAux, ClientMessageEvent, ConfigureWindowAux, ConnectionExt as _,
    CreateWindowAux, EventMask, PropMode, StackMode, Window, WindowClass,
};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::CURRENT_TIME;

x11rb::atom_manager! {
    pub Atoms: AtomsCookie {
        UTF8_STRING,
        WM_TRANSIENT_FOR,
        _NET_CLIENT_LIST,
        _NET_ACTIVE_WINDOW,
        _NET_SHOWING_DESKTOP,
        _NET_WORKAREA,
        _NET_CLOSE_WINDOW,
        _NET_WM_NAME,
        _NET_WM_PID,
        _NET_WM_DESKTOP,
        _NET_WM_STRUT,
        _NET_WM_STRUT_PARTIAL,
        _NET_WM_WINDOW_TYPE,
        _NET_WM_WINDOW_TYPE_NORMAL,
        _NET_WM_WINDOW_TYPE_DIALOG,
        _NET_WM_WINDOW_TYPE_DOCK,
        _NET_WM_WINDOW_TYPE_DESKTOP,
        _NET_WM_WINDOW_TYPE_UTILITY,
        _NET_WM_STATE,
        _NET_WM_STATE_SKIP_TASKBAR,
        _NET_WM_STATE_SKIP_PAGER,
        _NET_WM_STATE_STICKY,
        _NET_WM_STATE_ABOVE,
        _NET_WM_STATE_FULLSCREEN,
        _NET_FRAME_EXTENTS,
        _NET_MOVERESIZE_WINDOW,
        _XSETTINGS_SETTINGS,
        MANAGER,
        _GTK_UNIQUE_BUS_NAME,
        _GTK_MENUBAR_OBJECT_PATH,
        _GTK_APPLICATION_OBJECT_PATH,
        _GTK_WINDOW_OBJECT_PATH,
    }
}

/// WM_CLASS class names that belong to the shell or its helpers and must
/// never appear as switchable applications.
const SHELL_CLASSES: &[&str] = &["Pda-shell", "pda-shell", "Onboard", "onboard"];

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub struct X11 {
    conn: RustConnection,
    screen: usize,
    root: Window,
    atoms: Atoms,
    /// Keyboard mapping for typing, read on first use.
    keymap: std::cell::RefCell<Option<keys::Keymap>>,
}

impl X11 {
    pub fn connect() -> Result<Self> {
        let (conn, screen) = RustConnection::connect(None)?;
        let root = conn.setup().roots[screen].root;
        let atoms = Atoms::new(&conn)?.reply()?;
        Ok(Self { conn, screen, root, atoms, keymap: Default::default() })
    }

    fn send_root_message(&self, window: Window, kind: u32, data: [u32; 5]) -> Result<()> {
        let event = ClientMessageEvent::new(32, window, kind, data);
        self.conn.send_event(
            false,
            self.root,
            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
            event,
        )?;
        self.conn.flush()?;
        Ok(())
    }

    /// Give one of the shell's own (not yet mapped) windows its role.
    pub fn apply_role(&self, window: u32, role: SurfaceRole, rect: Rect) -> Result<()> {
        let a = &self.atoms;
        if role == SurfaceRole::Escape {
            // Not managed by the window manager at all, so it can sit above
            // full-screen windows, which Openbox stacks above every layer.
            let aux = ChangeWindowAttributesAux::new().override_redirect(1);
            self.conn.change_window_attributes(window, &aux)?;
            return self.place(window, rect);
        }
        let (kind, states): (u32, Vec<u32>) = match role {
            SurfaceRole::TopPanel | SurfaceRole::BottomPanel => (
                a._NET_WM_WINDOW_TYPE_DOCK,
                vec![a._NET_WM_STATE_STICKY, a._NET_WM_STATE_SKIP_TASKBAR, a._NET_WM_STATE_SKIP_PAGER],
            ),
            SurfaceRole::Desktop => (
                a._NET_WM_WINDOW_TYPE_DESKTOP,
                vec![a._NET_WM_STATE_STICKY, a._NET_WM_STATE_SKIP_TASKBAR, a._NET_WM_STATE_SKIP_PAGER],
            ),
            SurfaceRole::Overlay | SurfaceRole::Escape => (
                a._NET_WM_WINDOW_TYPE_UTILITY,
                vec![a._NET_WM_STATE_ABOVE, a._NET_WM_STATE_SKIP_TASKBAR, a._NET_WM_STATE_SKIP_PAGER],
            ),
        };
        self.conn.change_property32(PropMode::REPLACE, window, a._NET_WM_WINDOW_TYPE, AtomEnum::ATOM, &[kind])?;
        self.conn.change_property32(PropMode::REPLACE, window, a._NET_WM_STATE, AtomEnum::ATOM, &states)?;
        self.conn.change_property32(PropMode::REPLACE, window, a._NET_WM_DESKTOP, AtomEnum::CARDINAL, &[0xFFFF_FFFF])?;

        if let Some((strut, partial)) = struts(role, rect) {
            self.conn.change_property32(PropMode::REPLACE, window, a._NET_WM_STRUT, AtomEnum::CARDINAL, &strut)?;
            self.conn
                .change_property32(PropMode::REPLACE, window, a._NET_WM_STRUT_PARTIAL, AtomEnum::CARDINAL, &partial)?;
        }
        self.place(window, rect)
    }

    /// Change how much of the screen edge a panel reserves, e.g. to keep
    /// applications clear of the on-screen keyboard above the command bar.
    pub fn reserve(&self, window: u32, role: SurfaceRole, rect: Rect, extra: u32) -> Result<()> {
        let grown = Rect { height: rect.height + extra, ..rect };
        if let Some((strut, partial)) = struts(role, grown) {
            let a = &self.atoms;
            self.conn.change_property32(PropMode::REPLACE, window, a._NET_WM_STRUT, AtomEnum::CARDINAL, &strut)?;
            self.conn
                .change_property32(PropMode::REPLACE, window, a._NET_WM_STRUT_PARTIAL, AtomEnum::CARDINAL, &partial)?;
            self.conn.flush()?;
        }
        Ok(())
    }

    /// Move and resize one of the shell's own windows.
    pub fn place(&self, window: u32, rect: Rect) -> Result<()> {
        let aux = ConfigureWindowAux::new().x(rect.x).y(rect.y).width(rect.width).height(rect.height);
        self.conn.configure_window(window, &aux)?;
        self.conn.flush()?;
        Ok(())
    }

    /// Put one of the shell's unmanaged windows on top of the stack.
    pub fn raise(&self, window: u32) -> Result<()> {
        self.conn.configure_window(window, &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE))?;
        self.conn.flush()?;
        Ok(())
    }

    /// Publish font resolution and default font to every program started
    /// afterwards (see [`settings`]). The shell owns the XSETTINGS selection
    /// for as long as it runs.
    pub fn publish_settings(&self, dpi: u32, font: &str) -> Result<()> {
        let selection = self.conn.intern_atom(false, format!("_XSETTINGS_S{}", self.screen).as_bytes())?.reply()?.atom;
        let previous = self.conn.get_selection_owner(selection)?.reply()?.owner;
        if previous != x11rb::NONE {
            log::warn!("replacing the XSETTINGS manager {previous:#x}");
        }
        let owner = self.conn.generate_id()?;
        self.conn.create_window(
            0,
            owner,
            self.root,
            -1,
            -1,
            1,
            1,
            0,
            WindowClass::INPUT_ONLY,
            0,
            &CreateWindowAux::new(),
        )?;
        let blob = settings::xsettings_blob(0, &settings::session_settings(dpi, font));
        let a = &self.atoms;
        self.conn.change_property8(PropMode::REPLACE, owner, a._XSETTINGS_SETTINGS, a._XSETTINGS_SETTINGS, &blob)?;
        self.conn.set_selection_owner(owner, selection, CURRENT_TIME)?;
        let manager = ClientMessageEvent::new(32, self.root, a.MANAGER, [CURRENT_TIME, selection, owner, 0, 0]);
        self.conn.send_event(false, self.root, EventMask::STRUCTURE_NOTIFY, manager)?;

        // Toolkits without XSETTINGS (Xlib, Qt) read the Xft.dpi resource.
        let existing = self
            .conn
            .get_property(false, self.root, AtomEnum::RESOURCE_MANAGER, AtomEnum::STRING, 0, 1 << 16)?
            .reply()?;
        let merged = settings::merge_resources(&String::from_utf8_lossy(&existing.value), &[("Xft.dpi", dpi.to_string())]);
        self.conn.change_property8(
            PropMode::REPLACE,
            self.root,
            AtomEnum::RESOURCE_MANAGER,
            AtomEnum::STRING,
            merged.as_bytes(),
        )?;
        self.conn.flush()?;
        log::info!("published application settings: {dpi} dpi, font {font:?}");
        Ok(())
    }

    /// Type a key (with modifiers) into whatever window has the keyboard
    /// focus, as if pressed on a hardware keyboard (XTEST).
    pub fn send_key(&self, chord: KeyChord) -> Result<()> {
        use x11rb::protocol::xtest::ConnectionExt as _;
        let mut map = self.keymap.borrow_mut();
        if map.is_none() {
            let setup = self.conn.setup();
            let (min, max) = (setup.min_keycode, setup.max_keycode);
            let reply = self.conn.get_keyboard_mapping(min, max - min + 1)?.reply()?;
            *map = Some(keys::Keymap {
                min_keycode: min,
                per_keycode: reply.keysyms_per_keycode as usize,
                syms: reply.keysyms,
            });
        }
        let map = map.as_mut().expect("keymap");
        let sym = keys::keysym(chord.key);
        let (code, level) = match map.find(sym) {
            Some(found) => found,
            None => {
                // Not on the keyboard (e.g. a symbol of another language):
                // put it on an unused keycode, like xdotool does.
                let code = map.spare().ok_or("no spare keycode for the symbol")?;
                let syms = vec![sym; map.per_keycode];
                self.conn.change_keyboard_mapping(1, code, map.per_keycode as u8, &syms)?.check()?;
                map.set(code, sym);
                (code, 0)
            }
        };
        let modifier = |sym: u32| map.find(sym).map(|(c, _)| c);
        let mut mods = Vec::new();
        if chord.ctrl {
            mods.extend(modifier(keys::CONTROL_L));
        }
        if chord.alt {
            mods.extend(modifier(keys::ALT_L));
        }
        if chord.shift || level == 1 {
            mods.extend(modifier(keys::SHIFT_L));
        }
        const PRESS: u8 = 2;
        const RELEASE: u8 = 3;
        for m in &mods {
            self.conn.xtest_fake_input(PRESS, *m, CURRENT_TIME, self.root, 0, 0, 0)?;
        }
        self.conn.xtest_fake_input(PRESS, code, CURRENT_TIME, self.root, 0, 0, 0)?;
        self.conn.xtest_fake_input(RELEASE, code, CURRENT_TIME, self.root, 0, 0, 0)?;
        for m in mods.iter().rev() {
            self.conn.xtest_fake_input(RELEASE, *m, CURRENT_TIME, self.root, 0, 0, 0)?;
        }
        self.conn.flush()?;
        Ok(())
    }

    /// The work area the window manager computed from all struts.
    pub fn workarea(&self) -> Result<Option<Rect>> {
        let reply = self
            .conn
            .get_property(false, self.root, self.atoms._NET_WORKAREA, AtomEnum::CARDINAL, 0, 4)?
            .reply()?;
        let v: Vec<u32> = reply.value32().map(|i| i.collect()).unwrap_or_default();
        Ok((v.len() == 4).then(|| Rect::new(v[0] as i32, v[1] as i32, v[2], v[3])))
    }
}

/// `_NET_WM_STRUT` and `_NET_WM_STRUT_PARTIAL` values for a panel.
fn struts(role: SurfaceRole, r: Rect) -> Option<([u32; 4], [u32; 12])> {
    let x0 = r.x.max(0) as u32;
    let x1 = x0 + r.width.saturating_sub(1);
    match role {
        SurfaceRole::TopPanel => Some(([0, 0, r.height, 0], [0, 0, r.height, 0, 0, 0, 0, 0, x0, x1, 0, 0])),
        SurfaceRole::BottomPanel => Some(([0, 0, 0, r.height], [0, 0, 0, r.height, 0, 0, 0, 0, 0, 0, x0, x1])),
        _ => None,
    }
}

impl WindowSystem for X11 {
    fn activate(&self, id: WindowId) {
        // Source indication 2 = pager: the request comes from the user.
        let r = self.send_root_message(id.0, self.atoms._NET_ACTIVE_WINDOW, [2, CURRENT_TIME, 0, 0, 0]);
        if let Err(e) = r {
            log::warn!("activate {:#x}: {e}", id.0);
        }
    }

    fn close(&self, id: WindowId) {
        // _NET_CLOSE_WINDOW lets the application ask about unsaved work.
        let r = self.send_root_message(id.0, self.atoms._NET_CLOSE_WINDOW, [CURRENT_TIME, 2, 0, 0, 0]);
        if let Err(e) = r {
            log::warn!("close {:#x}: {e}", id.0);
        }
    }

    fn show_desktop(&self, on: bool) {
        let r = self.send_root_message(self.root, self.atoms._NET_SHOWING_DESKTOP, [on as u32, 0, 0, 0, 0]);
        if let Err(e) = r {
            log::warn!("show desktop: {e}");
        }
    }

    fn set_fullscreen(&self, id: WindowId, on: bool) {
        // _NET_WM_STATE: action (0 remove, 1 add), property, -, source (2 = pager).
        let a = &self.atoms;
        let r = self.send_root_message(id.0, a._NET_WM_STATE, [on as u32, a._NET_WM_STATE_FULLSCREEN, 0, 2, 0]);
        if let Err(e) = r {
            log::warn!("fullscreen {:#x}: {e}", id.0);
        }
    }
}

/// Raw facts about one managed window.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClientInfo {
    pub id: u32,
    pub title: String,
    pub class: String,
    pub pid: Option<u32>,
    pub types: Vec<WindowType>,
    pub skip_taskbar: bool,
    pub fullscreen: bool,
    pub transient_for: Option<u32>,
    pub menus: Option<super::AppMenus>,
}

impl ClientInfo {
    fn shell_owned(&self, own_pid: u32) -> bool {
        self.pid == Some(own_pid) || SHELL_CLASSES.contains(&self.class.as_str())
    }

    /// Dialogs and other windows that belong to a parent window.
    pub fn is_dialog(&self) -> bool {
        self.types.first() == Some(&WindowType::Dialog) || self.transient_for.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowType {
    Normal,
    Dialog,
    Utility,
    Dock,
    Desktop,
    Other,
}

/// Whether a managed window should appear in the switcher.
pub fn is_switchable(c: &ClientInfo, own_pid: u32) -> bool {
    if c.skip_taskbar || c.shell_owned(own_pid) {
        return false;
    }
    match c.types.first() {
        None | Some(WindowType::Normal) => true,
        // Free-standing dialogs act as windows of their own; transient ones
        // belong to their parent.
        Some(WindowType::Dialog) => c.transient_for.is_none(),
        Some(_) => false,
    }
}

/// Pick the application window the user is looking at.
///
/// `active` is `_NET_ACTIVE_WINDOW`. If it is a transient dialog, its parent
/// counts. If it is a shell window (Start menu) or the on-screen keyboard,
/// the previously active application stays current. The desktop or no
/// window means no application is in front.
pub fn resolve_active(
    clients: &[ClientInfo],
    apps: &[AppWindow],
    active: Option<u32>,
    previous: Option<WindowId>,
    own_pid: u32,
) -> Option<WindowId> {
    let is_app = |id: u32| apps.iter().any(|a| a.id.0 == id);
    let id = active?;
    if is_app(id) {
        return Some(WindowId(id));
    }
    // Not managed (yet): e.g. the Start menu just mapped and activated before
    // the window manager lists it. The program in front has not changed.
    let Some(client) = clients.iter().find(|c| c.id == id) else {
        return previous.filter(|p| is_app(p.0));
    };
    if let Some(parent) = client.transient_for.filter(|p| is_app(*p)) {
        return Some(WindowId(parent));
    }
    let shell_owned = client.shell_owned(own_pid);
    let desktop = client.types.first() == Some(&WindowType::Desktop);
    if shell_owned && !desktop {
        return previous.filter(|p| is_app(p.0));
    }
    None
}

/// `WM_NORMAL_HINTS` minimum size, if the window sets one (`PMinSize`).
pub fn min_size(hints: &[u32]) -> Option<(u32, u32)> {
    const P_MIN_SIZE: u32 = 1 << 4;
    match hints {
        [flags, _, _, _, _, w, h, ..] if flags & P_MIN_SIZE != 0 => Some((*w, *h)),
        _ => None,
    }
}

struct Watcher {
    x: X11,
    own_pid: u32,
    fit_dialogs: bool,
    watched: HashSet<u32>,
    /// Dialogs, the work area they were last checked against, and their
    /// size then (a dialog that resizes itself is checked again; one the
    /// user merely moves is left where the user put it).
    dialogs: HashMap<u32, (Option<Rect>, (u32, u32))>,
    /// Windows already reported as not fitting, so the log stays readable.
    reported: HashSet<u32>,
    last_active: Option<WindowId>,
}

impl Watcher {
    fn prop32(&self, window: u32, atom: u32, kind: impl Into<u32>) -> Vec<u32> {
        self.x
            .conn
            .get_property(false, window, atom, kind.into(), 0, 1024)
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|r| r.value32().map(|i| i.collect()))
            .unwrap_or_default()
    }

    fn prop_bytes(&self, window: u32, atom: u32, kind: impl Into<u32>) -> Vec<u8> {
        self.x
            .conn
            .get_property(false, window, atom, kind.into(), 0, 1024)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| r.value)
            .unwrap_or_default()
    }

    fn client(&self, id: u32) -> ClientInfo {
        let a = &self.x.atoms;
        let mut title = String::from_utf8_lossy(&self.prop_bytes(id, a._NET_WM_NAME, a.UTF8_STRING)).into_owned();
        if title.is_empty() {
            title = String::from_utf8_lossy(&self.prop_bytes(id, AtomEnum::WM_NAME.into(), AtomEnum::STRING))
                .into_owned();
        }
        // WM_CLASS is "instance\0class\0".
        let class_raw = self.prop_bytes(id, AtomEnum::WM_CLASS.into(), AtomEnum::STRING);
        let class = class_raw
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .nth(1)
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .unwrap_or_default();
        let types = self
            .prop32(id, a._NET_WM_WINDOW_TYPE, AtomEnum::ATOM)
            .into_iter()
            .map(|t| match t {
                t if t == a._NET_WM_WINDOW_TYPE_NORMAL => WindowType::Normal,
                t if t == a._NET_WM_WINDOW_TYPE_DIALOG => WindowType::Dialog,
                t if t == a._NET_WM_WINDOW_TYPE_UTILITY => WindowType::Utility,
                t if t == a._NET_WM_WINDOW_TYPE_DOCK => WindowType::Dock,
                t if t == a._NET_WM_WINDOW_TYPE_DESKTOP => WindowType::Desktop,
                _ => WindowType::Other,
            })
            .collect();
        let states = self.prop32(id, a._NET_WM_STATE, AtomEnum::ATOM);
        ClientInfo {
            id,
            title,
            class,
            pid: self.prop32(id, a._NET_WM_PID, AtomEnum::CARDINAL).first().copied(),
            types,
            skip_taskbar: states.contains(&a._NET_WM_STATE_SKIP_TASKBAR),
            fullscreen: states.contains(&a._NET_WM_STATE_FULLSCREEN),
            transient_for: self
                .prop32(id, a.WM_TRANSIENT_FOR, AtomEnum::WINDOW)
                .first()
                .copied()
                .filter(|w| *w != 0),
            menus: {
                let text = |atom| String::from_utf8_lossy(&self.prop_bytes(id, atom, a.UTF8_STRING)).into_owned();
                super::AppMenus::from_properties(
                    &text(a._GTK_UNIQUE_BUS_NAME),
                    &text(a._GTK_MENUBAR_OBJECT_PATH),
                    &text(a._GTK_APPLICATION_OBJECT_PATH),
                    &text(a._GTK_WINDOW_OBJECT_PATH),
                )
            },
        }
    }

    fn snapshot(&mut self) -> WindowSnapshot {
        let a = &self.x.atoms;
        let ids = self.prop32(self.x.root, a._NET_CLIENT_LIST, AtomEnum::WINDOW);
        let clients: Vec<ClientInfo> = ids.iter().map(|id| self.client(*id)).collect();

        // Follow title and size changes of windows we have not seen before.
        for id in &ids {
            if self.watched.insert(*id) {
                let mask = EventMask::PROPERTY_CHANGE | EventMask::STRUCTURE_NOTIFY;
                let _ = self.x.conn.change_window_attributes(*id, &ChangeWindowAttributesAux::new().event_mask(mask));
            }
        }
        self.watched.retain(|w| ids.contains(w));
        self.dialogs.retain(|w, _| ids.contains(w));
        self.reported.retain(|w| ids.contains(w));
        self.check_geometry(&clients);
        let a = &self.x.atoms;

        let windows: Vec<AppWindow> = clients
            .iter()
            .filter(|c| is_switchable(c, self.own_pid))
            .map(|c| AppWindow {
                id: WindowId(c.id),
                title: c.title.clone(),
                app_class: c.class.clone(),
                pid: c.pid,
                fullscreen: c.fullscreen,
                menus: c.menus.clone(),
            })
            .collect();
        let active_raw =
            self.prop32(self.x.root, a._NET_ACTIVE_WINDOW, AtomEnum::WINDOW).first().copied().filter(|w| *w != 0);
        let showing_desktop =
            self.prop32(self.x.root, a._NET_SHOWING_DESKTOP, AtomEnum::CARDINAL).first() == Some(&1);
        let active = resolve_active(&clients, &windows, active_raw, self.last_active, self.own_pid);
        self.last_active = active;
        let _ = self.x.conn.flush();
        WindowSnapshot { windows, active, showing_desktop }
    }

    /// Outer (frame) rectangle of a managed window in root coordinates.
    fn frame_rect(&self, id: u32) -> Option<(Rect, [u32; 4])> {
        let geo = self.x.conn.get_geometry(id).ok()?.reply().ok()?;
        let pos = self.x.conn.translate_coordinates(id, self.x.root, 0, 0).ok()?.reply().ok()?;
        let e = self.prop32(id, self.x.atoms._NET_FRAME_EXTENTS, AtomEnum::CARDINAL);
        let [l, r, t, b] = <[u32; 4]>::try_from(e.as_slice()).unwrap_or([0; 4]);
        let outer = Rect::new(
            pos.dst_x as i32 - l as i32,
            pos.dst_y as i32 - t as i32,
            geo.width as u32 + l + r,
            geo.height as u32 + t + b,
        );
        Some((outer, [l, r, t, b]))
    }

    /// Keep dialogs inside the work area (the keyboard shrinks it) and report
    /// windows whose minimum size is larger than the work area.
    fn check_geometry(&mut self, clients: &[ClientInfo]) {
        let Some(wa) = self.x.workarea().ok().flatten() else { return };
        for c in clients.iter().filter(|c| !c.shell_owned(self.own_pid) && !c.fullscreen) {
            let hints = self.prop32(c.id, AtomEnum::WM_NORMAL_HINTS.into(), AtomEnum::WM_SIZE_HINTS);
            if let Some((w, h)) = min_size(&hints).filter(|(w, h)| *w > wa.width || *h > wa.height) {
                if self.reported.insert(c.id) {
                    log::warn!(
                        "geometry: {} {:?} needs at least {w}x{h}, more than the {}x{} work area",
                        c.class, c.title, wa.width, wa.height
                    );
                }
            }
            if !c.is_dialog() || self.dialogs.get(&c.id).is_some_and(|(checked, _)| *checked == Some(wa)) {
                continue;
            }
            let Some((outer, [l, r, t, b])) = self.frame_rect(c.id) else { continue };
            let client_size = (outer.width - l - r, outer.height - t - b);
            self.dialogs.insert(c.id, (Some(wa), client_size));
            let min = min_size(&hints).map_or((0, 0), |(w, h)| (w + l + r, h + t + b));
            let Some(fit) = fit_within(outer, wa, min).filter(|_| self.fit_dialogs) else {
                if !wa.contains(&outer) {
                    log::warn!("geometry: dialog {:?} ({}) outside the work area: {outer:?}", c.title, c.class);
                }
                continue;
            };
            if !wa.contains(&fit) {
                log::warn!(
                    "geometry: dialog {:?} ({}) cannot shrink to the work area; aligned to keep its buttons visible",
                    c.title, c.class
                );
            }
            log::info!("geometry: fitting dialog {:?} ({}) from {outer:?} into {fit:?}", c.title, c.class);
            // Gravity NorthWest (1), x/y/width/height present, source = pager.
            let flags = 1 | 0xF00 | (2 << 12);
            let w = fit.width.saturating_sub(l + r).max(1);
            let h = fit.height.saturating_sub(t + b).max(1);
            if let Err(e) =
                self.x.send_root_message(c.id, self.x.atoms._NET_MOVERESIZE_WINDOW, [flags, fit.x as u32, fit.y as u32, w, h])
            {
                log::warn!("cannot move dialog {:#x}: {e}", c.id);
            }
        }
    }

    fn relevant(&mut self, event: &Event) -> bool {
        let a = &self.x.atoms;
        match event {
            Event::PropertyNotify(e) if e.window == self.x.root => [
                a._NET_CLIENT_LIST,
                a._NET_ACTIVE_WINDOW,
                a._NET_SHOWING_DESKTOP,
                a._NET_WORKAREA,
            ]
            .contains(&e.atom),
            Event::PropertyNotify(e) => {
                [
                    a._NET_WM_NAME,
                    AtomEnum::WM_NAME.into(),
                    a._NET_WM_STATE,
                    a._NET_WM_WINDOW_TYPE,
                    a._GTK_MENUBAR_OBJECT_PATH,
                ]
                .contains(&e.atom)
            }
            // A dialog that resizes itself after mapping is checked again.
            Event::ConfigureNotify(e) => match self.dialogs.get_mut(&e.window) {
                Some(entry) if entry.1 != (e.width as u32, e.height as u32) => {
                    entry.0 = None;
                    true
                }
                _ => false,
            },
            _ => false,
        }
    }

    fn run(mut self, tx: async_channel::Sender<WindowSnapshot>) {
        let aux = ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE);
        if let Err(e) = self.x.conn.change_window_attributes(self.x.root, &aux).map(|_| self.x.conn.flush()) {
            log::error!("cannot watch root window: {e}");
            return;
        }
        let mut last = None;
        loop {
            let snap = self.snapshot();
            if last.as_ref() != Some(&snap) {
                log::debug!(
                    "windows: {} apps, active {:?}, showing desktop {}",
                    snap.windows.len(),
                    snap.active,
                    snap.showing_desktop
                );
                if tx.send_blocking(snap.clone()).is_err() {
                    return;
                }
                last = Some(snap);
            }
            // Block for the next relevant event, then coalesce any burst.
            loop {
                match self.x.conn.wait_for_event() {
                    Ok(ev) if self.relevant(&ev) => break,
                    Ok(_) => continue,
                    Err(e) => {
                        log::error!("X connection lost: {e}");
                        return;
                    }
                }
            }
            while let Ok(Some(_)) = self.x.conn.poll_for_event() {}
        }
    }
}

/// Start the window watcher thread.
pub fn spawn_watcher(own_pid: u32, fit_dialogs: bool) -> Result<async_channel::Receiver<WindowSnapshot>> {
    let x = X11::connect()?;
    let (tx, rx) = async_channel::unbounded();
    std::thread::Builder::new().name("x11-watcher".into()).spawn(move || {
        Watcher {
            x,
            own_pid,
            fit_dialogs,
            watched: HashSet::new(),
            dialogs: HashMap::new(),
            reported: HashSet::new(),
            last_active: None,
        }
        .run(tx)
    })?;
    Ok(rx)
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWN: u32 = 100;

    fn client(id: u32, class: &str, types: &[WindowType]) -> ClientInfo {
        ClientInfo { id, class: class.into(), pid: Some(id + 1000), types: types.to_vec(), ..Default::default() }
    }

    fn app(c: &ClientInfo) -> AppWindow {
        AppWindow {
            id: WindowId(c.id),
            title: c.title.clone(),
            app_class: c.class.clone(),
            pid: c.pid,
            fullscreen: c.fullscreen,
            menus: None,
        }
    }

    #[test]
    fn minimum_size_hint() {
        // flags, x, y, w, h, min_w, min_h, ...
        assert_eq!(min_size(&[16, 0, 0, 0, 0, 640, 480, 0, 0]), Some((640, 480)));
        assert_eq!(min_size(&[8, 0, 0, 0, 0, 640, 480, 0, 0]), None, "PMinSize not set");
        assert_eq!(min_size(&[16, 0]), None);
        assert_eq!(min_size(&[]), None);
    }

    #[test]
    fn dialogs_are_dialog_typed_or_transient() {
        assert!(client(1, "Gnumeric", &[WindowType::Dialog]).is_dialog());
        let mut t = client(2, "Mousepad", &[WindowType::Normal]);
        assert!(!t.is_dialog());
        t.transient_for = Some(1);
        assert!(t.is_dialog());
    }

    #[test]
    fn struts_reserve_the_right_edge() {
        let top = Rect::new(0, 0, 600, 24);
        let (s, p) = struts(SurfaceRole::TopPanel, top).unwrap();
        assert_eq!(s, [0, 0, 24, 0]);
        assert_eq!(&p[8..10], &[0, 599]);
        let bottom = Rect::new(0, 998, 600, 26);
        let (s, p) = struts(SurfaceRole::BottomPanel, bottom).unwrap();
        assert_eq!(s, [0, 0, 0, 26]);
        assert_eq!(&p[10..12], &[0, 599]);
        assert!(struts(SurfaceRole::Desktop, top).is_none());
    }

    #[test]
    fn switchable_windows() {
        assert!(is_switchable(&client(1, "Mousepad", &[]), OWN));
        assert!(is_switchable(&client(1, "Mousepad", &[WindowType::Normal]), OWN));
        assert!(!is_switchable(&client(1, "Pda-shell", &[WindowType::Normal]), OWN));
        assert!(!is_switchable(&client(1, "Onboard", &[WindowType::Normal]), OWN));
        assert!(!is_switchable(&client(1, "Tint2", &[WindowType::Dock]), OWN));
        let mut own = client(1, "x", &[]);
        own.pid = Some(OWN);
        assert!(!is_switchable(&own, OWN));
        let mut skip = client(1, "x", &[]);
        skip.skip_taskbar = true;
        assert!(!is_switchable(&skip, OWN));
        let mut dialog = client(1, "Gnumeric", &[WindowType::Dialog]);
        assert!(is_switchable(&dialog, OWN));
        dialog.transient_for = Some(5);
        assert!(!is_switchable(&dialog, OWN));
    }

    #[test]
    fn active_resolution() {
        let editor = client(1, "Mousepad", &[]);
        let mut save_dialog = client(2, "Mousepad", &[WindowType::Dialog]);
        save_dialog.transient_for = Some(1);
        let start_menu = client(3, "Pda-shell", &[WindowType::Utility]);
        let today = client(4, "Pda-shell", &[WindowType::Desktop]);
        let clients = vec![editor.clone(), save_dialog, start_menu, today];
        let apps = vec![app(&editor)];
        let prev = Some(WindowId(1));

        assert_eq!(resolve_active(&clients, &apps, Some(1), None, OWN), Some(WindowId(1)));
        assert_eq!(resolve_active(&clients, &apps, Some(2), None, OWN), Some(WindowId(1)));
        assert_eq!(resolve_active(&clients, &apps, Some(3), prev, OWN), Some(WindowId(1)));
        assert_eq!(resolve_active(&clients, &apps, Some(3), None, OWN), None);
        assert_eq!(resolve_active(&clients, &apps, Some(4), prev, OWN), None);
        assert_eq!(resolve_active(&clients, &apps, None, prev, OWN), None);
        // A previous window that has since closed is not resurrected.
        assert_eq!(resolve_active(&clients, &apps, Some(3), Some(WindowId(9)), OWN), None);
        // Active but not managed yet (the Start menu right after mapping):
        // the program in front stays.
        assert_eq!(resolve_active(&clients, &apps, Some(77), prev, OWN), Some(WindowId(1)));
        assert_eq!(resolve_active(&clients, &apps, Some(77), None, OWN), None);
    }
}

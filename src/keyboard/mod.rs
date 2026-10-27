//! On-screen keyboard integration (Onboard).
//!
//! Onboard runs as a separate process. The shell starts it on first use,
//! positions it and picks its layout through Onboard's own GSettings keys
//! (directly above the command bar), and shows/hides it through Onboard's
//! D-Bus interface. Onboard's `Visible` property is followed, so hiding the
//! keyboard with its own close key (or Onboard exiting) is reported back.

pub mod focus;

use crate::config::KeyboardConfig;
use crate::shell::layout::{Layout, Orientation, Rect};
use gtk::gio;
use gtk::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::cell::RefCell;
use std::process::{Child, Command};
use std::rc::Rc;
use std::time::Duration;

/// The input panels the keyboard button can show: (menu label, config name).
pub const PANELS: [(&str, &str); 2] = [("Pocket PC keyboard", "pocketpc"), ("Full keyboard (Onboard)", "onboard")];

const BUS_NAME: &str = "org.onboard.Onboard";
const OBJECT_PATH: &str = "/org/onboard/Onboard/Keyboard";
const INTERFACE: &str = "org.onboard.Onboard.Keyboard";
const SYSTEM_LAYOUTS: &str = "/usr/share/onboard/layouts";
/// How long a freshly started keyboard is watched for exiting before it
/// showed itself (missing modules, no display, ...).
const STARTUP_WATCH: Duration = Duration::from_secs(15);

/// Resolve a configured layout ("Small" or a path) to an .onboard file.
pub fn layout_file(name: &str, system_dir: &Path) -> Option<PathBuf> {
    if name.is_empty() {
        return None;
    }
    if name.contains('/') {
        return Some(PathBuf::from(name));
    }
    let file = system_dir.join(format!("{name}.onboard"));
    file.exists().then_some(file)
}

/// Where the keyboard goes: the bottom of the work area, full width.
pub fn keyboard_rect(layout: &Layout) -> Rect {
    let h = match layout.orientation {
        Orientation::Portrait => (layout.workarea.height * 30 / 100).clamp(160, 320),
        Orientation::Landscape => (layout.workarea.height * 40 / 100).clamp(150, 260),
    };
    let wa = layout.workarea;
    Rect::new(wa.x, wa.y + (wa.height - h) as i32, wa.width, h)
}

pub struct Keyboard {
    cfg: KeyboardConfig,
    rect: Rect,
    orientation: Orientation,
    child: Rc<RefCell<Option<Child>>>,
    on_visible: Rc<dyn Fn(bool)>,
    /// Keeps the D-Bus subscriptions alive.
    _watch: Option<gio::SignalSubscription>,
}

impl Keyboard {
    /// `on_visible` is called with Onboard's real visibility whenever it
    /// changes, including when it is hidden from its own controls.
    pub fn new(cfg: KeyboardConfig, layout: &Layout, on_visible: Rc<dyn Fn(bool)>) -> Self {
        Self {
            cfg,
            rect: keyboard_rect(layout),
            orientation: layout.orientation,
            child: Rc::default(),
            _watch: watch_visibility(on_visible.clone()),
            on_visible,
        }
    }

    pub fn rect(&self) -> Rect {
        self.rect
    }

    fn configure_onboard(&self) {
        let Some(source) = gio::SettingsSchemaSource::default() else { return };
        let set = |schema: &str, f: &dyn Fn(&gio::Settings) -> Result<(), glib::BoolError>| {
            if source.lookup(schema, true).is_some() {
                if let Err(e) = f(&gio::Settings::new(schema)) {
                    log::warn!("onboard setting {schema}: {e}");
                }
            } else {
                log::warn!("GSettings schema {schema} missing; Onboard keeps its defaults");
            }
        };
        let r = self.rect;
        set("org.onboard.window", &|s| {
            s.set_boolean("docking-enabled", false)?;
            s.set_boolean("window-decoration", false)?;
            s.set_boolean("force-to-top", true)?;
            s.set_boolean("keep-aspect-ratio", false)?;
            Ok(())
        });
        let orient = match self.orientation {
            Orientation::Portrait => "org.onboard.window.portrait",
            Orientation::Landscape => "org.onboard.window.landscape",
        };
        set(orient, &|s| {
            s.set_int("x", r.x)?;
            s.set_int("y", r.y)?;
            s.set_int("width", r.width as i32)?;
            s.set_int("height", r.height as i32)?;
            Ok(())
        });
        set("org.onboard", &|s| {
            s.set_boolean("show-status-icon", false)?;
            s.set_boolean("start-minimized", false)?;
            if let Some(file) = layout_file(&self.cfg.layout, Path::new(SYSTEM_LAYOUTS)) {
                s.set_string("layout", &file.to_string_lossy())?;
            } else if !self.cfg.layout.is_empty() {
                log::warn!("Onboard layout {:?} not found; keeping Onboard's default", self.cfg.layout);
            }
            Ok(())
        });
        // Keyboard activation stays manual (command bar button) for the POC,
        // and the word suggestion row would cost a row of keys.
        set("org.onboard.auto-show", &|s| s.set_boolean("enabled", false));
        set("org.onboard.typing-assistance.word-suggestions", &|s| s.set_boolean("enabled", false));
        gio::Settings::sync();
    }

    fn running(&self) -> bool {
        child_running(&self.child)
    }

    /// Report "not visible" from the main loop: the caller is in the middle
    /// of running the effect that asked for the keyboard.
    fn report_hidden(&self) {
        let cb = self.on_visible.clone();
        glib::idle_add_local_once(move || cb(false));
    }

    fn call(&self, method: &str) {
        let Ok(bus) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) else {
            log::warn!("no session bus for keyboard control");
            return;
        };
        bus.call(
            Some(BUS_NAME),
            OBJECT_PATH,
            INTERFACE,
            method,
            None,
            None,
            gio::DBusCallFlags::NONE,
            2000,
            gio::Cancellable::NONE,
            move |r| {
                if let Err(e) = r {
                    log::warn!("onboard D-Bus call failed: {e}");
                }
            },
        );
    }

    pub fn set_visible(&mut self, visible: bool) {
        if visible && !self.running() {
            let Some((prog, args)) = self.cfg.command.split_first() else {
                log::warn!("no keyboard command configured");
                self.report_hidden();
                return;
            };
            self.configure_onboard();
            match Command::new(prog).args(args).spawn() {
                // A freshly started Onboard shows itself and reports it.
                Ok(child) => {
                    log::info!("started {prog} (pid {})", child.id());
                    *self.child.borrow_mut() = Some(child);
                    self.watch_startup();
                }
                Err(e) => {
                    log::error!("cannot start {prog}: {e}");
                    self.report_hidden();
                }
            }
            return;
        }
        if self.running() {
            self.call(if visible { "Show" } else { "Hide" });
        }
    }

    /// A keyboard that exits before it ever took its bus name produces no
    /// visibility signal; report it as hidden so the toggle rolls back.
    fn watch_startup(&self) {
        let (child, cb) = (Rc::downgrade(&self.child), self.on_visible.clone());
        let mut left = STARTUP_WATCH.as_secs();
        glib::timeout_add_seconds_local(1, move || {
            left = left.saturating_sub(1);
            let Some(child) = child.upgrade() else { return glib::ControlFlow::Break };
            if !child_running(&child) {
                cb(false);
                return glib::ControlFlow::Break;
            }
            if left == 0 {
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
    }

    pub fn stop(&mut self) {
        if let Some(mut c) = self.child.borrow_mut().take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn child_running(child: &RefCell<Option<Child>>) -> bool {
    let mut child = child.borrow_mut();
    match child.as_mut().map(|c| c.try_wait()) {
        Some(Ok(None)) => true,
        Some(Ok(Some(status))) => {
            log::warn!("onboard exited: {status}");
            *child = None;
            false
        }
        _ => false,
    }
}

/// Follow Onboard's `Visible` property and its presence on the bus.
fn watch_visibility(on_visible: Rc<dyn Fn(bool)>) -> Option<gio::SignalSubscription> {
    let bus = match gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) {
        Ok(bus) => bus,
        Err(e) => {
            log::warn!("no session bus; keyboard state is not followed: {e}");
            return None;
        }
    };
    let cb = on_visible.clone();
    let sub = bus.subscribe_to_signal(
        Some(BUS_NAME),
        Some("org.freedesktop.DBus.Properties"),
        Some("PropertiesChanged"),
        Some(OBJECT_PATH),
        None,
        gio::DBusSignalFlags::NONE,
        move |signal| {
            let Some((iface, changed, _)) =
                signal.parameters.get::<(String, HashMap<String, glib::Variant>, Vec<String>)>()
            else {
                return;
            };
            if iface == INTERFACE {
                if let Some(visible) = changed.get("Visible").and_then(|v| v.get::<bool>()) {
                    log::debug!("onboard reports visible={visible}");
                    cb(visible);
                }
            }
        },
    );
    // Onboard leaving the bus (exit, crash) means it is not visible. The
    // watch lasts as long as the shell.
    let _ = gio::bus_watch_name_on_connection(
        &bus,
        BUS_NAME,
        gio::BusNameWatcherFlags::NONE,
        |_, _, _| {},
        move |_, _| on_visible(false),
    );
    Some(sub)
}

impl Drop for Keyboard {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Design;

    #[test]
    fn layout_names_and_paths() {
        let dir = std::env::temp_dir().join(format!("pda-onboard-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Small.onboard"), "").unwrap();
        assert_eq!(layout_file("Small", &dir), Some(dir.join("Small.onboard")));
        assert_eq!(layout_file("Missing", &dir), None);
        assert_eq!(layout_file("", &dir), None);
        assert_eq!(layout_file("/opt/kbd/My.onboard", &dir), Some(PathBuf::from("/opt/kbd/My.onboard")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn keyboard_sits_on_top_of_the_command_bar() {
        for (w, h) in [(600, 1024), (1024, 600)] {
            let l = Layout::compute(w, h, &Design::default());
            let k = keyboard_rect(&l);
            assert_eq!(k.y as u32 + k.height, l.bottom_bar.y as u32, "{w}x{h}");
            assert_eq!(k.width, w);
            assert!(k.height * 2 < l.workarea.height, "keyboard leaves room for the app");
        }
    }
}

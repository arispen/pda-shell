//! On-screen keyboard integration (Onboard).
//!
//! Onboard runs as a separate process. The shell starts it on first use,
//! positions it through Onboard's own GSettings keys (directly above the
//! command bar), and shows/hides it through Onboard's D-Bus interface.

use crate::shell::layout::{Layout, Orientation, Rect};
use gtk::gio;
use gtk::prelude::*;
use std::process::{Child, Command};

const BUS_NAME: &str = "org.onboard.Onboard";
const OBJECT_PATH: &str = "/org/onboard/Onboard/Keyboard";
const INTERFACE: &str = "org.onboard.Onboard.Keyboard";

/// Where the keyboard goes: the bottom of the work area, full width.
pub fn keyboard_rect(layout: &Layout) -> Rect {
    let h = match layout.orientation {
        Orientation::Portrait => (layout.workarea.height * 30 / 100).clamp(160, 320),
        Orientation::Landscape => (layout.workarea.height * 42 / 100).clamp(150, 260),
    };
    let wa = layout.workarea;
    Rect::new(wa.x, wa.y + (wa.height - h) as i32, wa.width, h)
}

pub struct Keyboard {
    command: Vec<String>,
    rect: Rect,
    orientation: Orientation,
    child: Option<Child>,
}

impl Keyboard {
    pub fn new(command: Vec<String>, layout: &Layout) -> Self {
        Self { command, rect: keyboard_rect(layout), orientation: layout.orientation, child: None }
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
            Ok(())
        });
        gio::Settings::sync();
    }

    fn running(&mut self) -> bool {
        match self.child.as_mut().map(|c| c.try_wait()) {
            Some(Ok(None)) => true,
            Some(Ok(Some(status))) => {
                log::warn!("onboard exited: {status}");
                self.child = None;
                false
            }
            _ => false,
        }
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
            let Some((prog, args)) = self.command.split_first() else {
                log::warn!("no keyboard command configured");
                return;
            };
            self.configure_onboard();
            match Command::new(prog).args(args).spawn() {
                // A freshly started Onboard shows itself.
                Ok(child) => {
                    log::info!("started {prog} (pid {})", child.id());
                    self.child = Some(child);
                }
                Err(e) => log::error!("cannot start {prog}: {e}"),
            }
            return;
        }
        if self.running() {
            self.call(if visible { "Show" } else { "Hide" });
        }
    }

    pub fn stop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
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

//! Top status bar: Start button, active title, system status, clock.

pub mod system;

use super::ShellWindow;
use crate::config::Config;
use crate::platform::x11::X11;
use crate::platform::SurfaceRole;
use crate::shell::layout::Layout;
use crate::shell::state::Event;
use crate::shell::{Command, Dispatch};
use gtk::prelude::*;
use std::path::Path;
use std::rc::Rc;

pub struct StatusBar {
    pub win: ShellWindow,
    start: gtk::Button,
    title: gtk::Label,
    clock: gtk::Label,
    battery: gtk::Box,
    network: gtk::Image,
}

impl StatusBar {
    pub fn new(app: &gtk::Application, layout: &Layout, x11: &Rc<X11>, cfg: &Config, dispatch: Dispatch) -> Rc<Self> {
        let win = ShellWindow::new(app, "pda-shell status bar", "bar", SurfaceRole::TopPanel, layout.top_bar, x11, cfg);
        win.window.add_css_class("top-bar");
        let row = gtk::Box::new(gtk::Orientation::Horizontal, cfg.design.spacing_px as i32);

        let start = gtk::Button::new();
        start.add_css_class("start");
        let start_content = gtk::Box::new(gtk::Orientation::Horizontal, 3);
        let start_icon = (cfg.design.top_bar_px * 2 / 3).max(cfg.design.icon_px);
        start_content.append(&super::named_icon("pda-start", start_icon));
        start_content.append(&gtk::Label::new(Some("Start")));
        start.set_child(Some(&start_content));
        start.update_property(&[gtk::accessible::Property::Label("Start")]);
        let d = dispatch.clone();
        start.connect_clicked(move |_| d(Command::StartButton));

        let title = gtk::Label::new(Some("Today"));
        title.add_css_class("title");
        title.set_hexpand(true);
        title.set_xalign(0.0);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title.set_margin_start(cfg.design.padding_px as i32);

        let network = gtk::Image::new();
        network.set_pixel_size(cfg.design.icon_px as i32);
        network.set_visible(false);
        let battery = gtk::Box::new(gtk::Orientation::Horizontal, 1);
        battery.set_visible(false);

        // The clock opens Today, like tapping the clock on Pocket PC showed
        // the date.
        let clock = gtk::Label::new(None);
        let clock_button = gtk::Button::new();
        clock_button.set_child(Some(&clock));
        let d = dispatch;
        clock_button.connect_clicked(move |_| d(Command::Shell(Event::Today)));

        row.append(&start);
        row.append(&title);
        row.append(&network);
        row.append(&battery);
        row.append(&clock_button);
        win.set_content(&row);

        let bar = Rc::new(Self { win, start, title, clock, battery, network });
        bar.tick();
        bar.update_system_status(cfg);
        let b = Rc::downgrade(&bar);
        glib::timeout_add_seconds_local(1, move || match b.upgrade() {
            Some(b) => {
                b.tick();
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });
        let b = Rc::downgrade(&bar);
        let cfg = cfg.clone();
        glib::timeout_add_seconds_local(15, move || match b.upgrade() {
            Some(b) => {
                b.update_system_status(&cfg);
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });
        bar
    }

    fn tick(&self) {
        if let Ok(now) = glib::DateTime::now_local() {
            if let Ok(s) = now.format("%H:%M") {
                if self.clock.text() != s {
                    self.clock.set_text(&s);
                }
            }
        }
    }

    fn update_system_status(&self, cfg: &Config) {
        match system::networks(Path::new("/sys/class/net")).first() {
            Some(n) => {
                self.network.set_icon_name(Some(if n.wireless {
                    "network-wireless-symbolic"
                } else {
                    "network-wired-symbolic"
                }));
                self.network
                    .update_property(&[gtk::accessible::Property::Label(&format!("Connected: {}", n.interface))]);
                self.network.set_visible(true);
            }
            None => self.network.set_visible(false),
        }
        while let Some(c) = self.battery.first_child() {
            self.battery.remove(&c);
        }
        match system::battery(Path::new("/sys/class/power_supply")) {
            Some(b) => {
                let icon = gtk::Image::from_icon_name(&system::battery_icon(&b));
                icon.set_pixel_size(cfg.design.icon_px as i32);
                self.battery.append(&icon);
                self.battery.append(&gtk::Label::new(Some(&format!("{}%", b.percent))));
                self.battery.set_visible(true);
            }
            None => self.battery.set_visible(false),
        }
    }

    pub fn set_title(&self, title: &str) {
        self.title.set_text(title);
    }

    pub fn set_start_open(&self, open: bool) {
        if open {
            self.start.add_css_class("open");
        } else {
            self.start.remove_css_class("open");
        }
    }

    pub fn start_button(&self) -> &gtk::Button {
        &self.start
    }
}

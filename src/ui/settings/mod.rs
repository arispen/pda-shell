//! Settings: Personal, System and Connections icons like the Pocket PC
//! Settings screen, and the read-only screens they open (About, Screen,
//! Network) plus the Linux system's own settings dialogs.
//!
//! Host name, OS, kernel and memory live here (About), not on Today.

pub mod info;

use super::programs::{clear, note};
use crate::apps::{self, AppEntry};
use crate::config::{Config, Design};
use crate::shell::layout::Layout;
use crate::shell::state::{Event, Page};
use crate::shell::{Command, Dispatch};
use crate::ui::status_bar::system;
use gtk::prelude::*;
use std::path::Path;

/// Diagonal of the target panel (docs/hardware.md).
const PANEL_DIAGONAL_IN: f64 = 5.0;
/// The input panel's own settings program, when installed.
const INPUT_SETTINGS: &str = "onboard-settings.desktop";

pub struct Settings {
    pub settings: gtk::ScrolledWindow,
    pub about: gtk::ScrolledWindow,
    pub screen: gtk::ScrolledWindow,
    pub network: gtk::ScrolledWindow,
    pub linux_settings: gtk::ScrolledWindow,
    personal: gtk::Box,
    about_list: gtk::ListBox,
    network_list: gtk::ListBox,
    linux_list: gtk::ListBox,
    cfg: Config,
    columns: u32,
    dispatch: Dispatch,
}

fn scrolled(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(child)
        .build()
}

fn kv_list() -> gtk::ListBox {
    let l = gtk::ListBox::new();
    l.add_css_class("compact");
    l.add_css_class("lined");
    l.set_selection_mode(gtk::SelectionMode::None);
    l
}

fn section(title: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(title));
    l.add_css_class("section-title");
    l.set_xalign(0.0);
    l
}

/// Millimetres on the target panel for `px` pixels at `ppi`.
pub fn mm(px: u32, ppi: f64) -> f64 {
    px as f64 * 25.4 / ppi
}

/// Pixel density of the target panel for a screen of this size.
pub fn panel_ppi(width: u32, height: u32) -> f64 {
    ((width as f64).powi(2) + (height as f64).powi(2)).sqrt() / PANEL_DIAGONAL_IN
}

/// The Screen page: resolution, physical scale and the active tokens.
pub fn screen_rows(layout: &Layout, d: &Design) -> Vec<(&'static str, String)> {
    let (w, h) = (layout.screen.width, layout.screen.height);
    let ppi = panel_ppi(w, h);
    let size = |px: u32| format!("{px} px ({:.1} mm)", mm(px, ppi));
    // Point sizes at the published DPI: 10 pt, the usual program font.
    let app_px = (10.0 * d.app_dpi as f64 / 72.0).round() as u32;
    vec![
        ("Screen", format!("{w} × {h} pixels, {}", layout.orientation.name())),
        (
            "Panel",
            format!("{PANEL_DIAGONAL_IN:.0}-inch, {ppi:.0} ppi: 1 px is {:.3} mm", mm(1, ppi)),
        ),
        ("Density profile", d.density.clone()),
        ("Text", format!("{} body, {} secondary", size(d.font_px), size(d.small_font_px))),
        ("Stylus targets", format!("{} buttons, {} list rows", size(d.target_px), size(d.row_px))),
        ("Bars", format!("{} status bar, {} command bar", size(d.top_bar_px), size(d.bottom_bar_px))),
        ("Programs", format!("{} dpi: 10 pt text is {}", d.app_dpi, size(app_px))),
    ]
}

impl Settings {
    pub fn new(layout: &Layout, cfg: &Config, dispatch: Dispatch) -> Self {
        let pad = cfg.design.padding_px as i32;
        let spacing = cfg.design.spacing_px;
        let columns = layout.grid_columns;
        let px = cfg.design.today_icon_px;

        let item = |icon: &str, label: &str, cmd: Command| {
            let b = super::grid_button(super::named_icon(icon, px), label);
            b.update_property(&[gtk::accessible::Property::Label(label)]);
            let d = dispatch.clone();
            b.connect_clicked(move |_| d(cmd.clone()));
            b
        };
        let personal = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let system = super::icon_grid(
            vec![
                item("pda-about", "About", Command::Shell(Event::ShowPage(Page::About))),
                item("pda-screen", "Screen", Command::Shell(Event::ShowPage(Page::Screen))),
                item("pda-tools", "Linux settings", Command::Shell(Event::ShowPage(Page::LinuxSettings))),
                item("pda-power", "End session", Command::Shell(Event::EndSession)),
            ],
            columns,
            spacing,
        );
        let connections = super::icon_grid(
            vec![item("pda-network", "Network", Command::Shell(Event::ShowPage(Page::Network)))],
            columns,
            spacing,
        );
        let main = gtk::Box::new(gtk::Orientation::Vertical, pad);
        main.set_margin_start(pad);
        main.set_margin_end(pad);
        main.set_margin_top(pad);
        main.append(&personal);
        main.append(&section("System"));
        main.append(&system);
        main.append(&section("Connections"));
        main.append(&connections);

        let about_list = kv_list();
        let network_list = kv_list();
        let screen_list = kv_list();
        for (k, v) in screen_rows(layout, &cfg.design) {
            screen_list.append(&super::kv_row(k, &v));
        }
        let screen = gtk::Box::new(gtk::Orientation::Vertical, 0);
        screen.append(&screen_list);
        screen.append(&note(
            "Sizes are for the 5-inch target panel. Choose another profile with dev-session.sh --density NAME, \
             PDA_SHELL_DENSITY, or design.density in pda-shell.toml.",
        ));

        let linux_list = kv_list();
        linux_list.add_css_class("targets");
        linux_list.set_activate_on_single_click(true);
        let d = dispatch.clone();
        linux_list.connect_row_activated(move |_, row| d(Command::Shell(Event::Launch(row.widget_name().to_string()))));
        let linux = gtk::Box::new(gtk::Orientation::Vertical, 0);
        linux.append(&note("Settings dialogs of the Linux desktop programs. They are desktop-sized and may need the keyboard."));
        linux.append(&linux_list);

        Self {
            settings: scrolled(&main),
            about: scrolled(&about_list),
            screen: scrolled(&screen),
            network: scrolled(&network_list),
            linux_settings: scrolled(&linux),
            personal,
            about_list,
            network_list,
            linux_list,
            cfg: cfg.clone(),
            columns,
            dispatch,
        }
    }

    pub fn set_apps(&self, all: &[AppEntry]) {
        // Personal: only what is installed.
        clear(&self.personal);
        if let Some(entry) = all.iter().find(|a| a.id == INPUT_SETTINGS) {
            let b = super::grid_button(super::named_icon("pda-input", self.cfg.design.today_icon_px), "Input panel");
            b.update_property(&[gtk::accessible::Property::Label("Input panel")]);
            let (d, id) = (self.dispatch.clone(), entry.id.clone());
            b.connect_clicked(move |_| d(Command::Shell(Event::Launch(id.clone()))));
            self.personal.append(&section("Personal"));
            self.personal.append(&super::icon_grid(vec![b], self.columns, self.cfg.design.spacing_px));
        }

        self.linux_list.remove_all();
        for entry in all.iter().filter(|a| apps::is_settings(a)) {
            let row = gtk::ListBoxRow::new();
            row.set_widget_name(&entry.id);
            row.update_property(&[gtk::accessible::Property::Label(&entry.name)]);
            let b = gtk::Box::new(gtk::Orientation::Horizontal, self.cfg.design.padding_px as i32);
            b.append(&super::icon_image(apps::gicon(entry).as_ref(), "application-x-executable", self.cfg.design.icon_px));
            let l = gtk::Label::new(Some(&entry.name));
            l.set_xalign(0.0);
            l.set_ellipsize(gtk::pango::EllipsizeMode::End);
            b.append(&l);
            row.set_child(Some(&b));
            self.linux_list.append(&row);
        }
    }

    /// Re-read live values before a screen is shown.
    pub fn refresh(&self, page: Page) {
        match page {
            Page::About => {
                self.about_list.remove_all();
                for (k, v) in info::system_info() {
                    self.about_list.append(&super::kv_row(k, &v));
                }
            }
            Page::Network => {
                self.network_list.remove_all();
                let nets = system::networks(Path::new("/sys/class/net"));
                if nets.is_empty() {
                    self.network_list.append(&super::kv_row("Network", "Not connected"));
                }
                for n in nets {
                    let kind = if n.wireless { "wireless" } else { "wired" };
                    self.network_list.append(&super::kv_row(&n.interface, &format!("Connected ({kind})")));
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_panel_is_237_ppi() {
        let ppi = panel_ppi(1024, 600);
        assert!((ppi - 237.4).abs() < 0.1, "{ppi}");
        assert_eq!(panel_ppi(600, 1024), ppi);
        assert!((mm(22, ppi) - 2.35).abs() < 0.01);
    }

    #[test]
    fn screen_page_states_physical_sizes() {
        let mut d = Design::default();
        d.font_px = 22;
        d.app_dpi = 144;
        let rows = screen_rows(&Layout::compute(600, 1024, &d), &d);
        let get = |k: &str| rows.iter().find(|(key, _)| *key == k).map(|(_, v)| v.clone()).unwrap();
        assert_eq!(get("Screen"), "600 × 1024 pixels, portrait");
        assert!(get("Text").starts_with("22 px (2.4 mm)"), "{}", get("Text"));
        assert_eq!(get("Programs"), "144 dpi: 10 pt text is 20 px (2.1 mm)");
    }
}

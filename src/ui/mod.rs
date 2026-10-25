//! GTK user interface of the shell.

pub mod command_bar;
pub mod escape;
pub mod programs;
pub mod settings;
pub mod sip;
pub mod start_menu;
pub mod status_bar;
pub mod today;

use crate::config::Config;
use crate::input::{self, StylusFeedback};
use crate::platform::x11::{gtk_glue, X11};
use crate::platform::SurfaceRole;
use crate::shell::layout::Rect;
use gtk::gio;
use gtk::prelude::*;
use std::path::PathBuf;
use std::rc::Rc;

/// A CSS font-family list from a comma-separated config value.
fn font_families(list: &str) -> String {
    list.split(',')
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .map(|f| format!("\"{f}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Build the application stylesheet from the design tokens.
pub fn stylesheet(cfg: &Config) -> String {
    let d = &cfg.design;
    let c = &cfg.colors;
    format!(
        r#"
window.pda, window.pda *, popover.pda, popover.pda * {{
    font-family: {font};
    font-size: {font_px}px;
}}
window.pda {{ background: {surface}; color: {text}; }}
window.pda button {{
    min-height: 0; min-width: 0;
    padding: 1px {pad}px;
    border-radius: {radius}px;
    box-shadow: none;
}}
window.pda image {{ -gtk-icon-size: {icon}px; }}
window.pda scrollbar slider {{ min-width: {scroll}px; min-height: {scroll}px; border-radius: 0; }}
.dim, .dim * {{ color: {text_dim}; font-size: {small_px}px; }}
.heading, .item-title {{ font-weight: bold; }}
.section-title {{
    font-weight: bold; color: {accent};
    border-bottom: 1px solid {border};
    padding: {pad}px {pad}px 1px {pad}px;
}}

/* Status bar (top): blue gradient, white text */
window.top-bar {{ background: linear-gradient(to bottom, {bar_bg}, {bar_bg_end}); color: {bar_fg}; }}
window.top-bar button {{
    background: transparent; color: {bar_fg}; border: none;
    padding: 0 {pad}px; margin: 0; min-width: {target}px; border-radius: 0;
}}
window.top-bar button:hover {{ background: {bar_hover}; }}
window.top-bar button:active, window.top-bar button.open {{ background: {bar_pressed}; }}
window.top-bar label, window.top-bar image {{ color: {bar_fg}; }}
window.top-bar .title {{ font-weight: bold; font-size: {title_px}px; }}
window.top-bar .start label {{ font-weight: bold; }}
window.top-bar .start image {{ -gtk-icon-size: {start_icon}px; }}

/* Command bar (bottom): a light menu bar with text commands */
window.menu-bar {{
    background: linear-gradient(to bottom, {menu_bg}, {menu_bg_end});
    color: {menu_fg}; border-top: 1px solid {menu_border};
}}
window.menu-bar button, window.menu-bar menubutton > button {{
    background: transparent; color: {menu_fg}; border: none; border-radius: 0;
    padding: 0 {pad}px; margin: 0; min-width: {target}px;
}}
window.menu-bar label {{ color: {menu_fg}; }}
window.menu-bar button:hover {{ background: alpha(white, 0.6); }}
window.menu-bar button:active, window.menu-bar button:checked, window.menu-bar button.open,
window.menu-bar menubutton > button:checked {{ background: {accent}; }}
window.menu-bar button:active label, window.menu-bar button:checked label, window.menu-bar button.open label,
window.menu-bar menubutton > button:checked label {{ color: {accent_fg}; }}
window.menu-bar button:disabled label {{ color: {text_dim}; }}
window.menu-bar separator {{ background: {menu_border}; min-width: 1px; margin: {sep_margin}px 0; }}
window.menu-bar .sip image {{ -gtk-icon-size: {sip_icon}px; }}
window.menu-bar .arrow {{ font-size: {small_px}px; }}
window.menu-bar .sip-chooser > button {{ min-width: 0; padding: 0 2px; }}

/* Built-in input panel */
window.sip-panel {{ background: {menu_bg_end}; border-top: 1px solid {menu_border}; }}
window.sip-panel button.sip-key {{
    background: linear-gradient(to bottom, {surface}, {menu_bg});
    color: {text}; border: 1px solid {menu_border}; border-radius: 2px;
    margin: 1px; padding: 0; min-height: 0; min-width: 0;
}}
window.sip-panel button.sip-key label {{ color: {text}; }}
window.sip-panel button.sip-key:active, window.sip-panel button.sip-key.latched {{ background: {accent}; }}
window.sip-panel button.sip-key:active label, window.sip-panel button.sip-key.latched label {{ color: {accent_fg}; }}

/* Today */
window.today {{ background: {surface}; }}
.today-header {{
    background: linear-gradient(to bottom, {surface_alt}, {surface});
    border-bottom: 1px solid {border}; padding: {pad}px {pad2}px;
}}
.today-header .date {{ font-weight: bold; font-size: {date_px}px; color: {accent}; }}
.today-header .summary {{ color: {text_dim}; }}
.today-item {{ border-bottom: 1px solid {separator}; padding: {pad}px {pad}px {pad}px {pad2}px; }}
.today-column {{ border-left: 1px solid {separator}; }}
.today-item > image {{ -gtk-icon-size: {today_icon}px; }}
.today-item list.compact row {{ padding-left: 0; }}

/* Icon grids: Programs, Settings, favorites on Today */
.grid-item {{
    padding: {pad}px 1px; border: 1px solid transparent; background: transparent; border-radius: 0;
}}
.grid-item image {{ -gtk-icon-size: {today_icon}px; }}
.grid-item:hover {{ background: {surface_alt}; }}
.grid-item:active {{ background: {accent}; border-color: {bar_pressed}; }}
.grid-item:active label {{ color: {accent_fg}; }}
.page-note {{ padding: {pad}px {pad2}px; }}
.ask-bar {{ background: {surface_alt}; border-bottom: 1px solid {border}; padding: {pad}px {pad2}px; }}
.ask-bar button {{ min-height: {target}px; min-width: {target}px; }}
.ask-bar entry {{ min-height: {row}px; }}
window.native textview, window.native textview text {{ background: {surface}; color: {text}; }}

/* Lists (recent documents, Start menu, switcher, Settings pages) */
list.compact, list.compact row {{ background: transparent; }}
list.compact row {{ padding: 1px {pad}px; min-height: {row}px; border-radius: 0; }}
list.targets row {{ min-height: {target}px; }}
list.lined row {{ border-bottom: 1px solid {separator}; }}
list.compact row.destructive label, list.compact row.destructive image {{ color: #b3261e; }}
list.compact row.destructive.armed {{ background: #b3261e; }}
list.compact row.destructive.armed label, list.compact row.destructive.armed image {{ color: #ffffff; }}
list.compact row .active-marker {{ color: {accent}; font-weight: bold; }}
list.compact row.more label {{ color: {accent}; }}
list.compact row:hover {{ background: {surface_alt}; }}
list.compact row:selected, list.compact row:active {{ background: {accent}; color: {accent_fg}; }}
list.compact row:selected label, list.compact row:active label {{ color: {accent_fg}; }}
list.compact row:focus-visible {{ outline: 1px solid {focus}; }}
.kv-key {{ color: {text_dim}; font-size: {small_px}px; }}

/* Start menu: a drop-down anchored under the Start button */
window.start-menu {{ background: {surface}; border: 1px solid {menu_border}; border-top: none; }}
window.start-menu entry {{ min-height: {row}px; padding: 1px {pad}px; margin: {pad}px; border-radius: {radius}px; }}
.start-recent {{ background: {surface_alt}; border-bottom: 1px solid {border}; padding: 2px; }}
.start-recent button {{ background: transparent; border: none; border-radius: 0; padding: 2px; min-width: {target}px; min-height: {target}px; }}
.start-recent button:active {{ background: {accent}; }}
window.start-menu list.compact row separator {{ margin: 0; }}
window.start-menu row.minor label {{ color: {text_dim}; }}

/* Tooltips. The shell sets none (a stylus leaves the pointer where it
   tapped, so they would pop up after every tap); GTK still shows some on
   its own. Without a compositor GTK's translucent rounded tooltip renders
   black, and inside a bar it inherits the bar's text colour: make it a
   flat, opaque box with its own colours. */
tooltip, tooltip.background {{
    background: {surface}; color: {text}; border: 1px solid {menu_border};
    border-radius: 0; box-shadow: none; padding: 2px {pad}px;
}}
tooltip label, tooltip * {{ color: {text}; font-family: {font}; font-size: {small_px}px; }}

/* Popovers. Without a compositor the shadow area would render black, so
   popovers are flat with a plain border. Labels inside popovers opened from
   a bar must not inherit the bar's text colour. */
popover.pda, popover.pda > contents {{ box-shadow: none; }}
popover.pda > contents {{
    padding: 0; border-radius: 0;
    border: 1px solid {menu_border}; background: {surface};
}}
window.bar popover.pda label, window.bar popover.pda image {{ color: {text}; }}
window.bar popover.pda row:selected label, window.bar popover.pda row:active label {{ color: {accent_fg}; }}
popover.pda modelbutton {{ min-height: {target}px; padding: 0 {pad2}px; border-radius: 0; }}
popover.pda modelbutton:hover {{ background: {accent}; color: {accent_fg}; }}
popover.pda separator {{ margin: 2px 0; }}
popover.pda list.menu-list row {{ padding: 0 {pad2}px 0 {pad}px; }}
popover.pda .menu-mark {{ min-width: {font_px}px; color: {accent}; }}
popover.pda list.menu-list row:disabled label {{ color: {text_dim}; }}

/* Exit full screen: unmanaged, above full-screen programs */
window.escape {{ background: {bar_bg_end}; }}
window.escape button {{ background: {bar_bg_end}; color: {bar_fg}; border: 1px solid {bar_fg}; padding: 0 {pad}px; border-radius: 0; }}
window.escape button:active {{ background: {bar_pressed}; }}
window.escape label, window.escape image {{ color: {bar_fg}; }}
"#,
        font = font_families(&d.font_family),
        font_px = d.font_px,
        small_px = d.small_font_px,
        title_px = d.title_font_px,
        date_px = d.date_font_px,
        row = d.row_px,
        target = d.target_px,
        pad = d.padding_px,
        pad2 = d.padding_px * 2,
        radius = d.radius_px,
        icon = d.icon_px,
        start_icon = (d.top_bar_px * 2 / 3).max(d.icon_px),
        sip_icon = (d.bottom_bar_px * 2 / 3).max(d.icon_px),
        sep_margin = d.bottom_bar_px / 5,
        scroll = (d.target_px / 4).max(6),
        today_icon = d.today_icon_px,
        surface = c.surface,
        surface_alt = c.surface_alt,
        text = c.text,
        text_dim = c.text_dim,
        accent = c.accent,
        accent_fg = c.accent_fg,
        border = c.border,
        separator = c.separator,
        focus = c.focus,
        bar_bg = c.bar_bg,
        bar_bg_end = c.bar_bg_end,
        bar_fg = c.bar_fg,
        bar_hover = c.bar_hover,
        bar_pressed = c.bar_pressed,
        menu_bg = c.menu_bg,
        menu_bg_end = c.menu_bg_end,
        menu_fg = c.menu_fg,
        menu_border = c.menu_border,
    )
}

pub fn install_stylesheet(cfg: &Config) {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(&stylesheet(cfg));
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        let theme = gtk::IconTheme::for_display(&display);
        match icon_dir() {
            Some(dir) => theme.add_search_path(&dir),
            None => log::warn!("shell icons not found; set PDA_SHELL_ICONS"),
        }
    }
}

/// The shell's own icons (`pda-*`): `PDA_SHELL_ICONS`, the installed copy,
/// or the source tree.
fn icon_dir() -> Option<PathBuf> {
    let candidates = [
        std::env::var_os("PDA_SHELL_ICONS").map(PathBuf::from),
        Some(PathBuf::from("/usr/share/pda-shell/icons")),
        Some(PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icons"))),
    ];
    candidates.into_iter().flatten().find(|d| d.join("pda-start.svg").is_file())
}

/// A shell surface: an undecorated window with a role the window manager
/// understands, plus an overlay for long-press feedback.
pub struct ShellWindow {
    pub window: gtk::ApplicationWindow,
    pub overlay: gtk::Overlay,
    pub feedback: StylusFeedback,
    x11: Rc<X11>,
    rect: std::rc::Rc<std::cell::Cell<Rect>>,
}

impl ShellWindow {
    pub fn new(
        app: &gtk::Application,
        title: &str,
        css_class: &str,
        role: SurfaceRole,
        rect: Rect,
        x11: &Rc<X11>,
        cfg: &Config,
    ) -> Self {
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title(title)
            .decorated(false)
            .resizable(false)
            .default_width(rect.width as i32)
            .default_height(rect.height as i32)
            .build();
        window.add_css_class("pda");
        window.add_css_class(css_class);

        let overlay = gtk::Overlay::new();
        let feedback = StylusFeedback::new(cfg);
        overlay.add_overlay(feedback.widget());
        window.set_child(Some(&overlay));
        let current = std::rc::Rc::new(std::cell::Cell::new(rect));

        let x = x11.clone();
        window.connect_realize(move |w| match gtk_glue::xid(w.upcast_ref()) {
            Some(xid) => {
                if let Err(e) = x.apply_role(xid, role, rect) {
                    log::error!("cannot set window role {role:?}: {e}");
                }
            }
            None => log::error!("not an X11 surface; run with GDK_BACKEND=x11"),
        });
        // The window manager may place the window itself when mapping it;
        // put it back where the layout wants it.
        let (x, r) = (x11.clone(), current.clone());
        window.connect_map(move |w| {
            if let Some(xid) = gtk_glue::xid(w.upcast_ref()) {
                if let Err(e) = x.place(xid, r.get()) {
                    log::warn!("cannot place window: {e}");
                }
            }
        });
        Self { window, overlay, feedback, x11: x11.clone(), rect: current }
    }

    /// Move and resize the surface, e.g. to keep it clear of the on-screen
    /// keyboard.
    pub fn set_rect(&self, rect: Rect) {
        if self.rect.replace(rect) == rect {
            return;
        }
        self.window.set_default_size(rect.width as i32, rect.height as i32);
        if let Some(xid) = gtk_glue::xid(self.window.upcast_ref()) {
            if let Err(e) = self.x11.place(xid, rect) {
                log::warn!("cannot resize window: {e}");
            }
        }
    }

    pub fn set_content(&self, child: &impl IsA<gtk::Widget>) {
        self.overlay.set_child(Some(child));
    }
}

/// One entry in a context menu.
pub struct MenuItem {
    pub label: String,
    pub action: Box<dyn Fn()>,
}

impl MenuItem {
    pub fn new(label: impl Into<String>, action: impl Fn() + 'static) -> Self {
        Self { label: label.into(), action: Box::new(action) }
    }
}

/// Pop up a compact context menu at (x, y) in `widget` coordinates.
pub fn popup_menu(widget: &gtk::Widget, x: f64, y: f64, items: Vec<MenuItem>) {
    let model = gio::Menu::new();
    let group = gio::SimpleActionGroup::new();
    for (i, item) in items.into_iter().enumerate() {
        let name = format!("item{i}");
        let action = gio::SimpleAction::new(&name, None);
        let f = item.action;
        action.connect_activate(move |_, _| f());
        group.add_action(&action);
        model.append(Some(&item.label), Some(&format!("ctx.{name}")));
    }
    let popover = gtk::PopoverMenu::from_model(Some(&model));
    popover.add_css_class("pda");
    popover.set_has_arrow(false);
    popover.insert_action_group("ctx", Some(&group));
    popover.set_parent(widget);
    popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    popover.connect_closed(|p| {
        let p = p.clone();
        glib::idle_add_local_once(move || p.unparent());
    });
    popover.popup();
}

/// Attach long-press (and right-click) context menu behaviour to `widget`.
pub fn context_menu<F>(widget: &impl IsA<gtk::Widget>, feedback: &StylusFeedback, cfg: &Config, items: F)
where
    F: Fn() -> Vec<MenuItem> + 'static,
{
    let w = widget.as_ref().clone();
    input::attach_long_press(widget.as_ref(), feedback, cfg, move |x, y| {
        popup_menu(&w, x, y, items());
    });
}

/// A shell icon by name (see `assets/icons`).
pub fn named_icon(name: &str, px: u32) -> gtk::Image {
    let image = gtk::Image::from_icon_name(name);
    image.set_pixel_size(px as i32);
    image
}

/// A Programs/Settings style icon: large icon with the label under it.
pub fn grid_button(icon: gtk::Image, label: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("grid-item");
    let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
    content.append(&icon);
    let l = gtk::Label::new(Some(label));
    l.set_wrap(true);
    l.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    l.set_lines(2);
    l.set_ellipsize(gtk::pango::EllipsizeMode::End);
    l.set_justify(gtk::Justification::Center);
    l.set_max_width_chars(9);
    content.append(&l);
    button.set_child(Some(&content));
    button.update_property(&[gtk::accessible::Property::Label(label)]);
    button
}

/// Lay `buttons` out in a grid of `cols` equal columns.
pub fn icon_grid(buttons: Vec<gtk::Button>, cols: u32, spacing: u32) -> gtk::Grid {
    let grid = gtk::Grid::new();
    grid.set_column_homogeneous(true);
    grid.set_row_spacing(spacing);
    grid.set_column_spacing(spacing);
    let cols = cols.max(1) as i32;
    for (i, b) in buttons.iter().enumerate() {
        grid.attach(b, i as i32 % cols, i as i32 / cols, 1, 1);
    }
    // Empty cells keep a short first row at the grid's column width.
    for c in buttons.len() as i32..cols {
        grid.attach(&gtk::Box::new(gtk::Orientation::Vertical, 0), c, 0, 1, 1);
    }
    grid
}

/// A two-line list row: dim key above the value (About, Screen, Network).
pub fn kv_row(key: &str, value: &str) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_activatable(false);
    let b = gtk::Box::new(gtk::Orientation::Vertical, 0);
    b.set_valign(gtk::Align::Center);
    let k = gtk::Label::new(Some(key));
    k.add_css_class("kv-key");
    k.set_xalign(0.0);
    let v = gtk::Label::new(Some(value));
    v.set_xalign(0.0);
    v.set_wrap(true);
    v.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    b.append(&k);
    b.append(&v);
    row.set_child(Some(&b));
    row
}

pub fn icon_image(icon: Option<&gio::Icon>, fallback: &str, px: u32) -> gtk::Image {
    let image = match icon {
        Some(i) => gtk::Image::from_gicon(i),
        None => gtk::Image::from_icon_name(fallback),
    };
    image.set_pixel_size(px as i32);
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stylesheet_uses_tokens() {
        let mut cfg = Config::default();
        cfg.design.font_px = 13;
        cfg.colors.bar_bg = "#123456".into();
        cfg.design.target_px = 37;
        let css = stylesheet(&cfg);
        assert!(css.contains("font-size: 13px"));
        assert!(css.contains("min-height: 37px"));
        assert!(css.contains("linear-gradient(to bottom, #123456"));
        assert!(!css.contains("{font"), "unformatted placeholder left in CSS");
    }

    #[test]
    fn font_family_lists_are_quoted_one_by_one() {
        assert_eq!(font_families("Tahoma, DejaVu Sans"), "\"Tahoma\", \"DejaVu Sans\"");
        assert_eq!(font_families("DejaVu Sans"), "\"DejaVu Sans\"");
    }
}

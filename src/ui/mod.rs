//! GTK user interface of the shell.

pub mod command_bar;
pub mod escape;
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
use std::rc::Rc;

/// Build the application stylesheet from the design tokens.
pub fn stylesheet(cfg: &Config) -> String {
    let d = &cfg.design;
    let c = &cfg.colors;
    format!(
        r#"
window.pda, window.pda * {{
    font-family: "{font}";
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
.dim {{ color: {text_dim}; font-size: {small_px}px; }}
.section-title {{
    font-weight: bold; color: {accent};
    border-bottom: 1px solid {border};
    padding: {pad}px {pad}px 1px {pad}px;
}}

/* Persistent bars */
window.bar {{ background: {bar_bg}; color: {bar_fg}; }}
window.bar button, window.bar menubutton > button {{
    background: transparent; color: {bar_fg}; border: none;
    padding: 0 {pad}px; margin: 0; min-width: {target}px;
}}
window.bar button:hover {{ background: {bar_hover}; }}
window.bar button:active, window.bar button:checked, window.bar button.open {{ background: {bar_pressed}; }}
window.bar label, window.bar image {{ color: {bar_fg}; }}
window.bar .title {{ font-weight: bold; font-size: {title_px}px; }}
window.bar .start {{ font-weight: bold; }}
window.bar separator {{ background: {bar_hover}; min-width: 1px; margin: 3px 1px; }}

/* Today */
window.today {{ background: {surface}; }}
.today-header {{ background: {surface_alt}; border-bottom: 1px solid {border}; padding: {pad}px; }}
.today-header .date {{ font-weight: bold; font-size: {date_px}px; }}
.today-header .time {{ font-size: {date_px}px; }}
.today-header .summary {{ color: {text_dim}; }}
.shortcut {{ padding: {pad}px 1px; border: none; background: transparent; }}
.shortcut image {{ -gtk-icon-size: {today_icon}px; }}
.shortcut:hover {{ background: {surface_alt}; }}
.shortcut:active {{ background: {accent}; color: {accent_fg}; }}

/* Lists (recent documents, Start menu, switcher) */
list.compact, list.compact row {{ background: transparent; }}
list.compact row {{ padding: 1px {pad}px; min-height: {row}px; border-radius: 0; }}
list.targets row {{ min-height: {target}px; }}
list.compact row.destructive label, list.compact row.destructive image {{ color: #b3261e; }}
list.compact row.destructive.armed {{ background: #b3261e; }}
list.compact row.destructive.armed label, list.compact row.destructive.armed image {{ color: #ffffff; }}
list.compact row .active-marker {{ color: {accent}; font-weight: bold; }}
list.compact row.more label {{ color: {accent}; }}
list.compact row:hover {{ background: {surface_alt}; }}
list.compact row:selected, list.compact row:active {{ background: {accent}; color: {accent_fg}; }}
list.compact row:selected .dim, list.compact row:active .dim {{ color: {accent_fg}; }}
list.compact row:focus-visible {{ outline: 1px solid {focus}; }}

/* Start menu */
window.start-menu {{ background: {surface}; border-right: 1px solid {border}; border-bottom: 1px solid {border}; }}
window.start-menu entry {{ min-height: 0; padding: 1px {pad}px; margin: {pad}px; border-radius: {radius}px; }}
.system-actions {{ border-top: 1px solid {border}; background: {surface_alt}; }}
.system-actions button {{ background: transparent; border: none; }}

/* Popovers. Without a compositor the shadow area would render black, so
   popovers are flat with a plain border. Labels inside popovers opened from
   a bar must not inherit the bar's light text colour. */
popover.pda, popover.pda > contents {{ box-shadow: none; }}
popover.pda > contents {{
    padding: 2px; border-radius: {radius}px;
    border: 1px solid {border}; background: {surface};
}}
popover.pda, popover.pda * {{ font-family: "{font}"; font-size: {font_px}px; }}
window.bar popover.pda label, window.bar popover.pda image {{ color: {text}; }}
window.bar popover.pda row:selected label, window.bar popover.pda row:active label {{ color: {accent_fg}; }}
popover.pda modelbutton {{ min-height: {target}px; padding: 0 {pad}px; }}
popover.pda separator {{ margin: 2px 0; }}

/* Exit full screen: unmanaged, above full-screen programs */
window.escape {{ background: {bar_bg}; }}
window.escape button {{ background: {bar_bg}; color: {bar_fg}; border: 1px solid {bar_fg}; padding: 0 {pad}px; }}
window.escape button:active {{ background: {bar_pressed}; }}
window.escape label, window.escape image {{ color: {bar_fg}; }}
"#,
        font = d.font_family,
        font_px = d.font_px,
        small_px = d.small_font_px,
        title_px = d.title_font_px,
        date_px = d.date_font_px,
        row = d.row_px,
        target = d.target_px,
        pad = d.padding_px,
        radius = d.radius_px,
        icon = d.icon_px,
        today_icon = d.today_icon_px,
        surface = c.surface,
        surface_alt = c.surface_alt,
        text = c.text,
        text_dim = c.text_dim,
        accent = c.accent,
        accent_fg = c.accent_fg,
        border = c.border,
        focus = c.focus,
        bar_bg = c.bar_bg,
        bar_fg = c.bar_fg,
        bar_hover = c.bar_hover,
        bar_pressed = c.bar_pressed,
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
    }
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
        assert!(css.contains("background: #123456"));
        assert!(!css.contains("{font"), "unformatted placeholder left in CSS");
    }
}

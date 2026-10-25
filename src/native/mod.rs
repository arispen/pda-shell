//! First-party programs (P2 of issue #4): Pocket PC style frontends that run
//! as ordinary separate programs next to the Linux desktop programs, in the
//! same window lifecycle (maximized by the window manager, listed in the
//! switcher, closed with Close).
//!
//! They have no menu bar or toolbar of their own. Their commands are a
//! GtkApplication menu bar, which GTK publishes on the session bus and
//! announces in `_GTK_*` window properties; the shell's command bar shows
//! it. That is the whole command-bar API: any GtkApplication that sets a
//! menu bar gets its commands into the bar the same way.

pub mod files;
pub mod notes;

use crate::config::Config;
use crate::input::StylusFeedback;
use gtk::gio;
use gtk::prelude::*;

/// Start a native program: stylesheet and icons shared with the shell.
pub fn run(app_id: &str, prgname: &str, build: impl Fn(&gtk::Application, &Config, Vec<String>) + 'static) -> glib::ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    glib::set_prgname(Some(prgname));
    std::env::set_var("GDK_BACKEND", "x11");
    let cfg = Config::load();
    let app = gtk::Application::builder()
        .application_id(app_id)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let c = cfg.clone();
    app.connect_startup(move |_| crate::ui::install_stylesheet(&c));
    app.connect_command_line(move |app, cmd| {
        let args: Vec<String> = cmd.arguments().iter().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
        build(app, &cfg, args);
        glib::ExitCode::SUCCESS
    });
    app.run()
}

/// A program window: shell classes for the shared stylesheet, an overlay
/// for the long-press ring, and no menu bar of its own (the command bar
/// shows it).
pub struct NativeWindow {
    pub window: gtk::ApplicationWindow,
    pub overlay: gtk::Overlay,
    pub feedback: StylusFeedback,
}

impl NativeWindow {
    pub fn new(app: &gtk::Application, title: &str, icon: &str, cfg: &Config) -> Self {
        let window = gtk::ApplicationWindow::builder().application(app).title(title).icon_name(icon).build();
        window.set_show_menubar(false);
        window.add_css_class("pda");
        window.add_css_class("native");
        let overlay = gtk::Overlay::new();
        let feedback = StylusFeedback::new(cfg);
        overlay.add_overlay(feedback.widget());
        window.set_child(Some(&overlay));
        Self { window, overlay, feedback }
    }
}

/// A strip above the content that asks for one thing: a name (with an
/// entry) or a confirmation. Stylus-friendly replacement for dialogs, which
/// would be separate windows the size of a desktop.
#[derive(Clone)]
pub struct AskBar {
    pub widget: gtk::Box,
    question: gtk::Label,
    entry: gtk::Entry,
    ok: gtk::Button,
    on_ok: std::rc::Rc<std::cell::RefCell<Option<Box<dyn Fn(String)>>>>,
}

impl AskBar {
    pub fn new() -> Self {
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 4);
        widget.add_css_class("ask-bar");
        widget.set_visible(false);
        let question = gtk::Label::new(None);
        question.set_xalign(0.0);
        question.set_wrap(true);
        let entry = gtk::Entry::new();
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        buttons.set_halign(gtk::Align::End);
        let ok = gtk::Button::with_label("OK");
        let cancel = gtk::Button::with_label("Cancel");
        buttons.append(&cancel);
        buttons.append(&ok);
        widget.append(&question);
        widget.append(&entry);
        widget.append(&buttons);
        let bar = Self { widget, question, entry, ok, on_ok: Default::default() };
        let b = bar.clone();
        cancel.connect_clicked(move |_| b.hide());
        let b = bar.clone();
        bar.ok.connect_clicked(move |_| b.accept());
        let b = bar.clone();
        bar.entry.connect_activate(move |_| b.accept());
        bar
    }

    fn accept(&self) {
        let text = self.entry.text().trim().to_string();
        if WidgetExt::is_visible(&self.entry) && text.is_empty() {
            return;
        }
        let f = self.on_ok.borrow_mut().take();
        self.hide();
        if let Some(f) = f {
            f(text);
        }
    }

    pub fn hide(&self) {
        self.widget.set_visible(false);
        self.on_ok.borrow_mut().take();
    }

    /// Ask for a name; `on_ok` gets the trimmed, non-empty text.
    pub fn ask_name(&self, question: &str, initial: &str, ok_label: &str, on_ok: impl Fn(String) + 'static) {
        self.question.set_text(question);
        self.entry.set_visible(true);
        self.entry.set_text(initial);
        self.ok.set_label(ok_label);
        *self.on_ok.borrow_mut() = Some(Box::new(on_ok));
        self.widget.set_visible(true);
        self.entry.grab_focus();
        self.entry.select_region(0, -1);
    }

    /// Ask for a confirmation.
    pub fn confirm(&self, question: &str, ok_label: &str, on_ok: impl Fn() + 'static) {
        self.question.set_text(question);
        self.entry.set_visible(false);
        self.ok.set_label(ok_label);
        *self.on_ok.borrow_mut() = Some(Box::new(move |_| on_ok()));
        self.widget.set_visible(true);
    }
}

impl Default for AskBar {
    fn default() -> Self {
        Self::new()
    }
}

/// A plain window action (no parameter).
pub fn action(map: &impl IsA<gio::ActionMap>, name: &str, f: impl Fn() + 'static) -> gio::SimpleAction {
    let a = gio::SimpleAction::new(name, None);
    a.connect_activate(move |_, _| f());
    map.add_action(&a);
    a
}

/// "12 Oct 14:05" for a modification time.
pub fn short_time(unix: i64) -> String {
    glib::DateTime::from_unix_local(unix)
        .ok()
        .and_then(|t| t.format("%d %b %H:%M").ok())
        .map(|s| s.to_string())
        .unwrap_or_default()
}

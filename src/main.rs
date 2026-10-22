use gtk::gio;
use gtk::prelude::*;
use pda_shell::config::Config;
use pda_shell::shell::Shell;
use std::cell::RefCell;
use std::rc::Rc;

fn main() -> glib::ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    glib::set_prgname(Some("pda-shell"));
    glib::set_application_name("pda-shell");
    // The shell relies on X11 (EWMH struts, window types); never let GTK
    // pick another backend.
    std::env::set_var("GDK_BACKEND", "x11");
    // Programs inherit the working directory; start them in the user's home.
    if let Some(home) = std::env::var_os("HOME") {
        let _ = std::env::set_current_dir(home);
    }

    let cfg = Config::load();
    let app = gtk::Application::builder()
        .application_id("io.github.pda_shell.Shell")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();

    let shell: Rc<RefCell<Option<Rc<Shell>>>> = Rc::default();
    let keep = shell.clone();
    app.connect_activate(move |app| {
        if keep.borrow().is_some() {
            return;
        }
        match Shell::start(app, cfg.clone()) {
            Ok(s) => *keep.borrow_mut() = Some(s),
            Err(e) => {
                log::error!("cannot start shell: {e}");
                app.quit();
            }
        }
    });
    let code = app.run_with_args::<&str>(&[]);
    shell.borrow_mut().take();
    code
}

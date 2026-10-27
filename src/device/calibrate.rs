//! `pda-device calibrate`: tap five crosses, save the touchscreen
//! calibration for the stylus daemon.
//!
//! The calibration is switched off while measuring (the saved file is set
//! to the identity and the daemon reloads it with SIGHUP), so each tap
//! lands where the uncalibrated touchscreen puts it, with only the
//! session's rotation applied. The fitted calibration is saved to
//! `~/.config/pda-shell/stylus-calibration` and the daemon reloads again;
//! Cancel puts the previous file back.

use super::display::Rotation;
use super::stylus::{self, Affine, CALIBRATION_TARGETS};
use gtk::prelude::*;
use std::cell::RefCell;
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;

/// Remaining error (normalized) above which the taps are asked again.
const MAX_ERROR: f64 = 0.03;

fn reload_daemon() {
    // The daemon runs as `pda-device stylus` in the same session.
    let _ = Command::new("pkill").args(["-HUP", "-f", "pda-device stylus"]).status();
}

fn write(path: &PathBuf, m: &Affine) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let [a, b, c, d, e, f] = m.0;
    std::fs::write(path, format!("{a:.6} {b:.6} {c:.6}\n{d:.6} {e:.6} {f:.6}\n"))
}

struct State {
    step: usize,
    measured: Vec<(f64, f64)>,
    message: String,
    done: bool,
}

pub fn run(rotation: Rotation) -> glib::ExitCode {
    let Some(path) = stylus::calibration_file() else {
        eprintln!("pda-device: no HOME; cannot save a calibration");
        return glib::ExitCode::FAILURE;
    };
    let previous = std::fs::read_to_string(&path).ok();
    if let Err(e) = write(&path, &Affine::IDENTITY) {
        eprintln!("pda-device: {}: {e}", path.display());
        return glib::ExitCode::FAILURE;
    }
    reload_daemon();

    let app = gtk::Application::builder()
        .application_id("io.github.pda_shell.Calibrate")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    // Stopped from outside (session ending): quit normally, so the previous
    // calibration is put back below instead of the identity staying.
    for sig in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
        let app = app.clone();
        glib::unix_signal_add_local(sig, move || {
            app.quit();
            glib::ControlFlow::Break
        });
    }
    let saved = Rc::new(RefCell::new(false));
    let saved_in = saved.clone();
    let path_in = path.clone();
    app.connect_activate(move |app| {
        let win = gtk::ApplicationWindow::builder().application(app).title("Calibrate stylus").build();
        let state = Rc::new(RefCell::new(State {
            step: 0,
            measured: Vec::new(),
            message: "Tap the centre of each cross with the stylus.".into(),
            done: false,
        }));
        let area = gtk::DrawingArea::new();
        area.set_hexpand(true);
        area.set_vexpand(true);
        let st = state.clone();
        area.set_draw_func(move |_, cr, w, h| {
            let s = st.borrow();
            cr.set_source_rgb(1.0, 1.0, 1.0);
            let _ = cr.paint();
            cr.set_source_rgb(0.0, 0.0, 0.0);
            cr.set_font_size(22.0);
            let lines = [s.message.as_str(), if s.done { "" } else { "Short taps: holding is not needed." }];
            for (i, line) in lines.iter().enumerate() {
                let ext = cr.text_extents(line).ok();
                let tw = ext.map_or(0.0, |e| e.width());
                cr.move_to((w as f64 - tw) / 2.0, h as f64 * 0.3 + i as f64 * 30.0);
                let _ = cr.show_text(line);
            }
            if s.done {
                return;
            }
            let (tx, ty) = CALIBRATION_TARGETS[s.step];
            let (x, y) = (tx * w as f64, ty * h as f64);
            cr.set_source_rgb(0.85, 0.1, 0.1);
            cr.set_line_width(2.0);
            cr.move_to(x - 20.0, y);
            cr.line_to(x + 20.0, y);
            cr.move_to(x, y - 20.0);
            cr.line_to(x, y + 20.0);
            let _ = cr.stroke();
            cr.arc(x, y, 8.0, 0.0, std::f64::consts::TAU);
            let _ = cr.stroke();
            cr.set_source_rgb(0.3, 0.3, 0.3);
            let label = format!("{} / {}", s.step + 1, CALIBRATION_TARGETS.len());
            cr.move_to(w as f64 / 2.0 - 20.0, h as f64 * 0.3 + 70.0);
            let _ = cr.show_text(&label);
        });

        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        buttons.set_halign(gtk::Align::Center);
        // Below the centre cross, between the bottom corner crosses.
        buttons.set_valign(gtk::Align::End);
        buttons.set_margin_bottom(100);
        let cancel = gtk::Button::with_label("Cancel");
        let again = gtk::Button::with_label("Again");
        let close = gtk::Button::with_label("Close");
        for b in [&cancel, &again, &close] {
            b.set_size_request(120, 46);
            buttons.append(b);
        }
        again.set_visible(false);
        close.set_visible(false);
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&area));
        overlay.add_overlay(&buttons);
        win.set_child(Some(&overlay));

        let click = gtk::GestureClick::new();
        click.set_button(0);
        let (st, area_c, again_c, close_c, cancel_c) =
            (state.clone(), area.clone(), again.clone(), close.clone(), cancel.clone());
        let (path_c, saved_c) = (path_in.clone(), saved_in.clone());
        click.connect_pressed(move |_, _, x, y| {
            let mut s = st.borrow_mut();
            if s.done {
                return;
            }
            let (w, h) = (area_c.width().max(1) as f64, area_c.height().max(1) as f64);
            s.measured.push((x / w, y / h));
            s.step += 1;
            if s.step == CALIBRATION_TARGETS.len() {
                s.done = true;
                match stylus::calibrate(rotation, &s.measured, &CALIBRATION_TARGETS) {
                    Some((c, err)) if err <= MAX_ERROR => match write(&path_c, &c) {
                        Ok(()) => {
                            reload_daemon();
                            *saved_c.borrow_mut() = true;
                            log::info!("calibration saved: {:?}, error {:.1} %", c.0, err * 100.0);
                            println!("{:?}", c.0);
                            s.message = format!("Saved (largest error {:.1} % of the screen).", err * 100.0);
                        }
                        Err(e) => s.message = format!("Cannot save: {e}"),
                    },
                    Some((_, err)) => {
                        s.message = format!("The taps do not agree (error {:.0} %). Try again.", err * 100.0);
                    }
                    None => s.message = "The taps do not agree. Try again.".into(),
                }
                again_c.set_visible(true);
                close_c.set_visible(*saved_c.borrow());
                cancel_c.set_visible(!*saved_c.borrow());
            }
            drop(s);
            area_c.queue_draw();
        });
        area.add_controller(click);

        let (st, area_c, again_c, close_c, cancel_c, path_c, saved_c) =
            (state.clone(), area.clone(), again.clone(), close.clone(), cancel.clone(), path_in.clone(), saved_in.clone());
        again.connect_clicked(move |_| {
            let _ = write(&path_c, &Affine::IDENTITY);
            reload_daemon();
            *saved_c.borrow_mut() = false;
            *st.borrow_mut() = State {
                step: 0,
                measured: Vec::new(),
                message: "Tap the centre of each cross with the stylus.".into(),
                done: false,
            };
            again_c.set_visible(false);
            close_c.set_visible(false);
            cancel_c.set_visible(true);
            area_c.queue_draw();
        });
        let w = win.clone();
        close.connect_clicked(move |_| w.close());
        let w = win.clone();
        cancel.connect_clicked(move |_| w.close());
        win.fullscreen();
        win.present();
    });
    let code = app.run_with_args::<&str>(&[]);
    if !*saved.borrow() {
        // Cancelled or closed: the previous calibration comes back.
        let restored = match &previous {
            Some(text) => std::fs::write(&path, text),
            None => std::fs::remove_file(&path),
        };
        if let Err(e) = restored {
            eprintln!("pda-device: cannot restore {}: {e}", path.display());
        }
        reload_daemon();
    }
    code
}

//! Stylus input inside the shell's own windows.
//!
//! [`gesture`] holds the toolkit-independent recognizer. This module feeds it
//! from GTK events and draws the Pocket PC style "ring of dots" while a long
//! press is building up. System-wide translation for third-party
//! applications is a separate, future component (see docs/architecture.md).

pub mod gesture;

use crate::config::Config;
use gesture::{Gesture, GestureConfig, Recognizer};
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const DOTS: usize = 8;
const RING_RADIUS: f64 = 14.0;
const DOT_RADIUS: f64 = 2.2;
const TICK_MS: u64 = 20;

fn now_ms() -> u64 {
    (glib::monotonic_time() / 1000) as u64
}

fn parse_rgb(hex: &str) -> (f64, f64, f64) {
    let h = hex.trim_start_matches('#');
    let c = |i: usize| u8::from_str_radix(h.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0) as f64 / 255.0;
    (c(0), c(2), c(4))
}

/// Overlay that shows long-press progress at the stylus position.
#[derive(Clone)]
pub struct StylusFeedback {
    area: gtk::DrawingArea,
    state: Rc<Cell<Option<(f64, f64, f64)>>>,
}

impl StylusFeedback {
    pub fn new(cfg: &Config) -> Self {
        let area = gtk::DrawingArea::new();
        area.set_can_target(false);
        area.set_hexpand(true);
        area.set_vexpand(true);
        let state: Rc<Cell<Option<(f64, f64, f64)>>> = Rc::new(Cell::new(None));
        let (r, g, b) = parse_rgb(&cfg.colors.focus);
        let s = state.clone();
        area.set_draw_func(move |_, cr, _, _| {
            let Some((x, y, fraction)) = s.get() else { return };
            let lit = ((fraction * DOTS as f64).ceil() as usize).clamp(1, DOTS);
            for i in 0..lit {
                let angle = -std::f64::consts::FRAC_PI_2 + i as f64 * std::f64::consts::TAU / DOTS as f64;
                cr.arc(x + RING_RADIUS * angle.cos(), y + RING_RADIUS * angle.sin(), DOT_RADIUS, 0.0, std::f64::consts::TAU);
                cr.set_source_rgb(r, g, b);
                let _ = cr.fill();
            }
        });
        Self { area, state }
    }

    pub fn widget(&self) -> &gtk::DrawingArea {
        &self.area
    }

    fn show(&self, from: &gtk::Widget, x: f64, y: f64, fraction: f64) {
        let p = from
            .compute_point(&self.area, &gtk::graphene::Point::new(x as f32, y as f32))
            .map(|p| (p.x() as f64, p.y() as f64))
            .unwrap_or((x, y));
        self.state.set(Some((p.0, p.1, fraction)));
        self.area.queue_draw();
    }

    fn hide(&self) {
        if self.state.replace(None).is_some() {
            self.area.queue_draw();
        }
    }
}

/// Give `widget` stylus long-press behaviour: after the configured hold time
/// (with progress feedback) `on_long_press` runs and the press is claimed so
/// the widget's own click handler does not also fire. A mouse right-click
/// does the same, which keeps development with a mouse natural.
pub fn attach_long_press<F>(widget: &gtk::Widget, feedback: &StylusFeedback, cfg: &Config, on_long_press: F)
where
    F: Fn(f64, f64) + 'static,
{
    let on_long_press = Rc::new(on_long_press);
    let rec = Rc::new(RefCell::new(Recognizer::new(GestureConfig::from(&cfg.input))));
    let origin = Rc::new(Cell::new((0.0, 0.0)));
    let timer: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));

    let drag = gtk::GestureDrag::new();
    drag.set_button(gtk::gdk::BUTTON_PRIMARY);
    drag.set_propagation_phase(gtk::PropagationPhase::Capture);

    let handle = {
        let feedback = feedback.clone();
        let widget = widget.clone();
        let on_long_press = on_long_press.clone();
        let timer = timer.clone();
        Rc::new(move |drag: &gtk::GestureDrag, out: Vec<Gesture>| {
            for g in out {
                match g {
                    Gesture::LongPressProgress { x, y, fraction } => feedback.show(&widget, x, y, fraction),
                    Gesture::LongPress { x, y } => {
                        feedback.hide();
                        stop(&timer);
                        drag.set_state(gtk::EventSequenceState::Claimed);
                        on_long_press(x, y);
                    }
                    Gesture::LongPressCancelled | Gesture::DragEnd { .. } => feedback.hide(),
                    // Taps and drags are left to the widget's own handlers.
                    _ => {}
                }
            }
        })
    };

    {
        let rec = rec.clone();
        let origin = origin.clone();
        let timer = timer.clone();
        let handle = handle.clone();
        drag.connect_drag_begin(move |d, x, y| {
            origin.set((x, y));
            let out = rec.borrow_mut().press(now_ms(), x, y);
            handle(d, out);
            let (rec, handle, d) = (rec.clone(), handle.clone(), d.clone());
            let id = glib::timeout_add_local(std::time::Duration::from_millis(TICK_MS), move || {
                let mut r = rec.borrow_mut();
                if !r.is_pressed() {
                    return glib::ControlFlow::Break;
                }
                let out = r.tick(now_ms());
                drop(r);
                let fired = out.iter().any(|g| matches!(g, Gesture::LongPress { .. }));
                handle(&d, out);
                if fired {
                    glib::ControlFlow::Break
                } else {
                    glib::ControlFlow::Continue
                }
            });
            if let Some(old) = timer.borrow_mut().replace(id) {
                old.remove();
            }
        });
    }
    {
        let (rec, origin, handle) = (rec.clone(), origin.clone(), handle.clone());
        drag.connect_drag_update(move |d, dx, dy| {
            let (x0, y0) = origin.get();
            let out = rec.borrow_mut().motion(now_ms(), x0 + dx, y0 + dy);
            handle(d, out);
        });
    }
    {
        let (rec, origin, handle, timer) = (rec.clone(), origin.clone(), handle.clone(), timer.clone());
        drag.connect_drag_end(move |d, dx, dy| {
            let (x0, y0) = origin.get();
            let out = rec.borrow_mut().release(now_ms(), x0 + dx, y0 + dy);
            stop(&timer);
            handle(d, out);
        });
    }
    {
        let (rec, feedback, timer) = (rec.clone(), feedback.clone(), timer.clone());
        drag.connect_cancel(move |_, _| {
            rec.borrow_mut().cancel();
            stop(&timer);
            feedback.hide();
        });
    }
    widget.add_controller(drag);

    let right = gtk::GestureClick::new();
    right.set_button(gtk::gdk::BUTTON_SECONDARY);
    right.connect_pressed(move |g, _, x, y| {
        g.set_state(gtk::EventSequenceState::Claimed);
        on_long_press(x, y);
    });
    widget.add_controller(right);
}

fn stop(timer: &Rc<RefCell<Option<glib::SourceId>>>) {
    // The source may already have finished by returning Break.
    if let Some(id) = timer.borrow_mut().take() {
        if glib::MainContext::default().find_source_by_id(&id).is_some() {
            id.remove();
        }
    }
}

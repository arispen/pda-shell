//! Single-point stylus gesture recognition.
//!
//! Input is a stream of press / motion / release samples plus periodic ticks
//! while the stylus is down; output is tap, double tap, drag and long press.
//! It has no GTK or X11 dependency so the same recognizer can later sit in a
//! system-wide input translation layer fed by the resistive touchscreen.

use crate::config::InputConfig;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GestureConfig {
    pub long_press_ms: u64,
    pub slop_px: f64,
    pub double_tap_ms: u64,
    pub double_tap_slop_px: f64,
    /// Long-press progress is only reported after this much holding, so
    /// ordinary taps do not flash the indicator.
    pub feedback_delay_ms: u64,
}

impl From<&InputConfig> for GestureConfig {
    fn from(c: &InputConfig) -> Self {
        Self {
            long_press_ms: c.long_press_ms as u64,
            slop_px: c.long_press_slop_px,
            double_tap_ms: c.double_tap_ms as u64,
            double_tap_slop_px: c.double_tap_slop_px,
            feedback_delay_ms: (c.feedback_delay_ms as u64).min(c.long_press_ms as u64),
        }
    }
}

impl Default for GestureConfig {
    fn default() -> Self {
        (&InputConfig::default()).into()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Gesture {
    /// Behaves as a left click.
    Tap { x: f64, y: f64 },
    /// Second tap of a double click; always preceded by a `Tap`.
    DoubleTap { x: f64, y: f64 },
    DragBegin { x: f64, y: f64 },
    DragUpdate { x: f64, y: f64 },
    DragEnd { x: f64, y: f64 },
    /// Long-press progress in `0.0..=1.0` for visual feedback.
    LongPressProgress { x: f64, y: f64, fraction: f64 },
    /// The press was held long enough: behaves as a right click.
    LongPress { x: f64, y: f64 },
    /// Progress feedback was shown but the press turned into something else.
    LongPressCancelled,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    Idle,
    Pressed { t0: u64, x0: f64, y0: f64, feedback: bool },
    Dragging,
    /// Long press fired; ignore everything until release.
    Consumed,
}

#[derive(Debug, Clone)]
pub struct Recognizer {
    cfg: GestureConfig,
    state: State,
    last_tap: Option<(u64, f64, f64)>,
}

fn dist(ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    ((ax - bx).powi(2) + (ay - by).powi(2)).sqrt()
}

impl Recognizer {
    pub fn new(cfg: GestureConfig) -> Self {
        Self { cfg, state: State::Idle, last_tap: None }
    }

    pub fn config(&self) -> &GestureConfig {
        &self.cfg
    }

    pub fn is_pressed(&self) -> bool {
        !matches!(self.state, State::Idle)
    }

    pub fn press(&mut self, t: u64, x: f64, y: f64) -> Vec<Gesture> {
        // A press while already down means we missed a release; start over.
        self.state = State::Pressed { t0: t, x0: x, y0: y, feedback: false };
        Vec::new()
    }

    pub fn motion(&mut self, t: u64, x: f64, y: f64) -> Vec<Gesture> {
        match self.state {
            State::Pressed { x0, y0, feedback, .. } => {
                if dist(x, y, x0, y0) > self.cfg.slop_px {
                    self.state = State::Dragging;
                    self.last_tap = None;
                    let mut out = Vec::new();
                    if feedback {
                        out.push(Gesture::LongPressCancelled);
                    }
                    out.push(Gesture::DragBegin { x: x0, y: y0 });
                    out.push(Gesture::DragUpdate { x, y });
                    out
                } else {
                    self.tick(t)
                }
            }
            State::Dragging => vec![Gesture::DragUpdate { x, y }],
            State::Idle | State::Consumed => Vec::new(),
        }
    }

    pub fn release(&mut self, t: u64, x: f64, y: f64) -> Vec<Gesture> {
        let prev = std::mem::replace(&mut self.state, State::Idle);
        match prev {
            State::Pressed { t0, x0, y0, feedback } => {
                // The timer may not have ticked exactly at the threshold.
                if t.saturating_sub(t0) >= self.cfg.long_press_ms {
                    self.last_tap = None;
                    return vec![Gesture::LongPress { x: x0, y: y0 }];
                }
                let mut out = Vec::new();
                if feedback {
                    out.push(Gesture::LongPressCancelled);
                }
                out.push(Gesture::Tap { x, y });
                match self.last_tap {
                    Some((lt, lx, ly))
                        if t.saturating_sub(lt) <= self.cfg.double_tap_ms
                            && dist(x, y, lx, ly) <= self.cfg.double_tap_slop_px =>
                    {
                        out.push(Gesture::DoubleTap { x, y });
                        self.last_tap = None;
                    }
                    _ => self.last_tap = Some((t, x, y)),
                }
                out
            }
            State::Dragging => vec![Gesture::DragEnd { x, y }],
            State::Idle | State::Consumed => Vec::new(),
        }
    }

    /// Call periodically (e.g. every 16-30 ms) while pressed.
    pub fn tick(&mut self, t: u64) -> Vec<Gesture> {
        let State::Pressed { t0, x0, y0, feedback } = self.state else {
            return Vec::new();
        };
        let held = t.saturating_sub(t0);
        if held >= self.cfg.long_press_ms {
            self.state = State::Consumed;
            self.last_tap = None;
            return vec![Gesture::LongPress { x: x0, y: y0 }];
        }
        if held >= self.cfg.feedback_delay_ms {
            if !feedback {
                self.state = State::Pressed { t0, x0, y0, feedback: true };
            }
            let span = (self.cfg.long_press_ms - self.cfg.feedback_delay_ms).max(1) as f64;
            let fraction = ((held - self.cfg.feedback_delay_ms) as f64 / span).clamp(0.0, 1.0);
            return vec![Gesture::LongPressProgress { x: x0, y: y0, fraction }];
        }
        Vec::new()
    }

    /// Abandon the current interaction (e.g. the widget was hidden).
    pub fn cancel(&mut self) -> Vec<Gesture> {
        let prev = std::mem::replace(&mut self.state, State::Idle);
        match prev {
            State::Pressed { feedback: true, .. } => vec![Gesture::LongPressCancelled],
            State::Dragging => vec![Gesture::DragEnd { x: f64::NAN, y: f64::NAN }],
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec() -> Recognizer {
        Recognizer::new(GestureConfig {
            long_press_ms: 600,
            slop_px: 8.0,
            double_tap_ms: 400,
            double_tap_slop_px: 10.0,
            feedback_delay_ms: 150,
        })
    }

    fn is_tap(g: &[Gesture]) -> bool {
        matches!(g, [Gesture::Tap { .. }])
    }

    #[test]
    fn quick_press_release_is_a_tap() {
        let mut r = rec();
        assert!(r.press(0, 10.0, 10.0).is_empty());
        assert!(r.tick(50).is_empty());
        assert_eq!(r.release(80, 11.0, 10.0), vec![Gesture::Tap { x: 11.0, y: 10.0 }]);
        assert!(!r.is_pressed());
    }

    #[test]
    fn jitter_within_slop_is_still_a_tap() {
        let mut r = rec();
        r.press(0, 10.0, 10.0);
        assert!(r.motion(20, 15.0, 14.0).is_empty()); // 6.4 px
        assert!(is_tap(&r.release(60, 15.0, 14.0)));
    }

    #[test]
    fn two_taps_close_together_are_a_double_tap() {
        let mut r = rec();
        r.press(0, 10.0, 10.0);
        r.release(60, 10.0, 10.0);
        r.press(200, 12.0, 11.0);
        assert_eq!(
            r.release(260, 12.0, 11.0),
            vec![Gesture::Tap { x: 12.0, y: 11.0 }, Gesture::DoubleTap { x: 12.0, y: 11.0 }]
        );
        // A third tap starts a new sequence rather than another double tap.
        r.press(300, 12.0, 11.0);
        assert!(is_tap(&r.release(340, 12.0, 11.0)));
    }

    #[test]
    fn slow_second_tap_is_not_a_double_tap() {
        let mut r = rec();
        r.press(0, 10.0, 10.0);
        r.release(60, 10.0, 10.0);
        r.press(500, 10.0, 10.0);
        assert!(is_tap(&r.release(560, 10.0, 10.0)));
    }

    #[test]
    fn distant_second_tap_is_not_a_double_tap() {
        let mut r = rec();
        r.press(0, 10.0, 10.0);
        r.release(60, 10.0, 10.0);
        r.press(100, 40.0, 10.0);
        assert!(is_tap(&r.release(160, 40.0, 10.0)));
    }

    #[test]
    fn moving_beyond_slop_starts_a_drag() {
        let mut r = rec();
        r.press(0, 10.0, 10.0);
        assert_eq!(
            r.motion(30, 30.0, 10.0),
            vec![Gesture::DragBegin { x: 10.0, y: 10.0 }, Gesture::DragUpdate { x: 30.0, y: 10.0 }]
        );
        assert_eq!(r.motion(40, 40.0, 12.0), vec![Gesture::DragUpdate { x: 40.0, y: 12.0 }]);
        // A drag never turns into a long press, however long it lasts.
        assert!(r.tick(2000).is_empty());
        assert_eq!(r.release(2010, 50.0, 12.0), vec![Gesture::DragEnd { x: 50.0, y: 12.0 }]);
    }

    #[test]
    fn holding_reports_progress_then_long_press() {
        let mut r = rec();
        r.press(0, 5.0, 6.0);
        assert!(r.tick(100).is_empty(), "no feedback before the delay");
        match r.tick(375).as_slice() {
            [Gesture::LongPressProgress { fraction, .. }] => assert!((fraction - 0.5).abs() < 1e-9),
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(r.tick(600), vec![Gesture::LongPress { x: 5.0, y: 6.0 }]);
        // Release after a long press does not also click.
        assert!(r.release(700, 5.0, 6.0).is_empty());
    }

    #[test]
    fn moving_during_long_press_cancels_it() {
        let mut r = rec();
        r.press(0, 0.0, 0.0);
        r.tick(300);
        assert_eq!(
            r.motion(320, 20.0, 0.0),
            vec![
                Gesture::LongPressCancelled,
                Gesture::DragBegin { x: 0.0, y: 0.0 },
                Gesture::DragUpdate { x: 20.0, y: 0.0 }
            ]
        );
        assert!(r.tick(900).is_empty());
    }

    #[test]
    fn early_release_after_feedback_cancels_and_taps() {
        let mut r = rec();
        r.press(0, 0.0, 0.0);
        r.tick(300);
        assert_eq!(r.release(320, 0.0, 0.0), vec![Gesture::LongPressCancelled, Gesture::Tap { x: 0.0, y: 0.0 }]);
    }

    #[test]
    fn late_release_without_tick_still_long_presses() {
        let mut r = rec();
        r.press(0, 1.0, 1.0);
        assert_eq!(r.release(650, 1.0, 1.0), vec![Gesture::LongPress { x: 1.0, y: 1.0 }]);
    }

    #[test]
    fn long_press_does_not_count_toward_double_tap() {
        let mut r = rec();
        r.press(0, 1.0, 1.0);
        r.release(50, 1.0, 1.0);
        r.press(100, 1.0, 1.0);
        r.tick(700);
        r.release(710, 1.0, 1.0);
        r.press(750, 1.0, 1.0);
        assert!(is_tap(&r.release(800, 1.0, 1.0)));
    }

    #[test]
    fn threshold_is_configurable() {
        let mut cfg = GestureConfig::default();
        cfg.long_press_ms = 1000;
        let mut r = Recognizer::new(cfg);
        r.press(0, 0.0, 0.0);
        assert!(!r.tick(700).contains(&Gesture::LongPress { x: 0.0, y: 0.0 }));
        assert_eq!(r.tick(1000), vec![Gesture::LongPress { x: 0.0, y: 0.0 }]);
    }

    #[test]
    fn cancel_resets() {
        let mut r = rec();
        r.press(0, 0.0, 0.0);
        r.tick(200);
        assert_eq!(r.cancel(), vec![Gesture::LongPressCancelled]);
        assert!(!r.is_pressed());
        assert!(r.release(300, 0.0, 0.0).is_empty());
    }

    #[test]
    fn config_conversion_clamps_feedback_delay() {
        let ic = InputConfig { long_press_ms: 100, feedback_delay_ms: 500, ..InputConfig::default() };
        let gc = GestureConfig::from(&ic);
        assert_eq!(gc.feedback_delay_ms, 100);
    }
}

//! System-wide stylus translation: the touchscreen's evdev events in,
//! pointer events of a virtual absolute pointer out, with a held stylus
//! turned into a right click.
//!
//! The decisions are pure (no file descriptors), so every event sequence
//! can be replayed in tests; `evdev.rs` connects them to the kernel. The
//! touch-down is held back until the recognizer knows what it is:
//!
//! - released within the hold time and the slop: a left click (tap);
//! - moved beyond the slop: left button down at the press position, then
//!   the motion (drag, selection, scrolling);
//! - held still for the hold time: a right click at the press position, and
//!   nothing else until the stylus is lifted (no left click afterwards).

use crate::config::StylusConfig;
use crate::device::display::Rotation;
use crate::input::gesture::{Gesture, GestureConfig, Recognizer};

/// A 2×3 affine matrix on normalized coordinates (0..1), in libinput's
/// calibration-matrix order: x' = a·x + b·y + c, y' = d·x + e·y + f.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine(pub [f64; 6]);

impl Affine {
    pub const IDENTITY: Affine = Affine([1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);

    /// The matrix that follows an `xrandr --rotate` of the picture, so a
    /// touch lands on what is drawn under it.
    pub fn rotation(r: Rotation) -> Affine {
        Affine(match r {
            Rotation::Normal => [1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            Rotation::Left => [0.0, -1.0, 1.0, 1.0, 0.0, 0.0],
            Rotation::Inverted => [-1.0, 0.0, 1.0, 0.0, -1.0, 1.0],
            Rotation::Right => [0.0, 1.0, 0.0, -1.0, 0.0, 1.0],
        })
    }

    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        let [a, b, c, d, e, f] = self.0;
        (a * x + b * y + c, d * x + e * y + f)
    }

    /// `self` after `first`.
    pub fn after(&self, first: &Affine) -> Affine {
        let [a, b, c, d, e, f] = self.0;
        let [p, q, r, s, t, u] = first.0;
        Affine([a * p + b * s, a * q + b * t, a * r + b * u + c, d * p + e * s, d * q + e * t, d * r + e * u + f])
    }

    pub fn inverse(&self) -> Option<Affine> {
        let [a, b, c, d, e, f] = self.0;
        let det = a * e - b * d;
        if det.abs() < 1e-9 {
            return None;
        }
        let (ia, ib, id, ie) = (e / det, -b / det, -d / det, a / det);
        Some(Affine([ia, ib, -(ia * c + ib * f), id, ie, -(id * c + ie * f)]))
    }

    /// The 3×3 form for X's "Coordinate Transformation Matrix".
    pub fn to_ctm(&self) -> String {
        let m = self.0;
        format!("{} {} {} {} {} {} 0 0 1", m[0], m[1], m[2], m[3], m[4], m[5])
    }

    /// Least-squares affine map taking each `from` point to its `to` point
    /// (at least three points, not all on one line).
    pub fn fit(from: &[(f64, f64)], to: &[(f64, f64)]) -> Option<Affine> {
        if from.len() < 3 || from.len() != to.len() {
            return None;
        }
        // Normal equations: (AᵀA) p = Aᵀb for each output coordinate, with
        // rows (x, y, 1).
        let mut ata = [[0.0f64; 3]; 3];
        let (mut atx, mut aty) = ([0.0f64; 3], [0.0f64; 3]);
        for (&(x, y), &(u, v)) in from.iter().zip(to) {
            let row = [x, y, 1.0];
            for i in 0..3 {
                for j in 0..3 {
                    ata[i][j] += row[i] * row[j];
                }
                atx[i] += row[i] * u;
                aty[i] += row[i] * v;
            }
        }
        let px = solve3(ata, atx)?;
        let py = solve3(ata, aty)?;
        Some(Affine([px[0], px[1], px[2], py[0], py[1], py[2]]))
    }
}

/// Solve a 3×3 linear system (Cramer's rule; the systems here are tiny and
/// well-conditioned unless the points are collinear).
fn solve3(m: [[f64; 3]; 3], b: [f64; 3]) -> Option<[f64; 3]> {
    let det = |m: [[f64; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let d = det(m);
    if d.abs() < 1e-12 {
        return None;
    }
    let mut out = [0.0; 3];
    for (k, o) in out.iter_mut().enumerate() {
        let mut mk = m;
        for i in 0..3 {
            mk[i][k] = b[i];
        }
        *o = det(mk) / d;
    }
    Some(out)
}

/// One axis range of the touchscreen (EVIOCGABS).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AbsRange {
    pub min: i32,
    pub max: i32,
}

impl AbsRange {
    fn normalize(&self, v: i32) -> f64 {
        let span = (self.max - self.min).max(1) as f64;
        (v - self.min) as f64 / span
    }
}

/// What the touchscreen sends (already decoded from `input_event`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawEvent {
    X(i32),
    Y(i32),
    /// BTN_TOUCH (or BTN_LEFT on controllers that report a mouse button).
    Touch(bool),
    /// SYN_REPORT: the end of one frame.
    Sync,
    /// SYN_DROPPED: the kernel buffer overflowed; state is unknown until the
    /// next SYN_REPORT.
    Dropped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
}

/// What the virtual pointer emits. Positions are in screen pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Output {
    Move { x: f64, y: f64 },
    Button { button: Button, pressed: bool },
    /// SYN_REPORT: press and release must not share a frame.
    Sync,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    Up,
    /// Down, not yet a tap, drag or hold.
    Pending,
    /// Left button held for a drag.
    Dragging,
    /// The right click was sent; wait for the stylus to lift.
    Held,
    /// Translation off (`right_click = false`): left button follows touch.
    Direct,
}

/// Turns raw touchscreen frames into pointer events. Feed every event with
/// a monotonic time in ms, and call [`Translator::tick`] by
/// [`Translator::deadline`] while the stylus is down.
pub struct Translator {
    recognizer: Recognizer,
    right_click: bool,
    transform: Affine,
    x_range: AbsRange,
    y_range: AbsRange,
    screen: (u32, u32),
    raw: (i32, i32),
    touch: bool,
    touch_reported: bool,
    dropping: bool,
    phase: Phase,
    left_down: bool,
}

/// How often a held stylus must be checked for the long press.
pub const TICK_MS: u64 = 20;

impl Translator {
    pub fn new(
        gesture: GestureConfig,
        right_click: bool,
        transform: Affine,
        x_range: AbsRange,
        y_range: AbsRange,
        screen: (u32, u32),
    ) -> Self {
        Self {
            recognizer: Recognizer::new(gesture),
            right_click,
            transform,
            x_range,
            y_range,
            screen,
            raw: (x_range.min, y_range.min),
            touch: false,
            touch_reported: false,
            dropping: false,
            phase: Phase::Up,
            left_down: false,
        }
    }

    /// Calibration (or rotation) changed, e.g. after `pda-device calibrate`.
    pub fn set_transform(&mut self, transform: Affine) {
        self.transform = transform;
    }

    /// Screen position of the current raw sample.
    pub fn position(&self) -> (f64, f64) {
        let nx = self.x_range.normalize(self.raw.0);
        let ny = self.y_range.normalize(self.raw.1);
        let (sx, sy) = self.transform.apply(nx, ny);
        let (w, h) = (self.screen.0 as f64, self.screen.1 as f64);
        ((sx * w).clamp(0.0, w - 1.0), (sy * h).clamp(0.0, h - 1.0))
    }

    /// When [`Translator::tick`] must run next, if at all.
    pub fn deadline(&self, now: u64) -> Option<u64> {
        (self.phase == Phase::Pending).then_some(now + TICK_MS)
    }

    pub fn feed(&mut self, t: u64, ev: RawEvent) -> Vec<Output> {
        match ev {
            RawEvent::X(v) => self.raw.0 = v,
            RawEvent::Y(v) => self.raw.1 = v,
            RawEvent::Touch(down) => self.touch = down,
            RawEvent::Dropped => {
                // Lost events: end whatever was going on, cleanly, and ignore
                // the rest of this frame.
                self.dropping = true;
                self.touch = false;
                return self.reset();
            }
            RawEvent::Sync if self.dropping => {
                self.dropping = false;
                self.touch_reported = self.touch;
            }
            RawEvent::Sync => return self.frame(t),
        }
        Vec::new()
    }

    pub fn tick(&mut self, t: u64) -> Vec<Output> {
        if self.phase != Phase::Pending {
            return Vec::new();
        }
        let out = self.recognizer.tick(t);
        self.gestures(out)
    }

    /// Release anything held (device gone, daemon stopping, lost events).
    pub fn reset(&mut self) -> Vec<Output> {
        self.recognizer.cancel();
        self.phase = Phase::Up;
        self.touch_reported = false;
        if std::mem::take(&mut self.left_down) {
            vec![Output::Button { button: Button::Left, pressed: false }, Output::Sync]
        } else {
            Vec::new()
        }
    }

    fn frame(&mut self, t: u64) -> Vec<Output> {
        let (x, y) = self.position();
        let was = std::mem::replace(&mut self.touch_reported, self.touch);
        let mut out = Vec::new();
        match (was, self.touch) {
            (false, true) if !self.right_click => {
                self.phase = Phase::Direct;
                self.left_down = true;
                out.extend([Output::Move { x, y }, Output::Button { button: Button::Left, pressed: true }, Output::Sync]);
            }
            (false, true) => {
                self.phase = Phase::Pending;
                let g = self.recognizer.press(t, x, y);
                out.extend(self.gestures(g));
                // The pointer goes to the stylus at once (hover, tooltips);
                // the button waits for the decision.
                out.extend([Output::Move { x, y }, Output::Sync]);
            }
            (true, true) => match self.phase {
                Phase::Pending | Phase::Dragging => {
                    let g = self.recognizer.motion(t, x, y);
                    out.extend(self.gestures(g));
                }
                Phase::Direct => out.extend([Output::Move { x, y }, Output::Sync]),
                Phase::Held | Phase::Up => {}
            },
            (true, false) => {
                match self.phase {
                    Phase::Direct => {
                        self.left_down = false;
                        out.extend([
                            Output::Move { x, y },
                            Output::Button { button: Button::Left, pressed: false },
                            Output::Sync,
                        ]);
                    }
                    Phase::Pending | Phase::Dragging | Phase::Held => {
                        let g = self.recognizer.release(t, x, y);
                        out.extend(self.gestures(g));
                    }
                    Phase::Up => {}
                }
                self.phase = Phase::Up;
            }
            (false, false) => {}
        }
        out
    }

    fn gestures(&mut self, gestures: Vec<Gesture>) -> Vec<Output> {
        let mut out = Vec::new();
        for g in gestures {
            match g {
                Gesture::Tap { x, y } => {
                    out.extend(click(Button::Left, x, y));
                }
                Gesture::DragBegin { x, y } => {
                    self.phase = Phase::Dragging;
                    self.left_down = true;
                    out.extend([
                        Output::Move { x, y },
                        Output::Button { button: Button::Left, pressed: true },
                        Output::Sync,
                    ]);
                }
                Gesture::DragUpdate { x, y } => out.extend([Output::Move { x, y }, Output::Sync]),
                Gesture::DragEnd { x, y } => {
                    self.left_down = false;
                    if x.is_finite() {
                        out.push(Output::Move { x, y });
                    }
                    out.extend([Output::Button { button: Button::Left, pressed: false }, Output::Sync]);
                }
                Gesture::LongPress { x, y } => {
                    self.phase = Phase::Held;
                    out.extend(click(Button::Right, x, y));
                }
                // A double tap is two taps: the second one already clicked.
                Gesture::DoubleTap { .. } | Gesture::LongPressProgress { .. } | Gesture::LongPressCancelled => {}
            }
        }
        out
    }
}

fn click(button: Button, x: f64, y: f64) -> [Output; 5] {
    [
        Output::Move { x, y },
        Output::Button { button, pressed: true },
        Output::Sync,
        Output::Button { button, pressed: false },
        Output::Sync,
    ]
}

/// Capabilities of an input device that matter for choosing the
/// touchscreen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceInfo {
    pub path: String,
    pub name: String,
    pub vendor: u16,
    pub product: u16,
    pub abs_x: bool,
    pub abs_y: bool,
    pub btn_touch: bool,
    pub btn_left: bool,
    pub rel: bool,
    pub direct: bool,
}

/// Name of the virtual pointer; never chosen as the touchscreen, so the
/// daemon cannot feed on its own output.
pub const VIRTUAL_NAME: &str = "pda-shell stylus";

impl DeviceInfo {
    /// Why this device is not a touchscreen candidate, if it is not.
    pub fn rejection(&self) -> Option<&'static str> {
        if self.name == VIRTUAL_NAME {
            Some("the daemon's own virtual pointer")
        } else if !(self.abs_x && self.abs_y) {
            Some("no absolute X/Y axes (keyboard, mouse or button)")
        } else if self.rel {
            Some("has relative axes (a mouse)")
        } else if !(self.btn_touch || self.btn_left) {
            Some("no touch or button key (joystick, accelerometer)")
        } else {
            None
        }
    }

    fn matches(&self, selector: &str) -> bool {
        if let Some(id) = selector.strip_prefix("usb:") {
            let id = id.to_ascii_lowercase();
            id == format!("{:04x}:{:04x}", self.vendor, self.product)
        } else if selector.starts_with('/') {
            self.path == selector
        } else {
            self.name.to_lowercase().contains(&selector.to_lowercase())
        }
    }
}

/// Pick the touchscreen: `selector` is "auto", a device path (e.g. the udev
/// symlink /dev/input/pda-touchscreen, resolved by the caller),
/// "usb:VVVV:PPPP", or part of the device name. An explicitly chosen device
/// must still look like a touchscreen: the daemon never grabs a mouse.
pub fn choose<'a>(devices: &'a [DeviceInfo], selector: &str) -> Result<&'a DeviceInfo, String> {
    let selector = selector.trim();
    let candidates: Vec<&DeviceInfo> = devices.iter().filter(|d| d.rejection().is_none()).collect();
    if selector.is_empty() || selector == "auto" {
        return match candidates.as_slice() {
            [one] => Ok(one),
            [] => Err("no touchscreen found".into()),
            many => {
                let direct: Vec<&&DeviceInfo> = many.iter().filter(|d| d.direct).collect();
                match direct.as_slice() {
                    [one] => Ok(one),
                    _ => Err(format!(
                        "{} touchscreen candidates ({}); set [stylus] device",
                        many.len(),
                        many.iter().map(|d| format!("{} {}", d.path, d.name)).collect::<Vec<_>>().join(", ")
                    )),
                }
            }
        };
    }
    // A touchscreen that matches wins over another device with the same
    // name or id (the virtual pointer copies nothing, but be safe).
    let found = candidates.iter().copied().find(|d| d.matches(selector)).or_else(|| devices.iter().find(|d| d.matches(selector)));
    match found {
        Some(d) => match d.rejection() {
            None => Ok(d),
            Some(why) => Err(format!("{} ({}) is not a touchscreen: {why}", d.path, d.name)),
        },
        None => Err(format!("no input device matches {selector:?}")),
    }
}

/// Where `pda-device calibrate` saves the calibration of the session user:
/// six numbers, overriding `[stylus] calibration`.
pub fn calibration_file() -> Option<std::path::PathBuf> {
    // An empty XDG_CONFIG_HOME counts as unset (XDG base directory spec).
    let dir = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|d| !d.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".config")))?;
    Some(dir.join("pda-shell/stylus-calibration"))
}

/// Six finite numbers separated by spaces, commas or newlines.
pub fn parse_calibration(text: &str) -> Option<Vec<f64>> {
    let v: Vec<f64> = text
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    (v.len() == 6 && v.iter().all(|x| x.is_finite())).then_some(v)
}

/// The config with the saved calibration applied, if there is one.
pub fn effective(cfg: &StylusConfig) -> StylusConfig {
    let mut out = cfg.clone();
    if let Some(path) = calibration_file() {
        match std::fs::read_to_string(&path) {
            Ok(text) => match parse_calibration(&text) {
                Some(c) => out.calibration = c,
                None => log::warn!("ignoring {}: not six numbers", path.display()),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log::warn!("cannot read {}: {e}", path.display()),
        }
    }
    out
}

/// Calibration targets on the screen, normalized: near the corners, where
/// the bars are, and the centre.
pub const CALIBRATION_TARGETS: [(f64, f64); 5] = [(0.08, 0.08), (0.92, 0.08), (0.92, 0.92), (0.08, 0.92), (0.5, 0.5)];

/// A new calibration from taps measured with the calibration switched off
/// (only the rotation applied): `measured[i]` is where the tap aimed at
/// `targets[i]` landed, both normalized to the screen. Returns the
/// calibration and the largest remaining error (normalized).
pub fn calibrate(rotation: Rotation, measured: &[(f64, f64)], targets: &[(f64, f64)]) -> Option<(Affine, f64)> {
    // Undo the rotation on both sides: the calibration works in the
    // panel's own orientation, so it stays valid when the session turns.
    let unrotate = Affine::rotation(rotation).inverse()?;
    let from: Vec<(f64, f64)> = measured.iter().map(|&(x, y)| unrotate.apply(x, y)).collect();
    let to: Vec<(f64, f64)> = targets.iter().map(|&(x, y)| unrotate.apply(x, y)).collect();
    let c = Affine::fit(&from, &to)?;
    let full = Affine::rotation(rotation).after(&c);
    let err = measured
        .iter()
        .zip(targets)
        .map(|(&(mx, my), &(tx, ty))| {
            let (dx, dy) = unrotate.apply(mx, my);
            let (x, y) = full.apply(dx, dy);
            ((x - tx).powi(2) + (y - ty).powi(2)).sqrt()
        })
        .fold(0.0, f64::max);
    Some((c, err))
}

/// The full transform from the touchscreen's normalized coordinates to the
/// screen: calibration (in the panel's own orientation), then rotation.
pub fn transform(cfg: &StylusConfig, rotation: Rotation) -> Affine {
    let calibration = <[f64; 6]>::try_from(cfg.calibration.as_slice()).map(Affine).unwrap_or(Affine::IDENTITY);
    Affine::rotation(rotation).after(&calibration)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RANGE: AbsRange = AbsRange { min: 0, max: 4095 };

    fn translator(right_click: bool) -> Translator {
        let g = GestureConfig {
            long_press_ms: 600,
            slop_px: 8.0,
            double_tap_ms: 400,
            double_tap_slop_px: 10.0,
            feedback_delay_ms: 150,
        };
        Translator::new(g, right_click, Affine::IDENTITY, RANGE, RANGE, (1024, 600))
    }

    /// Raw units for a screen pixel (identity transform, 1024x600).
    fn raw(x: f64, y: f64) -> (i32, i32) {
        ((x / 1024.0 * 4095.0).round() as i32, (y / 600.0 * 4095.0).round() as i32)
    }

    fn down(t: &mut Translator, ms: u64, x: f64, y: f64) -> Vec<Output> {
        let (rx, ry) = raw(x, y);
        let mut out = t.feed(ms, RawEvent::X(rx));
        out.extend(t.feed(ms, RawEvent::Y(ry)));
        out.extend(t.feed(ms, RawEvent::Touch(true)));
        out.extend(t.feed(ms, RawEvent::Sync));
        out
    }

    fn motion(t: &mut Translator, ms: u64, x: f64, y: f64) -> Vec<Output> {
        let (rx, ry) = raw(x, y);
        let mut out = t.feed(ms, RawEvent::X(rx));
        out.extend(t.feed(ms, RawEvent::Y(ry)));
        out.extend(t.feed(ms, RawEvent::Sync));
        out
    }

    fn up(t: &mut Translator, ms: u64) -> Vec<Output> {
        let mut out = t.feed(ms, RawEvent::Touch(false));
        out.extend(t.feed(ms, RawEvent::Sync));
        out
    }

    /// Ticks every 20 ms from `from` to `to` inclusive.
    fn hold(t: &mut Translator, from: u64, to: u64) -> Vec<Output> {
        (from..=to).step_by(TICK_MS as usize).flat_map(|ms| t.tick(ms)).collect()
    }

    fn buttons(out: &[Output]) -> Vec<(Button, bool)> {
        out.iter()
            .filter_map(|o| match o {
                Output::Button { button, pressed } => Some((*button, *pressed)),
                _ => None,
            })
            .collect()
    }

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1.0
    }

    #[test]
    fn tap_is_a_left_click_after_release() {
        let mut t = translator(true);
        let out = down(&mut t, 0, 100.0, 200.0);
        assert!(buttons(&out).is_empty(), "no button before the decision: {out:?}");
        assert!(matches!(out[0], Output::Move { x, y } if near(x, 100.0) && near(y, 200.0)));
        assert!(hold(&mut t, 20, 100).is_empty());
        let out = up(&mut t, 110);
        assert_eq!(buttons(&out), vec![(Button::Left, true), (Button::Left, false)]);
        // Press and release in separate frames.
        let syncs = out.iter().filter(|o| **o == Output::Sync).count();
        assert_eq!(syncs, 2);
    }

    #[test]
    fn jitter_within_the_slop_is_still_a_tap() {
        let mut t = translator(true);
        down(&mut t, 0, 100.0, 200.0);
        assert!(buttons(&motion(&mut t, 20, 104.0, 203.0)).is_empty());
        assert!(buttons(&motion(&mut t, 40, 97.0, 198.0)).is_empty());
        let out = up(&mut t, 90);
        assert_eq!(buttons(&out), vec![(Button::Left, true), (Button::Left, false)]);
    }

    #[test]
    fn hold_is_a_right_click_and_no_left_click_follows() {
        let mut t = translator(true);
        down(&mut t, 0, 300.0, 300.0);
        let out = hold(&mut t, 20, 580);
        assert!(buttons(&out).is_empty(), "not before 600 ms");
        let out = hold(&mut t, 600, 600);
        assert_eq!(buttons(&out), vec![(Button::Right, true), (Button::Right, false)]);
        assert!(matches!(out[0], Output::Move { x, y } if near(x, 300.0) && near(y, 300.0)));
        // Jitter, more holding and the release produce nothing.
        assert!(motion(&mut t, 700, 303.0, 302.0).is_empty());
        assert!(motion(&mut t, 720, 340.0, 302.0).is_empty(), "moving after the hold does not drag");
        assert!(hold(&mut t, 740, 2000).is_empty());
        assert!(up(&mut t, 2010).is_empty());
        assert_eq!(t.deadline(2010), None);
    }

    #[test]
    fn hold_threshold_at_release_without_tick() {
        // A late tick (busy system): the release itself is past the hold time.
        let mut t = translator(true);
        down(&mut t, 0, 50.0, 50.0);
        let out = up(&mut t, 650);
        assert_eq!(buttons(&out), vec![(Button::Right, true), (Button::Right, false)]);
    }

    #[test]
    fn drag_presses_at_the_origin_then_moves() {
        let mut t = translator(true);
        down(&mut t, 0, 100.0, 100.0);
        let out = motion(&mut t, 30, 130.0, 100.0);
        assert_eq!(buttons(&out), vec![(Button::Left, true)]);
        assert!(matches!(out[0], Output::Move { x, .. } if near(x, 100.0)), "press where the stylus went down");
        assert!(matches!(out.last(), Some(Output::Sync)));
        assert!(out.iter().any(|o| matches!(o, Output::Move { x, .. } if near(*x, 130.0))));
        // Holding still during a drag never becomes a right click.
        assert!(hold(&mut t, 50, 3000).is_empty());
        let out = motion(&mut t, 3010, 200.0, 150.0);
        assert!(buttons(&out).is_empty());
        let out = up(&mut t, 3020);
        assert_eq!(buttons(&out), vec![(Button::Left, false)]);
    }

    #[test]
    fn moving_during_the_hold_cancels_it_into_a_drag() {
        let mut t = translator(true);
        down(&mut t, 0, 100.0, 100.0);
        hold(&mut t, 20, 400);
        let out = motion(&mut t, 420, 100.0, 140.0);
        assert_eq!(buttons(&out), vec![(Button::Left, true)]);
        assert!(hold(&mut t, 440, 1500).is_empty(), "no right click after the drag started");
        assert_eq!(buttons(&up(&mut t, 1500)), vec![(Button::Left, false)]);
    }

    #[test]
    fn double_tap_is_two_clicks() {
        let mut t = translator(true);
        down(&mut t, 0, 10.0, 10.0);
        let a = up(&mut t, 60);
        down(&mut t, 150, 12.0, 11.0);
        let b = up(&mut t, 210);
        assert_eq!(buttons(&a).len() + buttons(&b).len(), 4, "the X server sees the double click itself");
    }

    #[test]
    fn device_removal_releases_a_held_button() {
        let mut t = translator(true);
        down(&mut t, 0, 100.0, 100.0);
        motion(&mut t, 30, 160.0, 100.0);
        assert_eq!(t.reset(), vec![Output::Button { button: Button::Left, pressed: false }, Output::Sync]);
        assert!(t.reset().is_empty(), "nothing left to release");
        // Events of a stale frame after the reset do nothing.
        assert!(up(&mut t, 100).is_empty());
    }

    #[test]
    fn dropped_events_end_the_drag_and_resync() {
        let mut t = translator(true);
        down(&mut t, 0, 100.0, 100.0);
        motion(&mut t, 30, 160.0, 100.0);
        let out = t.feed(40, RawEvent::Dropped);
        assert_eq!(buttons(&out), vec![(Button::Left, false)]);
        assert!(t.feed(41, RawEvent::X(5)).is_empty());
        assert!(t.feed(42, RawEvent::Sync).is_empty(), "the frame after SYN_DROPPED is discarded");
        // A new touch works normally.
        down(&mut t, 100, 50.0, 50.0);
        assert_eq!(buttons(&up(&mut t, 150)), vec![(Button::Left, true), (Button::Left, false)]);
    }

    #[test]
    fn without_right_click_the_button_follows_the_stylus() {
        let mut t = translator(false);
        let out = down(&mut t, 0, 10.0, 10.0);
        assert_eq!(buttons(&out), vec![(Button::Left, true)]);
        assert!(hold(&mut t, 20, 2000).is_empty());
        assert_eq!(buttons(&up(&mut t, 2000)), vec![(Button::Left, false)]);
    }

    #[test]
    fn rotation_matrices_follow_xrandr() {
        // xrandr --rotate left turns the picture counter-clockwise: the
        // screen's top-left pixel is drawn at the panel's bottom-left.
        let left = Affine::rotation(Rotation::Left);
        assert_eq!(left.apply(0.0, 1.0), (0.0, 0.0));
        assert_eq!(left.apply(0.0, 0.0), (1.0, 0.0));
        let right = Affine::rotation(Rotation::Right);
        assert_eq!(right.apply(1.0, 0.0), (0.0, 0.0), "clockwise: top-right of the panel");
        let inv = Affine::rotation(Rotation::Inverted);
        assert_eq!(inv.apply(1.0, 1.0), (0.0, 0.0));
        for r in [Rotation::Left, Rotation::Right, Rotation::Inverted] {
            let back = Affine::rotation(r).inverse().unwrap().after(&Affine::rotation(r));
            let (x, y) = back.apply(0.3, 0.7);
            assert!((x - 0.3).abs() < 1e-9 && (y - 0.7).abs() < 1e-9);
        }
    }

    #[test]
    fn portrait_translation_maps_panel_corners() {
        // A 1024x600 panel turned left for a 600x1024 session.
        let g = GestureConfig::default();
        let mut t = Translator::new(g, true, Affine::rotation(Rotation::Left), RANGE, RANGE, (600, 1024));
        // The panel's bottom-left corner is the screen's top-left.
        t.feed(0, RawEvent::X(0));
        t.feed(0, RawEvent::Y(4095));
        let (x, y) = t.position();
        assert!(x < 1.0 && y < 1.0, "({x}, {y})");
        t.feed(0, RawEvent::X(4095));
        t.feed(0, RawEvent::Y(4095));
        let (x, y) = t.position();
        assert!(x < 1.0 && y > 1022.0, "panel bottom-right is screen bottom-left: ({x}, {y})");
    }

    #[test]
    fn calibration_fit_recovers_a_known_matrix() {
        let truth = Affine([1.05, 0.02, -0.03, -0.01, 0.97, 0.02]);
        let from = [(0.1, 0.1), (0.9, 0.1), (0.9, 0.9), (0.1, 0.9), (0.5, 0.5)];
        let to: Vec<(f64, f64)> = from.iter().map(|&(x, y)| truth.apply(x, y)).collect();
        let fit = Affine::fit(&from, &to).unwrap();
        for (a, b) in fit.0.iter().zip(truth.0) {
            assert!((a - b).abs() < 1e-9, "{fit:?}");
        }
        assert!(Affine::fit(&from[..2], &to[..2]).is_none(), "needs three points");
        assert!(Affine::fit(&[(0.0, 0.0), (0.5, 0.5), (1.0, 1.0)], &to[..3]).is_none(), "collinear");
        assert_eq!(Affine::IDENTITY.to_ctm(), "1 0 0 0 1 0 0 0 1");
    }

    #[test]
    fn transform_composes_calibration_then_rotation() {
        let cfg = StylusConfig { calibration: vec![1.0, 0.0, 0.1, 0.0, 1.0, 0.0], ..StylusConfig::default() };
        let m = transform(&cfg, Rotation::Left);
        // Calibration shifts x by 0.1 first; rotation then maps x to y.
        let (x, y) = m.apply(0.0, 1.0);
        assert!((x - 0.0).abs() < 1e-9 && (y - 0.1).abs() < 1e-9, "({x}, {y})");
        let bad = StylusConfig { calibration: vec![1.0], ..StylusConfig::default() };
        assert_eq!(transform(&bad, Rotation::Normal), Affine::IDENTITY);
    }

    #[test]
    fn calibration_from_measured_taps() {
        // The panel is a little off: true calibration C. With calibration
        // switched off, a tap aimed at target t lands at R·d where
        // R·C·d = t.
        let truth = Affine([1.04, 0.01, -0.02, -0.02, 0.95, 0.03]);
        for rotation in [Rotation::Normal, Rotation::Left, Rotation::Right] {
            let r = Affine::rotation(rotation);
            let to_device = truth.inverse().unwrap().after(&r.inverse().unwrap());
            let measured: Vec<(f64, f64)> = CALIBRATION_TARGETS
                .iter()
                .map(|&(x, y)| {
                    let (dx, dy) = to_device.apply(x, y);
                    r.apply(dx, dy)
                })
                .collect();
            let (c, err) = calibrate(rotation, &measured, &CALIBRATION_TARGETS).unwrap();
            for (a, b) in c.0.iter().zip(truth.0) {
                assert!((a - b).abs() < 1e-6, "{rotation:?}: {c:?}");
            }
            assert!(err < 1e-6);
        }
        // Taps that do not fit an affine map leave a visible error.
        let mut sloppy = CALIBRATION_TARGETS.to_vec();
        sloppy[4] = (0.7, 0.3);
        assert!(calibrate(Rotation::Normal, &sloppy, &CALIBRATION_TARGETS).unwrap().1 > 0.05);
    }

    #[test]
    fn calibration_text() {
        assert_eq!(parse_calibration("1 0 0\n0 1 0\n"), Some(vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0]));
        assert_eq!(parse_calibration("1.02, 0, -0.01, 0, 0.98, 0.01").unwrap()[0], 1.02);
        assert_eq!(parse_calibration("1 0 0 0 1"), None);
        assert_eq!(parse_calibration("1 0 0 0 1 x"), None);
        assert_eq!(parse_calibration("1 0 0 0 1 NaN"), None);
    }

    fn dev(path: &str, name: &str) -> DeviceInfo {
        DeviceInfo { path: path.into(), name: name.into(), ..Default::default() }
    }

    fn touchscreen(path: &str, name: &str) -> DeviceInfo {
        DeviceInfo { abs_x: true, abs_y: true, btn_touch: true, vendor: 0x0eef, product: 0x0001, ..dev(path, name) }
    }

    #[test]
    fn choosing_the_touchscreen() {
        let mouse = DeviceInfo { rel: true, btn_left: true, ..dev("/dev/input/event1", "USB Mouse") };
        let tablet_mouse = DeviceInfo { abs_x: true, abs_y: true, rel: true, btn_left: true, ..dev("/dev/input/event5", "Combo") };
        let keyboard = dev("/dev/input/event0", "USB Keyboard");
        let ours = DeviceInfo { name: VIRTUAL_NAME.into(), ..touchscreen("/dev/input/event9", "") };
        let ts = touchscreen("/dev/input/event3", "eGalax Inc. USB TouchController");
        let all = vec![keyboard.clone(), mouse.clone(), tablet_mouse, ours.clone(), ts.clone()];
        assert_eq!(choose(&all, "auto").unwrap(), &ts);
        assert_eq!(choose(&all, "usb:0EEF:0001").unwrap(), &ts);
        assert_eq!(choose(&all, "egalax").unwrap(), &ts);
        assert_eq!(choose(&all, "/dev/input/event3").unwrap(), &ts);
        // Never a mouse, the keyboard or the daemon's own device, even by name.
        assert!(choose(&all, "USB Mouse").unwrap_err().contains("not a touchscreen"));
        assert!(choose(&all, "Combo").unwrap_err().contains("relative axes"), "absolute mouse, e.g. a VM tablet");
        assert!(choose(&all, "/dev/input/event9").unwrap_err().contains("own virtual pointer"));
        assert!(choose(&[keyboard, mouse], "auto").unwrap_err().contains("no touchscreen"));
        assert!(choose(&all, "/dev/input/event7").unwrap_err().contains("no input device"));
        // Two candidates: the direct (screen-mounted) one wins, else ask.
        let pad = touchscreen("/dev/input/event4", "Pen tablet");
        let direct = DeviceInfo { direct: true, ..ts.clone() };
        assert_eq!(choose(&[pad.clone(), direct.clone()], "auto").unwrap(), &direct);
        assert!(choose(&[pad, ts], "auto").unwrap_err().contains("set [stylus] device"));
    }
}

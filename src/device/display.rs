//! Display bring-up on the physical device: which output, mode and
//! rotation to use, decided from `xrandr --query` and the `[device]`
//! section of the config. Pure functions, so every panel and resolution can
//! be tested without one.

use crate::config::{DeviceConfig, DeviceOrientation, TurnDirection};
use std::fmt;

/// xrandr rotation names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rotation {
    Normal,
    Left,
    Inverted,
    Right,
}

impl Rotation {
    pub fn name(self) -> &'static str {
        match self {
            Rotation::Normal => "normal",
            Rotation::Left => "left",
            Rotation::Inverted => "inverted",
            Rotation::Right => "right",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "normal" => Some(Rotation::Normal),
            "left" => Some(Rotation::Left),
            "inverted" => Some(Rotation::Inverted),
            "right" => Some(Rotation::Right),
            _ => None,
        }
    }

    /// Whether width and height swap.
    pub fn is_sideways(self) -> bool {
        matches!(self, Rotation::Left | Rotation::Right)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mode {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub current: bool,
    pub preferred: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub name: String,
    pub connected: bool,
    pub primary: bool,
    /// Current rotation; `None` when the output is off.
    pub rotation: Option<Rotation>,
    /// Physical size reported by the panel (EDID), if any.
    pub size_mm: Option<(u32, u32)>,
    pub modes: Vec<Mode>,
}

fn parse_size(s: &str) -> Option<(u32, u32)> {
    // "1024x600", "1024x600i", "1024x600_60.00", "1024x600+0+0"
    let (w, rest) = s.split_once('x')?;
    let h: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    Some((w.parse().ok()?, h.parse().ok()?))
}

/// Parse `xrandr --query`.
pub fn parse_xrandr(text: &str) -> Vec<Output> {
    let mut outputs: Vec<Output> = Vec::new();
    for line in text.lines() {
        if line.starts_with("Screen ") {
            continue;
        }
        if !line.starts_with(char::is_whitespace) {
            let mut words = line.split_whitespace();
            let Some(name) = words.next() else { continue };
            let Some(state) = words.next() else { continue };
            let rest: Vec<&str> = words.collect();
            let primary = rest.first() == Some(&"primary");
            let rest = if primary { &rest[1..] } else { &rest[..] };
            // "1024x600+0+0 left (normal left ...) 110mm x 65mm"
            let geometry = rest.first().filter(|w| w.contains('+') && parse_size(w).is_some());
            // `--verbose` puts the mode id first: "1024x600+0+0 (0x45) left (...)".
            let rotation = geometry.map(|_| {
                rest.iter().skip(1).find(|w| !w.starts_with("(0x")).and_then(|w| Rotation::parse(w)).unwrap_or(Rotation::Normal)
            });
            let size_mm = rest
                .windows(3)
                .find(|w| w[0].ends_with("mm") && w[1] == "x" && w[2].ends_with("mm"))
                .and_then(|w| Some((w[0].trim_end_matches("mm").parse().ok()?, w[2].trim_end_matches("mm").parse().ok()?)))
                .filter(|(w, h)| *w > 0 && *h > 0);
            outputs.push(Output {
                name: name.to_string(),
                connected: state == "connected",
                primary,
                rotation,
                size_mm,
                modes: Vec::new(),
            });
        } else if let Some(out) = outputs.last_mut() {
            let mut words = line.split_whitespace();
            let Some(name) = words.next() else { continue };
            // Property lines of --verbose start with a word and a colon.
            if name.ends_with(':') {
                continue;
            }
            let Some((width, height)) = parse_size(name) else { continue };
            let flags: String = words.collect();
            out.modes.push(Mode {
                name: name.to_string(),
                width,
                height,
                current: flags.contains('*'),
                preferred: flags.contains('+'),
            });
        }
    }
    outputs
}

/// What the device session does with the display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayPlan {
    pub output: String,
    pub mode: String,
    pub rotation: Rotation,
    /// Screen size after rotation: what the shell lays out.
    pub width: u32,
    pub height: u32,
    pub size_mm: Option<(u32, u32)>,
    /// Problems worth a line in the session log (requested output or mode
    /// missing, ...); the plan is still usable.
    pub warnings: Vec<String>,
}

impl DisplayPlan {
    pub fn orientation(&self) -> &'static str {
        if self.height > self.width {
            "portrait"
        } else {
            "landscape"
        }
    }

    /// Pixels per inch along the diagonal, from the panel's EDID size.
    pub fn ppi(&self) -> Option<f64> {
        let (wmm, hmm) = self.size_mm?;
        let (w, h) = if self.rotation.is_sideways() { (self.height, self.width) } else { (self.width, self.height) };
        let px = ((w * w + h * h) as f64).sqrt();
        let inch = ((wmm * wmm + hmm * hmm) as f64).sqrt() / 25.4;
        Some(px / inch)
    }
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

impl fmt::Display for DisplayPlan {
    /// Shell assignments, for `eval` in device-session.sh.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "OUTPUT={}", shell_quote(&self.output))?;
        writeln!(f, "MODE={}", shell_quote(&self.mode))?;
        writeln!(f, "ROTATION={}", self.rotation.name())?;
        writeln!(f, "WIDTH={}", self.width)?;
        writeln!(f, "HEIGHT={}", self.height)?;
        writeln!(f, "ORIENTATION={}", self.orientation())?;
        let (wmm, hmm) = self.size_mm.unwrap_or((0, 0));
        writeln!(f, "SIZE_MM={wmm}x{hmm}")?;
        writeln!(f, "PPI={}", self.ppi().map_or(0, |p| p.round() as u32))
    }
}

/// Pick output, mode and rotation. `None` if nothing is connected.
pub fn plan(outputs: &[Output], cfg: &DeviceConfig) -> Option<DisplayPlan> {
    let mut warnings = Vec::new();
    let connected: Vec<&Output> = outputs.iter().filter(|o| o.connected && !o.modes.is_empty()).collect();
    let wanted = cfg.output.trim();
    let output = if wanted.is_empty() || wanted == "auto" {
        None
    } else {
        let found = connected.iter().find(|o| o.name == wanted).copied();
        if found.is_none() {
            warnings.push(format!("output {wanted} is not connected; choosing automatically"));
        }
        found
    }
    .or_else(|| connected.iter().find(|o| o.primary).copied())
    .or_else(|| connected.first().copied())?;

    let wanted = cfg.mode.trim();
    let by_name = (!wanted.is_empty() && wanted != "auto").then(|| {
        let found = output.modes.iter().find(|m| m.name == wanted);
        if found.is_none() {
            warnings.push(format!("mode {wanted} is not offered by {}; choosing automatically", output.name));
        }
        found
    });
    let mode = by_name
        .flatten()
        .or_else(|| output.modes.iter().find(|m| m.preferred))
        .or_else(|| output.modes.iter().find(|m| m.current))
        .or_else(|| output.modes.first())?;

    let native_portrait = mode.height > mode.width;
    let turn = match cfg.orientation {
        DeviceOrientation::Native => false,
        DeviceOrientation::Landscape => native_portrait,
        DeviceOrientation::Portrait => !native_portrait,
    };
    let rotation = match (turn, cfg.rotate) {
        (false, _) => Rotation::Normal,
        (true, TurnDirection::Left) => Rotation::Left,
        (true, TurnDirection::Right) => Rotation::Right,
    };
    let (width, height) = if rotation.is_sideways() { (mode.height, mode.width) } else { (mode.width, mode.height) };
    Some(DisplayPlan {
        output: output.name.clone(),
        mode: mode.name.clone(),
        rotation,
        width,
        height,
        size_mm: output.size_mm,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 5" HDMI panel on an Orange Pi (modesetting driver), as xrandr shows it.
    const HDMI_PANEL: &str = "\
Screen 0: minimum 320 x 200, current 1024 x 600, maximum 8192 x 8192
HDMI-1 connected primary 1024x600+0+0 (normal left inverted right x axis y axis) 110mm x 65mm
   1024x600      60.00*+
   800x600       60.32    56.25
   800x480       60.00
   640x480       59.94
";

    const XVFB: &str = "\
Screen 0: minimum 1 x 1, current 600 x 1024, maximum 600 x 1024
screen connected 600x1024+0+0 0mm x 0mm
   600x1024       0.00*
";

    fn cfg(orientation: DeviceOrientation) -> DeviceConfig {
        DeviceConfig { orientation, ..DeviceConfig::default() }
    }

    #[test]
    fn parses_outputs_and_modes() {
        let text = format!("{HDMI_PANEL}HDMI-2 disconnected (normal left inverted right x axis y axis)\n");
        let outs = parse_xrandr(&text);
        assert_eq!(outs.len(), 2);
        let o = &outs[0];
        assert_eq!((o.name.as_str(), o.connected, o.primary), ("HDMI-1", true, true));
        assert_eq!(o.rotation, Some(Rotation::Normal));
        assert_eq!(o.size_mm, Some((110, 65)));
        assert_eq!(o.modes.len(), 4);
        assert!(o.modes[0].current && o.modes[0].preferred);
        assert_eq!((o.modes[2].width, o.modes[2].height), (800, 480));
        assert!(!outs[1].connected && outs[1].modes.is_empty() && outs[1].rotation.is_none());
    }

    #[test]
    fn parses_a_rotated_output_and_verbose_lines() {
        let text = "HDMI-1 connected 600x1024+0+0 (0x45) left (normal left inverted right x axis y axis) 110mm x 65mm\n\
                    \tIdentifier: 0x42\n\tTransform:  1.000000 0.000000 0.000000\n\
                    \x20  1024x600 (0x4a) 51.200MHz *current +preferred\n";
        let o = &parse_xrandr(text)[0];
        assert_eq!(o.rotation, Some(Rotation::Left));
        assert_eq!(o.modes.len(), 1);
        assert!(o.modes[0].current && o.modes[0].preferred);
    }

    #[test]
    fn landscape_panel_in_landscape_is_not_rotated() {
        let p = plan(&parse_xrandr(HDMI_PANEL), &cfg(DeviceOrientation::Landscape)).unwrap();
        assert_eq!((p.output.as_str(), p.mode.as_str(), p.rotation), ("HDMI-1", "1024x600", Rotation::Normal));
        assert_eq!((p.width, p.height, p.orientation()), (1024, 600, "landscape"));
        assert!(p.warnings.is_empty());
        let ppi = p.ppi().unwrap();
        assert!((230.0..245.0).contains(&ppi), "about 237 ppi, got {ppi}");
    }

    #[test]
    fn landscape_panel_in_portrait_is_turned() {
        let p = plan(&parse_xrandr(HDMI_PANEL), &cfg(DeviceOrientation::Portrait)).unwrap();
        assert_eq!((p.rotation, p.width, p.height, p.orientation()), (Rotation::Left, 600, 1024, "portrait"));
        let right = DeviceConfig { rotate: TurnDirection::Right, ..cfg(DeviceOrientation::Portrait) };
        assert_eq!(plan(&parse_xrandr(HDMI_PANEL), &right).unwrap().rotation, Rotation::Right);
        // The ppi does not depend on the rotation.
        let a = plan(&parse_xrandr(HDMI_PANEL), &cfg(DeviceOrientation::Landscape)).unwrap().ppi();
        assert_eq!(p.ppi(), a);
    }

    #[test]
    fn portrait_panel_in_landscape_is_turned() {
        let p = plan(&parse_xrandr(XVFB), &cfg(DeviceOrientation::Landscape)).unwrap();
        assert_eq!((p.rotation, p.width, p.height), (Rotation::Left, 1024, 600));
        let native = plan(&parse_xrandr(XVFB), &cfg(DeviceOrientation::Native)).unwrap();
        assert_eq!((native.rotation, native.width, native.height), (Rotation::Normal, 600, 1024));
        assert_eq!(native.ppi(), None, "Xvfb reports no physical size");
    }

    #[test]
    fn alternative_small_panel_mode() {
        let c = DeviceConfig { mode: "800x480".into(), ..cfg(DeviceOrientation::Landscape) };
        let p = plan(&parse_xrandr(HDMI_PANEL), &c).unwrap();
        assert_eq!((p.mode.as_str(), p.width, p.height), ("800x480", 800, 480));
        let c = DeviceConfig { mode: "800x480".into(), ..cfg(DeviceOrientation::Portrait) };
        assert_eq!(plan(&parse_xrandr(HDMI_PANEL), &c).unwrap().width, 480);
    }

    #[test]
    fn missing_output_or_mode_falls_back_with_a_warning() {
        let c = DeviceConfig { output: "DSI-1".into(), mode: "480x272".into(), ..cfg(DeviceOrientation::Landscape) };
        let p = plan(&parse_xrandr(HDMI_PANEL), &c).unwrap();
        assert_eq!((p.output.as_str(), p.mode.as_str()), ("HDMI-1", "1024x600"));
        assert_eq!(p.warnings.len(), 2, "{:?}", p.warnings);
    }

    #[test]
    fn first_connected_output_without_primary() {
        let text = "VGA-1 disconnected\nHDMI-2 connected 800x480+0+0 0mm x 0mm\n   800x480 60.00*\n   640x480 60.00\n";
        let p = plan(&parse_xrandr(text), &cfg(DeviceOrientation::Native)).unwrap();
        assert_eq!((p.output.as_str(), p.mode.as_str()), ("HDMI-2", "800x480"), "current mode without a preferred one");
        assert!(plan(&parse_xrandr("HDMI-1 disconnected\n"), &DeviceConfig::default()).is_none());
    }

    #[test]
    fn plan_prints_shell_assignments() {
        let p = plan(&parse_xrandr(HDMI_PANEL), &cfg(DeviceOrientation::Portrait)).unwrap();
        let text = p.to_string();
        assert!(text.contains("OUTPUT='HDMI-1'\n"));
        assert!(text.contains("ROTATION=left\nWIDTH=600\nHEIGHT=1024\nORIENTATION=portrait\n"));
        assert!(text.contains("SIZE_MM=110x65\n"));
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }
}

//! Device helper for `scripts/device-session.sh`.
//!
//!   pda-device display [--xrandr FILE] [--orientation O] [--mode M] [--output NAME]
//!       Read `xrandr --query` (or FILE) and the [device] config, print the
//!       output, mode and rotation to use (and the screen blanking delay)
//!       as shell assignments.
//!   pda-device info
//!       Board, memory, OS, kernel and power capabilities (backlight, sleep
//!       states, power supplies, power key).
//!   pda-device inputs
//!       Every input device, and whether it would be taken as the
//!       touchscreen (and why not).
//!   pda-device touch [--rotation R] [--device SEL]
//!       The chosen touchscreen and the X transformation matrix for it, as
//!       shell assignments (used when the stylus daemon is not running).
//!   pda-device stylus --screen WxH [--rotation R] [--device SEL]
//!       The stylus daemon: grabs the touchscreen, drives a virtual pointer,
//!       turns a held stylus into a right click.
//!   pda-device calibrate [--rotation R]
//!       Tap five crosses; saves ~/.config/pda-shell/stylus-calibration.
//!
//! The rotation defaults to $PDA_DEVICE_ROTATION (set by device-session.sh).

use pda_shell::config::{Config, DeviceOrientation};
use pda_shell::device::display::{self, Rotation};
use pda_shell::device::{stylus, system};
use std::process::{Command, ExitCode};

const USAGE: &str = "usage: pda-device display [--xrandr FILE] [--orientation landscape|portrait|native] \
[--mode WxH] [--output NAME]
       pda-device info
       pda-device inputs
       pda-device touch [--rotation R] [--device SEL]
       pda-device stylus --screen WxH [--rotation R] [--device SEL]
       pda-device calibrate [--rotation R]";

fn display_plan(args: &[String]) -> Result<(), String> {
    let mut cfg = Config::load().device;
    let mut xrandr_file = None;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().cloned().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--xrandr" => xrandr_file = Some(value()?),
            "--orientation" => {
                cfg.orientation = match value()?.as_str() {
                    "landscape" => DeviceOrientation::Landscape,
                    "portrait" => DeviceOrientation::Portrait,
                    "native" => DeviceOrientation::Native,
                    o => return Err(format!("unknown orientation {o}")),
                }
            }
            "--mode" => cfg.mode = value()?,
            "--output" => cfg.output = value()?,
            _ => return Err(format!("unknown argument {arg}\n{USAGE}")),
        }
    }
    let text = match xrandr_file {
        Some(f) => std::fs::read_to_string(&f).map_err(|e| format!("{f}: {e}"))?,
        None => {
            let out = Command::new("xrandr").arg("--query").output().map_err(|e| format!("xrandr: {e}"))?;
            if !out.status.success() {
                return Err(format!("xrandr failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
            }
            String::from_utf8_lossy(&out.stdout).into_owned()
        }
    };
    let plan = display::plan(&display::parse_xrandr(&text), &cfg).ok_or("no connected output with a mode")?;
    for w in &plan.warnings {
        eprintln!("pda-device: {w}");
    }
    print!("{plan}");
    println!("SCREEN_OFF_MINUTES={}", cfg.screen_off_minutes);
    Ok(())
}

/// Options shared by the stylus commands.
struct StylusArgs {
    rotation: Rotation,
    device: Option<String>,
    screen: Option<(u32, u32)>,
}

fn stylus_args(args: &[String]) -> Result<StylusArgs, String> {
    let env_rotation = std::env::var("PDA_DEVICE_ROTATION").ok();
    let mut out = StylusArgs {
        rotation: env_rotation.as_deref().and_then(Rotation::parse).unwrap_or(Rotation::Normal),
        device: None,
        screen: None,
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().cloned().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--rotation" => {
                let v = value()?;
                out.rotation = Rotation::parse(&v).ok_or(format!("unknown rotation {v}"))?;
            }
            "--device" => out.device = Some(value()?),
            "--screen" => {
                let v = value()?;
                let (w, h) = v.split_once('x').ok_or(format!("--screen WxH, not {v}"))?;
                out.screen = Some((w.parse().map_err(|_| format!("bad width {w}"))?, h.parse().map_err(|_| format!("bad height {h}"))?));
            }
            _ => return Err(format!("unknown argument {arg}\n{USAGE}")),
        }
    }
    Ok(out)
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(target_os = "linux")]
fn inputs() -> Result<(), String> {
    use pda_shell::device::evdev;
    let selector = Config::load().stylus.device;
    let devices = evdev::list_devices();
    if devices.is_empty() {
        return Err("no readable input devices (are you in the input group?)".into());
    }
    let chosen = stylus::choose(&devices, &selector).map(|d| d.path.clone());
    for d in &devices {
        let verdict = match d.rejection() {
            _ if chosen.as_deref() == Ok(&d.path) => "TOUCHSCREEN (chosen)".to_string(),
            None => "touchscreen-like".to_string(),
            Some(why) => format!("not taken: {why}"),
        };
        println!("{}  {:04x}:{:04x}  {:<40}  {verdict}", d.path, d.vendor, d.product, d.name);
    }
    if let Err(e) = chosen {
        println!("[stylus] device = {selector:?}: {e}");
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn touch(args: &[String]) -> Result<(), String> {
    use pda_shell::device::evdev;
    let a = stylus_args(args)?;
    let cfg = Config::load();
    let selector = a.device.unwrap_or(cfg.stylus.device.clone());
    let resolved = match selector.starts_with('/') {
        true => std::fs::canonicalize(&selector).map(|p| p.to_string_lossy().into_owned()).unwrap_or(selector),
        false => selector,
    };
    let devices = evdev::list_devices();
    let d = stylus::choose(&devices, &resolved)?;
    let m = stylus::transform(&stylus::effective(&cfg.stylus), a.rotation);
    println!("TOUCH_DEVICE={}", shell_quote(&d.path));
    println!("TOUCH_NAME={}", shell_quote(&d.name));
    println!("TOUCH_ID={:04x}:{:04x}", d.vendor, d.product);
    println!("TOUCH_MATRIX={}", shell_quote(&m.to_ctm()));
    Ok(())
}

#[cfg(target_os = "linux")]
fn run_stylus(args: &[String]) -> Result<(), String> {
    let a = stylus_args(args)?;
    let screen = a.screen.ok_or("--screen WxH is required")?;
    pda_shell::device::daemon::run(pda_shell::device::daemon::Options { device: a.device, rotation: a.rotation, screen })
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // The daemon logs what it does; the one-shot commands print results.
    let level = if args.first().map(String::as_str) == Some("stylus") { "info" } else { "warn" };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(level)).init();
    let rest = args.get(1..).unwrap_or_default();
    let result = match args.first().map(String::as_str) {
        Some("display") => display_plan(rest),
        Some("info") => {
            print!("{}", system::report());
            Ok(())
        }
        #[cfg(target_os = "linux")]
        Some("inputs") => inputs(),
        #[cfg(target_os = "linux")]
        Some("touch") => touch(rest),
        #[cfg(target_os = "linux")]
        Some("stylus") => run_stylus(rest),
        Some("calibrate") => {
            return match stylus_args(rest) {
                Ok(a) => match pda_shell::device::calibrate::run(a.rotation) {
                    c if c == glib::ExitCode::SUCCESS => ExitCode::SUCCESS,
                    _ => ExitCode::FAILURE,
                },
                Err(e) => {
                    eprintln!("pda-device: {e}");
                    ExitCode::FAILURE
                }
            };
        }
        Some("-h" | "--help") => {
            println!("{USAGE}");
            Ok(())
        }
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("pda-device: {e}");
            ExitCode::FAILURE
        }
    }
}

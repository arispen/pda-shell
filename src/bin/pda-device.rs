//! Device helper for `scripts/device-session.sh`.
//!
//!   pda-device display [--xrandr FILE] [--orientation O] [--mode M] [--output NAME]
//!       Read `xrandr --query` (or FILE) and the [device] config, print the
//!       output, mode and rotation to use (and the screen blanking delay)
//!       as shell assignments.
//!   pda-device info
//!       Board, memory, OS, kernel and power capabilities (backlight, sleep
//!       states, power supplies, power key).

use pda_shell::config::{Config, DeviceOrientation};
use pda_shell::device::{display, system};
use std::process::{Command, ExitCode};

const USAGE: &str = "usage: pda-device display [--xrandr FILE] [--orientation landscape|portrait|native] \
[--mode WxH] [--output NAME]\n       pda-device info";

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

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("display") => display_plan(&args[1..]),
        Some("info") => {
            print!("{}", system::report());
            Ok(())
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

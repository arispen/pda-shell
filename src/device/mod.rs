//! The thin layer for the physical device: display bring-up decisions,
//! system facts and system-wide stylus input. No X11 connection; only the
//! calibration screen uses GTK. `src/bin/pda-device.rs` exposes it to
//! `scripts/device-session.sh`.

pub mod calibrate;
#[cfg(target_os = "linux")]
pub mod daemon;
pub mod display;
#[cfg(target_os = "linux")]
pub mod evdev;
pub mod stylus;
pub mod system;

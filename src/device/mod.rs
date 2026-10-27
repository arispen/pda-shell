//! The thin layer for the physical device: display bring-up decisions and
//! system facts. No GTK and no X11 connection; `src/bin/pda-device.rs`
//! exposes it to `scripts/device-session.sh`.

pub mod display;
pub mod system;

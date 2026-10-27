//! `pda-device stylus`: the system-wide stylus daemon.
//!
//! Creates the virtual pointer first, then finds and grabs the
//! touchscreen, so input is never lost to a half-started daemon. If the
//! touchscreen disappears the held button is released and the daemon waits
//! for it to come back. SIGHUP reloads the calibration; SIGTERM/SIGINT
//! release everything and exit (the kernel also drops the grab if the
//! process dies), after which the X server gets the raw touchscreen back.

use super::display::Rotation;
use super::evdev::{list_devices, EvdevDevice, VirtualPointer};
use super::stylus::{self, Translator};
use crate::config::Config;
use crate::input::gesture::GestureConfig;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

static STOP: AtomicBool = AtomicBool::new(false);
static RELOAD: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(sig: libc::c_int) {
    if sig == libc::SIGHUP {
        RELOAD.store(true, Ordering::SeqCst);
    } else {
        STOP.store(true, Ordering::SeqCst);
    }
}

fn install_signals() {
    for sig in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
        // SAFETY: the handler only stores to atomics. No SA_RESTART, so
        // poll() returns EINTR and the loop sees the flag at once.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = on_signal as usize;
            libc::sigemptyset(&mut action.sa_mask);
            libc::sigaction(sig, &action, std::ptr::null_mut());
        }
    }
}

pub struct Options {
    /// Overrides `[stylus] device`.
    pub device: Option<String>,
    pub rotation: Rotation,
    pub screen: (u32, u32),
}

/// Wait for input on `fd` until `timeout`; false on timeout or signal.
fn wait_readable(fd: i32, timeout: Duration) -> bool {
    let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
    let ms = timeout.as_millis().min(i32::MAX as u128) as i32;
    // SAFETY: one valid pollfd.
    let r = unsafe { libc::poll(&mut pfd, 1, ms) };
    r > 0
}

fn now_ms(start: Instant) -> u64 {
    start.elapsed().as_millis() as u64
}

/// The touchscreen to use now, by the configured selector (a udev symlink
/// is resolved to its event node first).
fn find(selector: &str) -> Result<EvdevDevice, String> {
    let resolved = if selector.starts_with('/') {
        std::fs::canonicalize(selector).map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| selector.to_string())
    } else {
        selector.to_string()
    };
    let devices = list_devices();
    let chosen = stylus::choose(&devices, &resolved)?;
    EvdevDevice::open(Path::new(&chosen.path)).map_err(|e| format!("{}: {e}", chosen.path))
}

pub fn run(opts: Options) -> Result<(), String> {
    install_signals();
    let load = || {
        let cfg = Config::load();
        let selector = opts.device.clone().unwrap_or_else(|| cfg.stylus.device.clone());
        (cfg, selector)
    };
    let (mut cfg, mut selector) = load();
    let mut virt = VirtualPointer::create(opts.screen)
        .map_err(|e| format!("cannot create the virtual pointer (/dev/uinput): {e}"))?;
    log::info!(
        "virtual pointer {:?} for a {}x{} screen, rotation {}",
        stylus::VIRTUAL_NAME,
        opts.screen.0,
        opts.screen.1,
        opts.rotation.name()
    );
    let start = Instant::now();
    let mut last_error = String::new();

    while !STOP.load(Ordering::SeqCst) {
        if RELOAD.swap(false, Ordering::SeqCst) {
            (cfg, selector) = load();
        }
        let mut dev = match find(&selector) {
            Ok(d) => d,
            Err(e) => {
                if e != last_error {
                    log::warn!("{e}; waiting for the touchscreen");
                    last_error = e;
                }
                std::thread::sleep(Duration::from_millis(1000));
                continue;
            }
        };
        last_error.clear();
        if let Err(e) = dev.grab() {
            log::error!("cannot grab {} ({}): {e}", dev.info.path, dev.info.name);
            std::thread::sleep(Duration::from_millis(1000));
            continue;
        }
        let transform = stylus::transform(&stylus::effective(&cfg.stylus), opts.rotation);
        log::info!(
            "translating {} ({}, {:04x}:{:04x}), x {}..{}, y {}..{}, hold {} ms -> right click{}",
            dev.info.path,
            dev.info.name,
            dev.info.vendor,
            dev.info.product,
            dev.x.min,
            dev.x.max,
            dev.y.min,
            dev.y.max,
            cfg.input.long_press_ms,
            if cfg.stylus.right_click { "" } else { " (off)" }
        );
        let mut t = Translator::new(
            GestureConfig::from(&cfg.input),
            cfg.stylus.right_click,
            transform,
            dev.x,
            dev.y,
            opts.screen,
        );
        loop {
            if STOP.load(Ordering::SeqCst) {
                break;
            }
            if RELOAD.swap(false, Ordering::SeqCst) {
                (cfg, selector) = load();
                t.set_transform(stylus::transform(&stylus::effective(&cfg.stylus), opts.rotation));
                log::info!("calibration reloaded");
            }
            let now = now_ms(start);
            let timeout = t.deadline(now).map_or(Duration::from_millis(1000), |d| Duration::from_millis(d - now));
            let mut out = Vec::new();
            if wait_readable(dev.fd(), timeout) {
                match dev.read() {
                    Ok(events) => {
                        let now = now_ms(start);
                        for ev in events {
                            out.extend(t.feed(now, ev));
                        }
                    }
                    Err(e) => {
                        log::warn!("{} gone ({e}); releasing buttons and waiting for it", dev.info.path);
                        let _ = virt.emit(&t.reset());
                        break;
                    }
                }
            }
            out.extend(t.tick(now_ms(start)));
            if !out.is_empty() {
                if let Err(e) = virt.emit(&out) {
                    log::error!("virtual pointer: {e}");
                }
            }
        }
        let _ = virt.emit(&t.reset());
        drop(dev);
    }
    log::info!("stopping; the touchscreen goes back to the X server");
    Ok(())
}

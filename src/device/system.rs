//! Facts about the machine the session runs on, for benchmark records and
//! the power primitives the hardware offers (backlight, suspend, battery).
//! Read from /proc and /sys only; nothing is changed.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

/// `MemTotal` / `MemAvailable` from /proc/meminfo, in KiB.
pub fn meminfo(text: &str, key: &str) -> Option<u64> {
    text.lines()
        .find_map(|l| l.strip_prefix(key)?.strip_prefix(':'))
        .and_then(|v| v.split_whitespace().next()?.parse().ok())
}

/// `PRETTY_NAME` from os-release.
pub fn os_name(os_release: &str) -> Option<String> {
    os_release
        .lines()
        .find_map(|l| l.strip_prefix("PRETTY_NAME="))
        .map(|v| v.trim_matches('"').to_string())
}

/// Sleep states the kernel offers (/sys/power/state, mem_sleep).
pub fn sleep_states(state: &str, mem_sleep: &str) -> Vec<String> {
    let mut out: Vec<String> = state.split_whitespace().map(str::to_string).collect();
    // mem_sleep: "s2idle [deep]" (the bracketed one is in use).
    if out.iter().any(|s| s == "mem") {
        let kinds: Vec<&str> = mem_sleep.split_whitespace().collect();
        if !kinds.is_empty() {
            let i = out.iter().position(|s| s == "mem").unwrap();
            out[i] = format!("mem ({})", kinds.join(" "));
        }
    }
    out
}

fn read(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path).map(|s| s.trim_end_matches(['\0', '\n']).trim().to_string()).unwrap_or_default()
}

fn list(dir: &str) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    names.sort();
    names
}

/// A report of the board, OS and power capabilities, one `key: value` per
/// line (docs/test-results.md records it with every measurement).
pub fn report() -> String {
    let mut r = String::new();
    let model = read("/proc/device-tree/model");
    let model = if model.is_empty() { read("/sys/class/dmi/id/product_name") } else { model };
    let mem = read("/proc/meminfo");
    let mib = |k| meminfo(&mem, k).map_or("?".into(), |v| format!("{} MiB", v / 1024));
    let cpus = read("/proc/cpuinfo").lines().filter(|l| l.starts_with("processor")).count();
    let _ = writeln!(r, "board: {}", if model.is_empty() { "unknown (not a device tree board)" } else { &model });
    let _ = writeln!(r, "cpus: {cpus} ({})", std::env::consts::ARCH);
    let _ = writeln!(r, "memory: {} total, {} available", mib("MemTotal"), mib("MemAvailable"));
    let _ = writeln!(r, "os: {}", os_name(&read("/etc/os-release")).unwrap_or_else(|| "?".into()));
    let _ = writeln!(r, "kernel: {}", read("/proc/sys/kernel/osrelease"));
    let governor = read("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor");
    let max_khz = read("/sys/devices/system/cpu/cpu0/cpufreq/scaling_max_freq");
    let _ = writeln!(
        r,
        "cpufreq: {}",
        if governor.is_empty() { "not available".to_string() } else { format!("{governor}, max {max_khz} kHz") }
    );
    let in_container = Path::new("/.dockerenv").exists() || read("/proc/1/cgroup").contains("docker");
    let _ = writeln!(r, "container: {}", if in_container { "yes (not the device)" } else { "no" });

    let backlights = list("/sys/class/backlight");
    let _ = writeln!(
        r,
        "backlight: {}",
        if backlights.is_empty() {
            "none (brightness only on the panel itself; the screen can still be blanked with DPMS)".to_string()
        } else {
            backlights
                .iter()
                .map(|b| {
                    let d = format!("/sys/class/backlight/{b}");
                    format!("{b} {}/{}", read(format!("{d}/brightness")), read(format!("{d}/max_brightness")))
                })
                .collect::<Vec<_>>()
                .join(", ")
        }
    );
    let states = sleep_states(&read("/sys/power/state"), &read("/sys/power/mem_sleep"));
    let _ = writeln!(
        r,
        "sleep states: {}",
        if states.is_empty() { "none offered by the kernel".to_string() } else { states.join(", ") }
    );
    let supplies: Vec<String> = list("/sys/class/power_supply")
        .into_iter()
        .map(|s| {
            let d = format!("/sys/class/power_supply/{s}");
            let cap = read(format!("{d}/capacity"));
            format!("{s} ({}{})", read(format!("{d}/type")), if cap.is_empty() { String::new() } else { format!(", {cap} %") })
        })
        .collect();
    let _ = writeln!(r, "power supplies: {}", if supplies.is_empty() { "none".to_string() } else { supplies.join(", ") });
    // A power key reaches logind (HandlePowerKey) if an input device has one.
    let power_keys: Vec<String> = list("/sys/class/input")
        .into_iter()
        .filter(|e| e.starts_with("event"))
        .filter(|e| has_power_key(&read(format!("/sys/class/input/{e}/device/capabilities/key"))))
        .map(|e| format!("{e} ({})", read(format!("/sys/class/input/{e}/device/name"))))
        .collect();
    let _ = writeln!(r, "power key: {}", if power_keys.is_empty() { "none".to_string() } else { power_keys.join(", ") });
    r
}

/// Whether a sysfs key capability bitmap (hex words, most significant
/// first) has KEY_POWER (116).
pub fn has_power_key(bitmap: &str) -> bool {
    const KEY_POWER: usize = 116;
    let words: Vec<&str> = bitmap.split_whitespace().collect();
    let bits = usize::BITS as usize;
    let (word, bit) = (KEY_POWER / bits, KEY_POWER % bits);
    words
        .len()
        .checked_sub(word + 1)
        .and_then(|i| u64::from_str_radix(words[i], 16).ok())
        .is_some_and(|w| w & (1 << bit) != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_lines() {
        let text = "MemTotal:        2013992 kB\nMemFree:  10 kB\nMemAvailable:    1534000 kB\n";
        assert_eq!(meminfo(text, "MemTotal"), Some(2013992));
        assert_eq!(meminfo(text, "MemAvailable"), Some(1534000));
        assert_eq!(meminfo(text, "Mem"), None);
        assert_eq!(meminfo(text, "SwapTotal"), None);
    }

    #[test]
    fn os_release() {
        assert_eq!(os_name("ID=debian\nPRETTY_NAME=\"Debian GNU/Linux 13 (trixie)\"\n").as_deref(), Some("Debian GNU/Linux 13 (trixie)"));
        assert_eq!(os_name("ID=debian\n"), None);
    }

    #[test]
    fn sleep_state_list() {
        assert_eq!(sleep_states("freeze mem", "s2idle [deep]"), vec!["freeze", "mem (s2idle [deep])"]);
        assert_eq!(sleep_states("freeze", ""), vec!["freeze"]);
        assert!(sleep_states("", "").is_empty());
    }

    #[test]
    fn power_key_bit() {
        // KEY_POWER is bit 116: word 1 (bits 64..127), bit 52 of it.
        if usize::BITS == 64 {
            assert!(has_power_key("10000000000000 0"));
            assert!(!has_power_key("0 10000000000000"));
            assert!(!has_power_key("0"));
            assert!(!has_power_key(""));
        }
    }
}

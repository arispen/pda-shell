//! Real system status for the status bar, read from sysfs. When the
//! information is not available nothing is shown: no placeholder values.

use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Battery {
    pub percent: u8,
    pub charging: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Network {
    pub interface: String,
    pub wireless: bool,
}

fn read(p: &Path) -> Option<String> {
    fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

/// First battery under `root` (normally /sys/class/power_supply).
pub fn battery(root: &Path) -> Option<Battery> {
    let mut entries: Vec<_> = fs::read_dir(root).ok()?.flatten().map(|e| e.path()).collect();
    entries.sort();
    entries.into_iter().find_map(|dir| {
        if read(&dir.join("type"))? != "Battery" {
            return None;
        }
        if read(&dir.join("present")).is_some_and(|p| p == "0") {
            return None;
        }
        let percent: u8 = read(&dir.join("capacity"))?.parse::<u32>().ok()?.min(100) as u8;
        let status = read(&dir.join("status")).unwrap_or_default();
        Some(Battery { percent, charging: status == "Charging" || status == "Full" })
    })
}

/// Connected network interfaces under `root` (normally /sys/class/net),
/// wireless first.
pub fn networks(root: &Path) -> Vec<Network> {
    let Ok(dir) = fs::read_dir(root) else { return Vec::new() };
    let mut out: Vec<Network> = dir
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let path = e.path();
            if name == "lo" || read(&path.join("operstate"))? != "up" {
                return None;
            }
            Some(Network { wireless: path.join("wireless").exists(), interface: name })
        })
        .collect();
    out.sort_by(|a, b| b.wireless.cmp(&a.wireless).then(a.interface.cmp(&b.interface)));
    out
}

pub fn battery_icon(b: &Battery) -> String {
    let level = (b.percent as u32 + 5) / 10 * 10;
    if b.charging {
        format!("battery-level-{level}-charging-symbolic")
    } else {
        format!("battery-level-{level}-symbolic")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pda-shell-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn write(dir: &Path, file: &str, value: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(file), format!("{value}\n")).unwrap();
    }

    #[test]
    fn no_power_supply_means_no_battery() {
        let root = tmp("nobat");
        write(&root.join("AC"), "type", "Mains");
        assert_eq!(battery(&root), None);
        assert_eq!(battery(&root.join("missing")), None);
    }

    #[test]
    fn reads_battery() {
        let root = tmp("bat");
        let b = root.join("BAT0");
        write(&b, "type", "Battery");
        write(&b, "capacity", "57");
        write(&b, "status", "Discharging");
        assert_eq!(battery(&root), Some(Battery { percent: 57, charging: false }));
        write(&b, "status", "Charging");
        assert!(battery(&root).unwrap().charging);
        write(&b, "present", "0");
        assert_eq!(battery(&root), None);
    }

    #[test]
    fn battery_icon_levels() {
        assert_eq!(battery_icon(&Battery { percent: 57, charging: false }), "battery-level-60-symbolic");
        assert_eq!(battery_icon(&Battery { percent: 3, charging: true }), "battery-level-0-charging-symbolic");
        assert_eq!(battery_icon(&Battery { percent: 100, charging: false }), "battery-level-100-symbolic");
    }

    #[test]
    fn lists_connected_interfaces() {
        let root = tmp("net");
        write(&root.join("lo"), "operstate", "unknown");
        write(&root.join("eth0"), "operstate", "up");
        write(&root.join("eth1"), "operstate", "down");
        write(&root.join("wlan0"), "operstate", "up");
        fs::create_dir_all(root.join("wlan0/wireless")).unwrap();
        let n = networks(&root);
        assert_eq!(
            n,
            vec![
                Network { interface: "wlan0".into(), wireless: true },
                Network { interface: "eth0".into(), wireless: false }
            ]
        );
    }
}

//! System information for Settings → About. Only real values; anything
//! that cannot be read is left out.

use std::path::Path;

/// "PRETTY_NAME" from an os-release file.
pub fn os_name(os_release: &str) -> Option<String> {
    os_release.lines().find_map(|l| {
        let v = l.strip_prefix("PRETTY_NAME=")?;
        Some(v.trim_matches('"').to_string())
    })
}

/// (available, total) memory in MiB from /proc/meminfo contents.
pub fn memory_mib(meminfo: &str) -> Option<(u64, u64)> {
    let field = |name: &str| -> Option<u64> {
        meminfo
            .lines()
            .find(|l| l.starts_with(name))?
            .split_whitespace()
            .nth(1)?
            .parse::<u64>()
            .ok()
    };
    Some((field("MemAvailable:")? / 1024, field("MemTotal:")? / 1024))
}

/// (label, value) pairs for the About screen.
pub fn system_info() -> Vec<(&'static str, String)> {
    let read = |p: &str| std::fs::read_to_string(p).ok();
    let mut rows = Vec::new();
    if let Some(host) = read("/proc/sys/kernel/hostname") {
        rows.push(("Device name", host.trim().to_string()));
    }
    if let Some(os) = read("/etc/os-release").as_deref().and_then(os_name) {
        rows.push(("Operating system", os));
    }
    if let Some(k) = read("/proc/sys/kernel/osrelease") {
        rows.push(("Kernel", format!("Linux {} ({})", k.trim(), std::env::consts::ARCH)));
    }
    if let Some((avail, total)) = read("/proc/meminfo").as_deref().and_then(memory_mib) {
        rows.push(("Memory", format!("{avail} MB free of {total} MB")));
    }
    rows.push(("Shell", format!("pda-shell {}", env!("CARGO_PKG_VERSION"))));
    rows
}

/// Replace the home directory prefix with "~".
pub fn tilde(p: &Path) -> String {
    let s = p.display().to_string();
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && s.starts_with(&home) => format!("~{}", &s[home.len()..]),
        _ => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_os_release() {
        let text = "ID=debian\nPRETTY_NAME=\"Debian GNU/Linux 13 (trixie)\"\nVERSION_ID=\"13\"\n";
        assert_eq!(os_name(text).as_deref(), Some("Debian GNU/Linux 13 (trixie)"));
        assert_eq!(os_name("ID=x\n"), None);
    }

    #[test]
    fn parses_meminfo() {
        let text = "MemTotal:        8048576 kB\nMemFree:  100 kB\nMemAvailable:    4024288 kB\n";
        assert_eq!(memory_mib(text), Some((3929, 7859)));
        assert_eq!(memory_mib("MemTotal: 1 kB\n"), None);
    }
}

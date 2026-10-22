//! System information shown on Today. Only real values; anything that
//! cannot be read is left out.

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

pub fn system_lines() -> Vec<String> {
    let read = |p: &str| std::fs::read_to_string(p).ok();
    let mut lines = Vec::new();
    let host = read("/proc/sys/kernel/hostname").map(|s| s.trim().to_string());
    let os = read("/etc/os-release").as_deref().and_then(os_name);
    let kernel = read("/proc/sys/kernel/osrelease").map(|s| s.trim().to_string());
    let first: Vec<String> = [host, os].into_iter().flatten().collect();
    if !first.is_empty() {
        lines.push(first.join(" · "));
    }
    if let Some(k) = kernel {
        lines.push(format!("Linux {k} ({})", std::env::consts::ARCH));
    }
    if let Some((avail, total)) = read("/proc/meminfo").as_deref().and_then(memory_mib) {
        lines.push(format!("Memory {avail} MB free of {total} MB"));
    }
    lines
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

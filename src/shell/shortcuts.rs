//! User-edited Today shortcuts. The configured list is the default; once the
//! user adds or removes a shortcut the result is saved per user.

use std::io;
use std::path::PathBuf;

fn path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("pda-shell/today-shortcuts"))
}

pub fn parse(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(String::from)
        .collect()
}

pub fn serialize(ids: &[String]) -> String {
    let mut s = String::from("# Today shortcuts, one desktop entry ID per line.\n");
    for id in ids {
        s.push_str(id);
        s.push('\n');
    }
    s
}

pub fn load(defaults: &[String]) -> Vec<String> {
    match path().map(std::fs::read_to_string) {
        Some(Ok(text)) => parse(&text),
        _ => defaults.to_vec(),
    }
}

pub fn save(ids: &[String]) -> io::Result<()> {
    let p = path().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no config directory"))?;
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(p, serialize(ids))
}

pub fn add(ids: &mut Vec<String>, id: &str) {
    if !ids.iter().any(|i| i == id) {
        ids.push(id.to_string());
    }
}

pub fn remove(ids: &mut Vec<String>, id: &str) {
    ids.retain(|i| i != id);
}

/// How many recently started programs the Start menu shows.
pub const RECENT_APPS: usize = 6;

/// Move `id` to the front of the recently started programs.
pub fn add_recent(ids: &mut Vec<String>, id: &str) {
    ids.retain(|i| i != id);
    ids.insert(0, id.to_string());
    ids.truncate(RECENT_APPS);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let ids = vec!["a.desktop".to_string(), "b.desktop".to_string()];
        assert_eq!(parse(&serialize(&ids)), ids);
    }

    #[test]
    fn parse_skips_comments_and_blanks() {
        assert_eq!(parse("# x\n\n  a.desktop \n"), vec!["a.desktop"]);
    }

    #[test]
    fn add_and_remove() {
        let mut ids = vec!["a".to_string()];
        add(&mut ids, "b");
        add(&mut ids, "a");
        assert_eq!(ids, vec!["a", "b"]);
        remove(&mut ids, "a");
        assert_eq!(ids, vec!["b"]);
    }

    #[test]
    fn recent_programs_are_most_recent_first_and_bounded() {
        let mut ids = Vec::new();
        for i in 0..8 {
            add_recent(&mut ids, &i.to_string());
        }
        add_recent(&mut ids, "5");
        assert_eq!(ids, vec!["5", "7", "6", "4", "3", "2"]);
    }
}

//! The document list on Today: recently used documents (GTK's recent files)
//! topped up with the newest files under ~/Documents, so a fresh session or
//! documents saved by programs that do not record recent files still show.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Doc {
    pub path: PathBuf,
    /// Seconds since the epoch: last use, or last modification.
    pub modified: i64,
}

/// Newest first, without duplicates, at most `n`.
pub fn merge(mut docs: Vec<Doc>, n: usize) -> Vec<Doc> {
    docs.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.path.cmp(&b.path)));
    let mut out: Vec<Doc> = Vec::new();
    for d in docs {
        if !out.iter().any(|o| o.path == d.path) {
            out.push(d);
        }
        if out.len() == n {
            break;
        }
    }
    out
}

/// Regular, non-hidden files under `root`, at most `depth` folders deep.
pub fn scan(root: &Path, depth: usize) -> Vec<Doc> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else { return out };
    for e in entries.flatten() {
        if e.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let Ok(meta) = e.metadata() else { continue };
        if meta.is_dir() {
            if depth > 0 {
                out.extend(scan(&e.path(), depth - 1));
            }
        } else if meta.is_file() {
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs() as i64);
            out.push(Doc { path: e.path(), modified });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(p: &str, t: i64) -> Doc {
        Doc { path: PathBuf::from(p), modified: t }
    }

    #[test]
    fn newest_first_without_duplicates() {
        let docs = vec![doc("/a", 10), doc("/b", 30), doc("/a", 50), doc("/c", 20)];
        let m = merge(docs, 10);
        assert_eq!(m, vec![doc("/a", 50), doc("/b", 30), doc("/c", 20)]);
        assert_eq!(merge(vec![doc("/a", 1), doc("/b", 2), doc("/c", 3)], 2).len(), 2);
    }

    #[test]
    fn scan_skips_hidden_and_respects_depth() {
        let root = std::env::temp_dir().join(format!("pda-recent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("a/b/c")).unwrap();
        std::fs::create_dir_all(root.join(".hidden")).unwrap();
        for f in ["top.txt", "a/one.txt", "a/b/two.txt", "a/b/c/three.txt", ".dot", ".hidden/x.txt"] {
            std::fs::write(root.join(f), "x").unwrap();
        }
        let mut names: Vec<String> = scan(&root, 2)
            .into_iter()
            .map(|d| d.path.strip_prefix(&root).unwrap().display().to_string())
            .collect();
        names.sort();
        assert_eq!(names, vec!["a/b/two.txt", "a/one.txt", "top.txt"]);
        std::fs::remove_dir_all(&root).unwrap();
    }
}

//! File operations behind Files: copy, move, find and properties. No GTK
//! here; the long-running ones take a cancel flag and a progress callback
//! and run on a worker thread in the program.
//!
//! Safety rules: nothing at a destination is overwritten in place. A copied
//! file is written under a temporary name and renamed when complete, so an
//! interrupted copy never leaves a half file under the real name. Replacing
//! an existing item is an explicit choice, and the caller moves the old one
//! to the trash first (`Plan::replace`). A move removes the source only
//! after the copy succeeded.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Copy,
    Move,
}

/// What to do with an item whose name exists in the destination already.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Collision {
    /// The existing item goes to the trash, the new one takes its place.
    Replace,
    /// The new item gets a free name ("report 2.txt").
    KeepBoth,
    Skip,
}

/// "report 2.txt", "report 3.txt", ... (before the extension), or `name`
/// itself when it is free in `dir`. Folders and dot files get the number at
/// the end ("Projects 2", ".config 2").
pub fn free_name(dir: &Path, name: &str) -> String {
    if fs::symlink_metadata(dir.join(name)).is_err() {
        return name.to_string();
    }
    let (stem, ext) = split_ext(dir, name);
    (2..)
        .map(|i| format!("{stem} {i}{ext}"))
        .find(|n| fs::symlink_metadata(dir.join(n)).is_err())
        .expect("a free name")
}

fn split_ext<'a>(dir: &Path, name: &'a str) -> (&'a str, &'a str) {
    if dir.join(name).is_dir() {
        return (name, "");
    }
    match name.rfind('.') {
        Some(i) if i > 0 => name.split_at(i),
        _ => (name, ""),
    }
}

fn name_of(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// One item of a paste: where it comes from and where it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub from: PathBuf,
    pub to: PathBuf,
    /// An existing item at `to` that has to go to the trash first.
    pub replace: bool,
}

/// What a paste will do, worked out before anything is touched.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub steps: Vec<Step>,
    /// Sources that will not be pasted, with the reason.
    pub refused: Vec<(PathBuf, String)>,
}

impl Plan {
    /// Items that will replace an existing one.
    pub fn replace(&self) -> impl Iterator<Item = &Path> {
        self.steps.iter().filter(|s| s.replace).map(|s| s.to.as_path())
    }
}

/// Sources whose name exists in `dest` already. A source pasted into its
/// own folder is not a conflict: a copy gets a new name, a move does nothing.
pub fn conflicts(sources: &[PathBuf], dest: &Path) -> Vec<PathBuf> {
    sources
        .iter()
        .filter(|s| s.parent() != Some(dest) && fs::symlink_metadata(dest.join(name_of(s))).is_ok())
        .cloned()
        .collect()
}

/// Plan pasting `sources` into `dest`.
pub fn plan(sources: &[PathBuf], dest: &Path, mode: Mode, collision: Collision) -> Plan {
    let mut plan = Plan::default();
    let dest_real = fs::canonicalize(dest).unwrap_or_else(|_| dest.to_path_buf());
    for src in sources {
        let name = name_of(src);
        if fs::symlink_metadata(src).is_err() {
            plan.refused.push((src.clone(), "it no longer exists".into()));
            continue;
        }
        if fs::symlink_metadata(src).is_ok_and(|m| m.is_dir()) {
            let src_real = fs::canonicalize(src).unwrap_or_else(|_| src.clone());
            if dest_real.starts_with(&src_real) {
                plan.refused.push((src.clone(), "a folder cannot go into itself".into()));
                continue;
            }
        }
        if src.parent() == Some(dest) {
            if mode == Mode::Move {
                continue;
            }
            let to = dest.join(free_name(dest, &name));
            plan.steps.push(Step { from: src.clone(), to, replace: false });
            continue;
        }
        let target = dest.join(&name);
        if fs::symlink_metadata(&target).is_err() {
            plan.steps.push(Step { from: src.clone(), to: target, replace: false });
            continue;
        }
        match collision {
            Collision::Skip => plan.refused.push((src.clone(), "skipped, the name exists".into())),
            Collision::KeepBoth => {
                plan.steps.push(Step { from: src.clone(), to: dest.join(free_name(dest, &name)), replace: false })
            }
            Collision::Replace => plan.steps.push(Step { from: src.clone(), to: target, replace: true }),
        }
    }
    plan
}

/// Progress of a running paste or scan.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Progress {
    pub done_bytes: u64,
    pub total_bytes: u64,
    pub done_items: usize,
    pub total_items: usize,
}

impl Progress {
    pub fn fraction(&self) -> f64 {
        if self.total_bytes > 0 {
            self.done_bytes as f64 / self.total_bytes as f64
        } else if self.total_items > 0 {
            self.done_items as f64 / self.total_items as f64
        } else {
            0.0
        }
    }
}

/// Outcome of a paste.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub done: Vec<PathBuf>,
    pub failed: Vec<(PathBuf, String)>,
    pub cancelled: bool,
}

fn cancelled_error() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "cancelled")
}

/// Count files and bytes below `path` (symbolic links are not followed).
pub fn measure(path: &Path, cancel: &AtomicBool, limit: usize) -> (u64, usize, bool) {
    let mut bytes = 0;
    let mut items = 0;
    let mut stack = vec![path.to_path_buf()];
    while let Some(p) = stack.pop() {
        if cancel.load(Ordering::Relaxed) || items >= limit {
            return (bytes, items, false);
        }
        let Ok(meta) = fs::symlink_metadata(&p) else { continue };
        items += 1;
        if meta.is_dir() {
            if let Ok(rd) = fs::read_dir(&p) {
                stack.extend(rd.flatten().map(|e| e.path()));
            }
        } else {
            bytes += meta.len();
        }
    }
    (bytes, items, true)
}

/// Run the steps of `plan`. `trash` moves an existing item out of the way
/// (Files passes GIO's trash); `progress` is called after each file.
pub fn execute(
    plan: &Plan,
    mode: Mode,
    cancel: &AtomicBool,
    trash: &dyn Fn(&Path) -> io::Result<()>,
    progress: &mut dyn FnMut(Progress),
) -> Report {
    let mut report = Report::default();
    let mut p = Progress { total_items: plan.steps.len(), ..Default::default() };
    for s in &plan.steps {
        let (b, _, _) = measure(&s.from, cancel, usize::MAX);
        p.total_bytes += b;
    }
    progress(p);
    for step in &plan.steps {
        if cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            break;
        }
        let result = run_step(step, mode, cancel, trash, &mut p, progress);
        match result {
            Ok(()) => report.done.push(step.to.clone()),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {
                report.cancelled = true;
                break;
            }
            Err(e) => report.failed.push((step.from.clone(), e.to_string())),
        }
        p.done_items += 1;
        progress(p);
    }
    report
}

fn run_step(
    step: &Step,
    mode: Mode,
    cancel: &AtomicBool,
    trash: &dyn Fn(&Path) -> io::Result<()>,
    p: &mut Progress,
    progress: &mut dyn FnMut(Progress),
) -> io::Result<()> {
    if step.replace {
        // Never trash the source itself (e.g. a link to it).
        if fs::canonicalize(&step.to).ok() == fs::canonicalize(&step.from).ok() {
            return Err(io::Error::other("source and destination are the same item"));
        }
        trash(&step.to)?;
    } else if fs::symlink_metadata(&step.to).is_ok() {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, "the name exists already"));
    }
    if mode == Mode::Move && fs::rename(&step.from, &step.to).is_ok() {
        let (b, _, _) = measure(&step.to, cancel, usize::MAX);
        p.done_bytes += b;
        progress(*p);
        return Ok(());
    }
    // Copy, or a move across file systems: copy, then remove the source.
    if let Err(e) = copy_tree(&step.from, &step.to, cancel, p, progress) {
        // Leave nothing half-copied behind under the new name.
        remove_tree(&step.to);
        return Err(e);
    }
    if mode == Mode::Move {
        let removed =
            if fs::symlink_metadata(&step.from)?.is_dir() { fs::remove_dir_all(&step.from) } else { fs::remove_file(&step.from) };
        removed.map_err(|e| io::Error::new(e.kind(), format!("copied, but the original stayed: {e}")))?;
    }
    Ok(())
}

fn remove_tree(p: &Path) {
    match fs::symlink_metadata(p) {
        Ok(m) if m.is_dir() => {
            let _ = fs::remove_dir_all(p);
        }
        Ok(_) => {
            let _ = fs::remove_file(p);
        }
        Err(_) => {}
    }
}

/// Copy a file, folder or symbolic link to `to` (which must not exist).
fn copy_tree(
    from: &Path,
    to: &Path,
    cancel: &AtomicBool,
    p: &mut Progress,
    progress: &mut dyn FnMut(Progress),
) -> io::Result<()> {
    if cancel.load(Ordering::Relaxed) {
        return Err(cancelled_error());
    }
    let meta = fs::symlink_metadata(from)?;
    if meta.file_type().is_symlink() {
        let target = fs::read_link(from)?;
        std::os::unix::fs::symlink(target, to)?;
    } else if meta.is_dir() {
        fs::create_dir(to)?;
        let mut entries: Vec<_> = fs::read_dir(from)?.collect::<Result<_, _>>()?;
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            copy_tree(&e.path(), &to.join(e.file_name()), cancel, p, progress)?;
        }
        let _ = fs::set_permissions(to, meta.permissions());
    } else {
        let tmp = to.with_file_name(format!(".{}.pda-partial", name_of(to)));
        let copied = copy_file(from, &tmp, cancel, p, progress).and_then(|()| {
            if fs::symlink_metadata(to).is_ok() {
                return Err(io::Error::new(io::ErrorKind::AlreadyExists, "the name exists already"));
            }
            fs::rename(&tmp, to)
        });
        if copied.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        copied?;
    }
    Ok(())
}

fn copy_file(
    from: &Path,
    to: &Path,
    cancel: &AtomicBool,
    p: &mut Progress,
    progress: &mut dyn FnMut(Progress),
) -> io::Result<()> {
    use std::io::{Read, Write};
    let mut src = fs::File::open(from)?;
    let mut dst = fs::OpenOptions::new().write(true).create_new(true).open(to)?;
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(cancelled_error());
        }
        let n = src.read(&mut buf)?;
        if n == 0 {
            break;
        }
        dst.write_all(&buf[..n])?;
        p.done_bytes += n as u64;
        progress(*p);
    }
    dst.sync_all()?;
    let meta = src.metadata()?;
    let _ = fs::set_permissions(to, meta.permissions());
    if let Ok(m) = meta.modified() {
        let _ = dst.set_modified(m);
    }
    Ok(())
}

/// Limits of a search, so that it cannot run away on slow or huge storage.
#[derive(Debug, Clone, Copy)]
pub struct FindLimits {
    pub max_results: usize,
    pub max_visited: usize,
    pub max_depth: usize,
}

impl Default for FindLimits {
    fn default() -> Self {
        Self { max_results: 200, max_visited: 50_000, max_depth: 12 }
    }
}

/// Why a search stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindEnd {
    Complete,
    TooManyResults,
    TooManyFiles,
    Cancelled,
}

/// Find files and folders below `root` whose name contains `query`
/// (case-insensitive), breadth first, nearest first. Symbolic links to
/// folders are not followed. `found` gets each match as it is found.
pub fn find(
    root: &Path,
    query: &str,
    show_hidden: bool,
    limits: FindLimits,
    cancel: &AtomicBool,
    found: &mut dyn FnMut(&Path),
) -> FindEnd {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return FindEnd::Complete;
    }
    let mut queue = std::collections::VecDeque::from([(root.to_path_buf(), 0usize)]);
    let (mut visited, mut results) = (0, 0);
    while let Some((dir, depth)) = queue.pop_front() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name().to_string_lossy().to_lowercase());
        for e in entries {
            if cancel.load(Ordering::Relaxed) {
                return FindEnd::Cancelled;
            }
            visited += 1;
            if visited > limits.max_visited {
                return FindEnd::TooManyFiles;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            if !show_hidden && name.starts_with('.') {
                continue;
            }
            if name.to_lowercase().contains(&q) {
                found(&e.path());
                results += 1;
                if results >= limits.max_results {
                    return FindEnd::TooManyResults;
                }
            }
            if depth + 1 < limits.max_depth && e.file_type().is_ok_and(|t| t.is_dir()) {
                queue.push_back((e.path(), depth + 1));
            }
        }
    }
    FindEnd::Complete
}

/// What Properties shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Properties {
    pub name: String,
    pub kind: String,
    pub location: PathBuf,
    pub size: u64,
    pub modified: i64,
    pub is_dir: bool,
    pub link_target: Option<PathBuf>,
    pub read_only: bool,
}

pub fn properties(path: &Path, kind: impl FnOnce(&Path) -> String) -> io::Result<Properties> {
    let lmeta = fs::symlink_metadata(path)?;
    let link_target = lmeta.file_type().is_symlink().then(|| fs::read_link(path).ok()).flatten();
    let meta = fs::metadata(path).unwrap_or(lmeta);
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64);
    let read_only = {
        let c = std::ffi::CString::new(std::os::unix::ffi::OsStrExt::as_bytes(path.as_os_str())).unwrap_or_default();
        // SAFETY: access(2) only reads the NUL-terminated path.
        unsafe { libc::access(c.as_ptr(), libc::W_OK) != 0 }
    };
    Ok(Properties {
        name: name_of(path),
        kind: kind(path),
        location: path.parent().map(Path::to_path_buf).unwrap_or_default(),
        size: meta.len(),
        modified,
        is_dir: meta.is_dir(),
        link_target,
        read_only,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            let d = std::env::temp_dir().join(format!("pda-fileops-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&d);
            fs::create_dir_all(&d).unwrap();
            Self(d)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn no_cancel() -> AtomicBool {
        AtomicBool::new(false)
    }

    fn run(sources: &[PathBuf], dest: &Path, mode: Mode, c: Collision) -> Report {
        let plan = plan(sources, dest, mode, c);
        let trash = |p: &Path| -> io::Result<()> {
            let bin = p.parent().unwrap().join(".trash-test");
            fs::create_dir_all(&bin)?;
            fs::rename(p, bin.join(name_of(p)))
        };
        execute(&plan, mode, &no_cancel(), &trash, &mut |_| {})
    }

    #[test]
    fn free_names_keep_the_extension() {
        let t = TempDir::new("names");
        fs::write(t.0.join("report.txt"), "x").unwrap();
        fs::write(t.0.join("report 2.txt"), "x").unwrap();
        fs::create_dir(t.0.join("Projects.old")).unwrap();
        fs::write(t.0.join(".bashrc"), "x").unwrap();
        assert_eq!(free_name(&t.0, "report.txt"), "report 3.txt");
        assert_eq!(free_name(&t.0, "Projects.old"), "Projects.old 2");
        assert_eq!(free_name(&t.0, ".bashrc"), ".bashrc 2");
        assert_eq!(free_name(&t.0, "Zażółć ąę.ods"), "Zażółć ąę.ods");
    }

    #[test]
    fn copies_files_and_folders_with_contents() {
        let t = TempDir::new("copy");
        let (a, b) = (t.0.join("a"), t.0.join("b"));
        fs::create_dir_all(a.join("Folder/Empty")).unwrap();
        fs::write(a.join("Folder/inner.txt"), "inner").unwrap();
        fs::write(a.join("Ünïcode name with a long tail.txt"), "hello").unwrap();
        fs::create_dir(&b).unwrap();
        let r = run(&[a.join("Folder"), a.join("Ünïcode name with a long tail.txt")], &b, Mode::Copy, Collision::Skip);
        assert!(r.failed.is_empty() && !r.cancelled, "{r:?}");
        assert_eq!(fs::read_to_string(b.join("Folder/inner.txt")).unwrap(), "inner");
        assert!(b.join("Folder/Empty").is_dir());
        assert_eq!(fs::read_to_string(b.join("Ünïcode name with a long tail.txt")).unwrap(), "hello");
        // Sources stay.
        assert!(a.join("Folder/inner.txt").exists());
        // No temporary files left over.
        assert!(fs::read_dir(&b).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().contains("pda-partial")));
    }

    #[test]
    fn copy_into_the_same_folder_gets_a_new_name() {
        let t = TempDir::new("dup");
        fs::write(t.0.join("doc.txt"), "1").unwrap();
        let r = run(&[t.0.join("doc.txt")], &t.0, Mode::Copy, Collision::Skip);
        assert_eq!(r.done, vec![t.0.join("doc 2.txt")]);
        assert_eq!(fs::read_to_string(t.0.join("doc 2.txt")).unwrap(), "1");
        // Moving into the folder it is in does nothing.
        assert!(plan(&[t.0.join("doc.txt")], &t.0, Mode::Move, Collision::Replace).steps.is_empty());
    }

    #[test]
    fn collisions_never_overwrite_silently() {
        let t = TempDir::new("collide");
        let (a, b) = (t.0.join("a"), t.0.join("b"));
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        fs::write(a.join("x.txt"), "new").unwrap();
        fs::write(b.join("x.txt"), "old").unwrap();
        let src = [a.join("x.txt")];
        assert_eq!(conflicts(&src, &b), src.to_vec());

        let r = run(&src, &b, Mode::Copy, Collision::Skip);
        assert!(r.done.is_empty() && fs::read_to_string(b.join("x.txt")).unwrap() == "old");

        let r = run(&src, &b, Mode::Copy, Collision::KeepBoth);
        assert_eq!(r.done, vec![b.join("x 2.txt")]);
        assert_eq!(fs::read_to_string(b.join("x.txt")).unwrap(), "old");

        let r = run(&src, &b, Mode::Copy, Collision::Replace);
        assert_eq!(r.done, vec![b.join("x.txt")]);
        assert_eq!(fs::read_to_string(b.join("x.txt")).unwrap(), "new");
        // The replaced file went to the trash, not away.
        assert_eq!(fs::read_to_string(b.join(".trash-test/x.txt")).unwrap(), "old");
    }

    #[test]
    fn moves_remove_the_source_only_when_done() {
        let t = TempDir::new("move");
        let (a, b) = (t.0.join("a"), t.0.join("b"));
        fs::create_dir_all(a.join("Dir")).unwrap();
        fs::write(a.join("Dir/f"), "f").unwrap();
        fs::create_dir_all(&b).unwrap();
        let r = run(&[a.join("Dir")], &b, Mode::Move, Collision::Skip);
        assert!(r.failed.is_empty());
        assert!(!a.join("Dir").exists());
        assert_eq!(fs::read_to_string(b.join("Dir/f")).unwrap(), "f");
    }

    #[test]
    fn a_folder_cannot_go_into_itself() {
        let t = TempDir::new("self");
        fs::create_dir_all(t.0.join("A/B")).unwrap();
        for mode in [Mode::Copy, Mode::Move] {
            let p = plan(&[t.0.join("A")], &t.0.join("A/B"), mode, Collision::KeepBoth);
            assert!(p.steps.is_empty(), "{mode:?}");
            assert_eq!(p.refused.len(), 1);
        }
        // But a copy next to itself is fine.
        let p = plan(&[t.0.join("A")], &t.0, Mode::Copy, Collision::KeepBoth);
        assert_eq!(p.steps[0].to, t.0.join("A 2"));
    }

    #[test]
    fn vanished_sources_are_refused() {
        let t = TempDir::new("gone");
        let p = plan(&[t.0.join("missing")], &t.0, Mode::Copy, Collision::Skip);
        assert!(p.steps.is_empty());
        assert_eq!(p.refused.len(), 1);
    }

    #[test]
    fn permission_failures_are_reported_and_leave_nothing_behind() {
        let t = TempDir::new("perm");
        let (a, b) = (t.0.join("a"), t.0.join("b"));
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        fs::write(a.join("ok.txt"), "ok").unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&b, fs::Permissions::from_mode(0o555)).unwrap();
        let r = run(&[a.join("ok.txt")], &b, Mode::Move, Collision::Skip);
        fs::set_permissions(&b, fs::Permissions::from_mode(0o755)).unwrap();
        // Root may write anyway (container tests); then the move just works.
        if r.failed.is_empty() {
            return;
        }
        assert!(a.join("ok.txt").exists(), "a failed move keeps the source");
        assert_eq!(fs::read_dir(&b).unwrap().count(), 0);
    }

    #[test]
    fn cancelling_stops_and_cleans_up() {
        let t = TempDir::new("cancel");
        let (a, b) = (t.0.join("a"), t.0.join("b"));
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        fs::write(a.join("big"), vec![7u8; 2 * 1024 * 1024]).unwrap();
        let cancel = AtomicBool::new(false);
        let p = plan(&[a.join("big")], &b, Mode::Copy, Collision::Skip);
        let r = execute(&p, Mode::Copy, &cancel, &|_| Ok(()), &mut |pr| {
            if pr.done_bytes > 0 {
                cancel.store(true, Ordering::Relaxed);
            }
        });
        assert!(r.cancelled);
        assert_eq!(fs::read_dir(&b).unwrap().count(), 0, "no partial file");
    }

    #[test]
    fn symlinks_are_copied_as_links() {
        let t = TempDir::new("links");
        let (a, b) = (t.0.join("a"), t.0.join("b"));
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        std::os::unix::fs::symlink("..", a.join("loop")).unwrap();
        let r = run(&[a.join("loop")], &b, Mode::Copy, Collision::Skip);
        assert!(r.failed.is_empty());
        assert_eq!(fs::read_link(b.join("loop")).unwrap(), PathBuf::from(".."));
    }

    #[test]
    fn find_is_bounded_and_nearest_first() {
        let t = TempDir::new("find");
        fs::create_dir_all(t.0.join("deep/deeper")).unwrap();
        fs::write(t.0.join("deep/deeper/Budget old.xlsx"), "").unwrap();
        fs::write(t.0.join("budget.csv"), "").unwrap();
        fs::write(t.0.join(".budget-hidden"), "").unwrap();
        std::os::unix::fs::symlink(&t.0, t.0.join("deep/loop")).unwrap();
        let mut out = Vec::new();
        let end = find(&t.0, "BUDGET", false, FindLimits::default(), &no_cancel(), &mut |p| out.push(p.to_path_buf()));
        assert_eq!(end, FindEnd::Complete);
        assert_eq!(out, vec![t.0.join("budget.csv"), t.0.join("deep/deeper/Budget old.xlsx")]);

        let mut n = 0;
        let limits = FindLimits { max_results: 1, ..Default::default() };
        assert_eq!(find(&t.0, "budget", true, limits, &no_cancel(), &mut |_| n += 1), FindEnd::TooManyResults);
        assert_eq!(n, 1);
        let limits = FindLimits { max_depth: 1, ..Default::default() };
        let mut out = Vec::new();
        find(&t.0, "budget", false, limits, &no_cancel(), &mut |p| out.push(p.to_path_buf()));
        assert_eq!(out, vec![t.0.join("budget.csv")]);
        assert_eq!(find(&t.0, "x", false, FindLimits::default(), &AtomicBool::new(true), &mut |_| {}), FindEnd::Cancelled);
    }

    #[test]
    fn properties_and_folder_sizes() {
        let t = TempDir::new("props");
        fs::create_dir_all(t.0.join("F/G")).unwrap();
        fs::write(t.0.join("F/a"), "12345").unwrap();
        fs::write(t.0.join("F/G/b"), "123").unwrap();
        let p = properties(&t.0.join("F/a"), |_| "Text".into()).unwrap();
        assert_eq!((p.name.as_str(), p.size, p.is_dir, p.location.clone()), ("a", 5, false, t.0.join("F")));
        let (bytes, items, complete) = measure(&t.0.join("F"), &no_cancel(), 100);
        assert_eq!((bytes, items, complete), (8, 4, true));
        let (_, _, complete) = measure(&t.0.join("F"), &no_cancel(), 2);
        assert!(!complete);
    }
}

//! Files: a Pocket PC File Explorer style frontend for the file system.
//! One folder at a time as a single-column list, folders first. Tap opens a
//! folder or a document (with its default program); hold the stylus for
//! the item's commands. Deleting moves to the trash.
//!
//! Selection mode (Edit > Select, or Select in an item's menu) puts a check
//! box on every row; taps then select instead of open, and Cut, Copy,
//! Delete and Properties work on the selection. Paste copies or moves the
//! clipboard into the folder on a worker thread with a progress strip;
//! names that exist already are never overwritten without asking
//! (`fileops`). Find searches by name below the folder, bounded and
//! cancellable.
//!
//! Command bar menus: File (New folder, Find, Properties, Open in Linux file
//! manager), Edit (Select, Select all, Cut, Copy, Paste, Rename, Delete),
//! Go (Up, Home, Documents, Show hidden files, sort order); in selection
//! mode, Find and Properties: Done plus what applies. Every command is a
//! window action, shared by the menus and the hold-the-stylus menus. Thunar
//! stays available as the advanced Linux file manager.

use super::fileops::{self, Collision, FindEnd, FindLimits, Mode, Progress, Report};
use super::{action, menu, short_time, AskBar, NativeWindow};
use crate::config::Config;
use crate::ui::MenuItem;
use gtk::gio;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub const APP_ID: &str = "io.github.pda_shell.Files";
/// Folder sizes in Properties stop counting here.
const MEASURE_LIMIT: usize = 100_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileInfo {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    Name,
    /// Newest first.
    Date,
}

fn info(path: PathBuf) -> Option<FileInfo> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    // Follows symbolic links, like a file manager shows them; a dangling
    // link still gets a row.
    let meta = std::fs::metadata(&path).or_else(|_| std::fs::symlink_metadata(&path)).ok()?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64);
    Some(FileInfo { path, name, is_dir: meta.is_dir(), size: meta.len(), modified })
}

/// The entries of `dir`: folders first, then by `sort`.
pub fn list_dir(dir: &Path, show_hidden: bool, sort: Sort) -> std::io::Result<Vec<FileInfo>> {
    let mut out: Vec<FileInfo> = std::fs::read_dir(dir)?
        .flatten()
        .filter(|e| show_hidden || !e.file_name().to_string_lossy().starts_with('.'))
        // Half-copied files of a running paste.
        .filter(|e| !e.file_name().to_string_lossy().ends_with(".pda-partial"))
        .filter_map(|e| info(e.path()))
        .collect();
    out.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then_with(|| match sort {
            Sort::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            Sort::Date => b.modified.cmp(&a.modified),
        })
    });
    Ok(out)
}

/// "512 B", "4.2 KB", "1.3 MB".
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut v = bytes as f64 / 1024.0;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    format!("{v:.1} {}", UNITS[unit])
}

/// A name a file or folder can have.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains('/') && !name.contains('\0')
}

/// `base`, or "base 2", "base 3", ... if that exists in `dir`.
pub fn unique_name(dir: &Path, base: &str) -> String {
    if !dir.join(base).exists() {
        return base.to_string();
    }
    (2..).map(|i| format!("{base} {i}")).find(|n| !dir.join(n).exists()).expect("a free name")
}

/// Whether renaming `from` to `to` would replace another item. A name that
/// differs only in case is the item itself on a case-insensitive file
/// system, but may be a different item on a case-sensitive one.
pub fn rename_would_replace(from: &Path, to: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::symlink_metadata(from), std::fs::symlink_metadata(to)) {
        (Ok(a), Ok(b)) => (a.dev(), a.ino()) != (b.dev(), b.ino()),
        (_, Ok(_)) => true,
        (_, Err(_)) => false,
    }
}

/// "1 item", "3 items".
pub fn items(n: usize) -> String {
    if n == 1 {
        "1 item".into()
    } else {
        format!("{n} items")
    }
}

/// What a finished paste tells the user, if anything: nothing when all went
/// well, otherwise what was done and what was not, and why.
pub fn paste_message(mode: Mode, report: &Report, refused: &[(PathBuf, String)]) -> Option<String> {
    let verb = if mode == Mode::Copy { "Copied" } else { "Moved" };
    let mut problems: Vec<String> = refused
        .iter()
        .chain(report.failed.iter())
        .map(|(p, why)| format!("“{}”: {why}", file_name(p)))
        .collect();
    if report.cancelled {
        problems.push("stopped before the end".into());
    }
    if problems.is_empty() {
        return None;
    }
    const SHOWN: usize = 4;
    let more = problems.len().saturating_sub(SHOWN);
    problems.truncate(SHOWN);
    if more > 0 {
        problems.push(format!("and {more} more"));
    }
    Some(format!("{verb} {}. Not done: {}.", items(report.done.len()), problems.join("; ")))
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
}

/// "~/Documents" for display.
fn tilde(p: &Path) -> String {
    crate::ui::settings::info::tilde(p)
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

/// "Folder", or the content type's description ("Plain text document").
fn kind(path: &Path) -> String {
    if path.is_dir() {
        return "Folder".into();
    }
    let (ct, _) = gio::content_type_guess(Some(path), None::<&[u8]>);
    gio::content_type_get_description(&ct).to_string()
}

/// What the window shows; each has its own command bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Folder,
    Select,
    Find,
    Properties,
}

struct Files {
    win: NativeWindow,
    cfg: Config,
    dir: RefCell<PathBuf>,
    title: gtk::Label,
    path: gtk::Label,
    up: gtk::Button,
    stack: gtk::Stack,
    list: gtk::ListBox,
    note: gtk::Label,
    ask: AskBar,
    show_hidden: Cell<bool>,
    sort: Cell<Sort>,
    monitor: RefCell<Option<gio::FileMonitor>>,
    menubar: gio::Menu,
    view: Cell<View>,
    /// Selection mode: the strip with the count, and the selected paths.
    select_bar: gtk::Box,
    select_count: gtk::Label,
    selected: RefCell<BTreeSet<PathBuf>>,
    checks: RefCell<Vec<(PathBuf, gtk::CheckButton, gtk::ListBoxRow)>>,
    /// Cut or copied items.
    clipboard: RefCell<Option<(Mode, Vec<PathBuf>)>>,
    /// A running paste.
    progress_bar: gtk::Box,
    progress: gtk::ProgressBar,
    progress_label: gtk::Label,
    busy: RefCell<Option<Arc<AtomicBool>>>,
    /// Find.
    find_entry: gtk::Entry,
    find_scope: gtk::Label,
    find_status: gtk::Label,
    find_list: gtk::ListBox,
    find_button: gtk::Button,
    find_cancel: RefCell<Option<Arc<AtomicBool>>>,
    /// Properties.
    props: gtk::ListBox,
    props_cancel: RefCell<Option<Arc<AtomicBool>>>,
    /// Actions that need a selection, or something on the clipboard.
    selection_actions: Vec<gio::SimpleAction>,
    paste: gio::SimpleAction,
}

thread_local! {
    static FILES: RefCell<Option<Rc<Files>>> = const { RefCell::new(None) };
}

fn this() -> Option<Rc<Files>> {
    FILES.with(|f| f.borrow().clone())
}

/// `f` on the program, if it is running.
fn with(f: impl Fn(&Files) + 'static) -> impl Fn() + 'static {
    move || {
        if let Some(files) = this() {
            f(&files);
        }
    }
}

/// Entry point of `pda-files`: `pda-files [FOLDER-or-URI] [--new-folder]`.
pub fn main() -> glib::ExitCode {
    super::run(APP_ID, "pda-files", |app, cfg, args| {
        let files = this().unwrap_or_else(|| {
            let f = Files::new(app, cfg);
            FILES.with(|cell| *cell.borrow_mut() = Some(f.clone()));
            f
        });
        let start = args
            .iter()
            .find(|a| !a.starts_with("--"))
            .map(|a| if a.contains("://") { gio::File::for_uri(a) } else { gio::File::for_commandline_arg(a) })
            .and_then(|f| f.path())
            .filter(|p| p.is_dir());
        let documents = home().join("Documents");
        let dir = start.unwrap_or_else(|| if documents.is_dir() { documents } else { home() });
        files.go(dir);
        if args.iter().any(|a| a == "--new-folder") {
            files.ask_new_folder();
        }
        files.win.window.present();
    })
}

/// `f` on the program and `path`, if the program is running.
fn on_path(path: &Path, f: impl Fn(&Files, PathBuf) + 'static) -> impl Fn() + 'static {
    let path = path.to_path_buf();
    move || {
        if let Some(files) = this() {
            f(&files, path.clone());
        }
    }
}

fn strip(class: &str) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    b.add_css_class("ask-bar");
    b.add_css_class(class);
    b.set_visible(false);
    b
}

fn scrolled(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).vexpand(true).child(child).build()
}

fn new_list() -> gtk::ListBox {
    let list = gtk::ListBox::new();
    list.add_css_class("compact");
    list.add_css_class("targets");
    list.add_css_class("lined");
    list.set_selection_mode(gtk::SelectionMode::None);
    list.set_activate_on_single_click(true);
    list
}

fn button(label: &str, f: impl Fn() + 'static) -> gtk::Button {
    let b = gtk::Button::with_label(label);
    b.update_property(&[gtk::accessible::Property::Label(label)]);
    b.connect_clicked(move |_| f());
    b
}

impl Files {
    fn new(app: &gtk::Application, cfg: &Config) -> Rc<Self> {
        let win = NativeWindow::new(app, "Files", "pda-app-files", cfg);
        let pad = cfg.design.padding_px as i32;

        let header = gtk::Box::new(gtk::Orientation::Horizontal, pad);
        header.add_css_class("today-header");
        header.append(&crate::ui::named_icon("pda-folder", cfg.design.icon_px));
        let names = gtk::Box::new(gtk::Orientation::Vertical, 0);
        names.set_hexpand(true);
        let title = gtk::Label::new(None);
        title.add_css_class("item-title");
        title.set_xalign(0.0);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let path = gtk::Label::new(None);
        path.add_css_class("dim");
        path.set_xalign(0.0);
        path.set_ellipsize(gtk::pango::EllipsizeMode::Start);
        names.append(&title);
        names.append(&path);
        header.append(&names);
        let up = button("Up", with(Files::go_up));
        header.append(&up);

        let select_bar = strip("select-bar");
        let select_count = gtk::Label::new(None);
        select_count.set_hexpand(true);
        select_count.set_xalign(0.0);
        select_bar.append(&select_count);
        select_bar.append(&button("All", with(Files::select_all)));
        select_bar.append(&button("Cancel", with(|f| f.set_view(View::Folder))));

        let progress_bar = strip("progress-strip");
        progress_bar.set_orientation(gtk::Orientation::Vertical);
        let progress_label = gtk::Label::new(None);
        progress_label.set_xalign(0.0);
        progress_label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        let progress = gtk::ProgressBar::new();
        let progress_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        progress.set_hexpand(true);
        progress.set_valign(gtk::Align::Center);
        progress_row.append(&progress);
        progress_row.append(&button("Stop", with(|f| {
            if let Some(c) = f.busy.borrow().as_ref() {
                c.store(true, Ordering::Relaxed);
            }
        })));
        progress_bar.append(&progress_label);
        progress_bar.append(&progress_row);

        let list = new_list();
        let note = gtk::Label::new(None);
        note.add_css_class("dim");
        note.add_css_class("page-note");
        note.set_xalign(0.0);
        note.set_wrap(true);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&list);
        content.append(&note);

        // Find: a name field, where it looks, and the results.
        let find_entry = gtk::Entry::new();
        find_entry.set_placeholder_text(Some("Part of a file name"));
        find_entry.set_hexpand(true);
        find_entry.update_property(&[gtk::accessible::Property::Label("File name to find")]);
        find_entry.connect_activate(|_| {
            if let Some(f) = this() {
                f.start_find();
            }
        });
        let find_button = button("Find", with(Files::start_find));
        let find_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        find_row.append(&find_entry);
        find_row.append(&find_button);
        let find_scope = gtk::Label::new(None);
        find_scope.add_css_class("dim");
        find_scope.set_xalign(0.0);
        find_scope.set_ellipsize(gtk::pango::EllipsizeMode::Start);
        let find_head = gtk::Box::new(gtk::Orientation::Vertical, 4);
        find_head.add_css_class("ask-bar");
        find_head.append(&find_row);
        find_head.append(&find_scope);
        let find_list = new_list();
        let find_status = gtk::Label::new(None);
        find_status.add_css_class("dim");
        find_status.add_css_class("page-note");
        find_status.set_xalign(0.0);
        find_status.set_wrap(true);
        let find_results = gtk::Box::new(gtk::Orientation::Vertical, 0);
        find_results.append(&find_list);
        find_results.append(&find_status);
        let find_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        find_page.append(&find_head);
        find_page.append(&scrolled(&find_results));

        let props = new_list();
        props.remove_css_class("targets");

        let stack = gtk::Stack::new();
        stack.add_named(&scrolled(&content), Some("folder"));
        stack.add_named(&find_page, Some("find"));
        stack.add_named(&scrolled(&props), Some("properties"));

        let ask = AskBar::new();
        let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        page.append(&header);
        page.append(&ask.widget);
        page.append(&select_bar);
        page.append(&progress_bar);
        page.append(&stack);
        win.overlay.set_child(Some(&page));

        let menubar = gio::Menu::new();
        app.set_menubar(Some(&menubar));

        let w = &win.window;
        action(w, "new-folder", with(Files::ask_new_folder));
        action(w, "find", with(Files::show_find));
        action(w, "folder-properties", with(|f| f.show_properties(vec![f.dir.borrow().clone()])));
        action(w, "open-thunar", with(|f| {
            let dir = gio::File::for_path(&*f.dir.borrow());
            match gio::DesktopAppInfo::new("thunar.desktop") {
                Some(t) => {
                    if let Err(e) = t.launch(&[dir], gio::AppLaunchContext::NONE) {
                        log::error!("cannot start Thunar: {e}");
                    }
                }
                None => log::warn!("Thunar is not installed"),
            }
        }));
        action(w, "up", with(Files::go_up));
        action(w, "home", with(|f| f.go(home())));
        action(w, "documents", with(|f| f.go(home().join("Documents"))));
        action(w, "select", with(|f| f.set_view(View::Select)));
        action(w, "select-all", with(Files::select_all));
        action(w, "done", with(|f| f.set_view(View::Folder)));
        let selection_actions = vec![
            action(w, "cut", with(|f| f.cut_or_copy(Mode::Move, f.selection()))),
            action(w, "copy", with(|f| f.cut_or_copy(Mode::Copy, f.selection()))),
            action(w, "rename", with(|f| {
                if let [one] = f.selection().as_slice() {
                    f.ask_rename(one.clone());
                }
            })),
            action(w, "delete", with(|f| f.ask_delete(f.selection()))),
            action(w, "properties", with(|f| f.show_properties(f.selection()))),
        ];
        let paste = action(w, "paste", with(|f| f.paste_into(f.dir.borrow().clone())));
        let hidden = gio::SimpleAction::new_stateful("show-hidden", None, &false.to_variant());
        hidden.connect_activate(|a, _| {
            let on = !a.state().and_then(|s| s.get::<bool>()).unwrap_or(false);
            a.set_state(&on.to_variant());
            if let Some(f) = this() {
                f.show_hidden.set(on);
                f.refresh();
            }
        });
        w.add_action(&hidden);
        let sort = gio::SimpleAction::new_stateful("sort", Some(glib::VariantTy::STRING), &"name".to_variant());
        sort.connect_change_state(|a, v| {
            let Some(value) = v.and_then(|v| v.get::<String>()) else { return };
            a.set_state(&value.to_variant());
            if let Some(f) = this() {
                f.sort.set(if value == "date" { Sort::Date } else { Sort::Name });
                f.refresh();
            }
        });
        w.add_action(&sort);

        let files = Rc::new(Self {
            win,
            cfg: cfg.clone(),
            dir: RefCell::new(home()),
            title,
            path,
            up,
            stack,
            list,
            note,
            ask,
            show_hidden: Cell::new(false),
            sort: Cell::new(Sort::Name),
            monitor: RefCell::new(None),
            menubar,
            view: Cell::new(View::Folder),
            select_bar,
            select_count,
            selected: Default::default(),
            checks: Default::default(),
            clipboard: RefCell::new(None),
            progress_bar,
            progress,
            progress_label,
            busy: RefCell::new(None),
            find_entry,
            find_scope,
            find_status,
            find_list,
            find_button,
            find_cancel: RefCell::new(None),
            props,
            props_cancel: RefCell::new(None),
            selection_actions,
            paste,
        });

        files.list.connect_row_activated(|_, row| {
            if let Some(f) = this() {
                let path = PathBuf::from(row.widget_name().as_str());
                if f.view.get() == View::Select {
                    f.toggle(&path);
                } else {
                    f.open(path);
                }
            }
        });
        files.find_list.connect_row_activated(|_, row| {
            if let Some(f) = this() {
                f.open(PathBuf::from(row.widget_name().as_str()));
            }
        });
        files.win.window.connect_close_request(|_| {
            if let Some(f) = this() {
                for c in [&f.busy, &f.find_cancel, &f.props_cancel] {
                    if let Some(c) = c.borrow().as_ref() {
                        c.store(true, Ordering::Relaxed);
                    }
                }
            }
            glib::Propagation::Proceed
        });
        files.set_view(View::Folder);
        files
    }

    /// Switch what the window shows, with its command bar.
    fn set_view(&self, view: View) {
        if view != View::Find {
            self.stop(&self.find_cancel);
        }
        if view != View::Properties {
            self.stop(&self.props_cancel);
        }
        let was = self.view.replace(view);
        if (was == View::Select) != (view == View::Select) {
            self.selected.borrow_mut().clear();
            for (_, check, row) in self.checks.borrow().iter() {
                check.set_visible(view == View::Select);
                check.set_active(false);
                row.remove_css_class("checked");
            }
        }
        self.select_bar.set_visible(view == View::Select);
        self.stack.set_visible_child_name(match view {
            View::Folder | View::Select => "folder",
            View::Find => "find",
            View::Properties => "properties",
        });
        if matches!(view, View::Folder | View::Select) {
            let name = file_name(&self.dir.borrow());
            self.win.window.set_title(Some(&format!("{name} - Files")));
        }
        self.menubar.remove_all();
        let edit = |items: &[(&str, &str)]| self.menubar.append_submenu(Some("Edit"), &menu(items));
        match view {
            View::Folder => {
                self.menubar.append_submenu(
                    Some("File"),
                    &menu(&[
                        ("New folder", "win.new-folder"),
                        ("Find…", "win.find"),
                        ("Properties", "win.folder-properties"),
                        ("Open in Linux file manager", "win.open-thunar"),
                    ]),
                );
                edit(&[("Select", "win.select"), ("Select all", "win.select-all"), ("Paste", "win.paste")]);
                let go = menu(&[("Up", "win.up"), ("Home", "win.home"), ("Documents", "win.documents")]);
                let shown = gio::Menu::new();
                shown.append(Some("Show hidden files"), Some("win.show-hidden"));
                for (label, value) in [("Sort by name", "name"), ("Sort by date", "date")] {
                    let item = gio::MenuItem::new(Some(label), None);
                    item.set_action_and_target_value(Some("win.sort"), Some(&value.to_variant()));
                    shown.append_item(&item);
                }
                go.append_section(None, &shown);
                self.menubar.append_submenu(Some("Go"), &go);
            }
            View::Select => {
                self.menubar.append(Some("Done"), Some("win.done"));
                edit(&[
                    ("Select all", "win.select-all"),
                    ("Cut", "win.cut"),
                    ("Copy", "win.copy"),
                    ("Rename…", "win.rename"),
                    ("Delete", "win.delete"),
                    ("Properties", "win.properties"),
                ]);
            }
            View::Find | View::Properties => self.menubar.append(Some("Done"), Some("win.done")),
        }
        self.update_selection();
    }

    fn stop(&self, flag: &RefCell<Option<Arc<AtomicBool>>>) {
        if let Some(c) = flag.borrow_mut().take() {
            c.store(true, Ordering::Relaxed);
        }
    }

    fn selection(&self) -> Vec<PathBuf> {
        self.selected.borrow().iter().cloned().collect()
    }

    /// The count strip and which commands apply.
    fn update_selection(&self) {
        let n = self.selected.borrow().len();
        self.select_count.set_text(&if n == 0 { "Tap items to select them".into() } else { format!("{} selected", items(n)) });
        for a in &self.selection_actions {
            let ok = match a.name().as_str() {
                "rename" => n == 1,
                _ => n > 0,
            };
            a.set_enabled(ok && self.view.get() == View::Select);
        }
        self.paste.set_enabled(self.clipboard.borrow().is_some() && self.busy.borrow().is_none());
    }

    fn toggle(&self, path: &Path) {
        let on = {
            let mut sel = self.selected.borrow_mut();
            if !sel.remove(path) {
                sel.insert(path.to_path_buf());
                true
            } else {
                false
            }
        };
        if let Some((_, check, row)) = self.checks.borrow().iter().find(|(p, _, _)| p == path) {
            check.set_active(on);
            if on {
                row.add_css_class("checked");
            } else {
                row.remove_css_class("checked");
            }
        }
        self.update_selection();
    }

    fn select_all(&self) {
        if self.view.get() != View::Select {
            self.set_view(View::Select);
        }
        let all: BTreeSet<PathBuf> = self.checks.borrow().iter().map(|(p, _, _)| p.clone()).collect();
        for (_, check, row) in self.checks.borrow().iter() {
            check.set_active(true);
            row.add_css_class("checked");
        }
        *self.selected.borrow_mut() = all;
        self.update_selection();
    }

    /// Enter selection mode with `path` selected.
    fn select(&self, path: &Path) {
        self.set_view(View::Select);
        self.toggle(path);
    }

    fn go(&self, dir: PathBuf) {
        self.ask.hide();
        if self.view.get() != View::Folder {
            self.set_view(View::Folder);
        }
        *self.dir.borrow_mut() = dir.clone();
        let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "/".into());
        self.title.set_text(&name);
        self.path.set_text(&tilde(&dir));
        self.win.window.set_title(Some(&format!("{name} - Files")));
        self.up.set_sensitive(dir.parent().is_some());
        // Follow changes made by other programs.
        *self.monitor.borrow_mut() = gio::File::for_path(&dir)
            .monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
            .ok()
            .inspect(|m| {
                m.connect_changed(|_, _, _, _| {
                    if let Some(f) = this() {
                        f.refresh();
                    }
                });
            });
        self.refresh();
    }

    fn go_up(&self) {
        let parent = self.dir.borrow().parent().map(Path::to_path_buf);
        if let Some(p) = parent {
            self.go(p);
        }
    }

    /// A list row for a file or folder; `sub` replaces the size and date.
    fn row(&self, e: &FileInfo, sub: Option<String>, in_folder: bool) -> gtk::ListBoxRow {
        let row = gtk::ListBoxRow::new();
        row.set_widget_name(&e.path.to_string_lossy());
        row.update_property(&[gtk::accessible::Property::Label(&e.name)]);
        let b = gtk::Box::new(gtk::Orientation::Horizontal, self.cfg.design.padding_px as i32);
        if in_folder {
            let check = gtk::CheckButton::new();
            // The row takes the tap; the box only shows the state.
            check.set_can_target(false);
            check.set_can_focus(false);
            check.update_property(&[gtk::accessible::Property::Label(&format!("Select {}", e.name))]);
            let on = self.selected.borrow().contains(&e.path);
            check.set_active(on);
            if on {
                row.add_css_class("checked");
            }
            check.set_visible(self.view.get() == View::Select);
            b.append(&check);
            self.checks.borrow_mut().push((e.path.clone(), check, row.clone()));
        }
        let icon = if e.is_dir {
            "pda-folder".to_string()
        } else {
            let (ct, _) = gio::content_type_guess(Some(&e.path), None::<&[u8]>);
            crate::ui::today::document_icon(&ct).to_string()
        };
        b.append(&crate::ui::named_icon(&icon, self.cfg.design.icon_px));
        let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
        text.set_valign(gtk::Align::Center);
        let name = gtk::Label::new(Some(&e.name));
        name.set_xalign(0.0);
        name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        let sub = sub.unwrap_or_else(|| {
            let detail = if e.is_dir { "Folder".to_string() } else { human_size(e.size) };
            format!("{detail} · {}", short_time(e.modified))
        });
        let sub = gtk::Label::new(Some(&sub));
        sub.add_css_class("dim");
        sub.set_xalign(0.0);
        sub.set_ellipsize(gtk::pango::EllipsizeMode::Start);
        text.append(&name);
        text.append(&sub);
        b.append(&text);
        row.set_child(Some(&b));
        let (path, is_dir) = (e.path.clone(), e.is_dir);
        crate::ui::context_menu(&row, &self.win.feedback, &self.cfg, move || Files::item_menu(&path, is_dir, in_folder));
        row
    }

    /// The hold-the-stylus menu of an item: the same commands as the menus.
    fn item_menu(path: &Path, is_dir: bool, in_folder: bool) -> Vec<MenuItem> {
        let Some(f) = this() else { return Vec::new() };
        let on = |f: fn(&Files, PathBuf)| on_path(path, f);
        let mut items = vec![MenuItem::new("Open", on(|f, p| f.open(p)))];
        if !in_folder {
            items.push(MenuItem::new("Open containing folder", on(|f, p| {
                if let Some(dir) = p.parent() {
                    f.go(dir.to_path_buf());
                }
            })));
        }
        // In selection mode the commands apply to the whole selection.
        let selecting = f.view.get() == View::Select && f.selected.borrow().contains(path);
        let targets = move |f: &Files, p: PathBuf| if selecting { f.selection() } else { vec![p] };
        if in_folder && f.view.get() != View::Select {
            items.push(MenuItem::new("Select", on(|f, p| f.select(&p))));
        }
        items.push(MenuItem::new("Cut", on_path(path, move |f, p| f.cut_or_copy(Mode::Move, targets(f, p)))));
        items.push(MenuItem::new("Copy", on_path(path, move |f, p| f.cut_or_copy(Mode::Copy, targets(f, p)))));
        if is_dir && f.clipboard.borrow().is_some() {
            items.push(MenuItem::new("Paste into folder", on(|f, p| f.paste_into(p))));
        }
        if !selecting || f.selected.borrow().len() == 1 {
            items.push(MenuItem::new("Rename…", on(|f, p| f.ask_rename(p))));
        }
        items.push(MenuItem::new("Delete", on_path(path, move |f, p| f.ask_delete(targets(f, p)))));
        items.push(MenuItem::new("Properties", on_path(path, move |f, p| f.show_properties(targets(f, p)))));
        items
    }

    fn refresh(&self) {
        self.list.remove_all();
        self.checks.borrow_mut().clear();
        let dir = self.dir.borrow().clone();
        let entries = match list_dir(&dir, self.show_hidden.get(), self.sort.get()) {
            Ok(e) => e,
            Err(e) => {
                self.note.set_text(&format!("Cannot open this folder: {e}"));
                self.note.set_visible(true);
                return;
            }
        };
        // Items that went away (here or in another program) leave the selection.
        self.selected.borrow_mut().retain(|p| entries.iter().any(|e| &e.path == p));
        self.note.set_text("This folder is empty.");
        self.note.set_visible(entries.is_empty());
        for e in &entries {
            let row = self.row(e, None, true);
            self.list.append(&row);
        }
        self.update_selection();
    }

    fn open(&self, path: PathBuf) {
        if path.is_dir() {
            self.go(path);
            return;
        }
        if !path.exists() {
            self.ask.tell(&format!("“{}” is no longer there.", file_name(&path)));
            return;
        }
        let uri = gio::File::for_path(&path).uri();
        let ctx = gtk::gdk::Display::default().map(|d| d.app_launch_context());
        if let Err(e) = gio::AppInfo::launch_default_for_uri(&uri, ctx.as_ref()) {
            log::error!("cannot open {uri}: {e}");
            self.ask.tell(&format!("No program opens “{}”: {e}", file_name(&path)));
        }
    }

    fn ask_new_folder(&self) {
        let dir = self.dir.borrow().clone();
        let suggestion = unique_name(&dir, "New folder");
        self.ask.ask_name("Name of the new folder:", &suggestion, "Create", move |name| {
            let Some(f) = this() else { return };
            if !valid_name(&name) {
                f.ask.tell(&format!("“{name}” cannot be a folder name."));
                return;
            }
            if let Err(e) = std::fs::create_dir(dir.join(&name)) {
                log::error!("cannot create folder {name}: {e}");
                f.ask.tell(&format!("Cannot create “{name}”: {e}"));
            }
            f.refresh();
        });
    }

    fn ask_rename(&self, path: PathBuf) {
        let old = file_name(&path);
        let current = old.clone();
        self.ask.ask_name(&format!("New name for “{old}”:"), &old, "Rename", move |name| {
            let Some(f) = this() else { return };
            if name == current {
                return;
            }
            if !valid_name(&name) {
                f.ask.tell(&format!("“{name}” cannot be a file name."));
                return;
            }
            let target = path.with_file_name(&name);
            if rename_would_replace(&path, &target) {
                f.ask.tell(&format!("“{name}” exists already. Nothing was renamed."));
                return;
            }
            match std::fs::rename(&path, &target) {
                Ok(()) => {
                    if f.selected.borrow_mut().remove(&path) {
                        f.selected.borrow_mut().insert(target);
                    }
                }
                Err(e) => {
                    log::error!("cannot rename {}: {e}", path.display());
                    f.ask.tell(&format!("Cannot rename “{current}”: {e}"));
                }
            }
            f.refresh();
        });
    }

    fn ask_delete(&self, paths: Vec<PathBuf>) {
        let question = match paths.as_slice() {
            [] => return,
            [one] => {
                let what = if one.is_dir() { "folder" } else { "file" };
                format!("Move the {what} “{}” to the trash?", file_name(one))
            }
            many => format!("Move {} to the trash?", items(many.len())),
        };
        self.ask.confirm(&question, "Delete", move || {
            let Some(f) = this() else { return };
            let mut failed = Vec::new();
            for p in &paths {
                if let Err(e) = gio::File::for_path(p).trash(gio::Cancellable::NONE) {
                    log::error!("cannot delete {}: {e}", p.display());
                    failed.push(format!("“{}”: {e}", file_name(p)));
                }
            }
            if f.view.get() == View::Select {
                f.set_view(View::Folder);
            }
            f.refresh();
            if !failed.is_empty() {
                f.ask.tell(&format!("Not moved to the trash: {}", failed.join("; ")));
            }
        });
    }

    fn cut_or_copy(&self, mode: Mode, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        *self.clipboard.borrow_mut() = Some((mode, paths));
        if self.view.get() == View::Select {
            self.set_view(View::Folder);
        }
        self.update_selection();
    }

    /// Paste the clipboard into `dest`, asking first about names that exist.
    fn paste_into(&self, dest: PathBuf) {
        let Some((mode, sources)) = self.clipboard.borrow().clone() else { return };
        if self.busy.borrow().is_some() {
            return;
        }
        let clash = fileops::conflicts(&sources, &dest);
        let question = match clash.as_slice() {
            [] => None,
            [one] => Some(format!("“{}” exists in “{}” already.", file_name(one), file_name(&dest))),
            many => Some(format!("{} exist in “{}” already.", items(many.len()), file_name(&dest))),
        };
        let run = move |c: Collision| {
            if let Some(f) = this() {
                f.run_paste(mode, sources.clone(), dest.clone(), c);
            }
        };
        let Some(question) = question else {
            run(Collision::Skip);
            return;
        };
        let answers = [(Collision::Replace, "Replace"), (Collision::KeepBoth, "Keep both"), (Collision::Skip, "Skip")];
        self.ask.choose(&question, &answers.map(|a| a.1), move |i| run(answers[i].0));
    }

    fn run_paste(&self, mode: Mode, sources: Vec<PathBuf>, dest: PathBuf, collision: Collision) {
        let plan = fileops::plan(&sources, &dest, mode, collision);
        if plan.steps.is_empty() {
            if let Some(m) = paste_message(mode, &Report::default(), &plan.refused) {
                self.ask.tell(&m);
            }
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        *self.busy.borrow_mut() = Some(cancel.clone());
        self.update_selection();
        let verb = if mode == Mode::Copy { "Copying" } else { "Moving" };
        self.progress_label.set_text(&format!("{verb} {} to “{}”…", items(plan.steps.len()), file_name(&dest)));
        self.progress.set_fraction(0.0);
        self.progress_bar.set_visible(true);

        enum Msg {
            Progress(Progress),
            Done(Report),
        }
        let (tx, rx) = async_channel::unbounded::<Msg>();
        let refused = plan.refused.clone();
        std::thread::spawn(move || {
            let trash = |p: &Path| gio::File::for_path(p).trash(gio::Cancellable::NONE).map_err(std::io::Error::other);
            let mut last = std::time::Instant::now();
            let report = fileops::execute(&plan, mode, &cancel, &trash, &mut |p| {
                if last.elapsed() > std::time::Duration::from_millis(100) {
                    last = std::time::Instant::now();
                    let _ = tx.send_blocking(Msg::Progress(p));
                }
            });
            let _ = tx.send_blocking(Msg::Done(report));
        });
        glib::spawn_future_local(async move {
            while let Ok(msg) = rx.recv().await {
                let Some(f) = this() else { return };
                match msg {
                    Msg::Progress(p) => {
                        f.progress.set_fraction(p.fraction());
                        f.progress_label.set_text(&format!(
                            "{verb} {} of {} ({} of {})…",
                            p.done_items + 1,
                            p.total_items,
                            human_size(p.done_bytes),
                            human_size(p.total_bytes)
                        ));
                    }
                    Msg::Done(report) => {
                        f.busy.borrow_mut().take();
                        f.progress_bar.set_visible(false);
                        log::info!("{verb}: {} done, {} failed", report.done.len(), report.failed.len());
                        // What was cut is gone from where it was; paste it only once.
                        if mode == Mode::Move && report.failed.is_empty() && !report.cancelled {
                            f.clipboard.borrow_mut().take();
                        }
                        f.refresh();
                        if let Some(m) = paste_message(mode, &report, &refused) {
                            f.ask.tell(&m);
                        }
                        return;
                    }
                }
            }
        });
    }

    fn show_find(&self) {
        self.find_scope.set_text(&format!("In {} and its folders", tilde(&self.dir.borrow())));
        self.set_view(View::Find);
        self.win.window.set_title(Some("Find - Files"));
        self.find_entry.grab_focus();
    }

    fn start_find(&self) {
        self.stop(&self.find_cancel);
        let query = self.find_entry.text().trim().to_string();
        self.find_list.remove_all();
        if query.is_empty() {
            self.find_status.set_text("Type part of a name, then tap Find.");
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        *self.find_cancel.borrow_mut() = Some(cancel.clone());
        self.find_status.set_text("Searching…");
        self.find_button.set_sensitive(false);
        let root = self.dir.borrow().clone();
        let hidden = self.show_hidden.get();

        enum Msg {
            Found(Vec<PathBuf>),
            End(FindEnd),
        }
        let (tx, rx) = async_channel::unbounded::<Msg>();
        let mine = cancel.clone();
        std::thread::spawn(move || {
            let mut batch = Vec::new();
            let mut last = std::time::Instant::now();
            let end = fileops::find(&root, &query, hidden, FindLimits::default(), &cancel, &mut |p| {
                batch.push(p.to_path_buf());
                if last.elapsed() > std::time::Duration::from_millis(150) {
                    last = std::time::Instant::now();
                    let _ = tx.send_blocking(Msg::Found(std::mem::take(&mut batch)));
                }
            });
            let _ = tx.send_blocking(Msg::Found(batch));
            let _ = tx.send_blocking(Msg::End(end));
        });
        let root = self.dir.borrow().clone();
        glib::spawn_future_local(async move {
            let mut count = 0;
            while let Ok(msg) = rx.recv().await {
                let Some(f) = this() else { return };
                // A newer search (or leaving Find) replaced this one: its
                // late results and end must not touch the page.
                let current = f.find_cancel.borrow().as_ref().is_some_and(|c| Arc::ptr_eq(c, &mine));
                if !current {
                    if f.find_cancel.borrow().is_none() {
                        f.find_button.set_sensitive(true);
                    }
                    return;
                }
                match msg {
                    Msg::Found(paths) => {
                        for p in paths.into_iter().filter_map(info) {
                            let folder = p.path.parent().map(|d| tilde(d)).unwrap_or_default();
                            let row = f.row(&p, Some(folder), false);
                            f.find_list.append(&row);
                            count += 1;
                        }
                        f.find_status.set_text(&format!("Searching… {count} found"));
                    }
                    Msg::End(end) => {
                        f.find_button.set_sensitive(true);
                        let found = match count {
                            0 => "Nothing found".to_string(),
                            1 => "1 found".to_string(),
                            n => format!("{n} found"),
                        };
                        let limits = FindLimits::default();
                        let why = match end {
                            FindEnd::Complete => String::new(),
                            FindEnd::Cancelled => " (stopped)".into(),
                            FindEnd::TooManyResults => format!("; stopped at {} results", limits.max_results),
                            FindEnd::TooManyFiles => {
                                format!("; stopped after {} files, search a smaller folder", limits.max_visited)
                            }
                        };
                        f.find_status.set_text(&format!("{found} in {}{why}.", tilde(&root)));
                        f.find_cancel.borrow_mut().take();
                        return;
                    }
                }
            }
        });
    }

    fn show_properties(&self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        self.props.remove_all();
        let add = |k: &str, v: &str| self.props.append(&crate::ui::kv_row(k, v));
        let size_row = crate::ui::kv_row("Size", "Calculating…");
        let label_of = |row: &gtk::ListBoxRow| {
            row.child().and_then(|b| b.last_child()).and_then(|l| l.downcast::<gtk::Label>().ok())
        };
        let measure: Vec<PathBuf>;
        if let [one] = paths.as_slice() {
            let p = match fileops::properties(one, kind) {
                Ok(p) => p,
                Err(e) => {
                    self.ask.tell(&format!("Cannot read “{}”: {e}", file_name(one)));
                    return;
                }
            };
            self.win.window.set_title(Some(&format!("{} - Properties", p.name)));
            add("Name", &p.name);
            add("Type", &p.kind);
            add("Location", &tilde(&p.location));
            if let Some(t) = &p.link_target {
                add("Link to", &t.display().to_string());
            }
            if p.is_dir {
                self.props.append(&size_row);
                measure = vec![one.clone()];
            } else {
                add("Size", &format!("{} ({} bytes)", human_size(p.size), p.size));
                measure = Vec::new();
            }
            add("Modified", &short_time(p.modified));
            add("Access", if p.read_only { "Read only" } else { "Read and write" });
        } else {
            self.win.window.set_title(Some("Properties"));
            add("Selected", &items(paths.len()));
            let folder = paths[0].parent().map(tilde).unwrap_or_default();
            add("Location", &folder);
            self.props.append(&size_row);
            measure = paths;
        }
        self.set_view(View::Properties);
        if measure.is_empty() {
            return;
        }
        // Folder sizes: counted on a worker thread, and only up to a limit.
        let cancel = Arc::new(AtomicBool::new(false));
        *self.props_cancel.borrow_mut() = Some(cancel.clone());
        let (tx, rx) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let (mut bytes, mut files, mut complete) = (0, 0, true);
            for p in &measure {
                let (b, n, c) = fileops::measure(p, &cancel, MEASURE_LIMIT.saturating_sub(files));
                bytes += b;
                files += n;
                complete &= c;
            }
            let _ = tx.send_blocking((bytes, files, complete));
        });
        let label = label_of(&size_row);
        glib::spawn_future_local(async move {
            if let (Ok((bytes, n, complete)), Some(label)) = (rx.recv().await, label) {
                label.set_text(&if complete {
                    format!("{} ({} bytes) in {}", human_size(bytes), bytes, items(n))
                } else {
                    format!("more than {} in over {MEASURE_LIMIT} items", human_size(bytes))
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_human_readable() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(4300), "4.2 KB");
        assert_eq!(human_size(1_400_000), "1.3 MB");
    }

    #[test]
    fn names() {
        assert!(valid_name("Budget 2026.xlsx"));
        for bad in ["", ".", "..", "a/b"] {
            assert!(!valid_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn listing_puts_folders_first_and_hides_dot_files() {
        let dir = std::env::temp_dir().join(format!("pda-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("zeta")).unwrap();
        std::fs::write(dir.join("alpha.txt"), "x").unwrap();
        std::fs::write(dir.join(".hidden"), "x").unwrap();
        std::fs::write(dir.join(".alpha.txt.pda-partial"), "x").unwrap();
        std::os::unix::fs::symlink(dir.join("missing"), dir.join("dangling")).unwrap();
        let names = |hidden| -> Vec<String> {
            list_dir(&dir, hidden, Sort::Name).unwrap().into_iter().map(|f| f.name).collect()
        };
        assert_eq!(names(false), vec!["zeta", "alpha.txt", "dangling"]);
        assert_eq!(names(true), vec!["zeta", ".hidden", "alpha.txt", "dangling"]);
        assert_eq!(unique_name(&dir, "zeta"), "zeta 2");
        assert_eq!(unique_name(&dir, "new"), "new");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn paste_messages_say_what_was_not_done() {
        let ok = Report { done: vec!["a".into()], ..Default::default() };
        assert_eq!(paste_message(Mode::Copy, &ok, &[]), None);
        let partial = Report {
            done: vec!["a".into()],
            failed: vec![("/x/b.txt".into(), "Permission denied".into())],
            cancelled: false,
        };
        let refused = vec![(PathBuf::from("/x/A"), "a folder cannot go into itself".to_string())];
        assert_eq!(
            paste_message(Mode::Move, &partial, &refused).unwrap(),
            "Moved 1 item. Not done: “A”: a folder cannot go into itself; “b.txt”: Permission denied."
        );
        let many: Vec<_> = (0..6).map(|i| (PathBuf::from(format!("{i}")), "x".to_string())).collect();
        let m = paste_message(Mode::Copy, &Report { cancelled: true, ..Default::default() }, &many).unwrap();
        assert!(m.starts_with("Copied 0 items.") && m.ends_with("and 3 more."), "{m}");
    }

    #[test]
    fn renaming_never_replaces_another_item() {
        let dir = std::env::temp_dir().join(format!("pda-files-rename-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("report.txt"), "a").unwrap();
        std::fs::write(dir.join("other.txt"), "b").unwrap();
        assert!(rename_would_replace(&dir.join("report.txt"), &dir.join("other.txt")));
        assert!(!rename_would_replace(&dir.join("report.txt"), &dir.join("new.txt")));
        // The item itself (a case-only rename on a case-insensitive file
        // system resolves to the same file) is not a collision.
        assert!(!rename_would_replace(&dir.join("report.txt"), &dir.join("report.txt")));
        // On a case-sensitive file system REPORT.TXT is another file.
        std::fs::write(dir.join("REPORT.TXT"), "c").unwrap();
        if std::fs::read_to_string(dir.join("report.txt")).unwrap() == "a" {
            assert!(rename_would_replace(&dir.join("report.txt"), &dir.join("REPORT.TXT")));
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

//! Files: a Pocket PC File Explorer style frontend for the file system.
//! One folder at a time as a single-column list, folders first. Tap opens a
//! folder or a document (with its default program); hold the stylus for
//! Rename and Delete. Deleting moves to the trash.
//!
//! Command bar menus: File (New folder, Open in Linux file manager), Go (Up,
//! Home, Documents), View (Show hidden files, sort order). Thunar stays
//! available as the advanced Linux file manager.

use super::{action, short_time, AskBar, NativeWindow};
use crate::config::Config;
use crate::ui::MenuItem;
use gtk::gio;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub const APP_ID: &str = "io.github.pda_shell.Files";

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

/// The entries of `dir`: folders first, then by `sort`.
pub fn list_dir(dir: &Path, show_hidden: bool, sort: Sort) -> std::io::Result<Vec<FileInfo>> {
    let mut out: Vec<FileInfo> = std::fs::read_dir(dir)?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if !show_hidden && name.starts_with('.') {
                return None;
            }
            // Follows symbolic links, like a file manager shows them.
            let meta = std::fs::metadata(e.path()).ok()?;
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs() as i64);
            Some(FileInfo { path: e.path(), name, is_dir: meta.is_dir(), size: meta.len(), modified })
        })
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

/// "~/Documents" for display.
fn tilde(p: &Path) -> String {
    crate::ui::settings::info::tilde(p)
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

struct Files {
    win: NativeWindow,
    cfg: Config,
    dir: RefCell<PathBuf>,
    title: gtk::Label,
    path: gtk::Label,
    up: gtk::Button,
    list: gtk::ListBox,
    note: gtk::Label,
    ask: AskBar,
    show_hidden: Cell<bool>,
    sort: Cell<Sort>,
    monitor: RefCell<Option<gio::FileMonitor>>,
}

thread_local! {
    static FILES: RefCell<Option<Rc<Files>>> = const { RefCell::new(None) };
}

fn this() -> Option<Rc<Files>> {
    FILES.with(|f| f.borrow().clone())
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

fn menu(items: &[(&str, &str)]) -> gio::Menu {
    let m = gio::Menu::new();
    for (label, action) in items {
        m.append(Some(label), Some(action));
    }
    m
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
        let up = gtk::Button::with_label("Up");
        up.update_property(&[gtk::accessible::Property::Label("Up")]);
        header.append(&up);

        let list = gtk::ListBox::new();
        list.add_css_class("compact");
        list.add_css_class("targets");
        list.add_css_class("lined");
        list.set_selection_mode(gtk::SelectionMode::None);
        list.set_activate_on_single_click(true);
        let note = gtk::Label::new(None);
        note.add_css_class("dim");
        note.add_css_class("page-note");
        note.set_xalign(0.0);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&list);
        content.append(&note);
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&content)
            .build();

        let ask = AskBar::new();
        let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        page.append(&header);
        page.append(&ask.widget);
        page.append(&scroll);
        win.overlay.set_child(Some(&page));

        let menubar = gio::Menu::new();
        menubar.append_submenu(
            Some("File"),
            &menu(&[("New folder", "win.new-folder"), ("Open in Linux file manager", "win.open-thunar")]),
        );
        menubar.append_submenu(
            Some("Go"),
            &menu(&[("Up", "win.up"), ("Home", "win.home"), ("Documents", "win.documents")]),
        );
        let view = gio::Menu::new();
        view.append(Some("Show hidden files"), Some("win.show-hidden"));
        let sort = gio::Menu::new();
        for (label, value) in [("Sort by name", "name"), ("Sort by date", "date")] {
            let item = gio::MenuItem::new(Some(label), None);
            item.set_action_and_target_value(Some("win.sort"), Some(&value.to_variant()));
            sort.append_item(&item);
        }
        view.append_section(None, &sort);
        menubar.append_submenu(Some("View"), &view);
        app.set_menubar(Some(&menubar));

        let files = Rc::new(Self {
            win,
            cfg: cfg.clone(),
            dir: RefCell::new(home()),
            title,
            path,
            up,
            list,
            note,
            ask,
            show_hidden: Cell::new(false),
            sort: Cell::new(Sort::Name),
            monitor: RefCell::new(None),
        });

        let w = &files.win.window;
        action(w, "new-folder", || {
            if let Some(f) = this() {
                f.ask_new_folder();
            }
        });
        action(w, "open-thunar", || {
            if let Some(f) = this() {
                let dir = gio::File::for_path(&*f.dir.borrow());
                match gio::DesktopAppInfo::new("thunar.desktop") {
                    Some(t) => {
                        if let Err(e) = t.launch(&[dir], gio::AppLaunchContext::NONE) {
                            log::error!("cannot start Thunar: {e}");
                        }
                    }
                    None => log::warn!("Thunar is not installed"),
                }
            }
        });
        action(w, "up", || {
            if let Some(f) = this() {
                f.go_up();
            }
        });
        action(w, "home", || {
            if let Some(f) = this() {
                f.go(home());
            }
        });
        action(w, "documents", || {
            if let Some(f) = this() {
                f.go(home().join("Documents"));
            }
        });
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

        files.up.connect_clicked(|_| {
            if let Some(f) = this() {
                f.go_up();
            }
        });
        files.list.connect_row_activated(|_, row| {
            if let Some(f) = this() {
                f.open(PathBuf::from(row.widget_name().as_str()));
            }
        });
        files
    }

    fn go(&self, dir: PathBuf) {
        self.ask.hide();
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

    fn refresh(&self) {
        self.list.remove_all();
        let dir = self.dir.borrow().clone();
        let entries = match list_dir(&dir, self.show_hidden.get(), self.sort.get()) {
            Ok(e) => e,
            Err(e) => {
                self.note.set_text(&format!("Cannot open this folder: {e}"));
                self.note.set_visible(true);
                return;
            }
        };
        self.note.set_text("This folder is empty.");
        self.note.set_visible(entries.is_empty());
        let px = self.cfg.design.icon_px;
        for e in entries {
            let row = gtk::ListBoxRow::new();
            row.set_widget_name(&e.path.to_string_lossy());
            row.update_property(&[gtk::accessible::Property::Label(&e.name)]);
            let b = gtk::Box::new(gtk::Orientation::Horizontal, self.cfg.design.padding_px as i32);
            let icon = if e.is_dir {
                "pda-folder".to_string()
            } else {
                let (ct, _) = gio::content_type_guess(Some(&e.path), None::<&[u8]>);
                crate::ui::today::document_icon(&ct).to_string()
            };
            b.append(&crate::ui::named_icon(&icon, px));
            let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
            text.set_valign(gtk::Align::Center);
            let name = gtk::Label::new(Some(&e.name));
            name.set_xalign(0.0);
            name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
            let detail = if e.is_dir { "Folder".to_string() } else { human_size(e.size) };
            let sub = gtk::Label::new(Some(&format!("{detail} · {}", short_time(e.modified))));
            sub.add_css_class("dim");
            sub.set_xalign(0.0);
            text.append(&name);
            text.append(&sub);
            b.append(&text);
            row.set_child(Some(&b));
            let path = e.path.clone();
            crate::ui::context_menu(&row, &self.win.feedback, &self.cfg, move || {
                let (p1, p2, p3) = (path.clone(), path.clone(), path.clone());
                vec![
                    MenuItem::new("Open", move || {
                        if let Some(f) = this() {
                            f.open(p1.clone());
                        }
                    }),
                    MenuItem::new("Rename…", move || {
                        if let Some(f) = this() {
                            f.ask_rename(p2.clone());
                        }
                    }),
                    MenuItem::new("Delete", move || {
                        if let Some(f) = this() {
                            f.ask_delete(p3.clone());
                        }
                    }),
                ]
            });
            self.list.append(&row);
        }
    }

    fn open(&self, path: PathBuf) {
        if path.is_dir() {
            self.go(path);
            return;
        }
        let uri = gio::File::for_path(&path).uri();
        let ctx = gtk::gdk::Display::default().map(|d| d.app_launch_context());
        if let Err(e) = gio::AppInfo::launch_default_for_uri(&uri, ctx.as_ref()) {
            log::error!("cannot open {uri}: {e}");
        }
    }

    fn ask_new_folder(&self) {
        let dir = self.dir.borrow().clone();
        let suggestion = unique_name(&dir, "New folder");
        self.ask.ask_name("Name of the new folder:", &suggestion, "Create", move |name| {
            if !valid_name(&name) {
                return;
            }
            if let Err(e) = std::fs::create_dir(dir.join(&name)) {
                log::error!("cannot create folder {name}: {e}");
            }
            if let Some(f) = this() {
                f.refresh();
            }
        });
    }

    fn ask_rename(&self, path: PathBuf) {
        let old = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let current = old.clone();
        self.ask.ask_name(&format!("New name for “{old}”:"), &old, "Rename", move |name| {
            if !valid_name(&name) || name == current {
                return;
            }
            let target = path.with_file_name(&name);
            if target.exists() {
                log::warn!("{} exists already", target.display());
                return;
            }
            if let Err(e) = std::fs::rename(&path, &target) {
                log::error!("cannot rename {}: {e}", path.display());
            }
            if let Some(f) = this() {
                f.refresh();
            }
        });
    }

    fn ask_delete(&self, path: PathBuf) {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let what = if path.is_dir() { "folder" } else { "file" };
        self.ask.confirm(&format!("Move the {what} “{name}” to the trash?"), "Delete", move || {
            if let Err(e) = gio::File::for_path(&path).trash(gio::Cancellable::NONE) {
                log::error!("cannot delete {}: {e}", path.display());
            }
            if let Some(f) = this() {
                f.refresh();
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
        let names = |hidden| -> Vec<String> {
            list_dir(&dir, hidden, Sort::Name).unwrap().into_iter().map(|f| f.name).collect()
        };
        assert_eq!(names(false), vec!["zeta", "alpha.txt"]);
        assert_eq!(names(true), vec!["zeta", ".hidden", "alpha.txt"]);
        assert_eq!(unique_name(&dir, "zeta"), "zeta 2");
        assert_eq!(unique_name(&dir, "new"), "new");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

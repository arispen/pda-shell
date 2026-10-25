//! Notes: Pocket PC style notes. A list of notes, newest first, and an
//! editor. Notes are plain text files in ~/Documents/Notes, so the Linux
//! programs (Mousepad, Thunar, Files) see them too.
//!
//! Like Pocket PC Notes there is no Save: the note is written while typing
//! (after a short pause), on Done and when the program closes. An empty new
//! note is not kept.
//!
//! Command bar menus: list: New, Tools; editor: Done, Edit, Tools.

use super::{action, short_time, AskBar, NativeWindow};
use crate::config::Config;
use crate::ui::MenuItem;
use gtk::gio;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

pub const APP_ID: &str = "io.github.pda_shell.Notes";
const SAVE_DELAY: Duration = Duration::from_millis(800);

/// A note in the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteInfo {
    pub path: PathBuf,
    pub title: String,
    pub modified: i64,
}

/// The title of a note: its first non-empty line, shortened.
pub fn note_title(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    if line.is_empty() {
        return "Untitled note".into();
    }
    let mut t: String = line.chars().take(40).collect();
    if line.chars().count() > 40 {
        t.push('…');
    }
    t
}

/// File name for a note created at `stamp` ("2026-10-25 14.05.09").
pub fn note_file_name(stamp: &str) -> String {
    format!("Note {stamp}.txt")
}

/// Notes in `dir`, newest first.
pub fn list_notes(dir: &Path) -> Vec<NoteInfo> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut notes: Vec<NoteInfo> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "txt"))
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            let modified = meta
                .modified()
                .ok()?
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs() as i64);
            let text = std::fs::read_to_string(e.path()).unwrap_or_default();
            Some(NoteInfo { path: e.path(), title: note_title(&text), modified })
        })
        .collect();
    notes.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.path.cmp(&b.path)));
    notes
}

pub fn notes_dir() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    home.join("Documents").join("Notes")
}

struct Notes {
    win: NativeWindow,
    cfg: Config,
    dir: PathBuf,
    stack: gtk::Stack,
    list: gtk::ListBox,
    empty: gtk::Label,
    view: gtk::TextView,
    ask: AskBar,
    menubar: gio::Menu,
    /// The note being edited and whether the buffer differs from the file.
    current: RefCell<Option<PathBuf>>,
    dirty: Cell<bool>,
    save_timer: RefCell<Option<glib::SourceId>>,
    edit_actions: Vec<gio::SimpleAction>,
}

thread_local! {
    static NOTES: RefCell<Option<Rc<Notes>>> = const { RefCell::new(None) };
}

/// Entry point of `pda-notes`. `--new` starts a new note.
pub fn main() -> glib::ExitCode {
    super::run(APP_ID, "pda-notes", |app, cfg, args| {
        let notes = NOTES.with(|n| n.borrow().clone()).unwrap_or_else(|| {
            let n = Notes::new(app, cfg);
            NOTES.with(|cell| *cell.borrow_mut() = Some(n.clone()));
            n.show_list();
            n
        });
        if args.iter().any(|a| a == "--new") {
            notes.new_note();
        }
        notes.win.window.present();
    })
}

fn menu(items: &[(&str, &str)]) -> gio::Menu {
    let m = gio::Menu::new();
    for (label, action) in items {
        m.append(Some(label), Some(action));
    }
    m
}

impl Notes {
    fn new(app: &gtk::Application, cfg: &Config) -> Rc<Self> {
        let dir = notes_dir();
        if let Err(e) = std::fs::create_dir_all(&dir) {
            log::error!("cannot create {}: {e}", dir.display());
        }
        let win = NativeWindow::new(app, "Notes", "pda-app-notes", cfg);

        let list = gtk::ListBox::new();
        list.add_css_class("compact");
        list.add_css_class("targets");
        list.add_css_class("lined");
        list.set_selection_mode(gtk::SelectionMode::None);
        list.set_activate_on_single_click(true);
        let empty = gtk::Label::new(Some("No notes yet. Tap New to write one."));
        empty.add_css_class("dim");
        empty.add_css_class("page-note");
        empty.set_xalign(0.0);
        let list_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        list_page.append(&list);
        list_page.append(&empty);
        let list_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&list_page)
            .build();

        let view = gtk::TextView::new();
        view.set_wrap_mode(gtk::WrapMode::WordChar);
        let pad = cfg.design.padding_px as i32;
        view.set_left_margin(pad * 2);
        view.set_right_margin(pad * 2);
        view.set_top_margin(pad);
        view.buffer().set_enable_undo(true);
        let editor = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&view)
            .build();

        let stack = gtk::Stack::new();
        stack.add_named(&list_scroll, Some("list"));
        stack.add_named(&editor, Some("editor"));
        let ask = AskBar::new();
        let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        page.append(&ask.widget);
        page.append(&stack);
        win.overlay.set_child(Some(&page));

        let menubar = gio::Menu::new();
        app.set_menubar(Some(&menubar));

        let notes = Rc::new_cyclic(|weak: &std::rc::Weak<Notes>| {
            let w = &win.window;
            let n = weak.clone();
            action(app, "new", move || {
                if let Some(n) = n.upgrade() {
                    n.new_note();
                }
            });
            let d = dir.clone();
            action(app, "show-folder", move || {
                let uri = gio::File::for_path(&d).uri();
                if let Err(e) = gio::AppInfo::launch_default_for_uri(&uri, gio::AppLaunchContext::NONE) {
                    log::error!("cannot open {uri}: {e}");
                }
            });
            let n = weak.clone();
            action(w, "done", move || {
                if let Some(n) = n.upgrade() {
                    n.show_list();
                }
            });
            let buffer = view.buffer();
            let clip = view.clipboard();
            let b = buffer.clone();
            let mut edit_actions = vec![
                action(w, "undo", move || b.undo()),
                {
                    let b = buffer.clone();
                    action(w, "redo", move || b.redo())
                },
                {
                    let (b, c) = (buffer.clone(), clip.clone());
                    action(w, "cut", move || b.cut_clipboard(&c, true))
                },
                {
                    let (b, c) = (buffer.clone(), clip.clone());
                    action(w, "copy", move || b.copy_clipboard(&c))
                },
                {
                    let (b, c) = (buffer.clone(), clip.clone());
                    action(w, "paste", move || b.paste_clipboard(&c, None, true))
                },
                {
                    let b = buffer.clone();
                    action(w, "select-all", move || {
                        let (s, e) = b.bounds();
                        b.select_range(&s, &e);
                    })
                },
            ];
            let n = weak.clone();
            edit_actions.push(action(w, "delete", move || {
                if let Some(n) = n.upgrade() {
                    if let Some(path) = n.current.borrow().clone() {
                        n.ask_delete(path);
                    }
                }
            }));
            let n = weak.clone();
            edit_actions.push(action(w, "open-external", move || {
                if let Some(n) = n.upgrade() {
                    n.save();
                    if let Some(p) = n.current.borrow().as_ref() {
                        let uri = gio::File::for_path(p).uri();
                        if let Err(e) = gio::AppInfo::launch_default_for_uri(&uri, gio::AppLaunchContext::NONE) {
                            log::error!("cannot open {uri}: {e}");
                        }
                    }
                }
            }));
            Notes {
                win,
                cfg: cfg.clone(),
                dir: dir.clone(),
                stack,
                list,
                empty,
                view,
                ask,
                menubar,
                current: RefCell::new(None),
                dirty: Cell::new(false),
                save_timer: RefCell::new(None),
                edit_actions,
            }
        });

        let n = Rc::downgrade(&notes);
        notes.list.connect_row_activated(move |_, row| {
            if let Some(n) = n.upgrade() {
                n.open(PathBuf::from(row.widget_name().as_str()));
            }
        });
        let n = Rc::downgrade(&notes);
        notes.view.buffer().connect_changed(move |_| {
            if let Some(n) = n.upgrade() {
                n.dirty.set(true);
                n.schedule_save();
            }
        });
        let n = Rc::downgrade(&notes);
        notes.win.window.connect_close_request(move |_| {
            if let Some(n) = n.upgrade() {
                n.finish_current();
            }
            glib::Propagation::Proceed
        });
        notes
    }

    fn set_mode(&self, editing: bool) {
        self.menubar.remove_all();
        if editing {
            self.menubar.append(Some("Done"), Some("win.done"));
            self.menubar.append_submenu(
                Some("Edit"),
                &menu(&[
                    ("Undo", "win.undo"),
                    ("Redo", "win.redo"),
                    ("Cut", "win.cut"),
                    ("Copy", "win.copy"),
                    ("Paste", "win.paste"),
                    ("Select all", "win.select-all"),
                    ("Delete note", "win.delete"),
                ]),
            );
            self.menubar.append_submenu(
                Some("Tools"),
                &menu(&[("Open in text editor", "win.open-external"), ("Notes folder", "app.show-folder")]),
            );
        } else {
            self.menubar.append(Some("New"), Some("app.new"));
            self.menubar.append_submenu(Some("Tools"), &menu(&[("Notes folder", "app.show-folder")]));
        }
        for a in &self.edit_actions {
            a.set_enabled(editing);
        }
    }

    fn refresh_list(&self) {
        self.list.remove_all();
        let notes = list_notes(&self.dir);
        self.empty.set_visible(notes.is_empty());
        for note in notes {
            let row = gtk::ListBoxRow::new();
            row.set_widget_name(&note.path.to_string_lossy());
            row.update_property(&[gtk::accessible::Property::Label(&note.title)]);
            let b = gtk::Box::new(gtk::Orientation::Horizontal, self.cfg.design.padding_px as i32);
            b.append(&crate::ui::named_icon("pda-doc-text", self.cfg.design.icon_px));
            let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
            let t = gtk::Label::new(Some(&note.title));
            t.set_xalign(0.0);
            t.set_ellipsize(gtk::pango::EllipsizeMode::End);
            let when = gtk::Label::new(Some(&short_time(note.modified)));
            when.add_css_class("dim");
            when.set_xalign(0.0);
            text.append(&t);
            text.append(&when);
            b.append(&text);
            row.set_child(Some(&b));
            let path = note.path.clone();
            let n = NOTES.with(|n| n.borrow().as_ref().map(Rc::downgrade));
            crate::ui::context_menu(&row, &self.win.feedback, &self.cfg, move || {
                let (p1, p2, n1, n2) = (path.clone(), path.clone(), n.clone(), n.clone());
                vec![
                    MenuItem::new("Open", move || {
                        if let Some(n) = n1.as_ref().and_then(|w| w.upgrade()) {
                            n.open(p1.clone());
                        }
                    }),
                    MenuItem::new("Delete", move || {
                        if let Some(n) = n2.as_ref().and_then(|w| w.upgrade()) {
                            n.ask_delete(p2.clone());
                        }
                    }),
                ]
            });
            self.list.append(&row);
        }
    }

    fn show_list(&self) {
        self.finish_current();
        self.refresh_list();
        self.stack.set_visible_child_name("list");
        self.win.window.set_title(Some("Notes"));
        self.set_mode(false);
    }

    fn open(&self, path: PathBuf) {
        self.finish_current();
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        self.edit(path, &text);
    }

    fn edit(&self, path: PathBuf, text: &str) {
        let buffer = self.view.buffer();
        buffer.begin_irreversible_action();
        buffer.set_text(text);
        buffer.end_irreversible_action();
        buffer.place_cursor(&buffer.end_iter());
        self.win.window.set_title(Some(&note_title(text)));
        *self.current.borrow_mut() = Some(path);
        self.dirty.set(false);
        self.stack.set_visible_child_name("editor");
        self.set_mode(true);
        self.view.grab_focus();
    }

    fn new_note(&self) {
        self.finish_current();
        let stamp = glib::DateTime::now_local()
            .ok()
            .and_then(|t| t.format("%Y-%m-%d %H.%M.%S").ok())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "new".into());
        let path = self.dir.join(note_file_name(&stamp));
        self.edit(path, "");
    }

    fn schedule_save(&self) {
        if let Some(t) = self.save_timer.take() {
            t.remove();
        }
        let id = glib::timeout_add_local_once(SAVE_DELAY, || {
            if let Some(n) = NOTES.with(|n| n.borrow().clone()) {
                n.save_timer.take();
                n.save();
            }
        });
        *self.save_timer.borrow_mut() = Some(id);
    }

    fn text(&self) -> String {
        let b = self.view.buffer();
        b.text(&b.start_iter(), &b.end_iter(), false).to_string()
    }

    /// Write the note if it changed. A note with no text is not written.
    fn save(&self) {
        if let Some(t) = self.save_timer.take() {
            t.remove();
        }
        let Some(path) = self.current.borrow().clone() else { return };
        if !self.dirty.get() {
            return;
        }
        let text = self.text();
        if text.trim().is_empty() && !path.exists() {
            return;
        }
        match std::fs::write(&path, &text) {
            Ok(()) => {
                self.dirty.set(false);
                self.win.window.set_title(Some(&note_title(&text)));
                log::info!("saved {}", path.display());
            }
            Err(e) => log::error!("cannot save {}: {e}", path.display()),
        }
    }

    /// Save the note being edited and leave it; an emptied note is removed.
    fn finish_current(&self) {
        self.save();
        if let Some(path) = self.current.borrow_mut().take() {
            if self.text().trim().is_empty() && path.exists() {
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    fn ask_delete(&self, path: PathBuf) {
        let title = std::fs::read_to_string(&path).map(|t| note_title(&t)).unwrap_or_else(|_| note_title(&self.text()));
        self.ask.confirm(&format!("Delete the note “{title}”?"), "Delete", move || {
            let Some(n) = NOTES.with(|n| n.borrow().clone()) else { return };
            if n.current.borrow().as_ref() == Some(&path) {
                n.current.borrow_mut().take();
                n.dirty.set(false);
            }
            if path.exists() {
                if let Err(e) = gio::File::for_path(&path).trash(gio::Cancellable::NONE) {
                    log::error!("cannot delete {}: {e}", path.display());
                }
            }
            n.show_list();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_come_from_the_first_line() {
        assert_eq!(note_title("\n  Shopping list \n- milk"), "Shopping list");
        assert_eq!(note_title("   \n"), "Untitled note");
        let long = "a".repeat(50);
        assert_eq!(note_title(&long).chars().count(), 41);
    }

    #[test]
    fn notes_are_listed_newest_first() {
        let dir = std::env::temp_dir().join(format!("pda-notes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(note_file_name("a")), "Older").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(dir.join(note_file_name("b")), "Newer\nbody").unwrap();
        std::fs::write(dir.join("ignored.md"), "x").unwrap();
        let titles: Vec<String> = list_notes(&dir).into_iter().map(|n| n.title).collect();
        assert_eq!(titles, vec!["Newer", "Older"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

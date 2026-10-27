//! Notes: Pocket PC style notes. A list of notes, newest first, and an
//! editor. Notes are plain text files in ~/Documents/Notes, so the Linux
//! programs (Mousepad, Thunar, Files) see them too.
//!
//! Like Pocket PC Notes there is no Save: the note is written while typing
//! (after a short pause), on Done and when the program closes. An empty new
//! note is not kept. Writes are atomic (temporary file, then rename). If
//! another program changed the file since it was opened, the edits are kept
//! as a new note instead of overwriting the other program's version; a
//! failed write is shown, and the text stays in the editor.
//!
//! The list has a search field (titles and text), sorting by date or title,
//! and pinned notes on top. Pins are file names in `.pinned` in the notes
//! folder, so the notes themselves stay plain .txt files.
//!
//! Command bar menus: list: New, View; editor: Done, Edit, Tools.
//! `pda-notes [--new] [NOTE.txt]`.

use super::{action, menu, short_time, AskBar, NativeWindow};
use crate::config::Config;
use crate::ui::MenuItem;
use gtk::gio;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
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
    /// The whole text, for searching.
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    /// Newest first.
    Date,
    Title,
}

/// Whether `note` matches every word of `query` (title or text, any case).
pub fn matches(note: &NoteInfo, query: &str) -> bool {
    let hay = note.text.to_lowercase();
    query.split_whitespace().all(|w| hay.contains(&w.to_lowercase()))
}

/// The first line of the text (after the title line) containing the first
/// word of `query`: what the list shows under a found note.
pub fn snippet(text: &str, query: &str) -> Option<String> {
    let w = query.split_whitespace().next()?.to_lowercase();
    let line = text.lines().map(str::trim).find(|l| l.to_lowercase().contains(&w))?;
    let mut s: String = line.chars().take(60).collect();
    if line.chars().count() > 60 {
        s.push('…');
    }
    Some(s)
}

/// Pinned notes first, then by `sort`.
pub fn sort_notes(notes: &mut [NoteInfo], pinned: &BTreeSet<String>, sort: Sort) {
    let pin = |n: &NoteInfo| !pinned.contains(&file_name(&n.path));
    notes.sort_by(|a, b| {
        pin(a).cmp(&pin(b)).then_with(|| match sort {
            Sort::Date => b.modified.cmp(&a.modified),
            Sort::Title => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
        })
        .then_with(|| a.path.cmp(&b.path))
    });
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

const PINS: &str = ".pinned";

/// File names of the pinned notes in `dir` (that still exist).
pub fn read_pins(dir: &Path) -> BTreeSet<String> {
    std::fs::read_to_string(dir.join(PINS))
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && dir.join(l).is_file())
        .map(String::from)
        .collect()
}

pub fn write_pins(dir: &Path, pins: &BTreeSet<String>) -> std::io::Result<()> {
    let text: String = pins.iter().map(|p| format!("{p}\n")).collect();
    crate::pim::write_atomic(&dir.join(PINS), &text)
}

/// Modification time of a file in seconds (0 if it does not exist).
fn mtime(p: &Path) -> i64 {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos() as i64)
}

/// Save `text` as the note `path`. `seen` is the file's modification time
/// when it was opened or last saved by Notes (0 for a new note). If the file
/// changed since (another program), the text goes to a new note next to it.
/// Returns where the text went and its new modification time.
pub fn save_note(path: &Path, text: &str, seen: i64) -> std::io::Result<(PathBuf, i64)> {
    let now = mtime(path);
    let target = if now != 0 && now != seen {
        let dir = path.parent().unwrap_or(Path::new("."));
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        dir.join(super::fileops::free_name(dir, &format!("{stem} (edited here).txt")))
    } else {
        path.to_path_buf()
    };
    crate::pim::write_atomic(&target, text)?;
    Ok((target.clone(), mtime(&target)))
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
            Some(NoteInfo { path: e.path(), title: note_title(&text), modified, text })
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
    search: gtk::SearchEntry,
    sort: Cell<Sort>,
    pins: RefCell<BTreeSet<String>>,
    /// The note file's modification time when Notes last read or wrote it.
    seen: Cell<i64>,
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
        let file = args
            .iter()
            .find(|a| !a.starts_with("--"))
            .map(|a| if a.contains("://") { gio::File::for_uri(a) } else { gio::File::for_commandline_arg(a) })
            .and_then(|f| f.path())
            .filter(|p| p.is_file());
        if args.iter().any(|a| a == "--new") {
            notes.new_note();
        } else if let Some(path) = file {
            notes.open(path);
        }
        notes.win.window.present();
    })
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
        let search = gtk::SearchEntry::new();
        search.set_placeholder_text(Some("Search notes"));
        search.update_property(&[gtk::accessible::Property::Label("Search notes")]);
        search.add_css_class("notes-search");
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
        let list_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let search_bar = gtk::Box::new(gtk::Orientation::Vertical, 0);
        search_bar.add_css_class("ask-bar");
        search_bar.append(&search);
        list_box.append(&search_bar);
        list_box.append(&list_scroll);
        stack.add_named(&list_box, Some("list"));
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
            let sort = gio::SimpleAction::new_stateful("sort", Some(glib::VariantTy::STRING), &"date".to_variant());
            let n = weak.clone();
            sort.connect_change_state(move |a, v| {
                let Some(value) = v.and_then(|v| v.get::<String>()) else { return };
                a.set_state(&value.to_variant());
                if let Some(n) = n.upgrade() {
                    n.sort.set(if value == "title" { Sort::Title } else { Sort::Date });
                    n.refresh_list();
                }
            });
            w.add_action(&sort);
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
                search,
                sort: Cell::new(Sort::Date),
                pins: RefCell::new(read_pins(&dir)),
                seen: Cell::new(0),
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
        notes.search.connect_search_changed(move |_| {
            if let Some(n) = n.upgrade() {
                n.refresh_list();
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
            // A note that could not be saved keeps the window open.
            match n.upgrade() {
                Some(n) if !n.finish_current() => glib::Propagation::Stop,
                _ => glib::Propagation::Proceed,
            }
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
            let view = gio::Menu::new();
            let sort = gio::Menu::new();
            for (label, value) in [("Sort by date", "date"), ("Sort by title", "title")] {
                let item = gio::MenuItem::new(Some(label), None);
                item.set_action_and_target_value(Some("win.sort"), Some(&value.to_variant()));
                sort.append_item(&item);
            }
            view.append_section(None, &sort);
            view.append_section(None, &menu(&[("Notes folder", "app.show-folder")]));
            self.menubar.append_submenu(Some("View"), &view);
        }
        for a in &self.edit_actions {
            a.set_enabled(editing);
        }
    }

    fn refresh_list(&self) {
        self.list.remove_all();
        let mut notes = list_notes(&self.dir);
        let total = notes.len();
        *self.pins.borrow_mut() = read_pins(&self.dir);
        let query = self.search.text().trim().to_string();
        notes.retain(|n| query.is_empty() || matches(n, &query));
        sort_notes(&mut notes, &self.pins.borrow(), self.sort.get());
        self.empty.set_text(&if total == 0 {
            "No notes yet. Tap New to write one.".to_string()
        } else if notes.is_empty() {
            format!("No note contains “{query}”.")
        } else if !query.is_empty() {
            format!("{} of {total} notes", notes.len())
        } else {
            String::new()
        });
        self.empty.set_visible(!self.empty.text().is_empty());
        for note in notes {
            let pinned = self.pins.borrow().contains(&file_name(&note.path));
            let row = gtk::ListBoxRow::new();
            row.set_widget_name(&note.path.to_string_lossy());
            row.update_property(&[gtk::accessible::Property::Label(&note.title)]);
            let b = gtk::Box::new(gtk::Orientation::Horizontal, self.cfg.design.padding_px as i32);
            b.append(&crate::ui::named_icon("pda-doc-text", self.cfg.design.icon_px));
            let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
            let t = gtk::Label::new(Some(&note.title));
            t.set_xalign(0.0);
            t.set_ellipsize(gtk::pango::EllipsizeMode::End);
            let mut sub = short_time(note.modified);
            if pinned {
                sub = format!("Pinned · {sub}");
            }
            if let Some(found) = snippet(&note.text, &query).filter(|_| !query.is_empty()) {
                sub = format!("{sub} · {found}");
            }
            let when = gtk::Label::new(Some(&sub));
            when.set_ellipsize(gtk::pango::EllipsizeMode::End);
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
                let (p3, n3) = (path.clone(), n.clone());
                vec![
                    MenuItem::new("Open", move || {
                        if let Some(n) = n1.as_ref().and_then(|w| w.upgrade()) {
                            n.open(p1.clone());
                        }
                    }),
                    MenuItem::new(if pinned { "Unpin" } else { "Pin to top" }, move || {
                        if let Some(n) = n3.as_ref().and_then(|w| w.upgrade()) {
                            n.toggle_pin(&p3);
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

    fn toggle_pin(&self, path: &Path) {
        let name = file_name(path);
        let mut pins = read_pins(&self.dir);
        if !pins.remove(&name) {
            pins.insert(name);
        }
        if let Err(e) = write_pins(&self.dir, &pins) {
            self.ask.tell(&format!("Cannot pin the note: {e}"));
        }
        self.refresh_list();
    }

    fn show_list(&self) {
        if !self.finish_current() {
            return;
        }
        self.refresh_list();
        self.stack.set_visible_child_name("list");
        self.win.window.set_title(Some("Notes"));
        self.set_mode(false);
    }

    fn open(&self, path: PathBuf) {
        if !self.finish_current() {
            return;
        }
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                self.ask.tell(&format!("Cannot open “{}”: {e}", file_name(&path)));
                return;
            }
        };
        self.seen.set(mtime(&path));
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
        if !self.finish_current() {
            return;
        }
        let stamp = glib::DateTime::now_local()
            .ok()
            .and_then(|t| t.format("%Y-%m-%d %H.%M.%S").ok())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "new".into());
        let name = super::fileops::free_name(&self.dir, &note_file_name(&stamp));
        self.seen.set(0);
        self.edit(self.dir.join(name), "");
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
        match save_note(&path, &text, self.seen.get()) {
            Ok((saved, seen)) => {
                self.dirty.set(false);
                self.seen.set(seen);
                self.win.window.set_title(Some(&note_title(&text)));
                log::info!("saved {}", saved.display());
                if saved != path {
                    // From now on this note is the copy.
                    *self.current.borrow_mut() = Some(saved.clone());
                    self.ask.tell(&format!(
                        "Another program changed this note. Your text was saved as “{}”.",
                        file_name(&saved)
                    ));
                }
            }
            Err(e) => {
                log::error!("cannot save {}: {e}", path.display());
                self.ask.tell(&format!("Cannot save the note: {e}. The text is still here; try again with Done."));
            }
        }
    }

    /// Save the note being edited and leave it; an emptied note is removed.
    /// False (and the note stays open) if it could not be saved.
    fn finish_current(&self) -> bool {
        self.save();
        if self.dirty.get() && self.current.borrow().is_some() {
            return false;
        }
        if let Some(path) = self.current.borrow_mut().take() {
            if self.text().trim().is_empty() && path.exists() {
                let _ = std::fs::remove_file(&path);
            }
        }
        true
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

    fn note(name: &str, text: &str, modified: i64) -> NoteInfo {
        NoteInfo { path: PathBuf::from(format!("/n/{name}")), title: note_title(text), modified, text: text.into() }
    }

    #[test]
    fn search_matches_titles_and_text_in_any_case() {
        let n = note("a.txt", "Shopping\nMilk and Żółw eggs", 1);
        assert!(matches(&n, "milk"));
        assert!(matches(&n, "EGGS shopping"));
        assert!(matches(&n, "żółw"));
        assert!(!matches(&n, "milk bread"));
        assert_eq!(snippet(&n.text, "eggs").as_deref(), Some("Milk and Żółw eggs"));
    }

    #[test]
    fn pinned_notes_come_first_in_either_order() {
        let mut v = vec![note("old.txt", "Zebra", 1), note("new.txt", "Apple", 3), note("pin.txt", "Middle", 2)];
        let pins: BTreeSet<String> = ["pin.txt".to_string()].into();
        sort_notes(&mut v, &pins, Sort::Date);
        assert_eq!(v.iter().map(|n| n.title.as_str()).collect::<Vec<_>>(), ["Middle", "Apple", "Zebra"]);
        sort_notes(&mut v, &pins, Sort::Title);
        assert_eq!(v.iter().map(|n| n.title.as_str()).collect::<Vec<_>>(), ["Middle", "Apple", "Zebra"]);
        sort_notes(&mut v, &BTreeSet::new(), Sort::Title);
        assert_eq!(v[0].title, "Apple");
    }

    #[test]
    fn saving_is_atomic_and_never_overwrites_a_foreign_change() {
        let dir = std::env::temp_dir().join(format!("pda-notes-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(note_file_name("x"));
        // A new note.
        let (saved, seen) = save_note(&path, "one", 0).unwrap();
        assert_eq!((saved.as_path(), std::fs::read_to_string(&path).unwrap().as_str()), (path.as_path(), "one"));
        // Saved again by Notes: same file.
        let (saved, seen) = save_note(&path, "two", seen).unwrap();
        assert_eq!(saved, path);
        // Changed by another program meanwhile: Notes' text goes elsewhere.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&path, "theirs").unwrap();
        let (saved, _) = save_note(&path, "mine", seen).unwrap();
        assert_ne!(saved, path);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "theirs");
        assert_eq!(std::fs::read_to_string(&saved).unwrap(), "mine");
        // Pins survive, and pins of deleted notes are dropped.
        let pins: BTreeSet<String> = [file_name(&path), "gone.txt".to_string()].into();
        write_pins(&dir, &pins).unwrap();
        assert_eq!(read_pins(&dir), [file_name(&path)].into());
        // The pin file is not a note.
        assert!(list_notes(&dir).iter().all(|n| !n.path.ends_with(PINS)));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

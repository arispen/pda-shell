//! Installed applications and launching.
//!
//! [`AppEntry`] and the search logic are plain data so they can be tested
//! without GIO; [`load_installed`] fills them from the desktop entries GIO
//! finds on `XDG_DATA_DIRS`, and [`launch`] starts one through GIO.

use gtk::gio;
use gtk::prelude::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppEntry {
    /// Desktop entry ID, e.g. "org.xfce.mousepad.desktop".
    pub id: String,
    pub name: String,
    pub generic_name: String,
    pub keywords: Vec<String>,
    pub categories: Vec<String>,
    pub icon: Option<String>,
    pub startup_wm_class: Option<String>,
}

impl AppEntry {
    /// Relevance of this entry for `query`; `None` if it does not match.
    /// Higher is better. Matching is case-insensitive and every word of the
    /// query must match somewhere.
    pub fn score(&self, query: &str) -> Option<u32> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        if words.is_empty() {
            return Some(1);
        }
        let name = self.name.to_lowercase();
        let generic = self.generic_name.to_lowercase();
        let exec_id = self.id.trim_end_matches(".desktop").to_lowercase();
        let mut total = 0;
        for w in &words {
            let s = if name.starts_with(w.as_str()) {
                100
            } else if name.split(|c: char| !c.is_alphanumeric()).any(|p| p.starts_with(w.as_str())) {
                80
            } else if name.contains(w.as_str()) {
                60
            } else if generic.contains(w.as_str()) {
                40
            } else if self.keywords.iter().any(|k| k.to_lowercase().starts_with(w.as_str())) {
                30
            } else if exec_id.contains(w.as_str()) {
                20
            } else if self.categories.iter().any(|c| c.to_lowercase() == *w) {
                10
            } else {
                return None;
            };
            total += s;
        }
        Some(total)
    }
}

/// Entries matching `query`, best first; ties sorted by name.
pub fn search<'a>(apps: &'a [AppEntry], query: &str) -> Vec<&'a AppEntry> {
    let mut hits: Vec<(u32, &AppEntry)> =
        apps.iter().filter_map(|a| a.score(query).map(|s| (s, a))).collect();
    hits.sort_by(|(sa, a), (sb, b)| sb.cmp(sa).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    hits.into_iter().map(|(_, a)| a).collect()
}

/// Resolve configured shortcut IDs to installed entries, keeping order and
/// skipping IDs that are not installed.
pub fn resolve_shortcuts<'a>(apps: &'a [AppEntry], ids: &[String]) -> Vec<&'a AppEntry> {
    ids.iter().filter_map(|id| apps.iter().find(|a| &a.id == id)).collect()
}

/// The entry that most likely created a window of WM_CLASS `class`.
pub fn for_window_class<'a>(apps: &'a [AppEntry], class: &str) -> Option<&'a AppEntry> {
    let class = class.to_lowercase();
    if class.is_empty() {
        return None;
    }
    let stem = |a: &AppEntry| a.id.trim_end_matches(".desktop").to_lowercase();
    apps.iter()
        .find(|a| a.startup_wm_class.as_deref().is_some_and(|c| c.to_lowercase() == class))
        .or_else(|| apps.iter().find(|a| stem(a) == class))
        .or_else(|| apps.iter().find(|a| stem(a).rsplit('.').next() == Some(class.as_str())))
        .or_else(|| apps.iter().find(|a| a.name.to_lowercase() == class))
}

/// Applications that should appear in menus, sorted by name.
pub fn load_installed() -> Vec<AppEntry> {
    let mut apps: Vec<AppEntry> = gio::AppInfo::all()
        .into_iter()
        .filter(|info| info.should_show())
        .filter_map(|info| {
            let id = info.id()?.to_string();
            let desktop = info.downcast_ref::<gio::DesktopAppInfo>();
            let split = |s: Option<glib::GString>| -> Vec<String> {
                s.map(|s| s.split(';').filter(|p| !p.is_empty()).map(String::from).collect())
                    .unwrap_or_default()
            };
            Some(AppEntry {
                id,
                name: info.display_name().to_string(),
                generic_name: desktop
                    .and_then(|d| d.generic_name())
                    .map(|s| s.to_string())
                    .unwrap_or_default(),
                keywords: desktop
                    .map(|d| d.keywords().iter().map(|k| k.to_string()).collect())
                    .unwrap_or_default(),
                categories: split(desktop.and_then(|d| d.categories())),
                icon: info.icon().and_then(|i| IconExt::to_string(&i)).map(|s| s.to_string()),
                startup_wm_class: desktop.and_then(|d| d.startup_wm_class()).map(|s| s.to_string()),
            })
        })
        .collect();
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps.dedup_by(|a, b| a.id == b.id);
    apps
}

pub fn gicon(entry: &AppEntry) -> Option<gio::Icon> {
    entry.icon.as_deref().and_then(|s| gio::Icon::for_string(s).ok())
}

fn launch_context() -> Option<gio::AppLaunchContext> {
    gtk::gdk::Display::default().map(|d| d.app_launch_context().upcast())
}

/// Launch an application by desktop entry ID.
pub fn launch(id: &str) -> Result<(), String> {
    let info = gio::DesktopAppInfo::new(id).ok_or_else(|| format!("{id} is not installed"))?;
    info.launch(&[], launch_context().as_ref()).map_err(|e| e.to_string())?;
    log::info!("launched {id}");
    Ok(())
}

/// Open a document with its default application.
pub fn open_uri(uri: &str) -> Result<(), String> {
    gio::AppInfo::launch_default_for_uri(uri, launch_context().as_ref()).map_err(|e| e.to_string())?;
    log::info!("opened {uri}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str, name: &str, generic: &str, keywords: &[&str], cats: &[&str]) -> AppEntry {
        AppEntry {
            id: id.into(),
            name: name.into(),
            generic_name: generic.into(),
            keywords: keywords.iter().map(|s| s.to_string()).collect(),
            categories: cats.iter().map(|s| s.to_string()).collect(),
            icon: None,
            startup_wm_class: None,
        }
    }

    fn catalog() -> Vec<AppEntry> {
        vec![
            app("org.gnumeric.gnumeric.desktop", "Gnumeric", "Spreadsheet", &["xls", "excel"], &["Office"]),
            app("org.xfce.mousepad.desktop", "Mousepad", "Text Editor", &["text", "editor"], &["Utility"]),
            app("thunar.desktop", "Thunar File Manager", "File Manager", &["folder"], &["System"]),
            app("xfce4-terminal.desktop", "Xfce Terminal", "Terminal Emulator", &["shell"], &["System"]),
            app("atril.desktop", "Document Viewer", "", &["pdf"], &["Office"]),
        ]
    }

    fn ids(v: Vec<&AppEntry>) -> Vec<&str> {
        v.into_iter().map(|a| a.id.as_str()).collect()
    }

    #[test]
    fn empty_query_lists_everything_by_name() {
        let c = catalog();
        let r = ids(search(&c, "  "));
        assert_eq!(r.len(), 5);
        assert_eq!(r[0], "atril.desktop"); // "Document Viewer"
    }

    #[test]
    fn name_prefix_beats_generic_name() {
        let c = catalog();
        assert_eq!(ids(search(&c, "term"))[0], "xfce4-terminal.desktop");
        assert_eq!(ids(search(&c, "mou")), vec!["org.xfce.mousepad.desktop"]);
    }

    #[test]
    fn search_is_case_insensitive_and_uses_metadata() {
        let c = catalog();
        assert_eq!(ids(search(&c, "SPREAD")), vec!["org.gnumeric.gnumeric.desktop"]);
        assert_eq!(ids(search(&c, "pdf")), vec!["atril.desktop"]);
        assert_eq!(ids(search(&c, "atril")), vec!["atril.desktop"]);
        assert_eq!(ids(search(&c, "office")).len(), 2);
    }

    #[test]
    fn all_words_must_match() {
        let c = catalog();
        assert_eq!(ids(search(&c, "file man")), vec!["thunar.desktop"]);
        assert!(search(&c, "file spreadsheet").is_empty());
    }

    #[test]
    fn no_match_is_empty() {
        assert!(search(&catalog(), "zzz").is_empty());
    }

    #[test]
    fn window_class_lookup() {
        let mut c = catalog();
        c[2].startup_wm_class = Some("Thunar".into());
        let id = |class: &str| for_window_class(&c, class).map(|a| a.id.as_str());
        assert_eq!(id("Thunar"), Some("thunar.desktop"));
        assert_eq!(id("Mousepad"), Some("org.xfce.mousepad.desktop"));
        assert_eq!(id("Gnumeric"), Some("org.gnumeric.gnumeric.desktop"));
        assert_eq!(id("Xfce4-terminal"), Some("xfce4-terminal.desktop"));
        assert_eq!(id("Unknown"), None);
        assert_eq!(id(""), None);
    }

    #[test]
    fn shortcuts_keep_order_and_skip_missing() {
        let c = catalog();
        let wanted = vec![
            "xfce4-terminal.desktop".to_string(),
            "missing.desktop".to_string(),
            "atril.desktop".to_string(),
        ];
        assert_eq!(ids(resolve_shortcuts(&c, &wanted)), vec!["xfce4-terminal.desktop", "atril.desktop"]);
    }
}

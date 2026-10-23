//! Shell configuration: design tokens, input thresholds and Today contents.
//!
//! Loaded from TOML. Every field has a default so a partial file (or no file)
//! still gives a working shell.
//!
//! Density profiles: `[density.<name>]` tables hold any subset of the
//! `[design]` keys. The profile named by `design.density` (or the
//! `PDA_SHELL_DENSITY` environment variable) is laid over `[design]` before
//! the file is deserialized, so every size stays independently tunable.

use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    pub design: Design,
    pub colors: Colors,
    pub input: InputConfig,
    pub today: TodayConfig,
    pub keyboard: KeyboardConfig,
    pub start_menu: StartMenuConfig,
    pub windows: WindowsConfig,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct Design {
    /// Name of the density profile that was applied (informational).
    pub density: String,
    pub font_family: String,
    pub font_px: u32,
    pub small_font_px: u32,
    pub title_font_px: u32,
    /// The date on Today, the most prominent text of the shell.
    pub date_font_px: u32,
    pub top_bar_px: u32,
    pub bottom_bar_px: u32,
    pub icon_px: u32,
    pub today_icon_px: u32,
    /// Minimum height of ordinary list rows (Start menu, recent documents).
    pub row_px: u32,
    /// Minimum size of high-priority targets: bar buttons, switcher rows,
    /// context-menu entries.
    pub target_px: u32,
    pub spacing_px: u32,
    pub padding_px: u32,
    pub radius_px: u32,
    pub start_menu_width_px: u32,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct Colors {
    pub bar_bg: String,
    pub bar_fg: String,
    pub bar_hover: String,
    pub bar_pressed: String,
    pub surface: String,
    pub surface_alt: String,
    pub text: String,
    pub text_dim: String,
    pub accent: String,
    pub accent_fg: String,
    pub border: String,
    pub focus: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct InputConfig {
    pub long_press_ms: u32,
    pub long_press_slop_px: f64,
    pub double_tap_ms: u32,
    pub double_tap_slop_px: f64,
    pub feedback_delay_ms: u32,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct TodayConfig {
    pub shortcuts: Vec<String>,
    pub recent_documents: usize,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct KeyboardConfig {
    pub command: Vec<String>,
    /// Onboard layout: a name from /usr/share/onboard/layouts (e.g. "Small")
    /// or a path to a .onboard file. Empty keeps Onboard's own choice.
    pub layout: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct StartMenuConfig {
    /// Desktop entry IDs kept out of the program list (still found by
    /// search and under "Other programs"). `*` matches any text.
    pub hidden: Vec<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FullscreenMode {
    /// Allow full screen, with a small "Exit full screen" button on top.
    Escape,
    /// Take every window straight back out of full screen.
    Prevent,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct WindowsConfig {
    pub fullscreen: FullscreenMode,
    /// Move and shrink dialogs that do not fit the work area.
    pub fit_dialogs: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            design: Design::default(),
            colors: Colors::default(),
            input: InputConfig::default(),
            today: TodayConfig::default(),
            keyboard: KeyboardConfig::default(),
            start_menu: StartMenuConfig::default(),
            windows: WindowsConfig::default(),
        }
    }
}

impl Default for Design {
    fn default() -> Self {
        Self {
            density: "compact".into(),
            font_family: "DejaVu Sans".into(),
            font_px: 12,
            small_font_px: 10,
            title_font_px: 12,
            date_font_px: 16,
            top_bar_px: 24,
            bottom_bar_px: 26,
            icon_px: 16,
            today_icon_px: 24,
            row_px: 20,
            target_px: 24,
            spacing_px: 2,
            padding_px: 4,
            radius_px: 2,
            start_menu_width_px: 300,
        }
    }
}

impl Default for Colors {
    fn default() -> Self {
        Self {
            bar_bg: "#1d3f72".into(),
            bar_fg: "#ffffff".into(),
            bar_hover: "#2d5aa0".into(),
            bar_pressed: "#0f2547".into(),
            surface: "#ffffff".into(),
            surface_alt: "#eef2f8".into(),
            text: "#101010".into(),
            text_dim: "#5a6270".into(),
            accent: "#1f5fbf".into(),
            accent_fg: "#ffffff".into(),
            border: "#b8c2d0".into(),
            focus: "#ff9a00".into(),
        }
    }
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            long_press_ms: 600,
            long_press_slop_px: 8.0,
            double_tap_ms: 400,
            double_tap_slop_px: 10.0,
            feedback_delay_ms: 150,
        }
    }
}

impl Default for TodayConfig {
    fn default() -> Self {
        Self {
            shortcuts: vec![
                "thunar.desktop".into(),
                "org.xfce.mousepad.desktop".into(),
                "org.gnumeric.gnumeric.desktop".into(),
                "atril.desktop".into(),
                "firefox-esr.desktop".into(),
                "xfce4-terminal.desktop".into(),
            ],
            recent_documents: 6,
        }
    }
}

impl Default for KeyboardConfig {
    fn default() -> Self {
        Self { command: vec!["pda-onboard".into()], layout: "Small".into() }
    }
}

impl Default for StartMenuConfig {
    fn default() -> Self {
        Self {
            hidden: [
                "openbox.desktop",
                "x11vnc.desktop",
                "display-im*.desktop",
                "python3*.desktop",
                "thunar-bulk-rename.desktop",
                "onboard-settings.desktop",
                "onboard.desktop",
            ]
            .map(String::from)
            .to_vec(),
        }
    }
}

impl Default for WindowsConfig {
    fn default() -> Self {
        Self { fullscreen: FullscreenMode::Escape, fit_dialogs: true }
    }
}

impl Config {
    /// Parse a config file, applying the density profile it selects or the
    /// one named in `PDA_SHELL_DENSITY`.
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        let density = std::env::var("PDA_SHELL_DENSITY").ok().filter(|s| !s.is_empty());
        Self::parse_with_density(text, density.as_deref())
    }

    /// Parse a config file with an explicit density profile (`None`: the
    /// one the file selects in `design.density`, if any).
    pub fn parse_with_density(text: &str, density: Option<&str>) -> Result<Self, toml::de::Error> {
        let mut table: toml::Table = text.parse()?;
        let profiles = match table.remove("density") {
            Some(toml::Value::Table(t)) => t,
            _ => toml::Table::new(),
        };
        let design = table
            .entry("design")
            .or_insert_with(|| toml::Value::Table(toml::Table::new()));
        if let toml::Value::Table(design) = design {
            let name = density
                .map(str::to_string)
                .or_else(|| design.get("density").and_then(|v| v.as_str()).map(str::to_string));
            if let Some(name) = name {
                match profiles.get(&name) {
                    Some(toml::Value::Table(profile)) => {
                        for (k, v) in profile {
                            design.insert(k.clone(), v.clone());
                        }
                        design.insert("density".into(), toml::Value::String(name));
                    }
                    _ if name == Design::default().density => {}
                    _ => log::warn!("unknown density profile {name:?}; using [design] as is"),
                }
            }
        }
        table.try_into()
    }

    /// Names of the density profiles a config file defines.
    pub fn density_profiles(text: &str) -> Vec<String> {
        text.parse::<toml::Table>()
            .ok()
            .and_then(|t| t.get("density").and_then(|d| d.as_table()).map(|d| d.keys().cloned().collect()))
            .unwrap_or_default()
    }

    /// Candidate config locations, most specific first.
    pub fn search_paths() -> Vec<PathBuf> {
        let mut paths = Vec::new();
        if let Some(p) = std::env::var_os("PDA_SHELL_CONFIG") {
            paths.push(PathBuf::from(p));
        }
        let config_home = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".config")));
        if let Some(dir) = config_home {
            paths.push(dir.join("pda-shell/pda-shell.toml"));
        }
        paths.push(PathBuf::from("/etc/pda-shell/pda-shell.toml"));
        paths
    }

    /// Load the first readable config file, falling back to defaults.
    /// A file that exists but does not parse is reported and ignored.
    pub fn load() -> Self {
        for path in Self::search_paths() {
            match std::fs::read_to_string(&path) {
                Ok(text) => match Self::parse(&text) {
                    Ok(cfg) => {
                        log::info!("config loaded from {}", path.display());
                        return cfg;
                    }
                    Err(e) => log::error!("ignoring invalid config {}: {e}", path.display()),
                },
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => log::warn!("cannot read {}: {e}", path.display()),
            }
        }
        log::info!("no config file found, using defaults");
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_gives_defaults() {
        assert_eq!(Config::parse_with_density("", None).unwrap(), Config::default());
    }

    #[test]
    fn partial_section_keeps_other_defaults() {
        let cfg = Config::parse_with_density("[input]\nlong_press_ms = 750\n", None).unwrap();
        assert_eq!(cfg.input.long_press_ms, 750);
        assert_eq!(cfg.input.long_press_slop_px, InputConfig::default().long_press_slop_px);
        assert_eq!(cfg.design, Design::default());
    }

    #[test]
    fn shipped_config_parses() {
        let text = include_str!("../config/pda-shell.toml");
        let cfg = Config::parse_with_density(text, None).expect("config/pda-shell.toml must parse");
        assert_eq!(cfg.input.long_press_ms, 600);
        assert!(!cfg.today.shortcuts.is_empty());
    }

    #[test]
    fn unknown_type_is_an_error() {
        assert!(Config::parse_with_density("[design]\nfont_px = \"big\"\n", None).is_err());
    }

    const PROFILES: &str = "[design]\nfont_px = 12\nrow_px = 20\n\n\
        [density.readable]\nfont_px = 16\ntarget_px = 40\n";

    #[test]
    fn density_profile_overrides_only_its_keys() {
        let cfg = Config::parse_with_density(PROFILES, Some("readable")).unwrap();
        assert_eq!(cfg.design.density, "readable");
        assert_eq!(cfg.design.font_px, 16);
        assert_eq!(cfg.design.target_px, 40);
        assert_eq!(cfg.design.row_px, 20, "keys the profile does not set come from [design]");
        assert_eq!(cfg.design.icon_px, Design::default().icon_px);
    }

    #[test]
    fn density_selected_in_the_file() {
        let text = format!("{}\n", PROFILES.replace("[design]\n", "[design]\ndensity = \"readable\"\n"));
        assert_eq!(Config::parse_with_density(&text, None).unwrap().design.font_px, 16);
        // An explicit choice (command line / environment) wins.
        let cfg = Config::parse_with_density(&text, Some("compact")).unwrap();
        assert_eq!(cfg.design.font_px, 12);
    }

    #[test]
    fn unknown_density_keeps_design() {
        let cfg = Config::parse_with_density(PROFILES, Some("huge")).unwrap();
        assert_eq!(cfg.design.font_px, 12);
    }

    #[test]
    fn shipped_profiles_are_complete_and_ordered() {
        let text = include_str!("../config/pda-shell.toml");
        let names = Config::density_profiles(text);
        for name in ["compact", "balanced", "readable"] {
            assert!(names.iter().any(|n| n == name), "missing profile {name}");
        }
        let get = |n: &str| Config::parse_with_density(text, Some(n)).unwrap().design;
        let (c, b, r) = (get("compact"), get("balanced"), get("readable"));
        assert!(c.font_px < b.font_px && b.font_px < r.font_px);
        assert!(c.target_px < b.target_px && b.target_px < r.target_px);
        assert!((16..=18).contains(&r.font_px), "readable candidate text is 16-18 px");
        assert!((36..=40).contains(&r.target_px), "readable candidate targets are 36-40 px");
    }

    #[test]
    fn windows_and_start_menu_sections() {
        let cfg = Config::parse_with_density(
            "[windows]\nfullscreen = \"prevent\"\n[start_menu]\nhidden = [\"a.desktop\"]\n",
            None,
        )
        .unwrap();
        assert_eq!(cfg.windows.fullscreen, FullscreenMode::Prevent);
        assert!(cfg.windows.fit_dialogs);
        assert_eq!(cfg.start_menu.hidden, vec!["a.desktop".to_string()]);
    }
}

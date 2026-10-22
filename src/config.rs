//! Shell configuration: design tokens, input thresholds and Today contents.
//!
//! Loaded from TOML. Every field has a default so a partial file (or no file)
//! still gives a working shell.

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
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct Design {
    pub font_family: String,
    pub font_px: u32,
    pub small_font_px: u32,
    pub title_font_px: u32,
    pub top_bar_px: u32,
    pub bottom_bar_px: u32,
    pub icon_px: u32,
    pub today_icon_px: u32,
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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            design: Design::default(),
            colors: Colors::default(),
            input: InputConfig::default(),
            today: TodayConfig::default(),
            keyboard: KeyboardConfig::default(),
        }
    }
}

impl Default for Design {
    fn default() -> Self {
        Self {
            font_family: "DejaVu Sans".into(),
            font_px: 12,
            small_font_px: 10,
            title_font_px: 12,
            top_bar_px: 24,
            bottom_bar_px: 26,
            icon_px: 16,
            today_icon_px: 24,
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
        Self { command: vec!["onboard".into()] }
    }
}

impl Config {
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
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
        assert_eq!(Config::parse("").unwrap(), Config::default());
    }

    #[test]
    fn partial_section_keeps_other_defaults() {
        let cfg = Config::parse("[input]\nlong_press_ms = 750\n").unwrap();
        assert_eq!(cfg.input.long_press_ms, 750);
        assert_eq!(cfg.input.long_press_slop_px, InputConfig::default().long_press_slop_px);
        assert_eq!(cfg.design, Design::default());
    }

    #[test]
    fn shipped_config_parses() {
        let text = include_str!("../config/pda-shell.toml");
        let cfg = Config::parse(text).expect("config/pda-shell.toml must parse");
        assert_eq!(cfg.input.long_press_ms, 600);
        assert!(!cfg.today.shortcuts.is_empty());
    }

    #[test]
    fn unknown_type_is_an_error() {
        assert!(Config::parse("[design]\nfont_px = \"big\"\n").is_err());
    }
}

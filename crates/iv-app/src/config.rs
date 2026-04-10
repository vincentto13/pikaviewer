use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const APP_DIR: &str = "pikaviewer";
const CONFIG_FILE: &str = "config.toml";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub window: WindowSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowSettings {
    /// When true, window resizes to fit each image (min 400x300).
    /// When false, window stays at a fixed size and images are letterboxed.
    #[serde(default = "default_fit_to_image")]
    pub fit_to_image: bool,

    /// Last used window width (physical pixels). Saved implicitly on resize.
    #[serde(default = "default_width")]
    pub width: u32,

    /// Last used window height (physical pixels). Saved implicitly on resize.
    #[serde(default = "default_height")]
    pub height: u32,
}

fn default_fit_to_image() -> bool { true }
fn default_width() -> u32 { 1280 }
fn default_height() -> u32 { 720 }

impl Default for WindowSettings {
    fn default() -> Self {
        Self {
            fit_to_image: default_fit_to_image(),
            width:        default_width(),
            height:       default_height(),
        }
    }
}

impl Settings {
    /// Path to the config file.
    pub fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join(APP_DIR).join(CONFIG_FILE))
    }

    /// Load from disk, or create the file with defaults if it doesn't exist.
    pub fn load_or_create() -> Self {
        let Some(path) = Self::config_path() else {
            log::warn!("could not determine config directory, using defaults");
            return Self::default();
        };

        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(content) => match toml::from_str(&content) {
                    Ok(settings) => return settings,
                    Err(e) => {
                        log::error!("parse {}: {e} — using defaults", path.display());
                        return Self::default();
                    }
                },
                Err(e) => {
                    log::error!("read {}: {e} — using defaults", path.display());
                    return Self::default();
                }
            }
        }

        // File doesn't exist — create with defaults
        let settings = Self::default();
        settings.save();
        settings
    }

    /// Write current settings to disk.
    pub fn save(&self) {
        let Some(path) = Self::config_path() else { return };

        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                log::error!("create config dir {}: {e}", parent.display());
                return;
            }
        }

        match toml::to_string_pretty(self) {
            Ok(content) => {
                if let Err(e) = std::fs::write(&path, content) {
                    log::error!("write {}: {e}", path.display());
                }
            }
            Err(e) => log::error!("serialize settings: {e}"),
        }
    }
}

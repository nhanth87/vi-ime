// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! vi-ime configuration: schema types, effective-config resolution,
//! and the `ConfigManager` that loads/saves `setting.conf`.

pub mod builtin;
mod effective;
mod learned;
mod types;

pub use effective::{EffectiveConfig, ResolvedConfig};
pub use learned::LearnedStore;
pub use types::{AppConfig, ImeMode, InputMethod, OutputMode, Setting, ToneStyle};

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tracing::warn;

fn toml_to_io_error(e: toml::ser::Error) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, e)
}

/// Manages loading, saving, and watching `setting.conf`.
pub struct ConfigManager {
    path: PathBuf,
    setting: Setting,
    last_mtime: Option<SystemTime>,
}

impl ConfigManager {
    /// Get the default config file path:
    /// `$XDG_CONFIG_HOME/vi-ime/setting.conf` or `$HOME/.config/vi-ime/setting.conf`.
    pub fn default_path() -> PathBuf {
        let base = if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            PathBuf::from(xdg)
        } else if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join(".config")
        } else {
            PathBuf::from(".config")
        };
        base.join("vi-ime").join("setting.conf")
    }

    /// Create a new ConfigManager. If the file doesn't exist, writes a default config.
    pub fn new(path: Option<PathBuf>) -> std::io::Result<Self> {
        let path = path.unwrap_or_else(Self::default_path);
        let setting = if path.exists() {
            let content = fs::read_to_string(&path)?;
            toml::from_str(&content).unwrap_or_else(|e| {
                warn!("Failed to parse config at {:?}: {}. Using defaults.", path, e);
                Setting::default()
            })
        } else {
            let setting = Setting::default();
            // Ensure parent directory exists
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let content = toml::to_string_pretty(&setting).map_err(toml_to_io_error)?;
            fs::write(&path, content)?;
            setting
        };

        let last_mtime = Self::mtime_of(&path);
        Ok(Self { path, setting, last_mtime })
    }

    /// Get the current setting.
    pub fn setting(&self) -> &Setting {
        &self.setting
    }

    /// Get a mutable reference to the current setting (for runtime changes).
    pub fn setting_mut(&mut self) -> &mut Setting {
        &mut self.setting
    }

    /// Save current setting to disk.
    pub fn save(&mut self) -> std::io::Result<()> {
        let content = toml::to_string_pretty(&self.setting).map_err(toml_to_io_error)?;
        fs::write(&self.path, content)?;
        self.last_mtime = Self::mtime_of(&self.path);
        Ok(())
    }

    /// Reload setting from disk.
    pub fn reload(&mut self) -> std::io::Result<()> {
        let content = fs::read_to_string(&self.path)?;
        self.setting = toml::from_str(&content).unwrap_or_else(|e| {
            warn!("Failed to parse config at {:?}: {}. Keeping current setting.", self.path, e);
            self.setting.clone()
        });
        self.last_mtime = Self::mtime_of(&self.path);
        Ok(())
    }

    /// Reload only if the file changed on disk since we last read/wrote it.
    /// Returns `Ok(true)` when a reload happened.
    pub fn reload_if_changed(&mut self) -> std::io::Result<bool> {
        let current = Self::mtime_of(&self.path);
        if current.is_some() && current != self.last_mtime {
            self.reload()?;
            return Ok(true);
        }
        Ok(false)
    }

    /// Path to the config file.
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    fn mtime_of(path: &Path) -> Option<SystemTime> {
        fs::metadata(path).and_then(|m| m.modified()).ok()
    }
}


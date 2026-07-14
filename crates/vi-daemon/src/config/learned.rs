// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Learned cache — runtime-observed app capabilities, the second layer of
//! the 4-layer resolution (user override > learned > builtin > global).
//!
//! Fed by hard protocol signals from the IME thread (SurroundingText seen,
//! activate), NOT heuristics. Persisted to `~/.local/share/vi-ime/learned.toml`
//! so the second session starts smart.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::config::types::{AppConfig, ImeMode};

/// What we have observed about one app. All signals are protocol facts.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LearnedProfile {
    /// App emitted `surrounding_text` at least once → live model is safe.
    /// `Some(false)` = activated repeatedly but never sent it.
    pub surrounding_text: Option<bool>,
    /// App attached a text input (IME Activate seen) at least once.
    pub ime_activated: Option<bool>,
    /// Unix seconds of the last update (absolute, not relative).
    #[serde(default)]
    pub updated_at: u64,
}

impl LearnedProfile {
    /// Derive a config override from the observations, or None if we have
    /// nothing actionable. Only `ime_mode` is ever suggested — the learned
    /// layer adapts the DISPLAY path, never the user's method/output.
    pub fn suggested_config(&self) -> Option<AppConfig> {
        let mode = self.suggested_mode()?;
        Some(AppConfig { ime_mode: Some(mode), ..AppConfig::default() })
    }

    /// No surrounding-text seen after repeated activations → fall back to
    /// real preedit for this app (the live model needs it).
    pub fn suggested_mode(&self) -> Option<ImeMode> {
        match self.surrounding_text {
            Some(false) => Some(ImeMode::Preedit),
            _ => None, // supported or unknown → builtin/global decides
        }
    }
}

/// All learned profiles, keyed by app_id. Owned by the daemon; the settings
/// UI reads the file for origin badges.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LearnedStore {
    #[serde(default)]
    pub apps: HashMap<String, LearnedProfile>,
}

impl LearnedStore {
    /// `~/.local/share/vi-ime/learned.toml` (respects XDG_DATA_HOME).
    pub fn default_path() -> PathBuf {
        let base = if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
            PathBuf::from(xdg)
        } else if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join(".local").join("share")
        } else {
            PathBuf::from(".")
        };
        base.join("vi-ime").join("learned.toml")
    }

    /// Load from disk; missing or unparsable file → empty store.
    pub fn load(path: &Path) -> Self {
        match fs::read_to_string(path) {
            Ok(s) => toml::from_str(&s).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Persist to disk (creates parent dirs).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        fs::write(path, content)
    }

    pub fn profile(&self, app_id: &str) -> Option<&LearnedProfile> {
        self.apps.get(app_id)
    }

    /// Update (or create) the profile for an app; stamps `updated_at`.
    /// Returns true when the observation actually changed the profile —
    /// callers use this to decide whether a (throttled) save is needed.
    pub fn observe(
        &mut self,
        app_id: &str,
        f: impl FnOnce(&mut LearnedProfile),
    ) -> bool {
        let entry = self.apps.entry(app_id.to_string()).or_default();
        let before = entry.clone();
        f(entry);
        if *entry == before {
            return false;
        }
        entry.updated_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        true
    }

    /// Config override derived from learning for this app, if any.
    pub fn suggested_config(&self, app_id: &str) -> Option<AppConfig> {
        self.apps.get(app_id).and_then(|p| p.suggested_config())
    }

}

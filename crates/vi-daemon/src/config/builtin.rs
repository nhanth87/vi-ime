// SPDX-License-Identifier: GPL-3.0-or-later OR Commercial
// Copyright (c) 2024-2026 vi-im contributors
//! Builtin app profiles — the SEED layer of the 4-layer resolution
//! (user override > learned > builtin > global).
//!
//! This is DATA, not user config: it never touches `setting.conf`. It makes
//! day-one typing good; the learned layer then improves on it at runtime and
//! the user layer overrides both. Matching is case-insensitive on app_id.

use crate::config::types::{AppConfig, ImeMode};

/// Builtin per-app profiles: (app_id, mode). Kept lowercase.
///
/// Terminals/editors → NonPreedit (no preedit underline).
/// Browsers/chat → Preedit (visual feedback while composing).
const BUILTIN_APPS: &[(&str, ImeMode)] = &[
    // ── Terminals ──
    ("foot", ImeMode::NonPreedit),
    ("footclient", ImeMode::NonPreedit),
    ("kitty", ImeMode::NonPreedit),
    ("alacritty", ImeMode::NonPreedit),
    ("wezterm", ImeMode::NonPreedit),
    ("org.wezfurlong.wezterm", ImeMode::NonPreedit),
    ("com.mitchellh.ghostty", ImeMode::NonPreedit),
    ("konsole", ImeMode::NonPreedit),
    ("gnome-terminal", ImeMode::NonPreedit),
    ("xfce4-terminal", ImeMode::NonPreedit),
    ("rio", ImeMode::NonPreedit),
    // ── Browsers ──
    ("chromium-browser", ImeMode::Preedit),
    ("chromium", ImeMode::Preedit),
    ("google-chrome", ImeMode::Preedit),
    ("firefox", ImeMode::Preedit),
    ("firefox-esr", ImeMode::Preedit),
    ("zen-browser", ImeMode::Preedit),
    ("zen", ImeMode::Preedit),
    ("brave-browser", ImeMode::Preedit),
    ("vivaldi-stable", ImeMode::Preedit),
    // ── Editors/IDEs ──
    ("code", ImeMode::NonPreedit),
    ("code-oss", ImeMode::NonPreedit),
    ("codium", ImeMode::NonPreedit),
    ("vscodium", ImeMode::NonPreedit),
    ("neovide", ImeMode::NonPreedit),
    ("sublime_text", ImeMode::NonPreedit),
    ("jetbrains-idea", ImeMode::NonPreedit),
    ("jetbrains-rustrover", ImeMode::NonPreedit),
    // ── Chat ──
    ("discord", ImeMode::Preedit),
    ("slack", ImeMode::Preedit),
    ("telegram-desktop", ImeMode::Preedit),
    ("org.telegram.desktop", ImeMode::Preedit),
    ("signal", ImeMode::Preedit),
    ("element", ImeMode::Preedit),
];

/// Builtin per-site profiles: (title substring, mode). Rich-text web editors
/// fight preedit — force the hidden live path there.
const BUILTIN_SITES: &[(&str, ImeMode)] = &[
    ("facebook", ImeMode::NonPreedit),
    ("messenger", ImeMode::NonPreedit),
    ("tiktok", ImeMode::NonPreedit),
    ("google docs", ImeMode::NonPreedit),
    ("google sheets", ImeMode::NonPreedit),
    ("notion", ImeMode::NonPreedit),
];

fn mode_config(m: ImeMode) -> AppConfig {
    AppConfig { ime_mode: Some(m), ..AppConfig::default() }
}

/// Builtin profile for an app_id (case-insensitive exact match).
pub fn builtin_app_profile(app_id: &str) -> Option<AppConfig> {
    let id = app_id.to_lowercase();
    BUILTIN_APPS
        .iter()
        .find(|(k, _)| *k == id)
        .map(|(_, m)| mode_config(*m))
}

/// Builtin site profile: longest title-substring match (both lowercase),
/// mirroring the user site rules in R13.
pub fn builtin_site_profile(title: &str) -> Option<AppConfig> {
    let t = title.to_lowercase();
    BUILTIN_SITES
        .iter()
        .filter(|(k, _)| t.contains(k))
        .max_by_key(|(k, _)| k.len())
        .map(|(_, m)| mode_config(*m))
}


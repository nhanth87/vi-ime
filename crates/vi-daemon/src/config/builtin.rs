// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 vi-im contributors
//! Builtin app profiles — the SEED layer of the 4-layer resolution
//! (user override > learned > builtin > global).
//!
//! This is DATA, not user config: it never touches `setting.conf`. It makes
//! day-one typing good; the learned layer then improves on it at runtime and
//! the user layer overrides both. Matching is case-insensitive on app_id.

use crate::compositor::KNOWN_TERMINALS;
use crate::config::types::{AppConfig, ImeMode};

/// Builtin per-app profiles: (app_id, mode). Kept lowercase.
///
/// Terminals/editors → NonPreedit (no preedit underline).
/// Browsers/chat → Preedit (visual feedback while composing).
///
/// Terminals are NOT listed here — `builtin_app_profile` checks
/// `compositor::KNOWN_TERMINALS` first (single source of truth shared
/// with `AppCategory::classify` and `TerminalPlugin`), so every terminal
/// in that list gets NonPreedit without needing a duplicate entry here.
const BUILTIN_APPS: &[(&str, ImeMode)] = &[
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

/// Chromium-family ids: Preedit double-inputs under niri (the very reason
/// ChromiumNiriPlugin exists — but plugins are advisory-only per R13, so
/// the mode has to be decided HERE, in a resolution layer).
const CHROMIUM_FAMILY: &[&str] = &[
    "chromium-browser", "chromium", "google-chrome", "brave-browser",
    "vivaldi-stable", "discord", "slack", "element",
];

fn on_niri() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP")
        .map(|d| d.to_lowercase().contains("niri"))
        .unwrap_or(false)
}

/// Builtin profile for an app_id (case-insensitive exact match).
pub fn builtin_app_profile(app_id: &str) -> Option<AppConfig> {
    let id = app_id.to_lowercase();
    if KNOWN_TERMINALS.contains(&id.as_str()) {
        return Some(mode_config(ImeMode::NonPreedit));
    }
    let (_, m) = BUILTIN_APPS.iter().find(|(k, _)| *k == id)?;
    let mode = if *m == ImeMode::Preedit && on_niri() && CHROMIUM_FAMILY.contains(&id.as_str()) {
        ImeMode::NonPreedit
    } else {
        *m
    };
    Some(mode_config(mode))
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


// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Core config types: enums, per-app/per-site overrides, and the `Setting` root.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Input method supported by the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum InputMethod {
    #[default]
    Telex,
    Vni,
    /// Tự do (Freedom): accept tones from BOTH VNI digits (1-6)
    /// and Telex modifiers (s/f/r/x/j) in the same word.
    #[serde(alias = "smart", alias = "tudo", alias = "Tự do", alias = "Tu do")]
    Smart,
}

impl std::fmt::Display for InputMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InputMethod::Telex => write!(f, "Telex"),
            InputMethod::Vni => write!(f, "VNI"),
            InputMethod::Smart => write!(f, "Tự do"),
        }
    }
}

/// IME composition mode — controls preedit display only.
/// Hybrid mode removed: Smart/Freedom is now an InputMethod.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum ImeMode {
    /// Show preedit (underlined) while composing.
    #[default]
    Preedit,
    /// No preedit underline while composing.
    NonPreedit,
}

impl std::fmt::Display for ImeMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImeMode::Preedit => write!(f, "Preedit"),
            ImeMode::NonPreedit => write!(f, "NonPreedit"),
        }
    }
}

/// Tone placement style for glide clusters without coda (hòa vs hoà).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ToneStyle {
    /// "hòa", "thúy" — kiểu đặt dấu cũ, quen thuộc truyền thống.
    #[default]
    Classic,
    /// "hoà", "thuý" — dấu trên âm chính (chuẩn ngôn ngữ học).
    Modern,
}

impl std::fmt::Display for ToneStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToneStyle::Classic => write!(f, "Kiểu cũ (hòa)"),
            ToneStyle::Modern => write!(f, "Kiểu mới (hoà)"),
        }
    }
}

/// Unicode output mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum OutputMode {
    /// Precomposed Unicode (NFC) — dựng sẵn, e.g. ệ, ế
    #[default]
    UnicodeDungSan,
    /// Decomposed Unicode (NFD) — tổ hợp, e.g. ệ = e + ^ + .
    UnicodeToHop,
}

impl std::fmt::Display for OutputMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OutputMode::UnicodeDungSan => write!(f, "Dựng sẵn"),
            OutputMode::UnicodeToHop => write!(f, "Tổ hợp"),
        }
    }
}

/// Per-app (or per-site) configuration override.
/// All fields are optional; `None` = inherit from the level below
/// (site inherits from app, app inherits from global).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct AppConfig {
    /// Input method for this app. None = use global default.
    pub input_method: Option<InputMethod>,
    /// Whether IME is enabled for this app.
    pub enabled: Option<bool>,
    /// IME composition mode. None = use global default.
    pub ime_mode: Option<ImeMode>,
    /// Unicode output mode. None = use global default.
    pub output_mode: Option<OutputMode>,
    /// Free tone placement. None = use global default.
    pub free_tone_placement: Option<bool>,
    /// Auto-detect language. None = use global default.
    pub auto_detect_lang: Option<bool>,
}

/// Main configuration structure matching `setting.conf`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Setting {
    /// Global input method (Telex or VNI).
    pub input_method: InputMethod,
    /// Whether IME starts enabled.
    pub enabled: bool,
    /// Unicode output mode.
    pub output_mode: OutputMode,
    /// Enable free tone placement (tự do bỏ dấu). If false, use strict modern rules.
    pub free_tone_placement: bool,
    /// Auto-detect language (English vs Vietnamese).
    pub auto_detect_lang: bool,
    /// Enable per-app automatic switching.
    pub enable_per_app: bool,
    /// Global default IME mode (Preedit, NonPreedit, Hybrid).
    #[serde(default = "default_ime_mode")]
    pub ime_mode: ImeMode,
    /// Tone placement style (hòa vs hoà). Old configs default to Classic.
    #[serde(default)]
    pub tone_style: ToneStyle,
    /// Autocorrect: fix common Vietnamese typos on commit.
    #[serde(default = "default_true")]
    pub autocorrect: bool,
    /// Emoji shortcode expansion (e.g. ":smile:" → "😄").
    #[serde(default = "default_true")]
    pub emoji: bool,
    /// Clipboard Vietnamese conversion (convert pasted text to correct Unicode).
    #[serde(default = "default_true")]
    pub clipboard_convert: bool,
    /// Per-app overrides. Key is app_id/class.
    #[serde(default)]
    pub app_configs: HashMap<String, AppConfig>,
    /// Per-site overrides for browsers. Key is a lowercase substring
    /// matched against the focused window's title (e.g. "facebook").
    #[serde(default)]
    pub site_configs: HashMap<String, AppConfig>,
}

fn default_ime_mode() -> ImeMode {
    ImeMode::Preedit
}

fn default_true() -> bool {
    true
}

impl Default for Setting {
    fn default() -> Self {
        Self {
            input_method: InputMethod::Telex,
            enabled: true,
            output_mode: OutputMode::UnicodeDungSan,
            free_tone_placement: true,
            auto_detect_lang: true,
            enable_per_app: true,
            ime_mode: ImeMode::Preedit,
            tone_style: ToneStyle::Classic,
            autocorrect: true,
            emoji: true,
            clipboard_convert: true,
            app_configs: HashMap::new(),
            site_configs: HashMap::new(),
        }
    }
}

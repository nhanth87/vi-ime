// SPDX-License-Identifier: GPL-3.0-or-later OR Commercial
// Copyright (c) 2024-2026 vi-im contributors
/// Supported Vietnamese input methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMethod {
    Telex,
    Vni,
    /// Tự do (Freedom): accept tone marks from BOTH VNI digits (1-6)
    /// AND Telex modifiers (s/f/r/x/j) in the same word.
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

/// IME composition mode — controls display behavior only.
/// No more Hybrid: Smart/Freedom moved to InputMethod.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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

/// Actions returned by NonPreeditEngine for the Wayland layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NonPreeditAction {
    /// Commit text after backspacing N raw characters.
    /// The Wayland layer should:
    /// 1. Call delete_surrounding_text(-N, N)
    /// 2. Call commit_string(text)
    /// 3. Call commit(serial)
    CommitWithBackspace {
        backspace_count: usize,
        text: String,
    },
    /// Update the preedit string (used in Hybrid/Preedit mode).
    UpdatePreedit(String),
    /// Keep buffering — no Wayland action needed (NonPreedit mode).
    Buffer,
    /// Pass the key through unchanged.
    PassThrough,
    /// Clear preedit display (if any).
    ClearPreedit,
}

/// Unicode output mode — mirrors vi_config::OutputMode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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
            OutputMode::UnicodeDungSan => write!(f, "UnicodeDungSan"),
            OutputMode::UnicodeToHop => write!(f, "UnicodeToHop"),
        }
    }
}

/// Actions the engine requests the Wayland layer to perform.
/// The engine NEVER sends Wayland requests directly — this is a pure data return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Update the preedit string (in-progress composition).
    UpdatePreedit(String),
    /// Commit the final composed string to the application.
    Commit(String),
    /// Let the key pass through unchanged (not a Vietnamese-related key).
    PassThrough,
}

/// Whether the focused app supports the IME protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppSupport {
    Unknown,
    Supported,
    Unsupported,
}

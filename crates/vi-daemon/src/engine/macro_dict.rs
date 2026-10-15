// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Keyword macros (gõ tắt) + hotkey macros.
//!
//! Two trigger kinds, one O(1) matcher (fast even while touch-typing):
//! - keyword: typed word + word boundary → replacement text
//!   (`[macros] vn = "Việt Nam"`).
//! - hotkey: modifier chord → replacement text, consuming the chord
//!   (`[macro_hotkeys] "ctrl+alt+m" = "..."`).
//!
//! This is USER data (a dictionary), not a vowel table — R14's table-free
//! rule governs tone composition, not user-configured expansions.
//! Safety: R18 (IME off ⇒ never fires — callers gate before reaching here),
//! R11b (callers skip Secure/Url fields), expansion length capped so a
//! runaway macro can't stall the paced typer for minutes.

use std::collections::HashMap;

/// Maximum entries per table (rest ignored with a warning).
const MAX_MACROS: usize = 1000;
/// Maximum expansion length in chars (truncated with a warning).
const MAX_MACRO_TEXT_CHARS: usize = 500;

/// A hotkey macro trigger: modifiers + one key. At least one of
/// ctrl/alt/super is required — bare or shift-only keys must keep typing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hotkey {
    pub ctrl: bool,
    pub alt: bool,
    pub super_: bool,
    pub shift: bool,
    pub key: HotkeyKey,
}

/// The key part of a hotkey: alphanumerics (lowercased), F1–F12, or named
/// non-text keys. Wayland grab keycodes and evdev codes share numbering
/// (evdev codes, no +8 — `xkb::keycode_to_char` adds it internally).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyKey {
    Char(char),
    F(u8),
    Space,
    Enter,
    Tab,
    Backspace,
    Escape,
    Delete,
}

/// Parse `"ctrl+alt+m"` / `"super+f12"` / `"ctrl+shift+space"`. Returns `None`
/// for empty specs, missing key part, multiple key parts, unknown parts, or
/// chords without ctrl/alt/super.
pub fn parse_hotkey(spec: &str) -> Option<Hotkey> {
    let mut ctrl = false;
    let mut alt = false;
    let mut super_ = false;
    let mut shift = false;
    let mut key: Option<HotkeyKey> = None;
    for part in spec.split('+') {
        let p = part.trim().to_ascii_lowercase();
        if p.is_empty() {
            return None;
        }
        let k = match p.as_str() {
            "ctrl" | "control" => {
                ctrl = true;
                continue;
            }
            "alt" | "mod1" => {
                alt = true;
                continue;
            }
            "super" | "meta" | "win" | "mod4" => {
                super_ = true;
                continue;
            }
            "shift" => {
                shift = true;
                continue;
            }
            "space" | " " => HotkeyKey::Space,
            "enter" | "return" | "\n" | "\r" => HotkeyKey::Enter,
            "tab" | "\t" => HotkeyKey::Tab,
            "backspace" | "bs" => HotkeyKey::Backspace,
            "esc" | "escape" => HotkeyKey::Escape,
            "delete" | "del" => HotkeyKey::Delete,
            _ => {
                if let Some(f) = p.strip_prefix('f')
                    && let Ok(n) = f.parse::<u8>()
                    && (1..=12).contains(&n)
                {
                    HotkeyKey::F(n)
                } else if p.chars().count() == 1 {
                    let c = p.chars().next().unwrap_or('?').to_ascii_lowercase();
                    if c == ' ' {
                        HotkeyKey::Space
                    } else {
                        HotkeyKey::Char(c)
                    }
                } else {
                    return None;
                }
            }
        };
        if key.is_some() {
            return None; // two key parts ("a+b")
        }
        key = Some(k);
    }
    let key = key?;
    if !(ctrl || alt || super_) {
        return None;
    }
    Some(Hotkey {
        ctrl,
        alt,
        super_,
        shift,
        key,
    })
}

/// Parsed macro tables: keyword → text, hotkey chord → text.
#[derive(Debug, Clone, Default)]
pub struct MacroDict {
    keywords: HashMap<String, String>,
    hotkeys: Vec<(Hotkey, String)>,
}

fn truncate(text: &str) -> String {
    if text.chars().count() > MAX_MACRO_TEXT_CHARS {
        tracing::warn!(
            "[MACRO] expansion quá dài ({} chars) — cắt còn {MAX_MACRO_TEXT_CHARS}",
            text.chars().count()
        );
        text.chars().take(MAX_MACRO_TEXT_CHARS).collect()
    } else {
        text.to_string()
    }
}

impl MacroDict {
    /// Build from raw config tables. Invalid hotkey specs are skipped with a
    /// warning; empty keys/texts are ignored. Cheap — call on config change,
    /// never per keystroke.
    pub fn build(keywords: &HashMap<String, String>, hotkeys: &HashMap<String, String>) -> Self {
        let mut kw = HashMap::new();
        for (k, v) in keywords.iter().take(MAX_MACROS) {
            if !k.is_empty() && !v.is_empty() {
                kw.insert(k.clone(), truncate(v));
            }
        }
        if keywords.len() > MAX_MACROS {
            tracing::warn!("[MACRO] quá {MAX_MACROS} gõ tắt — bỏ phần thừa");
        }
        let mut hk = Vec::new();
        for (spec, text) in hotkeys.iter().take(MAX_MACROS) {
            match parse_hotkey(spec) {
                Some(h) if !text.is_empty() => hk.push((h, truncate(text))),
                _ => tracing::warn!("[MACRO] hotkey spec bỏ qua: {spec:?}"),
            }
        }
        if hotkeys.len() > MAX_MACROS {
            tracing::warn!("[MACRO] quá {MAX_MACROS} macro phím — bỏ phần thừa");
        }
        if !kw.is_empty() || !hk.is_empty() {
            tracing::info!("[MACRO] tables: {} keywords, {} hotkeys", kw.len(), hk.len());
        }
        Self {
            keywords: kw,
            hotkeys: hk,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.keywords.is_empty() && self.hotkeys.is_empty()
    }

    /// Keyword expansion for a completed word (raw keys as typed): exact
    /// match first, then lowercase fallback ("VN" → "vn" entry). Two O(1)
    /// lookups; `None` (empty table / no match) is the zero-cost common case.
    pub fn expand_keyword(&self, word: &str) -> Option<String> {
        if word.is_empty() || self.keywords.is_empty() {
            return None;
        }
        if let Some(t) = self.keywords.get(word) {
            return Some(t.clone());
        }
        let lower = word.to_ascii_lowercase();
        if lower != word {
            return self.keywords.get(&lower).cloned();
        }
        None
    }

    /// Hotkey match for a modifier chord. Only called on chords (rare), so
    /// the linear scan over a handful of hotkeys is negligible.
    pub fn match_hotkey(
        &self,
        ctrl: bool,
        alt: bool,
        sup: bool,
        shift: bool,
        key: &HotkeyKey,
    ) -> Option<String> {
        if self.hotkeys.is_empty() {
            return None;
        }
        self.hotkeys
            .iter()
            .find(|(h, _)| {
                h.ctrl == ctrl && h.alt == alt && h.super_ == sup && h.shift == shift && h.key == *key
            })
            .map(|(_, t)| t.clone())
    }
}

#[cfg(test)]
#[path = "macro_tests.rs"]
mod macro_tests;

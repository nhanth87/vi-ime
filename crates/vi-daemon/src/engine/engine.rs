// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Engine facade — thin state machine over the unified syllable engine.
//!
//! Raw keys are the single source of truth. Every keystroke the whole word is
//! re-parsed through ONE path ([`crate::engine::syllable`], the NFD/Unicode-math
//! engine) for Telex and VNI alike — there is no vowel-cluster table.
//! English restore is validity-based (R9): a word that never forms a valid
//! Vietnamese syllable is displayed/committed exactly as typed.

use unicode_normalization::UnicodeNormalization;

use crate::engine::style::ToneStyle;
use crate::engine::syllable::{self, Outcome};
use crate::engine::types::{Action, InputMethod, OutputMode};

/// Snapshot of the render state after the n-th keystroke — the undo stack
/// entry. Because [`Engine::reparse`] is a pure function of `raw_keys`,
/// restoring a snapshot is identical to re-parsing the shorter word, so
/// backspace is O(1) with no behavior change (P0-2).
#[derive(Clone)]
struct Snapshot {
    display: String,
    last_valid: bool,
}

/// The main engine state machine.
///
/// Transaction view of a word (P0-2): `raw_keys` = raw input, `display` =
/// what the user currently sees, committed text lives in the Wayland layer
/// (it leaves the engine on `Action::Commit` and is never mutated again).
pub struct Engine {
    method: InputMethod,
    /// Raw keystrokes for the current word — the single source of truth.
    raw_keys: Vec<char>,
    /// Rendered preedit (reused buffer; rewritten on every keystroke).
    display: String,
    /// Whether the last parse formed a valid Vietnamese syllable.
    last_valid: bool,
    /// Undo stack: `undo[i]` = render state after `i + 1` raw keys.
    /// Pushed on every composing keystroke, popped on backspace.
    undo: Vec<Snapshot>,
    auto_detect: bool,
    free_tone: bool,
    tone_style: ToneStyle,
    output_mode: OutputMode,
}

impl Engine {
    pub fn new(method: InputMethod) -> Self {
        Self {
            method,
            raw_keys: Vec::with_capacity(16),
            display: String::with_capacity(16),
            last_valid: false,
            undo: Vec::with_capacity(16),
            auto_detect: true,
            free_tone: true,
            tone_style: ToneStyle::Classic,
            output_mode: OutputMode::UnicodeDungSan,
        }
    }

    // ── Config (signatures fixed — RuntimeConfig::apply_snapshot depends) ──

    pub fn set_method(&mut self, method: InputMethod) {
        self.method = method;
    }
    pub fn method(&self) -> InputMethod {
        self.method
    }

    /// English auto-restore: invalid syllables are displayed/committed as raw.
    pub fn set_auto_detect(&mut self, enabled: bool) {
        self.auto_detect = enabled;
    }
    pub fn auto_detect(&self) -> bool {
        self.auto_detect
    }

    /// Kept for config compatibility: phonotactic validation subsumes the
    /// old "strict tone" mode — tones only ever land on valid targets.
    pub fn set_free_tone(&mut self, enabled: bool) {
        self.free_tone = enabled;
    }
    pub fn free_tone(&self) -> bool {
        self.free_tone
    }

    pub fn set_output_mode(&mut self, mode: OutputMode) {
        self.output_mode = mode;
    }
    pub fn output_mode(&self) -> OutputMode {
        self.output_mode
    }

    pub fn set_tone_style(&mut self, style: ToneStyle) {
        self.tone_style = style;
    }
    pub fn tone_style(&self) -> ToneStyle {
        self.tone_style
    }

    // ── State queries ──

    pub fn has_preedit(&self) -> bool {
        !self.raw_keys.is_empty()
    }
    pub fn preedit_string(&self) -> &str {
        &self.display
    }
    pub fn raw_key_count(&self) -> usize {
        self.raw_keys.len()
    }

    /// Buffer formatted for committing: applies the output mode (NFC/NFD).
    pub fn preedit_output(&self) -> String {
        match self.output_mode {
            OutputMode::UnicodeDungSan => self.display.clone(),
            OutputMode::UnicodeToHop => self.display.nfd().collect(),
        }
    }

    /// Conservative Smart-mode commit for the NonPreedit path (all real apps).
    /// Restores raw keys ONLY when they form a KNOWN English word (test→test,
    /// user→user). Does NOT use the `is_viet_syllable` fallback that
    /// `smart_commit_output` applies — that fallback strips ALL marks via
    /// `base_of` (ấ→a, mất→mat) and restores anything not matching the exact
    /// single-syllable list, which wrongly mangles valid multi-char Vietnamese
    /// ("ấ", "mất" → raw "aas"/"maas"; field bug 2026-07-12). Dictionary
    /// English-restore is the safe, targeted part of R9.
    pub fn smart_commit_english_only(&self, app_id: Option<&str>) -> String {
        // Cheat system: field-reported false positives get forced English
        if let Some(forced) = super::cheat::should_force_english(app_id, &self.raw_keys) {
            return forced;
        }
        if self.method != InputMethod::Smart || !self.auto_detect {
            return self.preedit_output();
        }
        if !self.last_valid {
            return self.preedit_output();
        }
        // Non-text heuristic (URLs, code, terminal commands): suppress
        // Vietnamese composition on address bars / terminals where the
        // compositor sends no ContentType signal (evdev fallback).
        // 2026-07-15: Chrome address bar "https://..." → "https://..."
        // instead of "http://..." composed with Telex tone keys.
        if super::viet_dict::looks_like_non_text(&self.raw_keys) {
            return self.raw_keys.iter().collect();
        }
        if super::viet_dict::is_english_word(&self.raw_keys) {
            return self.raw_keys.iter().collect();
        }
        self.preedit_output()
    }

    /// Smart mode commit: if the rendered word looks Vietnamese but the raw
    /// keys form a known English/programming word, restore raw keys instead.
    /// This extends R9 to handle false positives like test→tét, user→ủe.
    pub fn smart_commit_output(&self) -> String {
        use super::viet_dict;
        // Only applies to Smart mode with auto_detect enabled
        if self.method != InputMethod::Smart || !self.auto_detect {
            return self.preedit_output();
        }
        // If already invalid (raw keys shown), just commit as-is
        if !self.last_valid {
            return self.preedit_output();
        }
        // Check if raw keys form a known English word — if so, restore raw
        if viet_dict::is_english_word(&self.raw_keys) {
            return self.raw_keys.iter().collect();
        }
        // Check if rendered base is actually a valid Vietnamese syllable
        let base = viet_dict::strip_tones(&self.display);
        if !viet_dict::is_viet_syllable(&base) {
            // Rendered something that looks Vietnamese to the parser but
            // isn't in the real dictionary — restore raw keys
            return self.raw_keys.iter().collect();
        }
        self.preedit_output()
    }

    /// Hybrid-mode trigger: composition is ambiguous when the current word
    /// does not (yet) parse as Vietnamese.
    pub fn is_ambiguous(&self) -> bool {
        !self.raw_keys.is_empty() && !self.last_valid
    }

    // ── Key processing ──

    pub fn push_key(&mut self, ch: char) -> Action {
        // Backspace during composition: shrink the raw_keys buffer rather than
        // treating the control char as a word boundary (which would commit).
        // This lets the Wayland action handler route keycode 14 through the
        // engine on the normal push_key path.
        if ch == '\u{0008}' {
            return self.backspace();
        }

        // Word boundary: commit what we have (boundary char handled by caller).
        if syllable::is_word_boundary(ch, self.method) {
            if self.has_preedit() {
                // Validity-based restore (R9): the display already holds the
                // raw keys verbatim when the word isn't a valid syllable.
                // Extended R9 for Smart mode: if rendered looks Vietnamese but
                // raw keys are a known English word, restore raw.
                let committed = self.smart_commit_output();
                self.reset();
                return Action::Commit(committed);
            }
            return Action::PassThrough;
        }

        // First key of a word must be a letter (VNI digits need a vowel first).
        if self.raw_keys.is_empty() && !ch.is_ascii_alphabetic() {
            return Action::PassThrough;
        }

        self.raw_keys.push(ch);
        self.reparse();
        // Undo stack (P0-2): snapshot the render state so backspace can
        // restore it in O(1) instead of re-parsing the word.
        self.undo.push(Snapshot {
            display: self.display.clone(),
            last_valid: self.last_valid,
        });
        Action::UpdatePreedit(self.preedit_output())
    }

    pub fn backspace(&mut self) -> Action {
        if self.raw_keys.is_empty() {
            return Action::PassThrough;
        }
        self.raw_keys.pop();
        // O(1) restore from the undo stack (P0-2). `reparse` is a pure
        // function of `raw_keys`, so the popped-to snapshot is identical
        // to what re-parsing the shorter word would produce.
        self.undo.pop();
        match self.undo.last() {
            Some(snap) => {
                self.display.clear();
                self.display.push_str(&snap.display);
                self.last_valid = snap.last_valid;
                Action::UpdatePreedit(self.preedit_output())
            }
            None => {
                self.display.clear();
                self.last_valid = false;
                Action::UpdatePreedit(String::new())
            }
        }
    }

    pub fn reset(&mut self) {
        self.raw_keys.clear();
        self.display.clear();
        self.last_valid = false;
        self.undo.clear();
    }

    /// Re-derive the display from raw keys ("parse, don't mutate") through the
    /// single unified NFD path.
    fn reparse(&mut self) {
        match syllable::process(&self.raw_keys, self.method, self.tone_style) {
            Outcome::Rendered(s) => {
                self.display = s;
                self.last_valid = true;
            }
            Outcome::Raw => {
                // Not (yet) a Vietnamese syllable → show/commit raw keys.
                self.display.clear();
                self.display.extend(self.raw_keys.iter());
                self.last_valid = false;
            }
        }
    }
}

// ============================================================================
// Engine regression tests (R10)
// ============================================================================


#[cfg(test)]
#[path = "engine_tests.rs"]
mod engine_tests;

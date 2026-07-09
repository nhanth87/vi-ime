// SPDX-License-Identifier: GPL-3.0-or-later OR Commercial
// Copyright (c) 2024-2026 vi-im contributors
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

    pub fn set_method(&mut self, method: InputMethod) { self.method = method; }
    pub fn method(&self) -> InputMethod { self.method }

    /// English auto-restore: invalid syllables are displayed/committed as raw.
    pub fn set_auto_detect(&mut self, enabled: bool) { self.auto_detect = enabled; }
    pub fn auto_detect(&self) -> bool { self.auto_detect }

    /// Kept for config compatibility: phonotactic validation subsumes the
    /// old "strict tone" mode — tones only ever land on valid targets.
    pub fn set_free_tone(&mut self, enabled: bool) { self.free_tone = enabled; }
    pub fn free_tone(&self) -> bool { self.free_tone }

    pub fn set_output_mode(&mut self, mode: OutputMode) { self.output_mode = mode; }
    pub fn output_mode(&self) -> OutputMode { self.output_mode }

    pub fn set_tone_style(&mut self, style: ToneStyle) { self.tone_style = style; }
    pub fn tone_style(&self) -> ToneStyle { self.tone_style }

    // ── State queries ──

    pub fn has_preedit(&self) -> bool { !self.raw_keys.is_empty() }
    pub fn preedit_string(&self) -> &str { &self.display }
    pub fn raw_key_count(&self) -> usize { self.raw_keys.len() }

    /// Buffer formatted for committing: applies the output mode (NFC/NFD).
    pub fn preedit_output(&self) -> String {
        match self.output_mode {
            OutputMode::UnicodeDungSan => self.display.clone(),
            OutputMode::UnicodeToHop => self.display.nfd().collect(),
        }
    }

    /// Hybrid-mode trigger: composition is ambiguous when the current word
    /// does not (yet) parse as Vietnamese.
    pub fn is_ambiguous(&self) -> bool {
        !self.raw_keys.is_empty() && !self.last_valid
    }

    // ── Key processing ──

    pub fn push_key(&mut self, ch: char) -> Action {
        // Word boundary: commit what we have (boundary char handled by caller).
        if syllable::is_word_boundary(ch, self.method) {
            if self.has_preedit() {
                // Validity-based restore (R9): the display already holds the
                // raw keys verbatim when the word isn't a valid syllable.
                let committed = self.preedit_output();
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
mod tests {
    use super::*;

    // ── Basic word workflow ──

    #[test]
    fn telex_simple_word() {
        let mut e = Engine::new(InputMethod::Telex);
        for ch in "nha".chars() {
            let _ = e.push_key(ch);
        }
        assert!(e.has_preedit());
        assert_eq!(e.preedit_string(), "nha");
        let action = e.push_key(' ');
        assert!(matches!(action, Action::Commit(_)));
        assert!(!e.has_preedit());
    }

    #[test]
    fn telex_tone_sac() {
        let mut e = Engine::new(InputMethod::Telex);
        for ch in "toans".chars() {
            let _ = e.push_key(ch);
        }
        assert_eq!(e.preedit_string(), "toán");
    }

    #[test]
    fn telex_tone_nang() {
        let mut e = Engine::new(InputMethod::Telex);
        for ch in "nawngj".chars() {
            let _ = e.push_key(ch);
        }
        assert_eq!(e.preedit_string(), "nặng");
    }

    #[test]
    fn vni_tone_numbers() {
        let mut e = Engine::new(InputMethod::Vni);
        for ch in "nha2n".chars() {
            let _ = e.push_key(ch);
        }
        assert_eq!(e.preedit_string(), "nhàn");
    }

    // ── English restore (R9) ──

    #[test]
    fn english_raw_keys_restored() {
        let mut e = Engine::new(InputMethod::Telex);
        for ch in "windows".chars() {
            let _ = e.push_key(ch);
        }
        assert_eq!(e.preedit_string(), "windows");
        assert!(e.is_ambiguous());
    }

    #[test]
    fn english_word_boundary_in_telex() {
        let mut e = Engine::new(InputMethod::Telex);
        for ch in "win".chars() {
            let _ = e.push_key(ch);
        }
        let action = e.push_key('1');
        assert!(matches!(action, Action::Commit(_)));
    }

    #[test]
    fn english_word_boundary_in_vni() {
        let mut e = Engine::new(InputMethod::Vni);
        for ch in "win".chars() {
            let _ = e.push_key(ch);
        }
        let action = e.push_key(' ');
        assert!(matches!(action, Action::Commit(_)));
    }

    // ── Backspace ──

    #[test]
    fn backspace_removes_key() {
        let mut e = Engine::new(InputMethod::Telex);
        for ch in "nha".chars() {
            let _ = e.push_key(ch);
        }
        assert_eq!(e.preedit_string(), "nha");
        let action = e.backspace();
        assert!(matches!(action, Action::UpdatePreedit(_)));
        assert_eq!(e.preedit_string(), "nh");
    }

    #[test]
    fn backspace_on_empty() {
        let mut e = Engine::new(InputMethod::Telex);
        let action = e.backspace();
        assert!(matches!(action, Action::PassThrough));
    }

    #[test]
    fn reset_clears_all() {
        let mut e = Engine::new(InputMethod::Telex);
        for ch in "nha".chars() {
            let _ = e.push_key(ch);
        }
        e.reset();
        assert!(!e.has_preedit());
        assert_eq!(e.preedit_string(), "");
        assert_eq!(e.raw_key_count(), 0);
    }

    #[test]
    fn tone_style_classic() {
        let mut e = Engine::new(InputMethod::Telex);
        for ch in "hoaf".chars() {
            let _ = e.push_key(ch);
        }
        assert_eq!(e.preedit_string(), "hòa");
    }

    #[test]
    fn tone_style_modern() {
        let mut e = Engine::new(InputMethod::Telex);
        e.set_tone_style(ToneStyle::Modern);
        for ch in "hoaf".chars() {
            let _ = e.push_key(ch);
        }
        assert_eq!(e.preedit_string(), "hoà");
    }

    #[test]
    fn first_key_non_letter_passthrough() {
        let mut e = Engine::new(InputMethod::Telex);
        let action = e.push_key('2');
        assert!(matches!(action, Action::PassThrough));
    }

    // ── Hybrid mode regression: preedit visible while composing ──

    #[test]
    fn has_preedit_while_composing() {
        let mut e = Engine::new(InputMethod::Telex);
        for ch in "nha".chars() {
            let _ = e.push_key(ch);
        }
        assert!(e.has_preedit());
    }

    #[test]
    fn no_preedit_after_commit() {
        let mut e = Engine::new(InputMethod::Telex);
        for ch in "nha".chars() {
            let _ = e.push_key(ch);
        }
        let _ = e.push_key(' ');
        assert!(!e.has_preedit());
    }
}

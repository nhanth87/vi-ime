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
    /// Language of the recent words, for words that are BOTH a common
    /// English word and a valid Vietnamese syllable ("rest"/"rét",
    /// "conf"/"còn"). >0 = English context, <0 = Vietnamese, 0 = none.
    /// Survives `reset` (it spans words); Cell so the &self commit path can
    /// update it.
    lang: std::cell::Cell<i8>,
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
            lang: std::cell::Cell::new(0),
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

    /// Raw word as typed (tone keys included) — macro-expansion key and
    /// English-restore input. Cheap clone, called only at word boundaries.
    pub fn raw_word(&self) -> String {
        self.raw_keys.iter().collect()
    }

    /// Buffer formatted for committing: applies the output mode (NFC/NFD).
    pub fn preedit_output(&self) -> String {
        match self.output_mode {
            OutputMode::UnicodeDungSan => self.display.clone(),
            OutputMode::UnicodeToHop => self.display.nfd().collect(),
        }
    }

    /// Word-boundary commit (every NonPreedit path, R9): decides between the
    /// rendered Vietnamese and the raw keys.
    /// 1. Composition produced no real Vietnamese syllable ("text" → "tẽt",
    ///    "are" → "ảe") → raw keys. Structural check (rhyme.rs), any method.
    /// 2. Telex/Tự do: raw keys are a known English word → English, EXCEPT
    ///    when the rendering is also a real Vietnamese word ("rest"/"rét",
    ///    "conf"/"còn", "nos"/"nó"): then the recent words decide (English
    ///    sentence → English, Vietnamese sentence → Vietnamese), and with no
    ///    context the common Vietnamese reading wins for `prefer_vietnamese`
    ///    words. (Before 2026-10-24 the dictionary won outright: Tự do
    ///    typed "còn" as "conf" and "nó" as "nos".)
    /// No word-list mangling of valid Vietnamese ("ấ", "mất" — field bug
    /// 2026-07-12): Vietnamese is only ever replaced by the raw keys.
    pub fn smart_commit_english_only(&self, app_id: Option<&str>) -> String {
        use super::viet_dict;
        let raw: String = self.raw_keys.iter().collect();
        // Cheat system: field-reported false positives get forced English
        if let Some(forced) = super::cheat::should_force_english(app_id, &self.raw_keys) {
            self.note_lang(2);
            return forced;
        }
        // "thuowr" renders "thưở": ươ never ends a syllable, it is uơ.
        let display = super::rhyme::fix_open_uo(&self.display).unwrap_or_else(|| self.display.clone());
        if !self.auto_detect {
            return self.output_of(&display);
        }
        if !self.last_valid {
            self.note_lang(2); // not Vietnamese at all ("please"): English context
            return self.output_of(&display);
        }
        let changed = display != raw;
        // Digits that became marks (VNI keys, also in Tự do) never spell an
        // English word: restoring them only prints the keystrokes. A word cut
        // short or mistyped ("sẵ", "kiể", "đưồng") stays as typed in
        // Vietnamese — field 2026-10-27: "sa84", "kie63", "d9uo67n2g".
        if changed && raw.chars().any(|c| c.is_ascii_digit()) {
            return self.vietnamese(display);
        }
        // A plain ASCII word no Vietnamese syllable looks like ("take"):
        // English context too.
        if !changed && !super::rhyme::is_complete_word(&display) {
            self.note_lang(2);
            return raw;
        }
        if changed && !display.is_ascii() && !super::rhyme::is_complete_word(&display) {
            self.note_lang(2);
            return raw;
        }
        if self.method == InputMethod::Vni {
            return self.vietnamese(display);
        }
        if self.method == InputMethod::Smart && viet_dict::looks_like_non_text(&self.raw_keys) {
            return raw;
        }
        // A bare initial ("đ") is a Vietnamese prefix, never English (field
        // bug 2026-07-10: "đ" + space committed "dd").
        let has_vowel = display.chars().any(|c| super::syllable::is_vowel_char(c.to_lowercase().next().unwrap_or(c)));
        if has_vowel && super::lexicon::is_english(&raw) {
            if !changed {
                self.note_lang(1); // "code", "the": weak English evidence
                return raw;
            }
            // English keys whose rendering is no real Vietnamese word
            // (system hunspell list, lexicon.rs): English, no doubt.
            // (Not inside Vietnamese text: names like "Huế" are missing.)
            if self.lang.get() >= 0 && super::lexicon::is_vietnamese_word(&display) == Some(false) {
                self.note_lang(2);
                return raw;
            }
            // Both readings are real words: the sentence decides; with no
            // context, common English (curated list) beats rare Vietnamese
            // unless the Vietnamese reading is the common one.
            let lang = self.lang.get();
            let english = lang > 0
                || (lang == 0
                    && viet_dict::is_english_word(&self.raw_keys)
                    && !viet_dict::prefer_vietnamese(&raw));
            if english {
                self.note_lang(2);
                return raw;
            }
        }
        self.vietnamese(display)
    }

    fn vietnamese(&self, display: String) -> String {
        if !display.is_ascii() {
            self.note_lang(-2);
        }
        self.output_of(&display)
    }

    /// Forget the language context (focus left the text field — the next
    /// field's text has nothing to do with this one).
    pub fn reset_context(&mut self) {
        self.lang.set(0);
    }

    /// Nudge the language context (clamped to ±3 so it follows the text).
    fn note_lang(&self, delta: i8) {
        self.lang.set((self.lang.get() + delta).clamp(-3, 3));
    }

    fn output_of(&self, s: &str) -> String {
        match self.output_mode {
            OutputMode::UnicodeDungSan => s.to_string(),
            OutputMode::UnicodeToHop => s.nfd().collect(),
        }
    }

    /// Engine-boundary commit (Preedit path) — same decision as the
    /// NonPreedit path. The old body stripped every mark before a lookup in
    /// the incomplete syllable list and mangled real words (ấ→aas).
    pub fn smart_commit_output(&self) -> String {
        self.smart_commit_english_only(None)
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


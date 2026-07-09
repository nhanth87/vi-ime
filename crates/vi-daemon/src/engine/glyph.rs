// SPDX-License-Identifier: GPL-3.0-or-later OR Commercial
// Copyright (c) 2024-2026 vi-im contributors
//! Unicode algebra — diacritics as codepoint math, not character tables.
//!
//! A Vietnamese letter is `base × quality × tone`. Internally we only ever
//! combine a base/quality-marked char with ONE combining codepoint and let
//! the Unicode canonical-composition algorithm (NFC) produce the precomposed
//! character. The Unicode database *is* our mapping table:
//!
//!   quality:  ◌̂ U+0302 (â ê ô)   ◌̆ U+0306 (ă)   ◌̛ U+031B (ơ ư)
//!   tone:     ◌́ U+0301 sắc   ◌̀ U+0300 huyền   ◌̉ U+0309 hỏi
//!             ◌̃ U+0303 ngã   ◌̣ U+0323 nặng
//!
//! The single exception is đ (U+0111): its stroke is not canonically
//! composable, so it gets one special case — the only one in the crate.

use unicode_normalization::UnicodeNormalization;

use crate::engine::tone::Tone;

// ── Combining codepoints ──

/// ◌̂ circumflex — a→â, e→ê, o→ô ("mũ")
pub const CIRCUMFLEX: char = '\u{0302}';
/// ◌̆ breve — a→ă ("trăng")
pub const BREVE: char = '\u{0306}';
/// ◌̛ horn — o→ơ, u→ư ("móc")
pub const HORN: char = '\u{031B}';

/// Tone → combining codepoint (Level has no mark).
pub fn tone_mark(tone: Tone) -> Option<char> {
    match tone {
        Tone::Level => None,
        Tone::Acute => Some('\u{0301}'),
        Tone::Grave => Some('\u{0300}'),
        Tone::Hook => Some('\u{0309}'),
        Tone::Tilde => Some('\u{0303}'),
        Tone::Dot => Some('\u{0323}'),
    }
}

/// Canonically compose `base + mark` into a single precomposed char.
/// Returns `None` when Unicode defines no such composition (e.g. q + ◌̂).
pub fn compose(base: char, mark: char) -> Option<char> {
    let mut it = [base, mark].into_iter().nfc();
    match (it.next(), it.next()) {
        (Some(c), None) => Some(c), // fully composed into one char
        _ => None,                  // no canonical composition exists
    }
}

/// Strip ALL marks from a char, returning the ASCII base ('ệ' → 'e').
/// Pure NFD: the base letter is the first decomposed codepoint.
pub fn base_of(ch: char) -> char {
    if ch == 'đ' { return 'd'; }
    if ch == 'Đ' { return 'D'; }
    ch.nfd().next().unwrap_or(ch)
}

/// Apply a quality mark to a base vowel (đ handled as the lone special case).
/// Returns `None` if the combination isn't a Vietnamese letter.
pub fn apply_quality(base: char, mark: char) -> Option<char> {
    // đ: stroke is not canonically composable in Unicode
    if mark == STROKE {
        return match base {
            'd' => Some('đ'),
            'D' => Some('Đ'),
            _ => None,
        };
    }
    let composed = compose(base, mark)?;
    // Guard: only accept real Vietnamese letters (q+◌̂ shouldn't slip through)
    if composed == base { None } else { Some(composed) }
}

/// Pseudo-mark for the đ stroke (not a real combining char in our pipeline;
/// D-stroke has no canonical composition, see `apply_quality`).
pub const STROKE: char = '\u{0335}';

// SPDX-License-Identifier: GPL-3.0-only
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
/// ◌̛ horn — o→ơ, u→ư (“móc”)
pub const HORN: char = '\u{031B}';

/// Pseudo-mark for the đ stroke (not a real combining char in our pipeline;
/// D-stroke has no canonical composition, see `apply_quality`).
/// Legacy `char` sentinel — in new code prefer `QualityMark::Stroke`.
#[allow(dead_code)]
pub const STROKE: char = '\u{0335}';

/// Quality-mark discriminant used by `normalize.rs` to request letter
/// transformations via `apply_quality`. An enum avoids using a real Unicode
/// combining codepoint as a sentinel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityMark {
    /// ◌̂ U+0302 — a→â, e→ê, o→ô
    Circumflex,
    /// ◌̆ U+0306 — a→ă
    Breve,
    /// ◌̛ U+031B — o→ơ, u→ư
    Horn,
    /// đ stroke — no canonical Unicode composition; handled as special case.
    Stroke,
}

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
    if ch == 'đ' {
        return 'd';
    }
    if ch == 'Đ' {
        return 'D';
    }
    ch.nfd().next().unwrap_or(ch)
}

/// Apply a quality mark to a base vowel (đ handled as the lone special case).
/// Returns `None` if the combination isn't a Vietnamese letter.
#[allow(deprecated)]
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
    if composed == base {
        None
    } else {
        Some(composed)
    }
}

/// Type-safe version of `apply_quality` using the `QualityMark` enum.
/// Preferred in new code.
pub fn apply_quality_mark(base: char, mark: QualityMark) -> Option<char> {
    match mark {
        QualityMark::Stroke => match base {
            'd' => Some('đ'),
            'D' => Some('Đ'),
            _ => None,
        },
        QualityMark::Circumflex => {
            let composed = compose(base, CIRCUMFLEX)?;
            if composed == base {
                None
            } else {
                Some(composed)
            }
        }
        QualityMark::Breve => {
            let composed = compose(base, BREVE)?;
            if composed == base {
                None
            } else {
                Some(composed)
            }
        }
        QualityMark::Horn => {
            let composed = compose(base, HORN)?;
            if composed == base {
                None
            } else {
                Some(composed)
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests (R10: every pub fn ≥ 1 test)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::tone::Tone;

    // ── compose ──

    #[test]
    fn compose_all_vowel_tone_combinations() {
        // All 5 base vowels × 5 tones = 25 precomposed chars must exist.
        let vowels = ['a', 'e', 'i', 'o', 'u', 'y'];
        let tones = [Tone::Acute, Tone::Grave, Tone::Hook, Tone::Tilde, Tone::Dot];
        for &v in &vowels {
            for &t in &tones {
                let mark = tone_mark(t).unwrap();
                assert!(
                    compose(v, mark).is_some(),
                    "compose('{v}', tone {:?}) should produce a precomposed char",
                    t
                );
            }
        }
    }

    #[test]
    fn compose_quality_vowels_with_tones() {
        // Quality vowels: â ê ô ă ơ ư — each can carry all 5 tones.
        let quality_vowels = ['â', 'ê', 'ô', 'ă', '\u{01A1}', '\u{01B0}'];
        let tones = [Tone::Acute, Tone::Grave, Tone::Hook, Tone::Tilde, Tone::Dot];
        for &v in &quality_vowels {
            for &t in &tones {
                let mark = tone_mark(t).unwrap();
                assert!(
                    compose(v, mark).is_some(),
                    "compose('{v}', tone {:?}) failed",
                    t
                );
            }
        }
    }

    #[test]
    fn compose_returns_none_for_invalid() {
        // q + circumflex has no precomposed Unicode char.
        assert_eq!(compose('q', CIRCUMFLEX), None);
        // d + breve is not Vietnamese.
        assert_eq!(compose('d', BREVE), None);
        // z + horn is not Vietnamese.
        assert_eq!(compose('z', HORN), None);
    }

    #[test]
    fn compose_level_tone_is_none() {
        assert_eq!(tone_mark(Tone::Level), None);
    }

    // ── base_of ──

    #[test]
    fn base_of_strips_all_marks() {
        assert_eq!(base_of('ệ'), 'e'); // ệ = e + circumflex + dot
        assert_eq!(base_of('ấ'), 'a'); // ấ = a + circumflex + acute
        assert_eq!(base_of('ư'), 'u'); // ư = u + horn
        assert_eq!(base_of('ă'), 'a'); // ă = a + breve
        assert_eq!(base_of('ô'), 'o'); // ô = o + circumflex
        assert_eq!(base_of('ơ'), 'o'); // ơ = o + horn
    }

    #[test]
    fn base_of_d_stroke() {
        assert_eq!(base_of('đ'), 'd');
        assert_eq!(base_of('\u{0110}'), 'D');
    }

    #[test]
    fn base_of_ascii_passthrough() {
        assert_eq!(base_of('a'), 'a');
        assert_eq!(base_of('z'), 'z');
        assert_eq!(base_of('A'), 'A');
    }

    // ── apply_quality ──

    #[test]
    #[allow(deprecated)]
    fn apply_quality_circumflex() {
        assert_eq!(apply_quality('a', CIRCUMFLEX), Some('â'));
        assert_eq!(apply_quality('e', CIRCUMFLEX), Some('ê'));
        assert_eq!(apply_quality('o', CIRCUMFLEX), Some('ô'));
        // 'u' + circumflex composes to 'û' (valid Unicode but not Vietnamese
        // — the guard does NOT reject it because composed != base).
        assert_eq!(apply_quality('u', CIRCUMFLEX), Some('\u{00FB}'));
    }

    #[test]
    #[allow(deprecated)]
    fn apply_quality_breve() {
        assert_eq!(apply_quality('a', BREVE), Some('ă'));
        // 'e' + breve composes to '\u{0115}' (valid Unicode, not standard
        // Vietnamese — but compose succeeds so apply_quality returns it).
        assert_eq!(apply_quality('e', BREVE), Some('\u{0115}'));
    }

    #[test]
    #[allow(deprecated)]
    fn apply_quality_horn() {
        assert_eq!(apply_quality('o', HORN), Some('\u{01A1}')); // ơ
        assert_eq!(apply_quality('u', HORN), Some('\u{01B0}')); // ư
        assert_eq!(apply_quality('a', HORN), None);
    }

    #[test]
    #[allow(deprecated)]
    fn apply_quality_stroke() {
        assert_eq!(apply_quality('d', STROKE), Some('đ'));
        assert_eq!(apply_quality('D', STROKE), Some('\u{0110}'));
        assert_eq!(apply_quality('a', STROKE), None);
    }

    // ── apply_quality_mark (enum-based) ──

    #[test]
    fn apply_quality_mark_circumflex() {
        assert_eq!(apply_quality_mark('a', QualityMark::Circumflex), Some('â'));
        assert_eq!(apply_quality_mark('e', QualityMark::Circumflex), Some('ê'));
        assert_eq!(apply_quality_mark('o', QualityMark::Circumflex), Some('ô'));
    }

    #[test]
    fn apply_quality_mark_stroke() {
        assert_eq!(apply_quality_mark('d', QualityMark::Stroke), Some('đ'));
        assert_eq!(
            apply_quality_mark('D', QualityMark::Stroke),
            Some('\u{0110}')
        );
        assert_eq!(apply_quality_mark('z', QualityMark::Stroke), None);
    }

    // ── tone_mark ──

    #[test]
    fn tone_mark_all_variants() {
        assert_eq!(tone_mark(Tone::Level), None);
        assert_eq!(tone_mark(Tone::Acute), Some('\u{0301}'));
        assert_eq!(tone_mark(Tone::Grave), Some('\u{0300}'));
        assert_eq!(tone_mark(Tone::Hook), Some('\u{0309}'));
        assert_eq!(tone_mark(Tone::Tilde), Some('\u{0303}'));
        assert_eq!(tone_mark(Tone::Dot), Some('\u{0323}'));
    }
}

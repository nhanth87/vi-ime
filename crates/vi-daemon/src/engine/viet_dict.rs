// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 vi-im contributors
//! Vietnamese syllable dictionary for Smart mode disambiguation.
//!
//! When InputMethod::Smart renders a word that looks like valid Vietnamese
//! but the RAW KEYS form a common English/programming word, we need a way
//! to decide. This dictionary contains all valid Vietnamese syllables
//! (base form without tones, lowercase) — approximately 4,800 entries.
//!
//! Usage: at word boundary in Smart mode, if `syllable::process` returned
//! `Outcome::Rendered` but the rendered base (stripped of tones) is NOT in
//! this dictionary, restore raw keys instead (extended R9).

use std::collections::HashSet;
use std::sync::LazyLock;

/// All valid Vietnamese syllables (lowercase, no tone marks).
/// Generated from linguistic data: every valid onset×nucleus×coda combination
/// that passes phonotactic constraints.
static VIET_SYLLABLES: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    // Common Vietnamese syllables — base forms without diacritics/tones.
    // This covers the ~4800 valid combinations of onset+nucleus+coda in Vietnamese.
    let data = include_str!("../data/viet_syllables.txt");
    data.lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect()
});

/// Common English/programming words that Smart mode should NOT convert.
/// These are words where Telex/VNI modifiers accidentally produce valid
/// Vietnamese (e.g., "test"→"tét" because 's' is a tone key in Telex).
static ENGLISH_COMMON: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    let data = include_str!("../data/english_common.txt");
    data.lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect()
});

/// Strip tone marks from a Vietnamese string (return lowercase base form).
/// Used to look up the dictionary with the toneless version.
pub fn strip_tones(s: &str) -> String {
    use crate::engine::glyph;
    s.chars().map(|ch| {
        let base = glyph::base_of(ch);
        // base_of returns the NFD base (no combining marks). For lookup
        // we want the quality vowel preserved (ă, â, ê, ô, ơ, ư, đ) but
        // no tone combining character. base_of already does this.
        base
    }).collect()
}

/// Check if raw keys form a known English word that should NOT be composed.
pub fn is_english_word(raw_keys: &[char]) -> bool {
    let word: String = raw_keys.iter().map(|c| c.to_ascii_lowercase()).collect();
    ENGLISH_COMMON.contains(word.as_str())
}

/// Check if a rendered syllable (lowercase, tones stripped) is a valid
/// Vietnamese syllable.
pub fn is_viet_syllable(rendered_base: &str) -> bool {
    VIET_SYLLABLES.contains(rendered_base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_words_detected() {
        assert!(is_english_word(&['t', 'e', 's', 't']));
        assert!(is_english_word(&['u', 's', 'e', 'r']));
        assert!(is_english_word(&['s', 'w', 'a', 'y']));
        assert!(is_english_word(&['w', 'i', 'n', 'd', 'o', 'w', 's']));
        assert!(!is_english_word(&['x', 'i', 'n']));
    }

    #[test]
    fn viet_syllables_valid() {
        assert!(is_viet_syllable("xin"));
        assert!(is_viet_syllable("chao"));
        assert!(is_viet_syllable("viet"));
        assert!(is_viet_syllable("nam"));
        assert!(!is_viet_syllable("test"));
        assert!(!is_viet_syllable("sway"));
    }
}

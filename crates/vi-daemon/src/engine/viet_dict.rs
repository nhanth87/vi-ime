// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
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

/// Heuristic: the raw keys look like a URL, file path, or code token —
/// NOT natural-language text that should be composed as Vietnamese.
/// Used to suppress Telex/VNI composition on browser address bars,
/// terminal commands, and code editors where ContentType signals are
/// absent (evdev fallback).
pub fn looks_like_non_text(raw_keys: &[char]) -> bool {
    let s: String = raw_keys.iter().collect();
    let lower = s.to_lowercase();
    // URL patterns
    if lower.contains("://")
        || lower.contains("http")
        || lower.contains("www.")
        || lower.ends_with(".com")
        || lower.ends_with(".org")
        || lower.ends_with(".net")
        || lower.ends_with(".dev")
        || lower.ends_with(".io")
        || lower.ends_with(".vn")
        || lower.contains(".com/")
    {
        return true;
    }
    // Unix/terminal patterns
    if lower.starts_with("ssh ")
        || lower.starts_with("git ")
        || lower.starts_with("sudo ")
        || lower.starts_with("docker ")
        || lower.starts_with("pip ")
        || lower.starts_with("npm ")
        || lower.starts_with("cargo ")
        || lower.starts_with("./")
        || lower.starts_with("/usr/")
        || lower.starts_with("/home/")
        || lower.starts_with("/etc/")
        || lower.starts_with("cd ")
        || lower.starts_with("ls ")
        || lower.starts_with("rm ")
        || lower.starts_with("cp ")
        || lower.starts_with("mv ")
        || lower.starts_with("cat ")
        || lower.starts_with("echo ")
    {
        return true;
    }
    // Code/identifier patterns
    if lower.starts_with("def ")
        || lower.starts_with("fn ")
        || lower.starts_with("class ")
        || lower.starts_with("import ")
        || lower.starts_with("from ")
        || lower.starts_with("const ")
        || lower.starts_with("let ")
        || lower.starts_with("var ")
        || lower.starts_with("pub ")
    {
        return true;
    }
    false
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

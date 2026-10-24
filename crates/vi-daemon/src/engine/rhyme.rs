// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Commit-time check: is a rendered word a COMPLETE, well-formed Vietnamese
//! syllable? Used to restore the raw keys when Telex/VNI composition turned
//! an English word into something no Vietnamese word looks like
//! ("text" → "tẽt", "are" → "ảe", "our" → "ỏu", "were" → "ưể").
//!
//! Only applied at a word boundary — mid-word prefixes ("ngưo" on the way to
//! "người") are never judged. Structural rules over the finite inventory of
//! Vietnamese rhymes (onset + nucleus + coda + tone), not a word list:
//! `viet_syllables.txt` is incomplete and would reject real words.

use unicode_normalization::UnicodeNormalization;

use crate::engine::syllable::{CODAS, INITIALS};

/// Every Vietnamese nucleus (vowel cluster), toneless.
const NUCLEI: &[&str] = &[
    "a", "ă", "â", "e", "ê", "i", "o", "ô", "ơ", "u", "ư", "y",
    "ai", "ao", "au", "ay", "âu", "ây", "eo", "êu", "ia", "iê", "iu", "oa", "oă", "oe", "oi",
    "ôi", "ơi", "oo", "ua", "uâ", "uê", "ui", "uô", "uơ", "ưa", "ưi", "ưu", "ươ", "uy", "yê",
    "iêu", "oai", "oao", "oay", "oeo", "uây", "uôi", "uya", "uyê", "uyu", "ươi", "ươu", "yêu",
];

/// Nuclei ending in an off-glide (i/y/o/u) or the open "ia/ua/ưa": no coda.
const OPEN_ONLY: &[&str] = &[
    "ai", "ao", "au", "ay", "âu", "ây", "eo", "êu", "ia", "iu", "oi", "ôi", "ơi", "ua", "ui",
    "ưa", "ưi", "ưu", "iêu", "oai", "oao", "oay", "oeo", "uây", "uôi", "uya", "uyu", "ươi",
    "ươu", "yêu",
];

/// Nuclei that cannot end a syllable: they need a coda ("ăn", "tiên").
const NEEDS_CODA: &[&str] = &["ă", "â", "iê", "yê", "uô", "ươ", "uyê", "oă", "uâ"];

const TONE_MARKS: [char; 5] = ['\u{300}', '\u{301}', '\u{303}', '\u{309}', '\u{323}'];

fn is_vowel(c: char) -> bool {
    matches!(c, 'a' | 'ă' | 'â' | 'e' | 'ê' | 'i' | 'o' | 'ô' | 'ơ' | 'u' | 'ư' | 'y')
}

/// True when `word` (one rendered syllable, any case) is well-formed
/// Vietnamese. Pure ASCII without tone is not this module's business (true).
pub(crate) fn is_complete_word(word: &str) -> bool {
    let lower = word.to_lowercase();
    let mut tone = None;
    let mut base = String::new();
    for c in lower.nfd() {
        if TONE_MARKS.contains(&c) {
            if tone.replace(c).is_some() {
                return false; // two tone marks
            }
        } else {
            base.push(c);
        }
    }
    let base: Vec<char> = base.nfc().collect();
    if base.iter().any(|c| matches!(c, 'f' | 'j' | 'w' | 'z')) || base.iter().any(|c| !c.is_alphabetic()) {
        return false;
    }
    // Bare initial ("đ"): a prefix the user chose to commit — keep it.
    if INITIALS.iter().any(|i| i.chars().eq(base.iter().copied())) {
        return true;
    }
    // Longest onset first; "gi"+"n" must fall back to "g"+"in".
    let mut onsets: Vec<&str> = INITIALS.iter().copied().filter(|i| starts_with(&base, i)).collect();
    onsets.sort_by_key(|i| std::cmp::Reverse(i.chars().count()));
    onsets.push("");
    onsets.into_iter().any(|onset| rest_ok(onset, &base[onset.chars().count()..], tone))
}

fn rest_ok(onset: &str, rest: &[char], tone: Option<char>) -> bool {
    let vlen = rest.iter().take_while(|c| is_vowel(**c)).count();
    if vlen == 0 {
        return false;
    }
    let nucleus: String = rest[..vlen].iter().collect();
    let coda: String = rest[vlen..].iter().collect();
    if !NUCLEI.contains(&nucleus.as_str()) {
        return false;
    }
    if !coda.is_empty() && !CODAS.contains(&coda.as_str()) {
        return false;
    }
    if !coda.is_empty() && OPEN_ONLY.contains(&nucleus.as_str()) {
        return false;
    }
    // Only with an onset: a bare "ấ"/"ầ" is a real interjection (field bug
    // 2026-07-12: "ấ" committed as "aas").
    if coda.is_empty() && !onset.is_empty() && NEEDS_CODA.contains(&nucleus.as_str()) {
        return false;
    }
    // Stop codas take only sắc or nặng (a missing tone is left alone: "đat"
    // is a forgotten tone, not English).
    if matches!(coda.as_str(), "c" | "ch" | "p" | "t") && matches!(tone, Some('\u{300}' | '\u{303}' | '\u{309}')) {
        return false;
    }
    // -ch / -nh follow a, ê, i, y only (sách, lệnh, kịch, huỳnh).
    let last = rest[vlen - 1];
    if matches!(coda.as_str(), "ch" | "nh") && !matches!(last, 'a' | 'ê' | 'i' | 'y') {
        return false;
    }
    let first = rest[0];
    let front = matches!(first, 'i' | 'e' | 'ê' | 'y');
    match onset {
        "c" => !front,
        "k" => front,
        "gh" | "ngh" => front,
        "q" => false, // q only as "qu"
        _ => true,
    }
}

fn starts_with(chars: &[char], pat: &str) -> bool {
    let mut it = chars.iter();
    pat.chars().all(|pc| it.next() == Some(&pc))
}

/// "ươ" never ends a syllable — with no coda the word is "uơ" (thuở, huơ,
/// quở): Telex "thuowr" / VNI "thuo73" horn both vowels via the uo pair
/// rule. Commit-time only, so "người" never flickers u↔ư while typing.
pub(crate) fn fix_open_uo(word: &str) -> Option<String> {
    let chars: Vec<char> = word.chars().collect();
    let n = chars.len();
    if n < 2 {
        return None;
    }
    let horn_o = |c: char| matches!(c.to_lowercase().next(), Some('ơ' | 'ờ' | 'ớ' | 'ở' | 'ỡ' | 'ợ'));
    let i = n - 2;
    if !(chars[i] == 'ư' || chars[i] == 'Ư') || !horn_o(chars[n - 1]) {
        return None;
    }
    let mut out = chars;
    out[i] = if out[i] == 'Ư' { 'U' } else { 'u' };
    Some(out.into_iter().collect())
}

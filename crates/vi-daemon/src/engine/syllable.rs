// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Unified Vietnamese syllable engine — NFD/Unicode-math, table-free.
//!
//! One path for Telex and VNI. No char→char vowel map, no enumerated
//! cluster table: a syllable is decomposed structurally (onset = leading
//! consonants, nucleus = vowel run, coda = trailing consonants), validated by
//! phonotactic predicates, and every diacritic comes from Unicode canonical
//! composition (`glyph`). Tone placement is a pure algorithm (see
//! [`tone_index`]), not offset data.
//!
//! Pipeline: raw keys → `normalize` (modifiers + unified undo) → decompose →
//! tone placement → NFC compose → String.

use crate::engine::glyph;
use crate::engine::normalize;
use crate::engine::style::ToneStyle;
use crate::engine::tone::Tone;
use crate::engine::types::InputMethod;

/// Vietnamese initial consonants (âm đầu), longest-first for greedy matching
/// with backtracking — "gi"/"qu" fall back to "g"/"q" when the remainder
/// leaves no vowel ("gì" = g+i, but "già" = gi+a). Category list, not a map.
const INITIALS: &[&str] = &[
    "ngh", "ng", "gh", "gi", "kh", "nh", "ph", "qu", "th", "tr", "ch", "b", "c", "d", "đ", "g",
    "h", "k", "l", "m", "n", "p", "q", "r", "s", "t", "v", "x",
];

/// Vietnamese codas (âm cuối). Category list, not a vowel map.
const CODAS: &[&str] = &["ng", "nh", "ch", "c", "m", "n", "p", "t"];

/// Glide+vowel rising diphthongs whose tone placement depends on style
/// (Classic → glide, Modern → main vowel): hòa/hoà, khỏe/khoẻ, thúy/thuý.
const STYLE_DIPHTHONGS: &[&str] = &["oa", "oe", "uy"];

/// Case pattern detected from the raw keys, preserved through rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseHint {
    Lower,
    Upper,
    Capitalized,
}

/// What the engine should display/commit for the current word.
pub enum Outcome {
    /// A rendered string (valid syllable, or a user-forced literal via undo).
    Rendered(String),
    /// Not a Vietnamese syllable → caller shows the raw keys verbatim (R9).
    Raw,
}

/// A structurally-decomposed syllable.
struct Syllable {
    onset: &'static str,
    nucleus: Vec<char>,
    coda: &'static str,
}

/// Is `ch` a Vietnamese vowel? Uses NFD base, so â/ê/ô/ơ/ư/ă count; đ does not.
pub fn is_vowel_char(ch: char) -> bool {
    matches!(glyph::base_of(ch), 'a' | 'e' | 'i' | 'o' | 'u' | 'y')
}

/// A quality-marked vowel carries the tone in a diphthong (â/ê/ô/ơ/ư/ă).
fn is_quality_vowel(ch: char) -> bool {
    matches!(ch, 'ă' | 'â' | 'ê' | 'ô' | 'ơ' | 'ư')
}

/// Word boundary chars end composition.
/// Digits are boundaries ONLY in Telex. In VNI and Smart (Tự do),
/// digits are tone marks — the engine decides whether to use them.
pub fn is_word_boundary(ch: char, method: InputMethod) -> bool {
    if ch.is_ascii_whitespace() || ch.is_ascii_punctuation() || ch.is_ascii_control() {
        return true;
    }
    if ch.is_ascii_digit() {
        // Only Telex treats digits as boundaries. VNI and Smart use
        // digits as tone modifiers (1-6 = tone marks).
        return matches!(method, InputMethod::Telex);
    }
    !ch.is_ascii() && !is_vietnamese_char(ch)
}

fn is_vietnamese_char(ch: char) -> bool {
    matches!(ch,
        'a'..='z' | 'A'..='Z'
        | '\u{00C0}'..='\u{00FF}'
        | 'Ă' | 'ă' | 'Đ' | 'đ' | 'Ĩ' | 'ĩ' | 'Ũ' | 'ũ' | 'Ơ' | 'ơ' | 'Ư' | 'ư'
        | '\u{1EA0}'..='\u{1EF9}'
    )
}

/// Process one word's raw keystrokes into a display string (the single path
/// for every input method).
pub fn process(raw_keys: &[char], method: InputMethod, style: ToneStyle) -> Outcome {
    if raw_keys.is_empty() {
        return Outcome::Raw;
    }
    let case = detect_case(raw_keys);
    let lower: Vec<char> = raw_keys.iter().map(|c| c.to_ascii_lowercase()).collect();

    let norm = normalize::normalize(&lower, method);
    if norm.undo {
        // User explicitly undid a modifier (ass→as): display the normalized
        // chars literally, never as Vietnamese.
        return Outcome::Rendered(render_literal(&norm.chars, case));
    }

    match decompose(&norm.chars) {
        Some(syl) => Outcome::Rendered(render(&syl, norm.tone, style, case)),
        // Onset-only word: "đ" (dd/d9) has no nucleus yet, so decompose
        // fails — but every char forms a valid initial, i.e. a PREFIX of a
        // Vietnamese syllable. Render the normalized form ("đ"), don't
        // R9-restore the raw keys ("dd"/"d9") — field bug 2026-07-10:
        // typing "đ" + space committed "dd". Real English words are never
        // a bare Vietnamese onset with a consumed modifier, so R9 restore
        // (windows→windows) is unaffected.
        None if is_onset_only(&norm.chars) => Outcome::Rendered(render_literal(&norm.chars, case)),
        None => Outcome::Raw,
    }
}

/// Do these chars form exactly one valid Vietnamese initial ("đ", "ngh"…)?
fn is_onset_only(chars: &[char]) -> bool {
    INITIALS
        .iter()
        .any(|init| init.chars().count() == chars.len() && starts_with(chars, init))
}

// ── Decomposition (structural, table-free) ────────────────────────────────

/// Decompose into {onset, nucleus, coda}, longest valid initial first with
/// backtracking. Every char must be consumed.
fn decompose(chars: &[char]) -> Option<Syllable> {
    if chars.is_empty() {
        return None;
    }
    for onset in initial_candidates(chars) {
        let after = &chars[onset.chars().count()..];
        let vlen = after.iter().take_while(|c| is_vowel_char(**c)).count();
        // Nucleus must contain 1..=3 vowels (Vietnamese max triphthong).
        if vlen == 0 || vlen > 3 {
            continue;
        }
        let nucleus = after[..vlen].to_vec();
        let tail = &after[vlen..];
        let coda = match match_coda(tail) {
            Some(c) => c,
            None => continue,
        };
        let syl = Syllable {
            onset,
            nucleus,
            coda,
        };
        if is_valid(&syl) {
            return Some(syl);
        }
    }
    None
}

/// All initials that prefix `chars`, longest first, then the empty initial.
fn initial_candidates(chars: &[char]) -> impl Iterator<Item = &'static str> + '_ {
    INITIALS
        .iter()
        .copied()
        .filter(move |init| starts_with(chars, init))
        .chain(std::iter::once(""))
}

/// The coda must consume the ENTIRE tail (or be empty).
fn match_coda(tail: &[char]) -> Option<&'static str> {
    if tail.is_empty() {
        return Some("");
    }
    CODAS
        .iter()
        .copied()
        .find(|coda| coda.chars().count() == tail.len() && starts_with(tail, coda))
}

fn starts_with(chars: &[char], pat: &str) -> bool {
    let mut it = chars.iter();
    pat.chars().all(|pc| it.next() == Some(&pc))
}

/// Phonotactic predicates the structural pass can't catch (glide duplicates).
fn is_valid(syl: &Syllable) -> bool {
    let nuc0 = syl.nucleus[0];
    // qu carries the /w/ glide → reject a duplicate glide vowel (u/o).
    if syl.onset == "qu" && (nuc0 == 'u' || nuc0 == 'o') {
        return false;
    }
    // gi carries /j/ (spelled "i") → "gii" is a duplicate.
    !(syl.onset == "gi" && syl.nucleus.len() == 1 && nuc0 == 'i')
}

// ── Tone placement (pure algorithm — no offset table) ─────────────────────

/// Which index within the nucleus carries the tone. 1 vowel → on it; any coda
/// → last vowel; triphthong (no coda) → middle vowel; diphthong (no coda) →
/// the quality vowel if present, else oa/oe/uy split by style (Classic=glide,
/// Modern=main), else the first vowel.
fn tone_index(nucleus: &[char], has_coda: bool, style: ToneStyle) -> usize {
    let n = nucleus.len();
    if n <= 1 {
        return 0;
    }
    if has_coda {
        return n - 1;
    }
    if n == 3 {
        return 1;
    }
    if let Some(qi) = nucleus.iter().rposition(|&c| is_quality_vowel(c)) {
        return qi;
    }
    let pair: String = nucleus.iter().collect();
    if STYLE_DIPHTHONGS.contains(&pair.as_str()) {
        return match style {
            ToneStyle::Classic => 0,
            ToneStyle::Modern => 1,
        };
    }
    0
}

// ── Rendering (case + Unicode composition) ────────────────────────────────
fn render(syl: &Syllable, tone: Tone, style: ToneStyle, case: CaseHint) -> String {
    let ti = tone_index(&syl.nucleus, !syl.coda.is_empty(), style);
    let mut out = String::with_capacity(12);
    let mut pos = 0;
    for ch in syl.onset.chars() {
        push_cased(&mut out, ch, case, pos);
        pos += 1;
    }
    for (i, &v) in syl.nucleus.iter().enumerate() {
        let v = if i == ti { toned(v, tone) } else { v };
        push_cased(&mut out, v, case, pos);
        pos += 1;
    }
    for ch in syl.coda.chars() {
        push_cased(&mut out, ch, case, pos);
        pos += 1;
    }
    out
}

/// Render normalized chars verbatim (undo/literal path) with case restored.
fn render_literal(chars: &[char], case: CaseHint) -> String {
    let mut out = String::with_capacity(chars.len());
    for (i, &ch) in chars.iter().enumerate() {
        push_cased(&mut out, ch, case, i);
    }
    out
}

/// Apply a tone mark to one vowel via NFC — no lookup table.
fn toned(ch: char, tone: Tone) -> char {
    match glyph::tone_mark(tone) {
        None => ch,
        Some(mark) => glyph::compose(ch, mark).unwrap_or_else(|| {
            debug_assert!(
                false,
                "[GLYPH] compose('{ch}', U+{:04X}) failed — non-vowel in nucleus?",
                mark as u32
            );
            ch
        }),
    }
}

fn push_cased(out: &mut String, ch: char, case: CaseHint, pos: usize) {
    let upper = match case {
        CaseHint::Lower => false,
        CaseHint::Upper => true,
        CaseHint::Capitalized => pos == 0,
    };
    if upper {
        out.extend(ch.to_uppercase());
    } else {
        out.push(ch);
    }
}

/// Detect the case pattern from raw keys (before lowercasing).
fn detect_case(raw_keys: &[char]) -> CaseHint {
    let mut has_lower = false;
    let mut has_upper = false;
    let mut first_is_upper = false;
    for (i, &ch) in raw_keys.iter().enumerate() {
        if ch.is_ascii_uppercase() {
            has_upper = true;
            if i == 0 {
                first_is_upper = true;
            }
        } else if ch.is_ascii_lowercase() {
            has_lower = true;
        }
    }
    match (has_lower, has_upper) {
        (false, true) => CaseHint::Upper,
        (true, true) if first_is_upper => CaseHint::Capitalized,
        _ => CaseHint::Lower,
    }
}

// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Normalization pass: raw keystrokes → quality-marked chars + syllable tone.
//!
//! Handles Telex/VNI modifier semantics with a UNIFIED undo mechanism:
//! pressing the same modifier twice reverts it and emits the key literally,
//! after which the word enters literal mode (no further modifiers):
//! "ass"→"as", "ddd"→"dd", "uww"→"uw", "a66"→"a6".

use crate::engine::glyph;
use crate::engine::syllable::is_vowel_char;
use crate::engine::tone::Tone;
use crate::engine::types::InputMethod;

/// Result of the normalization pass.
pub struct NormResult {
    /// Quality-marked lowercase chars (â, ư, đ... — tone NOT applied).
    pub chars: Vec<char>,
    /// Syllable tone extracted from tone keys.
    pub tone: Tone,
    /// An explicit undo happened → the word is user-forced literal:
    /// on parse failure display `chars`, NOT the raw keys.
    pub undo: bool,
}

/// Last modifier applied, for the double-key undo rule.
#[derive(Debug)]
enum LastMod {
    None,
    /// Tone key (s/f/r/x/j, 1-5, z) that set the current tone.
    Tone(char),
    /// A merge rewrote `out[start..start+len]`; pressing `key` again
    /// (with nothing typed in between) reverts that span to `revert`.
    Merge {
        start: usize,
        len: usize,
        key: char,
        revert: &'static [char],
    },
}

pub fn normalize(lower_keys: &[char], method: InputMethod) -> NormResult {
    tracing::debug!("[NORMALIZE] input: {:?}, method: {:?}", lower_keys, method);
    let mut out: Vec<char> = Vec::with_capacity(lower_keys.len());
    let mut tone = Tone::Level;
    let mut last = LastMod::None;
    let mut undo = false;
    let mut has_vowel = false;

    for &ch in lower_keys {
        tracing::debug!(
            "[NORMALIZE] processing ch='{}' has_vowel={} last={:?} out={:?}",
            ch,
            has_vowel,
            last,
            out
        );
        if undo {
            out.push(ch); // literal mode after an explicit undo
            continue;
        }

        // ── Tone keys ──
        if let Some(t) = tone_for_key(ch, method, has_vowel) {
            if let LastMod::Tone(k) = last
                && k == ch
            {
                // Same tone key twice → cancel tone, emit literal, go literal
                tone = Tone::Level;
                out.push(ch);
                undo = true;
                last = LastMod::None;
                continue;
            }
            tone = t;
            last = LastMod::Tone(ch);
            continue;
        }

        // ── 'z' removes tone (Telex + Tự do) ──
        if ch == 'z'
            && matches!(method, InputMethod::Telex | InputMethod::Smart)
            && tone != Tone::Level
        {
            tone = Tone::Level;
            last = LastMod::Tone('z');
            continue;
        }

        // ── Undo check for merges (aaa→aa, ddd→dd, uww→uw, ww→w) ──
        // Model: a merge turned span S + modifier key k into M. Pressing k
        // again undoes it: restore S, then k lands literally.
        //   dd→đ, +d  → "d"+"d" = dd      uw→ư, +w → "u"+"w" = uw
        //   uo+w→ươ, +w → "uo"+"w" = uow  w→ư, +w  → ""+"w"  = w
        if let LastMod::Merge {
            start,
            len,
            key,
            revert,
        } = last
            && ch == key
        {
            out.splice(start..start + len, revert.iter().copied());
            out.push(ch);
            undo = true;
            last = LastMod::None;
            continue;
        }

        // ── Telex doubling: aa→â, ee→ê, oo→ô, dd→đ (Telex + Tự do) ──
        if matches!(method, InputMethod::Telex | InputMethod::Smart) {
            if let Some(merged) = doubling(ch)
                && out.last() == Some(&ch)
            {
                let start = out.len() - 1;
                out[start] = merged;
                last = LastMod::Merge {
                    start,
                    len: 1,
                    key: ch,
                    revert: revert_single(ch),
                };
                has_vowel |= is_vowel_char(merged);
                continue;
            }

            // ── w-modifier: scan back for target (uow→ươ, thuongw→thương) ──
            if ch == 'w' {
                if let Some(idx) = find_w_target(&out) {
                    let horned = w_modify(out[idx]).unwrap_or(out[idx]);
                    out[idx] = horned;
                    // uo→ươ pair rule: horn the preceding 'u' as well
                    if horned == 'ơ' && idx > 0 && out[idx - 1] == 'u' {
                        out[idx - 1] = 'ư';
                        last = LastMod::Merge {
                            start: idx - 1,
                            len: 2,
                            key: 'w',
                            revert: &['u', 'o'],
                        };
                    } else {
                        last = LastMod::Merge {
                            start: idx,
                            len: 1,
                            key: 'w',
                            revert: revert_single_w(horned),
                        };
                    }
                    has_vowel = true;
                    continue;
                }
                // Standalone w → ư (span S is empty: undo yields just "w")
                out.push('ư');
                last = LastMod::Merge {
                    start: out.len() - 1,
                    len: 1,
                    key: 'w',
                    revert: &[],
                };
                has_vowel = true;
                continue;
            }
        }

        // ── VNI quality digits 6/7/8/9: scan back for target (VNI + Tự do) ──
        if matches!(method, InputMethod::Vni | InputMethod::Smart) && matches!(ch, '6'..='9') {
            if let Some((idx, modified, revert)) = vni_target(&out, ch) {
                out[idx] = modified;
                // uo→ươ pair rule (same as Telex w): "duo7ng" → "dương"
                if modified == 'ơ' && idx > 0 && out[idx - 1] == 'u' {
                    out[idx - 1] = 'ư';
                    last = LastMod::Merge {
                        start: idx - 1,
                        len: 2,
                        key: ch,
                        revert: &['u', 'o'],
                    };
                } else {
                    last = LastMod::Merge {
                        start: idx,
                        len: 1,
                        key: ch,
                        revert,
                    };
                }
                continue;
            }
            // No target → literal digit
            out.push(ch);
            last = LastMod::None;
            continue;
        }

        // ── Plain char ──
        out.push(ch);
        has_vowel |= is_vowel_char(ch);
        if !matches!(last, LastMod::Tone(_)) {
            last = LastMod::None;
        }
    }

    NormResult {
        chars: out,
        tone,
        undo,
    }
}

fn telex_tone(ch: char) -> Option<Tone> {
    match ch {
        's' => Some(Tone::Acute),
        'f' => Some(Tone::Grave),
        'r' => Some(Tone::Hook),
        'x' => Some(Tone::Tilde),
        'j' => Some(Tone::Dot),
        _ => None,
    }
}

fn vni_tone(ch: char) -> Option<Tone> {
    match ch {
        '1' => Some(Tone::Acute),
        '2' => Some(Tone::Grave),
        '3' => Some(Tone::Hook),
        '4' => Some(Tone::Tilde),
        '5' => Some(Tone::Dot),
        _ => None,
    }
}

fn tone_for_key(ch: char, method: InputMethod, has_vowel: bool) -> Option<Tone> {
    if !has_vowel {
        tracing::debug!(
            "[NORMALIZE] tone_for_key: ch={:?} method={:?} has_vowel=false -> rejecting (no vowel yet)",
            ch,
            method
        );
        return None;
    }
    // Smart (Tự do): accept tones from BOTH VNI digits AND Telex modifiers.
    // (The old version matched Smart together with Telex only — VNI digits
    // never toned in "Tự do".)
    let result = match method {
        InputMethod::Telex => telex_tone(ch),
        InputMethod::Vni => vni_tone(ch),
        InputMethod::Smart => telex_tone(ch).or_else(|| vni_tone(ch)),
    };
    tracing::debug!(
        "[NORMALIZE] tone_for_key: ch={:?} method={:?} has_vowel=true -> {:?}",
        ch,
        method,
        result
    );
    result
}

/// Telex doubling: aa→â, ee→ê, oo→ô (circumflex algebra), dd→đ (stroke).
fn doubling(ch: char) -> Option<char> {
    match ch {
        'a' | 'e' | 'o' => glyph::apply_quality(ch, glyph::CIRCUMFLEX),
        'd' => glyph::apply_quality(ch, glyph::STROKE),
        _ => None,
    }
}

/// Telex w-modifier: a→ă (breve), o→ơ, u→ư (horn algebra).
fn w_modify(ch: char) -> Option<char> {
    match ch {
        'a' => glyph::apply_quality(ch, glyph::BREVE),
        'o' | 'u' => glyph::apply_quality(ch, glyph::HORN),
        _ => None,
    }
}

/// Find the rightmost char that `w` can modify (a/o/u), scanning back over
/// consonants so "thuongw" works. Stops at an already-modified vowel.
fn find_w_target(out: &[char]) -> Option<usize> {
    out.iter().rposition(|&c| w_modify(c).is_some())
}

/// Find the rightmost target for a VNI quality digit, scanning back.
/// Digit → quality mark: 6=◌̂, 7=◌̛, 8=◌̆, 9=stroke — pure algebra.
fn vni_target(out: &[char], digit: char) -> Option<(usize, char, &'static [char])> {
    let mark = match digit {
        '6' => glyph::CIRCUMFLEX,
        '7' => glyph::HORN,
        '8' => glyph::BREVE,
        '9' => glyph::STROKE,
        _ => return None,
    };
    // 6/7/8 only target the right vowels; the algebra itself rejects
    // impossible pairs (compose returns None), so just scan.
    for idx in (0..out.len()).rev() {
        let c = out[idx];
        // Restrict to sensible bases so e.g. '7' doesn't skip past a valid
        // target hunting for another
        let applicable = matches!(
            (digit, c),
            ('6', 'a' | 'e' | 'o') | ('7', 'o' | 'u') | ('8', 'a') | ('9', 'd')
        );
        if !applicable {
            continue;
        }
        if let Some(modified) = glyph::apply_quality(c, mark) {
            return Some((idx, modified, revert_vni(c)));
        }
    }
    None
}

/// Revert span for doubling merges: the span S is the single base char
/// (the trigger key is re-emitted separately on undo).
fn revert_single(ch: char) -> &'static [char] {
    match ch {
        'a' => &['a'],
        'e' => &['e'],
        'o' => &['o'],
        'd' => &['d'],
        _ => &[],
    }
}

/// Revert span for w-modified chars: NFD strips the mark → ASCII base.
fn revert_single_w(ch: char) -> &'static [char] {
    let slice = ascii_slice(glyph::base_of(ch));
    debug_assert!(
        !slice.is_empty(),
        "[NORMALIZE] revert_single_w('{ch}') got empty slice — unexpected base '{}'",
        glyph::base_of(ch)
    );
    slice
}

fn revert_vni(ch: char) -> &'static [char] {
    ascii_slice(ch)
}

/// Map an ASCII letter to a &'static one-char slice (for revert spans).
fn ascii_slice(ch: char) -> &'static [char] {
    match ch {
        'a' => &['a'],
        'e' => &['e'],
        'o' => &['o'],
        'u' => &['u'],
        'd' => &['d'],
        _ => &[],
    }
}

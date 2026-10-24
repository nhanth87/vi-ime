// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Tự do dictionary: real Vietnamese syllables + English words, used to
//! settle keys that read as BOTH ("rest" → rét, "test" → tét, "car" → cả).
//!
//! Loaded at run time from the hunspell dictionaries already on the system
//! (LibreOffice, OnlyOffice, /usr/share/hunspell) instead of shipping them:
//! the Vietnamese one is GPLv2, this project is GPL-3.0-only. A user copy in
//! `$XDG_DATA_HOME/vi-ime/dict/` wins. Without the files everything still
//! works — the built-in english_common.txt and the structural rhyme check
//! (rhyme.rs) remain; only the finer "is this a REAL Vietnamese word"
//! answer is unavailable (`is_vietnamese_word` → None).
//!
//! Vietnamese words are keyed by (toneless syllable, tone), so the Classic
//! and Modern tone placements match the same entry (vi_VN.dic only lists
//! "hoà"; users type "hòa").

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::LazyLock;

use unicode_normalization::UnicodeNormalization;

const TONE_MARKS: [char; 5] = ['\u{301}', '\u{300}', '\u{309}', '\u{303}', '\u{323}'];

/// Hunspell locations, most specific first.
fn candidates(lang: &str) -> Vec<PathBuf> {
    let mut v = Vec::new();
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    if let Some(d) = data {
        v.push(d.join(format!("vi-ime/dict/{lang}.dic")));
    }
    let short = &lang[..2];
    for p in [
        format!("/usr/share/hunspell/{lang}.dic"),
        format!("/usr/share/myspell/{lang}.dic"),
        format!("/usr/share/myspell/dicts/{lang}.dic"),
        format!("/usr/lib/libreoffice/share/extensions/dict-{short}/{lang}.dic"),
        format!("/opt/onlyoffice/desktopeditors/dictionaries/{lang}/{lang}.dic"),
    ] {
        v.push(PathBuf::from(p));
    }
    v
}

fn read_dic(lang: &str) -> Option<String> {
    candidates(lang).into_iter().find_map(|p| {
        let text = std::fs::read_to_string(&p).ok()?;
        tracing::info!("[LEXICON] {lang}: {}", p.display());
        Some(text)
    })
}

/// "(toneless syllable)(tone 0-5)" — placement-independent.
fn key(word: &str) -> String {
    let mut tone = 0;
    let base: String = word
        .to_lowercase()
        .nfd()
        .filter(|c| match TONE_MARKS.iter().position(|m| m == c) {
            Some(i) => {
                tone = i + 1;
                false
            }
            None => true,
        })
        .collect();
    let mut k: String = base.nfc().collect();
    k.push(char::from(b'0' + tone as u8));
    k
}

static VIETNAMESE: LazyLock<Option<HashSet<String>>> = LazyLock::new(|| {
    let text = read_dic("vi_VN")?;
    Some(
        text.lines()
            .skip(1) // hunspell line 1 = entry count
            .filter_map(|l| l.split('/').next())
            .map(str::trim)
            .filter(|w| !w.is_empty() && !w.contains(' '))
            .map(key)
            .collect(),
    )
});

static ENGLISH: LazyLock<HashSet<String>> = LazyLock::new(|| {
    let mut set: HashSet<String> = include_str!("../data/english_common.txt")
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect();
    if let Some(text) = read_dic("en_US") {
        for line in text.lines().skip(1) {
            let (word, flags) = line.split_once('/').unwrap_or((line, ""));
            // Lowercase entries of 3+ letters only: proper nouns and
            // abbreviations ("DD", "Hue") collide with real typing ("dd" → đ).
            if word.len() < 3 || !word.chars().all(|c| c.is_ascii_lowercase()) {
                continue;
            }
            let w = word.to_ascii_lowercase();
            // The affix forms that matter for typing collisions.
            if flags.contains('S') {
                set.insert(format!("{w}s"));
            }
            if flags.contains('D') {
                set.insert(if w.ends_with('e') { format!("{w}d") } else { format!("{w}ed") });
            }
            if flags.contains('G') {
                set.insert(format!("{}ing", w.strip_suffix('e').unwrap_or(&w)));
            }
            set.insert(w);
        }
    }
    set
});

/// Load both dictionaries off the typing path (first commit must not stall).
pub(crate) fn warm() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        let _ = std::thread::Builder::new().name("vi-lexicon".into()).spawn(|| {
            let vi = VIETNAMESE.as_ref().map_or(0, |s| s.len());
            tracing::info!("[LEXICON] {} âm tiết Việt, {} từ Anh", vi, ENGLISH.len());
        });
    });
}

/// Is `raw` (the keys as typed) an English word?
pub(crate) fn is_english(raw: &str) -> bool {
    ENGLISH.contains(&raw.to_ascii_lowercase())
}

/// Is `rendered` a real Vietnamese syllable? None when no dictionary is
/// installed — callers then fall back to the structural check.
pub(crate) fn is_vietnamese_word(rendered: &str) -> Option<bool> {
    VIETNAMESE.as_ref().map(|s| s.contains(&key(rendered)))
}

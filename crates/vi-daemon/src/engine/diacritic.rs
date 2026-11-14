// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Diacritic restoration by context (R37): "toi di hoc" → "tôi đi học".
//!
//! Syllable-bigram Viterbi after dmoreq/vndiacritics (MIT), over a compact
//! table (`data/vdia.bin`, 5 MB): 65 535 syllables, 1.6 M pairs as CSR rows
//! of `u16` ids with a 4-bit log-count each. Counts come from Wikipedia vi
//! (CC BY-SA), OPUS OpenSubtitles and Vietnamese legal texts (CC BY 4.0).
//!
//! Speed first: nothing here runs per key. The table is decoded once on a
//! background thread (`set_enabled`); `fix` runs only at a phrase end and returns
//! `None` at once while the table is not ready. Measured: ~30 µs a phrase.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

static DATA: &[u8] = include_bytes!("../data/vdia.bin");
static TABLE: OnceLock<Option<Table>> = OnceLock::new();
static ENABLED: AtomicBool = AtomicBool::new(false);
static OFFICE: AtomicBool = AtomicBool::new(false);
/// Tone placement of the output: "hoà" (modern) or "hòa" (classic), as the engine types it.
static MODERN: AtomicBool = AtomicBool::new(false);
/// Unaccented words the user took back once: never changed again.
static KEEP: Mutex<Option<HashSet<String>>> = Mutex::new(None);
const MAX_CAND: usize = 8;
const MAX_WORDS: usize = 40;

struct Table {
    words: Vec<&'static str>,
    ids: HashMap<&'static str, u16>,
    cand: HashMap<String, Vec<u16>>,
    uni: &'static [u8],
    rows: &'static [u8],
    id2: &'static [u8],
    nib: &'static [u8],
    dec: [f64; 16],
    k: f64,
    den: f64,
    unk: f64,
    skip: u32,
}

pub(crate) fn base(c: char) -> char {
    const GROUPS: [(&str, char); 7] = [
        ("àáảãạăằắẳẵặâầấẩẫậ", 'a'),
        ("èéẻẽẹêềếểễệ", 'e'),
        ("ìíỉĩị", 'i'),
        ("òóỏõọôồốổỗộơờớởỡợ", 'o'),
        ("ùúủũụưừứửữự", 'u'),
        ("ỳýỷỹỵ", 'y'),
        ("đ", 'd'),
    ];
    if c.is_ascii() {
        return c;
    }
    GROUPS.iter().find(|(s, _)| s.contains(c)).map_or(c, |&(_, b)| b)
}

fn strip(w: &str) -> String {
    w.chars().map(base).collect()
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    b.get(4 * i..4 * i + 4).map_or(0, |c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
}

fn f64_at(b: &[u8], off: usize) -> Option<f64> {
    Some(f64::from_le_bytes(b.get(off..off + 8)?.try_into().ok()?))
}

impl Table {
    fn parse(b: &'static [u8]) -> Option<Table> {
        if b.get(..5)? != b"VDIA1" {
            return None;
        }
        let head = b.get(8..24)?;
        let (v, np, blob) = (u32_at(head, 0) as usize, u32_at(head, 1) as usize, u32_at(head, 2) as usize);
        let (lo, hi) = (f64_at(b, 24)?, f64_at(b, 32)?);
        let mut p = 72;
        let mut take = |n: usize| {
            let s = b.get(p..p + n);
            p += n;
            s
        };
        let words: Vec<&'static str> = std::str::from_utf8(take(blob)?).ok()?.split('\n').collect();
        if words.len() != v || v > usize::from(u16::MAX) {
            return None;
        }
        let (uni, rows, id2) = (take(4 * v)?, take(4 * (v + 1))?, take(2 * np)?);
        let nib = b.get(p..)?;
        if nib.len() < np.div_ceil(2) {
            return None;
        }
        let mut dec = [0.0; 16];
        for (i, d) in dec.iter_mut().enumerate() {
            *d = (lo + i as f64 / 15.0 * (hi - lo)).exp();
        }
        let mut cand: HashMap<String, Vec<u16>> = HashMap::new();
        let mut ids = HashMap::with_capacity(v);
        for (i, w) in words.iter().enumerate() {
            ids.insert(*w, i as u16);
            let forms = cand.entry(strip(w)).or_default(); // words are count-sorted
            if forms.len() < MAX_CAND {
                forms.push(i as u16);
            }
        }
        Some(Table {
            words, ids, cand, uni, rows, id2, nib, dec,
            k: f64_at(b, 40)?, den: f64_at(b, 48)?, unk: f64_at(b, 56)?, skip: u32_at(head, 3),
        })
    }

    fn pair(&self, a: u16, b: u16) -> f64 {
        let (s, e) = (u32_at(self.rows, a as usize) as usize, u32_at(self.rows, a as usize + 1) as usize);
        let (mut lo, mut hi) = (s, e);
        while lo < hi {
            let mid = (lo + hi) / 2;
            let x = self.id2.get(2 * mid..2 * mid + 2).map_or(u16::MAX, |c| u16::from_le_bytes([c[0], c[1]]));
            if x < b {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        let hit = lo < e && self.id2.get(2 * lo..2 * lo + 2) == Some(&b.to_le_bytes()[..]);
        if !hit {
            return 0.0;
        }
        self.dec[usize::from((self.nib.get(lo / 2).copied().unwrap_or(0) >> (4 * (lo & 1))) & 15)]
    }

    /// Best accented form per word. A column is the candidate ids of one
    /// word; an empty column is a word outside the table (kept as typed).
    fn viterbi(&self, cols: &[Vec<u16>]) -> Vec<Option<u16>> {
        let n = cols.len();
        let width = |i: usize| cols[i].len().max(1);
        let count = |i: usize, j: usize| cols[i].get(j).map_or(0, |&id| u32_at(self.uni, id as usize));
        let mut score = vec![[0.0f64; MAX_CAND]; n];
        let mut back = vec![[0u8; MAX_CAND]; n];
        for i in 1..n {
            for j in 0..width(i) {
                let cur = count(i, j);
                let kp = self.k * (f64::from(cur) + 1.0) / self.den;
                let (mut best, mut arg) = (f64::MIN, 0);
                for p in 0..width(i - 1) {
                    let n1 = count(i - 1, p);
                    let n2 = match (cols[i - 1].get(p), cols[i].get(j)) {
                        (Some(&a), Some(&b)) if cur >= self.skip && n1 >= self.skip => self.pair(a, b),
                        _ => 0.0,
                    };
                    let mut s = score[i - 1][p]
                        + if n2 > 0.0 { ((n2 + kp) / (f64::from(n1) + self.k)).ln() } else { self.unk };
                    if i == 1 {
                        s += ((f64::from(n1) + 1.0) / self.den).ln();
                    }
                    if s > best {
                        (best, arg) = (s, p);
                    }
                }
                score[i][j] = best;
                back[i][j] = arg as u8;
            }
        }
        let mut idx = 0;
        for j in 1..width(n - 1) {
            if score[n - 1][j] > score[n - 1][idx] {
                idx = j;
            }
        }
        let mut out = vec![None; n];
        for i in (0..n).rev() {
            out[i] = cols[i].get(idx).copied();
            idx = usize::from(back[i][idx]);
        }
        out
    }
}

fn keep_file() -> Option<PathBuf> {
    let dir = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(dir.join("vi-ime/diacritic_keep.txt"))
}

/// Config switch (`auto_diacritics`). Turning it on decodes the table on a
/// background thread; the typing thread never waits for it.
pub(crate) fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
    if on && TABLE.get().is_none() {
        std::thread::spawn(|| {
            let _ = TABLE.get_or_init(|| Table::parse(DATA));
            let kept = keep_file().and_then(|f| std::fs::read_to_string(f).ok()).unwrap_or_default();
            if let Ok(mut k) = KEEP.lock() {
                k.get_or_insert_with(HashSet::new).extend(kept.lines().map(String::from));
            }
        });
    }
}

/// `auto_diacritics_office`: also on the evdev path (LibreOffice,
/// OnlyOffice). Separate until that path has a field test.
pub(crate) fn set_office(on: bool) {
    OFFICE.store(on, Ordering::Relaxed);
}

pub(crate) fn office_enabled() -> bool {
    enabled() && OFFICE.load(Ordering::Relaxed)
}

pub(crate) fn set_modern_tone(on: bool) {
    MODERN.store(on, Ordering::Relaxed);
}

/// `form` in the user's tone placement. The corpus mixes both; only open
/// oa/oe/uy differ ("qu" is an initial: "quý" has one spelling).
fn restyle(form: &str) -> String {
    const PAIRS: [(&str, &str); 15] = [
        ("òa", "oà"), ("óa", "oá"), ("ỏa", "oả"), ("õa", "oã"), ("ọa", "oạ"),
        ("òe", "oè"), ("óe", "oé"), ("ỏe", "oẻ"), ("õe", "oẽ"), ("ọe", "oẹ"),
        ("ùy", "uỳ"), ("úy", "uý"), ("ủy", "uỷ"), ("ũy", "uỹ"), ("ụy", "uỵ"),
    ];
    if form.starts_with('q') {
        return form.to_string();
    }
    let modern = MODERN.load(Ordering::Relaxed);
    for (classic, new) in PAIRS {
        let (from, to) = if modern { (classic, new) } else { (new, classic) };
        if let Some(head) = form.strip_suffix(from) {
            return format!("{head}{to}");
        }
    }
    form.to_string()
}

pub(crate) fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// The user undid a fix: leave these typed words alone from now on.
pub(crate) fn keep(words: &[String]) {
    let Ok(mut k) = KEEP.try_lock() else { return };
    let set = k.get_or_insert_with(HashSet::new);
    let fresh: Vec<String> = words.iter().map(|w| w.to_lowercase()).filter(|w| set.insert(w.clone())).collect();
    if fresh.is_empty() {
        return;
    }
    let all: String = set.iter().map(|w| format!("{w}\n")).collect();
    std::thread::spawn(move || {
        if let Some(f) = keep_file() {
            let _ = crate::privfile::write(&f, all);
        }
    });
}

/// Forget the kept words (with the other learned data).
pub(crate) fn clear() {
    if let Ok(mut k) = KEEP.lock() {
        *k = Some(HashSet::new());
    }
    if let Some(f) = keep_file() {
        let _ = std::fs::remove_file(f);
    }
}

/// The accented forms of a phrase as typed, or `None` when nothing changes,
/// the table is not ready, or the phrase is not plain Vietnamese (any word
/// that is neither accented nor a known syllable).
pub(crate) fn fix(words: &[String]) -> Option<Vec<String>> {
    fix_at(words, false)
}

/// `fix`, with `start` when the phrase opens a sentence: the table then
/// also knows how often each word comes first ("ban …" → "bạn", not "bàn";
/// its `<s>` row, measured +0.8 points on general text).
pub(crate) fn fix_at(words: &[String], start: bool) -> Option<Vec<String>> {
    if !enabled() || words.len() < 2 || words.len() > MAX_WORDS {
        return None;
    }
    let t = TABLE.get()?.as_ref()?;
    let kept = KEEP.try_lock().ok()?;
    let mut cols = Vec::with_capacity(words.len());
    let mut open = 0;
    for w in words {
        let low = w.to_lowercase();
        let own = || t.ids.get(low.as_str()).map(|&id| vec![id]).unwrap_or_default();
        if strip(&low) != low {
            cols.push(own()); // accented by the user: an anchor
        } else if kept.as_ref().is_some_and(|k| k.contains(&low)) {
            cols.push(own());
        } else {
            cols.push(t.cand.get(&low)?.clone());
            open += 1;
        }
    }
    if open == 0 {
        return None;
    }
    let opener = start.then(|| t.ids.get("<s>").copied()).flatten();
    if let Some(id) = opener {
        cols.insert(0, vec![id]);
    }
    let mut best = t.viterbi(&cols);
    if opener.is_some() {
        best.remove(0);
    }
    let mut forms: Vec<Option<String>> =
        best.iter().map(|id| id.map(|id| restyle(t.words[usize::from(id)]))).collect();
    personal(words, kept.as_ref(), &mut forms);
    let out: Vec<String> = words
        .iter()
        .zip(forms)
        .map(|(w, form)| match form {
            Some(form) => recase(w, &form),
            None => w.clone(),
        })
        .collect();
    (out != words).then_some(out)
}

/// Whether the table has seen `word` right after `prev` (Tự do: of two real
/// readings, the one that continues the sentence). False while the table is
/// not loaded.
pub(crate) fn follows(prev: &str, word: &str) -> bool {
    let Some(Some(t)) = enabled().then(|| TABLE.get()).flatten() else { return false };
    let id = |w: &str| t.ids.get(w.to_lowercase().as_str()).copied();
    matches!((id(prev), id(word)), (Some(a), Some(b)) if t.pair(a, b) > 0.0)
}

/// The `n` words the table has seen most often after `prev` (suggestion bar,
/// when the user's own history has nothing). One row scan, at a word end.
pub(crate) fn next_words(prev: &str, n: usize) -> Vec<String> {
    let Some(Some(t)) = enabled().then(|| TABLE.get()).flatten() else { return Vec::new() };
    let Some(&a) = t.ids.get(prev.to_lowercase().as_str()) else { return Vec::new() };
    let (s, e) = (u32_at(t.rows, usize::from(a)) as usize, u32_at(t.rows, usize::from(a) + 1) as usize);
    // Best = highest 4-bit count; ties go to the more common word (lower id).
    let mut best: Vec<(u8, u16)> = Vec::with_capacity(n + 1);
    for j in s..e {
        let score = (t.nib.get(j / 2).copied().unwrap_or(0) >> (4 * (j & 1))) & 15;
        let id = t.id2.get(2 * j..2 * j + 2).map_or(u16::MAX, |c| u16::from_le_bytes([c[0], c[1]]));
        let at = best.partition_point(|&(sc, i)| (sc, std::cmp::Reverse(i)) >= (score, std::cmp::Reverse(id)));
        if at < n {
            best.insert(at, (score, id));
            best.truncate(n);
        }
    }
    best.iter().filter_map(|&(_, id)| t.words.get(usize::from(id)).map(|w| (*w).to_string())).collect()
}

/// The user's own pairs win over the table: "cuc cuc" is "cục cức" for
/// someone who has typed that, whatever the corpus says (user 2026-11-14).
/// A pair typed once is enough to ADD diacritics; to leave a word bare that
/// the table accents it must have been typed 3 times (the history also holds
/// what was typed before this feature fixed it).
fn personal(words: &[String], kept: Option<&HashSet<String>>, forms: &mut [Option<String>]) {
    let low: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
    let mut locked = vec![false; words.len()];
    for (i, a, b, n) in super::predict::personal_pairs(&low, strip) {
        let ok = |j: usize, form: &String| {
            let bare = strip(form) == *form;
            let table_accents = forms[j].as_ref().is_some_and(|f| strip(f) != *f);
            form.chars().count() == low[j].chars().count()
                && !kept.is_some_and(|k| k.contains(&low[j]))
                && (!locked[j] || forms[j].as_ref() == Some(form))
                && (!bare || !table_accents || n >= 3)
        };
        if ok(i, &a) && ok(i + 1, &b) {
            forms[i] = Some(restyle(&a));
            forms[i + 1] = Some(restyle(&b));
            locked[i] = true;
            locked[i + 1] = true;
        }
    }
}

/// `form` with the capitals of `typed` (same length: one char per letter).
fn recase(typed: &str, form: &str) -> String {
    typed
        .chars()
        .zip(form.chars())
        .map(|(t, f)| if t.is_uppercase() { f.to_uppercase().next().unwrap_or(f) } else { f })
        .collect()
}

#[cfg(test)]
#[path = "diacritic_tests.rs"]
mod diacritic_tests;

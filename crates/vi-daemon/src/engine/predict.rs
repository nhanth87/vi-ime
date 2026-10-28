// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Word suggestions learned from the user's own typing (opt-out).
//!
//! Counts every committed word and every pair of consecutive words, and
//! suggests up to three words: completions of the word being typed, or —
//! right after a word ends — the words this user most often types next.
//! Shown by the suggestion bar, accepted with Alt+1..3.
//!
//! Privacy: stored only on this machine
//! (`$XDG_DATA_HOME/vi-ime/predict.tsv`), never fed from password / URL /
//! numeric fields or browsers on the evdev path (callers gate),
//! switchable off (`predict = false`) and erasable (`clear`).
//!
//! Hot path: `set_prefix` runs on every keystroke, so it never blocks
//! (`try_lock`; a skipped update only leaves the bar one key stale) and
//! never scans the whole table — words are sorted (prefix = range scan,
//! from `MIN_PREFIX` chars) and next-word counts are keyed by the previous
//! word. Saving formats the table on its own thread.
//!
//! One global state shared by both typing paths (Wayland, evdev) and the
//! IPC subscription; a Condvar wakes the bar only when suggestions change
//! (no polling, R15).

use std::collections::{BTreeMap, HashMap};
use std::ops::Bound;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, LazyLock, Mutex};
use std::time::{Duration, Instant};

use unicode_normalization::UnicodeNormalization;

/// Caps keep the table small; over a cap, words seen once are dropped.
const MAX_WORDS: usize = 40_000;
const MAX_PAIRS: usize = 80_000;
const MAX_WORD_CHARS: usize = 24;
/// Completions start at this many typed chars (one letter matches too much
/// to be useful and is the costliest scan).
const MIN_PREFIX: usize = 2;
/// Save after this many new observations (and on flush).
const SAVE_EVERY: u32 = 25;
/// Suggestions vanish this long after the last keystroke: the bar is for
/// the word being typed, and while it shows, Alt+1..3 belong to it — Alt+1
/// must go back to the app (Chrome: switch tab) once the user stops typing.
const LIVE: Duration = Duration::from_secs(4);

#[derive(Default)]
struct Predictor {
    words: BTreeMap<String, u32>,
    /// prev word → (next word → count).
    next: HashMap<String, HashMap<String, u32>>,
    pairs: usize,
    prev: Option<String>,
    prefix: String,
    suggestions: Vec<String>,
    generation: u64,
    enabled: bool,
    unsaved: u32,
    /// Last keystroke that fed the predictor (see `LIVE`).
    touched: Option<Instant>,
}

static STATE: LazyLock<(Mutex<Predictor>, Condvar)> = LazyLock::new(|| {
    let mut p = Predictor { enabled: true, ..Predictor::default() };
    load(&mut p);
    std::thread::Builder::new().name("vi-predict-expire".into()).spawn(expire_loop).ok();
    (Mutex::new(p), Condvar::new())
});

/// Clears the suggestions `LIVE` after the last keystroke. Sleeps on the
/// shared Condvar: no wakeups while nothing is shown (R15).
fn expire_loop() {
    let (lock, cv) = &*STATE;
    let Ok(mut p) = lock.lock() else { return };
    loop {
        let left = match p.touched {
            Some(t) if !p.suggestions.is_empty() => LIVE.saturating_sub(t.elapsed()),
            _ => {
                p = match cv.wait(p) {
                    Ok(g) => g,
                    Err(_) => return,
                };
                continue;
            }
        };
        if left.is_zero() {
            p.suggestions.clear();
            p.generation += 1;
            cv.notify_all();
            continue;
        }
        p = match cv.wait_timeout(p, left) {
            Ok((g, _)) => g,
            Err(_) => return,
        };
    }
}

fn file() -> Option<PathBuf> {
    if cfg!(test) {
        return None; // unit tests never touch the user's data
    }
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(base.join("vi-ime/predict.tsv"))
}

fn load(p: &mut Predictor) {
    let Some(text) = file().and_then(|f| std::fs::read_to_string(f).ok()) else { return };
    for line in text.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        match cols.as_slice() {
            ["w", w, n] => {
                if let Ok(n) = n.parse() {
                    p.words.insert((*w).to_string(), n);
                }
            }
            ["p", a, b, n] => {
                if let Ok(n) = n.parse() {
                    p.next.entry((*a).to_string()).or_default().insert((*b).to_string(), n);
                    p.pairs += 1;
                }
            }
            _ => {}
        }
    }
}

/// Write the table from a background thread (it takes the lock itself, so
/// the typing thread pays nothing but a brief wait at its next word end).
fn save_async() {
    let Some(path) = file() else { return };
    std::thread::spawn(move || {
        let out = {
            let Ok(p) = STATE.0.lock() else { return };
            let mut out = String::new();
            for (w, n) in &p.words {
                out.push_str(&format!("w\t{w}\t{n}\n"));
            }
            for (a, m) in &p.next {
                for (b, n) in m {
                    out.push_str(&format!("p\t{a}\t{b}\t{n}\n"));
                }
            }
            out
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, out).is_ok() {
            let _ = std::fs::rename(tmp, path);
        }
    });
}

/// Lowercase NFC, so precomposed and decomposed output modes share counts.
fn fold(word: &str) -> String {
    word.trim().nfc().collect::<String>().to_lowercase()
}

/// A word worth learning: letters only (Vietnamese included), short.
fn normalize(word: &str) -> Option<String> {
    let w = fold(word);
    let n = w.chars().count();
    (n >= 2 && n <= MAX_WORD_CHARS && w.chars().all(char::is_alphabetic)).then_some(w)
}

fn top3<'a>(it: impl Iterator<Item = (u32, &'a String)>) -> Vec<String> {
    let mut v: Vec<(u32, &String)> = it.collect();
    v.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    v.into_iter().take(3).map(|(_, w)| w.clone()).collect()
}

fn recompute(p: &mut Predictor) {
    let s = if !p.enabled {
        Vec::new()
    } else if p.prefix.is_empty() {
        match p.prev.as_ref().and_then(|a| p.next.get(a)) {
            Some(m) => top3(m.iter().map(|(w, n)| (*n, w))),
            None => Vec::new(),
        }
    } else if p.prefix.chars().count() < MIN_PREFIX {
        Vec::new()
    } else {
        let pre = p.prefix.as_str();
        let follow = p.prev.as_ref().and_then(|a| p.next.get(a));
        top3(
            p.words
                .range::<str, _>((Bound::Included(pre), Bound::Unbounded))
                .take_while(|(w, _)| w.starts_with(pre))
                .filter(|(w, _)| w.len() > pre.len())
                .map(|(w, n)| (n + 10 * follow.and_then(|m| m.get(w)).copied().unwrap_or(0), w)),
        )
    };
    if s != p.suggestions {
        p.suggestions = s;
        p.generation += 1;
        STATE.1.notify_all();
    }
}

fn with<R>(f: impl FnOnce(&mut Predictor) -> R) -> Option<R> {
    STATE.0.lock().ok().map(|mut p| f(&mut p))
}

/// Config switch (`predict` in setting.conf).
pub(crate) fn set_enabled(on: bool) {
    with(|p| {
        if p.enabled != on {
            p.enabled = on;
            recompute(p);
        }
    });
}

/// The word being composed changed ("" = between words). Every keystroke:
/// skipped rather than waiting when the table is busy (a save).
pub(crate) fn set_prefix(prefix: &str) {
    let Ok(mut p) = STATE.0.try_lock() else { return };
    if !p.enabled {
        return;
    }
    if apply_queued(&mut p) {
        p.unsaved = SAVE_EVERY; // saved by the next word end
    }
    p.touched = Some(Instant::now());
    let pre = fold(prefix);
    if p.prefix != pre {
        p.prefix = pre;
        recompute(&mut p);
    }
}

/// Typing-thread calls never wait for the table: while a save holds it,
/// committed words and context resets queue here (nobody holds these for
/// long) and are applied by the next call that gets the table. Probe
/// 2026-10-27: a blocking `observe` stalled a word end up to 28ms behind
/// a save formatting the table — visible as live-echo lag.
static QUEUED: Mutex<Vec<String>> = Mutex::new(Vec::new());
static RESET_QUEUED: AtomicBool = AtomicBool::new(false);

fn apply_queued(p: &mut Predictor) -> bool {
    if RESET_QUEUED.swap(false, Ordering::Relaxed) {
        p.prev = None;
        p.prefix.clear();
    }
    let words = QUEUED.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default();
    words.iter().fold(false, |due, w| learn(p, w) | due)
}

/// Count `word` (and the pair it ends); true when a save is due.
fn learn(p: &mut Predictor, word: &str) -> bool {
    if !p.enabled {
        return false;
    }
    p.prefix.clear();
    let Some(w) = normalize(word) else {
        p.prev = None; // numbers, URLs, symbols break the sentence chain
        return false;
    };
    *p.words.entry(w.clone()).or_insert(0) += 1;
    if let Some(prev) = p.prev.take() {
        let m = p.next.entry(prev).or_default();
        let c = m.entry(w.clone()).or_insert(0);
        if *c == 0 {
            p.pairs += 1;
        }
        *c += 1;
    }
    p.prev = Some(w);
    if p.words.len() > MAX_WORDS {
        p.words.retain(|_, n| *n > 1);
    }
    if p.pairs > MAX_PAIRS {
        p.next.values_mut().for_each(|m| m.retain(|_, n| *n > 1));
        p.next.retain(|_, m| !m.is_empty());
        p.pairs = p.next.values().map(HashMap::len).sum();
    }
    p.unsaved += 1;
    let due = p.unsaved >= SAVE_EVERY;
    if due {
        p.unsaved = 0;
    }
    due
}

/// A word was committed in a normal text field: learn it and follow it.
pub(crate) fn observe(word: &str) {
    let Ok(mut p) = STATE.0.try_lock() else {
        // Capped: draining stays ~µs (a save holds the table for ms, words
        // come every ~0.3s — the queue is normally 0-1 words long).
        if let Ok(mut q) = QUEUED.lock()
            && q.len() < 256
        {
            q.push(word.to_string());
        }
        return;
    };
    let due = apply_queued(&mut p) | learn(&mut p, word);
    p.touched = Some(Instant::now());
    recompute(&mut p);
    drop(p);
    if due {
        save_async();
    }
}

/// Focus moved / cursor jumped / field changed: start a new sentence.
pub(crate) fn reset_context() {
    let Ok(mut p) = STATE.0.try_lock() else {
        RESET_QUEUED.store(true, Ordering::Relaxed);
        return;
    };
    let due = apply_queued(&mut p);
    if p.prev.is_some() || !p.prefix.is_empty() {
        p.prev = None;
        p.prefix.clear();
        recompute(&mut p);
    }
    drop(p);
    if due {
        save_async();
    }
}

/// Suggestion `i` (0-based), cased like the prefix the user started with.
pub(crate) fn accept(i: usize, typed_prefix: &str) -> Option<String> {
    let w = with(|p| p.suggestions.get(i).cloned())??;
    let upper = typed_prefix.chars().next().is_some_and(char::is_uppercase);
    Some(if upper {
        let mut c = w.chars();
        c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or(w)
    } else {
        w
    })
}

/// Current suggestions and their generation.
pub(crate) fn current() -> (u64, Vec<String>) {
    with(|p| (p.generation, p.suggestions.clone())).unwrap_or_default()
}

/// Block until the suggestions differ from `seen` (IPC subscription thread).
pub(crate) fn wait_change(seen: u64) -> (u64, Vec<String>) {
    let (lock, cv) = &*STATE;
    let Ok(mut p) = lock.lock() else { return (seen, Vec::new()) };
    while p.generation == seen {
        p = match cv.wait(p) {
            Ok(g) => g,
            Err(_) => return (seen, Vec::new()),
        };
    }
    (p.generation, p.suggestions.clone())
}

/// Forget everything learned (settings window button).
pub(crate) fn clear() {
    with(|p| {
        p.words.clear();
        p.next.clear();
        p.pairs = 0;
        p.prev = None;
        p.unsaved = 0;
        recompute(p);
    });
    if let Some(f) = file() {
        let _ = std::fs::remove_file(f);
    }
}

/// Flush unsaved learning (daemon shutdown is abrupt, so also periodic).
pub(crate) fn flush() {
    let due = with(|p| std::mem::take(&mut p.unsaved) > 0);
    if due == Some(true) {
        save_async();
    }
}

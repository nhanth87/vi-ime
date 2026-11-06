// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Learns the user's own choices at the English/Vietnamese word boundary,
//! so the guesses get better the more they type (user 2026-11-06).
//!
//! Per raw key sequence ("bar", "test") a score: positive = the user wants
//! the raw keys (English), negative = the Vietnamese rendering.
//! - **Correction** (strong): the user deletes the word just committed and
//!   types the same keys again within `WINDOW` → they wanted the other form.
//!   That word comes out the other way at once, and the score is SET to
//!   ±`CORRECTED` so one correction decides from then on.
//! - **Kept** (weak): a choice left alone counts ±1, capped at `CAP`. It only
//!   reinforces; it cannot outweigh a correction (no self-reinforcing loop).
//! - **Style**: how Tự do tones are typed (Telex letters vs VNI digits),
//!   shown in Settings as a suggestion — never switched behind the user.
//!
//! Privacy: learning only while `set_learning(true)` — normal text fields
//! (Wayland) and office apps (evdev), never passwords/URLs/browsers on
//! evdev. Local file `$XDG_DATA_HOME/vi-ime/habit.tsv`, switchable off
//! (`learn_habits`), erased with the suggestion data.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

/// A correction must follow the commit within this time.
const WINDOW: Duration = Duration::from_secs(20);
/// Score set by a correction; |score| >= DECIDE overrides the rules.
const CORRECTED: i32 = 4;
const DECIDE: i32 = 3;
const CAP: i32 = 8;
const MAX_ENTRIES: usize = 20_000;
const SAVE_EVERY: u32 = 20;

struct Last {
    raw: String,
    english: bool,
    out_chars: usize,
    at: Instant,
}

#[derive(Default)]
struct Habit {
    score: HashMap<String, i32>,
    last: Option<Last>,
    deletions: usize,
    telex: u32,
    vni: u32,
    unsaved: u32,
}

static ENABLED: AtomicBool = AtomicBool::new(true);
static LEARNING: AtomicBool = AtomicBool::new(false);

static STATE: LazyLock<Mutex<Habit>> = LazyLock::new(|| {
    let mut h = Habit::default();
    load(&mut h);
    Mutex::new(h)
});

fn file() -> Option<PathBuf> {
    if cfg!(test) {
        return None; // unit tests never touch the user's data
    }
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(base.join("vi-ime/habit.tsv"))
}

fn load(h: &mut Habit) {
    let Some(text) = file().and_then(|f| std::fs::read_to_string(f).ok()) else { return };
    for line in text.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        match cols.as_slice() {
            ["s", raw, n] => {
                if let Ok(n) = n.parse() {
                    h.score.insert((*raw).to_string(), n);
                }
            }
            ["style", t, v] => {
                h.telex = t.parse().unwrap_or(0);
                h.vni = v.parse().unwrap_or(0);
            }
            _ => {}
        }
    }
}

fn save(h: &Habit) {
    let Some(path) = file() else { return };
    let mut out = format!("style\t{}\t{}\n", h.telex, h.vni);
    for (raw, n) in &h.score {
        out.push_str(&format!("s\t{raw}\t{n}\n"));
    }
    std::thread::spawn(move || {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, out).is_ok() {
            let _ = std::fs::rename(tmp, path);
        }
    });
}

/// Typing-thread calls never wait (same rule as predict.rs, R32).
fn with<R>(f: impl FnOnce(&mut Habit) -> R) -> Option<R> {
    STATE.try_lock().ok().map(|mut h| f(&mut h))
}

fn bump(h: &mut Habit, raw: &str, set: Option<i32>, delta: i32) {
    if !LEARNING.load(Ordering::Relaxed) {
        return;
    }
    let s = h.score.entry(raw.to_string()).or_insert(0);
    *s = set.unwrap_or((*s + delta).clamp(-CAP, CAP));
    h.unsaved += 1;
    if h.score.len() > MAX_ENTRIES {
        h.score.retain(|_, n| n.abs() >= DECIDE);
    }
    if h.unsaved >= SAVE_EVERY {
        h.unsaved = 0;
        save(h);
    }
}

/// Config switch (`learn_habits`).
pub(crate) fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// Whether the current field may teach us (set per key by the typing path).
pub(crate) fn set_learning(on: bool) {
    LEARNING.store(on, Ordering::Relaxed);
}

/// A BackSpace outside any composition (deleting committed text).
pub(crate) fn backspace() {
    with(|h| h.deletions += 1);
}

/// Before the rules: Some(true) = raw keys, Some(false) = Vietnamese.
pub(crate) fn decide(raw: &str) -> Option<bool> {
    if !ENABLED.load(Ordering::Relaxed) {
        return None;
    }
    let key = raw.to_lowercase();
    with(|h| {
        let corrected = h.last.as_ref().is_some_and(|l| {
            l.raw == key
                && l.at.elapsed() < WINDOW
                && h.deletions >= l.out_chars
                && h.deletions <= l.out_chars + 2 // that word, not a whole sentence
        });
        if corrected {
            let want_english = h.last.as_ref().is_some_and(|l| !l.english);
            h.last = None; // a corrected choice is not "kept"
            h.deletions = 0;
            bump(h, &key, Some(if want_english { CORRECTED } else { -CORRECTED }), 0);
            tracing::info!("[HABIT] correction: {key:?} → {}", if want_english { "raw" } else { "tiếng Việt" });
            return Some(want_english);
        }
        match h.score.get(&key).copied().unwrap_or(0) {
            s if s >= DECIDE => Some(true),
            s if s <= -DECIDE => Some(false),
            _ => None,
        }
    })
    .flatten()
}

/// After a decision with two possible forms (`vni` = digit tone keys).
pub(crate) fn record(raw: &str, english: bool, out_chars: usize, vni: bool) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    with(|h| {
        if let Some(prev) = h.last.take() {
            let raw = prev.raw.clone();
            bump(h, &raw, None, if prev.english { 1 } else { -1 }); // kept
        }
        if LEARNING.load(Ordering::Relaxed) && !english {
            if vni { h.vni += 1 } else { h.telex += 1 }
        }
        h.last = Some(Last { raw: raw.to_lowercase(), english, out_chars, at: Instant::now() });
        h.deletions = 0;
    });
}

/// Tự do tone style seen so far: (Telex words, VNI words).
pub(crate) fn style() -> (u32, u32) {
    STATE.lock().map(|h| (h.telex, h.vni)).unwrap_or((0, 0))
}

/// Forget everything learned (with the suggestion data).
pub(crate) fn clear() {
    if let Ok(mut h) = STATE.lock() {
        *h = Habit::default();
    }
    if let Some(f) = file() {
        let _ = std::fs::remove_file(f);
    }
}

/// Save unsaved learning (on app switch; the daemon may be killed).
pub(crate) fn flush() {
    if let Ok(mut h) = STATE.lock()
        && h.unsaved > 0
    {
        h.unsaved = 0;
        save(&h);
    }
}

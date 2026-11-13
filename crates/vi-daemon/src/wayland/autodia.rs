// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Phrase tracking for diacritic restoration (R37, `engine/diacritic.rs`).
//!
//! The phrase is the run of words typed one after another, in one field,
//! since the last thing that could have moved the cursor. Every key starts
//! by dropping the phrase unless the key before it was plain typing
//! (`begin_key`), so a path that forgets to report itself can only lose the
//! phrase, never rewrite text that is somewhere else. At `,` `.` `?` `!` the
//! phrase is re-read as a whole and the changed tail retyped by the typer,
//! punctuation included (R26). BackSpace right after puts the typed text back.

use std::sync::Mutex;
use std::sync::atomic::Ordering;

use tracing::info;

use crate::engine::{diacritic, predict};
use crate::wayland::state::ImeAppState;

const MAX_TAIL: usize = 120;
/// Phrase length bound: the table only looks at the last MAX_WORDS (40),
// so older words are dropped instead of growing without limit.
const PHRASE_CAP: usize = 48;
const PHRASE_KEEP: usize = 40;

#[derive(Default)]
struct Phrase {
    /// (word as on screen, separator typed after it).
    words: Vec<(String, String)>,
    /// The key before this one was plain typing.
    keep: bool,
    ctx: u64,
    /// (fixed tail, typed tail, typed words that changed).
    undo: Option<(String, String, Vec<String>)>,
}

static PHRASE: Mutex<Phrase> = Mutex::new(Phrase { words: Vec::new(), keep: false, ctx: 0, undo: None });

fn with<R: Default>(f: impl FnOnce(&mut Phrase) -> R) -> R {
    PHRASE.try_lock().map(|mut p| f(&mut p)).unwrap_or_default()
}

/// Start of every non-modifier key.
pub(crate) fn begin_key() {
    with(|p| {
        let ctx = predict::CONTEXT_GEN.load(Ordering::Relaxed);
        if !p.keep || p.ctx != ctx {
            p.words.clear();
            p.undo = None;
        }
        p.ctx = ctx;
        p.keep = false;
    });
}

/// A key that only edits the word being composed.
pub(crate) fn typing() {
    with(|p| {
        p.keep = true;
        p.undo = None;
    });
}

/// A key passed to the app as is: a space after a word extends the phrase.
pub(crate) fn passthrough(c: Option<char>) {
    with(|p| {
        p.undo = None;
        if let (Some(' '), Some(last)) = (c, p.words.last_mut()) {
            last.1.push(' ');
            p.keep = true;
        }
    });
}

pub(crate) fn clear() {
    with(|p| *p = Phrase::default());
}

/// A word committed with boundary `c`. At a phrase end returns
/// (typed tail, fixed tail): the text from the first changed word on.
/// A boundary that cannot trigger a fix (or a failed fix) leaves the
/// phrase intact for a later sentence end — only a successful fix
/// consumes it. So random mid-phrase punctuation no longer destroys
/// the sentence context.
fn on_word(text: &str, c: char) -> Option<(String, String)> {
    with(|p| {
        p.undo = None;
        if text.is_empty() || !text.chars().all(char::is_alphabetic) {
            p.words.clear();
            return None;
        }
        let push = |p: &mut Phrase, word: String, sep: String| {
            p.words.push((word, sep));
            if p.words.len() > PHRASE_CAP {
                p.words.drain(..p.words.len() - PHRASE_KEEP);
            }
            p.keep = true;
        };
        if c == ' ' {
            push(p, text.to_string(), " ".into());
            return None;
        }
        if !matches!(c, ',' | '.' | '?' | '!') {
            // `;` `:` …: part of the sentence, not its end.
            push(p, text.to_string(), c.to_string());
            return None;
        }
        let mut all = p.words.clone();
        all.push((text.to_string(), String::new()));
        let typed: Vec<String> = all.iter().map(|(w, _)| w.clone()).collect();
        let Some(fixed) = diacritic::fix(&typed) else {
            // Failed (unknown word, table off…): keep the whole sentence,
            // current word included, for the next sentence end.
            push(p, text.to_string(), String::new());
            return None;
        };
        let first = typed.iter().zip(&fixed).position(|(a, b)| a != b)?;
        let tail = |ws: &[String]| -> String {
            ws[first..].iter().zip(&all[first..]).map(|(w, (_, sep))| format!("{w}{sep}")).collect()
        };
        let (old, new) = (tail(&typed), tail(&fixed));
        if old.chars().count() > MAX_TAIL {
            p.words.clear(); // too messy to rewrite — start the next phrase over
            return None;
        }
        let changed = typed.iter().zip(&fixed).filter(|(a, b)| a != b).map(|(a, _)| a.clone()).collect();
        p.words.clear(); // consumed by the successful fix
        p.undo = Some((format!("{new}{c}"), format!("{old}{c}"), changed));
        p.keep = true;
        Some((old, new))
    })
}

impl ImeAppState {
    /// Live commit of `text` followed by the typed boundary char `c`.
    pub(crate) fn commit_boundary(&mut self, text: &str, c: char) {
        let inline = self.runtime.as_ref().is_some_and(|rt| rt.inline_complete());
        let plan = if diacritic::enabled() && self.learns() && !inline {
            on_word(text, c)
        } else {
            clear();
            None
        };
        let Some((old, new)) = plan else {
            return self.sync_shown(&format!("{text}{c}"));
        };
        // On screen: the typed tail, whose last word is `shown_word`.
        let head: String = old.chars().take(old.chars().count() - text.chars().count()).collect();
        self.shown_word.insert_str(0, &head);
        let target = format!("{new}{c}");
        self.sync_shown(&target);
        if self.shown_word == target {
            info!("[AUTODIA] phrase tail retyped ({} chars)", target.chars().count());
        } else {
            clear(); // typer refused: the screen still shows the typed text
        }
    }

    /// BackSpace right after a fix: put the typed text back. True = handled.
    pub(crate) fn undo_diacritics(&mut self) -> bool {
        let Some((fixed, typed, changed)) = with(|p| p.undo.take()) else { return false };
        self.shown_word = fixed;
        self.sync_shown(&typed);
        self.reset_word_state();
        diacritic::keep(&changed);
        clear();
        info!("[AUTODIA] undone, {} word(s) kept as typed from now on", changed.len());
        true
    }
}

#[cfg(test)]
#[path = "autodia_tests.rs"]
mod autodia_tests;

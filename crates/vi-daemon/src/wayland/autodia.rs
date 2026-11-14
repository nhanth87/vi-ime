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
    /// Leading words that are context only (the end of the clause before a
    /// `,`, already fixed): read by the table, never rewritten.
    frozen: usize,
    /// The key before this one was plain typing.
    keep: bool,
    ctx: u64,
    /// (fixed tail, typed tail, typed words that changed).
    undo: Option<(String, String, Vec<String>)>,
}

static PHRASE: Mutex<Phrase> = Mutex::new(Phrase { words: Vec::new(), frozen: 0, keep: false, ctx: 0, undo: None });

fn with<R: Default>(f: impl FnOnce(&mut Phrase) -> R) -> R {
    PHRASE.try_lock().map(|mut p| f(&mut p)).unwrap_or_default()
}

/// Start of every non-modifier key.
pub(crate) fn begin_key() {
    with(|p| {
        let ctx = predict::CONTEXT_GEN.load(Ordering::Relaxed);
        if !p.keep || p.ctx != ctx {
            p.words.clear();
            p.frozen = 0;
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

/// A key passed to the app as is: a space or a digit after a word extends the phrase.
pub(crate) fn passthrough(c: Option<char>) {
    with(|p| {
        p.undo = None;
        // Spaces and digits typed between words are separators on screen
        // ("toi co 2 con meo": the number must not cut the sentence in two).
        if let (Some(ch), Some(last)) = (c.filter(|c| *c == ' ' || c.is_ascii_digit()), p.words.last_mut()) {
            last.1.push(ch);
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
pub(crate) fn on_word(text: &str, c: char) -> Option<(String, String)> {
    with(|p| {
        p.undo = None;
        if text.is_empty() || !text.chars().all(char::is_alphabetic) {
            p.words.clear();
            p.frozen = 0;
            return None;
        }
        let push = |p: &mut Phrase, word: String, sep: String| {
            p.words.push((word, sep));
            if p.words.len() > PHRASE_CAP {
                p.words.drain(..p.words.len() - PHRASE_KEEP);
                p.frozen = 0;
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
        match end(p, all, c) {
            Ok(plan) => Some(plan),
            Err(keep) => {
                if keep {
                    // Failed (unknown word, table off…): keep the whole
                    // sentence for the next sentence end. The separator IS
                    // the punctuation: it is on screen, and a later rewrite
                    // that starts before it must count it.
                    push(p, text.to_string(), c.to_string());
                }
                None
            }
        }
    })
}

/// `,` `.` `?` `!` right after a number that follows words ("o so 15,"):
/// the phrase ends there as well. Same plan as `on_word`.
fn on_number_end(c: char) -> Option<(String, String)> {
    with(|p| {
        let after_number = p.words.last().is_some_and(|(_, sep)| sep.ends_with(|ch: char| ch.is_ascii_digit()));
        if !matches!(c, ',' | '.' | '?' | '!') || !after_number {
            return None;
        }
        match end(p, p.words.clone(), c) {
            Ok(plan) => Some(plan),
            Err(keep) => {
                if let (true, Some(last)) = (keep, p.words.last_mut()) {
                    last.1.push(c);
                    p.keep = true;
                }
                None
            }
        }
    })
}

/// Fix the phrase `all` (each word with the separator typed after it) that
/// ends at `c`. Err(true): nothing to rewrite, keep the phrase as context;
/// Err(false): phrase dropped.
fn end(p: &mut Phrase, all: Vec<(String, String)>, c: char) -> Result<(String, String), bool> {
    let typed: Vec<String> = all.iter().map(|(w, _)| w.clone()).collect();
    let mut fixed = diacritic::fix_at(&typed, p.frozen == 0).ok_or(true)?;
    fixed[..p.frozen].clone_from_slice(&typed[..p.frozen]);
    let first = typed.iter().zip(&fixed).position(|(a, b)| a != b).ok_or(true)?;
    let tail = |ws: &[String]| -> String {
        ws[first..].iter().zip(&all[first..]).map(|(w, (_, sep))| format!("{w}{sep}")).collect()
    };
    let (old, new) = (tail(&typed), tail(&fixed));
    p.words.clear();
    p.frozen = 0;
    if old.chars().count() > MAX_TAIL {
        return Err(false); // too messy to rewrite — start the next phrase over
    }
    let changed = typed.iter().zip(&fixed).filter(|(a, b)| a != b).map(|(a, _)| a.clone()).collect();
    // Consumed by the successful fix. After a comma the sentence goes on:
    // its last word stays as left context for the next clause ("…, ban o
    // lai." read "ban" with nothing before it and chose "bản").
    if let (',', Some(last), Some((_, sep))) = (c, fixed.last(), all.last()) {
        p.words.push((last.clone(), format!("{sep}{c}")));
        p.frozen = 1;
    }
    p.undo = Some((format!("{new}{c}"), format!("{old}{c}"), changed));
    p.keep = true;
    Ok((old, new))
}

/// The pending undo, if the key before this one was the fix:
/// (fixed tail, typed tail, typed words that changed).
pub(crate) fn take_undo() -> Option<(String, String, Vec<String>)> {
    with(|p| p.undo.take())
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
        let keys = self.shown_word.chars().count() + target.chars().count();
        self.sync_shown(&target);
        self.expect_echo(keys);
        if self.shown_word == target {
            info!("[AUTODIA] phrase tail retyped ({} chars)", target.chars().count());
        } else {
            clear(); // typer refused: the screen still shows the typed text
        }
    }

    /// A rewrite of `keys` keys comes back from the app as that many text
    /// changes. Until they have passed they are ours, not an external edit
    /// (`dispatch.rs` Done: `recent_live`), so they must not drop the undo
    /// or the next word.
    fn expect_echo(&mut self, keys: usize) {
        let ms = (keys as u64 * 15).min(1500);
        self.last_live_echo_at = Some(std::time::Instant::now() + std::time::Duration::from_millis(ms));
    }

    /// `,` `.` `?` `!` typed right after a number ending a phrase ("o so
    /// 15,"): rewrite the phrase, punctuation included. True = typed here.
    pub(crate) fn number_end(&mut self, c: Option<char>) -> bool {
        let inline = self.runtime.as_ref().is_some_and(|rt| rt.inline_complete());
        let live = self.live_echo() && !self.app_surrounding_capable();
        let Some(c) = c.filter(|_| diacritic::enabled() && self.learns() && live && !inline) else { return false };
        let Some((old, new)) = on_number_end(c) else { return false };
        let target = format!("{new}{c}");
        let keys = old.chars().count() + target.chars().count();
        self.shown_word = old; // on screen: the typed tail, number included
        self.sync_shown(&target);
        self.expect_echo(keys);
        if self.shown_word == target {
            info!("[AUTODIA] phrase tail retyped ({} chars)", target.chars().count());
        } else {
            clear();
        }
        self.reset_word_state();
        true
    }

    /// BackSpace right after a fix: put the typed text back. True = handled.
    pub(crate) fn undo_diacritics(&mut self) -> bool {
        let Some((fixed, typed, changed)) = with(|p| p.undo.take()) else { return false };
        let keys = fixed.chars().count() + typed.chars().count();
        self.shown_word = fixed;
        self.sync_shown(&typed);
        self.expect_echo(keys);
        self.reset_word_state();
        // Only a one-word fix names the word the user rejected. Keeping every
        // word of a longer phrase switched the feature off for "toi", "di",
        // "hoc"… after a single undo (nested test 2026-11-13).
        if changed.len() == 1 {
            diacritic::keep(&changed);
        }
        clear();
        info!("[AUTODIA] undone ({} word(s) had changed)", changed.len());
        true
    }
}

#[cfg(test)]
#[path = "autodia_tests.rs"]
mod autodia_tests;

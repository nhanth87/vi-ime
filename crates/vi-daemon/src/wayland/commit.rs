// SPDX-License-Identifier: GPL-3.0-or-later OR Commercial
// Copyright (c) 2024-2026 vi-im contributors
//! Commit path: two-phase delete+append (R7) for the live model.
//! Split from state.rs to keep both under the 300-line rule (R4).

use std::time::Instant;

use tracing::info;
use wayland_protocols_misc::zwp_input_method_v2::client::zwp_input_method_v2::ZwpInputMethodV2;
use crate::wayland::state::ImeAppState;

/// Second half of a two-phase commit (R7): run after the compositor
/// acknowledges the phase-1 `delete_surrounding_text` with `done`.
///
/// Live model: the app always shows the current rendered form of the word.
/// Phase-1 deleted the changed suffix; phase-2 commits `append` so the app
/// ends up showing `word_after`. `finalize` marks the word as done (stop
/// tracking it as editable); `replay` is a boundary/control key to re-inject
/// after the commit (space, Enter, punctuation…).
pub(crate) struct Phase2 {
    pub(crate) append: String,
    pub(crate) word_after: String,
    pub(crate) finalize: bool,
    pub(crate) replay: Option<u32>,
}

impl ImeAppState {
    /// Clear the waiting flag and run the deferred phase-2 (from "done",
    /// timeout, or deactivate).
    pub(crate) fn finish_waiting_and_run_phase2(&mut self) {
        self.waiting_for_done = false;
        self.waiting_since = None;
        let Some(im) = self.input_method.clone() else {
            self.pending_phase2 = None;
            return;
        };
        let Some(p2) = self.pending_phase2.take() else { return };
        if !p2.append.is_empty() {
            info!("[COMMIT] phase-2: append \"{}\" (serial={})", p2.append, self.serial);
            im.commit_string(p2.append.clone());
            im.commit(self.serial);
        }
        self.committed_word = if p2.finalize { String::new() } else { p2.word_after };
        if let Some(kc) = p2.replay {
            // The real press/release of the boundary key was consumed by the
            // engine — synthesize it now, after the word is committed.
            self.vk.tap(kc);
        }
    }

    /// Make the app show `target` for the current word (live model).
    ///
    /// Diffs against the text we already own and touches only the changed
    /// suffix: delete `del` BYTES (protocol counts bytes, and our committed
    /// text is multi-byte Vietnamese), then commit the appended suffix.
    /// A needed delete is a two-phase commit (R7): the append waits for `done`.
    ///
    /// PARKED (2026-07-09): field-test showed real apps ignore
    /// delete_surrounding_text while still acking `done` — only re-enable
    /// behind a per-app verify loop (docs/fix-plan P0-3 hướng a).
    #[allow(dead_code)]
    pub(crate) fn sync_word(
        &mut self,
        im: &ZwpInputMethodV2,
        target: String,
        finalize: bool,
        replay: Option<u32>,
    ) {
        let common = common_prefix_bytes(&self.committed_word, &target);
        let del = (self.committed_word.len() - common) as u32;
        let append = target[common..].to_string();
        if del == 0 {
            // Pure append: no delete, no roundtrip to wait for.
            if !append.is_empty() {
                im.commit_string(append);
                im.commit(self.serial);
            }
            self.committed_word = if finalize { String::new() } else { target };
            if let Some(kc) = replay {
                self.vk.tap(kc);
            }
            return;
        }
        // Two-phase: delete the changed suffix, defer the append until `done`.
        info!("[COMMIT] phase-1: delete_surrounding_text({del} bytes) then append \"{append}\"");
        im.delete_surrounding_text(del, 0);
        im.commit(self.serial);
        self.waiting_for_done = true;
        self.waiting_since = Some(Instant::now());
        self.pending_phase2 = Some(Phase2 { append, word_after: target, finalize, replay });
    }

    /// Finalize the in-progress word (arrows, shortcuts, reconfigure…).
    ///
    /// - Preedit (and NonPreedit without a Vietnamese keyboard): the
    ///   composition only exists as preedit — turn it into real text via
    ///   commit_string (replaces preedit per protocol spec).
    /// - Live mode (P0-3): the raw keys were live-forwarded and ARE real
    ///   text in the app. Committing again would DUPLICATE the word — the
    ///   raw form stays on screen (classic-IME behavior for an interrupted
    ///   word), we just stop tracking it.
    pub(crate) fn finalize_word(&mut self, im: &ZwpInputMethodV2) {
        let live =
            self.engine.mode() == crate::engine::ImeMode::NonPreedit && self.viet.ready();
        if self.engine.has_pending() && !live {
            let text = self.engine.inner().preedit_output();
            im.commit_string(text);
            im.commit(self.serial);
        }
        self.engine.reset();
        self.reset_word_state();
    }

    /// Finalize pending text, then replay a key (used by R8 reconfigure and
    /// by shortcut/navigation passthrough).
    pub(crate) fn commit_pending_then(&mut self, im: &ZwpInputMethodV2, replay: Option<u32>) {
        self.finalize_word(im);
        if let Some(kc) = replay {
            self.vk.tap(kc);
        }
    }

    /// Send a preedit update (classic Preedit mode only). Protocol cursor
    /// offsets are UTF-8 BYTES; (len, len) puts the caret at the end.
    pub(crate) fn set_preedit(&mut self, im: &ZwpInputMethodV2, s: &str) {
        if s.is_empty() {
            im.set_preedit_string(String::new(), 0, 0);
        } else {
            let end = s.len() as i32;
            im.set_preedit_string(s.to_string(), end, end);
        }
        im.commit(self.serial);
    }

    /// Forget per-word bookkeeping (after commit/cancel/deactivate).
    /// NOTE: clears the diff bases WITHOUT touching the app — on a cancel
    /// (click, focus change) whatever is on screen stays where it was.
    pub(crate) fn reset_word_state(&mut self) {
        self.committed_word.clear();
        self.shown_word.clear();
    }
}

/// Byte length of the longest common prefix, kept on char boundaries so the
/// resulting delete/append never splits a UTF-8 sequence.
#[allow(dead_code)] // parked with sync_word
fn common_prefix_bytes(a: &str, b: &str) -> usize {
    let mut idx = 0;
    let mut ai = a.chars();
    let mut bi = b.chars();
    while let (Some(x), Some(y)) = (ai.next(), bi.next()) {
        if x != y {
            break;
        }
        idx += x.len_utf8();
    }
    idx
}
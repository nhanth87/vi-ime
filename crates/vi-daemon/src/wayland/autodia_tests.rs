// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Phrase-tracking smoke tests (R37): accumulation + phrase-end gating.
//!
//! Single test fn: PHRASE is process-global, and `ImeAppState` methods
//! (`commit_boundary`, `undo_diacritics`) need a live Wayland connection,
//! so they are covered by field test, not here.

use super::*;

#[test]
fn phrase_accumulates_spaces_and_drops_at_end() {
    clear();
    begin_key();
    typing();
    // Space boundary: word kept for the phrase, no rewrite plan.
    assert!(on_word("toi", ' ').is_none());
    assert!(on_word("di", ' ').is_none());
    // Phrase end with the table off in tests: no plan, phrase consumed.
    assert!(on_word("hoc", '.').is_none());
    // Non-letters break the phrase instead of reaching the table.
    assert!(on_word("toi", ' ').is_none());
    assert!(on_word("car!", ',').is_none());
    // Helpers never panic on empty state.
    begin_key();
    typing();
    passthrough(Some(' '));
    passthrough(None);
    passthrough(Some('x'));
    clear();
}

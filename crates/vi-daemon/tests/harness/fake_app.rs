// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! P1: Fake text-input-v3 client that simulates what a real app does.
//!
//! The fake app:
//! 1. Connects to the compositor, enables `zwp_text_input_v3`
//! 2. Records every `commit_string`, `delete_surrounding_text`, `preedit_string`
//! 3. Builds a buffer that mirrors the *actual screen text* the user would see
//! 4. On `Done`, applies the batch atomically (just like a real toolkit)
//! 5. Optionally announces `surrounding_text` (toggle: with/without for P0 paths)
//!
//! The key invariant this tests: after driving a sequence of keys through
//! vi-ime, `fake_app.buffer` MUST match the expected Vietnamese text.
//! Unit tests test the engine; THIS catches the render-race that only
//! manifests with real protocol ordering (5 field-bug rounds in AGENTS.md).

pub struct FakeApp {
    /// The real on-screen text (after all commits + deletes applied).
    pub buffer: String,
    /// Cursor position in `buffer` (byte offset).
    pub cursor: usize,
    /// Pending preedit text (not yet committed — like toolkit's preedit layer).
    preedit: String,
    /// Whether to announce `surrounding_text` on `Done`.
    /// `true` = P0 atomic path. `false` = VietTyper fallback.
    pub announce_surrounding: bool,
}

impl FakeApp {
    pub fn new(announce_surrounding: bool) -> Self {
        Self {
            buffer: String::new(),
            cursor: 0,
            preedit: String::new(),
            announce_surrounding,
        }
    }

    /// Called by the test when vi-ime sends `commit_string(text)`.
    /// Replaces any active preedit with the committed text.
    pub fn commit_string(&mut self, text: &str) {
        // Remove preedit region first (if any), then insert new committed text.
        let preedit_len = self.preedit.len();
        if preedit_len > 0 {
            let start = self.cursor.saturating_sub(preedit_len);
            self.buffer.replace_range(start..self.cursor, "");
            self.cursor = start;
            self.preedit.clear();
        }
        self.buffer.insert_str(self.cursor, text);
        self.cursor += text.len();
    }

    /// Called when vi-ime sends `delete_surrounding_text(before, after)`.
    /// Removes `before` bytes left of cursor and `after` bytes right.
    pub fn delete_surrounding_text(&mut self, before: u32, after: u32) {
        self.remove_preedit();
        let b = before as usize;
        let a = after as usize;
        let s = self.cursor.saturating_sub(b);
        let e = (self.cursor + a).min(self.buffer.len());
        self.buffer.replace_range(s..e, "");
        self.cursor = s;
    }

    /// Called when vi-ime sends `set_preedit_string(text, cursor_begin, cursor_end)`.
    pub fn set_preedit(&mut self, text: &str, _cursor_begin: i32, _cursor_end: i32) {
        self.remove_preedit();
        self.buffer.insert_str(self.cursor, text);
        self.preedit = text.to_string();
        self.cursor += text.len(); // advance past the inserted preedit
    }

    /// Called on `Done` event. Applies the batch and optionally announces
    /// surrounding text.
    pub fn done(&mut self) -> Option<(String, u32, u32)> {
        if self.announce_surrounding {
            // Announce the buffer state (utf-8 text + cursor + anchor).
            // cursor = anchor (no selection)
            let before = self.cursor as u32;
            let after = (self.buffer.len() - self.cursor) as u32;
            let text = self.buffer.clone();
            Some((text, before, after))
        } else {
            None
        }
    }

    /// On `Deactivate` or clear: remove preedit, reset cursor.
    pub fn deactivate(&mut self) {
        self.remove_preedit();
        self.preedit.clear();
        self.cursor = 0;
    }

    fn remove_preedit(&mut self) {
        if !self.preedit.is_empty() {
            let len = self.preedit.len();
            let start = self.cursor.saturating_sub(len);
            self.buffer.replace_range(start..self.cursor, "");
            self.cursor = start;
            self.preedit.clear();
        }
    }

    /// Full visible text (what the user sees on screen).
    pub fn visible_text(&self) -> &str {
        &self.buffer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_app_basic_commit() {
        let mut app = FakeApp::new(true);
        app.commit_string("xin");
        assert_eq!(app.buffer, "xin");
        assert_eq!(app.cursor, 3);
    }

    #[test]
    fn fake_app_delete_surrounding() {
        let mut app = FakeApp::new(true);
        app.commit_string("xin chào");
        // Move cursor to end of "xin " (4 bytes)
        app.cursor = 4;
        app.delete_surrounding_text(4, 0);
        assert_eq!(app.buffer, "chào");
        assert_eq!(app.cursor, 0);
    }

    #[test]
    fn fake_app_preedit_then_commit() {
        let mut app = FakeApp::new(true);
        app.set_preedit("nhâ", 0, 3);
        assert_eq!(app.buffer, "nhâ");
        app.commit_string("nhà");
        assert_eq!(app.buffer, "nhà");
    }
}

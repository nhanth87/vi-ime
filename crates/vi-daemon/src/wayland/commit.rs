// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Word finalization: turn the in-progress composition into real text.
//! Split from state.rs to keep both under the 300-line rule (R4).

use wayland_protocols_misc::zwp_input_method_v2::client::zwp_input_method_v2::ZwpInputMethodV2;
use crate::wayland::state::ImeAppState;

impl ImeAppState {
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
        let live = self.live_echo();
        if self.engine.has_pending() && !live {
            let text = self.engine.inner().preedit_output();
            im.commit_string(text);
            im.commit(self.serial);
        }
        self.engine.reset();
        self.reset_word_state();
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

    /// Word suggestions learn only from normal text fields — never
    /// terminals (commands) or the gated Secure/Url/Numeric fields.
    pub(crate) fn learns(&self) -> bool {
        self.field_sensitivity == crate::wayland::state::FieldSensitivity::Normal
    }

    /// Alt+1/2/3 (Alt alone, evdev KEY_1..KEY_3) → suggestion 0/1/2.
    pub(crate) fn suggestion_slot(&self, keycode: u32) -> Option<usize> {
        let alt_only = self.xkb.is_mod_active("Mod1")
            && !self.xkb.is_control_active()
            && !self.xkb.is_mod_active("Mod4");
        (alt_only && (2..=4).contains(&keycode)).then(|| (keycode - 2) as usize)
    }

    /// Put suggestion `i` in place of the word being typed, plus a space.
    /// False when the bar shows no suggestion `i` — the chord then goes to
    /// the app as a normal shortcut.
    pub(crate) fn accept_suggestion(&mut self, im: &ZwpInputMethodV2, i: usize) -> bool {
        let typed = self.engine.inner().preedit_output();
        let Some(word) = crate::engine::predict::accept(i, &typed) else { return false };
        // The app saw Alt go down; a Ctrl tap before its release keeps that
        // release from reading as a bare Alt tap (Firefox/GTK menubar).
        self.vk.tap(29);
        let text = format!("{word} ");
        if self.live_echo() && self.app_surrounding_capable() {
            self.live_commit(im, &text);
        } else if self.live_echo() {
            self.sync_shown(&text);
        } else {
            // Preedit: replaces the preedit (protocol); silent: nothing shown.
            im.commit_string(text);
            im.commit(self.serial);
        }
        self.engine.reset();
        self.reset_word_state();
        crate::engine::predict::observe(&word);
        true
    }

    /// Forget per-word bookkeeping (after commit/cancel/deactivate).
    /// NOTE: clears the diff base WITHOUT touching the app — on a cancel
    /// (click, focus change) whatever is on screen stays where it was.
    pub(crate) fn reset_word_state(&mut self) {
        self.shown_word.clear();
        self.committed_bytes = 0;
    }
}

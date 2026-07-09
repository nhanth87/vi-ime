// SPDX-License-Identifier: GPL-3.0-or-later OR Commercial
// Copyright (c) 2024-2026 vi-im contributors
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
    /// NOTE: clears the diff base WITHOUT touching the app — on a cancel
    /// (click, focus change) whatever is on screen stays where it was.
    pub(crate) fn reset_word_state(&mut self) {
        self.shown_word.clear();
    }
}

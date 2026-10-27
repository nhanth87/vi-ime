// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Modifier bookkeeping between the grabbed keyboard and the app, and
//! Alt+1..3 on the evdev path (office apps): accept a word suggestion
//! (engine/predict.rs). Child of `evdev_compose` (R4: that file is full).

use super::*;

/// Alt bits in `Composer::sysmods` (left, right).
const ALT: u8 = 4 | 8;

impl Composer {
    /// Emit releases for any modifiers the uinput mirror still holds — on
    /// disengage (focus left) the user's real release lands AFTER ungrab
    /// and never reaches the mirror, which would pin Super/Ctrl forever
    /// (same class as the vk1 stuck-Super bug, virtual_keyboard.rs).
    pub(crate) fn release_mods(&mut self, ui: &mut VirtualDevice) {
        if self.shift {
            self.forward(ui, KeyCode::KEY_LEFTSHIFT, 0);
            self.forward(ui, KeyCode::KEY_RIGHTSHIFT, 0);
            self.shift = false;
        }
        const MODS: [(u8, KeyCode); 6] = [
            (1, KeyCode::KEY_LEFTCTRL),
            (2, KeyCode::KEY_RIGHTCTRL),
            (4, KeyCode::KEY_LEFTALT),
            (8, KeyCode::KEY_RIGHTALT),
            (16, KeyCode::KEY_LEFTMETA),
            (32, KeyCode::KEY_RIGHTMETA),
        ];
        for (bit, code) in MODS {
            if self.sysmods & bit != 0 {
                self.forward(ui, code, 0);
            }
        }
        self.sysmods = 0;
    }

    /// Alt+1/2/3 with Alt alone → suggestion 0/1/2 replaces the word being
    /// typed, plus a space. True when the key was consumed.
    pub(super) fn accept_suggestion(&mut self, ui: &mut VirtualDevice, code: KeyCode) -> bool {
        let i = match code {
            KeyCode::KEY_1 => 0,
            KeyCode::KEY_2 => 1,
            KeyCode::KEY_3 => 2,
            _ => return false,
        };
        if !self.learn || self.sysmods & ALT == 0 || self.sysmods & !ALT != 0 {
            return false;
        }
        let typed = self.engine.inner().preedit_output();
        let Some(word) = predict::accept(i, &typed) else { return false };
        self.flush();
        // The app holds the forwarded Alt. Tap Ctrl first so the release is
        // not a bare Alt tap (LibreOffice/OnlyOffice open the menu bar on
        // one), then release Alt app-side so the word isn't read as
        // Alt+letters. The physical Alt stays in `sysmods` — Alt+2 right
        // after still accepts; `restore_alt` re-presses it for any other
        // chord, and its real release is dropped (no longer in `down`).
        self.forward_tap(ui, KeyCode::KEY_LEFTCTRL);
        for alt in [KeyCode::KEY_LEFTALT, KeyCode::KEY_RIGHTALT] {
            if self.down.contains(&alt.code()) {
                self.forward(ui, alt, 0);
            }
        }
        self.sync_shown(&format!("{word} "), true);
        self.engine.reset();
        self.shown.clear();
        self.common_bytes = 0;
        predict::observe(&word);
        true
    }

    /// Before forwarding another chord key: give the app back the Alt that
    /// `accept_suggestion` released while the user still holds it.
    pub(super) fn restore_alt(&mut self, ui: &mut VirtualDevice) {
        for (bit, alt) in [(4, KeyCode::KEY_LEFTALT), (8, KeyCode::KEY_RIGHTALT)] {
            if self.sysmods & bit != 0 && !self.down.contains(&alt.code()) {
                self.forward(ui, alt, 1);
            }
        }
    }
}

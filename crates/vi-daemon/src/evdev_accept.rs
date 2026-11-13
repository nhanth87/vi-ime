// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Modifier bookkeeping between the grabbed keyboard and the app, and
//! Alt+1..3 on the evdev path (office apps): accept a word suggestion
//! (engine/predict.rs). Child of `evdev_compose` (R4: that file is full).

use super::*;
use crate::engine::diacritic;
use crate::wayland::autodia;

/// Alt bits in `Composer::sysmods` (left, right).
const ALT: u8 = 4 | 8;

impl Composer {
    /// Runtime switches the composer's engine follows (emoji, Tự do raw display).
    pub(crate) fn apply_runtime(&mut self, snap: &crate::wayland::runtime::RuntimeSnapshot) {
        self.engine.set_emoji_enabled(snap.emoji);
        self.engine.inner_mut().set_raw_incomplete(snap.raw_incomplete);
    }

    /// Enable emoji shortcode/emoticon expansion on the composer's engine.
    pub(crate) fn set_emoji_enabled(&mut self, enabled: bool) {
        self.engine.set_emoji_enabled(enabled);
    }

    /// Publish fresh R-macro tables (pointer-checked inside — ~free).
    pub(crate) fn set_macros(&mut self, macros: std::sync::Arc<crate::engine::macro_dict::MacroDict>) {
        self.engine.set_macros(macros);
    }

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

    /// Backspace OUTSIDE composition: the screen text was typed through
    /// the virtual-keyboard typer, NOT uinput. Forwarding raw backspace
    /// through uinput (`emit`) lands on a different device than what
    /// produced the text → compositor/client may reject/drop it, so the
    /// second backspace (or backspace past a deleted word) stops working.
    /// Route through the SAME typer path instead, with roundtrip for
    /// ordering guarantees (field bug 2026-07-15: backspace stops after
    /// 1 char outside of a composed word).
    pub(super) fn backspace_outside(&mut self) {
        // Force-settle any leftover shown tracking first.
        if !self.shown.is_empty() {
            self.finish_word();
        }
        // R37: right after a diacritic fix, BackSpace puts the typed text back.
        if let Some((fixed, typed, changed)) = autodia::take_undo() {
            self.retype(&fixed, &typed);
            if changed.len() == 1 {
                diacritic::keep(&changed);
            }
            autodia::clear();
            info!("[AUTODIA] undone ({} word(s) had changed)", changed.len());
            return;
        }
        crate::engine::habit::backspace();
        self.type_out(1, "", true);
    }

    /// A printable boundary `c` after `word` (None: nothing was composing).
    /// At a phrase end the changed tail is retyped with `c`, all on the
    /// typer (R26, R37). Office apps only, and only with
    /// `auto_diacritics_office` — this path has had no field test yet.
    pub(super) fn boundary(&mut self, word: Option<String>, c: char) {
        let plan = match word {
            Some(w) if self.learn && diacritic::office_enabled() => autodia::on_word(&w, c),
            Some(_) => {
                autodia::clear();
                None
            }
            None => {
                autodia::passthrough(Some(c));
                None
            }
        };
        match plan {
            Some((old, new)) => {
                self.retype(&old, &format!("{new}{c}"));
                info!("[AUTODIA] phrase tail retyped ({} chars)", new.chars().count() + 1);
            }
            None => self.type_out(0, &c.to_string(), false),
        }
    }

    /// Turn the last chars on screen from `old` into `new`, keeping their
    /// common start: the app may have capitalised it itself (office
    /// autocorrect), and fewer keys is faster.
    fn retype(&mut self, old: &str, new: &str) {
        let common = old.chars().zip(new.chars()).take_while(|(a, b)| a == b).count();
        let suffix: String = new.chars().skip(common).collect();
        self.type_out(old.chars().count() - common, &suffix, true);
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

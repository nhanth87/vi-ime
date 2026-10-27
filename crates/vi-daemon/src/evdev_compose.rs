// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Live composer for the evdev fallback (split from evdev_mode.rs, R4).
//!
//! LIVE echo model: letter/tone keys are consumed from the grabbed keyboard
//! and the *rendered* word is kept on screen key by key — each keystroke
//! diffs what's shown against the engine render and sends BackSpace × k +
//! the new suffix through the Unicode typer. Keys pressed while
//! Ctrl/Alt/Super is held are forwarded verbatim (Ctrl+A must reach the
//! app, not the engine — field bug 2026-07-10).

use std::time::{Duration, Instant};

use evdev::uinput::VirtualDevice;
use evdev::{EventType, InputEvent, KeyCode};
use tracing::info;

use crate::engine::fast_engine::NonPreeditEngine;
use crate::engine::predict;
use crate::engine::{ImeMode, InputMethod, NonPreeditAction};
use crate::evdev_inject::{boundary_char, hotkey_key, key_to_char, Typer};

/// Live composer: the engine plus what is currently on screen for the
/// in-progress word. Shared by `run` (--evdev) and `run_scoped` (auto
/// legacy fallback) so the two paths can never diverge again.
pub(crate) struct Composer {
    engine: NonPreeditEngine,
    shown: String,
    typer: Typer,
    shift: bool,
    /// Bitmask of held system modifiers (Ctrl/Alt/Super).
    sysmods: u8,
    /// Byte count of the common prefix between `shown` and the next target.
    /// Monotonic (never decreases within a word) except when a tone mark
    /// changes a vowel inside the prefix — that rare case is detected and
    /// falls back to a zero-based recompute. Avoids re-collecting into
    /// Vec<char> + zip on every keystroke (opt 3).
    common_bytes: usize,
    /// When the last key PRESS went out through the uinput mirror — see
    /// `type_out` (cross-channel ordering barrier).
    last_uinput_press: Option<Instant>,
    /// Keys whose PRESS went out through the uinput mirror and whose release
    /// hasn't yet — only these get their release forwarded (see `handle`).
    down: std::collections::HashSet<u16>,
    /// Coalescing: the latest mid-word target not typed yet. Composing keys
    /// only update it; `flush` (end of a drained batch, or before anything
    /// that must stay ordered) types the whole difference in ONE typer
    /// command. A slow typer (OnlyOffice's 150ms accent settle) then costs
    /// one wait per BATCH instead of per key — user report 2026-10-24
    /// "gõ trong OnlyOffice rất chậm" with per-key edits queuing up.
    deferred: Option<String>,
    /// Feed word suggestions (engine/predict.rs) — office apps only: the
    /// evdev path can't see password fields, so never for browsers.
    pub(crate) learn: bool,
}

/// How long a key press forwarded through the uinput mirror needs to reach
/// the app before the typer may write after it. The two channels race:
/// uinput → kernel → libinput → compositor → (XWayland) → app, while
/// `xdotool` talks to the X server directly and the native typer is a
/// separate Wayland client. Without this, "xin chào" typed fast can come
/// out "xinc hào": the 'c' typed by the injector overtakes the Space still
/// in transit through the compositor (race found by code reading on
/// Hyprland + Chrome X11, 2026-10-23 — not yet field-reproduced).
const UINPUT_SETTLE: Duration = Duration::from_millis(30);

/// Remaining wait before the typer may write after a uinput press at `last`.
fn settle_remaining(last: Option<Instant>, now: Instant) -> Duration {
    last.map_or(Duration::ZERO, |t| UINPUT_SETTLE.saturating_sub(now.duration_since(t)))
}

/// Bit for a system-modifier key (Ctrl/Alt/Super), 0 for everything else.
/// Shift is NOT here — it composes uppercase letters, it doesn't chord.
fn sysmod_bit(code: KeyCode) -> u8 {
    match code {
        KeyCode::KEY_LEFTCTRL => 1,
        KeyCode::KEY_RIGHTCTRL => 2,
        KeyCode::KEY_LEFTALT => 4,
        KeyCode::KEY_RIGHTALT => 8,
        KeyCode::KEY_LEFTMETA => 16,
        KeyCode::KEY_RIGHTMETA => 32,
        _ => 0,
    }
}

impl Composer {
    pub(crate) fn new(method: InputMethod, typer: Typer) -> Self {
        Self {
            engine: NonPreeditEngine::new(method, ImeMode::NonPreedit),
            shown: String::new(),
            typer,
            shift: false,
            sysmods: 0,
            common_bytes: 0,
            last_uinput_press: None,
            down: std::collections::HashSet::new(),
            deferred: None,
            learn: false,
        }
    }

    /// Enable emoji shortcode/emoticon expansion on the composer's engine.
    pub(crate) fn set_emoji_enabled(&mut self, enabled: bool) {
        self.engine.set_emoji_enabled(enabled);
    }

    /// Publish fresh R-macro tables (pointer-checked inside — ~free).
    pub(crate) fn set_macros(&mut self, macros: std::sync::Arc<crate::engine::macro_dict::MacroDict>) {
        self.engine.set_macros(macros);
    }

    /// R-macro hotkey identity for a chord on the grabbed keyboard.
    /// Plain/shift-only keys never qualify.
    fn match_hotkey(&self, code: KeyCode) -> Option<String> {
        let ctrl = self.sysmods & 0b11 != 0;
        let alt = self.sysmods & 0b1100 != 0;
        let sup = self.sysmods & 0b110000 != 0;
        if !(ctrl || alt || sup) {
            return None;
        }
        let key = hotkey_key(code, self.shift)?;
        self.engine.match_hotkey(ctrl, alt, sup, self.shift, &key)
    }

    /// One key event from a grabbed keyboard (value: 0=release 1=press 2=repeat).
    pub(crate) fn handle(&mut self, ui: &mut VirtualDevice, code: KeyCode, value: i32) {
        // Only plain composing keys (letters/digits, mid-word BackSpace)
        // may join the deferred batch; anything else — modifiers forwarded
        // to the app, boundaries, shortcuts — must see the screen up to date
        // first, or the order on screen would change.
        let composing = self.sysmods == 0
            && (key_to_char(code, self.shift).is_some()
                || (code == KeyCode::KEY_BACKSPACE && self.engine.has_pending()));
        if !composing {
            self.flush();
        }
        // Track shift for ASCII casing; forward it (apps need modifiers).
        if matches!(code, KeyCode::KEY_LEFTSHIFT | KeyCode::KEY_RIGHTSHIFT) {
            self.shift = value != 0;
            self.forward(ui, code, value);
            return;
        }

        // System modifiers are TRANSPARENT: track + forward, never touch
        // the word (mirrors MODIFIER_KEYS in the Wayland path).
        let bit = sysmod_bit(code);
        if bit != 0 {
            if value == 0 {
                self.sysmods &= !bit;
            } else {
                self.sysmods |= bit;
            }
            self.forward(ui, code, value);
            return;
        }

        // Shortcut chord (Ctrl/Alt/Super held): settle the word, forward
        // the key VERBATIM so the app's keybinding fires — Ctrl+A must
        // select all, not feed 'a' to the engine (field bug 2026-07-10).
        if self.sysmods != 0 {
            if value != 0 {
                if self.accept_suggestion(ui, code) {
                    return;
                }
                self.restore_alt(ui);
                // R-macro hotkey: chord → expansion, consuming the chord
                // (its release is dropped below — the press never went out). The
                // evdev thread only runs while the IME is enabled
                // (consumer_loop breaks otherwise) — R18 holds.
                if let Some(text) = self.match_hotkey(code) {
                    info!("[MACRO] hotkey → {} chars", text.chars().count());
                    self.finish_word();
                    self.type_out(0, &text, true);
                    return;
                }
                self.finish_word();
            } else if !self.down.contains(&code.code()) {
                return; // press consumed (suggestion/macro): no stray keyup
            }
            self.forward(ui, code, value);
            return;
        }

        // Mid-word Backspace: shrink the composition via the same diff —
        // consumed, NOT forwarded (the screen holds the rendered form,
        // which may be shorter than the raw keys typed).
        if code == KeyCode::KEY_BACKSPACE && self.engine.has_pending() {
            if value != 0 {
                let action = self.engine.push_key('\u{0008}');
                self.apply(ui, code, action);
            }
            return;
        }

        // Backspace OUTSIDE composition: the screen text was typed through
        // the virtual-keyboard typer, NOT uinput. Forwarding raw backspace
        // through uinput (`emit`) lands on a different device than what
        // produced the text → compositor/client may reject/drop it, so the
        // second backspace (or backspace past a deleted word) stops working.
        // Route through the SAME typer path instead, with roundtrip for
        // ordering guarantees (field bug 2026-07-15: backspace stops after
        // 1 char outside of a composed word).
        if code == KeyCode::KEY_BACKSPACE {
            if value != 0 {
                // Force-settle any leftover shown tracking, then issue one
                // raw BackSpace via the typer (the same device that wrote the
                // text) so the compositor respects it.
                if !self.shown.is_empty() {
                    self.finish_word();
                }
                self.type_out(1, "", true);
            }
            return;
        }

        let Some(ch) = key_to_char(code, self.shift) else {
            // Non-composing key (space, Enter, arrows…): the rendered word
            // is ALREADY on screen (live echo) — just stop tracking it.
            if value != 0 {
                self.finish_word();
                // Printable (space, punctuation): type it on the typer's
                // channel, behind the word — see `boundary_char`. The
                // release is then dropped (press never went to uinput).
                if let Some(c) = boundary_char(code, self.shift) {
                    self.type_out(0, &c.to_string(), false);
                    return;
                }
            }
            if value != 0 || self.down.contains(&code.code()) {
                self.forward(ui, code, value);
            }
            return;
        };

        // Letter/digit/tone key: the press is consumed, so its release is
        // too — UNLESS the press went out through the mirror (chord released
        // mid-key), which must not stay stuck down. A release whose press
        // was consumed is NOT a no-op for every app: OnlyOffice's editor saw
        // the stray keyup of "u" (held while "7" turned "tu" into "tư") and
        // dropped the "ư" — field test 2026-10-24 with human rollover timing;
        // replaying the typer's X events alone (no stray keyup) was correct.
        if value == 0 {
            if self.down.contains(&code.code()) {
                self.forward(ui, code, 0);
            }
            return;
        }
        let action = self.engine.push_key(ch);
        self.apply(ui, code, action);
    }

    fn apply(&mut self, ui: &mut VirtualDevice, code: KeyCode, action: NonPreeditAction) {
        match action {
            NonPreeditAction::Buffer | NonPreeditAction::UpdatePreedit(_) => {
                // Live echo: the screen follows the rendered form key by
                // key ("tie" + 'e' → "tiê" instantly). preedit_output
                // applies NFC/NFD (R12) — this is real text, not preedit.
                // sync=false: mid-word, no roundtrip (opt 1).
                // Deferred: typed by `flush` once the queued keys are in.
                let target = self.engine.inner().preedit_output();
                if self.learn {
                    predict::set_prefix(&target);
                }
                self.deferred = Some(target);
            }
            NonPreeditAction::CommitWithBackspace { text, .. } => {
                self.deferred = None; // superseded: diff runs from `shown`
                if self.learn {
                    predict::observe(&text);
                }
                // Screen already shows the word (usually a no-op diff).
                // Replay the boundary key that triggered the commit — the
                // engine never includes it in the committed text.
                // sync=true: word boundary, roundtrip before uinput replay.
                self.sync_shown(&text, true);
                self.shown.clear();
                self.common_bytes = 0;
                self.forward_tap(ui, code);
            }
            NonPreeditAction::CommitEmoji { text, .. } => {
                self.flush();
                // Emoji/flushed-literal on the evdev path. Capture was silent
                // (nothing on screen) and fires only when no VN word is pending
                // → no diff, just type `text`. Do NOT replay `code` (trigger is
                // folded into `text`). NOTE: the NATIVE virtual-keyboard typer's
                // keymap covers only ASCII+Vietnamese, so emoji glyphs type only
                // through the wtype/xdotool injector; on the native path a
                // multi-byte emoji is dropped (documented limit — legacy apps).
                self.type_out(0, &text, true);
                self.shown.clear();
                self.common_bytes = 0;
            }
            NonPreeditAction::ClearPreedit => {
                self.deferred = None;
                predict::set_prefix("");
                self.sync_shown("", true);
                self.common_bytes = 0;
            }
            // Digit at word start (VNI) or digit boundary (Telex): the char
            // was consumed above, so replay it or it would be LOST.
            NonPreeditAction::PassThrough => {
                self.flush();
                self.forward_tap(ui, code);
            }
        }
    }

    /// Type the deferred mid-word target (see `deferred`).
    pub(crate) fn flush(&mut self) {
        if let Some(target) = self.deferred.take() {
            self.sync_shown(&target, false);
        }
    }

    /// Forward one key event through the uinput mirror, stamping presses
    /// for the `type_out` barrier.
    fn forward(&mut self, ui: &mut VirtualDevice, code: KeyCode, value: i32) {
        self.typer.wait_settled();
        emit(ui, code, value);
        // Releases arm the barrier too: a Shift release still in transit
        // when the typer writes the next letter would capitalise it.
        self.last_uinput_press = Some(Instant::now());
        if value != 0 {
            self.down.insert(code.code());
        } else {
            self.down.remove(&code.code());
        }
    }

    /// `tap` through the uinput mirror, stamped like `forward`.
    fn forward_tap(&mut self, ui: &mut VirtualDevice, code: KeyCode) {
        self.typer.wait_settled();
        tap(ui, code);
        self.last_uinput_press = Some(Instant::now());
    }

    /// Every typer write goes through here: wait out `UINPUT_SETTLE` after a
    /// recent uinput press so the typed text can never overtake it. Costs
    /// nothing at normal typing speed (keys ≥30ms apart); only bites when
    /// keys are processed back to back (backlog, fast bursts).
    fn type_out(&mut self, backspaces: usize, text: &str, sync: bool) {
        let wait = settle_remaining(self.last_uinput_press.take(), Instant::now());
        if !wait.is_zero() {
            std::thread::sleep(wait);
        }
        self.typer.backspace_then_type(backspaces, text, sync);
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

    /// Physical click while composing: the cursor moved — stop tracking the
    /// word WITHOUT touching the screen (live text stays where it was
    /// typed; a later diff would backspace at the NEW cursor — R8/R17-C).
    pub(crate) fn click_reset(&mut self) {
        predict::reset_context(); // the cursor moved
        if !self.engine.has_pending() && self.shown.is_empty() {
            return;
        }
        info!("[EVDEV-CLICK] chuột click khi đang gõ dở — drop tracking (R8)");
        self.deferred = None;
        self.engine.reset();
        self.shown.clear();
        self.common_bytes = 0;
    }

    /// A non-composing key ended the word: it is already echoed on screen,
    /// so just forget it (mirrors `finalize_word` in the Wayland live path).
    pub(crate) fn finish_word(&mut self) {
        self.deferred = None; // the commit diff below runs from `shown`
        if self.engine.has_pending() {
            // R9 English-restore at word end: `test`→`test`, not `tét`.
            // Mirrors the NonPreeditEngine boundary sites (fast_engine.rs);
            // english_only never second-guesses valid Vietnamese (`ấ`).
            // R-macro keyword expansion wins over the heuristic (same order
            // as the Wayland path — `commit_text`).
            let target = self.engine.commit_text(None);
            if self.learn {
                predict::observe(&target);
            }
            // sync=true: word ending → roundtrip (next key is cross-channel).
            self.sync_shown(&target, true);
        }
        self.engine.reset();
        self.shown.clear();
        self.common_bytes = 0;
    }

    /// Make the app show `target` for the in-progress word: BackSpace × k
    /// for the divergent tail + the new suffix.
    ///
    /// `sync`: true at word boundary (roundtrip before cross-channel uinput);
    /// false mid-word (flush only — next keystroke is same channel, protocol
    /// ordering is guaranteed, ~80% latency reduction per keystroke).
    fn sync_shown(&mut self, target: &str, sync: bool) {
        // NO-OP guard (opt 5): nothing changed on screen → skip everything.
        if target == self.shown {
            return;
        }

        // Byte-based diff (opt 3): compare from the last known common prefix,
        // not from scratch. Rare case (tone mark changes a vowel inside the
        // prefix → bytes diverge) falls back to a zero-based compare.
        let mut common_bytes = self.common_bytes.min(self.shown.len()).min(target.len());
        if common_bytes > 0
            && self.shown.as_bytes().get(..common_bytes)
                != target.as_bytes().get(..common_bytes)
        {
            // Tone mark shifted the rendered form earlier than expected —
            // rare, just recompute from zero.
            common_bytes = 0;
        }
        while common_bytes < self.shown.len()
            && common_bytes < target.len()
            && self.shown.as_bytes()[common_bytes] == target.as_bytes()[common_bytes]
        {
            common_bytes += 1;
        }
        // Byte compare can stop MID-CHAR when two different composed chars
        // share a UTF-8 prefix — mọi chữ 2-dấu (U+1EA0..U+1EF9) đều bắt đầu
        // E1 BA/BB, nên đổi dấu (ứ→ừ, ề→ệ) chung 2 byte đầu → slicing panics
        // ("byte index is not a char boundary") và giết luôn thread grab.
        // Lùi về ranh giới ký tự của CẢ HAI chuỗi.
        while common_bytes > 0
            && !(self.shown.is_char_boundary(common_bytes)
                && target.is_char_boundary(common_bytes))
        {
            common_bytes -= 1;
        }

        let bs_count = self.shown[common_bytes..].chars().count();
        let suffix = &target[common_bytes..];
        tracing::debug!(
            "[EVDEV-SYNC] shown={:?} → target={target:?} (bs={}, suffix={suffix:?}, sync={sync})",
            self.shown,
            bs_count
        );
        self.type_out(bs_count, suffix, sync);
        self.shown.clear();
        self.shown.push_str(target);
        self.common_bytes = common_bytes;
    }
}

fn emit(ui: &mut VirtualDevice, code: KeyCode, value: i32) {
    // Kernel requires EV_SYN / SYN_REPORT after each event group — without it
    // press/release events can merge, causing stuck modifier keys (field bug
    // 2026-07-10: Super/Ctrl/Shift bị kẹt trong LibreOffice, phải kill vi-ime).
    let events = [
        InputEvent::new(EventType::KEY.0, code.code(), value),
        InputEvent::new(EventType::SYNCHRONIZATION.0, 0, 0),
    ];
    let _ = ui.emit(&events);
}

/// Press + release through the uinput mirror (boundary-key replay).
fn tap(ui: &mut VirtualDevice, code: KeyCode) {
    let events = [
        InputEvent::new(EventType::KEY.0, code.code(), 1),
        InputEvent::new(EventType::SYNCHRONIZATION.0, 0, 0),
        InputEvent::new(EventType::KEY.0, code.code(), 0),
        InputEvent::new(EventType::SYNCHRONIZATION.0, 0, 0),
    ];
    let _ = ui.emit(&events);
}

#[path = "evdev_accept.rs"]
mod accept;

#[cfg(test)]
#[path = "evdev_compose_tests.rs"]
mod barrier_tests;

// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 vi-im contributors
//! Live composer for the evdev fallback (split from evdev_mode.rs, R4).
//!
//! LIVE echo model: letter/tone keys are consumed from the grabbed keyboard
//! and the *rendered* word is kept on screen key by key — each keystroke
//! diffs what's shown against the engine render and sends BackSpace × k +
//! the new suffix through the Unicode typer. Keys pressed while
//! Ctrl/Alt/Super is held are forwarded verbatim (Ctrl+A must reach the
//! app, not the engine — field bug 2026-07-10).

use evdev::uinput::VirtualDevice;
use evdev::{EventType, InputEvent, KeyCode};
use tracing::{info, warn};

use crate::engine::fast_engine::NonPreeditEngine;
use crate::engine::{ImeMode, InputMethod, NonPreeditAction};
use crate::evdev_inject::{key_to_char, Injector};
use crate::evdev_typer::EvdevTyper;

/// Unicode output channel: a persistent virtual keyboard when the
/// compositor supports it (Wayland), else the xdotool fallback (X11).
pub(crate) enum Typer {
    Native(EvdevTyper),
    Cmd(Injector),
}

impl Typer {
    pub(crate) fn detect() -> Option<Self> {
        if let Some(t) = EvdevTyper::new() {
            info!("evdev fallback: Unicode qua virtual keyboard bền vững (native)");
            return Some(Typer::Native(t));
        }
        Injector::detect().map(|inj| {
            info!("evdev fallback: Unicode qua {}", inj.name());
            Typer::Cmd(inj)
        })
    }

    fn backspace_then_type(&mut self, backspaces: usize, text: &str) {
        let ok = match self {
            Typer::Native(t) => t.backspace_then_type(backspaces, text),
            Typer::Cmd(inj) => {
                inj.backspace_then_type(backspaces, text);
                true
            }
        };
        if !ok {
            // shown-tracking is now desynced from the screen for this word;
            // the log is the evidence trail (R17: identify mechanism first).
            warn!("[EVDEV-TYPER] gõ thất bại (bs={backspaces}, text={text:?}) — từ này có thể sai trên màn hình");
        }
    }
}

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
        }
    }

    /// One key event from a grabbed keyboard (value: 0=release 1=press 2=repeat).
    pub(crate) fn handle(&mut self, ui: &mut VirtualDevice, code: KeyCode, value: i32) {
        // Track shift for ASCII casing; forward it (apps need modifiers).
        if matches!(code, KeyCode::KEY_LEFTSHIFT | KeyCode::KEY_RIGHTSHIFT) {
            self.shift = value != 0;
            emit(ui, code, value);
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
            emit(ui, code, value);
            return;
        }

        // Shortcut chord (Ctrl/Alt/Super held): settle the word, forward
        // the key VERBATIM so the app's keybinding fires — Ctrl+A must
        // select all, not feed 'a' to the engine (field bug 2026-07-10).
        if self.sysmods != 0 {
            if value != 0 {
                self.finish_word();
            }
            emit(ui, code, value);
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

        let Some(ch) = key_to_char(code, self.shift) else {
            // Non-composing key (space, Enter, arrows…): the rendered word
            // is ALREADY on screen (live echo) — just stop tracking it,
            // then forward the key.
            if value != 0 {
                self.finish_word();
            }
            emit(ui, code, value);
            return;
        };

        // Letter/digit/tone key: the press is consumed. The RELEASE is
        // forwarded anyway — a release without a press is a no-op for the
        // app, and if the press went through (chord released mid-key) the
        // key must not stay stuck down.
        if value == 0 {
            emit(ui, code, 0);
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
                let target = self.engine.inner().preedit_output();
                self.sync_shown(&target);
            }
            NonPreeditAction::CommitWithBackspace { text, .. } => {
                // Screen already shows the word (usually a no-op diff).
                // Replay the boundary key that triggered the commit — the
                // engine never includes it in the committed text.
                self.sync_shown(&text);
                self.shown.clear();
                tap(ui, code);
            }
            NonPreeditAction::ClearPreedit => self.sync_shown(""),
            // Digit at word start (VNI) or digit boundary (Telex): the char
            // was consumed above, so replay it or it would be LOST.
            NonPreeditAction::PassThrough => tap(ui, code),
        }
    }

    /// Physical click while composing: the cursor moved — stop tracking the
    /// word WITHOUT touching the screen (live text stays where it was
    /// typed; a later diff would backspace at the NEW cursor — R8/R17-C).
    pub(crate) fn click_reset(&mut self) {
        if !self.engine.has_pending() && self.shown.is_empty() {
            return;
        }
        info!("[EVDEV-CLICK] chuột click khi đang gõ dở — drop tracking (R8)");
        self.engine.reset();
        self.shown.clear();
    }

    /// A non-composing key ended the word: it is already echoed on screen,
    /// so just forget it (mirrors `finalize_word` in the Wayland live path).
    pub(crate) fn finish_word(&mut self) {
        if self.engine.has_pending() {
            let target = self.engine.inner().preedit_output();
            self.sync_shown(&target);
        }
        self.engine.reset();
        self.shown.clear();
    }

    /// Make the app show `target` for the in-progress word: BackSpace × k
    /// for the divergent tail + the new suffix, in ONE typer call so the
    /// events cannot interleave with the uinput mirror (the typer blocks
    /// until the compositor has processed them).
    fn sync_shown(&mut self, target: &str) {
        let shown: Vec<char> = self.shown.chars().collect();
        let tgt: Vec<char> = target.chars().collect();
        let common = shown
            .iter()
            .zip(tgt.iter())
            .take_while(|(a, b)| a == b)
            .count();
        let suffix: String = tgt[common..].iter().collect();
        tracing::debug!(
            "[EVDEV-SYNC] shown={:?} → target={target:?} (bs={}, suffix={suffix:?})",
            self.shown,
            shown.len() - common
        );
        self.typer.backspace_then_type(shown.len() - common, &suffix);
        self.shown.clear();
        self.shown.push_str(target);
    }
}

fn emit(ui: &mut VirtualDevice, code: KeyCode, value: i32) {
    let ev = InputEvent::new(EventType::KEY.0, code.code(), value);
    let _ = ui.emit(&[ev]);
}

/// Press + release through the uinput mirror (boundary-key replay).
fn tap(ui: &mut VirtualDevice, code: KeyCode) {
    emit(ui, code, 1);
    emit(ui, code, 0);
}

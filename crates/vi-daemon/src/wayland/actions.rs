// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 vi-im contributors
//! Key processing: keycode → engine action → Wayland/virtual-keyboard output.
//!
//! **Preedit everywhere** — all output uses commit_string / set_preedit_string.
//! No delete_surrounding_text, no terminal guessing. One universal commit
//! path that works on every Wayland app (terminal, browser, editor, game).
//! Split from state.rs to keep both under the 300-line rule (R4).

use crate::engine::{ImeMode, NonPreeditAction};
use tracing::info;
use wayland_client::Connection;
use wayland_protocols_misc::zwp_input_method_v2::client::zwp_input_method_v2::ZwpInputMethodV2;

use crate::wayland::feedback::{ImeFeedback, PipelineStage};
use crate::wayland::state::{FieldSensitivity, ImeAppState};

/// Modifier keycodes (evdev): LCTRL, LSHIFT, RSHIFT, LALT, CAPSLOCK,
/// RCTRL, RALT, LMETA, RMETA. Forwarded transparently, never break a word.
const MODIFIER_KEYS: [u32; 9] = [29, 42, 54, 56, 58, 97, 100, 125, 126];

/// Engine-stage sample (helper keeps the call site short).
fn vi_engine_stage(us: u32) -> ImeFeedback {
    ImeFeedback::StageSample {
        stage: PipelineStage::Engine,
        us,
    }
}

impl ImeAppState {
    /// Handle one pressed key from the buffer. Runs only between commit
    /// sequences (the buffer holds keys while `waiting_for_done`).
    pub(crate) fn process_key(&mut self, keycode: u32, _conn: &Connection) {
        self.maybe_reconfigure();
        let Some(im) = self.input_method.clone() else {
            return;
        };

        // Physical-click guard (evdev watcher): a mouse click moved the
        // cursor but this app sends NO protocol signal for it. Drop the
        // half-typed word BEFORE processing this key so it can never be
        // committed at the click position (R8: Drop, Don't Commit).
        if let Some(rt) = &self.runtime {
            let clicks = rt.clicks();
            if clicks != self.last_clicks {
                self.last_clicks = clicks;
                if self.engine.has_pending() {
                    info!("[CLICK] chuột đã click khi đang gõ dở — drop composition (R8)");
                    self.engine.reset();
                    self.reset_word_state();
                    self.set_preedit(&im, "");
                }
            }
        }

        // ── Game mode toggle: Ctrl+Shift+G ──────────────────────────────
        if self.is_game_mode_toggle(keycode) {
            self.game_mode = !self.game_mode;
            info!("[GAME-MODE] toggled → {}", self.game_mode);
            self.emit(ImeFeedback::StageSample {
                stage: PipelineStage::Engine,
                us: 0,
            });
            return;
        }

        // ── Game mode: raw passthrough, no IME processing ────────────────
        if self.game_mode {
            self.vk.press(keycode);
            return;
        }

        // ── Modifier keys are TRANSPARENT ────────────────────────────────
        // Shift/Ctrl/Alt/Super/CapsLock presses must never touch the word.
        // (They used to fall into the non-text-key branch below and
        // finalize the composition — every Shift press broke the word, so
        // shifted punctuation `< > { } : " + _ *` could never commit it.)
        if MODIFIER_KEYS.contains(&keycode) {
            self.vk.press(keycode);
            return;
        }

        // Shortcut (Ctrl/Alt/Super held): finalize the word and let the raw
        // key through so the app's keybinding fires.
        if self.xkb.is_system_modifier_active() {
            self.finalize_word(&im);
            self.vk.press(keycode);
            return;
        }

        // IME off: pure passthrough. (Normally the grab is already released,
        // but stay correct if a key slips through.)
        if !self.ime_enabled {
            self.vk.press(keycode);
            return;
        }

        // Per-field ContentType gate: password/PIN and numeric fields get
        // raw keys, no composition, no commit_string, no logging.
        if matches!(
            self.field_sensitivity,
            FieldSensitivity::Secure | FieldSensitivity::NumericRaw | FieldSensitivity::Url
        ) {
            self.vk.press(keycode);
            return;
        }

        // Godmod (R6): count backspaces — kept below the Secure gate so no
        // keystrokes are recorded in password/PIN fields (R11b).
        if keycode == 14 {
            // 14 = evdev KEY_BACKSPACE
            crate::godmod::log_backspace();
            // ── Backspace during composition: route through engine ──
            // When the engine is composing a word, backspace must shrink the
            // raw_keys buffer (engine.push_key '\u{0008}') rather than
            // finalize + passthrough, which would commit partial text.
            if self.engine.has_pending() {
                let action = self.engine.push_key('\u{0008}');
                self.apply_action(action, keycode, &im);
                return;
            }
            // No pending composition → pass through to app.
            self.vk.press(keycode);
            return;
        }

        let Some(ch) = self.xkb.keycode_to_char(keycode) else {
            // Non-text key (arrows, F-keys, Home/End…): finalize + passthrough.
            self.finalize_word(&im);
            self.vk.press(keycode);
            return;
        };

        let app_id = self.current_app_id.as_deref();
        // Engine stage: our own processing time. ≥1ms = OUR bug (the
        // engine budget is microseconds) — measured so blame lands here.
        // Sub-timed (P0-1): `engine` = pure compute (plugins + push_key),
        // `apply` = Wayland requests + logging, so a slow sample names the
        // culprit instead of blaming "the engine" as a whole.
        let t0 = std::time::Instant::now();
        let action = if let Some(a) = self.plugin_manager.pre_process_key(ch, false, app_id) {
            a
        } else {
            let a = self.engine.push_key(ch);
            self.plugin_manager.post_process_action(a, app_id)
        };
        let engine_us = t0.elapsed().as_micros().min(u128::from(u32::MAX)) as u32;
        self.apply_action(action, keycode, &im);
        // Arm/refresh the idle auto-commit (state.rs) while composing.
        self.last_key_at = self.engine.has_pending().then(std::time::Instant::now);
        let spent = t0.elapsed();
        if spent.as_millis() >= 1 {
            let us = spent.as_micros().min(u128::from(u32::MAX)) as u32;
            self.emit(vi_engine_stage(us));
        }

        // Godmod (R6): full per-key trace (no-op unless enabled).
        let t_god = std::time::Instant::now();
        crate::godmod::log(
            keycode,
            Some(ch),
            self.engine.mode(),
            "key",
            spent.as_micros().min(u128::from(u64::MAX)) as u64,
            self.engine.inner().raw_key_count(),
            self.engine.has_pending(),
            self.engine.inner().preedit_string(),
        );
        let godmod_us = t_god.elapsed().as_micros().min(u128::from(u32::MAX)) as u32;

        // Spike forensics (P0-1): one warn per slow keystroke with the
        // per-substage breakdown. warn! itself is off the fast path here
        // (only fires on the rare slow sample) and the writer is
        // non-blocking, so this cannot recurse into a new spike.
        let total_us = spent.as_micros().min(u128::from(u32::MAX)) as u32 + godmod_us;
        if total_us >= 5_000 {
            let apply_us = spent.as_micros().min(u128::from(u32::MAX)) as u32 - engine_us;
            tracing::warn!(
                "[SLOW-KEY] total={total_us}µs — engine={engine_us}µs \
                 apply={apply_us}µs godmod={godmod_us}µs (keycode={keycode})"
            );
        }
    }

    /// Apply a NonPreeditAction — two paths:
    ///
    /// - **Preedit**: composing text lives in set_preedit_string; the word
    ///   is finalized with commit_string (replaces preedit per protocol).
    /// - **NonPreedit (Live mode, P0-3 hướng b)**: everything happens on
    ///   the ONE wl_keyboard channel, so ordering can never race. Raw keys
    ///   forward live (the app shows "nha6" as real text — no preedit, no
    ///   underline, nothing lost on a mouse click); at the word boundary:
    ///   Backspace × raw_count, then the composed word is TYPED on the
    ///   second virtual keyboard whose generated keymap carries every
    ///   Vietnamese glyph (viet_typer.rs).
    ///
    /// Two rejected designs live in git^ and docs/fix-plan (commit_string
    /// after vk backspaces → cross-channel reorder; delete_surrounding_text
    /// → silently ignored by real apps). Don't resurrect them.
    pub(crate) fn apply_action(
        &mut self,
        action: NonPreeditAction,
        keycode: u32,
        im: &ZwpInputMethodV2,
    ) {
        let live = self.live_echo();
        // NonPreedit ngoài terminal = buffer ÂM THẦM (R2): không live echo
        // (Blink áp keymap trễ — repro 2026-07-10), không set_preedit
        // (user chọn NonPreedit chính vì không muốn gạch chân). Từ hiện
        // nguyên khối qua commit_string ở word boundary.
        let silent = !live && self.engine.mode() == ImeMode::NonPreedit;

        match action {
            NonPreeditAction::Buffer | NonPreeditAction::UpdatePreedit(_) => {
                if live {
                    // Live conversion: the screen follows the rendered form
                    // key by key ("nha" + '6' → "nhâ" instantly, live
                    // style). Mid-word backspace is the same diff, one
                    // char shorter.
                    let target = self.engine.inner().preedit_output();
                    self.sync_shown(&target);
                } else if !silent {
                    let s = self.engine.inner().preedit_string().to_string();
                    self.set_preedit(im, &s);
                }
            }
            NonPreeditAction::CommitWithBackspace { text, .. } => {
                crate::godmod::log_commit(!text.is_ascii());
                info!("[COMMIT] word done: \"{text}\" + replay code={keycode}");
                if live {
                    // Screen already shows the rendered form (live sync) —
                    // usually a no-op diff, then replay the boundary key.
                    self.sync_shown(&text);
                } else {
                    // commit_string replaces preedit per protocol spec.
                    im.commit_string(text);
                    im.commit(self.serial);
                }
                self.engine.reset();
                self.reset_word_state();
                self.vk.tap(keycode);
            }
            NonPreeditAction::CommitEmoji { text, .. } => {
                // Emoji/flushed-literal. Always via commit_string (the
                // input-method channel) — NOT viet_typer, whose static keymap
                // covers only ASCII+Vietnamese, never emoji codepoints. Safe in
                // live mode too: emoji only fires when no Vietnamese word is
                // pending, so `shown_word` is empty (nothing to diff/backspace).
                // The trigger key is folded into `text`, so it is NOT replayed.
                crate::godmod::log_commit(!text.is_ascii());
                info!("[COMMIT] emoji/flush: \"{text}\"");
                im.commit_string(text);
                im.commit(self.serial);
            }
            NonPreeditAction::ClearPreedit => {
                if live {
                    // Word fully deleted by backspace — remove what we own.
                    self.sync_shown("");
                } else if !silent {
                    // Silent buffer never showed anything — nothing to clear.
                    self.set_preedit(im, "");
                }
            }
            NonPreeditAction::PassThrough => {
                self.vk.press(keycode);
            }
        }
    }

    /// Immediate physical-click handler (eventfd wakeup, NOT waiting for
    /// the next key). A click moved the cursor: any hanging composition
    /// must vanish now — in Preedit mode the app is about to re-anchor or
    /// self-commit the preedit at the click position (R8: Drop, Don't
    /// Commit). Live mode needs only the state reset: what's on screen
    /// is real text and stays where it was typed.
    pub(crate) fn on_physical_click(&mut self, conn: &Connection) {
        if let Some(rt) = &self.runtime {
            // Sync the counter so the per-key guard doesn't double-fire.
            self.last_clicks = rt.clicks();
        }
        if !self.engine.has_pending() {
            return;
        }
        info!("[CLICK] chuột click khi đang gõ dở — drop composition NGAY (R8)");
        let live = self.live_echo();
        if !live && let Some(im) = self.input_method.clone() {
            self.set_preedit(&im, "");
        }
        self.engine.reset();
        self.reset_word_state();
        let _ = conn.flush();
    }

    /// Live mode (P0-3): make the app show `target` for the in-progress
    /// word. Diffs against what we already typed and touches only the
    /// changed suffix: BackSpace × k + the new suffix, ALL on the per-word
    /// keymap keyboard (viet_typer) — one object, so keymap-before-keys and
    /// key order are protocol-guaranteed, and the BackSpaces are paced
    /// (VCL/gtk3 swallows BS+char bursts whole, probe-verified 2026-07-10).
    /// The failure modes of commit_string (cross-channel reorder) and
    /// delete_surrounding_text (silently ignored) can't occur here.
    fn sync_shown(&mut self, target: &str) {
        let shown: Vec<char> = self.shown_word.chars().collect();
        let tgt: Vec<char> = target.chars().collect();
        let common = shown
            .iter()
            .zip(tgt.iter())
            .take_while(|(a, b)| a == b)
            .count();
        let suffix: String = tgt[common..].iter().collect();
        tracing::debug!(
            "[LIVE-SYNC] shown={:?} → target={target:?} (bs={}, suffix={suffix:?})",
            self.shown_word,
            shown.len() - common
        );
        // Pacing DEFAULT-ON; only known burst-safe terminals go fast.
        // Regression 2026-07-10 (repro: uinput rollover 20ms/key vào Electron
        // live-mode): whitelist "chỉ libreoffice cần pace" làm Electron/
        // Chromium mất ký tự có keycode MỚI trong keymap vừa upload —
        // client áp keymap trễ một nhịp, tap giải mã theo keymap CŨ →
        // "quà"→"q", "kẹ"→"k" (đúng lớp lỗi R16 đã cảnh báo: mọi biến thể
        // ít-pace-hơn đều fail thực địa). app_id None → paced (phía an toàn).
        let paced = !self.current_app_id.as_deref().is_some_and(|id| {
            let id = id.to_lowercase();
            crate::compositor::KNOWN_TERMINALS.contains(&id.as_str())
        });
        // Arm the live-echo guard BEFORE the vk call: each `sync_shown`
        // increments the counter; `Done` (end of text-input-v3 batch)
        // decrements it. The increment MUST happen before
        // `backspace_then_type` because its internal `roundtrip()`
        // dispatches the app's `TextChangeCause`+`Done` response
        // synchronously on the vk connection — if the counter is still 0
        // at that point, the `Other` cause is NOT suppressed and
        // composition gets dropped (tone keys appear as literal digits).
        self.live_echo_pending = self.live_echo_pending.saturating_add(1);
        if !self
            .viet
            .backspace_then_type(shown.len() - common, &suffix, paced)
        {
            tracing::warn!(
                "[VIET-TYPER] không gõ được (bs={}, \"{suffix}\") — giữ nguyên",
                shown.len() - common
            );
        }
        self.shown_word.clear();
        self.shown_word.push_str(target);
    }
}

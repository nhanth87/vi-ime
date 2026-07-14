// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
use tracing::{info, warn};
use wayland_client::protocol::wl_keyboard::KeyState;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, WEnum};
use wayland_protocols_misc::zwp_input_method_v2::client::{
    zwp_input_method_keyboard_grab_v2::ZwpInputMethodKeyboardGrabV2,
    zwp_input_method_v2::ZwpInputMethodV2,
};

use wayland_protocols::wp::text_input::zv3::client::zwp_text_input_v3::{
    ChangeCause, ContentPurpose,
};

use crate::engine::ImeMode;
use crate::wayland::feedback::ImeFeedback;
use crate::wayland::state::{FieldSensitivity, ImeAppState};
use crate::wayland::{ImUserData, KeyboardGrabUserData};

/// ⚠️ Chrome/Firefox depend on this: `ContentPurpose::Url` → `FieldSensitivity::Url`
/// is what makes the address bar type raw ASCII instead of composing
/// Vietnamese ("tự chuyển tiếng Anh khi gõ trong address bar"). Verified
/// working 2026-07-10 against Chrome — do NOT touch this mapping without
/// re-testing an actual browser address bar, since the browser's own
/// text-input-v3 behavior here has historically been inconsistent across
/// versions and this is the ONLY place that consumes `ContentPurpose::Url`.
/// Map the app's self-declared field purpose to our per-field gate.
/// Terminal gets its own variant to force NonPreedit mode (preedit-everywhere
/// commit_string works, but preedit underline breaks in terminals).
fn sensitivity_of(purpose: ContentPurpose) -> FieldSensitivity {
    match purpose {
        // Security-critical: engine off, never logged. Non-negotiable.
        ContentPurpose::Password | ContentPurpose::Pin => FieldSensitivity::Secure,
        // URL fields: passthrough keys so browser autocomplete works.
        ContentPurpose::Url => FieldSensitivity::Url,
        // Terminal: force NonPreedit mode (unless user explicitly chose otherwise).
        ContentPurpose::Terminal => FieldSensitivity::Terminal,
        // Digits, Number etc. → Normal (preedit works everywhere).
        ContentPurpose::Digits
        | ContentPurpose::Number
        | ContentPurpose::Phone
        | ContentPurpose::Date
        | ContentPurpose::Time
        | ContentPurpose::Datetime => FieldSensitivity::Normal,
        _ => FieldSensitivity::Normal,
    }
}

// ============================================================================
// Dispatch implementations
// ============================================================================

impl Dispatch<ZwpInputMethodV2, ImUserData> for ImeAppState {
    fn event(
        state: &mut Self,
        proxy: &ZwpInputMethodV2,
        event: <ZwpInputMethodV2 as Proxy>::Event,
        _data: &ImUserData,
        conn: &Connection,
        qhandle: &QueueHandle<Self>,
    ) {
        use wayland_protocols_misc::zwp_input_method_v2::client::zwp_input_method_v2::Event;
        match event {
            Event::Activate => {
                // Pick up any daemon-side config change before deciding to grab:
                // ime_enabled gates the grab below, and a disabled→enabled
                // transition re-grabs here (the only place with a QueueHandle).
                let prev_app = state.current_app_id.clone();
                state.maybe_reconfigure();
                let mode = format!("{:?}", state.engine.mode());
                info!(
                    "[SCENARIO] ✅ ACTIVATE — IME={mode} composer attached, grabbing keyboard..."
                );
                state.active = true;
                // Only clear the per-field classification on a REAL app switch.
                // A same-app re-activation (focus churn: DEACTIVATE→ACTIVATE with
                // no field change) must KEEP Terminal/Secure — the app sends
                // ContentType only on first focus, so resetting here would
                // silently lose it and a terminal would fall back to the delete
                // path (which it ignores → "nhaân"). Capability tracking still
                // resets each activation.
                //
                // ⚠️ `state.current_app_id` is written by `maybe_reconfigure`
                // (called just above) from `rt.app_id()` — the MAIN thread's
                // niri-focus-tracked value, delivered over a SEPARATE channel
                // from this Wayland thread's own Activate/ContentType events.
                // Chrome/Firefox's address-bar Url passthrough and kitty/
                // terminal's NonPreedit forcing both key off
                // `field_sensitivity`, which this block resets — so a lagging
                // `rt.app_id()` (main thread hasn't caught up yet) means this
                // reset silently doesn't fire on a genuine app switch. Verified
                // working 2026-07-10 with both threads live; if you change how
                // `current_app_id` is populated (compositor/niri.rs, main.rs's
                // focus pipeline), re-test an actual app switch into a browser
                // AND into a terminal, not just one or the other.
                if state.current_app_id != prev_app {
                    state.field_sensitivity = FieldSensitivity::Normal;
                }
                state.surrounding_seen = false;
                state.emit(ImeFeedback::Activated);
                crate::godmod::log_activate();
                if state.keyboard_grab.is_none() && state.ime_enabled {
                    let grab =
                        proxy.grab_keyboard(qhandle, KeyboardGrabUserData);
                    state.keyboard_grab = Some(grab);
                    info!("[SCENARIO] ⌨️  Keyboard GRABBED (all keys go to IME now)");
                }
            }
            Event::Deactivate => {
                let had_pending = state.engine.has_pending();
                info!(
                    "[SCENARIO] ❌ DEACTIVATE — focus lost, had_pending={had_pending}"
                );
                // R8: do NOT commit on Deactivate. The compositor already moved
                // the cursor by the time we see this event (e.g. mouse click).
                // Committing now places text at the wrong position.
                // Instead: drop the buffer. Compositor clears preedit from the
                // old position automatically when the input method deactivates.
                state.engine.reset();
                crate::godmod::log_deactivate();
                state.active = false;
                state.key_buffer.clear();
                state.field_sensitivity = FieldSensitivity::Normal;
                state.surrounding_seen = false;
                state.external_change = false;
                state.live_echo_pending = 0;
                state.reset_word_state();
                state.last_key_time = None;
                // Never leave the app with a stuck forwarded key.
                state.vk.release_all();
                if let Some(grab) = state.keyboard_grab.take() {
                    grab.release();
                    info!(
                        "[SCENARIO] 🔓 Keyboard RELEASED (app gets keys directly again)"
                    );
                }
            }
            Event::Done => {
                state.serial += 1;
                // P1-2: the app reported an external text/cursor change
                // (mouse click inside the SAME field never fires
                // Deactivate — this is the only signal we get). R8: drop
                // the half-typed word, never commit it at the new cursor.
                // Secondary guard: even if external_change is true
                // and counter is 0, don't drop if a live-echo update
                // is still settling (app hasn't finished rendering).
                // A mid-word `Other` cause with a recent `sync_shown`
                // (<200ms) is very likely our own vk typing, not a
                // genuine external edit.
                let recent_live = state
                    .last_live_echo_at
                    .map_or(false, |t| t.elapsed().as_millis() < 200);
                if std::mem::take(&mut state.external_change)
                    && state.engine.has_pending()
                    && !recent_live
                {
                    info!(
                        "[SCENARIO] 🖱️ external cursor/text change — \
                         dropping composition (R8: Drop, Don't Commit)"
                    );
                    state.engine.reset();
                    state.reset_word_state();
                    // ⚠️ Mode-aware, same rule as `finalize_word`/
                    // `on_physical_click`/`maybe_reconfigure`'s app-switch
                    // branch: NonPreedit/live mode (terminals) never calls
                    // set_preedit_string in the first place — raw keys are
                    // forwarded live and ARE already real text on screen.
                    // Sending an empty set_preedit_string here anyway is a
                    // spurious protocol message and reproduces the exact
                    // "nhảy chữ theo con trỏ" symptom this block exists to
                    // prevent (confirmed live on kitty, 2026-07-10 — a first
                    // cut of the app-switch fix made this same mistake).
                    let live = state.live_echo();
                    if !live && let Some(im) = state.input_method.clone() {
                        state.set_preedit(&im, "");
                    }
                }
                // Flush any keys buffered during the commit sequence.
                let buf_len = state.key_buffer.len();
                if buf_len > 0 {
                    info!(
                        "[SCENARIO] 🔄 DONE received — flushing {} buffered keys",
                        buf_len
                    );
                }
                state.flush_key_buffer(conn);
                // Decrement live-echo pending counter: one batch done.
                // When it reaches zero, subsequent `TextChangeCause::Other`
                // events are genuine external changes (not our own vk typing).
                state.live_echo_pending =
                    state.live_echo_pending.saturating_sub(1);
            }
            Event::Unavailable => {
                // Single-owner seat: a rival grabbed the input-method first.
                // Name it and hand the user the one-liner to take the seat.
                let rivals = crate::rivals::detect();
                if rivals.is_empty() {
                    warn!(
                        "IME unavailable — another input method holds the seat \
                         (không rõ tiến trình). Đảm bảo chỉ vi-ime chạy."
                    );
                } else {
                    warn!(
                        "IME unavailable — {} đang giữ input-method seat. \
                         Chạy `vi-ime --take-over` rồi khởi động lại vi-ime.",
                        crate::rivals::describe(&rivals)
                    );
                }
                state.active = false;
                state.emit(ImeFeedback::Unavailable);
            }
            Event::SurroundingText {
                text,
                cursor,
                anchor: _,
            } => {
                info!(
                    "[SURROUNDING] len={} cursor={cursor} pending={}",
                    text.len(),
                    state.engine.has_pending()
                );
                // Hard capability signal: this app supports surrounding
                // text → the live delete+commit model is safe. Report the
                // first sighting per activation to the learned cache.
                if !state.surrounding_seen {
                    state.surrounding_seen = true;
                    state.emit(ImeFeedback::SurroundingTextSeen);
                }
            }
            Event::TextChangeCause { cause } => {
                // P1-2: `other` = the text/cursor changed app-side (mouse
                // click, arrow keys handled by the app, undo…) — NOT by us.
                // Latched here, applied at `done` (end of the state batch).
                //
                // ⚠️ Live-echo guard: in NonPreedit mode, `sync_shown`
                // types composed glyphs through the virtual keyboard. The
                // app CORRECTLY reports those changes as `Other` (the
                // change came through the vk, not `commit_string`). If we
                // treat that as external, every tone-key update drops the
                // composition — tone keys appear as literal digits.
                // `live_echo_pending` (inc'd by sync_shown, dec'd at Done)
                // suppresses `Other` for our own vk typing.
                info!("[CAUSE] text_change_cause={cause:?}");
                let is_other = matches!(cause, WEnum::Value(ChangeCause::Other));
                if is_other && state.live_echo_pending > 0 {
                    // Our own live-echo vk typing. Not external.
                    state.external_change = false;
                } else {
                    state.external_change = is_other;
                }
            }
            Event::ContentType { hint: _, purpose } => {
                let sens = match purpose {
                    WEnum::Value(p) => sensitivity_of(p),
                    WEnum::Unknown(_) => FieldSensitivity::Normal,
                };
                if sens != state.field_sensitivity {
                    info!("[CONTENT-TYPE] field sensitivity → {sens:?} (had_pending={})", state.engine.has_pending());
                    state.field_sensitivity = sens;
                    // The ContentType event often arrives AFTER the first
                    // keystroke already entered the engine (seen live:
                    // KEY-IN 'n' → then "→ Url"). Switching to a
                    // passthrough class would strand that pending key —
                    // the "address bar / password loses the first char"
                    // bug. Flush it as real text before going raw.
                    // BUT: don't finalize for Terminal - we want to keep composing.
                    let should_finalize = matches!(sens, FieldSensitivity::Secure | FieldSensitivity::Url)
                        
                        && state.engine.has_pending()
                        && state.input_method.is_some();
                    if should_finalize {
                        info!("[CONTENT-TYPE] flushing pending first key(s) before passthrough (sens={sens:?})");
                        state.finalize_word(&state.input_method.as_ref().unwrap().clone());
                    }
                    // If field is Terminal, enforce NonPreedit immediately (unless user explicitly chose).
                    if sens == FieldSensitivity::Terminal && !state.mode_from_user {
                        // If there's pending text composed in Preedit mode, finalize it
                        // in the new NonPreedit mode to avoid commit errors on click/cursor move.
                        if state.engine.has_pending() && state.input_method.is_some() {
                            info!("[CONTENT-TYPE] Terminal field with pending text → finalizing in NonPreedit mode");
                            state.finalize_word(&state.input_method.as_ref().unwrap().clone());
                        }
                        state.engine.set_mode(ImeMode::NonPreedit);
                        info!("[CONTENT-TYPE] Terminal field → forcing NonPreedit mode");
                    }
                }
            }
            _ => {}
        }
        let _ = conn.flush();
    }
}

#[allow(unreachable_patterns)]
impl Dispatch<ZwpInputMethodKeyboardGrabV2, KeyboardGrabUserData> for ImeAppState {
    fn event(
        state: &mut Self,
        _proxy: &ZwpInputMethodKeyboardGrabV2,
        event: <ZwpInputMethodKeyboardGrabV2 as Proxy>::Event,
        _data: &KeyboardGrabUserData,
        conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        use wayland_protocols_misc::zwp_input_method_v2::client::zwp_input_method_keyboard_grab_v2::Event;
        match event {
            Event::Keymap { format, fd, size } => {
                info!("Received keymap (size: {size})");
                // Mirror to the virtual keyboard FIRST (borrows the fd), so
                // forwarded keycodes decode identically app-side…
                state.vk.set_keymap(format, &fd, size);
                match format {
                    WEnum::Value(_) => {
                        // …then hand the fd to xkb (consumes it).
                        state.xkb.set_keymap(fd, size);
                    }
                    WEnum::Unknown(raw) => {
                        warn!("Unsupported keymap format: {raw}");
                    }
                }
            }
            Event::Key {
                serial: _,
                time,
                key,
                state: key_state,
            } => {
                let pressed = match key_state {
                    WEnum::Value(KeyState::Pressed) => true,
                    WEnum::Value(KeyState::Released) => false,
                    WEnum::Value(_) | WEnum::Unknown(_) => return,
                };
                if pressed {
                    // Telemetry: non-monotonic compositor timestamps mean the
                    // delivery order differs from the typing order. The wl
                    // clock is u32 ms and wraps (~49 days) — treat huge
                    // "backwards" jumps as wraparound, not reordering.
                    if let Some(last) = state.last_key_time {
                        let back = last.wrapping_sub(time);
                        if back > 0 && back < 10_000 {
                            warn!("[REORDER] key time went back {back}ms (last={last} now={time})");
                            state.emit(ImeFeedback::KeyReorder { delta_ms: back });
                            crate::godmod::log_rollover();
                        }
                    }
                    state.last_key_time = Some(time);
                    // Delivery stage: compositor → us. ≥1ms = transport lag
                    // worth attributing (blame compositor, not the IME).
                    if let Some(us) = state.delivery_latency_us(time)
                        && us >= 1000 {
                            state.emit(ImeFeedback::StageSample {
                                stage: crate::wayland::feedback::PipelineStage::Delivery,
                                us,
                            });
                        }
                }
                // Buffer press AND release: releases ride the same queue so a
                // forwarded press is always followed by its release in order.
                state.buffer_key(key, pressed);
                state.flush_key_buffer(conn);
            }
            Event::Modifiers {
                serial: _,
                mods_depressed,
                mods_latched,
                mods_locked,
                group,
            } => {
                state.xkb.update_modifiers(
                    mods_depressed,
                    mods_latched,
                    mods_locked,
                    group,
                );
                // Mirror so forwarded keys carry the right modifier state.
                state.vk.modifiers(mods_depressed, mods_latched, mods_locked, group);
            }
            Event::RepeatInfo {
                rate: _,
                delay: _,
            } => {}
            _ => {}
        }
        let _ = conn.flush();
    }
}

// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 vi-im contributors
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;

use tracing::info;
use wayland_client::Connection;
use wayland_protocols_misc::zwp_input_method_v2::client::{
    zwp_input_method_keyboard_grab_v2::ZwpInputMethodKeyboardGrabV2,
    zwp_input_method_v2::ZwpInputMethodV2,
};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1;
use crate::engine::fast_engine::{CompositorKind, NonPreeditEngine};
use crate::engine::ImeMode;
use crate::plugin::PluginManager;

use crate::wayland::feedback::{FeedbackFn, ImeFeedback};
use crate::wayland::runtime::{self, RuntimeConfig};
use crate::wayland::viet_typer::VietTyper;
use crate::wayland::virtual_keyboard::VkForwarder;
use crate::wayland::xkb::XkbState;

/// Per-field input class, from the app's own `content_type` declaration.
/// This is the most precise adaptation signal we have — it is per text
/// field, not per app — and it is transient: reset on every Deactivate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum FieldSensitivity {
    #[default]
    Normal,
    /// Password/PIN: engine OFF, no preedit, and keys are never logged.
    Secure,
    /// Digits/number/phone/date/time: mapped to Normal today, but the gate
    /// (actions.rs) already treats it as passthrough — kept for the day
    /// `sensitivity_of` starts distinguishing numeric fields for real.
    #[allow(dead_code)]
    NumericRaw,
    /// URL fields (address bar): passthrough raw keys so browser
    /// autocomplete can see them. Vietnamese composition disabled.
    Url,
    /// Terminal fields: force NonPreedit mode (commit_string works, preedit underline breaks).
    /// This overrides user config unless mode_from_user is explicitly set.
    Terminal,
}

// ============================================================================
// Key event buffer entry for rollover handling
// ============================================================================

#[derive(Debug, Clone)]
pub(crate) struct KeyEvent {
    pub(crate) keycode: u32,
    /// true = press, false = release. Releases ride the same queue so a
    /// forwarded press is always followed by its forwarded release in order.
    pub(crate) pressed: bool,
    timestamp: Instant,
}

// ============================================================================
// Main IME app state
// ============================================================================

pub struct ImeAppState {
    pub engine: NonPreeditEngine,
    pub active: bool,
    pub input_method: Option<ZwpInputMethodV2>,
    pub keyboard_grab: Option<ZwpInputMethodKeyboardGrabV2>,
    pub(crate) xkb: XkbState,
    pub serial: u32,
    pub ime_enabled: bool,
    /// Virtual-keyboard forwarder — the ONLY way a grabbed key that we do not
    /// turn into text (arrows, shortcuts, Enter, boundary keys…) reaches the app.
    pub(crate) vk: VkForwarder,
    /// Second virtual keyboard with the generated Vietnamese keymap —
    /// Live mode types composed words on it (see viet_typer.rs).
    pub(crate) viet: VietTyper,
    /// Key event buffer for rollover handling.
    pub(crate) key_buffer: VecDeque<KeyEvent>,
    /// Live mode (P0-3): what we have typed into the app for the current
    /// word via the Vietnamese virtual keyboard — the diff base for
    /// `sync_shown`. Empty when no word is in progress.
    pub(crate) shown_word: String,
    /// Shared runtime config written by the daemon (None = static config).
    pub(crate) runtime: Option<Arc<RuntimeConfig>>,
    /// Last runtime generation we applied.
    pub(crate) last_generation: u64,
    /// Plugin middleware chain for per-app behavior customization.
    pub(crate) plugin_manager: PluginManager,
    /// Current focused app_id for plugin dispatch.
    pub(crate) current_app_id: Option<String>,
    /// Per-field class from the app's content_type (transient, R2-adjacent).
    pub(crate) field_sensitivity: FieldSensitivity,
    /// Whether the ime_mode in the active snapshot was an explicit USER
    /// choice — ContentType-Terminal must not override that (layer order).
    pub(crate) mode_from_user: bool,
    /// Whether the app sent surrounding_text during this activation.
    pub(crate) surrounding_seen: bool,
    /// P1-2: the current text-input state batch carries an app-side text
    /// change (`text_change_cause = other`, e.g. mouse click in the same
    /// field). Latched by TextChangeCause, consumed at Done.
    pub(crate) external_change: bool,
    /// Last seen physical-click counter (evdev watcher via RuntimeConfig).
    pub(crate) last_clicks: u64,
    /// Timestamp (compositor clock, ms) of the last key press — detects
    /// non-monotonic delivery for telemetry.
    pub(crate) last_key_time: Option<u32>,
    /// Process start, the base for comparing our clock to event times.
    pub(crate) run_start: Instant,
    /// Smallest observed (recv_ms - event_time_ms): the clock offset between
    /// the compositor's input clock and ours. Delivery delay = sample - base.
    pub(crate) clock_base_ms: Option<i64>,
    /// Daemon feedback sink (None = standalone run, signals dropped).
    pub(crate) feedback: Option<FeedbackFn>,
    /// Game mode: raw key passthrough, no IME processing.
    pub(crate) game_mode: bool,
    /// When the last composing key was processed — arms the idle
    /// auto-commit (see [`Self::idle_commit_deadline_ms`]).
    pub(crate) last_key_at: Option<Instant>,
    /// Live-echo guard: incremented by `sync_shown` before each
    /// `backspace_then_type` call, decremented at each `Done` (end
    /// of text-input-v3 batch). While >0, `TextChangeCause::Other`
    /// is suppressed — it is our own vk typing, not external.
    pub(crate) live_echo_pending: u32,
}

/// Preedit-only compositions are DROPPED on a mouse click (R8) — the field
/// complaint (2026-07-10, LibreOffice): "gõ dở rồi click là mất chữ". A
/// mid-word commit at click time is a proven race (R16). Instead: after
/// this long without a key, finalize the word while the cursor is
/// guaranteed still in place — semantically identical to the user pressing
/// the boundary themselves, zero race. Trade-off: a mid-word pause longer
/// than this finalizes the word, so a LATE tone key starts a new word
/// (rare; tone keys follow within a word almost immediately).
const IDLE_COMMIT_MS: u128 = 1500;

impl ImeAppState {
    pub(crate) fn new(
        engine: NonPreeditEngine,
        _compositor: CompositorKind,
        virtual_keyboard: Option<ZwpVirtualKeyboardV1>,
    ) -> Self {
        Self {
            engine,
            active: false,
            input_method: None,
            keyboard_grab: None,
            xkb: XkbState::new(),
            serial: 0,
            ime_enabled: true,
            vk: VkForwarder::new(virtual_keyboard),
            viet: VietTyper::new(),
            key_buffer: VecDeque::with_capacity(16),
            shown_word: String::new(),
            runtime: None,
            last_generation: 0,
            plugin_manager: Self::init_plugins(),
            current_app_id: None,
            field_sensitivity: FieldSensitivity::default(),
            mode_from_user: false,
            surrounding_seen: false,
            external_change: false,
            last_clicks: 0,
            last_key_time: None,
            run_start: Instant::now(),
            clock_base_ms: None,
            feedback: None,
            game_mode: false,
            last_key_at: None,
            live_echo_pending: 0,
        }
    }

    /// Live-echo mode: gõ thẳng từng glyph qua viet_typer. An toàn trên MỌI
    /// app từ khi viet_typer dùng keymap TĨNH 8-level (upload một lần, không
    /// bao giờ đổi — Blink áp keymap trễ vô hạn định nên mọi biến thể keymap
    /// động đều đã fail thực địa, xem viet_typer.rs). Khi không có virtual
    /// keyboard (viet not ready), NonPreedit rơi về buffer âm thầm +
    /// commit_string (apply_action). MỌI nhánh cần phân biệt live/preedit
    /// PHẢI gọi hàm này — đừng inline lại predicate (R16 bài học 2: 6 chỗ
    /// từng lệch nhau).
    pub(crate) fn live_echo(&self) -> bool {
        // Live-echo (backspace-diff qua viet_typer) cho mọi app khi
        // NonPreedit + viet.ready() (commit 1e80bed, keymap tĩnh 8-level).
        // Field Url/Secure/NumericRaw KHÔNG bao giờ tới đây — process_key
        // (actions.rs) đã raw-passthrough + return trước khi gọi apply_action.
        self.engine.mode() == ImeMode::NonPreedit && self.viet.ready()
    }

    /// ms left until the idle auto-commit fires, or None when unarmed.
    /// Armed ONLY while a composition exists solely as preedit (non-live):
    /// live mode's text is already real, nothing to lose on a click.
    pub(crate) fn idle_commit_deadline_ms(&self) -> Option<i32> {
        if !self.active || !self.engine.has_pending() {
            return None;
        }
        if self.live_echo() {
            return None;
        }
        let elapsed = self.last_key_at?.elapsed().as_millis();
        Some(IDLE_COMMIT_MS.saturating_sub(elapsed).min(i32::MAX as u128) as i32)
    }

    /// Fire the idle auto-commit if its deadline passed (poll timeout path).
    pub(crate) fn idle_commit(&mut self, conn: &Connection) {
        match self.idle_commit_deadline_ms() {
            Some(ms) if ms <= 0 => {}
            _ => return,
        }
        let Some(im) = self.input_method.clone() else { return };
        info!("[IDLE-COMMIT] {IDLE_COMMIT_MS}ms không gõ — chốt từ đang soạn (kẻo click là mất, R8)");
        self.finalize_word(&im);
        self.last_key_at = None;
        let _ = conn.flush();
    }

    /// Delivery-stage latency (compositor → us) for one key event, in µs.
    /// Uses a running minimum as the inter-clock offset; the first samples
    /// calibrate, later samples measure genuine transport delay.
    pub(crate) fn delivery_latency_us(&mut self, event_time_ms: u32) -> Option<u32> {
        let now_ms = self.run_start.elapsed().as_millis() as i64;
        let delta = now_ms - i64::from(event_time_ms);
        let base = match self.clock_base_ms {
            Some(b) => b.min(delta),
            None => delta,
        };
        self.clock_base_ms = Some(base);
        u32::try_from((delta - base) * 1000).ok()
    }

    /// Report a protocol observation to the daemon (non-blocking).
    pub(crate) fn emit(&self, signal: ImeFeedback) {
        if let Some(f) = &self.feedback {
            f(signal);
        }
    }

    /// Initialize PluginManager with all builtin plugins.
    fn init_plugins() -> PluginManager {
        let mut mgr = PluginManager::new();
        mgr.register(Box::new(crate::plugin::TerminalPlugin));
        mgr.register(Box::new(crate::plugin::BrowserPlugin));
        mgr.register(Box::new(crate::plugin::ChromiumNiriPlugin::new()));
        mgr.register(Box::new(crate::plugin::AutoCommitShortcutPlugin::new()));
        mgr.register(Box::new(crate::plugin::ElectronFlagAdvisorPlugin::new()));
        mgr
    }

    /// Pick up daemon-side config changes (lock-free, generation-gated).
    /// Called at the top of `process_key` and on `Activate`.
    ///
    /// Per R8: any pending composition is finalized before the engine is
    /// reconfigured, so no text is ever lost on a settings change.
    /// On enabled→disabled the keyboard grab is released here (otherwise the
    /// early-return in `process_key` would swallow every key — user couldn't
    /// type at all). Disabled→enabled re-grabs on the next `Activate`.
    pub(crate) fn maybe_reconfigure(&mut self) {
        let Some(rt) = &self.runtime else { return };
        let snap = rt.snapshot();
        if snap.generation == self.last_generation {
            return;
        }
        // Read the focused app_id while `rt` is still borrowed (its borrow
        // ends here); the plugin lifecycle is driven from it below.
        let new_app_id = rt.app_id();
        // ⚠️ PREEDIT-JUMP-WITH-CURSOR — read this before adding ANY special
        // case back into this block. History (2026-07-10, same day, three
        // regressions in a row):
        //   1. This used to unconditionally COMMIT pending text before every
        //      reconfigure ("safe" per the R12 doc comment). That commit
        //      lands wherever the CURRENT cursor is — fine if generation
        //      bumped because of a same-app setting change, wrong if it
        //      bumped because the app actually switched (the new app's
        //      cursor has nothing to do with the old composition) →
        //      "nhảy theo con trỏ".
        //   2. Fix attempt #1 special-cased "app switched" to drop instead
        //      of commit — correct idea, but called `set_preedit(&im, "")`
        //      unconditionally, which is itself a spurious protocol message
        //      for NonPreedit/terminal apps (they never set a real preedit,
        //      so clearing one is a message the app never asked for) →
        //      same symptom, different mechanism, confirmed on kitty within
        //      the hour.
        //   3. Fix attempt #2 made the drop mode-aware — correct, verified
        //      live on kitty — but STILL reported broken afterward. Root
        //      cause: this whole "is it worth trying to commit safely"
        //      question is the wrong thing to be answering here at all.
        // Every OTHER interruption point in this file (Deactivate,
        // on_physical_click, external_change) uses the SAME unconditional
        // rule: drop, never try to commit "safely". Reconfigure now matches
        // them instead of trying to be clever — one fewer place that can
        // disagree with the other three. The cost is a mid-word Telex/VNI
        // toggle drops the in-progress word instead of finalizing it, same
        // as any other interruption; that trade already exists everywhere
        // else in this file and users haven't complained about IT.
        if self.engine.has_pending() {
            let live = self.live_echo();
            info!("[RECONFIG] reconfigure mid-composition — drop, don't commit (R8)");
            self.engine.reset();
            self.reset_word_state();
            if !live && let Some(im) = self.input_method.clone() {
                self.set_preedit(&im, "");
            }
        }
        runtime::apply_snapshot(&mut self.engine, &snap);
        let was_enabled = self.ime_enabled;
        self.ime_enabled = snap.enabled;
        self.mode_from_user = snap.mode_from_user;
        self.game_mode = snap.game_mode;
        self.last_generation = snap.generation;
        info!(
            "[RECONFIG] gen={} enabled={} method={:?} mode={:?} output={:?}",
            snap.generation, snap.enabled, snap.method, snap.mode, snap.output
        );
        // Terminal enforcement: if field is Terminal and user didn't explicitly
        // choose a mode, force NonPreedit (preedit underline breaks in terminals).
        if self.field_sensitivity == FieldSensitivity::Terminal && !self.mode_from_user {
            self.engine.set_mode(ImeMode::NonPreedit);
            info!("[RECONFIG] Terminal field → forcing NonPreedit mode");
        }
        // App focus changed → drive the AppPlugin lifecycle (on_blur old /
        // on_focus new) and update the app_id used for per-app plugin routing
        // in pre_process_key/post_process_action. The config layer (R13) stays
        // the single source of truth for ime_mode; a plugin's recommended_mode
        // is advisory only (logged for diagnostics, never overrides R13).
        if new_app_id != self.current_app_id {
            if let Some(ref app_id) = new_app_id {
                self.plugin_manager.on_focus_change(app_id);
                if let Some(rec) = self.plugin_manager.recommended_mode(app_id)
                    && rec != snap.mode {
                        info!(
                            "[PLUGIN] {app_id}: plugin suggests {rec:?}, config resolved {:?} (config wins, R13)",
                            snap.mode
                        );
                    }
            }
            self.current_app_id = new_app_id;
        }
        if was_enabled && !snap.enabled
            && let Some(grab) = self.keyboard_grab.take() {
                grab.release();
                self.vk.release_all();
                info!("[RECONFIG] 🔓 IME disabled — keyboard grab RELEASED");
            }
    }

    /// Buffer a key event for rollover handling.
    pub(crate) fn buffer_key(&mut self, keycode: u32, pressed: bool) {
        let now = Instant::now();
        // Coalesce: same keycode pressed again within 20ms = key repeat, drop
        if pressed
            && let Some(last) = self.key_buffer.back() {
                let gap = last.timestamp.elapsed().as_micros();
                if last.pressed && last.keycode == keycode && gap < 20_000 {
                    info!("[ROLLOVER] SKIP key-repeat code={keycode} gap={gap}µs");
                    self.emit(ImeFeedback::KeyChatter { keycode });
                    return;
                }
            }
        self.key_buffer.push_back(KeyEvent {
            keycode,
            pressed,
            timestamp: now,
        });
        // NEVER log key identities in secure fields (password/PIN).
        if pressed && self.field_sensitivity != FieldSensitivity::Secure {
            let ch = self.xkb.keycode_to_char(keycode);
            let ch_str = ch.map(|c| format!("'{c}'")).unwrap_or_else(|| "?".into());
            info!(
                "[KEY-IN] code={keycode} char={} mode={:?} queue={}/16",
                ch_str,
                self.engine.mode(),
                self.key_buffer.len()
            );
        }
    }

    /// Process buffered keys in order. Called after key events and "done".
    pub(crate) fn flush_key_buffer(&mut self, conn: &Connection) {
        while let Some(ev) = self.key_buffer.pop_front() {
            if ev.pressed {
                // QueueWait stage: time the key sat in our buffer (waiting
                // for a `done` ack or a burst) — ≥1ms is worth reporting.
                let waited = ev.timestamp.elapsed();
                if waited.as_millis() >= 1 {
                    let us = waited.as_micros().min(u128::from(u32::MAX)) as u32;
                    self.emit(ImeFeedback::StageSample {
                        stage: crate::wayland::feedback::PipelineStage::QueueWait,
                        us,
                    });
                }
                self.process_key(ev.keycode, conn);
            } else {
                // Forward the release only if we forwarded its press.
                self.vk.release(ev.keycode);
            }
        }
    }

    /// Check whether `keycode` is the game mode toggle: Ctrl+Shift+G.
    pub(crate) fn is_game_mode_toggle(&self, keycode: u32) -> bool {
        let Some(ch) = self.xkb.keycode_to_char(keycode) else {
            return false;
        };
        if !(ch == 'g' || ch == 'G') {
            return false;
        }
        self.xkb.is_control_active() && self.xkb.is_shift_active()
    }

}



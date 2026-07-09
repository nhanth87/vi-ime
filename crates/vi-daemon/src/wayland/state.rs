// SPDX-License-Identifier: GPL-3.0-or-later OR Commercial
// Copyright (c) 2024-2026 vi-im contributors
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tracing::{info, warn};
use wayland_client::Connection;
use wayland_protocols_misc::zwp_input_method_v2::client::{
    zwp_input_method_keyboard_grab_v2::ZwpInputMethodKeyboardGrabV2,
    zwp_input_method_v2::ZwpInputMethodV2,
};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1;
use crate::engine::fast_engine::{CompositorKind, NonPreeditEngine};
use crate::plugin::PluginManager;

use crate::wayland::commit::Phase2;
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
    /// Terminal field: no longer special-cased (preedit-everywhere), kept for API.
    #[allow(dead_code)]
    Terminal,
    /// Digits/number/phone/date/time: mapped to Normal (preedit-everywhere).
    #[allow(dead_code)]
    NumericRaw,
    /// URL fields (address bar): passthrough raw keys so browser
    /// autocomplete can see them. Vietnamese composition disabled.
    Url,
}

/// If the compositor never answers a phase-1 delete with `done` (app without
/// surrounding-text support), give up waiting and send phase-2 anyway —
/// otherwise the key queue is stuck forever.
const DONE_TIMEOUT: Duration = Duration::from_millis(150);

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
    /// Whether we are currently waiting for a "done" event (mid-commit sequence).
    pub(crate) waiting_for_done: bool,
    /// When the phase-1 delete was sent (drives DONE_TIMEOUT).
    pub(crate) waiting_since: Option<Instant>,
    /// Deferred phase-2 to run after "done" (or timeout).
    pub(crate) pending_phase2: Option<Phase2>,
    /// The rendered text WE currently own in the app for the in-progress word.
    /// This is the exact string a `delete_surrounding_text` must remove before
    /// committing the next form. Empty when no word is in progress.
    pub(crate) committed_word: String,
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
}

impl ImeAppState {
    pub(crate) fn new(
        engine: NonPreeditEngine,
        _compositor: CompositorKind,
        virtual_keyboard: Option<ZwpVirtualKeyboardV1>,
        viet_keyboard: Option<ZwpVirtualKeyboardV1>,
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
            viet: VietTyper::new(viet_keyboard),
            key_buffer: VecDeque::with_capacity(16),
            waiting_for_done: false,
            waiting_since: None,
            pending_phase2: None,
            committed_word: String::new(),
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
        }
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
        // Finalize pending text first (R8).
        if self.engine.has_pending() {
            let Some(im) = self.input_method.clone() else {
                return; // no proxy yet — defer until we can commit safely
            };
            info!("[RECONFIG] finalize pending word before applying new config");
            self.commit_pending_then(&im, None);
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
            if self.waiting_for_done {
                if !self.done_timed_out() {
                    // Can't process until the current commit sequence finishes
                    self.key_buffer.push_front(ev);
                    return;
                }
                warn!("[COMMIT] done timeout — forcing phase-2 without ack");
                self.emit(ImeFeedback::DoneTimeout);
                self.finish_waiting_and_run_phase2();
            }
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

    fn done_timed_out(&self) -> bool {
        self.waiting_since
            .map(|t| t.elapsed() >= DONE_TIMEOUT)
            .unwrap_or(true)
    }

    /// How long the event loop may block before it MUST force phase-2: the
    /// remaining slice of `DONE_TIMEOUT` while a commit awaits `done`, or
    /// `None` when nothing is pending — then the loop blocks indefinitely, so
    /// an idle IME still costs zero CPU (R15). Without this the 150 ms timeout
    /// could only fire reactively on the next key, leaving a toned character
    /// "stuck" for a keystroke (1–2 s in natural typing).
    pub(crate) fn done_wait_remaining(&self) -> Option<Duration> {
        if !self.waiting_for_done {
            return None;
        }
        Some(match self.waiting_since {
            Some(t) => DONE_TIMEOUT.saturating_sub(t.elapsed()),
            None => Duration::ZERO,
        })
    }

    /// Force the deferred phase-2 append when `done` never arrived within the
    /// timeout, so the toned character appears on its own (not on the next
    /// key). Called by the event loop after a poll wakeup/timeout.
    pub(crate) fn force_phase2_if_timed_out(&mut self, conn: &Connection) {
        if self.waiting_for_done && self.done_timed_out() {
            warn!("[COMMIT] done timeout — forcing phase-2 without ack");
            self.emit(ImeFeedback::DoneTimeout);
            self.finish_waiting_and_run_phase2();
            self.flush_key_buffer(conn);
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



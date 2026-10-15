//! Re-arm detection + idle-commit timing. Extracted from state.rs (R4).

use std::time::{Duration, Instant};

use super::ImeAppState;
use super::{IDLE_COMMIT_MS, REARM_WINDOW_MS};
use crate::wayland::ActivePath;
use crate::wayland::actions::MODIFIER_KEYS;
use crate::wayland::feedback::ImeFeedback;
use tracing::info;
use wayland_client::Connection;

impl ImeAppState {
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

    /// Phase 5: true if the current app is one-shot (enable() called once,
    /// no re-arm on refocus — like LibreOffice VCL).
    pub(crate) fn is_one_shot_app(&self) -> bool {
        !self.app_rearms
    }

    /// ms left until the re-arm detection window closes, or None when
    /// not armed (already confirmed, already classified, or no activate yet).
    pub(crate) fn rearm_deadline_ms(&self) -> Option<i32> {
        // Only armed during the optimistic window: first Activate seen,
        // not yet confirmed by a second one, and not yet timed out.
        if self.enable_count != 1 || !self.app_rearms {
            return None;
        }
        let elapsed = self.last_enable_ts?.elapsed().as_millis();
        Some(REARM_WINDOW_MS.saturating_sub(elapsed).min(i32::MAX as u128) as i32)
    }

    /// Check whether the 2-second re-arm window closed without a second
    /// Activate. If so, classify the app as one-shot and log.
    /// Phase 7: also emits `ImeFeedback::OneShotDetected` so the daemon
    /// can engage the evdev fallback for this app.
    pub(crate) fn check_rearm_timeout(&mut self) {
        if self.enable_count == 1 && self.app_rearms {
            if let Some(ts) = self.last_enable_ts {
                if ts.elapsed().as_millis() >= REARM_WINDOW_MS {
                    self.app_rearms = false;
                    info!(
                        "[REARM] enable_count={} app_rearms=false — app classified as one-shot (no re-arm within {}ms)",
                        self.enable_count, REARM_WINDOW_MS
                    );
                    // Phase 7: signal the daemon to engage evdev fallback.
                    self.emit(crate::wayland::feedback::ImeFeedback::OneShotDetected);
                }
            }
        }
    }

    /// Combined deadline for poll timeout: the soonest of idle-commit,
    /// re-arm detection and key repeat, or None if none is armed.
    pub(crate) fn poll_timeout_ms(&self) -> Option<i32> {
        let idle = self.idle_commit_deadline_ms();
        let rearm = self.rearm_deadline_ms();
        let repeat = self.repeat_deadline_ms();
        [idle, rearm, repeat]
            .into_iter()
            .flatten()
            .min()
    }

    /// ms left until the armed key repeat fires, or None when disarmed.
    pub(crate) fn repeat_deadline_ms(&self) -> Option<i32> {
        let next = self.repeat_next?;
        if self.repeat_key.is_none() {
            return None;
        }
        let left = next.saturating_duration_since(Instant::now()).as_millis();
        Some(left.min(i32::MAX as u128) as i32)
    }

    /// Arm client-side repeat for a freshly pressed grab key. Modifiers
    /// never repeat (state, not input); rate 0 (compositor-disabled) disarms.
    pub(crate) fn arm_repeat(&mut self, keycode: u32) {
        if self.repeat_rate == 0 || MODIFIER_KEYS.contains(&keycode) {
            self.repeat_key = None;
            self.repeat_next = None;
            return;
        }
        self.repeat_key = Some(keycode);
        self.repeat_next = Some(Instant::now() + Duration::from_millis(self.repeat_delay_ms));
    }

    /// Disarm repeat when this key is released (a different key's release
    /// must not kill the still-held key's repeat).
    pub(crate) fn disarm_repeat(&mut self, keycode: u32) {
        if self.repeat_key == Some(keycode) {
            self.repeat_key = None;
            self.repeat_next = None;
        }
    }

    /// Fire a client-side repeat: re-inject the still-held key, re-arm at
    /// the compositor's rate. Skips the game-mode toggle chord (repeating
    /// it would flap game mode on/off). Min 25ms step: below the 20ms
    /// rollover-coalesce window repeats would be eaten as key chatter.
    /// No-op unless this is the owning path (evdev owns keys otherwise).
    pub(crate) fn fire_repeat(&mut self, conn: &Connection) {
        let Some(keycode) = self.repeat_key else { return };
        let Some(next) = self.repeat_next else { return };
        if Instant::now() < next {
            return;
        }
        if let Some(rt) = &self.runtime
            && rt.active_path() != ActivePath::Wayland
        {
            self.repeat_key = None;
            self.repeat_next = None;
            return;
        }
        if self.is_game_mode_toggle(keycode) {
            self.repeat_key = None;
            self.repeat_next = None;
            return;
        }
        let step = 1000u64
            .checked_div(u64::from(self.repeat_rate.max(1)))
            .unwrap_or(40)
            .max(25);
        self.repeat_next = Some(Instant::now() + Duration::from_millis(step));
        self.buffer_key(keycode, true);
        self.flush_key_buffer(conn);
    }
}

//! Re-arm detection + idle-commit timing. Extracted from state.rs (R4).

use super::ImeAppState;
use crate::wayland::feedback::ImeFeedback;
use tracing::info;
use wayland_client::Connection;
use super::{IDLE_COMMIT_MS, REARM_WINDOW_MS};

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

    /// Combined deadline for poll timeout: the sooner of idle-commit and
    /// re-arm detection, or None if neither is armed.
    pub(crate) fn poll_timeout_ms(&self) -> Option<i32> {
        let idle = self.idle_commit_deadline_ms();
        let rearm = self.rearm_deadline_ms();
        match (idle, rearm) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        }
    }
}

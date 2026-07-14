// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! vi-godmod — Debug & Development Telemetry
//!
//! Logs EVERY keystroke with full context: timestamp, char, keycode,
//! app_id, compositor, IME mode, engine state, latency metrics.
//!
//! **Only active when:** `RUST_LOG=debug` or `--godmod` flag.
//! **Output:** `~/.local/share/vi-ime/godmod/<session-id>.jsonl`

mod models;
mod session;

pub use models::SessionSummary;
pub use session::GodmodSession;

use std::sync::{Mutex, OnceLock};
use crate::engine::ImeMode;

// Global singleton
static GODMOD: OnceLock<Mutex<GodmodSession>> = OnceLock::new();

/// Initialize global godmod session. Call once at startup.
pub fn init(enabled: bool) {
    let _ = GODMOD.set(Mutex::new(GodmodSession::new(enabled)));
}

/// Log a key event (no-op if not initialized or disabled).
#[allow(clippy::too_many_arguments)]
pub fn log(
    keycode: u32, ch: Option<char>, mode: ImeMode,
    action: &str, latency_us: u64, buffer_depth: usize,
    has_pending: bool, preedit_text: &str,
) {
    if let Some(m) = GODMOD.get()
        && let Ok(mut s) = m.lock() {
            s.log_key(keycode, ch, mode, action, latency_us, buffer_depth, has_pending, preedit_text);
        }
}

pub fn log_commit(is_vn: bool) {
    if let Some(m) = GODMOD.get() && let Ok(mut s) = m.lock() { s.log_commit(is_vn); }
}

pub fn log_backspace() {
    if let Some(m) = GODMOD.get() && let Ok(mut s) = m.lock() { s.log_backspace(); }
}

pub fn log_rollover() {
    if let Some(m) = GODMOD.get() && let Ok(mut s) = m.lock() { s.log_rollover(); }
}

pub fn log_activate() {
    if let Some(m) = GODMOD.get() && let Ok(mut s) = m.lock() { s.log_activate(); }
}

pub fn log_deactivate() {
    if let Some(m) = GODMOD.get() && let Ok(mut s) = m.lock() { s.log_deactivate(); }
}

pub fn set_app(app_id: &str) {
    if let Some(m) = GODMOD.get() && let Ok(mut s) = m.lock() { s.set_app(app_id); }
}

pub fn finish() -> Option<SessionSummary> {
    GODMOD.get().and_then(|m| m.lock().ok().and_then(|mut s| s.finish()))
}


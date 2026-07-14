// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Notification system for vi-ime.
//!
//! The "app chưa nhận bộ gõ" advice popup (once-per-app "unsupported"
//! toast) was removed 2026-07-10 — the user found it too noisy, and it was
//! inherently noisy by construction: a focused app with no editable field
//! (e.g. you clicked a toolbar icon, not a text box) never sends Activate
//! either, so it fired just as often for perfectly working apps as for
//! genuinely broken ones. The detection + `[UNSUPPORTED]` log line stays
//! in `learning.rs::probe_timeout` for `--doctor`/troubleshooting; only the
//! popup is gone. Don't re-add a popup here without a way to tell "no text
//! field was ever focused" apart from "text field focused, IME never
//! spoke" — right now the protocol gives us no such signal.

/// Desktop toast for a user-initiated state change (tray click, CLI
/// `--switch`/`--toggle`/`--mode`) — best-effort, non-blocking, silent if
/// no notification daemon is running. Uses vi-im's own icon (installed by
/// `tray::install_icons` at startup) so the toast is recognizable even
/// with no title text visible in compact notification styles.
pub fn popup(title: &str, body: &str) {
    let _ = std::process::Command::new("notify-send")
        .args([title, body, "--icon=vi-im", "--expire-time=3000"])
        .spawn();
}

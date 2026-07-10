// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 vi-im contributors
//! Notification system for vi-ime.
//! Channels: stderr log, tray state callback. DBus optional.

use crate::engine::AppSupport;

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

/// Minimal notifier — logs to stderr, calls tray callback.
/// DBus notification is done via shell `notify-send`.
pub struct Notifier {
    on_state_change: Box<dyn Fn(AppSupport) + Send>,
}

impl Notifier {
    pub fn new(on_state_change: Box<dyn Fn(AppSupport) + Send>) -> Self {
        Self { on_state_change }
    }

    /// Actionable advice notification (once per app per session, the caller
    /// enforces the throttle): tells the user HOW to fix typing in this app
    /// (control panel profile, Electron flags…). Best-effort notify-send.
    pub fn notify_advice(&self, app_name: &str, body: &str) {
        eprintln!("[ADVICE] app={app_name}: {body}");
        (self.on_state_change)(AppSupport::Unsupported);
        let title = format!("vi-ime — {app_name}");
        let _ = std::process::Command::new("notify-send")
            .args([&title, body, "--icon=input-keyboard", "--expire-time=10000"])
            .spawn();
    }
}

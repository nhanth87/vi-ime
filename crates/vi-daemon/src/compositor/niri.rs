// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Niri IPC: focused-window queries + real-time event stream.

use std::io::BufRead;
use std::process::{Command, Stdio};

use serde::Deserialize;
use tracing::warn;

use crate::compositor::FocusEvent;

#[derive(Debug, Deserialize)]
pub(crate) struct NiriWindow {
    #[serde(default)]
    pub(crate) app_id: Option<String>,
    #[serde(default)]
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) pid: Option<i32>,
    #[serde(rename = "is_focused", default)]
    pub(crate) is_focused: Option<bool>,
}

/// Parse `niri msg --json windows` output into the focused window's
/// FocusEvent. Pure function — unit-testable without niri running.
///
/// `niri msg --json windows` returns a bare JSON array (`[{...}, {...}]`),
/// NOT `{"Windows": [...]}` — that wrapped shape is only how the
/// `WindowsChanged` EVENT-STREAM tag looks. Confirmed live against niri
/// 26.04 2026-07-10: the wrapped-struct parse silently failed on every
/// single call (`serde_json::from_str().ok()` swallows the mismatch),
/// meaning `current_app_id` in main.rs's focus loop was **always None** —
/// per-app profile switching, the app-support probe, and (before this
/// session) the auto evdev-fallback trigger never actually saw an app_id.
pub(crate) fn parse_focused(json: &str) -> Option<FocusEvent> {
    let windows: Vec<NiriWindow> = serde_json::from_str(json).ok()?;
    windows
        .into_iter()
        .find(|w| w.is_focused == Some(true))
        .map(|w| FocusEvent { app_id: w.app_id, title: w.title, pid: w.pid })
}

/// Spawn a thread following `niri msg event-stream` and forward the focused
/// window as `FocusEvent`s. `WindowOpenedOrChanged` is included so browser
/// tab switches (title change without focus change) trigger per-site rules.
///
/// Reconnects internally with capped backoff when the stream dies (niri
/// restart, pipe error) — the receiver never has to poll or respawn. The
/// thread is blocked on pipe reads its whole life: zero CPU while idle.
pub fn spawn_niri_event_stream(tx: std::sync::mpsc::Sender<FocusEvent>) {
    let binary = std::env::var("NIRI_BINARY").unwrap_or_else(|_| "niri".to_string());
    std::thread::spawn(move || {
        let mut backoff_secs = 2u64;
        loop {
            match follow_stream(&binary, &tx) {
                StreamEnd::ReceiverGone => return, // daemon dropped rx — exit
                StreamEnd::StreamDied => {
                    warn!("niri event-stream ended — reconnecting in {backoff_secs}s");
                    std::thread::sleep(std::time::Duration::from_secs(backoff_secs));
                    backoff_secs = (backoff_secs * 2).min(30);
                }
                StreamEnd::Connected => {
                    backoff_secs = 2; // healthy run → reset backoff
                }
            }
        }
    });
}

enum StreamEnd {
    /// The receiver hung up — stop the thread entirely.
    ReceiverGone,
    /// Stream ended after a healthy run (reset backoff, reconnect).
    Connected,
    /// Could not spawn / immediate failure (increase backoff).
    StreamDied,
}

/// Follow one `niri msg event-stream` child until it ends.
fn follow_stream(binary: &str, tx: &std::sync::mpsc::Sender<FocusEvent>) -> StreamEnd {
    let mut child = match Command::new(binary)
        .arg("msg").arg("--json").arg("event-stream")
        .stdout(Stdio::piped()).stderr(Stdio::null()).spawn()
    {
        Ok(c) => c,
        Err(e) => {
            warn!("Failed to spawn niri event-stream: {e}");
            return StreamEnd::StreamDied;
        }
    };
    let Some(stdout) = child.stdout.take() else { return StreamEnd::StreamDied };
    let reader = std::io::BufReader::new(stdout);
    let mut delivered = false;

    for line in reader.lines() {
        match line {
            Ok(line) => {
                if line.contains("WindowFocusChanged")
                    || line.contains("WindowsChanged")
                    || line.contains("WindowOpenedOrChanged")
                {
                    let focus = Command::new(binary)
                        .arg("msg").arg("--json").arg("windows")
                        .output().ok()
                        .and_then(|o| String::from_utf8(o.stdout).ok())
                        .and_then(|s| parse_focused(&s));
                    if tx.send(focus.unwrap_or_default()).is_err() {
                        let _ = child.kill();
                        return StreamEnd::ReceiverGone;
                    }
                    delivered = true;
                }
            }
            Err(e) => {
                warn!("niri event-stream read error: {e}");
                break;
            }
        }
    }
    let _ = child.kill();
    if delivered { StreamEnd::Connected } else { StreamEnd::StreamDied }
}

// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Physical mouse-click watcher (evdev) — the last-resort click signal.
//!
//! Field-tested 2026-07-09: some apps send NO protocol signal on a mouse
//! click inside the same text field — no Deactivate, no surrounding_text,
//! no text_change_cause. The cursor still moves, so the next word-commit
//! lands at the click position. The only universal signal left is the
//! physical button itself. This watcher bumps `RuntimeConfig::record_click`
//! on every button press; the IME thread compares the counter before each
//! key and drops the half-typed word (R8: Drop, Don't Commit).
//!
//! Zero-config degradation: needs read access to /dev/input (`input`
//! group — same requirement as the evdev fallback). Without it the
//! watcher logs one hint and stays off; everything else works as before.

use std::sync::Arc;
use std::thread;

use evdev::{Device, EventSummary, KeyCode};
use tracing::{info, warn};

use crate::wayland::RuntimeConfig;

/// Buttons that move a text cursor. Wheel scrolling doesn't.
const CLICK_BUTTONS: [KeyCode; 3] = [KeyCode::BTN_LEFT, KeyCode::BTN_RIGHT, KeyCode::BTN_MIDDLE];

/// Spawn one reader thread per pointer device. Returns the number of
/// devices being watched (0 = no permission or no mouse — feature off).
pub fn spawn(runtime: Arc<RuntimeConfig>) -> usize {
    // Diagnostic escape hatch: set VI_IME_NO_CLICK_WATCH=1 to rule out
    // the watcher as the cause of mouse/scrolling issues. If scroll works
    // with the env var set, the watcher's poll loop is the culprit — the
    // permanent fix would replace evdev polling with a libei/libinput listener.
    if std::env::var_os("VI_IME_NO_CLICK_WATCH").is_some() {
        info!("[CLICK-WATCH] VI_IME_NO_CLICK_WATCH set — watcher disabled");
        return 0;
    }

    let mut watched = 0;
    let mut denied = false;

    for (path, device) in evdev::enumerate() {
        let Some(keys) = device.supported_keys() else { continue };
        if !CLICK_BUTTONS.iter().any(|b| keys.contains(*b)) {
            continue;
        }
        let rt = Arc::clone(&runtime);
        let name = device.name().unwrap_or("?").to_string();
        info!("[CLICK-WATCH] watching {name} ({})", path.display());
        thread::Builder::new()
            .name("vi-click-watch".into())
            .spawn(move || watch_device(device, rt))
            .ok();
        watched += 1;
    }

    // Distinguish "no permission" from "no mouse" for the hint.
    if watched == 0 {
        denied = std::fs::read_dir("/dev/input")
            .map(|mut d| d.any(|e| e.is_ok()))
            .unwrap_or(false);
    }
    if watched == 0 && denied {
        warn!(
            "[CLICK-WATCH] không đọc được /dev/input — click-detect tắt. \
             Từ đang gõ dở có thể bị commit sai chỗ khi click chuột trong \
             app không báo tín hiệu. Bật: thêm user vào nhóm `input` \
             (sudo usermod -aG input $USER, đăng nhập lại)."
        );
    }
    watched
}

fn watch_device(mut device: Device, runtime: Arc<RuntimeConfig>) {
    // 2026-07-15: some terminal emulators (foot, kitty, alacritty) interpret
    // scroll wheel as Up/Down arrow (history) when the default shell is
    // active. This is NORMAL terminal behavior, NOT a vi-ime bug. However,
    // the click_watch polling loop reads ALL mouse events (including scroll)
    // in a tight loop, which can starve the compositor's event processing
    // on slow machines / high IRQ load — mimicking the symptom.
    //
    // Mitigations: sleep between polls so the compositor gets a fair share
    // of the device's event buffer (each fd has its own buffer, but tight
    // user-space polling on a shared IRQ line can cause event reordering).
    // Also: `VI_IME_NO_CLICK_WATCH=1` disables the watcher entirely for
    // diagnostic isolation — if scroll works with the env set, the watcher
    // is the culprit; the permanent fix would be a libei/libinput listener.
    loop {
        let events = match device.fetch_events() {
            Ok(ev) => ev,
            Err(_) => return, // device unplugged / revoked — thread ends
        };
        for ev in events {
            if let EventSummary::Key(_, code, 1) = ev.destructure()
                && CLICK_BUTTONS.contains(&code)
            {
                runtime.record_click();
                // Wake the IME event loop NOW: Preedit mode must clear its
                // preedit before the app reacts to the click — the next
                // keystroke is too late.
                let fd = runtime.click_fd();
                if fd >= 0 {
                    let one: u64 = 1;
                    unsafe {
                        libc::write(fd, (&raw const one).cast(), 8);
                    }
                }
            }
        }
        // Yield the CPU: the compositor needs time to process the same
        // events from its own fd. Without a yield/sleep, this thread
        // busy-polls the evdev fd, draining CPU time from the compositor's
        // libinput thread — which CAN cause event reordering on shared IRQ.
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

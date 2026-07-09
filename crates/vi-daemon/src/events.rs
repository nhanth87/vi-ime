// SPDX-License-Identifier: GPL-3.0-or-later OR Commercial
// Copyright (c) 2024-2026 vi-im contributors
//! Unified daemon event bus — everything blocks, nothing polls.
//!
//! The main loop does a single `rx.recv()` and sleeps at the kernel level
//! until something actually happens. Feeders (each a blocking thread):
//! - niri event-stream  → `Focus` (self-reconnecting, blocking pipe read)
//! - inotify watch      → `ConfigChanged` (blocking inotify fd read)
//!
//! Zero wakeups when idle — the old design polled at 5 Hz (recv_timeout
//! 200ms + a stat() per tick).

use std::path::Path;
use std::sync::mpsc::{Receiver, Sender};

use inotify::{Inotify, WatchMask};
use tracing::{info, warn};

use crate::compositor::FocusEvent;
use crate::ipc::IpcCommand;
use crate::wayland::ImeFeedback;

/// Everything the daemon reacts to.
pub enum DaemonEvent {
    Focus(FocusEvent),
    ConfigChanged,
    /// Hard protocol signal from the IME thread (capability detection).
    ImeFeedback(ImeFeedback),
    /// The probe delay for `app_id` elapsed — check whether it ever
    /// attached a text input (app-support detection, R11).
    ProbeTimeout(String),
    /// IPC read command (get_config, list_apps, get_learned).
    /// The oneshot reply channel carries the serialized response.
    IpcRead {
        command: IpcCommand,
        reply: std::sync::mpsc::Sender<crate::ipc::IpcResponse>,
    },
    /// IPC write command (set_config, add_app, remove_app).
    /// Async — the main loop handles the mutation and saves.
    IpcWrite {
        command: IpcCommand,
    },
}

/// How long after a focus change we wait for an IME Activate before
/// concluding the app does not speak the input-method path. Generous so a
/// slow compositor never causes a false verdict (R11).
const PROBE_DELAY: std::time::Duration = std::time::Duration::from_millis(1500);

/// One dedicated delay thread for app-support probes. The MAIN loop stays a
/// pure recv (R15) — the sleep lives here, and the thread blocks on its own
/// channel when idle (zero CPU). Rapid focus changes simply queue up;
/// stale verdicts are discarded by the receiver.
pub fn spawn_probe_timer(tx: Sender<DaemonEvent>) -> Sender<String> {
    let (probe_tx, probe_rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        for app_id in probe_rx {
            std::thread::sleep(PROBE_DELAY);
            if tx.send(DaemonEvent::ProbeTimeout(app_id)).is_err() {
                return; // daemon gone
            }
        }
    });
    probe_tx
}

/// Forward focus events from the compositor stream into the unified bus.
/// A trivial blocking pump — zero CPU while idle.
pub fn spawn_focus_forwarder(focus_rx: Receiver<FocusEvent>, tx: Sender<DaemonEvent>) {
    std::thread::spawn(move || {
        for ev in focus_rx {
            if tx.send(DaemonEvent::Focus(ev)).is_err() {
                return;
            }
        }
    });
}

/// Watch the config file via inotify and emit `ConfigChanged`.
/// Watches the parent directory (editors/settings often replace the file,
/// which would invalidate a file-level watch) and filters by file name.
/// The thread blocks on the inotify fd — zero CPU until a write happens.
pub fn spawn_config_watch(config_path: &Path, tx: Sender<DaemonEvent>) {
    let Some(dir) = config_path.parent().map(|p| p.to_path_buf()) else {
        warn!("Config path has no parent dir — file watch disabled");
        return;
    };
    let Some(file_name) = config_path.file_name().map(|n| n.to_owned()) else {
        warn!("Config path has no file name — file watch disabled");
        return;
    };

    std::thread::spawn(move || {
        let mut inotify = match Inotify::init() {
            Ok(i) => i,
            Err(e) => {
                warn!("inotify init failed: {e} — config file watch disabled");
                return;
            }
        };
        // CLOSE_WRITE: in-place writes; MOVED_TO/CREATE: atomic replace
        let mask = WatchMask::CLOSE_WRITE | WatchMask::MOVED_TO | WatchMask::CREATE;
        if let Err(e) = inotify.watches().add(&dir, mask) {
            warn!("inotify watch on {dir:?} failed: {e} — config watch disabled");
            return;
        }
        info!("Config watch active on {dir:?} (inotify, event-driven)");

        let mut buffer = [0u8; 1024];
        loop {
            // Blocking read — the thread sleeps until the kernel has events
            let events = match inotify.read_events_blocking(&mut buffer) {
                Ok(evs) => evs,
                Err(e) => {
                    warn!("inotify read error: {e} — config watch stopped");
                    return;
                }
            };
            let mut hit = false;
            for event in events {
                if event.name.is_some_and(|n| n == file_name.as_os_str()) {
                    hit = true;
                }
            }
            if hit && tx.send(DaemonEvent::ConfigChanged).is_err() {
                return; // daemon gone
            }
        }
    });
}

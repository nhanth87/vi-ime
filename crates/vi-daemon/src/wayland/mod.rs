// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Wayland input-method-v2 protocol implementation.
//!
//! Live model: the app always shows the current rendered form of the word.
//! Vietnamese text is managed through delete_surrounding_text + commit_string
//! (suffix diff only); the zwp_virtual_keyboard_v1 forwarder re-injects every
//! grabbed key we do not turn into text (shortcuts, navigation, boundaries).

pub mod actions;
pub mod commit;
pub mod dispatch;
pub mod dispatch_stubs;
pub mod feedback;
pub mod runtime;
pub mod state;
pub mod viet_typer;
pub mod virtual_keyboard;
pub mod xkb;

pub use feedback::{FeedbackFn, ImeFeedback};
pub use runtime::{RuntimeConfig, RuntimeSnapshot};

use std::os::unix::io::AsRawFd;
use std::sync::Arc;

use tracing::{error, info, warn};
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::Connection;
use wayland_protocols_misc::zwp_input_method_v2::client::zwp_input_method_manager_v2::ZwpInputMethodManagerV2;
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1;
use crate::engine::fast_engine::{CompositorKind, NonPreeditEngine};

use crate::wayland::state::ImeAppState;

// ============================================================================
// Wayland protocol user data types
// ============================================================================

pub struct ImUserData;
pub struct KeyboardGrabUserData;
pub struct VkUserData;

// ============================================================================
// Public API
// ============================================================================

fn run_ime_internal(
    engine: NonPreeditEngine,
    compositor: CompositorKind,
    runtime: Option<Arc<RuntimeConfig>>,
    feedback: Option<feedback::FeedbackFn>,
) -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::connect_to_env()?;
    info!("Connected to Wayland compositor");

    let (globals, mut event_queue) = registry_queue_init::<ImeAppState>(&conn)?;
    let qh = event_queue.handle();

    let im_manager: ZwpInputMethodManagerV2 = globals
        .bind(&qh, 1..=1, ())
        .map_err(|_| "zwp_input_method_manager_v2 not available")?;
    info!("Bound zwp_input_method_manager_v2");

    let seat: WlSeat = globals
        .bind(&qh, 1..=9, ())
        .map_err(|_| "wl_seat not available")?;

    // Virtual keyboard: required to re-inject grabbed keys we don't consume.
    // Missing it means shortcuts/navigation die inside the grab — warn loudly.
    // (VietTyper — the SECOND virtual keyboard that types composed words —
    // opens its OWN Wayland connection now, see viet_typer.rs 2026-07-13:
    // it needs `roundtrip()` for real compositor confirmation, which would
    // be re-entrant dispatch if called on THIS event queue from inside a
    // key handler.)
    let virtual_keyboard = match globals.bind::<ZwpVirtualKeyboardManagerV1, _, _>(&qh, 1..=1, ()) {
        Ok(mgr) => Some(mgr.create_virtual_keyboard(&seat, &qh, VkUserData)),
        Err(_) => {
            warn!("zwp_virtual_keyboard_manager_v1 not available — passthrough keys will be LOST");
            None
        }
    };

    let input_method = im_manager.get_input_method(&seat, &qh, ImUserData);
    info!("Got zwp_input_method_v2");

    let mut state = ImeAppState::new(engine, compositor, virtual_keyboard);
    state.input_method = Some(input_method);
    state.feedback = feedback;
    if let Some(rt) = runtime {
        // Sync ime_enabled with the daemon's view before the first event.
        state.ime_enabled = rt.snapshot().enabled;
        state.runtime = Some(rt);
    }

    event_queue.roundtrip(&mut state)?;
    info!(
        "vi-wayland-im initialized - entering event loop (compositor: {:?})",
        compositor
    );

    // Event loop: blocks forever on the Wayland fd (zero-CPU idle, R15).
    // The only other wakeup source is the physical-click eventfd below.
    loop {
        if let Err(e) = event_queue.flush() {
            error!("Wayland flush error: {e}");
            break;
        }
        if let Err(e) = event_queue.dispatch_pending(&mut state) {
            error!("Wayland dispatch error: {e}");
            break;
        }

        let Some(read_guard) = event_queue.prepare_read() else {
            // Events arrived during dispatch — loop back to drain them.
            continue;
        };
        let fd = read_guard.connection_fd().as_raw_fd();
        // Second poll source: the physical-click eventfd. A click while a
        // preedit hangs must clear it IMMEDIATELY (R8) — the app reacts to
        // the click on its own; waiting for the next keystroke is too late.
        let click_fd = state
            .runtime
            .as_ref()
            .map(|rt| rt.click_fd())
            .unwrap_or(-1);
        let mut pfds = [
            libc::pollfd { fd, events: libc::POLLIN, revents: 0 },
            libc::pollfd {
                fd: click_fd,
                // Negative fds are legal in poll(2): ignored, revents = 0.
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // Timeout only while an idle auto-commit is armed (a composition
        // that exists solely as preedit) — otherwise block forever (R15).
        let timeout = state
            .idle_commit_deadline_ms()
            .map(|ms| ms.max(1))
            .unwrap_or(-1);
        let ret = unsafe { libc::poll(pfds.as_mut_ptr(), 2, timeout) };
        if ret > 0 && (pfds[1].revents & libc::POLLIN) != 0 {
            // Drain the eventfd (nonblocking) and drop any hanging word.
            let mut buf: u64 = 0;
            unsafe {
                libc::read(click_fd, (&raw mut buf).cast(), 8);
            }
            state.on_physical_click(&conn);
        }
        let pfd = pfds[0];
        if ret > 0 && (pfd.revents & libc::POLLIN) != 0 {
            if let Err(e) = read_guard.read() {
                // EAGAIN/EINTR are NOT fatal: poll() can wake spuriously and
                // the socket has nothing to read (seen live 2026-07-09 —
                // "os error 11" killed the whole IME). Just poll again.
                use wayland_client::backend::WaylandError;
                let benign = matches!(
                    &e,
                    WaylandError::Io(io) if matches!(
                        io.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    )
                );
                if !benign {
                    error!("Wayland read error: {e}");
                    break;
                }
            }
        } else {
            // Timeout (or signal): release the read intent without reading.
            drop(read_guard);
        }
        if ret == 0 {
            // Poll timeout: the idle auto-commit deadline passed.
            state.idle_commit(&conn);
        }

        if let Err(e) = event_queue.dispatch_pending(&mut state) {
            error!("Wayland dispatch error: {e}");
            break;
        }

        if !state.active && state.keyboard_grab.is_some()
            && let Some(grab) = state.keyboard_grab.take() {
                grab.release();
            }
    }

    info!("vi-wayland-im event loop ended");
    Ok(())
}

/// Run the IME with a live-reconfigurable shared config plus a feedback
/// callback the daemon uses to receive hard protocol signals ([`ImeFeedback`])
/// for the learned cache, telemetry, and user notifications. The daemon keeps
/// the `Arc<RuntimeConfig>` and calls `store()` to change method/mode/output/
/// enabled at runtime — no IME restart needed. The callback must be
/// non-blocking (a channel send).
pub fn run_ime_shared_with_feedback(
    runtime: Arc<RuntimeConfig>,
    feedback: Option<feedback::FeedbackFn>,
) -> Result<(), Box<dyn std::error::Error>> {
    let compositor = CompositorKind::detect();
    let snap = runtime.snapshot();
    let mut engine = NonPreeditEngine::new(snap.method, snap.mode);
    runtime::apply_snapshot(&mut engine, &snap);
    run_ime_internal(engine, compositor, Some(runtime), feedback)
}

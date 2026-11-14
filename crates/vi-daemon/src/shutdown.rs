// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Save what was learned before the daemon exits. Learned words and habits
//! were only written on an app switch, so "Thoát vi-im", `systemctl stop` or
//! a logout lost everything typed since (2026-11-14: a restart would have
//! dropped "nồi than" typed minutes before).

use std::sync::Once;
use std::sync::atomic::{AtomicI32, Ordering};

static INSTALLED: Once = Once::new();
/// Write end of the self-pipe the signal handler pokes.
static PIPE_W: AtomicI32 = AtomicI32::new(-1);

fn save() {
    crate::engine::predict::save_now();
    crate::engine::habit::save_now();
}

/// Save, then exit (IPC `quit`, tray quit, signals).
pub(crate) fn exit(code: i32) -> ! {
    save();
    std::process::exit(code)
}

extern "C" fn on_signal(sig: libc::c_int) {
    let b = sig as u8;
    // SAFETY: write(2) is async-signal-safe; one byte to our own pipe.
    unsafe { libc::write(PIPE_W.load(Ordering::Relaxed), (&raw const b).cast(), 1) };
}

/// SIGTERM/SIGINT/SIGHUP → a waiting thread saves and exits. A handler, not
/// a blocked mask: blocked signals are inherited by every child (suggestion
/// bar, settings window), which then ignored `systemctl stop` and lingered
/// as orphans (2026-11-14). Handlers are reset to default on exec.
pub(crate) fn install() {
    INSTALLED.call_once(|| {
        let mut fds = [0 as libc::c_int; 2];
        // SAFETY: plain libc calls on local, initialised storage.
        unsafe {
            if libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) != 0 {
                return;
            }
            PIPE_W.store(fds[1], Ordering::Relaxed);
            let mut sa: libc::sigaction = std::mem::zeroed();
            sa.sa_sigaction = on_signal as libc::sighandler_t;
            sa.sa_flags = libc::SA_RESTART;
            libc::sigemptyset(&mut sa.sa_mask);
            for sig in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
                libc::sigaction(sig, &sa, std::ptr::null_mut());
            }
        }
        let read_fd = fds[0];
        std::thread::spawn(move || {
            let mut b = 0u8;
            // SAFETY: blocking read of one byte from our own pipe.
            if unsafe { libc::read(read_fd, (&raw mut b).cast(), 1) } == 1 {
                tracing::info!("signal {b}: saving learned data, exiting");
                exit(0);
            }
        });
    });
}

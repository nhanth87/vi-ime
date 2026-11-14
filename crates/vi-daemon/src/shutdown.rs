// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Save what was learned before the daemon exits. Learned words and habits
//! were only written on an app switch, so "Thoát vi-im", `systemctl stop` or
//! a logout lost everything typed since (2026-11-14: a restart would have
//! dropped "nồi than" typed minutes before).

use std::sync::Once;

static BLOCKED: Once = Once::new();

fn save() {
    crate::engine::predict::save_now();
    crate::engine::habit::save_now();
}

/// Save, then exit (IPC `quit`).
pub(crate) fn exit(code: i32) -> ! {
    save();
    std::process::exit(code)
}

/// Call before any thread starts: SIGTERM/SIGINT/SIGHUP are blocked in
/// every thread and taken by one waiting thread, which saves and exits.
pub(crate) fn install() {
    BLOCKED.call_once(|| {
        // SAFETY: plain libc signal-mask calls on a zeroed, initialised set.
        let set = unsafe {
            let mut set: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut set);
            for sig in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
                libc::sigaddset(&mut set, sig);
            }
            if libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut()) != 0 {
                return;
            }
            set
        };
        std::thread::spawn(move || {
            let mut sig: libc::c_int = 0;
            // SAFETY: `set` is the initialised mask blocked above.
            if unsafe { libc::sigwait(&set, &mut sig) } == 0 {
                tracing::info!("signal {sig}: saving learned data, exiting");
                exit(0);
            }
        });
    });
}

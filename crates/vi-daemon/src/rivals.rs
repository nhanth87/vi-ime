// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 vi-im contributors
//! Rival input-method detection & takeover.
//!
//! `zwp_input_method_v2` is single-owner per seat: whoever binds the
//! input-method first owns it. If fcitx5/ibus is already running, vi-ime gets
//! `Event::Unavailable` and cannot override it at the protocol level. So we
//! detect the rival by scanning `/proc`, tell the user exactly what is blocking
//! us, and — only on explicit opt-in (`--take-over`) — stop it so vi-ime
//! becomes the SOLE input method. We never kill anything silently.

use std::fs;
use std::process::{Command, Stdio};

/// Known competing input methods: process `comm` name → systemd --user unit
/// (if it ships one). The unit lets us disable autostart, not just kill once.
const KNOWN: &[(&str, Option<&str>)] = &[
    ("fcitx5", Some("fcitx5")),
    ("fcitx", Some("fcitx")),
    ("ibus-daemon", Some("ibus")),
    ("ibus-x11", Some("ibus")),
    ("gcin", None),
    ("hime", None),
    ("uim-xim", None),
    ("uim-toolbar", None),
    ("scim", None),
    ("kimpanel", None),
    ("nimf", None),
];

/// A running rival IME process.
pub struct Rival {
    pub proc_name: &'static str,
    pub pid: u32,
    pub service: Option<&'static str>,
}

/// Scan `/proc/*/comm` for running rival IME processes (deduplicated by
/// name), PLUS any other process also named `vi-ime` — now that vi-ime
/// ships three ways (systemd service, direct binary, AppImage), the most
/// common real-world "rival" is actually a leftover copy of ITSELF: a
/// console-run instance from an earlier test session still holding the
/// seat, so a freshly launched AppImage/systemd instance gets
/// `Event::Unavailable` and silently passes every key through raw with no
/// Vietnamese conversion (looks like "the AppImage doesn't type Vietnamese
/// but the console binary does" — same binary, wrong instance won the
/// race). Matched by comm name, so it also catches an AppImage-extracted
/// copy (same binary name inside the mount).
pub fn detect() -> Vec<Rival> {
    let mut found: Vec<Rival> = Vec::new();
    let my_pid = std::process::id();
    let Ok(entries) = fs::read_dir("/proc") else {
        return found;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid_str) = name.to_str() else { continue };
        let Ok(pid) = pid_str.parse::<u32>() else { continue };
        let comm = fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
        let comm = comm.trim();
        if comm == "vi-ime" {
            if pid != my_pid && !found.iter().any(|r| r.proc_name == "vi-ime") {
                found.push(Rival { proc_name: "vi-ime", pid, service: Some("vi-ime") });
            }
            continue;
        }
        // Prefix match, not exact: /proc comm is truncated to 15 chars and
        // rivals ship helper daemons — the field case (2026-07-10) was
        // `fcitx5_uinput_server` (comm "fcitx5_uinput_s"), an evdev-level
        // key injector that never matched "fcitx5" exactly, so it silently
        // fought vi-ime for every keystroke (mất dấu, nuốt shortcut).
        if let Some((n, svc)) = KNOWN.iter().find(|(n, _)| comm.starts_with(*n)) {
            // One entry per distinct rival name is enough for the message.
            if !found.iter().any(|r| r.proc_name == *n) {
                found.push(Rival { proc_name: n, pid, service: *svc });
            }
        }
    }
    found
}

/// Short human summary for logs / doctor, e.g. "fcitx5 (pid 1234), ibus-daemon (pid 5678)".
pub fn describe(rivals: &[Rival]) -> String {
    rivals
        .iter()
        .map(|r| format!("{} (pid {})", r.proc_name, r.pid))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Best-effort takeover: disable each rival's systemd --user unit (so it does
/// not autostart next login) and SIGTERM the running process. Returns how many
/// rivals we acted on. Never panics; missing systemctl / permission errors are
/// ignored (the kill still lands for user-owned processes).
pub fn take_over(rivals: &[Rival]) -> usize {
    let mut acted = 0;
    for r in rivals {
        if let Some(svc) = r.service {
            // Stop + disable autostart; ignore failures (unit may not exist —
            // fcitx5 often autostarts via xdg .desktop, handled by install.sh).
            let _ = Command::new("systemctl")
                .args(["--user", "disable", "--now", svc])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        // SIGTERM the live process so it releases the input-method seat now.
        let killed = unsafe { libc::kill(r.pid as libc::pid_t, libc::SIGTERM) } == 0;
        if killed || r.service.is_some() {
            acted += 1;
        }
    }
    acted
}

// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! /proc inspection — cheap kernel-boundary signals about the focused app.
//!
//! Only used when the compositor IPC provides a PID (niri does). Read-only,
//! best-effort: any error just means "no advice".

use std::fs;
use std::path::Path;

/// What we could read about a process.
#[derive(Debug, Clone, Default)]
pub struct ProcInfo {
    /// Resolved /proc/PID/exe target (lossy string, may be empty).
    pub exe: String,
    /// /proc/PID/cmdline with NULs replaced by spaces.
    pub cmdline: String,
}

/// Read exe + cmdline for a PID. None when the process is gone/unreadable.
pub fn read_proc(pid: i32) -> Option<ProcInfo> {
    let cmdline_raw = fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let cmdline = String::from_utf8_lossy(&cmdline_raw).replace('\0', " ");
    let exe = fs::read_link(format!("/proc/{pid}/exe"))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    Some(ProcInfo { exe, cmdline })
}

/// Does the directory containing `exe` look like an electron-builder /
/// electron-packager output? Checked live 2026-07-10 against an app whose
/// binary and cmdline contain no "electron" substring at all (renamed
/// product, e.g. `orca-ide --no-sandbox`) yet is unmistakably Electron —
/// `strings` on the binary shows `electron::fuses` symbols, and the install
/// dir sits right next to it with the files below. These three are bundled
/// by every mainstream Electron packaging tool regardless of product name:
///   - `LICENSE.electron.txt`  — Electron's own MIT notice, always shipped
///   - `chrome-sandbox`        — the setuid sandbox helper binary
///   - `resources/app.asar`    — the packaged app archive
/// Far more reliable than string-matching the exe path/cmdline for
/// "electron", which misses any app that renames its binary/install dir.
fn looks_like_electron_dir(exe_path: &str) -> bool {
    let Some(dir) = Path::new(exe_path).parent() else { return false };
    dir.join("LICENSE.electron.txt").is_file()
        || dir.join("chrome-sandbox").is_file()
        || dir.join("resources").join("app.asar").is_file()
}

/// Electron apps need explicit flags to speak Wayland text-input; without
/// them the IME never sees an Activate. Returns actionable Vietnamese
/// advice when the process looks like Electron AND the flag is missing.
/// Pure (besides the directory-marker stat calls) — unit-testable with a
/// synthetic ProcInfo pointing at a real or fake path.
pub fn electron_advice(info: &ProcInfo) -> Option<String> {
    let hay = format!("{} {}", info.exe, info.cmdline).to_lowercase();
    let looks_electron = hay.contains("electron")
        || hay.contains("app.asar")
        || hay.contains("--ozone-platform")
        || looks_like_electron_dir(&info.exe);
    if !looks_electron {
        return None;
    }
    if info.cmdline.contains("--enable-wayland-ime") {
        return None; // flag already present — not the problem
    }
    Some(
        "Đây là app Electron chạy thiếu cờ IME. Thêm vào lệnh khởi động:\n--enable-wayland-ime --wayland-text-input-version=3"
            .to_string(),
    )
}

/// Generic advice when an app never attaches a text input.
pub fn generic_advice() -> String {
    "App chưa nhận bộ gõ (không thấy text-input). Mở Bảng điều khiển vi-ime → tab Ứng dụng để đặt chế độ riêng cho app này."
        .to_string()
}


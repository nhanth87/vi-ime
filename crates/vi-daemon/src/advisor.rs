// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 vi-im contributors
//! /proc inspection — cheap kernel-boundary signals about the focused app.
//!
//! Only used when the compositor IPC provides a PID (niri does). Read-only,
//! best-effort: any error just means "no advice".

use std::fs;

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

/// Electron apps need explicit flags to speak Wayland text-input; without
/// them the IME never sees an Activate. Returns actionable Vietnamese
/// advice when the process looks like Electron AND the flag is missing.
/// Pure — unit-testable.
pub fn electron_advice(info: &ProcInfo) -> Option<String> {
    let hay = format!("{} {}", info.exe, info.cmdline).to_lowercase();
    let looks_electron =
        hay.contains("electron") || hay.contains("app.asar") || hay.contains("--ozone-platform");
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


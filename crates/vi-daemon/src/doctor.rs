// SPDX-License-Identifier: GPL-3.0-or-later OR Commercial
// Copyright (c) 2024-2026 vi-im contributors
//! `vi-ime --doctor` — one-shot diagnosis, the vi-im analogue of
//! fcitx5-diagnose. Walks the stack bottom-up and says which LAYER is
//! broken, so users paste ONE output into a bug report instead of running
//! WAYLAND_DEBUG archaeology themselves.
//!
//! Layers checked:
//!   L0 env       — Wayland session sanity (variables are hints only)
//!   L1 globals   — what the compositor actually offers (ground truth)
//!   L2 learned   — per-app capability observed at runtime
//!   L3 telemetry — where keystrokes lost time (blame per app)

use crate::compositor::probe;
use crate::config::LearnedStore;
use crate::engine::fast_engine::CompositorKind;
use crate::telemetry::Telemetry;

/// Interfaces vi-im needs (hard) or benefits from (soft).
const NEED: &[(&str, &str, bool)] = &[
    ("zwp_input_method_manager_v2", "IME protocol — KHÔNG có là không chạy được", true),
    ("zwp_virtual_keyboard_manager_v1", "forward phím passthrough (shortcut/nav)", true),
    ("zwp_text_input_manager_v3", "phía app — thiếu nghĩa là app không nói chuyện được với IME", true),
    ("zwlr_foreign_toplevel_manager_v1", "focus tracking generic (không có → cần niri/hyprland IPC)", false),
];

pub fn run() {
    println!("═══ vi-ime doctor ═══\n");

    // ── L0: environment (hints, not ground truth) ──
    println!("── L0 · Môi trường ──");
    for var in [
        "WAYLAND_DISPLAY",
        "XDG_CURRENT_DESKTOP",
        "XDG_SESSION_TYPE",
        "NIRI_SOCKET",
        "HYPRLAND_INSTANCE_SIGNATURE",
        "SWAYSOCK",
        "DISPLAY",
    ] {
        match std::env::var(var) {
            Ok(v) => println!("  {var} = {v}"),
            Err(_) => println!("  {var} (unset)"),
        }
    }
    let compositor = CompositorKind::detect();
    println!("  → compositor detect: {compositor:?}");
    if std::env::var("DISPLAY").is_ok() {
        println!("  → XWayland có mặt: app X11 KHÔNG đi qua input-method-v2 (xem DEBUG.md)");
    }

    // ── L0.5: rival input methods (single-owner seat) ──
    println!("\n── L0.5 · IME đối thủ (seat single-owner) ──");
    let rivals = crate::rivals::detect();
    if rivals.is_empty() {
        println!("  ✅ Không có IME khác đang chạy — vi-ime độc chiếm seat.");
    } else {
        for r in &rivals {
            println!(
                "  ⚠️  {} (pid {}) đang chạy → sẽ chặn vi-ime giữ seat.",
                r.proc_name, r.pid
            );
        }
        println!("  → `vi-ime --take-over` để dừng + tắt autostart chúng.");
    }

    // ── L1: registry globals (the ground truth) ──
    println!("\n── L1 · Wayland globals (sự thật duy nhất) ──");
    let globals = probe::list_globals();
    if globals.is_empty() {
        println!("  ❌ Không kết nối được Wayland display — vi-im không thể chạy ở đây.");
        return;
    }
    let mut fatal = false;
    for (iface, why, hard) in NEED {
        let ok = probe::has_global(&globals, iface);
        let mark = if ok { "✅" } else if *hard { "❌" } else { "⚠️ " };
        println!("  {mark} {iface} — {why}");
        fatal |= !ok && *hard;
    }
    if fatal {
        println!("\n  KẾT LUẬN: compositor này thiếu protocol bắt buộc → dùng wlroots-based");
        println!("  (niri / Hyprland / Sway / river). GNOME/KWin: dùng fcitx5/ibus.");
        return;
    }

    // ── L2: learned capabilities ──
    println!("\n── L2 · Learned per-app (learned.toml) ──");
    let learned = LearnedStore::load(&LearnedStore::default_path());
    if learned.apps.is_empty() {
        println!("  (chưa có gì — sẽ tự học khi bạn gõ)");
    } else {
        let mut names: Vec<&String> = learned.apps.keys().collect();
        names.sort();
        for name in names {
            let Some(p) = learned.profile(name) else { continue };
            let surr = match p.surrounding_text {
                Some(true) => "surrounding ✅",
                Some(false) => "surrounding ❌ (→ preedit fallback)",
                None => "surrounding ?",
            };
            let act = match p.ime_activated {
                Some(true) => "activate ✅",
                _ => "activate ?",
            };
            println!(
                "  {name}: {act}, {surr}, ack ema {}µs, timeouts {}",
                p.done_ack_ema_us.unwrap_or(0),
                p.done_timeouts
            );
        }
    }

    // ── L3: telemetry blame ──
    println!("\n── L3 · Blame — phím kẹt ở tầng nào (telemetry.toml) ──");
    let telemetry = Telemetry::load(&Telemetry::default_path());
    if telemetry.apps.is_empty() {
        println!("  (chưa có số liệu)");
    } else {
        let mut blamed = false;
        let mut names: Vec<&String> = telemetry.apps.keys().collect();
        names.sort();
        for name in &names {
            if let Some(verdict) = telemetry.blame(name) {
                println!("  🔥 {verdict}");
                blamed = true;
            }
        }
        if !blamed {
            println!("  ✅ Không app nào vượt ngưỡng — pipeline khoẻ.");
        }
        print!("\n{}", indent(&telemetry.report(), "  "));
    }

    // ── L4: evdev fallback readiness (for X11/XWayland apps) ──
    println!("\n── L4 · evdev fallback (app X11, dùng `--evdev`) ──");
    for line in crate::evdev_mode::doctor_lines() {
        println!("  {line}");
    }

    println!("\nTrace sâu hơn từng tầng: xem docs/DEBUG.md (WAYLAND_DEBUG, godmod, compositor log).");
}

fn indent(s: &str, pad: &str) -> String {
    s.lines().map(|l| format!("{pad}{l}\n")).collect()
}

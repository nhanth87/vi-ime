// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 vi-im contributors
//! Unicode injection + keycode mapping for the evdev fallback
//! (split from evdev_mode.rs to honor the 300-line rule, R4).

use std::process::Command;

use evdev::KeyCode;
use tracing::{info, warn};

use crate::evdev_typer::EvdevTyper;

/// Unicode output channel: a persistent virtual keyboard when the
/// compositor supports it (Wayland), else the xdotool fallback (X11).
pub(crate) enum Typer {
    Native(EvdevTyper),
    Cmd(Injector),
}

impl Typer {
    pub(crate) fn detect() -> Option<Self> {
        if let Some(t) = EvdevTyper::new() {
            info!("evdev fallback: Unicode qua virtual keyboard bền vững (native)");
            return Some(Typer::Native(t));
        }
        Injector::detect().map(|inj| {
            info!("evdev fallback: Unicode qua {}", inj.name());
            Typer::Cmd(inj)
        })
    }

    pub(crate) fn backspace_then_type(&mut self, backspaces: usize, text: &str, sync: bool) {
        let ok = match self {
            Typer::Native(t) => t.backspace_then_type(backspaces, text, sync),
            Typer::Cmd(inj) => {
                inj.backspace_then_type(backspaces, text);
                // External process always blocks until exit — equivalent to
                // sync=true; that's the best xdotool/wtype can do.
                true
            }
        };
        if !ok {
            // shown-tracking is now desynced from the screen for this word;
            // the log is the evidence trail (R17: identify mechanism first).
            warn!("[EVDEV-TYPER] gõ thất bại (bs={backspaces}, text={text:?}) — từ này có thể sai trên màn hình");
        }
    }
}

/// Minimal US-QWERTY evdev keycode → char (lowercase unless `shift`). Enough for
/// Telex/VNI composition; non-letter keys return None (forwarded verbatim).
pub(crate) fn key_to_char(code: KeyCode, shift: bool) -> Option<char> {
    let base = match code {
        KeyCode::KEY_A => 'a', KeyCode::KEY_B => 'b', KeyCode::KEY_C => 'c',
        KeyCode::KEY_D => 'd', KeyCode::KEY_E => 'e', KeyCode::KEY_F => 'f',
        KeyCode::KEY_G => 'g', KeyCode::KEY_H => 'h', KeyCode::KEY_I => 'i',
        KeyCode::KEY_J => 'j', KeyCode::KEY_K => 'k', KeyCode::KEY_L => 'l',
        KeyCode::KEY_M => 'm', KeyCode::KEY_N => 'n', KeyCode::KEY_O => 'o',
        KeyCode::KEY_P => 'p', KeyCode::KEY_Q => 'q', KeyCode::KEY_R => 'r',
        KeyCode::KEY_S => 's', KeyCode::KEY_T => 't', KeyCode::KEY_U => 'u',
        KeyCode::KEY_V => 'v', KeyCode::KEY_W => 'w', KeyCode::KEY_X => 'x',
        KeyCode::KEY_Y => 'y', KeyCode::KEY_Z => 'z',
        KeyCode::KEY_1 => '1', KeyCode::KEY_2 => '2', KeyCode::KEY_3 => '3',
        KeyCode::KEY_4 => '4', KeyCode::KEY_5 => '5', KeyCode::KEY_6 => '6',
        KeyCode::KEY_7 => '7', KeyCode::KEY_8 => '8', KeyCode::KEY_9 => '9',
        KeyCode::KEY_0 => '0',
        _ => return None,
    };
    // Digits are tone/quality keys in VNI — never uppercase them.
    if shift && base.is_ascii_alphabetic() {
        Some(base.to_ascii_uppercase())
    } else {
        Some(base)
    }
}

/// External Unicode typer (uinput cannot emit arbitrary Unicode).
pub(crate) enum Injector {
    Wtype,
    Xdotool,
}
impl Injector {
    pub(crate) fn detect() -> Option<Self> {
        // Prefer wtype under Wayland; fall back to xdotool for X11/XWayland.
        if std::env::var_os("WAYLAND_DISPLAY").is_some() && which("wtype") {
            Some(Injector::Wtype)
        } else if which("xdotool") {
            Some(Injector::Xdotool)
        } else if which("wtype") {
            Some(Injector::Wtype)
        } else {
            None
        }
    }
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Injector::Wtype => "wtype",
            Injector::Xdotool => "xdotool",
        }
    }
    /// BackSpace × n, then type `text` — one process, so the app receives
    /// everything in order. `text` only ever holds engine output (letters,
    /// digits, diacritics), never something an option parser could eat.
    pub(crate) fn backspace_then_type(&self, backspaces: usize, text: &str) {
        if backspaces == 0 && text.is_empty() {
            return;
        }
        let _ = match self {
            Injector::Wtype => {
                let mut cmd = Command::new("wtype");
                for _ in 0..backspaces {
                    cmd.args(["-k", "BackSpace"]);
                }
                if !text.is_empty() {
                    cmd.arg(text);
                }
                cmd.status()
            }
            Injector::Xdotool => {
                let mut cmd = Command::new("xdotool");
                if backspaces > 0 {
                    cmd.arg("key");
                    for _ in 0..backspaces {
                        cmd.arg("BackSpace");
                    }
                }
                if !text.is_empty() {
                    cmd.args(["type", "--", text]);
                }
                cmd.status()
            }
        };
    }
}

fn which(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|p| p.join(bin).is_file())
        })
        .unwrap_or(false)
}

/// Readiness lines for `--doctor`: how many keyboards we can see and whether a
/// Unicode typer exists. Read-only (never grabs).
pub fn doctor_lines() -> Vec<String> {
    let mut out = Vec::new();
    let kbds = evdev::enumerate()
        .filter(|(_, d)| {
            d.supported_keys()
                .is_some_and(|k| k.contains(KeyCode::KEY_A) && k.contains(KeyCode::KEY_Z))
        })
        .count();
    if kbds > 0 {
        out.push(format!("✅ thấy {kbds} bàn phím có thể grab (quyền /dev/input OK)"));
    } else {
        out.push(
            "❌ không mở được /dev/input — thêm group `input`: sudo usermod -aG input $USER (rồi re-login)"
                .to_string(),
        );
    }
    if crate::evdev_typer::EvdevTyper::new().is_some() {
        out.push("✅ Unicode typer: virtual keyboard (native, không cần wtype)".to_string());
    } else {
        match Injector::detect() {
            Some(inj) => out.push(format!("✅ Unicode typer: {} (fallback)", inj.name())),
            None => out.push(
                "❌ compositor thiếu zwp_virtual_keyboard_v1 và không có `xdotool` để gõ Unicode"
                    .to_string(),
            ),
        }
    }
    out
}

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
    /// `force_xdotool`: true for apps whose editable surface is confirmed
    /// broken with the native virtual-keyboard typer (see
    /// `legacy_grab::needs_injector_typer` — currently ONLYOFFICE only).
    ///
    /// Root cause (field bug 2026-07-12): ONLYOFFICE Desktop Editors hosts
    /// its document-editing surface as an embedded Chromium (CEF) child
    /// window inside the outer Qt/XCB shell (`QXcbConnection`). Regular
    /// standalone Chrome/Chromium ALSO falls back to this same native
    /// typer under XWayland and types correctly (see
    /// `legacy_grab::XWAYLAND_FALLBACK_PREFIXES` — field-proven), so this
    /// is not a general "XWayland can't do Mod3/Mod5" limitation. What's
    /// different for ONLYOFFICE is the extra embedding hop: key events hit
    /// the outer Qt window first, which forwards them into the CEF child.
    /// That forwarding path drops the synthetic Mod3/Mod5 modifier state
    /// our static 8-level keymap uses to select the Vietnamese glyph
    /// level, so every composed key decoded at level 1/2 instead — typing
    /// "cửa hàng á phi âu" produced the base-keycode ASCII punctuation of
    /// the underlying SAFE_CODES row instead. `xdotool` sidesteps this
    /// entirely: it types via a temporary per-character keysym remap +
    /// `XTestFakeKeyEvent`, never touching a custom modifier mask, so
    /// there is nothing for the embedding hop to drop.
    pub(crate) fn detect(force_xdotool: bool) -> Option<Self> {
        if force_xdotool {
            if let Some(inj) = Injector::detect_xdotool_preferred() {
                info!(
                    "evdev fallback: Unicode qua {} (app cần xdotool — virtual keyboard mất level Mod3/Mod5)",
                    inj.name()
                );
                return Some(Typer::Cmd(inj));
            }
            warn!(
                "evdev fallback: app cần xdotool nhưng không tìm thấy — \
                 dùng virtual keyboard, có thể gõ sai dấu"
            );
        }
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
            // External process always blocks until exit — equivalent to
            // sync=true; that's the best xdotool/wtype can do.
            Typer::Cmd(inj) => inj.backspace_then_type(backspaces, text),
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
pub(crate) enum InjectorKind {
    Wtype,
    Xdotool,
}

pub(crate) struct Injector {
    kind: InjectorKind,
}
impl Injector {
    fn from_kind(kind: InjectorKind) -> Self {
        Self { kind }
    }

    pub(crate) fn detect() -> Option<Self> {
        // Prefer wtype under Wayland; fall back to xdotool for X11/XWayland.
        if std::env::var_os("WAYLAND_DISPLAY").is_some() && which("wtype") {
            Some(Self::from_kind(InjectorKind::Wtype))
        } else if which("xdotool") {
            Some(Self::from_kind(InjectorKind::Xdotool))
        } else if which("wtype") {
            Some(Self::from_kind(InjectorKind::Wtype))
        } else {
            None
        }
    }

    /// For apps that need the injector specifically for its `XTestFakeKeyEvent`
    /// X11 delivery (see `Typer::detect`'s `force_xdotool` doc) — xdotool
    /// first regardless of `WAYLAND_DISPLAY` (wtype's virtual-keyboard
    /// protocol path is what we're routing AROUND).
    pub(crate) fn detect_xdotool_preferred() -> Option<Self> {
        if which("xdotool") {
            Some(Self::from_kind(InjectorKind::Xdotool))
        } else if which("wtype") {
            Some(Self::from_kind(InjectorKind::Wtype))
        } else {
            None
        }
    }
    pub(crate) fn name(&self) -> &'static str {
        match self.kind {
            InjectorKind::Wtype => "wtype",
            InjectorKind::Xdotool => "xdotool",
        }
    }
    /// BackSpace × n, then type `text` — one process, so the app receives
    /// everything in order. `text` only ever holds engine output (letters,
    /// digits, diacritics), never something an option parser could eat.
    pub(crate) fn backspace_then_type(&mut self, backspaces: usize, text: &str) -> bool {
        if backspaces == 0 && text.is_empty() {
            return true;
        }
        let status = match self.kind {
            InjectorKind::Wtype => {
                let mut cmd = Command::new("wtype");
                for _ in 0..backspaces {
                    cmd.args(["-d", "15", "-k", "BackSpace"]);
                }
                if !text.is_empty() {
                    cmd.args(["-d", "15"]);
                    cmd.arg(text);
                }
                cmd.status()
            }
            InjectorKind::Xdotool => {
                let mut cmd = Command::new("xdotool");
                // vi-daemon inherits whatever DISPLAY its own launcher had —
                // field bug 2026-07-12: a session with a stale/mismatched
                // DISPLAY (login env says :1, the real Xwayland socket
                // OnlyOffice's X11 clients use is :0) makes xdotool fail
                // with "Failed creating new xdo instance" and every
                // keystroke silently drops. Same fix as the
                // onlyoffice-desktopeditors wrapper script: probe for a
                // live socket in /tmp/.X11-unix and override DISPLAY for
                // just this child process, not the whole daemon.
                if let Some(display) = resolve_x11_display() {
                    cmd.env("DISPLAY", display);
                }
                // --delay 15 (per-subcommand, xdotool has no single global
                // flag when chaining `key ... type ...` in one invocation):
                // for non-ASCII, `xdotool type`/`key` remaps a scratch
                // keycode to the target Unicode codepoint, taps it, then
                // unmaps it — the SAME "keymap động" pattern this codebase
                // bans for its own native typer (viet_typer.rs) because a
                // lagging client can decode a tap against the OLD mapping
                // and drop the char. Field-confirmed 2026-07-12: unpaced
                // xdotool into ONLYOFFICE's embedded CEF surface dropped a
                // different, random char on each of 3 back-to-back runs of
                // the same string ("Cửa"→"Ửa", "áo"→"o", "sắt"→"st") — a
                // burst/lag issue, not a one-time warm-up issue (an earlier
                // fix that paused only before the FIRST keystroke of a grab
                // session did not help). Pace every tap, same 15ms as the
                // native typer's proven-safe interval.
                if backspaces > 0 {
                    cmd.args(["key", "--delay", "15"]);
                    for _ in 0..backspaces {
                        cmd.arg("BackSpace");
                    }
                }
                if !text.is_empty() {
                    cmd.args(["type", "--delay", "15", "--", text]);
                }
                let s = cmd.status();
                // Live-echo calls this on almost every keystroke — a new
                // xdotool PROCESS per call. `cmd.status()` only confirms
                // xdotool finished QUEUING its X11 events, not that
                // ONLYOFFICE's embedded CEF surface consumed/rendered them
                // before the next invocation's keymap remap begins. Same
                // 15ms settle the native typer pays between taps (R17:
                // "always pace" against legacy apps), applied here between
                // separate xdotool invocations instead of between taps
                // inside one.
                std::thread::sleep(std::time::Duration::from_millis(15));
                s
            }
        };
        match status {
            Ok(s) if s.success() => true,
            Ok(s) => {
                warn!("[EVDEV-INJECTOR] {} exited với status {s} (bs={backspaces}, text={text:?})", self.name());
                false
            }
            Err(e) => {
                warn!("[EVDEV-INJECTOR] không spawn được {}: {e}", self.name());
                false
            }
        }
    }
}

/// Find a live X11 socket for the injector child process. `DISPLAY` as
/// inherited by vi-daemon may point at a socket that no longer serves the
/// XWayland instance the target app actually connects to (multi-seat /
/// nested-X setups, or a wrapper script's login-env mismatch) — verify the
/// inherited value first, only scan `/tmp/.X11-unix` if it doesn't resolve.
fn resolve_x11_display() -> Option<String> {
    if let Some(d) = std::env::var_os("DISPLAY") {
        let d = d.to_string_lossy();
        let num = d.strip_prefix(':').unwrap_or(&d);
        if std::path::Path::new(&format!("/tmp/.X11-unix/X{num}")).exists() {
            return None; // inherited DISPLAY already valid — no override needed
        }
    }
    let mut entries: Vec<_> = std::fs::read_dir("/tmp/.X11-unix").ok()?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    entries.into_iter().find_map(|e| {
        let name = e.file_name();
        let name = name.to_str()?;
        let num = name.strip_prefix('X')?;
        num.parse::<u32>().ok()?;
        Some(format!(":{num}"))
    })
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

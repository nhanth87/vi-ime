// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 vi-im contributors
//! EXPERIMENTAL evdev fallback for apps invisible to `zwp_input_method_v2`
//! (mainly XWayland/X11). Opt-in via `--evdev`; MUTUALLY EXCLUSIVE with the
//! Wayland input-method path — grabbing the keyboard means the compositor no
//! longer sees it, so vi-ime becomes the SOLE keyboard handler.
//!
//! Model (buffer-and-commit, like the terminal-preedit path): letter/tone keys
//! feed the engine and are NOT forwarded; every other key (space, punctuation,
//! Enter, arrows, shortcuts, modifiers) is mirrored 1:1 through a uinput device.
//! At a word boundary the composed word is typed into the focused app via
//! `wtype` (Wayland) or `xdotool` (X11) — uinput emits keycodes, not Unicode,
//! so an external typer is required for diacritics.
//!
//! SAFETY: the keyboard is ALWAYS ungrabbed on exit/panic (Drop). Worst case a
//! bug still lets you switch VT (Ctrl+Alt+F3) or `pkill vi-daemon` from another
//! machine/SSH. Never auto-enabled.

use std::process::Command;

use evdev::{Device, EventType, InputEvent, KeyCode};
use evdev::uinput::VirtualDevice;
use tracing::{error, info, warn};

use crate::engine::fast_engine::NonPreeditEngine;
use crate::engine::{ImeMode, InputMethod, NonPreeditAction};

/// A grabbed keyboard that ungrabs itself on drop (panic-safe).
struct Grabbed(Device);
impl Drop for Grabbed {
    fn drop(&mut self) {
        let _ = self.0.ungrab();
    }
}

/// Entry point for `--evdev`. Blocks forever (or until error). Never returns Ok
/// in normal operation; returns Err on a fatal setup problem so main can log it.
pub fn run(method: InputMethod) -> Result<(), Box<dyn std::error::Error>> {
    let injector = Injector::detect().ok_or(
        "no Unicode typer found — install `wtype` (Wayland) or `xdotool` (X11)",
    )?;
    info!("evdev mode: Unicode output via {}", injector.name());

    // Find real keyboards (devices that report letter keys).
    let mut keyboards: Vec<Grabbed> = Vec::new();
    for (path, dev) in evdev::enumerate() {
        let is_kbd = dev
            .supported_keys()
            .is_some_and(|k| k.contains(KeyCode::KEY_A) && k.contains(KeyCode::KEY_Z));
        if is_kbd {
            info!("evdev: grabbing {:?} ({})", path, dev.name().unwrap_or("?"));
            let mut dev = dev;
            match dev.grab() {
                Ok(()) => keyboards.push(Grabbed(dev)),
                Err(e) => warn!("evdev: cannot grab {:?}: {e} (need group `input`?)", path),
            }
        }
    }
    if keyboards.is_empty() {
        return Err(
            "no grabbable keyboard found. Add yourself to the `input` group \
             (sudo usermod -aG input $USER) and re-login, then retry `--evdev`."
            .into(),
        );
    }

    // uinput mirror: advertise every standard key so passthrough works.
    let mut keys = evdev::AttributeSet::<KeyCode>::new();
    for code in 1u16..=248 {
        keys.insert(KeyCode::new(code));
    }
    let mut ui = VirtualDevice::builder()?
        .name("vi-ime evdev virtual keyboard")
        .with_keys(&keys)?
        .build()?;

    let mut engine = NonPreeditEngine::new(method, ImeMode::NonPreedit);
    let mut shift = false;

    info!("evdev mode ACTIVE — vi-ime is the sole keyboard handler now.");
    loop {
        for kbd in &mut keyboards {
            let events: Vec<InputEvent> = match kbd.0.fetch_events() {
                Ok(evs) => evs.collect(),
                Err(e) => {
                    error!("evdev: read error: {e}");
                    return Ok(());
                }
            };
            for ev in events {
                if ev.event_type() != EventType::KEY {
                    continue;
                }
                let code = KeyCode::new(ev.code());
                let value = ev.value(); // 0=release 1=press 2=repeat

                // Track shift for ASCII casing.
                if matches!(code, KeyCode::KEY_LEFTSHIFT | KeyCode::KEY_RIGHTSHIFT) {
                    shift = value != 0;
                    emit(&mut ui, code, value);
                    continue;
                }

                // Only act on press/repeat for composition; releases of
                // consumed letter keys are simply dropped.
                let ch = key_to_char(code, shift);
                let is_letterish = ch.is_some();

                // Any modifier held (ctrl/alt/super) → flush + passthrough.
                // (We don't track them individually; treat as boundary.)
                if !is_letterish {
                    // Non-composing key: flush pending word first, then forward.
                    if value != 0 {
                        flush(&mut engine, &injector);
                    }
                    emit(&mut ui, code, value);
                    continue;
                }

                // Letter/digit/tone key: consume (do NOT forward) on press.
                if value == 0 {
                    continue; // swallow release of consumed key
                }
                let Some(ch) = ch else { continue };
                // Buffer/preedit/clear actions keep composing silently; only a
                // completed word produces output (typed via the injector).
                if let NonPreeditAction::CommitWithBackspace { text, .. } = engine.push_key(ch) {
                    injector.type_text(&text);
                }
            }
        }
    }
}

/// Commit any in-progress word (called before a boundary/shortcut key).
fn flush(engine: &mut NonPreeditEngine, injector: &Injector) {
    if engine.has_pending() {
        let text = engine.inner().preedit_output();
        engine.reset();
        if !text.is_empty() {
            injector.type_text(&text);
        }
    }
}

fn emit(ui: &mut VirtualDevice, code: KeyCode, value: i32) {
    let ev = InputEvent::new(EventType::KEY.0, code.code(), value);
    let _ = ui.emit(&[ev]);
}

/// Minimal US-QWERTY evdev keycode → char (lowercase unless `shift`). Enough for
/// Telex/VNI composition; non-letter keys return None (forwarded verbatim).
fn key_to_char(code: KeyCode, shift: bool) -> Option<char> {
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
enum Injector {
    Wtype,
    Xdotool,
}
impl Injector {
    fn detect() -> Option<Self> {
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
    fn name(&self) -> &'static str {
        match self {
            Injector::Wtype => "wtype",
            Injector::Xdotool => "xdotool",
        }
    }
    fn type_text(&self, text: &str) {
        let _ = match self {
            Injector::Wtype => Command::new("wtype").arg(text).status(),
            Injector::Xdotool => Command::new("xdotool").args(["type", "--", text]).status(),
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
    match Injector::detect() {
        Some(inj) => out.push(format!("✅ Unicode typer: {}", inj.name())),
        None => out.push("❌ thiếu `wtype` (Wayland) hoặc `xdotool` (X11) để gõ Unicode".to_string()),
    }
    out
}

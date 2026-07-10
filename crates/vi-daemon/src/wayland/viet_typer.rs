// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 vi-im contributors
//! VietTyper — a second virtual keyboard that types composed words.
//!
//! P0-3 hướng (b), "wtype-style". Both earlier NonPreedit designs died on
//! channel mixing: commit_string (text-input) does not stay ordered against
//! virtual-keyboard events, and delete_surrounding_text is silently ignored
//! by some apps. The only channel with guaranteed ordering AND universal
//! app support is wl_keyboard itself — so the word conversion happens
//! there: raw keys forward live (live style) and at the word boundary we
//! send Backspace × n followed by the composed characters typed here.
//!
//! **Per-word dynamic keymap** (field-tested 2026-07-09): a static keymap
//! spanning keycodes up to 170 lost exactly the chars that landed on
//! special evdev codes (â→107=KEY_END, đ→162…) while è(113)/ê(118)
//! survived — apps/compositors treat some hardware codes specially no
//! matter what the keymap says. So instead, before each word we upload a
//! tiny keymap that maps ONLY that word's unique chars onto the proven-safe
//! typing-row codes 2..=33, exactly like wtype. keymap + key events travel
//! on the same object, so ordering is guaranteed end-to-end. Any Unicode
//! char works (multilang-ready), no preedit → no underline.

use std::collections::HashMap;
use std::io::Write;
use std::os::fd::{AsFd, FromRawFd, OwnedFd};
use std::time::Instant;

use tracing::warn;
use wayland_client::Proxy;
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1;

/// wl_keyboard keymap format: xkb_v1.
pub(crate) const KEYMAP_FORMAT_XKB_V1: u32 = 1;

/// Safe evdev codes for injected keys: the main typing rows ('1'..'=',
/// QWERTY row…). Nothing here doubles as a navigation/media key anywhere.
pub(crate) const FIRST_CODE: u32 = 2;
const LAST_CODE: u32 = 33;
pub(crate) const MAX_UNIQUE: usize = (LAST_CODE - FIRST_CODE + 1) as usize; // 32

/// Generate a minimal keymap for this word's unique chars. `'\u{0008}'`
/// maps to the BackSpace keysym (used by the evdev fallback typer — U0008
/// is NOT the BackSpace keysym).
pub(crate) fn build_keymap(chars: &[(char, u32)]) -> String {
    let mut codes = String::new();
    let mut syms = String::new();
    for (ch, evdev) in chars {
        codes.push_str(&format!("<K{evdev}> = {};\n", evdev + 8));
        if *ch == '\u{0008}' {
            syms.push_str(&format!("key <K{evdev}> {{ [ BackSpace ] }};\n"));
        } else {
            syms.push_str(&format!("key <K{evdev}> {{ [ U{:04X} ] }};\n", *ch as u32));
        }
    }
    format!(
        "xkb_keymap {{\n\
         xkb_keycodes \"vi\" {{ minimum = 8; maximum = 255;\n{codes}}};\n\
         xkb_types \"vi\" {{ include \"complete\" }};\n\
         xkb_compatibility \"vi\" {{ include \"complete\" }};\n\
         xkb_symbols \"vi\" {{\n{syms}}};\n\
         }};\n"
    )
}

/// Write the keymap into a memfd (protocol wants fd + size, NUL-terminated).
pub(crate) fn memfd_keymap(text: &str) -> Option<(OwnedFd, u32)> {
    let name = std::ffi::CString::new("vi-viet-keymap").ok()?;
    let raw = unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC) };
    if raw < 0 {
        return None;
    }
    let mut f = unsafe { std::fs::File::from_raw_fd(raw) };
    f.write_all(text.as_bytes()).ok()?;
    f.write_all(&[0]).ok()?;
    Some((f.into(), text.len() as u32 + 1))
}

pub(crate) struct VietTyper {
    vk: Option<ZwpVirtualKeyboardV1>,
    start: Instant,
    /// Char→keycode cache from the last uploaded keymap. A new call to
    /// `backspace_then_type` reuses this mapping (skipping keymap build +
    /// memfd + upload) when all chars are already present — the common case
    /// for consecutive keystrokes that share overlapping character sets.
    /// Field bug 2026-07-10: rebuilding the keymap per keystroke caused
    /// 15-30ms `[SLOW-KEY]` spikes on tone-key presses in terminal live mode.
    cached_map: HashMap<char, u32>,
}

impl VietTyper {
    pub(crate) fn new(vk: Option<ZwpVirtualKeyboardV1>) -> Self {
        Self {
            vk,
            start: Instant::now(),
            cached_map: HashMap::new(),
        }
    }

    /// The live path is usable (the second virtual keyboard exists).
    pub(crate) fn ready(&self) -> bool {
        self.vk.is_some()
    }

    /// Type `s` by uploading a per-word keymap and tapping its keycodes.
    /// All-or-nothing: returns false (typing nothing) when impossible.
    pub(crate) fn type_str(&mut self, s: &str) -> bool {
        self.backspace_then_type(0, s, false)
    }

    /// BackSpace × n, then type `s`, ALL on this one keyboard. Keymap is
    /// cached across calls: consecutive keystrokes sharing the same char set
    /// skip the keymap-build + memfd + upload round (field bug 2026-07-10:
    /// 15-30ms SLOW-KEY spikes on tone-key presses in terminal live mode).
    ///
    /// `paced` = flush + 15ms after each BackSpace. Default burst-mode is
    /// fine for terminals (kitty/foot accept mixed BS+char bursts), but
    /// VCL/gtk3 (LibreOffice) swallows such a burst WHOLE — probe-verified
    /// 2026-07-10 (`scripts/vk-probe`) — so the caller passes `paced=true`
    /// when the focused app is in that family. Latency only ever hits the
    /// app that needs it.
    pub(crate) fn backspace_then_type(&mut self, backspaces: usize, s: &str, paced: bool) -> bool {
        let Some(vk) = &self.vk else { return false };
        if backspaces == 0 && s.is_empty() {
            return true;
        }

        // Everything this call needs mapped: BackSpace (if any) + s's chars.
        let mut needed: Vec<char> = Vec::new();
        if backspaces > 0 {
            needed.push('\u{0008}');
        }
        for ch in s.chars() {
            if !needed.contains(&ch) {
                needed.push(ch);
            }
        }
        if needed.len() > MAX_UNIQUE {
            warn!("[VIET-TYPER] >{MAX_UNIQUE} ký tự khác nhau trong một lần gõ — bỏ qua");
            return false;
        }

        let pace = |vk: &ZwpVirtualKeyboardV1| {
            if let Some(backend) = vk.backend().upgrade() {
                let _ = wayland_client::Connection::from_backend(backend).flush();
            }
            std::thread::sleep(std::time::Duration::from_millis(15));
        };

        let missing = needed
            .iter()
            .filter(|c| !self.cached_map.contains_key(c))
            .count();
        if missing > 0 {
            // Grow-only cache: EVICT everything when this call would spill
            // past the safe keycode window (2..=33) — otherwise the first
            // overflow would make every later word with a new char fail
            // forever (and un-checked growth would assign keycodes beyond
            // LAST_CODE, the known char-eating zone, R16 field lesson).
            if self.cached_map.len() + missing > MAX_UNIQUE {
                self.cached_map.clear();
            }
            for ch in &needed {
                if !self.cached_map.contains_key(ch) {
                    let code = FIRST_CODE + self.cached_map.len() as u32;
                    self.cached_map.insert(*ch, code);
                }
            }
            let assigned: Vec<(char, u32)> =
                self.cached_map.iter().map(|(c, k)| (*c, *k)).collect();
            let keymap = build_keymap(&assigned);
            let Some((fd, size)) = memfd_keymap(&keymap) else {
                warn!("[VIET-TYPER] memfd failed — không gõ được từ này");
                return false;
            };
            vk.keymap(KEYMAP_FORMAT_XKB_V1, fd.as_fd(), size);
            // Keymap-apply beat (repro 2026-07-10): Electron/Chromium áp
            // keymap TRỄ MỘT NHỊP — tap keycode mới trong cùng burst giải
            // mã theo keymap cũ → ký tự biến mất ("quà"→"q", "kẹ"→"k").
            // Flush + 15ms sau upload để client kịp áp trước tap đầu tiên.
            if paced {
                pace(vk);
            }
        }

        let mut t = self.start.elapsed().as_millis() as u32;
        for _ in 0..backspaces {
            let code = self.cached_map[&'\u{0008}'];
            vk.key(t, code, 1);
            vk.key(t.wrapping_add(1), code, 0);
            t = t.wrapping_add(2);
            if paced {
                pace(vk);
            }
        }
        for ch in s.chars() {
            let code = self.cached_map[&ch];
            vk.key(t, code, 1);
            vk.key(t.wrapping_add(1), code, 0);
            t = t.wrapping_add(2);
            // Paced mode spaces EVERY tap, not just BackSpace: field case
            // 2026-07-10 "cua73"→screen "cưử" — the op BS→pause→"ửa" burst
            // still lost the char AFTER the first ('a'); the probe mode
            // that passes on VCL paces all taps (`scripts/vk-probe` paced).
            if paced {
                pace(vk);
            }
        }
        true
    }
}

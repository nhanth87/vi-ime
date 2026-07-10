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
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1;

/// wl_keyboard keymap format: xkb_v1.
const KEYMAP_FORMAT_XKB_V1: u32 = 1;

/// Safe evdev codes for injected keys: the main typing rows ('1'..'=',
/// QWERTY row…). Nothing here doubles as a navigation/media key anywhere.
const FIRST_CODE: u32 = 2;
const LAST_CODE: u32 = 33;
const MAX_UNIQUE: usize = (LAST_CODE - FIRST_CODE + 1) as usize; // 32

/// Generate a minimal keymap for this word's unique chars.
fn build_keymap(chars: &[(char, u32)]) -> String {
    let mut codes = String::new();
    let mut syms = String::new();
    for (ch, evdev) in chars {
        codes.push_str(&format!("<K{evdev}> = {};\n", evdev + 8));
        syms.push_str(&format!("key <K{evdev}> {{ [ U{:04X} ] }};\n", *ch as u32));
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
fn memfd_keymap(text: &str) -> Option<(OwnedFd, u32)> {
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
}

impl VietTyper {
    pub(crate) fn new(vk: Option<ZwpVirtualKeyboardV1>) -> Self {
        Self {
            vk,
            start: Instant::now(),
        }
    }

    /// The live path is usable (the second virtual keyboard exists).
    pub(crate) fn ready(&self) -> bool {
        self.vk.is_some()
    }

    /// Type `s` by uploading a per-word keymap and tapping its keycodes.
    /// All-or-nothing: returns false (typing nothing) when impossible.
    pub(crate) fn type_str(&mut self, s: &str) -> bool {
        let Some(vk) = &self.vk else { return false };
        if s.is_empty() {
            return true;
        }

        // Assign safe keycodes to this word's unique chars.
        let mut map: HashMap<char, u32> = HashMap::new();
        let mut assigned: Vec<(char, u32)> = Vec::new();
        for ch in s.chars() {
            if map.contains_key(&ch) {
                continue;
            }
            let code = FIRST_CODE + assigned.len() as u32;
            if assigned.len() >= MAX_UNIQUE {
                warn!("[VIET-TYPER] >{MAX_UNIQUE} ký tự khác nhau trong một từ — bỏ qua");
                return false;
            }
            map.insert(ch, code);
            assigned.push((ch, code));
        }

        let keymap = build_keymap(&assigned);
        let Some((fd, size)) = memfd_keymap(&keymap) else {
            warn!("[VIET-TYPER] memfd failed — không gõ được từ này");
            return false;
        };
        // keymap and key events go out on the SAME object in order — the
        // app is guaranteed to see the new keymap before the first tap.
        vk.keymap(KEYMAP_FORMAT_XKB_V1, fd.as_fd(), size);

        let mut t = self.start.elapsed().as_millis() as u32;
        for ch in s.chars() {
            let code = map[&ch];
            vk.key(t, code, 1);
            vk.key(t.wrapping_add(1), code, 0);
            t = t.wrapping_add(2);
        }
        true
    }
}

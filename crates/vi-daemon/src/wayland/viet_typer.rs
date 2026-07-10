// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 vi-im contributors
//! VietTyper — a second virtual keyboard that types composed words.
//!
//! P0-3 hướng (b), "wtype-style". Both earlier NonPreedit designs died on
//! channel mixing: commit_string (text-input) does not stay ordered against
//! virtual-keyboard events, and delete_surrounding_text is silently ignored
//! by some apps. The only channel with guaranteed ordering AND universal
//! app support is wl_keyboard itself — so the word conversion happens here.
//!
//! **STATIC 8-level keymap (kiến trúc cuối, 2026-07-10 khuya):** hai đời
//! trước đều chết:
//! - keymap tĩnh trải keycode tới 170: các code đặc biệt ăn chữ
//!   (â→107=KEY_END, đ→162 — field 2026-07-09);
//! - keymap ĐỘNG per-word/cached trên dải an toàn 2..33: Blink/Electron áp
//!   `wl_keyboard.keymap` trễ VÔ HẠN ĐỊNH → tap keycode mới giải mã theo
//!   keymap cũ ("tu72"→"phò", 'ấ' trúng code 28 = Enter tự gửi message —
//!   field 2026-07-10, không pacing nào cứu nổi).
//! Giải cả hai ràng buộc cùng lúc: chỉ dùng 36 keycode an toàn (hàng phím
//! chữ/số, loại BS/Tab/Enter/Ctrl) nhưng mỗi key mang 8 LEVEL (type VIIM:
//! Shift/Mod3/Mod5) → 280 slot, đủ toàn bộ ASCII + chữ Việt hoa/thường.
//! Keymap upload MỘT LẦN khi tạo typer rồi KHÔNG BAO GIỜ đổi — Blink áp
//! trễ cũng chỉ trễ một lần trước từ đầu tiên. Level chọn bằng
//! `vk.modifiers(depressed)` ngay trước tap, trên CÙNG object nên ordering
//! được protocol bảo đảm (y hệt cách gõ chữ hoa bằng Shift).

use std::collections::HashMap;
use std::io::Write;
use std::os::fd::{AsFd, FromRawFd, OwnedFd};
use std::time::Instant;

use tracing::warn;
use wayland_client::Proxy;
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1;

/// wl_keyboard keymap format: xkb_v1.
pub(crate) const KEYMAP_FORMAT_XKB_V1: u32 = 1;

/// Safe evdev codes for injected keys: the main typing rows, EXCLUDING the
/// codes that mean something to apps regardless of keymap when the client
/// applies our keymap late (Blink/Electron lag, R17): 14=BACKSPACE,
/// 15=TAB, 28=ENTER, 29=LEFTCTRL. KHÔNG BAO GIỜ gán ký tự vào 4 code đó
/// dù keymap có remap ("gõ 'mất' là nó tự commit thành dấu enter",
/// field 2026-07-10 khuya).
pub(crate) const SAFE_CODES: [u32; 36] = [
    2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, // digit row '1'..'='
    16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, // qwerty row + [ ]
    30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, // home row a..' + `
];
pub(crate) const FIRST_CODE: u32 = SAFE_CODES[0];
/// Per-call unique-char cap for the evdev fallback typer (per-word keymap).
pub(crate) const MAX_UNIQUE: usize = 28;

/// Modifier masks per level (xkb real mods: Shift=0x1, Mod3=0x20, Mod5=0x80).
const LEVEL_MASKS: [u32; 8] = [0, 0x1, 0x80, 0x81, 0x20, 0x21, 0xA0, 0xA1];

/// Toàn bộ chữ Việt có dấu (thường). Hoa sinh bằng to_uppercase.
const VIET_LOWER: &str = "àáảãạằắẳẵặầấẩẫậèéẻẽẹềếểễệìíỉĩịòóỏõọồốổỗộờớởỡợùúủũụừứửữựỳýỷỹỵăâđêôơư";

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

/// Everything the static keymap covers: printable ASCII + Vietnamese
/// (both cases). Engine output can only ever contain these (raw keys are
/// ASCII; rendered syllables are Vietnamese).
fn char_inventory() -> Vec<char> {
    let mut v: Vec<char> = (0x20u8..0x7f).map(|b| b as char).collect(); // 95
    v.extend(VIET_LOWER.chars()); // 67
    for c in VIET_LOWER.chars() {
        v.extend(c.to_uppercase()); // 67
    }
    v
}

/// Build the ONE static keymap: key <K2> = BackSpace (ONE_LEVEL), the rest
/// of SAFE_CODES carry 8 chars each via type VIIM. Returns (keymap_text,
/// char → (evdev code, level 0-based)).
fn build_static_keymap() -> (String, HashMap<char, (u32, u8)>) {
    let inv = char_inventory();
    let mut map: HashMap<char, (u32, u8)> = HashMap::new();
    map.insert('\u{0008}', (SAFE_CODES[0], 0));

    let mut codes = String::new();
    let mut syms = String::new();
    for code in SAFE_CODES {
        codes.push_str(&format!("<K{code}> = {};\n", code + 8));
    }
    syms.push_str(&format!("key <K{}> {{ [ BackSpace ] }};\n", SAFE_CODES[0]));

    for (slot, chunk) in inv.chunks(8).enumerate() {
        // Slot 0 of SAFE_CODES is BackSpace — chars start at slot+1.
        let Some(&code) = SAFE_CODES.get(slot + 1) else {
            warn!("[VIET-TYPER] bảng ký tự tràn {} keycode — cắt bớt", SAFE_CODES.len());
            break;
        };
        let mut levels: Vec<String> = Vec::with_capacity(8);
        for (li, &ch) in chunk.iter().enumerate() {
            map.insert(ch, (code, li as u8));
            levels.push(format!("U{:04X}", ch as u32));
        }
        while levels.len() < 8 {
            levels.push("NoSymbol".into());
        }
        syms.push_str(&format!(
            "key <K{code}> {{ type[Group1]=\"VIIM\", symbols[Group1]=[ {} ] }};\n",
            levels.join(", ")
        ));
    }

    let text = format!(
        "xkb_keymap {{\n\
         xkb_keycodes \"vi\" {{ minimum = 8; maximum = 255;\n{codes}}};\n\
         xkb_types \"vi\" {{ include \"complete\"\n\
         type \"VIIM\" {{\n\
           modifiers = Shift+Mod3+Mod5;\n\
           map[Shift] = Level2;\n\
           map[Mod5] = Level3;\n\
           map[Shift+Mod5] = Level4;\n\
           map[Mod3] = Level5;\n\
           map[Shift+Mod3] = Level6;\n\
           map[Mod3+Mod5] = Level7;\n\
           map[Shift+Mod3+Mod5] = Level8;\n\
           level_name[Level1] = \"1\"; level_name[Level2] = \"2\";\n\
           level_name[Level3] = \"3\"; level_name[Level4] = \"4\";\n\
           level_name[Level5] = \"5\"; level_name[Level6] = \"6\";\n\
           level_name[Level7] = \"7\"; level_name[Level8] = \"8\";\n\
         }};\n\
         }};\n\
         xkb_compatibility \"vi\" {{ include \"complete\" }};\n\
         xkb_symbols \"vi\" {{\n{syms}}};\n\
         }};\n"
    );
    (text, map)
}

pub(crate) struct VietTyper {
    vk: Option<ZwpVirtualKeyboardV1>,
    start: Instant,
    /// Static char → (keycode, level). Built once with the keymap.
    map: HashMap<char, (u32, u8)>,
    /// Modifier mask currently depressed on this keyboard (level selector).
    cur_mask: u32,
}

impl VietTyper {
    /// Upload the ONE static keymap immediately: Blink applies keymaps with
    /// unbounded lag, so the upload must happen long before the first word,
    /// not per-word (R17 — mọi biến thể keymap-động đều fail thực địa).
    pub(crate) fn new(vk: Option<ZwpVirtualKeyboardV1>) -> Self {
        let mut map = HashMap::new();
        if let Some(vk) = &vk {
            let (text, m) = build_static_keymap();
            match memfd_keymap(&text) {
                Some((fd, size)) => {
                    vk.keymap(KEYMAP_FORMAT_XKB_V1, fd.as_fd(), size);
                    map = m;
                }
                None => warn!("[VIET-TYPER] memfd failed — live path tắt"),
            }
        }
        Self {
            vk,
            start: Instant::now(),
            map,
            cur_mask: 0,
        }
    }

    /// The live path is usable (vk exists AND the static keymap uploaded).
    pub(crate) fn ready(&self) -> bool {
        self.vk.is_some() && !self.map.is_empty()
    }

    /// Type `s` on the static keymap. All-or-nothing: false = nothing sent.
    pub(crate) fn type_str(&mut self, s: &str) -> bool {
        self.backspace_then_type(0, s, false)
    }

    /// BackSpace × n, then type `s`, ALL on this one keyboard — keymap is
    /// STATIC (uploaded once at creation), each char is (keycode, level) and
    /// the level rides `vk.modifiers()` on the same object right before the
    /// tap, so ordering is protocol-guaranteed end-to-end.
    ///
    /// `paced` = flush + 15ms after each tap. Burst-mode is fine for known
    /// terminals; VCL/gtk3 swallows BS+char bursts whole (probe-verified
    /// 2026-07-10) and Blink drops burst taps under load — non-terminal apps
    /// pass `paced=true`.
    pub(crate) fn backspace_then_type(&mut self, backspaces: usize, s: &str, paced: bool) -> bool {
        let Some(vk) = &self.vk else { return false };
        if self.map.is_empty() {
            return false;
        }
        if backspaces == 0 && s.is_empty() {
            return true;
        }
        // All-or-nothing: verify coverage BEFORE sending anything.
        if let Some(bad) = s.chars().find(|c| !self.map.contains_key(c)) {
            warn!("[VIET-TYPER] ký tự ngoài bảng tĩnh: {bad:?} — không gõ được");
            return false;
        }

        let pace = |vk: &ZwpVirtualKeyboardV1| {
            if let Some(backend) = vk.backend().upgrade() {
                let _ = wayland_client::Connection::from_backend(backend).flush();
            }
            std::thread::sleep(std::time::Duration::from_millis(15));
        };

        let mut t = self.start.elapsed().as_millis() as u32;
        let mut mask_now = self.cur_mask;
        let mut tap = |vk: &ZwpVirtualKeyboardV1, code: u32, level: u8, mask_now: &mut u32, t: &mut u32| {
            let want = LEVEL_MASKS[level as usize];
            if *mask_now != want {
                vk.modifiers(want, 0, 0, 0);
                *mask_now = want;
            }
            vk.key(*t, code, 1);
            vk.key(t.wrapping_add(1), code, 0);
            *t = t.wrapping_add(2);
        };

        for _ in 0..backspaces {
            let (code, level) = self.map[&'\u{0008}'];
            tap(vk, code, level, &mut mask_now, &mut t);
            if paced {
                pace(vk);
            }
        }
        for ch in s.chars() {
            let (code, level) = self.map[&ch];
            tap(vk, code, level, &mut mask_now, &mut t);
            if paced {
                pace(vk);
            }
        }
        // Never leave synthetic modifiers depressed on the seat.
        if mask_now != 0 {
            vk.modifiers(0, 0, 0, 0);
            mask_now = 0;
        }
        self.cur_mask = mask_now;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_keymap_covers_engine_output() {
        let (text, map) = build_static_keymap();
        // Mọi ký tự engine có thể sinh ra đều phải có slot.
        for ch in char_inventory() {
            assert!(map.contains_key(&ch), "thiếu {ch:?} trong bảng tĩnh");
        }
        assert!(map.contains_key(&'\u{0008}'));
        // Không slot nào rơi vào keycode nguy hiểm (BS/Tab/Enter/Ctrl).
        for (ch, (code, level)) in &map {
            assert!(
                SAFE_CODES.contains(code),
                "{ch:?} nằm ở code {code} ngoài SAFE_CODES"
            );
            assert!(!matches!(code, 14 | 15 | 28 | 29), "{ch:?} ở code nguy hiểm {code}");
            assert!(*level < 8);
        }
        // Keymap phải compile được bằng xkbcommon thật (nếu có xkbcli).
        if std::path::Path::new("/usr/bin/xkbcli").exists() {
            use std::io::Write as _;
            use std::process::{Command, Stdio};
            let mut child = Command::new("/usr/bin/xkbcli")
                .args(["compile-keymap", "--from-xkb"])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .expect("spawn xkbcli");
            child.stdin.as_mut().unwrap().write_all(text.as_bytes()).unwrap();
            let out = child.wait_with_output().unwrap();
            assert!(
                out.status.success(),
                "keymap tĩnh không compile: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }
}

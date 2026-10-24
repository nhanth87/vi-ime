// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! XTEST typer for the evdev fallback on X11/XWayland apps (Chrome X11,
//! OnlyOffice, any XWayland app on Hyprland).
//!
//! Replaces one `xdotool` process per character (R19). xdotool remaps a
//! scratch keycode for EVERY non-ASCII glyph, types it, then unmaps it: two
//! MappingNotify per glyph, and the app decodes a tap against whichever
//! mapping it has processed by then. That race is what forced the 200ms
//! settle per glyph (R19 rounds 2–5) — and 200ms per glyph is slower than
//! people type, so the queue backed up and the app still dropped glyphs
//! (field log 2026-10-23: `[EVDEV-QUEUE] backlog 10` in Google-chrome).
//!
//! Here the mapping is STATIC, like `viet_typer.rs`'s 8-level keymap (R14
//! rule: no per-key keymap changes): at start all 67 lowercase Vietnamese
//! letters are mapped two per keycode (level 1 / level 2 via Shift) onto
//! keys no keyboard sends while typing. Typing is then plain XTEST taps on
//! one ordered X connection — nothing is remapped while text is in flight,
//! so there is nothing to settle. Uppercase letters (rare: sentence starts,
//! names) and anything else (emoji) share a few extra keycodes, assigned
//! least-recently-used on first use; only that first use waits for clients
//! to pick up the new mapping.
//!
//! Xwayland can reload the seat keymap from the compositor (Hyprland does
//! when the active keyboard device changes), wiping our mapping. Before
//! each write the typer compares the server's map with its own (one request)
//! and maps everything again if needed.
//!
//! Measured 2026-10-24 (Hyprland, Chrome X11 textarea, 51 live-echo steps):
//! xdotool ≈ 10 s, this typer ≈ 0.4 s with no per-key delay.

use std::collections::HashMap;
use std::time::Duration;

use tracing::{info, warn};
use unicode_normalization::UnicodeNormalization;
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::xproto::{ConnectionExt as _, Keycode, Keysym, KEY_PRESS_EVENT, KEY_RELEASE_EVENT};
use x11rb::protocol::xtest::{self, ConnectionExt as _};
use x11rb::rust_connection::RustConnection;

const XK_BACKSPACE: Keysym = 0xff08;
const XK_SHIFT_L: Keysym = 0xffe1;

/// After (re)mapping keycodes, how long X clients get to process the
/// MappingNotify before the first tap that uses them. Paid once per remap
/// (start, keymap reload, first use of an uppercase letter), never per glyph.
const REMAP_SETTLE: Duration = Duration::from_millis(60);

/// Fewest LRU keycodes worth running with (uppercase / emoji).
const MIN_DYNAMIC: usize = 4;

/// Evdev codes (X keycode − 8) that are safe to carry a glyph. Chromium does
/// NOT decode many keys by keysym: it maps the evdev code to a fixed DOM
/// `code` and runs that key's command — Paste (evdev 135) inserted the
/// clipboard, Cut (137) too, New (181) opened a tab, Back/Refresh navigate,
/// media keys go to the media session — or drops the key (evdev 0, 160,
/// 218+). Measured 2026-10-24 by mapping [ê, Ê] onto every spare keycode and
/// typing `a <key> b` into a Chrome X11 textarea; the keys that gave "aêb"
/// minus every key Chromium binds a command to (a Paste with an empty
/// clipboard also gave "aêb"). GTK/Qt decode by keysym, so this set is safe
/// for them too. Keyed by evdev code, so it holds for any keyboard layout.
/// Level 2 (Shift) checked separately (field bug 2026-10-24: "á" typed as
/// "đ" in Chrome): [ê, ơ] + `a <key> Shift+<key> b` must give "aêơb".
/// Dropped: 85 Zenkaku/Hankaku (Shift+it is swallowed as an IME toggle —
/// it carried the pair đ/á) and 202 PROG3 (level 2 decoded as level 1).
/// Also 194 F24: a core ChangeKeyboardMapping keeps a key's EXPLICIT XKB
/// type, and xkeyboard-config gives FK24 `PC_CONTROL_SUPER_LEVEL2` (level 2
/// = Ctrl+Super, not Shift) — "ỳ" typed as "ý" (xkbcomp dump 2026-10-24).
/// Every other key here has no explicit type, so the server infers
/// TWO_LEVEL from our two keysyms and Shift selects level 2.
const SAFE_EVDEV: &[u8] = &[
    89, 95, 120, 124, 141, 144, 145, 146, 147, 148, 149, 151, 153, 154, 161,
    162, 169, 170, 171, 175, 176, 183, 184, 185, 186, 187, 188, 189, 190, 191, 192,
    203, 204, 211, 212, 213, 214, 215, 216,
];

/// Where a character lives on the X keyboard.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Slot {
    keycode: Keycode,
    shift: bool,
}

/// An LRU keycode for characters outside the static lowercase set.
struct Dynamic {
    keycode: Keycode,
    holds: Option<char>,
    last_used: u64,
}

pub(crate) struct XTestTyper {
    conn: RustConnection,
    root: u32,
    backspace: Keycode,
    shift: Keycode,
    /// Existing keycodes for printable ASCII (from the server's own map).
    ascii: HashMap<char, Slot>,
    /// Every non-ASCII character currently on the keyboard (static + LRU).
    glyphs: HashMap<char, Slot>,
    /// (keycode, [level 1, level 2]) as we mapped them, for verify / remap.
    mapped: Vec<(Keycode, [Keysym; 2])>,
    dynamic: Vec<Dynamic>,
    tick: u64,
    /// Borrowed keycodes' original keysyms (width `per`), put back on drop.
    original: HashMap<Keycode, Vec<Keysym>>,
    per: u8,
    /// Pause after each tap; 0 for most apps (per-app `typing_delay_ms`).
    tap_delay: Duration,
    /// Settle between the BackSpaces and the first new glyph — only for
    /// apps that drop the first glyph after a BackSpace burst (OnlyOffice
    /// CEF, R19): field 2026-10-24 rollover 35ms, "tướng" → "tớng" (the ư of
    /// the BS+"ư" edit vanished). Zero for Chrome (5/5 at 20ms/key).
    pre_glyph: Duration,
}

impl XTestTyper {
    /// Connect and map the Vietnamese glyph set. None when there is no X
    /// server, no XTEST, or too few usable keycodes — the caller falls back.
    pub(crate) fn new(dpy: Option<&str>, tap_delay_ms: u64, pre_glyph_ms: u64) -> Option<Self> {
        let (conn, screen) = match x11rb::connect(dpy) {
            Ok(c) => c,
            Err(e) => {
                warn!("[XTEST] không kết nối được X server ({dpy:?}): {e}");
                return None;
            }
        };
        if conn.extension_information(xtest::X11_EXTENSION_NAME).ok().flatten().is_none() {
            warn!("[XTEST] X server không có extension XTEST");
            return None;
        }
        let root = conn.setup().roots.get(screen)?.root;
        let (min, max) = (conn.setup().min_keycode, conn.setup().max_keycode);
        let reply = conn.get_keyboard_mapping(min, max - min + 1).ok()?.reply().ok()?;
        let per = reply.keysyms_per_keycode as usize;
        if per == 0 {
            return None;
        }

        let set = vietnamese_lowercase();
        let own: std::collections::HashSet<Keysym> =
            set.iter().flat_map(|&c| [keysym_for(c), keysym_for(upper(c))]).collect();
        let mut ascii = HashMap::new();
        let mut usable = Vec::new();
        let mut original = HashMap::new();
        let (mut backspace, mut shift) = (None, None);
        for (i, syms) in reply.keysyms.chunks(per).enumerate() {
            let kc = min + i as u8;
            if kc >= 8 && SAFE_EVDEV.contains(&(kc - 8)) {
                // Empty, multimedia/F13+ only, or still holding one of our
                // glyphs because an earlier typer could not give it back.
                if syms.iter().all(|&s| s == 0) || own.contains(&syms[0]) {
                    usable.push(kc);
                    continue;
                }
                if syms.iter().all(|&s| s == 0 || is_spare_keysym(s)) {
                    usable.push(kc);
                    original.insert(kc, syms.to_vec());
                    continue;
                }
            }
            for (level, &s) in syms.iter().take(2).enumerate() {
                if s == XK_BACKSPACE && backspace.is_none() {
                    backspace = Some(kc);
                }
                if s == XK_SHIFT_L && shift.is_none() {
                    shift = Some(kc);
                }
                if (0x20..0x7f).contains(&s)
                    && let Some(c) = char::from_u32(s)
                {
                    ascii.entry(c).or_insert(Slot { keycode: kc, shift: level == 1 });
                }
            }
        }
        let (Some(backspace), Some(shift)) = (backspace, shift) else {
            warn!("[XTEST] keymap X thiếu BackSpace/Shift_L");
            return None;
        };

        let pairs: Vec<&[char]> = set.chunks(2).collect();
        if usable.len() < pairs.len() + MIN_DYNAMIC {
            warn!(
                "[XTEST] chỉ có {} keycode dùng được, cần {} — dùng typer khác",
                usable.len(),
                pairs.len() + MIN_DYNAMIC
            );
            return None;
        }
        let mut glyphs = HashMap::new();
        let mut mapped = Vec::new();
        for (pair, &kc) in pairs.iter().zip(usable.iter()) {
            let second = pair.get(1).copied().unwrap_or(pair[0]);
            glyphs.insert(pair[0], Slot { keycode: kc, shift: false });
            glyphs.insert(second, Slot { keycode: kc, shift: true });
            mapped.push((kc, [keysym_for(pair[0]), keysym_for(second)]));
        }
        let dynamic = usable[pairs.len()..]
            .iter()
            .map(|&keycode| Dynamic { keycode, holds: None, last_used: 0 })
            .collect();

        let mut t = Self {
            conn,
            root,
            backspace,
            shift,
            ascii,
            glyphs,
            mapped,
            dynamic,
            tick: 0,
            original,
            per: per as u8,
            tap_delay: Duration::from_millis(tap_delay_ms),
            pre_glyph: Duration::from_millis(pre_glyph_ms),
        };
        if !t.map_all() || !t.mapping_intact() {
            warn!("[XTEST] X server không nhận bảng phím");
            return None;
        }
        info!(
            "[XTEST] typer sẵn sàng: {} chữ thường gán cố định trên {} phím, {} phím LRU, nghỉ {}ms/phím",
            set.len(),
            t.mapped.len(),
            t.dynamic.len(),
            tap_delay_ms
        );
        Some(t)
    }

    /// Write every mapping we own, then let clients catch up once.
    fn map_all(&mut self) -> bool {
        for &(kc, syms) in &self.mapped {
            if self.conn.change_keyboard_mapping(1, kc, 2, &syms).is_err() {
                return false;
            }
        }
        let ok = self.sync();
        std::thread::sleep(REMAP_SETTLE);
        ok
    }

    /// Does the server's map still hold every mapping we made? One
    /// GetKeyboardMapping for the whole range — a few KB over a local
    /// socket, well under a millisecond.
    fn mapping_intact(&self) -> bool {
        let setup = self.conn.setup();
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let Some(r) = self
            .conn
            .get_keyboard_mapping(min, max - min + 1)
            .ok()
            .and_then(|c| c.reply().ok())
        else {
            return false;
        };
        let per = r.keysyms_per_keycode as usize;
        self.mapped.iter().all(|&(kc, syms)| {
            let at = (kc - min) as usize * per;
            r.keysyms.get(at) == Some(&syms[0]) && r.keysyms.get(at + 1) == Some(&syms[1])
        })
    }

    /// Round trip: when it returns, the server has executed every request
    /// sent so far (taps included), in order.
    fn sync(&self) -> bool {
        self.conn.get_input_focus().ok().and_then(|c| c.reply().ok()).is_some()
    }

    fn tap(&self, slot: Slot) -> bool {
        let fake = |kind, kc| self.conn.xtest_fake_input(kind, kc, 0, self.root, 0, 0, 0).is_ok();
        let mut ok = true;
        if slot.shift {
            ok &= fake(KEY_PRESS_EVENT, self.shift);
        }
        ok &= fake(KEY_PRESS_EVENT, slot.keycode);
        ok &= fake(KEY_RELEASE_EVENT, slot.keycode);
        if slot.shift {
            ok &= fake(KEY_RELEASE_EVENT, self.shift);
        }
        if !self.tap_delay.is_zero() {
            ok &= self.conn.flush().is_ok();
            std::thread::sleep(self.tap_delay);
        }
        ok
    }

    /// Slot for `c`: ASCII from the server's map, lowercase Vietnamese from
    /// the static map, anything else on the least-recently-used extra key.
    fn slot(&mut self, c: char) -> Option<Slot> {
        self.tick += 1;
        if let Some(&s) = self.ascii.get(&c) {
            return Some(s);
        }
        if let Some(&s) = self.glyphs.get(&c) {
            if let Some(d) = self.dynamic.iter_mut().find(|d| d.holds == Some(c)) {
                d.last_used = self.tick;
            }
            return Some(s);
        }
        let d = self.dynamic.iter_mut().min_by_key(|d| d.last_used)?;
        if let Some(old) = d.holds.take() {
            self.glyphs.remove(&old);
        }
        // Same glyph on both levels: a Shift the user is still holding
        // cannot turn it into something else.
        let syms = [keysym_for(c), keysym_for(c)];
        let kc = d.keycode;
        d.holds = Some(c);
        d.last_used = self.tick;
        self.conn.change_keyboard_mapping(1, kc, 2, &syms).ok()?;
        match self.mapped.iter_mut().find(|(k, _)| *k == kc) {
            Some(m) => m.1 = syms,
            None => self.mapped.push((kc, syms)),
        }
        self.sync();
        std::thread::sleep(REMAP_SETTLE);
        let slot = Slot { keycode: kc, shift: false };
        self.glyphs.insert(c, slot);
        Some(slot)
    }

    /// BackSpace × n, then type `text`. False = something did not reach the
    /// X server (the word on screen may now differ from what we track).
    pub(crate) fn backspace_then_type(&mut self, backspaces: usize, text: &str) -> bool {
        if backspaces == 0 && text.is_empty() {
            return true;
        }
        // Drop queued events (MappingNotify etc.) so they never pile up.
        while let Ok(Some(_)) = self.conn.poll_for_event() {}
        if !text.is_ascii() && !self.mapping_intact() {
            info!("[XTEST] Xwayland đã nạp lại keymap — gán lại chữ có dấu");
            if !self.map_all() {
                return false;
            }
        }
        let mut ok = true;
        for _ in 0..backspaces {
            ok &= self.tap(Slot { keycode: self.backspace, shift: false });
        }
        if backspaces > 0 && !text.is_empty() && !self.pre_glyph.is_zero() {
            ok &= self.sync();
            std::thread::sleep(self.pre_glyph);
        }
        for c in text.chars() {
            match self.slot(c) {
                Some(s) => ok &= self.tap(s),
                None => {
                    warn!("[XTEST] không có keycode cho {c:?} — bỏ qua");
                    ok = false;
                }
            }
        }
        ok && self.sync()
    }
}

impl Drop for XTestTyper {
    /// Give the borrowed keycodes back (only where our mapping is still in
    /// place — after a keymap reload they already belong to someone else).
    fn drop(&mut self) {
        if !self.mapping_intact() {
            info!("[XTEST] keymap đã được nạp lại — không cần trả keycode");
            return;
        }
        for &(kc, _) in &self.mapped {
            let _ = match self.original.get(&kc) {
                Some(syms) => self.conn.change_keyboard_mapping(1, kc, self.per, syms),
                None => self.conn.change_keyboard_mapping(1, kc, 1, &[0]),
            };
        }
        // Round trip, not just flush: closing the socket right after a flush
        // lost most of these requests in testing.
        self.sync();
    }
}

/// X keysym for a character: Latin-1 keysyms equal the code point,
/// everything else uses the Unicode keysym range.
fn keysym_for(c: char) -> Keysym {
    let cp = c as u32;
    if (0x20..0x7f).contains(&cp) || (0xa0..0x100).contains(&cp) { cp } else { 0x0100_0000 | cp }
}

/// Keysyms an X app only sees from dedicated multimedia keys: the XF86
/// range and F13–F35 — never produced while typing text.
fn is_spare_keysym(s: Keysym) -> bool {
    (0x1008_ff00..=0x1008_ffff).contains(&s) || (0xffca..=0xffe0).contains(&s)
}

fn upper(c: char) -> char {
    c.to_uppercase().next().unwrap_or(c)
}

/// Every lowercase Vietnamese letter that is not plain ASCII: the 12 vowel
/// bases × 6 tones (composed to NFC), minus the bare ASCII vowels, plus đ.
fn vietnamese_lowercase() -> Vec<char> {
    const BASES: [char; 12] = ['a', 'ă', 'â', 'e', 'ê', 'i', 'o', 'ô', 'ơ', 'u', 'ư', 'y'];
    const MARKS: [Option<char>; 6] =
        [None, Some('\u{301}'), Some('\u{300}'), Some('\u{309}'), Some('\u{303}'), Some('\u{323}')];
    let mut out = vec!['đ'];
    for b in BASES {
        for m in MARKS {
            let s: String = match m {
                Some(m) => [b, m].iter().collect::<String>().nfc().collect(),
                None => b.to_string(),
            };
            if let Some(c) = s.chars().next()
                && s.chars().count() == 1
                && !c.is_ascii()
            {
                out.push(c);
            }
        }
    }
    out
}

// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
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
//!
//! **Kết nối Wayland RIÊNG (2026-07-13):** trước đây `VietTyper` dùng
//! CHUNG connection/`EventQueue` với vòng lặp IME chính (`wayland/mod.rs`),
//! nên `pace()` chỉ gọi được `flush()` (đẩy byte ra socket) — không có
//! cách xác nhận thật compositor đã xử lý xong tap trước khi tap kế tiếp
//! tới, vì gọi `roundtrip()` trên event queue CHÍNH giữa lúc đang xử lý
//! callback của CHÍNH event queue đó (re-entrant dispatch) là rủi ro. Field
//! bug 2026-07-13: LibreOffice "chữ"→"chu" — 2 lần sửa dấu liên tiếp trên
//! cùng từ bị VCL/gtk3 nuốt trọn vì thiếu xác nhận (đúng lớp lỗi "BS+char
//! burst bị nuốt whole" mà `evdev_typer.rs` đã vá bằng `roundtrip()` cho
//! đường evdev fallback từ 2026-07-10, nhưng chưa lan sang đây).
//!
//! Fix: `VietTyper` giờ tự mở connection + `EventQueue` RIÊNG (giống hệt
//! `EvdevTyper`, pattern đã field-proven) — độc lập hoàn toàn với vòng lặp
//! chính, nên `roundtrip()` ở đây không đụng gì tới event queue chính,
//! không còn rủi ro re-entrant. BackSpace roundtrip thật (không chỉ flush)
//! + 15ms, ký tự flush + 15ms — giống chính xác scheme `evdev_typer.rs` đã
//! field-confirm hoạt động.

use std::collections::HashMap;
use std::io::Write;
use std::os::fd::{AsFd, FromRawFd, OwnedFd};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use tracing::warn;
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use crate::client_profile::ClientProfile;
use wayland_client::protocol::wl_registry;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1,
    zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
};

/// Event sink for the typer's private connection — nothing to handle
/// (identical to `evdev_typer.rs`'s `TyperState`; kept separate here so
/// this file has no dependency on evdev_typer.rs and vice versa).
struct TyperSink;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for TyperSink {
    fn event(
        _state: &mut Self,
        _proxy: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlSeat, ()> for TyperSink {
    fn event(
        _state: &mut Self,
        _proxy: &WlSeat,
        _event: <WlSeat as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpVirtualKeyboardManagerV1, ()> for TyperSink {
    fn event(
        _state: &mut Self,
        _proxy: &ZwpVirtualKeyboardManagerV1,
        _event: <ZwpVirtualKeyboardManagerV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpVirtualKeyboardV1, ()> for TyperSink {
    fn event(
        _state: &mut Self,
        _proxy: &ZwpVirtualKeyboardV1,
        _event: <ZwpVirtualKeyboardV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
    }
}

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
/// pub(crate): the evdev fallback typer reuses the SAME static keymap +
/// level-selection scheme (evdev_typer.rs) so LibreOffice/VCL gets the same
/// lag-proof "upload once, pick level per tap" path as the Wayland live path.
pub(crate) const LEVEL_MASKS: [u32; 8] = [0, 0x1, 0x80, 0x81, 0x20, 0x21, 0xA0, 0xA1];


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
///
/// SINH bằng chính Unicode algebra của engine (glyph.rs) — không có chuỗi
/// literal liệt kê bảng chữ (tinh thần R14): nguyên âm quality qua
/// `apply_quality`, tone qua `compose(tone_mark)`. Engine ngôn ngữ mới
/// (multilang vision) mở rộng algebra là inventory tự theo.
fn char_inventory() -> Vec<char> {
    use crate::engine::glyph;
    use crate::engine::tone::Tone;

    let mut v: Vec<char> = (0x20u8..0x7f).map(|b| b as char).collect(); // 95

    // Nguyên âm nền: ASCII + biến thể quality sinh bằng algebra
    // (a→â/ă, e→ê, o→ô/ơ, u→ư; các cặp vô nghĩa tự trả None).
    let mut vowels: Vec<char> = vec!['a', 'e', 'i', 'o', 'u', 'y'];
    for base in ['a', 'e', 'o', 'u'] {
        for mark in [glyph::CIRCUMFLEX, glyph::BREVE, glyph::HORN] {
            if let Some(c) = glyph::apply_quality(base, mark) {
                vowels.push(c);
            }
        }
    }

    let mut viet: Vec<char> = Vec::with_capacity(67);
    if let Some(dd) = glyph::apply_quality('d', glyph::STROKE) {
        viet.push(dd); // đ — ngoại lệ duy nhất của algebra (R14)
    }
    for &vw in &vowels {
        if !vw.is_ascii() {
            viet.push(vw); // â ă ê ô ơ ư (quality, chưa tone)
        }
        for tone in [Tone::Acute, Tone::Grave, Tone::Hook, Tone::Tilde, Tone::Dot] {
            if let Some(c) = glyph::tone_mark(tone).and_then(|m| glyph::compose(vw, m)) {
                viet.push(c); // 12 nguyên âm × 5 tone = 60
            }
        }
    }
    let upper: Vec<char> = viet.iter().flat_map(|c| c.to_uppercase()).collect();
    v.extend(viet);
    v.extend(upper);
    v
}

/// Build the ONE static keymap: key <K2> = BackSpace (ONE_LEVEL), the rest
/// of SAFE_CODES carry 8 chars each via type VIIM. Returns (keymap_text,
/// char → (evdev code, level 0-based)).
pub(crate) fn build_static_keymap() -> (String, HashMap<char, (u32, u8)>) {
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

/// A single type command from the main event loop to the typer thread.
#[derive(Debug)]
enum TypeCmd {
    Type { backspaces: usize, suffix: String, paced: bool },
    Shutdown,
}

pub(crate) struct VietTyper {
    /// Non-blocking channel to the typer thread. `None` when unavailable.
    tx: Option<std::sync::mpsc::Sender<TypeCmd>>,
    start: Instant,
    /// Static char → (keycode, level). Built once with the keymap.
    /// Shared with the typer thread (read-only after construction).
    map: HashMap<char, (u32, u8)>,
    /// In-flight type commands (incremented on send, decremented by thread
    /// on completion). Used by the live_echo_pending guard to suppress
    /// TextChangeCause::Other while the typer thread is still typing.
    inflight: Arc<AtomicU64>,
    /// Adaptive timing profile (backspace, glyph, pre-first-glyph delays).
    profile: ClientProfile,
}

impl VietTyper {
    /// Opens a Wayland connection in a DEDICATED THREAD (P0b: zero sleep in
    /// event loop). The thread uploads the static keymap, then sits in a
    /// `recv()` loop executing type commands with proper pacing (roundtrip
    /// + sleep per glyph). The main loop sends commands non-blocking.
    pub(crate) fn new(profile: ClientProfile) -> Self {
        let (text, map) = build_static_keymap();
        let Some((mut queue, vk)) = Self::connect(&text) else {
            warn!("[VIET-TYPER] không mở được Wayland connection riêng — live path tắt");
            return Self { tx: None, start: Instant::now(), map: HashMap::new(), inflight: Arc::new(AtomicU64::new(0)), profile };
        };
        let map_shared = map.clone();
        let (tx, rx) = std::sync::mpsc::channel::<TypeCmd>();
        let inflight = Arc::new(AtomicU64::new(0));
        let inflight_clone = inflight.clone();
        let thread_profile = profile.clone();

        std::thread::spawn(move || {
            let start = Instant::now();
            let mut mask_now: u32 = 0;

            let tap = |vk: &ZwpVirtualKeyboardV1, code: u32, level: u8,
                        mask_now: &mut u32, t: &mut u32| {
                let want = LEVEL_MASKS[level as usize];
                if *mask_now != want {
                    vk.modifiers(want, 0, 0, 0);
                    *mask_now = want;
                }
                vk.key(*t, code, 1);
                vk.key(t.wrapping_add(1), code, 0);
                *t = t.wrapping_add(2);
            };

            for cmd in rx {
                match cmd {
                    TypeCmd::Type { backspaces, suffix, paced } => {
                        let mut t = start.elapsed().as_millis() as u32;
                        for _ in 0..backspaces {
                            let (code, level) = map_shared[&'\u{0008}'];
                            tap(&vk, code, level, &mut mask_now, &mut t);
                            if paced {
                                let _ = queue.roundtrip(&mut TyperSink);
                                std::thread::sleep(std::time::Duration::from_millis(thread_profile.backspace_delay_ms));
                            }
                        }
                        if paced && backspaces > 0 {
                            std::thread::sleep(std::time::Duration::from_millis(thread_profile.pre_first_glyph_delay_ms));
                        }
                        for ch in suffix.chars() {
                            let (code, level) = map_shared[&ch];
                            tap(&vk, code, level, &mut mask_now, &mut t);
                            if paced {
                                let _ = queue.roundtrip(&mut TyperSink);
                                std::thread::sleep(std::time::Duration::from_millis(thread_profile.glyph_delay_ms));
                            }
                        }
                        if mask_now != 0 {
                            vk.modifiers(0, 0, 0, 0);
                            mask_now = 0;
                        }
                        let _ = queue.flush();
                        inflight_clone.fetch_sub(1, Ordering::Release);
                    }
                    TypeCmd::Shutdown => break,
                }
            }
        });

        Self { tx: Some(tx), start: Instant::now(), map, inflight, profile }
    }

    fn connect(keymap_text: &str) -> Option<(EventQueue<TyperSink>, ZwpVirtualKeyboardV1)> {
        let conn = Connection::connect_to_env().ok()?;
        let (globals, mut queue) = registry_queue_init::<TyperSink>(&conn).ok()?;
        let qh = queue.handle();
        let seat: WlSeat = globals.bind(&qh, 1..=9, ()).ok()?;
        let mgr: ZwpVirtualKeyboardManagerV1 = globals.bind(&qh, 1..=1, ()).ok()?;
        let vk = mgr.create_virtual_keyboard(&seat, &qh, ());
        let (fd, size) = memfd_keymap(keymap_text)?;
        vk.keymap(KEYMAP_FORMAT_XKB_V1, fd.as_fd(), size);
        queue.roundtrip(&mut TyperSink).ok()?;
        Some((queue, vk))
    }

    pub(crate) fn ready(&self) -> bool {
        self.tx.is_some() && !self.map.is_empty()
    }

    pub(crate) fn type_str(&mut self, s: &str) -> bool {
        self.backspace_then_type(0, s, false)
    }

    /// P0b: NON-BLOCKING. Checks coverage, sends command to typer thread,
    /// returns immediately. The thread does all roundtrip/sleep pacing.
    pub(crate) fn backspace_then_type(&mut self, backspaces: usize, s: &str, paced: bool) -> bool {
        let Some(tx) = &self.tx else { return false };
        if self.map.is_empty() { return false; }
        if backspaces == 0 && s.is_empty() { return true; }
        if let Some(bad) = s.chars().find(|c| !self.map.contains_key(c)) {
            warn!("[VIET-TYPER] ký tự ngoài bảng tĩnh: {bad:?} — không gõ được");
            return false;
        }
        self.inflight.fetch_add(1, Ordering::Release);
        if tx.send(TypeCmd::Type { backspaces, suffix: s.to_string(), paced }).is_err() {
            self.inflight.fetch_sub(1, Ordering::Release);
            warn!("[VIET-TYPER] thread đã chết — không gửi được lệnh");
            return false;
        }
        true
    }

    pub(crate) fn inflight(&self) -> u64 {
        self.inflight.load(Ordering::Acquire)
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_keymap_covers_engine_output() {
        let (text, map) = build_static_keymap();
        // Inventory sinh bằng algebra phải phủ đúng bảng chữ Việt đầy đủ
        // (chuỗi tham chiếu literal CHỈ nằm trong test — R14 cấm table ở
        // engine path, không cấm oracle trong test).
        const REF: &str = "àáảãạằắẳẵặầấẩẫậèéẻẽẹềếểễệìíỉĩịòóỏõọồốổỗộ\
                           ờớởỡợùúủũụừứửữựỳýỷỹỵăâđêôơư";
        let inv = char_inventory();
        for ch in REF.chars().filter(|c| !c.is_whitespace()) {
            assert!(inv.contains(&ch), "algebra thiếu {ch:?}");
            for up in ch.to_uppercase() {
                assert!(inv.contains(&up), "algebra thiếu {up:?}");
            }
        }
        // Mọi ký tự engine có thể sinh ra đều phải có slot.
        for ch in inv {
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

    #[test]
    fn static_keymap_no_level_collision() {
        // Moi ky tu engine co the sinh PHAI co (code, level) RIENG BIET.
        // Neu 2 ky tu khac nhau trung (code, level), mot cai se render thanh
        // cai kia - CHINH la lop loi "nguoi"->"nguoi" (u mat dau sung vi trung
        // slot voi 'u' base). Test nay la oracle bat regression gop level.
        let (_text, map) = build_static_keymap();
        let mut seen: std::collections::HashMap<(u32, u8), char> =
            std::collections::HashMap::new();
        for (&ch, &slot) in &map {
            if ch == '\u{0008}' {
                continue; // BackSpace la slot rieng, khong phai glyph
            }
            if let Some(prev) = seen.insert(slot, ch) {
                panic!("2 chars share slot {:?}: {:?} and {:?}", slot, prev, ch);
            }
        }
        let u = map[&'\u{01b0}']; let o = map[&'\u{01a1}'];
        let a = map[&'\u{00e2}']; let f = map[&'\u{00f4}'];
        let e = map[&'\u{00ea}']; let c = map[&'\u{0103}'];
        assert_ne!(u, map[&'u'], "u-horn must not collide with u");
        assert_ne!(o, map[&'o'], "o-horn must not collide with o");
        assert_ne!(a, map[&'a'], "a-circ must not collide with a");
        assert_ne!(f, map[&'o'], "o-circ must not collide with o");
        assert_ne!(e, map[&'e'], "e-circ must not collide with e");
        assert_ne!(c, map[&'a'], "a-breve must not collide with a");
    }
}

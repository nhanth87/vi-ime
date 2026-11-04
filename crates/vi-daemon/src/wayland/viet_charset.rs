// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! The typer's character table and its ONE static keymap (split from
//! viet_typer.rs, R4). Characters sit in (unshifted, Shift) pairs like a
//! real keyboard — see `char_pairs`.

use std::collections::HashMap;

use tracing::warn;

use super::viet_typer::SAFE_CODES;

/// Everything the static keymap covers: printable ASCII + Vietnamese
/// (both cases). Engine output can only ever contain these (raw keys are
/// ASCII; rendered syllables are Vietnamese).
///
/// SINH bằng chính Unicode algebra của engine (glyph.rs) — không có chuỗi
/// literal liệt kê bảng chữ (tinh thần R14): nguyên âm quality qua
/// `apply_quality`, tone qua `compose(tone_mark)`. Engine ngôn ngữ mới
/// (multilang vision) mở rộng algebra là inventory tự theo.
///
/// Returned as (unshifted, Shift) PAIRS, laid out like a real keyboard:
/// letters with their capital, digits/punctuation as on a US keyboard
/// ('1'/'!', '-'/'_'). The user's own Shift is still held while the typer
/// writes a capital, and Chrome applies it to our key too: with plain ASCII
/// order "H" sat on level 0 and "I" on the Shift level, so every capital
/// typed with Shift held came out as the NEXT character — field 2026-11-04
/// "gõ chữ H (hoa) nó ra chữ I". Paired, a stray Shift gives the same char.
fn char_pairs() -> Vec<(char, char)> {
    use crate::engine::glyph;
    use crate::engine::tone::Tone;

    // US keyboard: unshifted + shifted on the same key (space keeps space).
    let mut v: Vec<(char, char)> = vec![(' ', ' ')];
    for pair in ["`~", "1!", "2@", "3#", "4$", "5%", "6^", "7&", "8*", "9(", "0)", "-_", "=+",
                 "[{", "]}", "\\|", ";:", "'\"", ",<", ".>", "/?"] {
        let mut c = pair.chars();
        if let (Some(a), Some(b)) = (c.next(), c.next()) {
            v.push((a, b));
        }
    }
    v.extend(('a'..='z').map(|c| (c, c.to_ascii_uppercase()))); // 95 ASCII in all

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
    v.extend(viet.iter().map(|&c| (c, c.to_uppercase().next().unwrap_or(c))));
    v
}

/// Every char the static keymap covers, flat (see `char_pairs`).
#[cfg(test)]
pub(crate) fn char_inventory() -> Vec<char> {
    let mut v: Vec<char> = Vec::new();
    for (lo, up) in char_pairs() {
        v.push(lo);
        if up != lo {
            v.push(up);
        }
    }
    v
}

/// Build the ONE static keymap: key <K2> = BackSpace (ONE_LEVEL), the rest
/// of SAFE_CODES carry 8 chars each via type VIIM. Returns (keymap_text,
/// char → (evdev code, level 0-based)).
pub(crate) fn build_static_keymap() -> (String, HashMap<char, (u32, u8)>) {
    let inv = char_pairs();
    let mut map: HashMap<char, (u32, u8)> = HashMap::new();
    map.insert('\u{0008}', (SAFE_CODES[0], 0));

    let mut codes = String::new();
    let mut syms = String::new();
    for code in SAFE_CODES {
        codes.push_str(&format!("<K{code}> = {};\n", code + 8));
    }
    syms.push_str(&format!("key <K{}> {{ [ BackSpace ] }};\n", SAFE_CODES[0]));

    // 4 pairs per key: levels (0,1) (2,3) (4,5) (6,7) = (x, Shift+x).
    for (slot, chunk) in inv.chunks(4).enumerate() {
        // Slot 0 of SAFE_CODES is BackSpace — chars start at slot+1.
        let Some(&code) = SAFE_CODES.get(slot + 1) else {
            warn!("[VIET-TYPER] bảng ký tự tràn {} keycode — cắt bớt", SAFE_CODES.len());
            break;
        };
        let mut levels: Vec<String> = Vec::with_capacity(8);
        for (pi, &(lo, up)) in chunk.iter().enumerate() {
            let li = 2 * pi as u8;
            map.entry(lo).or_insert((code, li));
            map.entry(up).or_insert((code, li + 1)); // space: level 0 only
            levels.push(format!("U{:04X}", lo as u32));
            levels.push(format!("U{:04X}", up as u32));
        }
        while levels.len() < 8 {
            levels.push("NoSymbol".into());
        }
        syms.push_str(&format!(
            "key <K{code}> {{ type[Group1]=\"VIIM\", symbols[Group1]=[ {} ] }};\n",
            levels.join(", ")
        ));
    }
    // Forward Delete ('\u{7f}') on the next free key, ONE_LEVEL like
    // BackSpace: clears the browser's selected autocomplete before a
    // live-echo BackSpace in the address bar (actions.rs `sync_shown`).
    // On the typer's own keyboard so it can never arrive after them.
    if let Some(&code) = SAFE_CODES.get(inv.chunks(4).count() + 1) {
        map.insert('\u{7f}', (code, 0));
        syms.push_str(&format!("key <K{code}> {{ [ Delete ] }};\n"));
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

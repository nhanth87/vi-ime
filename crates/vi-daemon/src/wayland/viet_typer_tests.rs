//! VietTyper regression tests.

use super::*;

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

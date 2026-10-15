// SPDX-License-Identifier: GPL-3.0-only
//! R-macro unit tests — extracted to keep macro_dict.rs under R4.

use super::*;

#[cfg(test)]
mod tests {
    use super::*;

    fn tables() -> (HashMap<String, String>, HashMap<String, String>) {
        let mut kw = HashMap::new();
        kw.insert("vn".to_string(), "Việt Nam".to_string());
        kw.insert("dc".to_string(), "được".to_string());
        let mut hk = HashMap::new();
        hk.insert("ctrl+alt+m".to_string(), "macrom text".to_string());
        hk.insert("super+f12".to_string(), "F macro".to_string());
        (kw, hk)
    }

    #[test]
    fn keyword_exact_and_lowercase() {
        let (kw, hk) = tables();
        let d = MacroDict::build(&kw, &hk);
        assert_eq!(d.expand_keyword("vn").as_deref(), Some("Việt Nam"));
        assert_eq!(d.expand_keyword("VN").as_deref(), Some("Việt Nam"));
        assert_eq!(d.expand_keyword("Vn").as_deref(), Some("Việt Nam"));
        assert_eq!(d.expand_keyword("xyz"), None);
        assert_eq!(d.expand_keyword(""), None);
    }

    #[test]
    fn hotkey_parse_and_match() {
        let (kw, hk) = tables();
        let d = MacroDict::build(&kw, &hk);
        let m = HotkeyKey::Char('m');
        assert_eq!(
            d.match_hotkey(true, true, false, false, &m).as_deref(),
            Some("macrom text")
        );
        // Wrong modifiers → no match.
        assert_eq!(d.match_hotkey(true, false, false, false, &m), None);
        assert_eq!(
            d.match_hotkey(true, true, false, true, &m),
            None,
            "shift mismatch must not match"
        );
        assert_eq!(
            d.match_hotkey(false, false, true, false, &HotkeyKey::F(12))
                .as_deref(),
            Some("F macro")
        );
    }

    #[test]
    fn hotkey_parse_rejects_unsafe() {
        assert!(parse_hotkey("").is_none());
        assert!(parse_hotkey("m").is_none(), "bare key must not grab typing");
        assert!(parse_hotkey("shift+m").is_none(), "shift-only must not grab");
        assert!(parse_hotkey("ctrl+alt").is_none(), "modifiers alone hit nothing");
        assert!(parse_hotkey("ctrl+m+m").is_none());
        assert!(parse_hotkey("ctrl+bogus").is_none());
        assert!(parse_hotkey("ctrl++m").is_none());
        let h = parse_hotkey("Ctrl+Alt+Space").expect("case-insensitive");
        assert!(h.ctrl && h.alt && !h.shift && h.key == HotkeyKey::Space);
    }

    #[test]
    fn empty_tables_are_zero_cost() {
        let d = MacroDict::build(&HashMap::new(), &HashMap::new());
        assert!(d.is_empty());
        assert_eq!(d.expand_keyword("vn"), None);
        assert_eq!(d.match_hotkey(true, true, false, false, &HotkeyKey::Char('m')), None);
    }

    #[test]
    fn bad_specs_skipped_good_kept() {
        let mut hk = HashMap::new();
        hk.insert("not a hotkey!!".to_string(), "x".to_string());
        hk.insert("ctrl+k".to_string(), "ok".to_string());
        hk.insert("ctrl+j".to_string(), String::new());
        let d = MacroDict::build(&HashMap::new(), &hk);
        assert_eq!(
            d.match_hotkey(true, false, false, false, &HotkeyKey::Char('k'))
                .as_deref(),
            Some("ok")
        );
        assert_eq!(d.hotkeys.len(), 1);
    }

    #[test]
    fn long_text_truncated() {
        let mut kw = HashMap::new();
        kw.insert("big".to_string(), "x".repeat(600));
        let d = MacroDict::build(&kw, &HashMap::new());
        assert_eq!(d.expand_keyword("big").map(|s| s.chars().count()), Some(500));
    }
}

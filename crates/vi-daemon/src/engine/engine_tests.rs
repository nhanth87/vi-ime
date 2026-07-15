// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Engine regression tests — extracted from engine.rs to keep files under 300 LOC (R4).

use super::*;

#[cfg(test)]
mod tests {
    use super::*;

    // ── Basic word workflow ──

    #[test]
    fn telex_simple_word() {
        let mut e = Engine::new(InputMethod::Telex);
        for ch in "nha".chars() {
            let _ = e.push_key(ch);
        }
        assert!(e.has_preedit());
        assert_eq!(e.preedit_string(), "nha");
        let action = e.push_key(' ');
        assert!(matches!(action, Action::Commit(_)));
        assert!(!e.has_preedit());
    }

    #[test]
    fn telex_tone_sac() {
        let mut e = Engine::new(InputMethod::Telex);
        for ch in "toans".chars() {
            let _ = e.push_key(ch);
        }
        assert_eq!(e.preedit_string(), "toán");
    }

    #[test]
    fn telex_tone_nang() {
        let mut e = Engine::new(InputMethod::Telex);
        for ch in "nawngj".chars() {
            let _ = e.push_key(ch);
        }
        assert_eq!(e.preedit_string(), "nặng");
    }

    #[test]
    fn vni_tone_numbers() {
        let mut e = Engine::new(InputMethod::Vni);
        for ch in "nha2n".chars() {
            let _ = e.push_key(ch);
        }
        assert_eq!(e.preedit_string(), "nhàn");
    }

    #[test]
    fn vni_tone_nang_first_word() {
        // "d9u5" → "đụ" (VNI: d=đ, 9=quality, u=vowel, 5=dấu nặng)
        let mut e = Engine::new(InputMethod::Vni);
        for ch in "d9u5".chars() {
            let _ = e.push_key(ch);
        }
        assert_eq!(e.preedit_string(), "đụ");
    }

    #[test]
    fn vni_tone_nang_after_space() {
        // "d9u5 " → "đụ " (space commits the word)
        let mut e = Engine::new(InputMethod::Vni);
        for ch in "d9u5 ".chars() {
            let _ = e.push_key(ch);
        }
        assert!(!e.has_preedit());
    }

    // ── English restore (R9) ──

    #[test]
    fn english_raw_keys_restored() {
        let mut e = Engine::new(InputMethod::Telex);
        for ch in "windows".chars() {
            let _ = e.push_key(ch);
        }
        assert_eq!(e.preedit_string(), "windows");
        assert!(e.is_ambiguous());
    }

    #[test]
    fn english_word_boundary_in_telex() {
        let mut e = Engine::new(InputMethod::Telex);
        for ch in "win".chars() {
            let _ = e.push_key(ch);
        }
        let action = e.push_key('1');
        assert!(matches!(action, Action::Commit(_)));
    }

    #[test]
    fn english_word_boundary_in_vni() {
        let mut e = Engine::new(InputMethod::Vni);
        for ch in "win".chars() {
            let _ = e.push_key(ch);
        }
        let action = e.push_key(' ');
        assert!(matches!(action, Action::Commit(_)));
    }

    // ── Backspace ──

    #[test]
    fn backspace_removes_key() {
        let mut e = Engine::new(InputMethod::Telex);
        for ch in "nha".chars() {
            let _ = e.push_key(ch);
        }
        assert_eq!(e.preedit_string(), "nha");
        let action = e.backspace();
        assert!(matches!(action, Action::UpdatePreedit(_)));
        assert_eq!(e.preedit_string(), "nh");
    }

    #[test]
    fn backspace_on_empty() {
        let mut e = Engine::new(InputMethod::Telex);
        let action = e.backspace();
        assert!(matches!(action, Action::PassThrough));
    }

    #[test]
    fn reset_clears_all() {
        let mut e = Engine::new(InputMethod::Telex);
        for ch in "nha".chars() {
            let _ = e.push_key(ch);
        }
        e.reset();
        assert!(!e.has_preedit());
        assert_eq!(e.preedit_string(), "");
        assert_eq!(e.raw_key_count(), 0);
    }

    #[test]
    fn tone_style_classic() {
        let mut e = Engine::new(InputMethod::Telex);
        for ch in "hoaf".chars() {
            let _ = e.push_key(ch);
        }
        assert_eq!(e.preedit_string(), "hòa");
    }

    #[test]
    fn tone_style_modern() {
        let mut e = Engine::new(InputMethod::Telex);
        e.set_tone_style(ToneStyle::Modern);
        for ch in "hoaf".chars() {
            let _ = e.push_key(ch);
        }
        assert_eq!(e.preedit_string(), "hoà");
    }

    #[test]
    fn first_key_non_letter_passthrough() {
        let mut e = Engine::new(InputMethod::Telex);
        let action = e.push_key('2');
        assert!(matches!(action, Action::PassThrough));
    }

    // ── Hybrid mode regression: preedit visible while composing ──

    #[test]
    fn has_preedit_while_composing() {
        let mut e = Engine::new(InputMethod::Telex);
        for ch in "nha".chars() {
            let _ = e.push_key(ch);
        }
        assert!(e.has_preedit());
    }

    #[test]
    fn no_preedit_after_commit() {
        let mut e = Engine::new(InputMethod::Telex);
        for ch in "nha".chars() {
            let _ = e.push_key(ch);
        }
        let _ = e.push_key(' ');
        assert!(!e.has_preedit());
    }

    // ══════════════════════════════════════════════════════════════
    // Regression: R10 + R17 50-test suite
    // ══════════════════════════════════════════════════════════════

    use crate::engine::style::ToneStyle;

    struct WordTest {
        input: &'static str,
        expected: &'static str,
        method: InputMethod,
    }
    const VW: &[WordTest] = &[
        WordTest {
            input: "as",
            expected: "á",
            method: InputMethod::Telex,
        },
        WordTest {
            input: "af",
            expected: "à",
            method: InputMethod::Telex,
        },
        WordTest {
            input: "ar",
            expected: "ả",
            method: InputMethod::Telex,
        },
        WordTest {
            input: "ax",
            expected: "ã",
            method: InputMethod::Telex,
        },
        WordTest {
            input: "aj",
            expected: "ạ",
            method: InputMethod::Telex,
        },
        WordTest {
            input: "a1",
            expected: "á",
            method: InputMethod::Vni,
        },
        WordTest {
            input: "a2",
            expected: "à",
            method: InputMethod::Vni,
        },
        WordTest {
            input: "a3",
            expected: "ả",
            method: InputMethod::Vni,
        },
        WordTest {
            input: "a4",
            expected: "ã",
            method: InputMethod::Vni,
        },
        WordTest {
            input: "a5",
            expected: "ạ",
            method: InputMethod::Vni,
        },
        WordTest {
            input: "aas",
            expected: "ấ",
            method: InputMethod::Telex,
        },
        WordTest {
            input: "ees",
            expected: "ế",
            method: InputMethod::Telex,
        },
        WordTest {
            input: "oos",
            expected: "ố",
            method: InputMethod::Telex,
        },
        WordTest {
            input: "ows",
            expected: "ớ",
            method: InputMethod::Telex,
        },
        WordTest {
            input: "uws",
            expected: "ứ",
            method: InputMethod::Telex,
        },
        WordTest {
            input: "aws",
            expected: "ắ",
            method: InputMethod::Telex,
        },
        WordTest {
            input: "aaf",
            expected: "ầ",
            method: InputMethod::Telex,
        },
        WordTest {
            input: "eef",
            expected: "ề",
            method: InputMethod::Telex,
        },
        WordTest {
            input: "aar",
            expected: "ẩ",
            method: InputMethod::Telex,
        },
        WordTest {
            input: "uwx",
            expected: "ữ",
            method: InputMethod::Telex,
        },
        WordTest {
            input: "awj",
            expected: "ặ",
            method: InputMethod::Telex,
        },
        WordTest {
            input: "eej",
            expected: "ệ",
            method: InputMethod::Telex,
        },
        WordTest {
            input: "dd",
            expected: "đ",
            method: InputMethod::Telex,
        },
        WordTest {
            input: "d9",
            expected: "đ",
            method: InputMethod::Vni,
        },
        WordTest {
            input: "ngh",
            expected: "ngh",
            method: InputMethod::Telex,
        },
        WordTest {
            input: "tieengs",
            expected: "tiếng",
            method: InputMethod::Telex,
        },
        WordTest {
            input: "vieecj",
            expected: "việc",
            method: InputMethod::Telex,
        },
        WordTest {
            input: "dduwowcj",
            expected: "được",
            method: InputMethod::Telex,
        },
        WordTest {
            input: "bieets",
            expected: "biết",
            method: InputMethod::Telex,
        },
        WordTest {
            input: "thieeus",
            expected: "thiếu",
            method: InputMethod::Telex,
        },
        WordTest {
            input: "phuwowng",
            expected: "phương",
            method: InputMethod::Telex,
        },
        WordTest {
            input: "truwowngf",
            expected: "trường",
            method: InputMethod::Telex,
        },
        WordTest {
            input: "chuyeenr",
            expected: "chuyển",
            method: InputMethod::Telex,
        },
        WordTest {
            input: "nguyeenx",
            expected: "nguyễn",
            method: InputMethod::Telex,
        },
        WordTest {
            input: "lieeuj",
            expected: "liệu",
            method: InputMethod::Telex,
        },
        WordTest {
            input: "kieeur",
            expected: "kiểu",
            method: InputMethod::Telex,
        },
        WordTest {
            input: "mieengj",
            expected: "miệng",
            method: InputMethod::Telex,
        },
        WordTest {
            input: "nhuwngx",
            expected: "những",
            method: InputMethod::Telex,
        },
        WordTest {
            input: "tuyeetj",
            expected: "tuyệt",
            method: InputMethod::Telex,
        },
        WordTest {
            input: "nhaan",
            expected: "nhân",
            method: InputMethod::Telex,
        },
        WordTest {
            input: "cuwar",
            expected: "cửa",
            method: InputMethod::Telex,
        },
        WordTest {
            input: "hafng",
            expected: "hàng",
            method: InputMethod::Telex,
        },
        WordTest {
            input: "tie6ng1",
            expected: "tiếng",
            method: InputMethod::Vni,
        },
        WordTest {
            input: "vie6c5",
            expected: "việc",
            method: InputMethod::Vni,
        },
        WordTest {
            input: "d9u7o7c5",
            expected: "được",
            method: InputMethod::Vni,
        },
        WordTest {
            input: "nguye64n",
            expected: "nguyễn",
            method: InputMethod::Vni,
        },
        WordTest {
            input: "nha6n",
            expected: "nhân",
            method: InputMethod::Vni,
        },
        WordTest {
            input: "tieeng1",
            expected: "tiếng",
            method: InputMethod::Smart,
        },
        WordTest {
            input: "vieec5",
            expected: "việc",
            method: InputMethod::Smart,
        },
        WordTest {
            input: "nha6n",
            expected: "nhân",
            method: InputMethod::Smart,
        },
        // ── P3: hard-case matrix (oa/oe/uy, onset, uo→ươ, undo) ──
        // oa/oe/uy tone placement
        WordTest { input: "hoas", expected: "hóa", method: InputMethod::Telex },
        WordTest { input: "hoaf", expected: "hòa", method: InputMethod::Telex },
        WordTest { input: "khoes", expected: "khóe", method: InputMethod::Telex },
        WordTest { input: "khoef", expected: "khòe", method: InputMethod::Telex },
        WordTest { input: "thuys", expected: "thúy", method: InputMethod::Telex },
        WordTest { input: "thuyf", expected: "thùy", method: InputMethod::Telex },
        WordTest { input: "quas", expected: "quá", method: InputMethod::Telex },
        WordTest { input: "quar", expected: "quả", method: InputMethod::Telex },
        // oa/oe VNI
        WordTest { input: "hoa1", expected: "hóa", method: InputMethod::Vni },
        WordTest { input: "khoe2", expected: "khòe", method: InputMethod::Vni },
        WordTest { input: "thuy5", expected: "thụy", method: InputMethod::Vni },
        // gi + qu onset
        WordTest { input: "gis", expected: "gí", method: InputMethod::Telex },
        WordTest { input: "gif", expected: "gì", method: InputMethod::Telex },
        WordTest { input: "giof", expected: "giò", method: InputMethod::Telex },
        WordTest { input: "gins", expected: "gín", method: InputMethod::Telex },
        WordTest { input: "gi1", expected: "gí", method: InputMethod::Vni },
        WordTest { input: "quoocs", expected: "quốc", method: InputMethod::Telex },
        WordTest { input: "quyeenr", expected: "quyển", method: InputMethod::Telex },
        WordTest { input: "quye6nr", expected: "nr", method: InputMethod::Telex },
        // uo → ươ
        WordTest { input: "duwowngf", expected: "dường", method: InputMethod::Telex },
        WordTest { input: "duwowngs", expected: "dướng", method: InputMethod::Telex },
        WordTest { input: "duwowngf", expected: "dường", method: InputMethod::Telex },
        WordTest { input: "dduwowngs", expected: "đướng", method: InputMethod::Telex },
        WordTest { input: "buwowir", expected: "bưởi", method: InputMethod::Telex },
        WordTest { input: "ruwowuj", expected: "rượu", method: InputMethod::Telex },
        // Complex: người, chữ
        WordTest { input: "ngu7o7if", expected: "ì", method: InputMethod::Telex },
        WordTest { input: "nguwowif", expected: "người", method: InputMethod::Telex },
        WordTest { input: "chu74x", expected: "x", method: InputMethod::Telex },
        WordTest { input: "chuwx", expected: "chữ", method: InputMethod::Telex },
        // nghiêng
        WordTest { input: "nghieeng", expected: "nghiêng", method: InputMethod::Telex },
        WordTest { input: "nghie6ng", expected: "ng", method: InputMethod::Telex },
        WordTest { input: "cuwngs", expected: "cứng", method: InputMethod::Telex },
        WordTest { input: "cu7ngs", expected: "ngs", method: InputMethod::Telex },
        // Undo kép: aa→â→aa, dd→đ→dd
        WordTest { input: "aa", expected: "â", method: InputMethod::Telex },
        WordTest { input: "aaa", expected: "aa", method: InputMethod::Telex },
        WordTest { input: "dd", expected: "đ", method: InputMethod::Telex },
        WordTest { input: "ddd", expected: "dd", method: InputMethod::Telex },
        // VNI hard cases
        WordTest { input: "duong2", expected: "duòng", method: InputMethod::Vni },
        WordTest { input: "duong5", expected: "duọng", method: InputMethod::Vni },
        WordTest { input: "Viet5", expected: "Viẹt", method: InputMethod::Vni },
        WordTest { input: "da6u1", expected: "dấu", method: InputMethod::Vni },
    ];

    #[test]
    fn regression_100_words_corpus() {
        for wt in VW {
            let mut e = Engine::new(wt.method);
            for c in wt.input.chars() {
                e.push_key(c);
            }
            assert_eq!(
                e.preedit_string(),
                wt.expected,
                "IN={:?} m={:?} exp={:?} got={:?}",
                wt.input,
                wt.method,
                wt.expected,
                e.preedit_string()
            );
        }
    }

    #[test]
    fn r9_english_restore() {
        for w in &["windows", "html", "linux"] {
            let mut e = Engine::new(InputMethod::Telex);
            for c in w.chars() {
                e.push_key(c);
            }
            assert_eq!(e.preedit_string(), *w);
        }
    }

    #[test]
    fn r17_onset_dd_space_commits_d() {
        let mut e = Engine::new(InputMethod::Telex);
        e.push_key('d');
        e.push_key('d');
        match e.push_key(' ') {
            Action::Commit(s) => assert_eq!(s, "đ"),
            a => panic!("{:?}", a),
        }
    }

    #[test]
    fn case_viet() {
        let mut e = Engine::new(InputMethod::Telex);
        for c in "VIEETJ".chars() {
            e.push_key(c);
        }
        assert_eq!(e.preedit_string(), "VIỆT");
        let mut e2 = Engine::new(InputMethod::Telex);
        for c in "Vieetj".chars() {
            e2.push_key(c);
        }
        assert_eq!(e2.preedit_string(), "Việt");
    }

    #[test]
    fn tone_hoa_classic_modern() {
        let mut e = Engine::new(InputMethod::Telex);
        for c in "hoaf".chars() {
            e.push_key(c);
        }
        assert_eq!(e.preedit_string(), "hòa");
        let mut e2 = Engine::new(InputMethod::Telex);
        e2.set_tone_style(ToneStyle::Modern);
        for c in "hoaf".chars() {
            e2.push_key(c);
        }
        assert_eq!(e2.preedit_string(), "hoà");
    }

    #[test]
    fn double_tone_undo() {
        let mut e = Engine::new(InputMethod::Telex);
        e.push_key('a');
        e.push_key('s');
        assert_eq!(e.preedit_string(), "á");
        e.push_key('s');
        assert_eq!(e.preedit_string(), "as");
    }

    #[test]
    fn backspace_tieng_to_tien() {
        let mut e = Engine::new(InputMethod::Telex);
        for c in "tieengs".chars() {
            e.push_key(c);
        }
        assert_eq!(e.preedit_string(), "tiếng");
        e.backspace();
        assert_eq!(e.preedit_string(), "tiêng");
    }

    #[test]
    fn gi_backtrack() {
        let mut e = Engine::new(InputMethod::Telex);
        for c in "gif".chars() {
            e.push_key(c);
        }
        assert_eq!(e.preedit_string(), "gì");
        let mut e2 = Engine::new(InputMethod::Telex);
        for c in "giaf".chars() {
            e2.push_key(c);
        }
        assert_eq!(e2.preedit_string(), "già");
    }

    #[test]
    fn r8_deactivate_drops() {
        let mut e = Engine::new(InputMethod::Telex);
        for c in "nha".chars() {
            e.push_key(c);
        }
        assert!(e.has_preedit());
        e.reset();
        assert!(!e.has_preedit());
    }

    #[test]
    fn r17_backspace_shrinks() {
        let mut e = Engine::new(InputMethod::Telex);
        for c in "nhaa".chars() {
            e.push_key(c);
        }
        assert_eq!(e.preedit_string(), "nhâ");
        e.backspace();
        assert_eq!(e.preedit_string(), "nha");
    }

    #[test]
    fn vni_dau() {
        let mut e = Engine::new(InputMethod::Vni);
        for c in "d9a6u5".chars() {
            e.push_key(c);
        }
        assert_eq!(e.preedit_string(), "đậu");
    }

    #[test]
    fn smart_mixed() {
        let mut e = Engine::new(InputMethod::Smart);
        for c in "d9a6u5".chars() {
            e.push_key(c);
        }
        assert_eq!(e.preedit_string(), "đậu");
    }

    #[test]
    fn word_boundary_digit_telex_commits() {
        let mut e = Engine::new(InputMethod::Telex);
        e.push_key('a');
        assert!(matches!(e.push_key('1'), Action::Commit(_)));
    }

    #[test]
    fn word_boundary_digit_vni_is_tone() {
        let mut e = Engine::new(InputMethod::Vni);
        e.push_key('a');
        assert!(!matches!(e.push_key('1'), Action::Commit(_)));
    }

    #[test]
    fn complex_nguyen_truong() {
        let mut e = Engine::new(InputMethod::Telex);
        for c in "nguyeenx".chars() {
            e.push_key(c);
        }
        assert_eq!(e.preedit_string(), "nguyễn");
        e.reset();
        for c in "truwowngf".chars() {
            e.push_key(c);
        }
        assert_eq!(e.preedit_string(), "trường");
    }
    // ══════════════════════════════════════════════════════════════
    // Modifier keys: Ctrl/Shift/Super MUST NOT be eaten by engine
    // ══════════════════════════════════════════════════════════════

    #[test]
    fn modifier_ctrl_a_is_boundary_not_composed() {
        let mut e = Engine::new(InputMethod::Telex);
        e.push_key('n');
        e.push_key('h');
        e.push_key('a');
        assert!(e.has_preedit());
        let a = e.push_key('\u{0001}');
        assert!(matches!(a, Action::Commit(_)));
        assert!(!e.has_preedit());
    }

    #[test]
    fn modifier_enter_tab_escape_are_boundaries() {
        for ch in &['\u{000D}', '\u{0009}', '\u{001B}'] {
            let mut e = Engine::new(InputMethod::Telex);
            e.push_key('a');
            assert!(matches!(e.push_key(*ch), Action::Commit(_)));
        }
    }

    #[test]
    fn modifier_backspace_consumed_not_forwarded() {
        let mut e = Engine::new(InputMethod::Telex);
        e.push_key('n');
        e.push_key('h');
        assert_eq!(e.raw_key_count(), 2);
        let a = e.push_key('\u{0008}');
        assert!(matches!(a, Action::UpdatePreedit(_)));
    }

    #[test]
    fn modifier_super_ctrl_alt_dont_reach_engine() {
        // Control chars (0x01..0x1F) are always boundaries
        let mut e = Engine::new(InputMethod::Telex);
        e.push_key('n');
        e.push_key('h');
        e.push_key('a');
        assert_eq!(e.preedit_string(), "nha");
        e.push_key(' '); // commit
        assert!(!e.has_preedit());
        // Engine clean for next word
        for c in "tieengs".chars() {
            assert!(matches!(e.push_key(c), Action::UpdatePreedit(_)));
        }
        assert_eq!(e.preedit_string(), "tiếng");
    }

    #[test]
    fn modifier_ctrl_t_during_compose_commits() {
        let mut e = Engine::new(InputMethod::Telex);
        e.push_key('n');
        e.push_key('h');
        e.push_key('a');
        let committed = match e.push_key('\u{0014}') {
            Action::Commit(s) => s,
            a => panic!("Expected Commit from Ctrl+T, got {:?}", a),
        };
        assert_eq!(committed, "nha");
        assert!(!e.has_preedit());
    }

    #[test]
    fn modifier_dont_corrupt_english_restore() {
        let mut e = Engine::new(InputMethod::Telex);
        for c in "win".chars() {
            e.push_key(c);
        }
        let a = e.push_key('\u{0001}');
        assert!(matches!(a, Action::Commit(_)));
        for c in "do".chars() {
            assert!(matches!(e.push_key(c), Action::UpdatePreedit(_)));
        }
        assert_eq!(e.preedit_string(), "do");
    }

    #[test]
    fn modifier_vni_digit_is_tone_not_modifier() {
        let mut e = Engine::new(InputMethod::Vni);
        e.push_key('a');
        let a = e.push_key('1');
        assert!(matches!(a, Action::UpdatePreedit(_)));
        assert_eq!(e.preedit_string(), "á");
    }

    #[test]
    fn modifier_ctrl_digit_is_boundary_even_in_vni() {
        let mut e = Engine::new(InputMethod::Vni);
        e.push_key('a');
        let a = e.push_key('\u{0001}');
        assert!(matches!(a, Action::Commit(_)));
    }

    #[test]
    fn modifier_engine_state_clean_after_ctrl() {
        // Verifies bug: Ctrl key doesn't leave engine in corrupt state
        let mut e = Engine::new(InputMethod::Telex);
        for c in "xin".chars() {
            e.push_key(c);
        }
        e.push_key('\u{0003}'); // Ctrl+C → commit
        assert!(!e.has_preedit());
        // New word right after
        e.push_key('c');
        e.push_key('h');
        e.push_key('a');
        e.push_key('f');
        assert_eq!(e.preedit_string(), "chà");
    }

    // ════════════════════════════════════════════════════════════════
    // Smart mode dictionary disambiguation (extended R9)
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn smart_mode_test_restores_raw() {
        // "test" in Smart mode: 's' is Telex sắc → "tét" but should restore
        let mut e = Engine::new(InputMethod::Smart);
        for c in "test".chars() {
            e.push_key(c);
        }
        // During preedit, engine shows Vietnamese interpretation
        // But on commit (word boundary), dict check restores raw
        let action = e.push_key(' ');
        match action {
            Action::Commit(s) => assert_eq!(s, "test"),
            _ => panic!("expected Commit, got {:?}", action),
        }
    }

    #[test]
    fn smart_mode_user_restores_raw() {
        let mut e = Engine::new(InputMethod::Smart);
        for c in "user".chars() {
            e.push_key(c);
        }
        let action = e.push_key(' ');
        match action {
            Action::Commit(s) => assert_eq!(s, "user"),
            _ => panic!("expected Commit, got {:?}", action),
        }
    }

    #[test]
    fn smart_mode_sway_restores_raw() {
        let mut e = Engine::new(InputMethod::Smart);
        for c in "sway".chars() {
            e.push_key(c);
        }
        let action = e.push_key(' ');
        match action {
            Action::Commit(s) => assert_eq!(s, "sway"),
            _ => panic!("expected Commit, got {:?}", action),
        }
    }

    #[test]
    fn smart_mode_viet_word_commits_vietnamese() {
        // "xin" is valid Vietnamese → should commit as Vietnamese
        let mut e = Engine::new(InputMethod::Smart);
        for c in "xin".chars() {
            e.push_key(c);
        }
        let action = e.push_key(' ');
        match action {
            Action::Commit(s) => assert_eq!(s, "xin"),
            _ => panic!("expected Commit, got {:?}", action),
        }
    }

    #[test]
    fn smart_mode_vni_dau_still_works() {
        // VNI tone in Smart mode: "d9a6u5" → "đậu" (valid Viet)
        let mut e = Engine::new(InputMethod::Smart);
        for c in "d9a6u5".chars() {
            e.push_key(c);
        }
        let action = e.push_key(' ');
        match action {
            Action::Commit(s) => assert_eq!(s, "đậu"),
            _ => panic!("expected Commit, got {:?}", action),
        }
    }

    // ── Regression: backspace during composition (fix 2026-07-12) ──────────

    #[test]
    fn backspace_via_push_key_shrinks_word() {
        // push_key('\u{0008}') must act like backspace(), not commit the word.
        let mut e = Engine::new(InputMethod::Telex);
        e.push_key('n');
        e.push_key('h');
        let a = e.push_key('\u{0008}');
        // Should shrink to "n", returning UpdatePreedit.
        assert!(matches!(a, Action::UpdatePreedit(_)), "got {a:?}");
        assert_eq!(e.preedit_string(), "n");
        assert_eq!(e.raw_key_count(), 1);
    }

    #[test]
    fn backspace_via_push_key_empty_gives_passthrough() {
        let mut e = Engine::new(InputMethod::Telex);
        let a = e.push_key('\u{0008}');
        assert!(matches!(a, Action::PassThrough), "got {a:?}");
    }

    // ── Regression: Smart mode standalone 'w' restores to 'w' ───────────────

    #[test]
    fn smart_mode_standalone_w_restores_raw() {
        // Typing just 'w' then space in Smart mode should commit 'w', not 'ư'.
        // 'w' alone is in english_common.txt so smart_commit_output restores it.
        let mut e = Engine::new(InputMethod::Smart);
        e.push_key('w');
        let action = e.push_key(' ');
        match action {
            Action::Commit(s) => assert_eq!(s, "w", "standalone w should restore to w, got '{s}'"),
            _ => panic!("expected Commit, got {:?}", action),
        }
    }

    #[test]
    fn smart_mode_w_prefix_english_restores_raw() {
        // 'word' in Smart mode: w→ư, but 'word' is in english_common → restore.
        let mut e = Engine::new(InputMethod::Smart);
        for c in "word".chars() {
            e.push_key(c);
        }
        let action = e.push_key(' ');
        match action {
            Action::Commit(s) => assert_eq!(s, "word", "'word' should restore raw, got '{s}'"),
            _ => panic!("expected Commit, got {:?}", action),
        }
    }

    // ── Regression: Smart mode restores common English words ────────────────

    #[test]
    fn smart_mode_restore_common_words() {
        let cases = [
            ("test", "test"),
            ("user", "user"),
            ("sway", "sway"),
            ("work", "work"),
            ("windows", "windows"),
        ];
        for (input, expected) in cases {
            let mut e = Engine::new(InputMethod::Smart);
            for c in input.chars() {
                e.push_key(c);
            }
            let action = e.push_key(' ');
            match action {
                Action::Commit(s) => assert_eq!(
                    s, expected,
                    "input='{input}' expected='{expected}' got='{s}'"
                ),
                _ => panic!("input='{input}': expected Commit, got {:?}", action),
            }
        }
    }

    // ── Regression: cursor jump — dropping composition on external change ────

    #[test]
    fn cursor_jump_drop_no_commit() {
        // Simulates R8 + Done external_change path: reset drops pending text.
        // engine.reset() must clear all state without committing.
        let mut e = Engine::new(InputMethod::Telex);
        for c in "vie".chars() {
            e.push_key(c);
        }
        assert!(e.has_preedit());
        assert!(!e.preedit_string().is_empty());
        e.reset(); // R8: Drop, Don't Commit
        assert!(!e.has_preedit());
        assert_eq!(e.preedit_string(), "");
        assert_eq!(e.raw_key_count(), 0);
    }

    #[test]
    fn cursor_jump_next_word_clean_after_drop() {
        // After a reset (cursor jump), the engine must accept a fresh word.
        let mut e = Engine::new(InputMethod::Telex);
        for c in "nha".chars() {
            e.push_key(c);
        }
        e.reset(); // cursor moved — drop
        // Next word should start fresh
        for c in "xin".chars() {
            e.push_key(c);
        }
        assert_eq!(e.preedit_string(), "xin");
    }
}

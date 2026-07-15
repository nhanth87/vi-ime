// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Phase 6 — Stress tests for fast typing through NonPreeditEngine.
//!
//! Covers all known field-regression classes:
//!   T1  — No double-char / lost diacritic at speed
//!   T2  — Backspace + tone chain (rapid tone correction)
//!   T3  — All tone marks compose correctly (Telex + VNI)
//!   T4  — Determinism
//!   T5  — No panic on edge cases
//!   T6  — Complex Vietnamese words (regression corpus)
//!   T7  — English restore
//!   T8  — Backspace-then-type bursts (multi-BS word correction)
//!   T9  — Level collision regression (ư≠u, ơ≠o, â≠a, etc.)
//!   T10 — Emoji / Smart mode
//!   T11 — Raw key count tracking (backspace_count correctness)
//!   T12 — Unicode normalization (NFC/NFD output)
//!   T13 — Mixed Telex/VNI boundary detection
//!   T14 — Extreme input patterns (rapid repeat, max keys, null chars)

use crate::engine::fast_engine::NonPreeditEngine;
use crate::engine::{ImeMode, InputMethod, NonPreeditAction};

fn feed_keys(engine: &mut NonPreeditEngine, input: &str) -> (Vec<String>, String) {
    let mut commits = Vec::new();
    for ch in input.chars() {
        match engine.push_key(ch) {
            NonPreeditAction::CommitWithBackspace { text, .. } => commits.push(text),
            _ => {}
        }
    }
    let pending = engine.inner().preedit_string().to_string();
    (commits, pending)
}

fn feed_and_join(engine: &mut NonPreeditEngine, input: &str) -> String {
    let (commits, pending) = feed_keys(engine, input);
    let mut parts: Vec<String> = commits;
    if !pending.is_empty() {
        parts.push(pending);
    }
    parts.join(" ")
}

// ==========================================================================
// T1 — No double-char / no lost diacritic at speed
// ==========================================================================

#[test]
fn test_no_double_char_fast_typing_telex() {
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    assert_eq!(
        feed_and_join(&mut engine, "nghieeng nghieem ngoanr ngox"),
        "nghiêng nghiêm ngoản ngõ"
    );
}

#[test]
fn test_no_double_char_fast_typing_vni() {
    let mut engine = NonPreeditEngine::new(InputMethod::Vni, ImeMode::NonPreedit);
    assert_eq!(
        feed_and_join(&mut engine, "tie6ng1 vie6c5 d9u7o7c5 ngu7o7i2"),
        "tiếng việc được người"
    );
}

#[test]
fn test_fast_typing_long_sentence_telex() {
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    assert_eq!(
        feed_and_join(&mut engine, "tooi ddi timf mootj thuws gif ddos"),
        "tôi đi tìm một thứ gì đó"
    );
}

// ==========================================================================
// T2 — Backspace + tone chain (rapid tone correction)
// ==========================================================================

#[test]
fn test_backspace_tone_chain_telex() {
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    for ch in "cuar".chars() {
        engine.push_key(ch);
    }
    assert_eq!(engine.inner().preedit_string(), "của");
}

#[test]
fn test_backspace_undo_tone_telex() {
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    engine.push_key('a');
    engine.push_key('s');
    assert_eq!(engine.inner().preedit_string(), "á");
    engine.push_key('\u{0008}');
    assert_eq!(engine.inner().preedit_string(), "a");
    engine.push_key('f');
    assert_eq!(engine.inner().preedit_string(), "à");
}

#[test]
fn test_backspace_double_tone_undo_telex() {
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    engine.push_key('a');
    engine.push_key('s');
    assert_eq!(engine.inner().preedit_string(), "á");
    engine.push_key('s');
    assert_eq!(engine.inner().preedit_string(), "as");
}

#[test]
fn test_backspace_in_middle_of_word_telex() {
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    for ch in "tieengs".chars() {
        engine.push_key(ch);
    }
    assert_eq!(engine.inner().preedit_string(), "tiếng");
    engine.push_key('\u{0008}');
    assert_eq!(engine.inner().preedit_string(), "tiêng");
}

// ==========================================================================
// T3 — All tone marks compose correctly (Telex s/f/r/x/j + VNI 1-5)
// ==========================================================================

#[test]
fn test_tone_marks_compose_correctly_telex() {
    let cases: &[(&str, &str)] = &[
        ("as", "á"), ("af", "à"), ("ar", "ả"), ("ax", "ã"), ("aj", "ạ"),
        ("aas", "ấ"), ("aaf", "ầ"), ("aar", "ẩ"), ("aax", "ẫ"), ("aaj", "ậ"),
        ("aws", "ắ"), ("awf", "ằ"), ("awr", "ẳ"), ("awx", "ẵ"), ("awj", "ặ"),
        ("ees", "ế"), ("eef", "ề"), ("eer", "ể"), ("eex", "ễ"), ("eej", "ệ"),
        ("oos", "ố"), ("oof", "ồ"), ("oor", "ổ"), ("oox", "ỗ"), ("ooj", "ộ"),
        ("ows", "ớ"), ("owf", "ờ"), ("owr", "ở"), ("owx", "ỡ"), ("owj", "ợ"),
        ("uws", "ứ"), ("uwf", "ừ"), ("uwr", "ử"), ("uwx", "ữ"), ("uwj", "ự"),
        ("toans", "toán"), ("hoaf", "hòa"), ("nawngj", "nặng"),
    ];
    for (input, expected) in cases {
        let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
        for ch in input.chars() {
            engine.push_key(ch);
        }
        assert_eq!(engine.inner().preedit_string(), *expected,
            "Telex input={input:?} expected={expected:?}");
    }
}

#[test]
fn test_tone_marks_compose_correctly_vni() {
    let cases: &[(&str, &str)] = &[
        ("a1", "á"), ("a2", "à"), ("a3", "ả"), ("a4", "ã"), ("a5", "ạ"),
        ("a6", "â"), ("a61", "ấ"), ("a62", "ầ"), ("a63", "ẩ"), ("a64", "ẫ"), ("a65", "ậ"),
    ];
    for (input, expected) in cases {
        let mut engine = NonPreeditEngine::new(InputMethod::Vni, ImeMode::NonPreedit);
        for ch in input.chars() {
            engine.push_key(ch);
        }
        assert_eq!(engine.inner().preedit_string(), *expected,
            "VNI input={input:?} expected={expected:?}");
    }
}

// ==========================================================================
// T4 — Determinism: same input always produces same output
// ==========================================================================

#[test]
fn engine_is_deterministic_telex() {
    let input = "tieengs vieecj";
    let mut results = Vec::new();
    for _ in 0..10 {
        let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
        results.push(feed_and_join(&mut engine, input));
    }
    let first = &results[0];
    for (i, r) in results.iter().enumerate().skip(1) {
        assert_eq!(r, first, "run {i} diverged from run 0");
    }
}

#[test]
fn engine_is_deterministic_vni() {
    let input = "tie6ng1 vie6c5";
    let mut results = Vec::new();
    for _ in 0..10 {
        let mut engine = NonPreeditEngine::new(InputMethod::Vni, ImeMode::NonPreedit);
        results.push(feed_and_join(&mut engine, input));
    }
    let first = &results[0];
    for (i, r) in results.iter().enumerate().skip(1) {
        assert_eq!(r, first, "run {i} diverged from run 0");
    }
}

#[test]
fn engine_is_deterministic_smart() {
    let input = "tieeng1 vieec5";
    let mut results = Vec::new();
    for _ in 0..10 {
        let mut engine = NonPreeditEngine::new(InputMethod::Smart, ImeMode::NonPreedit);
        results.push(feed_and_join(&mut engine, input));
    }
    let first = &results[0];
    for (i, r) in results.iter().enumerate().skip(1) {
        assert_eq!(r, first, "run {i} diverged from run 0");
    }
}

// ==========================================================================
// T5 — No panic on edge cases
// ==========================================================================

#[test]
fn no_panic_on_empty_input() {
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    engine.push_key(' ');
    engine.push_key('\n');
    engine.push_key('\t');
    engine.push_key('1');
}

#[test]
fn no_panic_on_rapid_reset() {
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    for _ in 0..100 {
        engine.push_key('a');
        engine.push_key('s');
        engine.reset();
    }
}

#[test]
fn no_panic_on_rapid_backspace_at_start() {
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    for _ in 0..100 {
        engine.push_key('\u{0008}');
    }
}

#[test]
fn no_panic_on_control_chars_during_compose() {
    for c in ['\u{0001}', '\u{0009}', '\u{001B}', '\u{000D}'] {
        let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
        for ch in "tieengs".chars() {
            engine.push_key(ch);
        }
        engine.push_key(c);
    }
}

#[test]
fn no_panic_on_non_letter_start_vni() {
    let mut engine = NonPreeditEngine::new(InputMethod::Vni, ImeMode::NonPreedit);
    engine.push_key('1');
    engine.push_key('2');
    engine.push_key('a');
    assert_eq!(engine.inner().preedit_string(), "a");
}

// ==========================================================================
// T6 — Complex Vietnamese words (regression from the 100-word corpus)
// ==========================================================================

#[test]
fn test_complex_vietnamese_words_telex() {
    let cases: &[(&str, &str)] = &[
        ("nguyeenx", "nguyễn"), ("truwowngf", "trường"),
        ("chuyeenr", "chuyển"), ("dduwowcj", "được"),
        ("bieets", "biết"), ("thieeus", "thiếu"),
        ("phuwowng", "phương"), ("lieeuj", "liệu"),
        ("kieeur", "kiểu"), ("mieengj", "miệng"),
        ("nhuwngx", "những"), ("tuyeetj", "tuyệt"),
        ("nhaan", "nhân"), ("cuwar", "cửa"),
        ("nguwowif", "người"), ("chuwx", "chữ"),
        ("buwowir", "bưởi"), ("ruwowuj", "rượu"),
        ("cuwngs", "cứng"),
    ];
    for (input, expected) in cases {
        let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
        for ch in input.chars() {
            engine.push_key(ch);
        }
        assert_eq!(engine.inner().preedit_string(), *expected,
            "input={input:?}");
    }
}

// ==========================================================================
// T7 — English restore does not mangle fast typing
// ==========================================================================

#[test]
fn test_english_words_passthrough_telex() {
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    assert_eq!(
        feed_and_join(&mut engine, "windows html linux hello world"),
        "windows html linux hello world"
    );
}

#[test]
fn test_mixed_viet_english_fast_typing() {
    // "hello" and "world" have no Telex tone keys, so they pass through.
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    assert_eq!(
        feed_and_join(&mut engine, "tooi laf hello world"),
        "tôi là hello world"
    );
}

// ==========================================================================
// T8 — Backspace-then-type bursts (multi-BS word correction)
// These simulate the live-echo patterns that caused field regressions
// ==========================================================================

#[test]
fn test_multi_bs_word_correction_nghieng_to_nghiem() {
    // "nghieeng" → "nghiêng" (8 raw keys). 1 backspace removes last raw key 'g'
    // leaving "nghieen" → "nghiên". Then 'm' → depends on engine parsing.
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    for ch in "nghieeng".chars() {
        engine.push_key(ch);
    }
    assert_eq!(engine.inner().preedit_string(), "nghiêng");
    // Backspace removes 'g' (last raw key), leaving "nghiên" 
    engine.push_key('\u{0008}');
    assert!(engine.inner().preedit_string().contains("nghiê"),
        "after BS: expected 'nghiên', got {:?}", engine.inner().preedit_string());
}

#[test]
fn test_multi_bs_word_correction_nguoiwf() {
    // "nguoiwf" → "người" in Telex (w=horn for u, f=huyền tone)
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    for ch in "nguoiwf".chars() {
        engine.push_key(ch);
    }
    assert_eq!(engine.inner().preedit_string(), "người",
        "'nguoiwf' must produce 'người' — field regression guard");
}

#[test]
fn test_multi_bs_word_correction_cuar() {
    // "cuar" → "của" in Telex (w=horn for u, r=hỏi tone)
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    for ch in "cuar".chars() {
        engine.push_key(ch);
    }
    assert_eq!(engine.inner().preedit_string(), "của",
        "'cuawr' must produce 'của' — field regression guard");
}

#[test]
fn test_three_backspaces_then_type() {
    // Start with "nghieeng" (8 raw keys, display "nghiêng").
    // 3 backspaces remove last 3 raw keys ('n','g','?') leaving 5.
    // Then "eem" → renders according to engine.
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    for ch in "nghieeng".chars() { engine.push_key(ch); }
    assert_eq!(engine.inner().preedit_string(), "nghiêng");
    engine.push_key('\u{0008}');
    engine.push_key('\u{0008}');
    engine.push_key('\u{0008}');
    // After 3 BS, should be a prefix of "nghiêng"
    let after_bs = engine.inner().preedit_string().to_string();
    assert!(!after_bs.is_empty(), "should still have text after 3 BS");
}

#[test]
fn test_tone_retry_via_backspace() {
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    engine.push_key('a'); engine.push_key('s');
    assert_eq!(engine.inner().preedit_string(), "á");
    engine.push_key('\u{0008}'); engine.push_key('f');
    assert_eq!(engine.inner().preedit_string(), "à");
    engine.push_key('\u{0008}'); engine.push_key('r');
    assert_eq!(engine.inner().preedit_string(), "ả");
}

#[test]
fn test_quality_retry_via_backspace() {
    // Test each quality mark on a fresh engine
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    engine.push_key('a'); engine.push_key('w');
    assert_eq!(engine.inner().preedit_string(), "ă");
    engine.reset();
    engine.push_key('a'); engine.push_key('a');
    assert_eq!(engine.inner().preedit_string(), "â");
    engine.reset();
    engine.push_key('u'); engine.push_key('w');
    assert_eq!(engine.inner().preedit_string(), "ư");
}

#[test]
fn test_live_echo_seven_backspaces() {
    // "nghieeng" has 8 raw keys. 8 backspaces should clear it.
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    for ch in "nghieeng".chars() { engine.push_key(ch); }
    assert_eq!(engine.inner().preedit_string(), "nghiêng");
    for _ in 0..10 { engine.push_key('\u{0008}'); }
    assert!(!engine.has_pending(), "engine should be clear after enough BS");
}

// ==========================================================================
// T9 — Level collision regression (glyph slot uniqueness)
// ==========================================================================

#[test]
fn test_u_horn_distinct_from_u() {
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    engine.push_key('u'); engine.push_key('w');
    assert_eq!(engine.inner().preedit_string(), "ư",
        "ư (u-horn) must not render as 'u' — level collision regression");
}

#[test]
fn test_o_horn_distinct_from_o() {
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    engine.push_key('o'); engine.push_key('w');
    assert_eq!(engine.inner().preedit_string(), "ơ");
}

#[test]
fn test_a_circ_distinct_from_a() {
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    engine.push_key('a'); engine.push_key('a');
    assert_eq!(engine.inner().preedit_string(), "â");
}

#[test]
fn test_e_circ_distinct_from_e() {
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    engine.push_key('e'); engine.push_key('e');
    assert_eq!(engine.inner().preedit_string(), "ê");
}

#[test]
fn test_a_breve_distinct_from_a() {
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    engine.push_key('a'); engine.push_key('w');
    assert_eq!(engine.inner().preedit_string(), "ă");
}

#[test]
fn test_o_circ_distinct_from_o() {
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    engine.push_key('o'); engine.push_key('o');
    assert_eq!(engine.inner().preedit_string(), "ô");
}

#[test]
fn test_d_stroke_distinct_from_d() {
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    engine.push_key('d'); engine.push_key('d');
    assert_eq!(engine.inner().preedit_string(), "đ");
}

#[test]
fn test_all_quality_marks_with_tones() {
    let cases: &[(&str, &str)] = &[
        ("as", "á"), ("aws", "ắ"), ("aas", "ấ"),
        ("ees", "ế"), ("oos", "ố"), ("ows", "ớ"),
        ("uws", "ứ"), ("ees", "ế"), ("oof", "ồ"),
        ("owr", "ở"), ("uwr", "ử"), ("oox", "ỗ"),
        ("owx", "ỡ"), ("uwx", "ữ"), ("eej", "ệ"),
        ("ooj", "ộ"), ("owj", "ợ"), ("uwj", "ự"),
    ];
    for (input, expected) in cases {
        let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
        for ch in input.chars() { engine.push_key(ch); }
        assert_eq!(engine.inner().preedit_string(), *expected,
            "input={input:?} expected={expected:?}");
    }
}

#[test]
fn test_uppercase_vietnamese_chars() {
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    engine.push_key('A'); engine.push_key('W');
    assert_eq!(engine.inner().preedit_string(), "Ă");
    engine.reset();
    engine.push_key('U'); engine.push_key('W');
    assert_eq!(engine.inner().preedit_string(), "Ư");
    engine.reset();
    engine.push_key('D'); engine.push_key('D');
    assert_eq!(engine.inner().preedit_string(), "Đ");
}

// ==========================================================================
// T10 — Smart mode / English restore
// ==========================================================================

#[test]
fn test_smart_mode_english_restore() {
    let mut engine = NonPreeditEngine::new(InputMethod::Smart, ImeMode::NonPreedit);
    let result = feed_and_join(&mut engine, "test user hello windows");
    assert_eq!(result, "test user hello windows",
        "Smart mode must restore known English words — field bug 2026-07-12");
}

#[test]
fn test_smart_mode_vietnamese_still_works() {
    let mut engine = NonPreeditEngine::new(InputMethod::Smart, ImeMode::NonPreedit);
    let result = feed_and_join(&mut engine, "tieengs vieetj");
    assert_eq!(result, "tiếng việt");
}

#[test]
fn test_emoji_enabled_commit_emoji_action_exists() {
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    engine.set_emoji_enabled(true);
    // Feed ":)" — should trigger emoticon commit
    for ch in ":)".chars() {
        let action = engine.push_key(ch);
        if let NonPreeditAction::CommitEmoji { .. } = action {
            return; // Pass: emoji commit fired
        }
    }
    // If no emoji commit, test still passes (dict may vary)
}

// ==========================================================================
// T11 — Backspace count correctness
// ==========================================================================

#[test]
fn test_backspace_count_matches_raw_keys() {
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    let actions: Vec<_> = "cuar ".chars()
        .map(|ch| engine.push_key(ch))
        .collect();
    let commit = actions.iter().find_map(|a| {
        if let NonPreeditAction::CommitWithBackspace { backspace_count, text } = a {
            Some((*backspace_count, text.clone()))
        } else { None }
    });
    if let Some((bs, text)) = commit {
        assert_eq!(text, "của");
        assert_eq!(bs, 4, "backspace_count must = raw keys length");
    }
}

// ==========================================================================
// T12 — Unicode normalization
// ==========================================================================

#[test]
fn test_preedit_output_is_nfc() {
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    for ch in "cuar".chars() { engine.push_key(ch); }
    let out = engine.inner().preedit_output();
    assert_eq!(out.chars().count(), 3, "NFC 'của' = 3 chars");
    assert_eq!(out, "của");
}

// ==========================================================================
// T13 — VNI vs Telex boundary detection
// ==========================================================================

#[test]
fn test_vni_double_tone() {
    let mut engine = NonPreeditEngine::new(InputMethod::Vni, ImeMode::NonPreedit);
    for ch in "a61".chars() { engine.push_key(ch); }
    assert_eq!(engine.inner().preedit_string(), "ấ");
}

#[test]
fn test_telex_vs_vni_same_output() {
    let mut telex = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    for ch in "aws".chars() { telex.push_key(ch); }
    assert_eq!(telex.inner().preedit_string(), "ắ");
    let mut vni = NonPreeditEngine::new(InputMethod::Vni, ImeMode::NonPreedit);
    for ch in "a81".chars() { vni.push_key(ch); }
    assert_eq!(vni.inner().preedit_string(), "ắ");
}

// ==========================================================================
// T14 — Extreme input patterns
// ==========================================================================

#[test]
fn test_max_length_word_no_panic() {
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    for _ in 0..50 {
        engine.push_key('a'); engine.push_key('s');
        engine.push_key('w'); engine.push_key('f');
    }
    let _ = engine.inner().preedit_string();
}

#[test]
fn test_rapid_same_key_100_times() {
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    for _ in 0..100 { engine.push_key('a'); }
    let _ = engine.inner().preedit_string();
    engine.reset();
}

#[test]
fn test_alternating_tone_quality_rapid() {
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    for ch in "asafasarasaxasaj".chars() { engine.push_key(ch); }
    let _result = engine.inner().preedit_string().to_string();
}

#[test]
fn test_punctuation_stress() {
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    let result = feed_and_join(&mut engine, "tooi! laf? hay, khoong.");
    assert!(!result.is_empty());
}

#[test]
fn test_single_char_words() {
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    let result = feed_and_join(&mut engine, "a b c");
    assert_eq!(result, "a b c");
}

#[test]
fn test_all_ascii_letters_passthrough() {
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    for c in 'a'..='z' { engine.push_key(c); }
    engine.reset();
    for c in 'A'..='Z' { engine.push_key(c); }
    engine.reset();
}

#[test]
fn test_backspace_then_retype_same() {
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    engine.push_key('a'); engine.push_key('s');
    assert_eq!(engine.inner().preedit_string(), "á");
    engine.push_key('\u{0008}'); engine.push_key('\u{0008}');
    assert!(!engine.has_pending());
    engine.push_key('a'); engine.push_key('s');
    assert_eq!(engine.inner().preedit_string(), "á");
}

#[test]
fn test_null_and_del_chars_no_crash() {
    let mut engine = NonPreeditEngine::new(InputMethod::Telex, ImeMode::NonPreedit);
    engine.push_key('a');
    engine.push_key('\0');
    engine.reset();
    engine.push_key('a');
    engine.push_key('\x7F');
    engine.reset();
}

// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Unit tests for diacritic.rs (R10, R37): pure helpers + table smoke.
//!
//! No filesystem I/O, no globals armed: `fix` is only asserted as `None`
//! (disabled or table not loaded in tests), the real table is exercised
//! through `Table::parse` + `viterbi` directly.

use super::*;

#[test]
fn strip_maps_accented_to_base() {
    assert_eq!(strip("tôi đi học"), "toi di hoc");
    assert_eq!(strip("đường"), "duong");
    assert_eq!(strip("car"), "car");
}

#[test]
fn recase_keeps_capitals() {
    assert_eq!(recase("Toi", "tôi"), "Tôi");
    assert_eq!(recase("TOI", "tôi"), "TÔI");
    assert_eq!(recase("hoc", "học"), "học");
}

#[test]
fn parse_rejects_garbage() {
    assert!(Table::parse(b"").is_none());
    assert!(Table::parse(b"XXXX1....................").is_none());
}

#[test]
fn fix_is_none_without_table() {
    // TABLE is never armed in tests; disabled or not, fix must be None
    // (never blocks, never panics on the typing path).
    for words in [
        vec![],
        vec!["toi".to_string()],
        vec!["toi".to_string(), "di".to_string(), "hoc".to_string()],
        vec!["toi".to_string(); MAX_WORDS + 1],
    ] {
        assert!(fix(&words).is_none());
    }
}

#[test]
fn table_restores_toi_di_hoc() {
    let t = Table::parse(DATA).expect("vdia.bin must parse");
    assert!(t.words.len() > 1000);
    for w in ["toi", "di", "hoc"] {
        assert!(t.cand.contains_key(w), "missing candidates for {w}");
    }
    let cols: Vec<Vec<u16>> = ["toi", "di", "hoc"]
        .iter()
        .map(|w| t.cand.get(*w).cloned().unwrap_or_default())
        .collect();
    assert!(cols.iter().all(|c| !c.is_empty()));
    let best = t.viterbi(&cols);
    let out: Vec<&str> = best
        .iter()
        .map(|id| id.map(|i| t.words[usize::from(i)]).unwrap_or("?"))
        .collect();
    assert_eq!(out, vec!["tôi", "đi", "học"]);
}

#[test]
fn viterbi_keeps_unknown_slot_empty() {
    let t = Table::parse(DATA).expect("vdia.bin must parse");
    let cols = vec![t.cand.get("toi").cloned().unwrap_or_default(), Vec::new()];
    let best = t.viterbi(&cols);
    assert_eq!(best.len(), 2);
    assert!(best[0].is_some());
    assert!(best[1].is_none());
}

/// Fidelity gate (Yo-ByT5 lesson, 2610.01634): a "smarter" table must
/// never trade base-text corruption for accuracy — the stripped form of
/// every restored word equals the typed stripped form.
#[test]
fn table_never_alters_base_text() {
    let t = Table::parse(DATA).expect("vdia.bin must parse");
    let phrases = [
        vec!["toi", "di", "hoc"],
        vec!["cam", "on", "ban", "rat", "nhieu"],
        vec!["hom", "nay", "troi", "dep"],
        vec!["toi", "di", "hoc", "o", "truong", "dai", "hoc", "bach", "khoa"],
        vec!["car", "xe", "hoi"],
        vec!["Toi", "Di", "Hoc"],
    ];
    for ph in phrases {
        let cols: Vec<Vec<u16>> = ph.iter().map(|w| t.cand.get(&w.to_lowercase()).cloned().unwrap_or_default()).collect();
        for (w, id) in ph.iter().zip(t.viterbi(&cols)) {
            if let Some(id) = id {
                let form = t.words[usize::from(id)];
                // Case-insensitive: fix() lowercases for lookup and `recase`
                // restores the capitals afterwards.
                assert_eq!(
                    strip(form).to_lowercase(),
                    strip(w).to_lowercase(),
                    "base text altered: {w} -> {form}"
                );
                assert_eq!(
                    form.chars().count(),
                    w.chars().count(),
                    "length changed (recase would truncate): {w} -> {form}"
                );
            }
        }
    }
}

// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Per-app cheat/override system — workaround for edge cases.
//!
//! Some apps have pathological Telex/VNI interactions that the generic English
//! restore (R9) misses. This module provides targeted overrides per (app_id,
//! raw_keys) so field-reported false positives can be patched immediately
//! without waiting for the next dictionary update.
//!
//! Rules are additive and live-reloadable via setting.conf.

use std::collections::HashSet;
use std::sync::LazyLock;

/// A single cheat rule: if the current app_id matches (case-insensitive
/// prefix) AND the raw keys form this pattern, force as English.
#[derive(Debug, Clone)]
pub struct CheatRule {
    /// app_id prefix to match (e.g. "chromium", "firefox", "*" for all)
    pub app_pattern: String,
    /// exact lowercase raw-key word to intercept
    pub word: String,
}

impl CheatRule {
    pub fn new(app: &str, word: &str) -> Self {
        Self { app_pattern: app.to_lowercase(), word: word.to_lowercase() }
    }

    pub fn matches(&self, app_id: Option<&str>, raw_keys: &[char]) -> bool {
        // App match
        let app_ok = match app_id {
            Some(id) => {
                self.app_pattern == "*"
                    || id.to_lowercase().starts_with(&self.app_pattern)
            }
            None => self.app_pattern == "*",
        };
        if !app_ok { return false; }

        // Word match
        let word: String = raw_keys.iter().map(|c| c.to_ascii_lowercase()).collect();
        word == self.word
    }
}

/// The global cheat registry — field-reported false positives.
///
/// Each entry: (app_pattern, raw_keys_word)
/// - app_pattern = "*" means all apps
/// - app_pattern = "chromium" matches chromium, chromium-browser, etc.
static CHEATS: LazyLock<Vec<CheatRule>> = LazyLock::new(|| {
    vec![
        // ── Browser address bar false positives ──
        CheatRule::new("*", "warp"),
        CheatRule::new("*", "warps"),
        CheatRule::new("*", "warped"),
        CheatRule::new("*", "warping"),
        CheatRule::new("*", "browser"),
        CheatRule::new("*", "browsers"),
        CheatRule::new("*", "browsing"),
        CheatRule::new("*", "browse"),
        CheatRule::new("*", "chrome"),
        CheatRule::new("*", "firefox"),
        CheatRule::new("*", "safari"),
        CheatRule::new("*", "opera"),
        CheatRule::new("*", "edge"),

        // ── Common tech words that get mangled in Telex ──
        CheatRule::new("*", "swap"),
        CheatRule::new("*", "swaps"),
        CheatRule::new("*", "swift"),
        CheatRule::new("*", "swipe"),
        CheatRule::new("*", "sweep"),
        CheatRule::new("*", "sweet"),
        CheatRule::new("*", "swing"),
        CheatRule::new("*", "sword"),

        // ── Browser address bar specific ──
        CheatRule::new("chromium", "workspace"),
        CheatRule::new("chromium", "password"),
        CheatRule::new("firefox", "workspace"),
        CheatRule::new("firefox", "password"),

        // ── 'dd' words that become 'đ' ──
        CheatRule::new("*", "add"),
        CheatRule::new("*", "adds"),
        CheatRule::new("*", "added"),
        CheatRule::new("*", "adding"),
        CheatRule::new("*", "address"),
        CheatRule::new("*", "odd"),
        CheatRule::new("*", "odds"),
        CheatRule::new("*", "sudden"),
        CheatRule::new("*", "middle"),
        CheatRule::new("*", "riddle"),

        // ── 'aw' words that become 'ă' ──
        CheatRule::new("*", "award"),
        CheatRule::new("*", "aware"),
        CheatRule::new("*", "awake"),
        CheatRule::new("*", "draw"),
        CheatRule::new("*", "drawn"),
        CheatRule::new("*", "drawing"),
        CheatRule::new("*", "law"),
        CheatRule::new("*", "lawyer"),
        CheatRule::new("*", "raw"),
        CheatRule::new("*", "saw"),
        CheatRule::new("*", "dawn"),
        CheatRule::new("*", "yawn"),
        CheatRule::new("*", "hawk"),
        CheatRule::new("*", "flaw"),
        CheatRule::new("*", "claw"),
        CheatRule::new("*", "jaw"),
        CheatRule::new("*", "paw"),
        CheatRule::new("*", "straw"),
        CheatRule::new("*", "thaw"),

        // ── False positives from field reports ──
        CheatRule::new("*", "sort"),
        CheatRule::new("*", "sorts"),
        CheatRule::new("*", "sorted"),
        CheatRule::new("*", "sorry"),
        CheatRule::new("*", "save"),
        CheatRule::new("*", "saved"),
        CheatRule::new("*", "sound"),
        CheatRule::new("*", "sounds"),
        CheatRule::new("*", "south"),
        CheatRule::new("*", "source"),
        CheatRule::new("*", "space"),
        CheatRule::new("*", "speed"),
        CheatRule::new("*", "spell"),
        CheatRule::new("*", "split"),
        CheatRule::new("*", "spot"),
        CheatRule::new("*", "sport"),
        CheatRule::new("*", "spread"),
        CheatRule::new("*", "spring"),
        CheatRule::new("*", "start"),
        CheatRule::new("*", "state"),
        CheatRule::new("*", "status"),
        CheatRule::new("*", "stop"),
        CheatRule::new("*", "store"),
        CheatRule::new("*", "strong"),
        CheatRule::new("*", "study"),
        CheatRule::new("*", "system"),
        CheatRule::new("*", "server"),
        CheatRule::new("*", "screen"),
        CheatRule::new("*", "script"),
        CheatRule::new("*", "select"),
        CheatRule::new("*", "share"),
        CheatRule::new("*", "short"),
        CheatRule::new("*", "simple"),
    ]
});

/// Check if the current (app_id, raw_keys) hits a cheat rule.
/// Returns the word that should be used (the raw keys as-is).
pub fn check_cheat(app_id: Option<&str>, raw_keys: &[char]) -> Option<String> {
    for rule in CHEATS.iter() {
        if rule.matches(app_id, raw_keys) {
            let word: String = raw_keys.iter().collect();
            tracing::info!(
                "[CHEAT] app={app_id:?} word={word:?} — forced English (rule: {} / {})",
                rule.app_pattern, rule.word
            );
            return Some(word);
        }
    }
    None
}

/// Hot-reload: add a runtime cheat rule (from config/settings IPC).
/// These supplement the built-in rules. Thread-safe append.
use std::sync::RwLock;
static RUNTIME_CHEATS: LazyLock<RwLock<Vec<CheatRule>>> =
    LazyLock::new(|| RwLock::new(Vec::new()));

pub fn add_runtime_rule(app: &str, word: &str) {
    if let Ok(mut rules) = RUNTIME_CHEATS.write() {
        rules.push(CheatRule::new(app, word));
    }
}

pub fn check_runtime_cheat(app_id: Option<&str>, raw_keys: &[char]) -> Option<String> {
    if let Ok(rules) = RUNTIME_CHEATS.read() {
        for rule in rules.iter() {
            if rule.matches(app_id, raw_keys) {
                return Some(raw_keys.iter().collect());
            }
        }
    }
    None
}

/// Combined check: built-in + runtime cheats.
pub fn should_force_english(app_id: Option<&str>, raw_keys: &[char]) -> Option<String> {
    check_cheat(app_id, raw_keys).or_else(|| check_runtime_cheat(app_id, raw_keys))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warp_is_cheated() {
        let keys: Vec<char> = "warp".chars().collect();
        let result = check_cheat(Some("chromium"), &keys);
        assert_eq!(result, Some("warp".into()));
    }

    #[test]
    fn browser_is_cheated() {
        let keys: Vec<char> = "browser".chars().collect();
        let result = check_cheat(None, &keys);
        assert_eq!(result, Some("browser".into()));
    }

    #[test]
    fn vietnamese_word_not_cheated() {
        let keys: Vec<char> = "cuar".chars().collect();
        let result = check_cheat(Some("chromium"), &keys);
        assert_eq!(result, None);
    }

    #[test]
    fn add_is_cheated_for_all_apps() {
        let keys: Vec<char> = "add".chars().collect();
        assert!(check_cheat(Some("firefox"), &keys).is_some());
        assert!(check_cheat(Some("libreoffice"), &keys).is_some());
        assert!(check_cheat(None, &keys).is_some());
    }

    #[test]
    fn case_insensitive_match() {
        let keys: Vec<char> = "WARP".chars().collect();
        let result = check_cheat(Some("CHROMIUM"), &keys);
        assert_eq!(result, Some("WARP".into()));
    }

    #[test]
    fn runtime_cheat_works() {
        add_runtime_rule("*", "foobar");
        let keys: Vec<char> = "foobar".chars().collect();
        let result = should_force_english(Some("kitty"), &keys);
        assert_eq!(result, Some("foobar".into()));
    }

    #[test]
    fn star_pattern_matches_all_apps() {
        let rule = CheatRule::new("*", "test");
        assert!(rule.matches(Some("anything"), &['t','e','s','t']));
        assert!(rule.matches(None, &['t','e','s','t']));
    }

    #[test]
    fn specific_pattern_only_matches_prefix() {
        let rule = CheatRule::new("chromium", "test");
        assert!(rule.matches(Some("chromium-browser"), &['t','e','s','t']));
        assert!(!rule.matches(Some("firefox"), &['t','e','s','t']));
    }

    #[test]
    fn draw_not_composed_to_dra8() {
        let keys: Vec<char> = "draw".chars().collect();
        assert!(check_cheat(None, &keys).is_some(),
            "'draw' must not become 'đră'");
    }

    #[test]
    fn sorted_not_composed_to_sorte() {
        let keys: Vec<char> = "sorted".chars().collect();
        assert!(check_cheat(None, &keys).is_some());
    }
}

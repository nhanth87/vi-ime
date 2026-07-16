// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Unit tests for text-input-v3.2 content_hint flags.

#[cfg(test)]
mod hint_v3_tests {
    use crate::wayland::state::ContentHintV3;

    // ── Default state ──────────────────────────────────────────────────────

    #[test]
    fn default_all_false() {
        let h = ContentHintV3::default();
        assert!(!h.on_screen_input);
        assert!(!h.no_emoji);
        assert!(!h.preedit_shown);
    }

    // ── Raw hint decoding (0x400, 0x800, 0x1000) ───────────────────────────

    #[test]
    fn raw_hint_no_emoji_only() {
        let raw = 0x800u32;
        let h = ContentHintV3 {
            on_screen_input: raw & 0x400 != 0,
            no_emoji: raw & 0x800 != 0,
            preedit_shown: raw & 0x1000 != 0,
        };
        assert!(!h.on_screen_input);
        assert!(h.no_emoji);
        assert!(!h.preedit_shown);
    }

    #[test]
    fn raw_hint_preedit_shown_only() {
        let raw = 0x1000u32;
        let h = ContentHintV3 {
            on_screen_input: raw & 0x400 != 0,
            no_emoji: raw & 0x800 != 0,
            preedit_shown: raw & 0x1000 != 0,
        };
        assert!(!h.on_screen_input);
        assert!(!h.no_emoji);
        assert!(h.preedit_shown);
    }

    #[test]
    fn raw_hint_on_screen_input_only() {
        let raw = 0x400u32;
        let h = ContentHintV3 {
            on_screen_input: raw & 0x400 != 0,
            no_emoji: raw & 0x800 != 0,
            preedit_shown: raw & 0x1000 != 0,
        };
        assert!(h.on_screen_input);
        assert!(!h.no_emoji);
        assert!(!h.preedit_shown);
    }

    // ── Combined flags ─────────────────────────────────────────────────────

    #[test]
    fn raw_hint_all_three() {
        let raw = 0x400 | 0x800 | 0x1000;
        let h = ContentHintV3 {
            on_screen_input: raw & 0x400 != 0,
            no_emoji: raw & 0x800 != 0,
            preedit_shown: raw & 0x1000 != 0,
        };
        assert!(h.on_screen_input);
        assert!(h.no_emoji);
        assert!(h.preedit_shown);
    }

    #[test]
    fn raw_hint_no_emoji_with_legacy() {
        // no_emoji (0x800) combined with v3.0 base hints like spellcheck (0x2), latin (0x100)
        let raw = 0x800u32 | 0x2 | 0x100;
        let h = ContentHintV3 {
            on_screen_input: raw & 0x400 != 0,
            no_emoji: raw & 0x800 != 0,
            preedit_shown: raw & 0x1000 != 0,
        };
        assert!(!h.on_screen_input);
        assert!(h.no_emoji);
        assert!(!h.preedit_shown);
    }

    // ── Zero / unknown compositor ──────────────────────────────────────────

    #[test]
    fn raw_hint_zero_all_default() {
        let raw = 0u32;
        let h = ContentHintV3 {
            on_screen_input: raw & 0x400 != 0,
            no_emoji: raw & 0x800 != 0,
            preedit_shown: raw & 0x1000 != 0,
        };
        assert!(!h.on_screen_input);
        assert!(!h.no_emoji);
        assert!(!h.preedit_shown);
    }

    // ── Non-v3.2 bits don't interfere ──────────────────────────────────────

    #[test]
    fn raw_hint_legacy_bits_ignored() {
        // v3.0 base hints: completion(0x1) | auto_capitalization(0x4) | hidden_text(0x40) | multiline(0x200)
        let raw = 0x1u32 | 0x4 | 0x40 | 0x200;
        let h = ContentHintV3 {
            on_screen_input: raw & 0x400 != 0,
            no_emoji: raw & 0x800 != 0,
            preedit_shown: raw & 0x1000 != 0,
        };
        assert!(!h.on_screen_input);
        assert!(!h.no_emoji);
        assert!(!h.preedit_shown);
    }

    // ── Copy / Clone ───────────────────────────────────────────────────────

    #[test]
    fn clone_roundtrip() {
        let h = ContentHintV3 {
            on_screen_input: true,
            no_emoji: false,
            preedit_shown: true,
        };
        let c = h.clone();
        assert_eq!(h.on_screen_input, c.on_screen_input);
        assert_eq!(h.no_emoji, c.no_emoji);
        assert_eq!(h.preedit_shown, c.preedit_shown);
    }

    // ── Equality ───────────────────────────────────────────────────────────

    #[test]
    fn equal_when_same() {
        let a = ContentHintV3 {
            on_screen_input: false,
            no_emoji: true,
            preedit_shown: false,
        };
        let b = ContentHintV3 {
            on_screen_input: false,
            no_emoji: true,
            preedit_shown: false,
        };
        assert!(a.no_emoji == b.no_emoji);
        assert!(a.on_screen_input == b.on_screen_input);
        assert!(a.preedit_shown == b.preedit_shown);
    }

    #[test]
    fn not_equal_when_different() {
        let a = ContentHintV3 {
            on_screen_input: false,
            no_emoji: true,
            preedit_shown: false,
        };
        let b = ContentHintV3 {
            on_screen_input: false,
            no_emoji: false,
            preedit_shown: false,
        };
        assert!(a.no_emoji != b.no_emoji);
    }
}

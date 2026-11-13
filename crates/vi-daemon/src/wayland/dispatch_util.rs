// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Wayland dispatch helpers (split from dispatch.rs, R4): field-sensitivity
//! mapping, own-keymap detection, and the blink/drop helpers.

use wayland_protocols::wp::text_input::zv3::client::zwp_text_input_v3::ContentPurpose;

use crate::wayland::state::{FieldSensitivity, ImeAppState};

/// ⚠️ Chrome/Firefox depend on this: `ContentPurpose::Url` → `FieldSensitivity::Url`
/// is what makes the address bar type raw ASCII instead of composing
/// Vietnamese ("tự chuyển tiếng Anh khi gõ trong address bar"). Verified
/// working 2026-07-10 against Chrome — do NOT touch this mapping without
/// re-testing an actual browser address bar, since the browser's own
/// text-input-v3 behavior here has historically been inconsistent across
/// versions and this is the ONLY place that consumes `ContentPurpose::Url`.
/// Map the app's self-declared field purpose to our per-field gate.
/// Terminal gets its own variant to force NonPreedit mode (preedit-everywhere
/// commit_string works, but preedit underline breaks in terminals).
pub(crate) fn sensitivity_of(purpose: ContentPurpose) -> FieldSensitivity {
    match purpose {
        // Security-critical: engine off, never logged. Non-negotiable.
        ContentPurpose::Password | ContentPurpose::Pin => FieldSensitivity::Secure,
        // URL fields (address bar): compose as preedit, no live BackSpaces
        // (browser autocomplete stays intact) — state.rs `live_echo`.
        ContentPurpose::Url => FieldSensitivity::Url,
        // Terminal: force NonPreedit mode (unless user explicitly chose otherwise).
        ContentPurpose::Terminal => FieldSensitivity::Terminal,
        // Digits, Number etc. → NumericRaw: never composed, logged, or
        // learned (R38). An OTP/PIN field declares purpose=Digits, and a
        // one-time code is a secret even though it looks like digits.
        ContentPurpose::Digits
        | ContentPurpose::Number
        | ContentPurpose::Phone
        | ContentPurpose::Date
        | ContentPurpose::Time
        | ContentPurpose::Datetime => FieldSensitivity::NumericRaw,
        _ => FieldSensitivity::Normal,
    }
}

/// Does this keymap come from one of vi-ime's own virtual keyboards? Their
/// keymaps (viet_typer.rs) all name the keycodes section "vi"; libxkbcommon
/// keeps that name when a compositor re-serializes the keymap.
pub(crate) fn keymap_is_ours(fd: &std::os::fd::OwnedFd, size: u32) -> bool {
    use std::os::unix::fs::FileExt;
    let Ok(dup) = fd.try_clone() else { return false };
    let file = std::fs::File::from(dup);
    let mut buf = vec![0u8; (size as usize).min(4096)];
    let Ok(n) = file.read_at(&mut buf, 0) else { return false };
    String::from_utf8_lossy(&buf[..n]).contains("xkb_keycodes \"vi\"")
}

/// A deactivate+activate faster than this, in the same app with no click,
/// is a page re-rendering its field, not the user leaving it.
pub(crate) const BLINK_MS: u128 = 150;

/// Forget the word being composed (R8: dropped, never committed).
pub(crate) fn drop_word(state: &mut ImeAppState) {
    state.engine.reset();
    state.engine.inner_mut().reset_context();
    crate::engine::predict::reset_context();
    state.reset_word_state();
}

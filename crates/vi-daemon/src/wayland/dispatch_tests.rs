// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Unit tests for dispatch field sensitivity (R38): secret-capable
//! purposes must never land in the Normal (compose/log/learn) class.

use super::*;
use wayland_protocols::wp::text_input::zv3::client::zwp_text_input_v3::ContentPurpose;

#[test]
fn secrets_are_not_normal() {
    // Passwords and PINs: engine off, nothing logged or learned.
    assert_eq!(sensitivity_of(ContentPurpose::Password), FieldSensitivity::Secure);
    assert_eq!(sensitivity_of(ContentPurpose::Pin), FieldSensitivity::Secure);
    // Numeric purposes (OTP codes are secrets): raw passthrough, R38.
    for p in [
        ContentPurpose::Digits,
        ContentPurpose::Number,
        ContentPurpose::Phone,
        ContentPurpose::Date,
        ContentPurpose::Time,
        ContentPurpose::Datetime,
    ] {
        assert_eq!(sensitivity_of(p), FieldSensitivity::NumericRaw, "{p:?}");
    }
}

#[test]
fn text_fields_stay_composable() {
    assert_eq!(sensitivity_of(ContentPurpose::Normal), FieldSensitivity::Normal);
    assert_eq!(sensitivity_of(ContentPurpose::Url), FieldSensitivity::Url);
    assert_eq!(sensitivity_of(ContentPurpose::Terminal), FieldSensitivity::Terminal);
    // Free-form text (names, search,…) composes as before.
    assert_eq!(sensitivity_of(ContentPurpose::Name), FieldSensitivity::Normal);
}

// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! `type_out` barrier tests for evdev_compose (split out, R4b).

use super::*;

#[test]
fn no_recent_press_no_wait() {
    assert_eq!(settle_remaining(None, Instant::now()), Duration::ZERO);
}

#[test]
fn fresh_press_waits_the_remainder() {
    let t0 = Instant::now();
    let left = settle_remaining(Some(t0), t0 + Duration::from_millis(10));
    assert_eq!(left, UINPUT_SETTLE - Duration::from_millis(10));
}

#[test]
fn old_press_no_wait() {
    let t0 = Instant::now();
    assert_eq!(settle_remaining(Some(t0), t0 + UINPUT_SETTLE * 2), Duration::ZERO);
}

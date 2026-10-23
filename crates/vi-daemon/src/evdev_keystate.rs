// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Held-key tracking for the evdev fallback's pre-grab wait (split from
//! evdev_mode.rs, R4).
//!
//! `wait_keys_clear` refuses to grab while a key is down (grabbing would
//! swallow its release → stuck key in the compositor). Field bug 2026-10-23
//! (Hyprland, laptop "AT Translated Set 2 keyboard"): the KERNEL reported
//! `KEY_KP0` held for days — a lost release, no finger on the key. The wait
//! never finished, every engage ended in "no grabbable keyboard" and Chrome
//! X11 got zero Vietnamese. A non-modifier key held continuously for
//! `STALE_KEY` is now treated as stale kernel state and ignored.
//!
//! Modifiers are NEVER treated as stale: holding Super while moving between
//! windows is normal and a swallowed Super release is exactly the stuck-Super
//! bug the wait exists for (field 2026-07-10).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use evdev::KeyCode;

/// How long a non-modifier key must stay down before it counts as stale.
/// Long on purpose: a real held key (Backspace repeat) grabbed mid-hold
/// loses its release to the grab. 3s of continuous hold during a focus
/// change is not human typing.
pub(crate) const STALE_KEY: Duration = Duration::from_secs(3);

/// Keys already confirmed stale per device path, so the NEXT engage on the
/// same stuck key is instant instead of re-paying `STALE_KEY` on every focus
/// change into an XWayland app.
static KNOWN_STALE: Mutex<Vec<(String, KeyCode)>> = Mutex::new(Vec::new());

/// Ctrl/Alt/Super/Shift — keys that must always be waited out.
pub(crate) fn is_modifier(code: KeyCode) -> bool {
    matches!(
        code,
        KeyCode::KEY_LEFTCTRL
            | KeyCode::KEY_RIGHTCTRL
            | KeyCode::KEY_LEFTALT
            | KeyCode::KEY_RIGHTALT
            | KeyCode::KEY_LEFTMETA
            | KeyCode::KEY_RIGHTMETA
            | KeyCode::KEY_LEFTSHIFT
            | KeyCode::KEY_RIGHTSHIFT
    )
}

/// Continuous-hold clock for each key seen down during one wait.
pub(crate) struct HeldKeys {
    since: Vec<(KeyCode, Instant)>,
    known: Vec<KeyCode>,
}

impl HeldKeys {
    /// `known`: keys confirmed stale on this device by an earlier wait —
    /// they start already past `STALE_KEY`.
    pub(crate) fn new(known: Vec<KeyCode>) -> Self {
        Self { since: Vec::new(), known }
    }

    /// Feed one key-state snapshot. Returns true when the grab may proceed:
    /// nothing held, or only non-modifier keys held for ≥ `STALE_KEY`.
    pub(crate) fn update(&mut self, held: &[KeyCode], now: Instant) -> bool {
        self.since.retain(|(k, _)| held.contains(k));
        for &k in held {
            if !self.since.iter().any(|(c, _)| *c == k) {
                let start = if self.known.contains(&k) {
                    now.checked_sub(STALE_KEY).unwrap_or(now)
                } else {
                    now
                };
                self.since.push((k, start));
            }
        }
        self.since
            .iter()
            .all(|&(k, t)| !is_modifier(k) && now.duration_since(t) >= STALE_KEY)
    }

    /// Keys still held when `update` returned true — the stale ones.
    pub(crate) fn stale(&self) -> Vec<KeyCode> {
        self.since.iter().map(|(k, _)| *k).collect()
    }
}

/// Stale keys remembered for `device` (empty when none).
pub(crate) fn known_stale(device: &str) -> Vec<KeyCode> {
    KNOWN_STALE
        .lock()
        .map(|v| v.iter().filter(|(d, _)| d == device).map(|(_, k)| *k).collect())
        .unwrap_or_default()
}

/// Replace the remembered stale keys for `device`.
pub(crate) fn remember_stale(device: &str, keys: &[KeyCode]) {
    if let Ok(mut v) = KNOWN_STALE.lock() {
        v.retain(|(d, _)| d != device);
        v.extend(keys.iter().map(|&k| (device.to_string(), k)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_held_proceeds() {
        let mut h = HeldKeys::new(Vec::new());
        assert!(h.update(&[], Instant::now()));
        assert!(h.stale().is_empty());
    }

    #[test]
    fn fresh_key_blocks_until_stale() {
        let t0 = Instant::now();
        let mut h = HeldKeys::new(Vec::new());
        assert!(!h.update(&[KeyCode::KEY_KP0], t0));
        assert!(!h.update(&[KeyCode::KEY_KP0], t0 + STALE_KEY / 2));
        assert!(h.update(&[KeyCode::KEY_KP0], t0 + STALE_KEY));
        assert_eq!(h.stale(), vec![KeyCode::KEY_KP0]);
    }

    #[test]
    fn release_resets_the_clock() {
        let t0 = Instant::now();
        let mut h = HeldKeys::new(Vec::new());
        assert!(!h.update(&[KeyCode::KEY_A], t0));
        assert!(h.update(&[], t0 + Duration::from_millis(20)));
        assert!(!h.update(&[KeyCode::KEY_A], t0 + STALE_KEY));
    }

    #[test]
    fn modifier_never_stale() {
        let t0 = Instant::now();
        let mut h = HeldKeys::new(vec![KeyCode::KEY_LEFTMETA]);
        assert!(!h.update(&[KeyCode::KEY_LEFTMETA], t0 + STALE_KEY * 10));
    }

    #[test]
    fn modifier_blocks_alongside_stale_key() {
        let t0 = Instant::now();
        let mut h = HeldKeys::new(vec![KeyCode::KEY_KP0]);
        assert!(!h.update(&[KeyCode::KEY_KP0, KeyCode::KEY_LEFTCTRL], t0));
        assert!(h.update(&[KeyCode::KEY_KP0], t0 + Duration::from_millis(20)));
    }

    #[test]
    fn known_stale_key_is_instant() {
        let mut h = HeldKeys::new(vec![KeyCode::KEY_KP0]);
        assert!(h.update(&[KeyCode::KEY_KP0], Instant::now()));
    }

    #[test]
    fn remember_and_recall_per_device() {
        remember_stale("/dev/input/test-a", &[KeyCode::KEY_KP0]);
        remember_stale("/dev/input/test-b", &[KeyCode::KEY_KP1]);
        assert_eq!(known_stale("/dev/input/test-a"), vec![KeyCode::KEY_KP0]);
        remember_stale("/dev/input/test-a", &[]);
        assert!(known_stale("/dev/input/test-a").is_empty());
        assert_eq!(known_stale("/dev/input/test-b"), vec![KeyCode::KEY_KP1]);
    }

    #[test]
    fn is_modifier_covers_shift_ctrl_alt_super() {
        assert!(is_modifier(KeyCode::KEY_RIGHTSHIFT));
        assert!(is_modifier(KeyCode::KEY_LEFTALT));
        assert!(!is_modifier(KeyCode::KEY_KP0));
    }
}

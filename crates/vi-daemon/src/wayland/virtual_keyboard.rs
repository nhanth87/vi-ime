// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 vi-im contributors
//! Key forwarding via zwp_virtual_keyboard_v1.
//!
//! The input-method-v2 keyboard grab swallows EVERY key; the compositor
//! never delivers grabbed keys to the app. Any key the IME does not turn
//! into committed text must be re-injected here, or it is lost (arrows,
//! Ctrl+C, Enter, the raw letters of NonPreedit words, ...).
//!
//! The forwarder mirrors the grab's keymap and modifier state onto the
//! virtual keyboard so the app decodes forwarded keycodes identically.

use std::collections::HashSet;
use std::os::unix::io::{AsFd, OwnedFd};
use std::time::Instant;

use tracing::{info, warn};
use wayland_client::protocol::wl_keyboard::KeymapFormat;
use wayland_client::WEnum;
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1;

/// Key state values per wl_keyboard (virtual-keyboard takes raw u32).
const KEY_RELEASED: u32 = 0;
const KEY_PRESSED: u32 = 1;

pub(crate) struct VkForwarder {
    vk: Option<ZwpVirtualKeyboardV1>,
    /// Protocol requires a keymap before any key event.
    keymap_ready: bool,
    /// Keycodes whose press we forwarded — their release must follow,
    /// everything else's release is swallowed with its press.
    held: HashSet<u32>,
    /// Fallback timestamp source for synthesized events.
    start: Instant,
}

impl VkForwarder {
    pub(crate) fn new(vk: Option<ZwpVirtualKeyboardV1>) -> Self {
        if vk.is_none() {
            warn!(
                "zwp_virtual_keyboard_manager_v1 unavailable — \
                 unhandled keys CANNOT be forwarded to apps"
            );
        }
        Self {
            vk,
            keymap_ready: false,
            held: HashSet::new(),
            start: Instant::now(),
        }
    }

    /// Mirror the grab's keymap. Borrows the fd — caller keeps ownership.
    pub(crate) fn set_keymap(
        &mut self,
        format: WEnum<KeymapFormat>,
        fd: &OwnedFd,
        size: u32,
    ) {
        let Some(vk) = &self.vk else { return };
        let WEnum::Value(fmt) = format else { return };
        vk.keymap(fmt as u32, fd.as_fd(), size);
        self.keymap_ready = true;
        info!("virtual keyboard keymap mirrored ({size} bytes)");
    }

    /// Mirror the grab's modifier state so forwarded keys decode correctly.
    pub(crate) fn modifiers(
        &self,
        depressed: u32,
        latched: u32,
        locked: u32,
        group: u32,
    ) {
        if let Some(vk) = &self.vk
            && self.keymap_ready {
                vk.modifiers(depressed, latched, locked, group);
            }
    }

    fn now_ms(&self) -> u32 {
        self.start.elapsed().as_millis() as u32
    }

    /// Forward a key press; its real release will be forwarded too.
    /// Holding the key repeats app-side (apps do their own wl_keyboard repeat).
    pub(crate) fn press(&mut self, keycode: u32) {
        let Some(vk) = &self.vk else { return };
        if !self.keymap_ready {
            return;
        }
        vk.key(self.now_ms(), keycode, KEY_PRESSED);
        self.held.insert(keycode);
    }

    /// Forward the release of a previously forwarded press.
    /// Returns true if this release belonged to a forwarded key.
    pub(crate) fn release(&mut self, keycode: u32) -> bool {
        if !self.held.remove(&keycode) {
            return false;
        }
        if let Some(vk) = &self.vk
            && self.keymap_ready {
                vk.key(self.now_ms(), keycode, KEY_RELEASED);
            }
        true
    }

    /// Synthesize a full press+release (for keys replayed after a commit
    /// sequence — the real release was already swallowed or is untracked).
    pub(crate) fn tap(&mut self, keycode: u32) {
        let Some(vk) = &self.vk else { return };
        if !self.keymap_ready {
            return;
        }
        let t = self.now_ms();
        vk.key(t, keycode, KEY_PRESSED);
        vk.key(t.wrapping_add(1), keycode, KEY_RELEASED);
        self.held.remove(&keycode);
    }

    /// Release everything still held (grab teardown / deactivate) so the
    /// app never sees a stuck key.
    pub(crate) fn release_all(&mut self) {
        let keys: Vec<u32> = self.held.drain().collect();
        if let Some(vk) = &self.vk
            && self.keymap_ready {
                for kc in keys {
                    vk.key(self.now_ms(), kc, KEY_RELEASED);
                }
            }
    }
}

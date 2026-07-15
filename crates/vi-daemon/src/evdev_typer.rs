// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Persistent virtual-keyboard typer for the evdev fallback.
//!
//! Replaces the wtype-per-keystroke design (field failure 2026-07-10,
//! LibreOffice "mèo"→"mèe"): every wtype invocation created a BRAND NEW
//! virtual keyboard with a keymap whose keysym→keycode assignment depends
//! on that call's text, so the seat keymap flapped real↔wtype on every
//! keystroke and a lagging client rendered the wrong glyph entirely.
//! Also paid a process spawn + Wayland connect per keystroke.
//!
//! Here: ONE `zwp_virtual_keyboard_v1` for the whole grab, on the daemon's
//! own connection. Each sync uploads a tiny keymap (BackSpace + the suffix
//! chars, proven-safe typing-row keycodes 2..=33 — same scheme as
//! `wayland/viet_typer.rs`, whose keymap builder is reused) and taps keys
//! on the SAME object, so keymap-before-keys ordering is guaranteed by the
//! protocol. `backspace_then_type` ends with a roundtrip so everything is
//! compositor-processed BEFORE any later uinput-mirror event (cross-channel
//! order can't invert).

use std::os::fd::AsFd;
use std::time::Instant;

use tracing::warn;
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::wl_registry;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1,
    zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
};

use std::collections::HashMap;

use crate::client_profile::ClientProfile;
use crate::wayland::viet_typer::{
    build_static_keymap, memfd_keymap, KEYMAP_FORMAT_XKB_V1, LEVEL_MASKS,
};

/// Event sink for the typer's private connection — nothing to handle.
struct TyperState;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for TyperState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlSeat, ()> for TyperState {
    fn event(
        _state: &mut Self,
        _proxy: &WlSeat,
        _event: <WlSeat as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpVirtualKeyboardManagerV1, ()> for TyperState {
    fn event(
        _state: &mut Self,
        _proxy: &ZwpVirtualKeyboardManagerV1,
        _event: <ZwpVirtualKeyboardManagerV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpVirtualKeyboardV1, ()> for TyperState {
    fn event(
        _state: &mut Self,
        _proxy: &ZwpVirtualKeyboardV1,
        _event: <ZwpVirtualKeyboardV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
    }
}

/// One long-lived virtual keyboard on a private Wayland connection.
///
/// **STATIC keymap (2026-07-12):** the previous design rebuilt a per-word
/// keymap and re-uploaded it on EVERY `backspace_then_type` call. That is the
/// exact "keymap động" anti-pattern that fails on lag-prone clients (R16/R17):
/// LibreOffice/VCL applies `wl_keyboard.keymap` a beat late, so a freshly
/// mapped diacritic keycode decoded against the OLD keymap and vanished —
/// field bug "dân"→"dan", every tone/quality mark dropped. Now it uploads the
/// SAME static 8-level keymap as the Wayland `VietTyper` ONCE at creation and
/// selects the glyph by modifier level per tap (Shift/Mod3/Mod5), so the
/// keymap never changes after the first upload and there is nothing to lag on.
pub(crate) struct EvdevTyper {
    queue: EventQueue<TyperState>,
    vk: ZwpVirtualKeyboardV1,
    start: Instant,
    /// Static char → (keycode, level), built once with the keymap.
    map: HashMap<char, (u32, u8)>,
    /// Modifier mask currently depressed (level selector) — mirrors VietTyper.
    cur_mask: u32,
    /// Per-client pacing profile (adaptive delays from ClientProfile).
    profile: ClientProfile,
}

impl EvdevTyper {
    /// None when there is no Wayland display or the compositor lacks
    /// `zwp_virtual_keyboard_manager_v1` (caller falls back to xdotool).
    pub(crate) fn new(profile: ClientProfile) -> Option<Self> {
        let conn = Connection::connect_to_env().ok()?;
        let (globals, mut queue) = registry_queue_init::<TyperState>(&conn).ok()?;
        let qh = queue.handle();
        let seat: WlSeat = globals.bind(&qh, 1..=9, ()).ok()?;
        let mgr: ZwpVirtualKeyboardManagerV1 = globals.bind(&qh, 1..=1, ()).ok()?;
        let vk = mgr.create_virtual_keyboard(&seat, &qh, ());
        // Upload the ONE static keymap immediately (before any typing), so a
        // lagging client applies it long before the first word — never per
        // word (the old flapping-keymap bug). Same builder as VietTyper.
        let (text, map) = build_static_keymap();
        let (fd, size) = memfd_keymap(&text)?;
        vk.keymap(KEYMAP_FORMAT_XKB_V1, fd.as_fd(), size);
        queue.roundtrip(&mut TyperState).ok()?;
        Some(Self { queue, vk, start: Instant::now(), map, cur_mask: 0, profile })
    }

    /// BackSpace × n, then type `text`. All-or-nothing: false = nothing sent.
    ///
    /// `sync`: when true, block on a roundtrip so cross-channel uinput events
    /// (space, Enter, boundary-key replay) can never overtake this text.
    /// Mid-word composition passes `false` (flush only — next keystroke
    /// goes through the same channel so ordering is protocol-guaranteed).
    /// Word boundary passes `true` (roundtrip → uinput safe to emit next).
    pub(crate) fn backspace_then_type(
        &mut self,
        backspaces: usize,
        text: &str,
        sync: bool,
    ) -> bool {
        if backspaces == 0 && text.is_empty() {
            return true;
        }
        if self.map.is_empty() {
            return false;
        }
        // All-or-nothing: verify coverage BEFORE sending anything (a char
        // outside the static keymap — e.g. an emoji glyph — can't be typed on
        // this native path; caller logs and the word stays as-is).
        if let Some(bad) = text.chars().find(|c| !self.map.contains_key(c)) {
            warn!("[EVDEV-TYPER] ký tự ngoài bảng tĩnh: {bad:?} — không gõ được");
            return false;
        }
        let (bs_code, bs_level) = self.map[&'\u{0008}'];

        let mut t = self.start.elapsed().as_millis() as u32;
        let mut mask_now = self.cur_mask;
        // Select the char's level via depressed modifiers, then tap — same
        // object, so keymap-before-keys and key order are protocol-guaranteed
        // (identical scheme to VietTyper::backspace_then_type).
        let tap = |vk: &ZwpVirtualKeyboardV1, code: u32, level: u8, mask_now: &mut u32, t: &mut u32| {
            let want = LEVEL_MASKS[level as usize];
            if *mask_now != want {
                vk.modifiers(want, 0, 0, 0);
                *mask_now = want;
            }
            vk.key(*t, code, 1);
            vk.key(t.wrapping_add(1), code, 0);
            *t = t.wrapping_add(2);
        };
        if backspaces > 1 && self.profile.batch_safe {
            // Batch-safe apps (Chromium/XWayland, Firefox/Wayland, terminals,
            // default): fire all BackSpace taps back-to-back without per-BS
            // roundtrip, then confirm the whole batch with ONE roundtrip +
            // ONE batch_delay before any composed glyphs arrive.
            for _ in 0..backspaces {
                tap(&self.vk, bs_code, bs_level, &mut mask_now, &mut t);
            }
            let _ = self.queue.roundtrip(&mut TyperState);
            if self.profile.batch_delay_ms > 0 {
                std::thread::sleep(std::time::Duration::from_millis(
                    self.profile.batch_delay_ms,
                ));
            }
        } else {
            // Non-batch-safe apps (LibreOffice VCL, OnlyOffice CEF — swallows
            // BS+char bursts whole, probe-verified 2026-07-10) and the single-BS
            // case: per-BS roundtrip + sleep so each BackSpace has its own beat
            // before anything follows.
            for _ in 0..backspaces {
                tap(&self.vk, bs_code, bs_level, &mut mask_now, &mut t);
                let _ = self.queue.roundtrip(&mut TyperState);
                std::thread::sleep(std::time::Duration::from_millis(
                    self.profile.backspace_delay_ms,
                ));
            }
        }
        // Adaptive settle: apps known to drop the first glyph after BackSpace
        // (LibreOffice VCL, OnlyOffice CEF) get extra time before the first
        // composed character arrives (pre_first_glyph_delay_ms from ClientProfile).
        if backspaces > 0 && self.profile.pre_first_glyph_delay_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(self.profile.pre_first_glyph_delay_ms));
        }
        for ch in text.chars() {
            let (code, level) = self.map[&ch];
            tap(&self.vk, code, level, &mut mask_now, &mut t);
            // Pace EVERY tap, not just BackSpace: VCL still lost the
            // char after the first in a post-BS burst (field
            // "cua73"→"cưử", 2026-07-10). This typer only ever targets
            // legacy apps (LibreOffice/OnlyOffice), so always pace.
            let _ = self.queue.flush();
            std::thread::sleep(std::time::Duration::from_millis(self.profile.glyph_delay_ms));
        }
        // Never leave synthetic modifiers depressed on the seat.
        if mask_now != 0 {
            self.vk.modifiers(0, 0, 0, 0);
            mask_now = 0;
        }
        self.cur_mask = mask_now;
        // sync=true (word boundary): block until compositor has processed
        // everything — cross-channel uinput events must not overtake this.
        // sync=false (mid-word): flush is enough; next keystroke rides the
        // same channel so ordering is protocol-guaranteed (no roundtrip =
        // ~80% latency reduction per keystroke).
        if sync {
            self.queue.roundtrip(&mut TyperState).is_ok()
        } else {
            self.queue.flush().is_ok()
        }
    }
}

// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 vi-im contributors
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

use crate::wayland::viet_typer::{
    build_keymap, memfd_keymap, FIRST_CODE, KEYMAP_FORMAT_XKB_V1, MAX_UNIQUE,
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
pub(crate) struct EvdevTyper {
    queue: EventQueue<TyperState>,
    vk: ZwpVirtualKeyboardV1,
    start: Instant,
}

impl EvdevTyper {
    /// None when there is no Wayland display or the compositor lacks
    /// `zwp_virtual_keyboard_manager_v1` (caller falls back to xdotool).
    pub(crate) fn new() -> Option<Self> {
        let conn = Connection::connect_to_env().ok()?;
        let (globals, mut queue) = registry_queue_init::<TyperState>(&conn).ok()?;
        let qh = queue.handle();
        let seat: WlSeat = globals.bind(&qh, 1..=9, ()).ok()?;
        let mgr: ZwpVirtualKeyboardManagerV1 = globals.bind(&qh, 1..=1, ()).ok()?;
        let vk = mgr.create_virtual_keyboard(&seat, &qh, ());
        queue.roundtrip(&mut TyperState).ok()?;
        Some(Self { queue, vk, start: Instant::now() })
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
        // Assign safe keycodes: BackSpace first (if needed), suffix after.
        let mut assigned: Vec<(char, u32)> = Vec::new();
        if backspaces > 0 {
            assigned.push(('\u{0008}', FIRST_CODE));
        }
        for ch in text.chars() {
            if assigned.iter().any(|(c, _)| *c == ch) {
                continue;
            }
            if assigned.len() >= MAX_UNIQUE {
                warn!("[EVDEV-TYPER] >{MAX_UNIQUE} ký tự khác nhau trong một lần gõ — bỏ qua");
                return false;
            }
            assigned.push((ch, FIRST_CODE + assigned.len() as u32));
        }

        let keymap = build_keymap(&assigned);
        let Some((fd, size)) = memfd_keymap(&keymap) else {
            warn!("[EVDEV-TYPER] memfd failed — không gõ được");
            return false;
        };
        self.vk.keymap(KEYMAP_FORMAT_XKB_V1, fd.as_fd(), size);
        // Keymap-apply beat (cùng lớp lỗi repro 2026-07-10 ở viet_typer):
        // client áp keymap trễ một nhịp → tap đầu tiên (khi không có BS đi
        // trước) giải mã theo keymap cũ và biến mất. Typer này chỉ nhắm
        // legacy app (VCL/Qt-XWayland) nên luôn pace.
        let _ = self.queue.flush();
        std::thread::sleep(std::time::Duration::from_millis(15));

        let mut t = self.start.elapsed().as_millis() as u32;
        let mut tap = |vk: &ZwpVirtualKeyboardV1, code: u32| {
            vk.key(t, code, 1);
            vk.key(t.wrapping_add(1), code, 0);
            t = t.wrapping_add(2);
        };
        for _ in 0..backspaces {
            tap(&self.vk, FIRST_CODE);
            // VCL/gtk3 (LibreOffice) swallows an event burst that mixes
            // BackSpace with other keys WHOLE — probe-verified 2026-07-10:
            // BS+"ệ" flushed together typed NOTHING (both keys dropped,
            // monotonic timestamps don't help), while BS alone, multi-char
            // bursts, and BS→15ms pause→chars all work. So each BackSpace
            // is flushed and given its own beat before anything follows.
            let _ = self.queue.roundtrip(&mut TyperState);
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
        for ch in text.chars() {
            if let Some((_, code)) = assigned.iter().find(|(c, _)| *c == ch) {
                tap(&self.vk, *code);
                // Pace EVERY tap, not just BackSpace: VCL still lost the
                // char after the first in a post-BS burst (field
                // "cua73"→"cưử", 2026-07-10). This typer only ever targets
                // legacy apps (LibreOffice/OnlyOffice), so always pace.
                let _ = self.queue.flush();
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
        }
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

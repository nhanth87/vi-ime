// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 vi-im contributors
//! Compositor-agnostic focus tracking via zwlr_foreign_toplevel_management_v1.
//!
//! Every wlroots-based compositor (Sway, Hyprland, river, labwc, Wayfire…)
//! exposes this protocol: app_id + title + activated state of all toplevels,
//! event-driven over a plain Wayland connection — no per-compositor CLI, no
//! polling (R15). Niri keeps its own IPC path (it also provides the PID,
//! which this protocol does not).
//!
//! The stream thread blocks on the Wayland socket its whole life and sends a
//! `FocusEvent` whenever the activated toplevel (or its title) changes.

use std::collections::HashMap;
use std::sync::mpsc::Sender;

use tracing::{info, warn};
use wayland_client::backend::ObjectId;
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::wl_registry;
use wayland_client::{event_created_child, Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1},
    zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
};

use crate::compositor::FocusEvent;

const MANAGER_INTERFACE: &str = "zwlr_foreign_toplevel_manager_v1";
/// `state` array entry meaning the toplevel has keyboard focus.
const STATE_ACTIVATED: u32 = 2;

/// Decode the protocol's `state` array (native-endian u32s) and report
/// whether it contains `activated`. Pure — unit-testable.
pub(crate) fn is_activated(bytes: &[u8]) -> bool {
    bytes
        .chunks_exact(4)
        .map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
        .any(|s| s == STATE_ACTIVATED)
}

#[derive(Default)]
struct Toplevel {
    app_id: Option<String>,
    title: Option<String>,
    /// Double-buffered per protocol: applied on `done`.
    pending_active: bool,
    active: bool,
}

struct TlState {
    tx: Sender<FocusEvent>,
    toplevels: HashMap<ObjectId, Toplevel>,
    last_sent: Option<FocusEvent>,
    /// Manager sent `finished` or the receiver hung up — stop the loop.
    stop: bool,
}

impl TlState {
    /// Send the focus of the active toplevel if it changed since last send.
    fn maybe_send(&mut self, id: &ObjectId) {
        let Some(tl) = self.toplevels.get(id) else { return };
        if !tl.active {
            return;
        }
        // The wlr protocol does not expose the PID — /proc advice is
        // unavailable on this path (niri/hyprland IPC provide it).
        let ev = FocusEvent { app_id: tl.app_id.clone(), title: tl.title.clone(), pid: None };
        if self.last_sent.as_ref() == Some(&ev) {
            return;
        }
        self.last_sent = Some(ev.clone());
        if self.tx.send(ev).is_err() {
            self.stop = true; // daemon dropped the receiver
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for TlState {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for TlState {
    fn event(
        state: &mut Self,
        _: &ZwlrForeignToplevelManagerV1,
        event: zwlr_foreign_toplevel_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } => {
                state.toplevels.insert(toplevel.id(), Toplevel::default());
            }
            zwlr_foreign_toplevel_manager_v1::Event::Finished => {
                warn!("wlr-toplevel: manager finished — stream ends");
                state.stop = true;
            }
            _ => {}
        }
    }

    event_created_child!(TlState, ZwlrForeignToplevelManagerV1, [
        zwlr_foreign_toplevel_manager_v1::EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for TlState {
    fn event(
        state: &mut Self,
        proxy: &ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use zwlr_foreign_toplevel_handle_v1::Event;
        let id = proxy.id();
        match event {
            Event::AppId { app_id } => {
                if let Some(tl) = state.toplevels.get_mut(&id) {
                    tl.app_id = Some(app_id);
                }
            }
            Event::Title { title } => {
                if let Some(tl) = state.toplevels.get_mut(&id) {
                    tl.title = Some(title);
                }
            }
            Event::State { state: bytes } => {
                if let Some(tl) = state.toplevels.get_mut(&id) {
                    tl.pending_active = is_activated(&bytes);
                }
            }
            Event::Done => {
                if let Some(tl) = state.toplevels.get_mut(&id) {
                    tl.active = tl.pending_active;
                }
                // Fires on activation AND on title change of the active
                // toplevel (browser tab switch → per-site rules).
                state.maybe_send(&id);
            }
            Event::Closed => {
                state.toplevels.remove(&id);
                proxy.destroy();
            }
            _ => {}
        }
    }
}

/// Probe + spawn: returns false when the compositor does not expose
/// zwlr_foreign_toplevel_management_v1 (caller falls back to its own IPC).
/// On success a blocking thread follows the stream for the process lifetime.
pub fn spawn_wlr_toplevel_stream(tx: Sender<FocusEvent>) -> bool {
    let Ok(conn) = Connection::connect_to_env() else {
        warn!("wlr-toplevel: no Wayland display");
        return false;
    };
    let Ok((globals, mut queue)) = registry_queue_init::<TlState>(&conn) else {
        warn!("wlr-toplevel: registry init failed");
        return false;
    };
    let qh = queue.handle();
    let manager: ZwlrForeignToplevelManagerV1 = match globals.bind(&qh, 1..=3, ()) {
        Ok(m) => m,
        Err(_) => {
            info!("wlr-toplevel: {MANAGER_INTERFACE} not offered by compositor");
            return false;
        }
    };
    info!("wlr-toplevel: focus stream connected (generic wlroots path)");

    std::thread::spawn(move || {
        let _manager = manager; // keep alive for the thread's lifetime
        let mut state = TlState {
            tx,
            toplevels: HashMap::new(),
            last_sent: None,
            stop: false,
        };
        while !state.stop {
            if let Err(e) = queue.blocking_dispatch(&mut state) {
                warn!("wlr-toplevel: dispatch error: {e} — stream ends");
                break;
            }
        }
    });
    true
}


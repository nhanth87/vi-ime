// SPDX-License-Identifier: GPL-3.0-or-later OR Commercial
// Copyright (c) 2024-2026 vi-im contributors
//! Registry probe — list the compositor's advertised globals.
//!
//! The globals are the ground truth for "can vi-im run here": no
//! `zwp_input_method_manager_v2` means no IME, period, regardless of what
//! XDG_CURRENT_DESKTOP claims. Used by `vi-daemon --doctor`.

use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::wl_registry;
use wayland_client::{Connection, Dispatch, QueueHandle};

struct Probe;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Probe {
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

/// All (interface, version) pairs the compositor advertises.
/// Empty when there is no Wayland display at all.
pub fn list_globals() -> Vec<(String, u32)> {
    let Ok(conn) = Connection::connect_to_env() else {
        return Vec::new();
    };
    let Ok((globals, _queue)) = registry_queue_init::<Probe>(&conn) else {
        return Vec::new();
    };
    globals
        .contents()
        .clone_list()
        .into_iter()
        .map(|g| (g.interface, g.version))
        .collect()
}

/// Does the registry offer `interface`?
pub fn has_global(globals: &[(String, u32)], interface: &str) -> bool {
    globals.iter().any(|(name, _)| name == interface)
}

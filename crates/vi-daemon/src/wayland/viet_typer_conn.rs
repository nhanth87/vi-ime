// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! VietTyper connection selection — split from viet_typer.rs (R4).
//!
//! Which Wayland connection the typer's virtual keyboard lives on.
//! General auto-selection (zero config, every desktop): Hyprland only
//! exempts a vk owned by the grab client's connection
//! (`shouldIgnoreVirtualKeyboard` compares `wl_client`), so it needs the
//! shared connection; everywhere else uses an OWN connection (field-proven
//! since 2026-07, no shared-socket roundtrip contention with the main event
//! loop). The vk↔grab routing is UNSPECIFIED in both protocol XMLs
//! (input-method-v2, virtual-keyboard-v1), hence per-compositor behavior.
//! Env overrides exist for diagnosis only:
//! - `VI_IME_TYPER_OWN_CONNECTION=1` forces Owned,
//! - `VI_IME_TYPER_SHARED_CONNECTION=1` forces Shared.

use crate::engine::fast_engine::CompositorKind;
use wayland_client::EventQueue;

/// Which Wayland connection the typer's virtual keyboard lives on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TyperConnection {
    /// Same `wl_client` as the input-method grab.
    Shared,
    /// Own connection (pre-c80c777 behavior).
    Owned,
}

/// Auto-selection + env overrides. Env wins (diagnosis); otherwise the
/// compositor decides — no user action needed on any desktop.
pub(crate) fn resolve_connection(compositor: CompositorKind) -> TyperConnection {
    if std::env::var("VI_IME_TYPER_OWN_CONNECTION").is_ok() {
        return TyperConnection::Owned;
    }
    if std::env::var("VI_IME_TYPER_SHARED_CONNECTION").is_ok() {
        return TyperConnection::Shared;
    }
    auto_connection(compositor)
}

/// Pure compositor → connection mapping (unit-tested; env handling stays in
/// `resolve_connection` because process-global env is racy under parallel
/// test threads).
pub(crate) fn auto_connection(compositor: CompositorKind) -> TyperConnection {
    match compositor {
        CompositorKind::Hyprland => TyperConnection::Shared,
        _ => TyperConnection::Owned,
    }
}

/// Compositor-ack roundtrip with anomaly logging: slower than 50ms means
/// glyphs visibly lag behind typing (field 2026-10-15: user re-presses keys
/// thinking they didn't register). Normal local roundtrips are <5ms.
/// Generic over the queue sink so both typer threads share it. `backlog` =
/// commands queued + in flight at stall time (proves/negates pile-up).
pub(crate) fn timed_roundtrip<S>(
    queue: &mut EventQueue<S>,
    state: &mut S,
    what: &str,
    backlog: u64,
) {
    let t0 = std::time::Instant::now();
    let _ = queue.roundtrip(state);
    let ms = t0.elapsed().as_millis();
    if ms >= 50 {
        tracing::warn!(
            "[VIET-TYPER] roundtrip chậm: {ms}ms ({what}, backlog={backlog}) — glyph hiện trễ"
        );
    }
}

#[cfg(test)]
mod conn_tests {
    use super::*;

    /// Only Hyprland shares the grab connection; every other compositor
    /// (and unknown) uses an own connection.
    #[test]
    fn auto_connection_per_compositor() {
        assert_eq!(auto_connection(CompositorKind::Hyprland), TyperConnection::Shared);
        assert_eq!(auto_connection(CompositorKind::Niri), TyperConnection::Owned);
        assert_eq!(auto_connection(CompositorKind::Kde), TyperConnection::Owned);
        assert_eq!(auto_connection(CompositorKind::Gnome), TyperConnection::Owned);
        assert_eq!(auto_connection(CompositorKind::Cosmic), TyperConnection::Owned);
        assert_eq!(auto_connection(CompositorKind::Unknown), TyperConnection::Owned);
    }
}

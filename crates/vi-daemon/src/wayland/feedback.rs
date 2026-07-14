// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Hard protocol signals the IME thread reports back to the daemon.
//!
//! These are FACTS from the wire (capability detection), not heuristics:
//! the daemon attributes them to its current focus, feeds the learned
//! cache (vi-config `LearnedStore`) and telemetry, and decides whether to
//! notify the user. The IME thread never blocks on the callback — it must
//! be a cheap channel send.

/// One stage of a keystroke's pipeline — used to localize WHERE a key got
/// stuck so blame lands on the right component:
/// - `Delivery`: compositor → IME transport (blame compositor/Wayland).
/// - `QueueWait`: held in the IME key buffer waiting for rollover coalescing.
/// - `Engine`: vi-im's own processing (blame us — should be µs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineStage {
    Delivery,
    QueueWait,
    Engine,
}

/// One observation from the input-method protocol stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImeFeedback {
    /// A text input attached (Activate): the focused app speaks the
    /// input-method path at all.
    Activated,
    /// The app sent `surrounding_text` this activation — a hard capability
    /// signal, fed into the learned cache.
    SurroundingTextSeen,
    /// The compositor reported another IME owns the seat.
    Unavailable,
    /// Key events arrived with non-monotonic timestamps (reordering on the
    /// path keyboard → compositor → us). `delta_ms` = how far back in time.
    KeyReorder { delta_ms: u32 },
    /// Same keycode re-pressed within the chatter window (key bounce /
    /// stuck-repeat "buzz") — coalesced, but counted for telemetry.
    KeyChatter { keycode: u32 },
    /// Per-keystroke stage latency sample (see [`PipelineStage`]).
    StageSample { stage: PipelineStage, us: u32 },
}

/// Callback the daemon installs to receive feedback. Must be non-blocking.
pub type FeedbackFn = Box<dyn Fn(ImeFeedback) + Send>;

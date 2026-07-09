// SPDX-License-Identifier: GPL-3.0-or-later OR Commercial
// Copyright (c) 2024-2026 vi-im contributors
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
/// - `QueueWait`: held in the IME key buffer waiting for a commit ack
///   (blame the pending ack chain, not typing speed).
/// - `Engine`: vi-im's own processing (blame us — should be µs).
/// - `AckWait` is reported via `DoneAck`/`DoneTimeout`: the
///   compositor↔app text-input-v3 leg (blame app/v3 bridge).
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
    /// The app sent `surrounding_text` this activation → the live
    /// delete+commit model is safe here.
    SurroundingTextSeen,
    /// Phase-1 delete was acked by `done` after `latency_us` µs.
    DoneAck { latency_us: u32 },
    /// `done` never came within the timeout; phase-2 was forced.
    DoneTimeout,
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

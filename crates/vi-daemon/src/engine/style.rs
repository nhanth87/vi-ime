// SPDX-License-Identifier: GPL-3.0-or-later OR Commercial
// Copyright (c) 2024-2026 vi-im contributors
//! Tone-placement style — the only user-visible orthographic choice.
//!
//! Stable crate-root type: `vi-daemon` (runtime.rs/sync.rs/ipc.rs) encodes it
//! as Classic=0, Modern=1. Do NOT change the variants.

/// Tone placement style for glide clusters without coda (hòa vs hoà).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToneStyle {
    /// "hòa", "thúy" — kiểu đặt dấu cũ, quen thuộc truyền thống.
    #[default]
    Classic,
    /// "hoà", "thuý" — dấu trên âm chính (chuẩn ngôn ngữ học).
    Modern,
}

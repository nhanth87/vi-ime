// SPDX-License-Identifier: GPL-3.0-or-later OR Commercial
// Copyright (c) 2024-2026 vi-im contributors
#![allow(dead_code)]
//! Vietnamese IME core (was the `vi-engine` leaf crate, folded in to cut crate
//! count). Free of any Wayland/GUI deps; all I/O goes through the `Engine` API.
//!
//! "Parse, don't mutate": raw keys are the single source of truth; each
//! keystroke re-derives the whole syllable through ONE unified NFD/Unicode-math
//! path (`syllable`), for every input method — no cluster lookup table, tone
//! placement is a pure algorithm, diacritics come from NFC composition.
//!
//! Parts unused by the daemon binary are kept as library API (for reuse by
//! other language front-ends), hence the module-level `allow(dead_code)`.

#[allow(clippy::module_inception)]
mod engine;
pub mod fast_engine;
mod glyph;
mod normalize;
mod style;
mod syllable;
mod tone;
mod types;

pub use engine::Engine;
pub use style::ToneStyle;
pub use types::{Action, AppSupport, ImeMode, InputMethod, NonPreeditAction, OutputMode};

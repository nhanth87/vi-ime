// SPDX-License-Identifier: GPL-3.0-or-later OR Commercial
// Copyright (c) 2024-2026 vi-im contributors
//! Bridging vi-config types → vi-engine runtime snapshots.
//! vi-engine is the crate-DAG leaf, so the enum duplication between
//! vi-config and vi-engine is intentional; this is the single mapping point.

use crate::config::{EffectiveConfig, ResolvedConfig};
use crate::wayland::RuntimeSnapshot;

/// Map a 4-layer resolved config to a runtime snapshot, carrying whether
/// the mode was an explicit user choice (ContentType-Terminal respects it).
pub fn resolved_to_snapshot(r: &ResolvedConfig) -> RuntimeSnapshot {
    RuntimeSnapshot {
        mode_from_user: r.mode_source.is_user(),
        ..to_snapshot(&r.config)
    }
}

/// Map a resolved config to a runtime snapshot for the IME thread.
/// `generation` is managed by `RuntimeConfig::store`, so it's left at 0 here.
pub fn to_snapshot(eff: &EffectiveConfig) -> RuntimeSnapshot {
    RuntimeSnapshot {
        enabled: eff.enabled,
        method: match eff.input_method {
            crate::config::InputMethod::Telex => crate::engine::InputMethod::Telex,
            crate::config::InputMethod::Vni => crate::engine::InputMethod::Vni,
            crate::config::InputMethod::Smart => crate::engine::InputMethod::Smart,
        },
        mode: match eff.ime_mode {
            crate::config::ImeMode::Preedit => crate::engine::ImeMode::Preedit,
            crate::config::ImeMode::NonPreedit => crate::engine::ImeMode::NonPreedit,
        },
        output: match eff.output_mode {
            crate::config::OutputMode::UnicodeDungSan => crate::engine::OutputMode::UnicodeDungSan,
            crate::config::OutputMode::UnicodeToHop => crate::engine::OutputMode::UnicodeToHop,
        },
        free_tone: eff.free_tone_placement,
        auto_detect: eff.auto_detect_lang,
        tone_style: match eff.tone_style {
            crate::config::ToneStyle::Classic => crate::engine::ToneStyle::Classic,
            crate::config::ToneStyle::Modern => crate::engine::ToneStyle::Modern,
        },
        mode_from_user: false,
        game_mode: false,
        generation: 0,
    }
}


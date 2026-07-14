// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Per-app IME telemetry — aggregates the hard protocol signals the IME
//! thread reports (via the daemon), persisted to
//! `~/.local/share/vi-ime/telemetry.toml`.
//!
//! Answers, with data instead of guesses:
//! - which apps send `surrounding_text` (live-model capability),
//! - whether the compositor ever delivered key events out of order,
//! - how often key chatter ("buzz") was coalesced,
//! - where a keystroke lost time (delivery / queue / engine).
//!
//! Pure counters + EMA — no timers, no threads (R15-friendly); the owner
//! decides when to `save()` (event-driven, throttled).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// EMA smoothing for stage latency (weight of the newest sample).
const EMA_ALPHA: f64 = 0.2;

/// One keystroke pipeline stage, for localizing WHERE keys get stuck.
/// Blame map: Delivery → compositor/Wayland transport; QueueWait → the
/// rollover coalescing buffer; Engine → vi-im itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Delivery,
    QueueWait,
    Engine,
}

impl Stage {
    /// Above this the stage counts as a stall (µs).
    fn stall_threshold_us(self) -> u32 {
        match self {
            Stage::Delivery => 20_000,  // >20ms compositor→IME = lag
            Stage::QueueWait => 50_000, // >50ms in buffer = coalescing stuck
            Stage::Engine => 5_000,     // >5ms in vi-im = our bug
        }
    }

    /// Who to blame, for the report.
    fn blame_label(self) -> &'static str {
        match self {
            Stage::Delivery => "compositor/Wayland transport",
            Stage::QueueWait => "rollover coalescing buffer",
            Stage::Engine => "vi-im engine (OUR bug)",
        }
    }
}

/// Latency aggregate for one stage.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StageStat {
    #[serde(default)]
    pub samples: u64,
    #[serde(default)]
    pub ema_us: f64,
    #[serde(default)]
    pub max_us: u32,
    /// Samples that exceeded the stage's stall threshold.
    #[serde(default)]
    pub stalls: u64,
}

impl StageStat {
    fn add(&mut self, us: u32, threshold_us: u32) {
        self.samples += 1;
        self.max_us = self.max_us.max(us);
        self.ema_us = if self.samples == 1 {
            f64::from(us)
        } else {
            EMA_ALPHA * f64::from(us) + (1.0 - EMA_ALPHA) * self.ema_us
        };
        if us > threshold_us {
            self.stalls += 1;
        }
    }
}

/// Aggregated metrics for one app (or the unattributed bucket).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AppMetrics {
    /// IME activations (text input attached).
    #[serde(default)]
    pub activations: u64,
    /// Activations in which surrounding_text was seen.
    #[serde(default)]
    pub surrounding_seen: u64,
    /// Key events delivered with non-monotonic timestamps.
    #[serde(default)]
    pub key_reorders: u64,
    /// Largest observed backwards time jump, ms.
    #[serde(default)]
    pub reorder_max_ms: u32,
    /// Coalesced chatter/bounce ("buzz") events.
    #[serde(default)]
    pub key_chatter: u64,
    /// Compositor→IME transport latency (blame: compositor).
    #[serde(default)]
    pub stage_delivery: StageStat,
    /// Time keys sat in the IME buffer (blame: rollover coalescing).
    #[serde(default)]
    pub stage_queue: StageStat,
    /// vi-im's own processing time (blame: us).
    #[serde(default)]
    pub stage_engine: StageStat,
}

/// All telemetry, keyed by app_id. `"?"` holds unattributed signals
/// (no focused app known at the time).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Telemetry {
    #[serde(default)]
    pub apps: HashMap<String, AppMetrics>,
}

impl Telemetry {
    /// `~/.local/share/vi-ime/telemetry.toml` (respects XDG_DATA_HOME).
    pub fn default_path() -> PathBuf {
        let base = if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
            PathBuf::from(xdg)
        } else if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join(".local").join("share")
        } else {
            PathBuf::from(".")
        };
        base.join("vi-ime").join("telemetry.toml")
    }

    /// Load from disk to accumulate across sessions; missing → empty.
    pub fn load(path: &Path) -> Self {
        match fs::read_to_string(path) {
            Ok(s) => toml::from_str(&s).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Persist to disk (creates parent dirs).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        fs::write(path, content)
    }

    fn app(&mut self, app_id: Option<&str>) -> &mut AppMetrics {
        self.apps.entry(app_id.unwrap_or("?").to_string()).or_default()
    }

    pub fn record_activate(&mut self, app_id: Option<&str>) {
        self.app(app_id).activations += 1;
    }

    pub fn record_surrounding_seen(&mut self, app_id: Option<&str>) {
        self.app(app_id).surrounding_seen += 1;
    }

    /// Feed one pipeline-stage latency sample (see [`Stage`] blame map).
    pub fn record_stage(&mut self, app_id: Option<&str>, stage: Stage, us: u32) {
        let threshold = stage.stall_threshold_us();
        let m = self.app(app_id);
        let stat = match stage {
            Stage::Delivery => &mut m.stage_delivery,
            Stage::QueueWait => &mut m.stage_queue,
            Stage::Engine => &mut m.stage_engine,
        };
        stat.add(us, threshold);
    }

    /// Where do this app's keystrokes get stuck? Returns the guiltiest
    /// stage (worst EMA relative to its stall threshold) when any stage
    /// is over threshold — the "đổ lỗi" line for debugging.
    pub fn blame(&self, app_id: &str) -> Option<String> {
        let m = self.apps.get(app_id)?;
        let stages: [(Stage, &StageStat); 3] = [
            (Stage::Delivery, &m.stage_delivery),
            (Stage::QueueWait, &m.stage_queue),
            (Stage::Engine, &m.stage_engine),
        ];
        let (stage, stat, ratio) = stages
            .iter()
            .filter(|(_, s)| s.samples > 0)
            .map(|(st, s)| (*st, *s, s.ema_us / f64::from(st.stall_threshold_us())))
            .max_by(|a, b| a.2.total_cmp(&b.2))?;
        if ratio < 1.0 {
            return None; // everything within budget — nobody to blame
        }
        Some(format!(
            "{app_id}: keys stuck at {} — ema {:.1}ms, max {:.1}ms, stalls {}/{}",
            stage.blame_label(),
            stat.ema_us / 1000.0,
            f64::from(stat.max_us) / 1000.0,
            stat.stalls,
            stat.samples,
        ))
    }

    pub fn record_key_reorder(&mut self, app_id: Option<&str>, delta_ms: u32) {
        let m = self.app(app_id);
        m.key_reorders += 1;
        m.reorder_max_ms = m.reorder_max_ms.max(delta_ms);
    }

    pub fn record_key_chatter(&mut self, app_id: Option<&str>) {
        self.app(app_id).key_chatter += 1;
    }

    /// Human-readable summary for logs / the control panel.
    pub fn report(&self) -> String {
        let mut names: Vec<&String> = self.apps.keys().collect();
        names.sort();
        let mut out = String::with_capacity(names.len() * 80);
        for name in names {
            let Some(m) = self.apps.get(name) else { continue };
            out.push_str(&format!(
                "{name}: act={} surr={} reorder={} (max {}ms) chatter={}\n",
                m.activations,
                m.surrounding_seen,
                m.key_reorders,
                m.reorder_max_ms,
                m.key_chatter,
            ));
        }
        out
    }
}

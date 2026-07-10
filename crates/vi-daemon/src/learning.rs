// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 vi-im contributors
//! Adaptation state: learned cache + telemetry + notify throttling.
//!
//! Consumes `ImeFeedback` signals (hard protocol facts from the IME thread),
//! updates the learned store and telemetry, and decides when a config
//! re-resolve or a user notification is warranted. Persistence is throttled
//! and event-driven — no timers (R15).

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use tracing::{info, warn};
use crate::config::{AppConfig, LearnedStore};
use crate::telemetry::Telemetry;
use crate::wayland::feedback::PipelineStage;
use crate::wayland::ImeFeedback;

use crate::advisor;

/// Don't hit the disk more often than this (checked on events only).
const PERSIST_EVERY: Duration = Duration::from_secs(5);

pub struct Adaptation {
    learned: LearnedStore,
    learned_path: PathBuf,
    telemetry: Telemetry,
    telemetry_path: PathBuf,
    /// Apps already advised this session (notify once per app per session).
    notified: HashSet<String>,
    /// Whether the IME saw an Activate since the last focus change.
    activated_since_focus: bool,
    last_persist: Instant,
    dirty: bool,
}

impl Adaptation {
    pub fn load() -> Self {
        let learned_path = LearnedStore::default_path();
        let telemetry_path = Telemetry::default_path();
        Self {
            learned: LearnedStore::load(&learned_path),
            learned_path,
            telemetry: Telemetry::load(&telemetry_path),
            telemetry_path,
            notified: HashSet::new(),
            activated_since_focus: false,
            last_persist: Instant::now(),
            dirty: false,
        }
    }

    /// Learned override for the 4-layer resolution (layer 2).
    pub fn learned_config(&self, app_id: Option<&str>) -> Option<AppConfig> {
        app_id.and_then(|id| self.learned.suggested_config(id))
    }

    /// Access the raw learned store (for IPC read commands).
    pub fn learned_store(&self) -> &LearnedStore {
        &self.learned
    }

    /// Reset per-focus tracking; call on every focus change.
    pub fn on_focus_change(&mut self) {
        self.activated_since_focus = false;
    }

    /// Fold one IME signal in. Returns true when the learned suggestion for
    /// this app may have CHANGED — the caller should re-resolve the config.
    pub fn handle_feedback(&mut self, app_id: Option<&str>, fb: ImeFeedback) -> bool {
        let mut suggestion_changed = false;
        match fb {
            ImeFeedback::Activated => {
                self.activated_since_focus = true;
                self.telemetry.record_activate(app_id);
                if let Some(id) = app_id {
                    self.dirty |= self.learned.observe(id, |p| p.ime_activated = Some(true));
                }
            }
            ImeFeedback::SurroundingTextSeen => {
                self.telemetry.record_surrounding_seen(app_id);
                if let Some(id) = app_id {
                    let before = self.learned.suggested_config(id);
                    let changed =
                        self.learned.observe(id, |p| p.surrounding_text = Some(true));
                    self.dirty |= changed;
                    suggestion_changed = changed && before != self.learned.suggested_config(id);
                }
            }
            ImeFeedback::Unavailable => {
                warn!("another IME owns the seat — signals paused");
            }
            ImeFeedback::KeyReorder { delta_ms } => {
                self.telemetry.record_key_reorder(app_id, delta_ms);
            }
            ImeFeedback::KeyChatter { .. } => {
                self.telemetry.record_key_chatter(app_id);
            }
            ImeFeedback::StageSample { stage, us } => {
                // Pipeline blame: where did this keystroke lose time?
                let st = match stage {
                    PipelineStage::Delivery => crate::telemetry::Stage::Delivery,
                    PipelineStage::QueueWait => crate::telemetry::Stage::QueueWait,
                    PipelineStage::Engine => crate::telemetry::Stage::Engine,
                };
                self.telemetry.record_stage(app_id, st, us);
                if let Some(id) = app_id
                    && let Some(verdict) = self.telemetry.blame(id) {
                        info!("[BLAME] {verdict}");
                    }
            }
        }
        self.dirty = true; // telemetry counters moved regardless
        self.persist_if_due();
        suggestion_changed
    }

    /// Probe verdict: the app got focus a while ago and never attached a
    /// text input. Returns the advice message to show (once per app per
    /// session), or None when stale/already handled/supported.
    pub fn probe_timeout(
        &mut self,
        probed_app: &str,
        current_app: Option<&str>,
        pid: Option<i32>,
    ) -> Option<String> {
        if current_app != Some(probed_app) {
            return None; // stale probe — focus moved on
        }
        if self.activated_since_focus {
            return None; // app spoke IME meanwhile
        }
        if !self.notified.insert(probed_app.to_string()) {
            return None; // already advised this session
        }
        info!("[UNSUPPORTED] app={probed_app} — no Activate since focus");
        // NOTE (R11): we deliberately do NOT persist "unsupported" into the
        // learned store — a focused app without a text field also never
        // activates; only positive capability is stored permanently.
        let advice = pid
            .and_then(advisor::read_proc)
            .and_then(|info| advisor::electron_advice(&info))
            .unwrap_or_else(advisor::generic_advice);
        Some(advice)
    }

    /// Throttled event-driven persistence (no timers).
    pub fn persist_if_due(&mut self) {
        if self.dirty && self.last_persist.elapsed() >= PERSIST_EVERY {
            self.persist_now();
        }
    }

    /// Unconditional flush (shutdown path).
    pub fn persist_now(&mut self) {
        if !self.dirty {
            return;
        }
        if let Err(e) = self.learned.save(&self.learned_path) {
            warn!("learned.toml save failed: {e}");
        }
        if let Err(e) = self.telemetry.save(&self.telemetry_path) {
            warn!("telemetry.toml save failed: {e}");
        }
        self.dirty = false;
        self.last_persist = Instant::now();
    }
}

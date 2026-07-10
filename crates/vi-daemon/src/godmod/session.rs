// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 vi-im contributors
//! GodmodSession — the core telemetry recorder.

use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::engine::ImeMode;

use crate::godmod::models::{AppMetrics, KeyEvent, SessionSummary};

const GODMOD_DIR: &str = "vi-ime/godmod";

/// Civil (Y, M, D, h, m, s) from Unix seconds — Howard Hinnant's algorithm,
/// so godmod timestamps need no date library (debug-only path).
fn civil_utc(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let rem = secs.rem_euclid(86_400);
    let (hh, mi, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m as u32, d as u32, hh as u32, mi as u32, ss as u32)
}

fn now_utc() -> ((i64, u32, u32, u32, u32, u32), u32) {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    (civil_utc(now.as_secs() as i64), now.subsec_millis())
}

/// Compact session id: `YYYYMMDD_HHMMSS_mmm`.
fn session_stamp() -> String {
    let ((y, mo, d, h, mi, s), ms) = now_utc();
    format!("{y:04}{mo:02}{d:02}_{h:02}{mi:02}{s:02}_{ms:03}")
}

/// RFC 3339 UTC instant: `YYYY-MM-DDTHH:MM:SSZ`.
fn rfc3339_utc() -> String {
    let ((y, mo, d, h, mi, s), _) = now_utc();
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

pub struct GodmodSession {
    enabled: bool,
    session_id: String,
    started_at: Instant,
    writer: Option<BufWriter<File>>,
    output_dir: PathBuf,
    latencies: VecDeque<u64>,

    // Counters
    pub(crate) total_keystrokes: u64,
    pub(crate) vietnamese_words: u64,
    pub(crate) english_words: u64,
    pub(crate) commits: u64,
    pub(crate) backspaces: u64,
    pub(crate) rollover_skips: u64,
    pub(crate) deactivate_events: u64,
    pub(crate) activate_events: u64,
    pub(crate) max_latency_us: u64,
    pub(crate) current_app_id: Option<String>,
    pub(crate) app_metrics: Vec<AppMetrics>,
}

fn output_dir() -> PathBuf {
    std::env::var("VI_GODMOD_DIR")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::data_local_dir()
                .map(|d| d.join(GODMOD_DIR))
                .unwrap_or_else(|| PathBuf::from("."))
        })
}

impl GodmodSession {
    pub fn new(enabled: bool) -> Self {
        let session_id = session_stamp();
        let dir = output_dir();
        let writer = if enabled {
            fs::create_dir_all(&dir).ok();
            File::create(dir.join(format!("{session_id}.jsonl")))
                .ok()
                .map(|f| BufWriter::with_capacity(8192, f))
        } else {
            None
        };
        Self {
            enabled,
            session_id,
            started_at: Instant::now(),
            writer,
            output_dir: dir,
            latencies: VecDeque::with_capacity(1000),
            total_keystrokes: 0,
            vietnamese_words: 0,
            english_words: 0,
            commits: 0,
            backspaces: 0,
            rollover_skips: 0,
            deactivate_events: 0,
            activate_events: 0,
            max_latency_us: 0,
            current_app_id: None,
            app_metrics: Vec::new(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn log_key(
        &mut self, keycode: u32, ch: Option<char>, mode: ImeMode,
        action: &str, latency_us: u64, buffer_depth: usize,
        has_pending: bool, preedit_text: &str,
    ) {
        if !self.enabled { return; }
        self.total_keystrokes += 1;
        self.latencies.push_back(latency_us);
        if self.latencies.len() > 1000 { self.latencies.pop_front(); }
        if latency_us > self.max_latency_us { self.max_latency_us = latency_us; }

        let event = KeyEvent {
            timestamp: rfc3339_utc(),
            elapsed_us: self.started_at.elapsed().as_micros() as u64,
            keycode,
            character: ch,
            app_id: self.current_app_id.clone(),
            ime_mode: format!("{mode:?}"),
            action: action.to_string(),
            latency_us,
            buffer_depth,
            has_pending,
            preedit_text: preedit_text.to_string(),
        };
        if let Some(ref mut w) = self.writer
            && let Ok(json) = serde_json::to_string(&event) {
                let _ = writeln!(w, "{json}");
            }
        // Per-app stats
        if let Some(ref id) = self.current_app_id
            && let Some(m) = self.app_metrics.iter_mut().find(|a| a.app_id == *id) {
                m.keystrokes += 1;
                if latency_us > m.max_latency_us { m.max_latency_us = latency_us; }
                m.avg_latency_us = (m.avg_latency_us * (m.keystrokes - 1) as f64 + latency_us as f64) / m.keystrokes as f64;
            }
    }

    pub fn log_commit(&mut self, is_vn: bool) {
        if !self.enabled { return; }
        self.commits += 1;
        if is_vn { self.vietnamese_words += 1; } else { self.english_words += 1; }
        if let Some(ref id) = self.current_app_id
            && let Some(m) = self.app_metrics.iter_mut().find(|a| a.app_id == *id) {
                m.commits += 1;
            }
    }

    pub fn log_backspace(&mut self) { if self.enabled { self.backspaces += 1; } }
    pub fn log_rollover(&mut self) { if self.enabled { self.rollover_skips += 1; } }
    pub fn log_activate(&mut self) { if self.enabled { self.activate_events += 1; } }
    pub fn log_deactivate(&mut self) { if self.enabled { self.deactivate_events += 1; } }

    pub fn set_app(&mut self, app_id: &str) {
        let id = app_id.to_string();
        if !self.app_metrics.iter().any(|a| a.app_id == id) {
            self.app_metrics.push(AppMetrics { app_id: id.clone(), keystrokes: 0, commits: 0, avg_latency_us: 0.0, max_latency_us: 0 });
        }
        self.current_app_id = Some(id);
    }

    pub fn finish(&mut self) -> Option<SessionSummary> {
        if !self.enabled { return None; }
        let mut sorted: Vec<u64> = self.latencies.iter().copied().collect();
        sorted.sort_unstable();
        let len = sorted.len();
        let p50 = if len > 0 { sorted[len / 2] } else { 0 };
        let p99 = if len > 0 { sorted[(len * 99) / 100] } else { 0 };
        let avg = if len > 0 { sorted.iter().sum::<u64>() as f64 / len as f64 } else { 0.0 };

        let summary = SessionSummary {
            session_id: self.session_id.clone(),
            started_at: String::new(),
            ended_at: rfc3339_utc(),
            duration_secs: self.started_at.elapsed().as_secs_f64(),
            total_keystrokes: self.total_keystrokes,
            vietnamese_words: self.vietnamese_words,
            english_words: self.english_words,
            commits: self.commits,
            backspaces: self.backspaces,
            rollover_skips: self.rollover_skips,
            deactivate_events: self.deactivate_events,
            activate_events: self.activate_events,
            avg_latency_us: avg,
            max_latency_us: self.max_latency_us,
            p50_latency_us: p50,
            p99_latency_us: p99,
            compositor: std::env::var("XDG_SESSION_TYPE").unwrap_or_default(),
            apps_used: self.app_metrics.iter().map(|a| a.app_id.clone()).collect(),
        };
        if let Some(ref mut w) = self.writer
            && let Ok(json) = serde_json::to_string(&summary) {
                let _ = writeln!(w, "# SUMMARY: {json}");
                let _ = w.flush();
            }
        // Per-app JSON
        if let Ok(f) = File::create(self.output_dir.join(format!("{}_apps.json", self.session_id))) {
            let mut w = BufWriter::new(f);
            if let Ok(json) = serde_json::to_string_pretty(&self.app_metrics) {
                let _ = writeln!(w, "{json}");
            }
        }
        Some(summary)
    }
}

impl Drop for GodmodSession {
    fn drop(&mut self) { self.finish(); }
}

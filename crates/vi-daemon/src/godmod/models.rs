// SPDX-License-Identifier: GPL-3.0-or-later OR Commercial
// Copyright (c) 2024-2026 vi-im contributors
//! Data models for godmod telemetry.

use serde::Serialize;

/// A single keystroke event with full context.
#[derive(Debug, Clone, Serialize)]
pub struct KeyEvent {
    pub timestamp: String,
    pub elapsed_us: u64,
    pub keycode: u32,
    pub character: Option<char>,
    pub app_id: Option<String>,
    pub ime_mode: String,
    pub action: String,
    pub latency_us: u64,
    pub buffer_depth: usize,
    pub has_pending: bool,
    pub preedit_text: String,
}

/// Summary of a typing session.
#[derive(Debug, Clone, Serialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub started_at: String,
    pub ended_at: String,
    pub duration_secs: f64,
    pub total_keystrokes: u64,
    pub vietnamese_words: u64,
    pub english_words: u64,
    pub commits: u64,
    pub backspaces: u64,
    pub rollover_skips: u64,
    pub deactivate_events: u64,
    pub activate_events: u64,
    pub avg_latency_us: f64,
    pub max_latency_us: u64,
    pub p50_latency_us: u64,
    pub p99_latency_us: u64,
    pub compositor: String,
    pub apps_used: Vec<String>,
}

/// Per-app metrics.
#[derive(Debug, Clone, Serialize)]
pub struct AppMetrics {
    pub app_id: String,
    pub keystrokes: u64,
    pub commits: u64,
    pub avg_latency_us: f64,
    pub max_latency_us: u64,
}

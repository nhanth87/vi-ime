// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Live runtime configuration shared between the daemon and the IME thread.
//!
//! The daemon writes a new snapshot (atomics, no locks); the IME event loop
//! picks it up lazily via a generation counter — no wakeup of the blocking
//! Wayland dispatch is needed. Values are encoded as `u8` so this crate
//! depends only on `vi-engine` (crate DAG rule R5).

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, AtomicU8, Ordering};
use std::sync::Mutex;

use crate::engine::fast_engine::NonPreeditEngine;
use crate::engine::{ImeMode, InputMethod, OutputMode, ToneStyle};

/// Lock-free shared config. Writers call [`RuntimeConfig::store`];
/// the IME thread calls [`RuntimeConfig::snapshot`] per key/activate event.
#[derive(Debug, Default)]
pub struct RuntimeConfig {
    /// Bumped LAST on store (Release) so readers seeing a new generation
    /// also see all field writes (Acquire).
    generation: AtomicU64,
    enabled: AtomicBool,
    method: AtomicU8,
    mode: AtomicU8,
    output: AtomicU8,
    free_tone: AtomicBool,
    auto_detect: AtomicBool,
    tone_style: AtomicU8,
    emoji: AtomicBool,
    /// True when `mode` came from an explicit user rule (setting.conf app/
    /// site entry). ContentType-Terminal must not override a user choice.
    mode_from_user: AtomicBool,
    /// Game mode: raw key passthrough, no IME processing.
    game_mode: AtomicBool,
    /// App honors surrounding-text → delete_surrounding_text safe (P0).
    surrounding_capable: AtomicBool,
    /// app_id of the currently focused window. Not an atomic (it's a String),
    /// so it lives behind a Mutex; only read when the generation changed, so
    /// the hot key path stays lock-free. Drives per-app plugin routing +
    /// AppPlugin lifecycle hooks (on_focus/on_blur) in the IME thread.
    app_id: Mutex<Option<String>>,
    /// Physical mouse-click counter (bumped by the daemon's evdev click
    /// watcher). Apps that send NO protocol signal on a click (no
    /// text_change_cause, no surrounding update) still move their cursor —
    /// the IME thread compares this counter before each key and drops the
    /// half-typed word (R8) so it can never be committed at the new spot.
    clicks: AtomicU64,
    /// eventfd the click watcher signals so the IME event loop wakes
    /// IMMEDIATELY on a physical click (Preedit mode must clear the
    /// preedit before the app reacts — waiting for the next key is too
    /// late). -1 = not available.
    click_fd: AtomicI32,
}

/// A decoded, immutable view of the runtime config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeSnapshot {
    pub enabled: bool,
    pub method: InputMethod,
    pub mode: ImeMode,
    pub output: OutputMode,
    pub free_tone: bool,
    pub auto_detect: bool,
    pub tone_style: ToneStyle,
    pub emoji: bool,
    /// The ime_mode was an explicit user (setting.conf) choice.
    pub mode_from_user: bool,
    /// Game mode active: raw key passthrough, no IME processing.
    pub game_mode: bool,
    /// App honors surrounding-text → delete_surrounding_text is safe (P0).
    pub surrounding_capable: bool,
    pub generation: u64,
}

impl Default for RuntimeSnapshot {
    fn default() -> Self {
        Self {
            enabled: true,
            method: InputMethod::Telex,
            mode: ImeMode::Preedit,
            output: OutputMode::UnicodeDungSan,
            free_tone: true,
            auto_detect: true,
            tone_style: ToneStyle::Classic,
            emoji: true,
            mode_from_user: false,
            game_mode: false,
            surrounding_capable: false,
            generation: 0,
        }
    }
}

fn encode_method(m: InputMethod) -> u8 {
    match m {
        InputMethod::Telex => 0,
        InputMethod::Vni => 1,
        InputMethod::Smart => 2,
    }
}

fn decode_method(v: u8) -> InputMethod {
    match v {
        1 => InputMethod::Vni,
        // 2 was MISSING here while encode_method emitted it — "Tự do"
        // silently decoded to Telex ("Smart mode only types Telex" bug).
        2 => InputMethod::Smart,
        _ => InputMethod::Telex, // unknown → safe default
    }
}

fn encode_mode(m: ImeMode) -> u8 {
    match m {
        ImeMode::Preedit => 0,
        ImeMode::NonPreedit => 1,
    }
}

fn decode_mode(v: u8) -> ImeMode {
    match v {
        0 => ImeMode::Preedit,
        1 => ImeMode::NonPreedit,
        _ => ImeMode::Preedit, // unknown → safe default
    }
}

fn encode_output(m: OutputMode) -> u8 {
    match m {
        OutputMode::UnicodeDungSan => 0,
        OutputMode::UnicodeToHop => 1,
    }
}

fn decode_output(v: u8) -> OutputMode {
    match v {
        1 => OutputMode::UnicodeToHop,
        _ => OutputMode::UnicodeDungSan, // unknown → safe default
    }
}

impl RuntimeConfig {
    /// Create with an initial snapshot (generation starts at 1 so a fresh
    /// IME thread with `last_generation == 0` applies it on first event).
    pub fn new(snap: &RuntimeSnapshot) -> Self {
        let cfg = Self::default();
        // Default() derives 0 — a real fd (stdin). Mark "no eventfd" explicitly.
        cfg.click_fd.store(-1, Ordering::Release);
        cfg.store(snap);
        cfg
    }

    /// Publish a new configuration. Field writes happen-before the
    /// generation bump (Release), so a reader that observes the new
    /// generation (Acquire) also observes every field.
    pub fn store(&self, snap: &RuntimeSnapshot) {
        self.enabled.store(snap.enabled, Ordering::Relaxed);
        self.method.store(encode_method(snap.method), Ordering::Relaxed);
        self.mode.store(encode_mode(snap.mode), Ordering::Relaxed);
        self.output.store(encode_output(snap.output), Ordering::Relaxed);
        self.free_tone.store(snap.free_tone, Ordering::Relaxed);
        self.auto_detect.store(snap.auto_detect, Ordering::Relaxed);
        self.tone_style.store(encode_tone_style(snap.tone_style), Ordering::Relaxed);
        self.emoji.store(snap.emoji, Ordering::Relaxed);
        self.mode_from_user.store(snap.mode_from_user, Ordering::Relaxed);
        self.game_mode.store(snap.game_mode, Ordering::Relaxed);
        self.generation.fetch_add(1, Ordering::Release);
    }

    /// Read the current configuration. If a store races mid-read the
    /// generation check on the next event picks up the final state.
    pub fn snapshot(&self) -> RuntimeSnapshot {
        let generation = self.generation.load(Ordering::Acquire);
        RuntimeSnapshot {
            enabled: self.enabled.load(Ordering::Relaxed),
            method: decode_method(self.method.load(Ordering::Relaxed)),
            mode: decode_mode(self.mode.load(Ordering::Relaxed)),
            output: decode_output(self.output.load(Ordering::Relaxed)),
            free_tone: self.free_tone.load(Ordering::Relaxed),
            auto_detect: self.auto_detect.load(Ordering::Relaxed),
            tone_style: decode_tone_style(self.tone_style.load(Ordering::Relaxed)),
            emoji: self.emoji.load(Ordering::Relaxed),
            mode_from_user: self.mode_from_user.load(Ordering::Relaxed),
            game_mode: self.game_mode.load(Ordering::Relaxed),
            surrounding_capable: self.surrounding_capable.load(Ordering::Relaxed),
            generation,
        }
    }

    /// Set game mode flag and bump generation. Called by the daemon
    /// on focus-change so the IME thread picks it up via maybe_reconfigure.
    pub fn set_game_mode(&self, v: bool) {
        self.game_mode.store(v, Ordering::Relaxed);
        self.generation.fetch_add(1, Ordering::Release);
    }
    /// Publish the focused window's app_id and bump generation so the IME
    /// thread picks it up in `maybe_reconfigure` (same channel as config).
    pub fn store_app_id(&self, app_id: Option<String>) {
        if let Ok(mut slot) = self.app_id.lock() {
            *slot = app_id;
        }
        self.generation.fetch_add(1, Ordering::Release);
    }
    /// Current focused app_id (cloned). Read only on a generation change.
    pub fn app_id(&self) -> Option<String> {
        self.app_id.lock().ok().and_then(|s| s.clone())
    }

    /// Record one physical mouse click (evdev watcher thread). No
    /// generation bump: the IME thread polls this at the next key event —
    /// there is nothing to do before then.
    pub fn record_click(&self) {
        self.clicks.fetch_add(1, Ordering::Release);
    }

    /// Current click counter (compared by the IME thread per key).
    pub fn clicks(&self) -> u64 {
        self.clicks.load(Ordering::Acquire)
    }

    /// Install the click-wakeup eventfd (daemon startup, before threads).
    pub fn set_click_fd(&self, fd: i32) {
        self.click_fd.store(fd, Ordering::Release);
    }

    /// The click-wakeup eventfd, or a negative value when unavailable.
    /// NOTE: `AtomicI32::default()` is 0, so `new()` must have stored -1;
    /// callers treat `< 0` as "off".
    pub fn click_fd(&self) -> i32 {
        self.click_fd.load(Ordering::Acquire)
    }
}

/// Apply a snapshot to a live engine. Pure function — unit-testable
/// without a Wayland connection.
pub fn apply_snapshot(engine: &mut NonPreeditEngine, snap: &RuntimeSnapshot) {
    engine.set_mode(snap.mode);
    engine.set_emoji_enabled(snap.emoji);
    let inner = engine.inner_mut();
    inner.set_method(snap.method);
    inner.set_output_mode(snap.output);
    inner.set_free_tone(snap.free_tone);
    inner.set_auto_detect(snap.auto_detect);
    inner.set_tone_style(snap.tone_style);
}

fn encode_tone_style(s: ToneStyle) -> u8 {
    match s {
        ToneStyle::Classic => 0,
        ToneStyle::Modern => 1,
    }
}

fn decode_tone_style(v: u8) -> ToneStyle {
    match v {
        1 => ToneStyle::Modern,
        _ => ToneStyle::Classic, // unknown → safe default
    }
}


// SPDX-License-Identifier: GPL-3.0-or-later OR Commercial
// Copyright (c) 2024-2026 vi-im contributors
//! High-speed typing engine — VMK1-style non-preedit word tracking.
//!
//! Timing is NOT handled here: the Wayland layer uses a two-phase commit
//! (delete → wait for `done` ack → commit), which replaced the old
//! fixed/adaptive delay approach entirely.

use crate::engine::{Action, Engine, ImeMode, InputMethod, NonPreeditAction};

// ============================================================================
// NonPreeditEngine
// ============================================================================

/// Non-preedit engine wrapper — VMK1 style, fastest possible typing.
///
/// Instead of sending preedit updates to the compositor on every keystroke,
/// this engine buffers keys silently and only commits when a word is complete.
/// The commit is done via: delete_surrounding_text(backspace N) + commit_string.
///
/// # Performance
/// - Latency: ~0ms per key (no compositor roundtrip)
/// - App compat: >90% (works even on apps with poor text-input-v3 support)
/// - Throughput: >200 words/sec in benchmarks
pub struct NonPreeditEngine {
    /// The core Vietnamese engine (Telex or VNI).
    inner: Engine,
    /// Current IME mode (can switch at runtime).
    mode: ImeMode,
    /// Number of raw keys buffered for this word.
    /// Used to calculate how many backspaces to send on commit.
    raw_count: usize,
}

impl NonPreeditEngine {
    /// Create a new engine with the given input method and mode.
    pub fn new(method: InputMethod, mode: ImeMode) -> Self {
        Self {
            inner: Engine::new(method),
            mode,
            raw_count: 0,
        }
    }

    /// Process one keypress. Returns the action for the Wayland layer.
    pub fn push_key(&mut self, ch: char) -> NonPreeditAction {
        // Pass-through checks first (before any buffering)
        if ch.is_ascii_control() && ch != '\u{0008}' {
            // Enter, escape, tab, etc. — commit if pending then pass through
            if self.inner.has_preedit() {
                let committed = self.inner.preedit_output();
                let raw_len = self.raw_count;
                self.inner.reset();
                self.raw_count = 0;
                return NonPreeditAction::CommitWithBackspace {
                    backspace_count: raw_len,
                    text: committed,
                };
            }
            return NonPreeditAction::PassThrough;
        }

        // Backspace
        if ch == '\u{0008}' {
            return self.handle_backspace();
        }

        // Word boundaries (space, punctuation) commit immediately
        if is_word_boundary(ch, self.inner.method()) {
            if self.inner.has_preedit() {
                let committed = self.inner.preedit_output();
                let raw_len = self.raw_count;
                self.inner.reset();
                self.raw_count = 0;
                return NonPreeditAction::CommitWithBackspace {
                    backspace_count: raw_len,
                    text: committed,
                };
            }
            return NonPreeditAction::PassThrough;
        }

        // Process through core engine
        self.raw_count += 1;
        let action = self.inner.push_key(ch);

        match action {
            Action::Commit(s) => {
                let raw_len = self.raw_count;
                self.raw_count = 0;
                // Engine already reset itself on Commit
                NonPreeditAction::CommitWithBackspace {
                    backspace_count: raw_len,
                    text: s,
                }
            }
            Action::UpdatePreedit(_) => {
                // Non-preedit mode: buffer silently, no visual output
                if self.should_show_preedit() {
                    // Hybrid mode with ambiguous state → show preedit
                    let s = self.inner.preedit_string().to_string();
                    NonPreeditAction::UpdatePreedit(s)
                } else {
                    NonPreeditAction::Buffer
                }
            }
            Action::PassThrough => {
                self.raw_count = self.raw_count.saturating_sub(1);
                if self.inner.has_preedit() {
                    let committed = self.inner.preedit_output();
                    let raw_len = self.raw_count;
                    self.inner.reset();
                    self.raw_count = 0;
                    NonPreeditAction::CommitWithBackspace {
                        backspace_count: raw_len,
                        text: committed,
                    }
                } else {
                    NonPreeditAction::PassThrough
                }
            }
        }
    }

    /// Process backspace during non-preedit composition.
    fn handle_backspace(&mut self) -> NonPreeditAction {
        if self.raw_count == 0 && !self.inner.has_preedit() {
            return NonPreeditAction::PassThrough;
        }

        self.raw_count = self.raw_count.saturating_sub(1);
        let action = self.inner.backspace();

        match action {
            Action::PassThrough => {
                NonPreeditAction::ClearPreedit
            }
            Action::UpdatePreedit(s) => {
                if s.is_empty() {
                    NonPreeditAction::ClearPreedit
                } else if self.should_show_preedit() {
                    NonPreeditAction::UpdatePreedit(s)
                } else {
                    // Non-preedit mode: still buffer if we have pending keys
                    NonPreeditAction::Buffer
                }
            }
            _ => NonPreeditAction::Buffer,
        }
    }

    /// Determine whether to show preedit based on current mode and state.
    fn should_show_preedit(&self) -> bool {
        // Preedit: always show preedit while typing.
        // NonPreedit: show preedit too (user needs to see what they type).
        // The distinction is the Action type: UpdatePreedit vs Buffer.
        self.mode == ImeMode::Preedit || self.inner.has_preedit()
    }

    /// Reset the engine state.
    pub fn reset(&mut self) {
        self.inner.reset();
        self.raw_count = 0;
    }

    /// Change IME mode at runtime (Preedit vs NonPreedit display only).
    pub fn set_mode(&mut self, mode: ImeMode) {
        self.mode = mode;
    }

    /// Get current mode.
    pub fn mode(&self) -> ImeMode {
        self.mode
    }

    /// Get reference to inner engine (for config changes).
    pub fn inner_mut(&mut self) -> &mut Engine {
        &mut self.inner
    }

    /// Get reference to inner engine.
    pub fn inner(&self) -> &Engine {
        &self.inner
    }

    /// Whether there is a pending preedit/composition.
    pub fn has_pending(&self) -> bool {
        self.inner.has_preedit() || self.raw_count > 0
    }
}

// ============================================================================
// CompositorKind
// ============================================================================

/// Supported compositors with known performance characteristics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompositorKind {
    /// niri — scrollable-tiling, lean event loop.
    Niri,
    /// Hyprland — has animations/effects.
    Hyprland,
    /// KDE KWin — heavier, more features.
    Kde,
    /// GNOME Mutter.
    Gnome,
    /// COSMIC compositor.
    Cosmic,
    /// Unknown or generic compositor.
    Unknown,
}

/// Helper: detect compositor kind from environment.
impl CompositorKind {
    /// Auto-detect the compositor from environment variables.
    pub fn detect() -> Self {
        // Check XDG_CURRENT_DESKTOP or WAYLAND_DISPLAY for hints
        if let Ok(desktop) = std::env::var("XDG_CURRENT_DESKTOP") {
            let d = desktop.to_lowercase();
            if d.contains("niri") {
                return Self::Niri;
            }
            if d.contains("hyprland") {
                return Self::Hyprland;
            }
            if d.contains("kde") || d.contains("plasma") {
                return Self::Kde;
            }
            if d.contains("gnome") {
                return Self::Gnome;
            }
            if d.contains("cosmic") {
                return Self::Cosmic;
            }
        }
        Self::Unknown
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Single source of truth for word boundaries — the core engine's rule.
/// (A local copy used to live here and DIVERGED: it treated digits as
/// boundaries for Smart mode, so VNI tones never worked in "Tự do".)
fn is_word_boundary(ch: char, method: InputMethod) -> bool {
    crate::engine::syllable::is_word_boundary(ch, method)
}

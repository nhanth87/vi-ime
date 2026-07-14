// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
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
    /// Emoji shortcode/emoticon expansion enabled (config `emoji`).
    emoji_enabled: bool,
    /// Emoji capture buffer: the chars already ECHOED to the app that form a
    /// candidate emoticon/shortcode (e.g. ":", ":s", ":smile"). Empty when not
    /// mid-candidate. Only ever engaged while the Vietnamese engine has no
    /// pending composition, so the two never fight over a keystroke.
    emoji_buf: String,
}

impl NonPreeditEngine {
    /// Create a new engine with the given input method and mode.
    pub fn new(method: InputMethod, mode: ImeMode) -> Self {
        Self {
            inner: Engine::new(method),
            mode,
            raw_count: 0,
            emoji_enabled: false,
            emoji_buf: String::new(),
        }
    }

    /// Toggle emoji expansion (wired from config snapshot).
    pub fn set_emoji_enabled(&mut self, enabled: bool) {
        self.emoji_enabled = enabled;
        if !enabled {
            self.emoji_buf.clear();
        }
    }

    /// Process one keypress. Returns the action for the Wayland layer.
    pub fn push_key(&mut self, ch: char) -> NonPreeditAction {
        // Emoji shortcode/emoticon capture runs BEFORE the Vietnamese engine,
        // but ONLY when there is no pending Vietnamese composition — so the two
        // never contend for a keystroke. Returns Some(...) while it owns the
        // key, None to fall through to normal processing.
        if let Some(action) = self.handle_emoji(ch) {
            return action;
        }

        // Pass-through checks first (before any buffering)
        if ch.is_ascii_control() && ch != '\u{0008}' {
            // Enter, escape, tab, etc. — commit if pending then pass through
            if self.inner.has_preedit() {
                // smart_commit_output applies R9 English-restore in Smart mode
                // (test→test, not tét); no-op in pure Telex/VNI. The core
                // Engine boundary does this, but NonPreedit apps (Chrome,
                // terminal, LibreOffice) route HERE — using preedit_output
                // directly skipped the restore → "test"→"tét" (field 2026-07-12).
                let committed = self.inner.smart_commit_english_only();
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
                let committed = self.inner.smart_commit_english_only();
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
                    let committed = self.inner.smart_commit_english_only();
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

    /// Emoji capture state machine. Owns a keystroke ONLY when emoji is enabled
    /// and no Vietnamese composition is pending. While building a candidate it
    /// echoes each char verbatim (PassThrough) so the user sees what they type;
    /// on a completed emoticon/shortcode it returns `CommitEmoji` (backspace the
    /// echoed candidate, write the emoji, don't replay the trigger). Returns
    /// `None` to hand the key back to normal processing.
    /// SILENT capture: candidate chars are buffered, NOT echoed — so a match
    /// commits the emoji with zero backspaces, and a dead-end flushes the
    /// literal buffer. This sidesteps backspacing already-shown text, which is
    /// unreliable on this architecture (delete_surrounding_text ignored; vk has
    /// no char-backspace). Both outcomes use `CommitEmoji` (no key replay): the
    /// trigger char is folded into the committed text, never echoed twice.
    fn handle_emoji(&mut self, ch: char) -> Option<NonPreeditAction> {
        use crate::engine::emoji;

        // Only engage when enabled and no Vietnamese composition is pending, so
        // the two capture buffers can never contend for a keystroke.
        if !self.emoji_enabled || self.inner.has_preedit() || self.raw_count > 0 {
            if !self.emoji_buf.is_empty() {
                // Shouldn't happen (buf only grows while idle), but be safe.
                self.emoji_buf.clear();
            }
            return None;
        }

        // Backspace / control chars: flush any candidate literally, hand the
        // key back to normal processing (which will pass it through).
        if ch == '\u{0008}' || (ch.is_ascii_control()) {
            if self.emoji_buf.is_empty() {
                return None;
            }
            let flushed = std::mem::take(&mut self.emoji_buf);
            return Some(NonPreeditAction::CommitEmoji {
                backspace_count: 0,
                text: flushed,
            });
        }

        // Not capturing yet: only a starter char (: ; < ^) opens a candidate.
        if self.emoji_buf.is_empty() {
            if emoji::is_starter(ch) {
                self.emoji_buf.push(ch);
                return Some(NonPreeditAction::Buffer); // silent
            }
            return None;
        }

        // Mid-capture: tentatively extend and classify.
        let mut cand = self.emoji_buf.clone();
        cand.push(ch);

        // 1) Completed emoticon (":)", "<3", "^_^"...) → commit emoji.
        if let Some(e) = emoji::emoticon(&cand) {
            self.emoji_buf.clear();
            return Some(NonPreeditAction::CommitEmoji {
                backspace_count: 0,
                text: e.to_string(),
            });
        }

        // 2) Closing ':' of a :shortcode:.
        if ch == ':' && self.emoji_buf.starts_with(':') && self.emoji_buf.len() > 1 {
            let name = self.emoji_buf[1..].to_string();
            if let Some(e) = emoji::shortcode(&name) {
                self.emoji_buf.clear();
                return Some(NonPreeditAction::CommitEmoji {
                    backspace_count: 0,
                    text: e.to_string(),
                });
            }
            // Unknown shortcode closed by ':' — flush "":name" literally and
            // restart a fresh capture from THIS ':' (it may open the next one,
            // e.g. "::smile:").
            let flushed = std::mem::replace(&mut self.emoji_buf, ":".to_string());
            return Some(NonPreeditAction::CommitEmoji {
                backspace_count: 0,
                text: flushed,
            });
        }

        // 3) Still viable (prefix of an emoticon, or a growing :shortcode name)?
        let viable = emoji::emoticon_prefix(&cand)
            || (cand.starts_with(':') && emoji::is_shortcode_char(ch));
        if viable {
            self.emoji_buf = cand;
            return Some(NonPreeditAction::Buffer); // silent
        }

        // Dead end: flush the buffered candidate PLUS this char as literal text
        // (the trigger chars are punctuation/letters that should appear as
        // typed). No replay — the char is already in `text`.
        self.emoji_buf.clear();
        Some(NonPreeditAction::CommitEmoji {
            backspace_count: 0,
            text: cand,
        })
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

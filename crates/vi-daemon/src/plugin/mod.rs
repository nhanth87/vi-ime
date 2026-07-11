// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 vi-im contributors
//! Plugin system for vi-ime.
//! Plugins form a middleware chain: ALL matching plugins process each key.
//! Global plugins (handles_app returns true for everything) run for every app.

use crate::engine::NonPreeditAction;

// ============================================================================
// AppPlugin trait v2
// ============================================================================

pub trait AppPlugin: Send + Sync {
    fn name(&self) -> &str;
    fn handles_app(&self, app_id: &str) -> bool;
    fn on_focus(&mut self, app_id: &str);
    fn on_blur(&mut self, app_id: &str);
    fn pre_process_key(&mut self, ch: char, system_mod: bool) -> Option<NonPreeditAction>;
    fn post_process_action(&mut self, action: NonPreeditAction) -> NonPreeditAction {
        action
    }
    fn recommended_mode(&self, _app_id: &str) -> Option<crate::engine::ImeMode> {
        None
    }
}

// ============================================================================
// PluginManager multi-plugin middleware chain
// ============================================================================

pub struct PluginManager {
    plugins: Vec<Box<dyn AppPlugin>>,
    current_app: Option<String>,
}

impl PluginManager {
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
            current_app: None,
        }
    }

    pub fn register(&mut self, plugin: Box<dyn AppPlugin>) {
        self.plugins.push(plugin);
    }

    pub fn on_focus_change(&mut self, app_id: &str) {
        if let Some(ref old) = self.current_app {
            for p in &mut self.plugins {
                if p.handles_app(old) {
                    p.on_blur(old);
                }
            }
        }
        self.current_app = Some(app_id.to_string());
        for p in &mut self.plugins {
            if p.handles_app(app_id) {
                tracing::debug!("[PLUGIN] {} on_focus({app_id})", p.name());
                p.on_focus(app_id);
            }
        }
    }

    pub fn pre_process_key(
        &mut self,
        ch: char,
        system_mod: bool,
        app_id: Option<&str>,
    ) -> Option<NonPreeditAction> {
        let app = app_id.unwrap_or("");
        for p in &mut self.plugins {
            if p.handles_app(app)
                && let Some(action) = p.pre_process_key(ch, system_mod)
            {
                return Some(action);
            }
        }
        None
    }

    pub fn post_process_action(
        &mut self,
        mut action: NonPreeditAction,
        app_id: Option<&str>,
    ) -> NonPreeditAction {
        let app = app_id.unwrap_or("");
        for p in &mut self.plugins {
            if p.handles_app(app) {
                action = p.post_process_action(action);
            }
        }
        action
    }

    pub fn recommended_mode(&self, app_id: &str) -> Option<crate::engine::ImeMode> {
        for p in &self.plugins {
            if p.handles_app(app_id)
                && let Some(m) = p.recommended_mode(app_id)
            {
                return Some(m);
            }
        }
        None
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Builtin Plugin 1: TerminalPlugin — auto NonPreedit for terminals
// ============================================================================

pub struct TerminalPlugin;
impl AppPlugin for TerminalPlugin {
    fn name(&self) -> &str {
        "TerminalPlugin"
    }
    fn handles_app(&self, id: &str) -> bool {
        // Single source of truth: compositor::KNOWN_TERMINALS.
        crate::compositor::AppCategory::classify(id) == crate::compositor::AppCategory::Terminal
    }
    fn on_focus(&mut self, _id: &str) {}
    fn on_blur(&mut self, _id: &str) {}
    fn pre_process_key(&mut self, _ch: char, _m: bool) -> Option<NonPreeditAction> {
        None
    }
    fn recommended_mode(&self, _app_id: &str) -> Option<crate::engine::ImeMode> {
        Some(crate::engine::ImeMode::NonPreedit)
    }
}

// ============================================================================
// Builtin Plugin 2: BrowserPlugin — auto Hybrid for browsers
// ============================================================================

pub struct BrowserPlugin;
impl AppPlugin for BrowserPlugin {
    fn name(&self) -> &str {
        "BrowserPlugin"
    }
    fn handles_app(&self, id: &str) -> bool {
        let id = id.to_lowercase();
        matches!(
            id.as_str(),
            "chromium-browser"
                | "chromium"
                | "google-chrome"
                | "google-chrome-stable"
                | "firefox"
                | "firefoxdeveloperedition"
                | "firefox-esr"
                | "firefox-nightly"
                | "brave-browser"
                | "brave"
                | "microsoft-edge"
                | "edge"
                | "opera"
                | "vivaldi-stable"
                | "zen-browser"
                | "zen"
        )
    }
    fn on_focus(&mut self, _id: &str) {}
    fn on_blur(&mut self, _id: &str) {}
    fn pre_process_key(&mut self, _ch: char, _m: bool) -> Option<NonPreeditAction> {
        None
    }
    fn recommended_mode(&self, _app_id: &str) -> Option<crate::engine::ImeMode> {
        Some(crate::engine::ImeMode::Preedit)
    }
}

// ============================================================================
// Builtin Plugin 3: ChromiumNiriPlugin — force NonPreedit on Chromium+Niri
// Prevents double-input bug
// ============================================================================

pub struct ChromiumNiriPlugin {
    is_niri: bool,
}
impl ChromiumNiriPlugin {
    pub fn new() -> Self {
        let is_niri = std::env::var("XDG_CURRENT_DESKTOP")
            .map(|d| d.to_lowercase().contains("niri"))
            .unwrap_or(false);
        Self { is_niri }
    }
}
impl AppPlugin for ChromiumNiriPlugin {
    fn name(&self) -> &str {
        "ChromiumNiriPlugin"
    }
    fn handles_app(&self, id: &str) -> bool {
        if !self.is_niri {
            return false;
        }
        let id = id.to_lowercase();
        id.contains("chrom")
            || id.contains("chrome")
            || id.contains("brave")
            || id.contains("edge")
            || id.contains("opera")
            || id.contains("vivaldi")
    }
    fn on_focus(&mut self, id: &str) {
        tracing::info!(
            "[ChromiumNiriPlugin] {}: forcing NonPreedit to prevent double-input",
            id
        );
    }
    fn on_blur(&mut self, _id: &str) {}
    fn pre_process_key(&mut self, _ch: char, _m: bool) -> Option<NonPreeditAction> {
        None
    }
    fn recommended_mode(&self, _app_id: &str) -> Option<crate::engine::ImeMode> {
        Some(crate::engine::ImeMode::NonPreedit)
    }
}

// ============================================================================
// Builtin Plugin 4: AutoCommitShortcutPlugin — commit pending text before shortcut
// ============================================================================

#[derive(Debug, Default)]
pub struct ShortcutState {
    pub has_pending: bool,
    pub pending_text: String,
    pub raw_count: usize,
}

pub struct AutoCommitShortcutPlugin {
    state: ShortcutState,
}
impl AutoCommitShortcutPlugin {
    pub fn new() -> Self {
        Self {
            state: ShortcutState::default(),
        }
    }
}
impl AppPlugin for AutoCommitShortcutPlugin {
    fn name(&self) -> &str {
        "AutoCommitShortcut"
    }
    fn handles_app(&self, _id: &str) -> bool {
        true
    }
    fn on_focus(&mut self, _id: &str) {}
    fn on_blur(&mut self, _id: &str) {
        self.state = ShortcutState::default();
    }
    fn pre_process_key(&mut self, _ch: char, system_mod: bool) -> Option<NonPreeditAction> {
        if system_mod && self.state.has_pending {
            let text = std::mem::take(&mut self.state.pending_text);
            let raw = self.state.raw_count;
            self.state.has_pending = false;
            self.state.raw_count = 0;
            tracing::info!(
                "[AutoCommitShortcut] auto-commit pending \"{}\" before shortcut",
                text
            );
            return Some(NonPreeditAction::CommitWithBackspace {
                backspace_count: raw,
                text,
            });
        }
        None
    }
}

// ============================================================================
// Builtin Plugin 5: ElectronFlagAdvisorPlugin — warn about missing Wayland flags
// ============================================================================

pub struct ElectronFlagAdvisorPlugin {
    warned: std::collections::HashSet<String>,
}
impl ElectronFlagAdvisorPlugin {
    pub fn new() -> Self {
        Self {
            warned: std::collections::HashSet::with_capacity(16),
        }
    }
}
impl AppPlugin for ElectronFlagAdvisorPlugin {
    fn name(&self) -> &str {
        "ElectronFlagAdvisor"
    }
    fn handles_app(&self, id: &str) -> bool {
        let id = id.to_lowercase();
        id.contains("discord")
            || id.contains("slack")
            || id.contains("teams")
            || id.contains("capcut")
            || id.contains("code")
            || id.contains("codium")
            || id.contains("vscode")
            || id.contains("sublime")
            || id.contains("subl")
            || id.contains("telegram")
            || id.contains("signal")
            || id.contains("element")
            || id.contains("spotify")
            || id.contains("notion")
            || id.contains("figma")
    }
    fn on_focus(&mut self, id: &str) {
        if self.warned.insert(id.to_string()) {
            tracing::warn!(
                "[ElectronFlagAdvisor] {} is Electron/Chromium. Add flags: --ozone-platform=wayland --enable-wayland-ime",
                id
            );
        }
    }
    fn on_blur(&mut self, _id: &str) {}
    fn pre_process_key(&mut self, _ch: char, _m: bool) -> Option<NonPreeditAction> {
        None
    }
}

// ============================================================================
// Extension point: AbbreviationPlugin trait (gõ tắt)
// ============================================================================

/// A plugin that expands abbreviations into longer text.
/// Designed for: gõ tắt (shorthand expansion), emoji shortcodes,
/// and custom vocabulary/phrases.
#[allow(dead_code)]
pub trait AbbreviationProvider: Send + Sync {
    /// Plugin name (for debugging/logging).
    fn name(&self) -> &str;
    /// Whether this provider is currently enabled.
    fn enabled(&self) -> bool;
    /// Given a completed word (raw keys), return the expansion if the word
    /// matches an abbreviation. Returns `None` if no match.
    fn expand(&self, word: &str) -> Option<String>;
    /// Reload the abbreviation dictionary (called on config change).
    fn reload(&mut self);
}

// ============================================================================
// Extension point: LanguagePlugin trait (tiếng dân tộc)
// ============================================================================

/// A plugin that provides an alternative input method for non-Vietnamese
/// languages. Designed for: Hmong, Tay, Ede, Cham, and 56 minority languages
/// of Vietnam, as well as other languages with diacritics.
///
/// When active, the LanguagePlugin intercepts ALL key processing (replaces
/// the Vietnamese engine path entirely). The daemon routes to the active
/// language plugin when the user selects it via config/shortcut.
#[allow(dead_code)]
pub trait LanguagePlugin: Send + Sync {
    /// Language identifier (e.g. "hmong", "tay", "ede").
    fn language_id(&self) -> &str;
    /// Human-readable language name (e.g. "Tiếng Hmong").
    fn language_name(&self) -> &str;
    /// Process one keypress. Returns the preedit/commit action.
    /// If `None`, fall through to the default Vietnamese engine.
    fn push_key(&mut self, ch: char) -> Option<NonPreeditAction>;
    /// Handle backspace during composition.
    fn backspace(&mut self) -> Option<NonPreeditAction>;
    /// Reset internal state (word boundary / focus change).
    fn reset(&mut self);
    /// Whether this plugin has pending (uncommitted) composition.
    fn has_pending(&self) -> bool;
    /// Get the current preedit string for display.
    fn preedit_string(&self) -> &str;
}

// ============================================================================
// AbbreviationManager — manages multiple abbreviation providers
// ============================================================================

#[allow(dead_code)]
pub struct AbbreviationManager {
    providers: Vec<Box<dyn AbbreviationProvider>>,
}

impl AbbreviationManager {
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
        }
    }

    pub fn register(&mut self, provider: Box<dyn AbbreviationProvider>) {
        self.providers.push(provider);
    }

    /// Try to expand a word through all registered providers (first match wins).
    pub fn expand(&self, word: &str) -> Option<String> {
        for p in &self.providers {
            if p.enabled() {
                if let Some(expansion) = p.expand(word) {
                    tracing::info!("[ABBREV] '{}' → '{}' (via {})", word, expansion, p.name());
                    return Some(expansion);
                }
            }
        }
        None
    }

    /// Reload all providers' dictionaries.
    pub fn reload_all(&mut self) {
        for p in &mut self.providers {
            p.reload();
        }
    }
}

impl Default for AbbreviationManager {
    fn default() -> Self {
        Self::new()
    }
}

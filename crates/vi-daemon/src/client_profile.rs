// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Per-client timing profiles for adaptive pacing.
//!
//! Different apps have different latency characteristics:
//! - LibreOffice VCL / OnlyOffice CEF need generous per-glyph pacing (20ms+)
//!   because their rendering pipeline lags behind the compositor event stream.
//! - Chromium under XWayland with static keymap is stable at moderate pacing.
//! - Native Wayland apps (terminals, text editors) can go fast (5ms floor).
//!
//! This module provides the SINGLE SOURCE OF TRUTH for app-classified timing,
//! consumed by both the evdev fallback typer and the Wayland VietTyper.

/// Timing parameters tuned per application class.
#[derive(Debug, Clone)]
pub struct ClientProfile {
    /// Human-readable label for logging.
    pub label: &'static str,
    /// Delay after each BackSpace key before the next event (ms).
    pub backspace_delay_ms: u64,
    /// Delay after each composed glyph (ms).
    pub glyph_delay_ms: u64,
    /// Extra settle before the FIRST glyph after any BackSpace (ms).
    /// Only applied when `backspaces > 0` in a `backspace_then_type` call.
    pub pre_first_glyph_delay_ms: u64,
    /// Single delay covering N batched BackSpaces (ms). 0 = per-BS pacing
    /// only (no batch). >0 = batch N BS into one burst then wait once.
    pub batch_delay_ms: u64,
    /// Whether this app can safely batch BackSpaces (LibreOffice VCL cannot —
    /// it swallows BS+char bursts whole; field-proven 2026-07-10).
    pub batch_safe: bool,
    /// Whether this app is known slow (needs roundtrip, not just flush).
    pub is_slow: bool,
    /// Whether this app needs the xdotool injector instead of native vk typer
    /// (OnlyOffice CEF embedding drops Mod3/Mod5, see R19).
    pub needs_injector: bool,
    /// Whether this app is a legacy app unreachable via zwp_input_method_v2.
    pub is_legacy: bool,
    /// Whether this app needs evdev fallback only under XWayland.
    pub xwayland_fallback: bool,
}

impl ClientProfile {
    /// Detect profile from app_id (case-insensitive prefix match).
    /// Returns `default()` for unknown apps.
    pub fn detect(app_id: &str) -> Self {
        let id = app_id.to_lowercase();

        // ── LibreOffice: VCL gtk3, one-shot enable(), swallows BS+char bursts ──
        if id.starts_with("libreoffice") || id.starts_with("soffice") {
            return Self {
                label: "LibreOffice (VCL, one-shot enable)",
                backspace_delay_ms: 20,
                glyph_delay_ms: 20,
                pre_first_glyph_delay_ms: 30,
                batch_delay_ms: 0,    // NEVER batch — VCL swallows bursts
                batch_safe: false,
                is_slow: true,
                needs_injector: false,
                is_legacy: true,
                xwayland_fallback: false,
            };
        }

        // ── OnlyOffice: X11/Qt + CEF child, needs xdotool + extra settle ──
        if id.starts_with("onlyoffice") {
            return Self {
                label: "OnlyOffice (X11/Qt + CEF, xdotool injector)",
                backspace_delay_ms: 20,
                glyph_delay_ms: 20,
                pre_first_glyph_delay_ms: 30,
                batch_delay_ms: 0,    // NEVER batch — CEF drops first char of burst
                batch_safe: false,
                is_slow: true,
                needs_injector: true,
                is_legacy: true,
                xwayland_fallback: false,
            };
        }

        // ── Chromium/Electron browsers under XWayland ──
        if XWAYLAND_BROWSERS.contains(&id.as_str())
            || XWAYLAND_BROWSERS.iter().any(|p| id.starts_with(p))
        {
            return Self {
                label: "Chromium browser (XWayland, static keymap OK)",
                backspace_delay_ms: 8,
                glyph_delay_ms: 10,
                pre_first_glyph_delay_ms: 20,
                batch_delay_ms: 15,
                batch_safe: true,
                is_slow: false,
                needs_injector: false,
                is_legacy: false,
                xwayland_fallback: true,
            };
        }

        // ── Native Wayland browsers ──
        if NATIVE_BROWSERS.contains(&id.as_str())
            || NATIVE_BROWSERS.iter().any(|p| id.starts_with(p))
        {
            return Self {
                label: "Browser (Wayland native)",
                backspace_delay_ms: 5,
                glyph_delay_ms: 5,
                pre_first_glyph_delay_ms: 15,
                batch_delay_ms: 10,
                batch_safe: true,
                is_slow: false,
                needs_injector: false,
                is_legacy: false,
                xwayland_fallback: false,
            };
        }

        // ── Terminals: fast ──
        if crate::compositor::KNOWN_TERMINALS.contains(&id.as_str()) {
            return Self {
                label: "Terminal (fast)",
                backspace_delay_ms: 3,
                glyph_delay_ms: 3,
                pre_first_glyph_delay_ms: 5,
                batch_delay_ms: 5,
                batch_safe: true,
                is_slow: false,
                needs_injector: false,
                is_legacy: false,
                xwayland_fallback: false,
            };
        }

        Self::default()
    }

    /// Default profile for unknown apps (conservative).
    pub fn default() -> Self {
        Self {
            label: "unknown (conservative default)",
            backspace_delay_ms: 8,
            glyph_delay_ms: 8,
            pre_first_glyph_delay_ms: 15,
            batch_delay_ms: 12,
            batch_safe: true,
            is_slow: false,
            needs_injector: false,
            is_legacy: false,
            xwayland_fallback: false,
        }
    }
}

/// Browser app_ids known to run under XWayland (Chromium, Brave, Edge, Opera, Vivaldi).
const XWAYLAND_BROWSERS: &[&str] = &[
    "google-chrome",
    "google-chrome-stable",
    "chromium",
    "chromium-browser",
    "brave-browser",
    "brave",
    "microsoft-edge",
    "opera",
    "vivaldi-stable",
    "vivaldi",
];

/// Browser app_ids known to support native Wayland (--ozone-platform=wayland).
const NATIVE_BROWSERS: &[&str] = &[
    "firefox",
    "firefoxdeveloperedition",
    "firefox-nightly",
    "org.mozilla.firefox",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_libreoffice_is_slow() {
        let p = ClientProfile::detect("libreoffice-writer");
        assert!(p.is_slow);
        assert!(p.is_legacy);
        assert!(!p.batch_safe);
        assert_eq!(p.backspace_delay_ms, 20);
    }

    #[test]
    fn detect_onlyoffice_needs_injector() {
        let p = ClientProfile::detect("onlyoffice-desktopeditors");
        assert!(p.needs_injector);
        assert!(p.is_slow);
        assert!(!p.batch_safe);
    }

    #[test]
    fn detect_chromium_xwayland() {
        let p = ClientProfile::detect("chromium-browser");
        assert!(p.xwayland_fallback);
        assert!(!p.is_slow);
        assert_eq!(p.backspace_delay_ms, 8);
    }

    #[test]
    fn detect_firefox_native() {
        let p = ClientProfile::detect("firefox");
        assert!(!p.xwayland_fallback);
        assert!(!p.is_slow);
        assert_eq!(p.backspace_delay_ms, 5);
    }

    #[test]
    fn detect_terminal_is_fast() {
        let p = ClientProfile::detect("kitty");
        assert!(!p.is_slow);
        assert!(p.batch_safe);
        assert_eq!(p.backspace_delay_ms, 3);
    }

    #[test]
    fn detect_unknown_is_default() {
        let p = ClientProfile::detect("some-unknown-app");
        assert!(!p.is_slow);
        assert_eq!(p.backspace_delay_ms, 8);
        assert_eq!(p.glyph_delay_ms, 8);
    }

    #[test]
    fn default_profile_is_conservative() {
        let p = ClientProfile::default();
        assert_eq!(p.backspace_delay_ms, 8);
        assert_eq!(p.glyph_delay_ms, 8);
        assert!(p.batch_safe);
    }

    #[test]
    fn case_insensitive_match() {
        let p = ClientProfile::detect("LibreOffice-Writer");
        assert!(p.is_legacy);
    }
}

    // ── Batch safety tests ──

    #[test]
    fn batch_safe_apps_can_batch() {
        // Terminal and Chromium XWayland are batch-safe
        assert!(ClientProfile::detect("kitty").batch_safe);
        assert!(ClientProfile::detect("chromium").batch_safe);
        assert!(ClientProfile::detect("firefox").batch_safe);
        assert!(ClientProfile::default().batch_safe);
    }

    #[test]
    fn slow_apps_cannot_batch() {
        assert!(!ClientProfile::detect("libreoffice-writer").batch_safe);
        assert!(!ClientProfile::detect("onlyoffice-desktopeditors").batch_safe);
        assert!(!ClientProfile::detect("soffice").batch_safe);
    }

    #[test]
    fn slow_apps_have_higher_delays() {
        let lo = ClientProfile::detect("libreoffice");
        let def = ClientProfile::default();
        assert!(lo.backspace_delay_ms > def.backspace_delay_ms);
        assert!(lo.glyph_delay_ms > def.glyph_delay_ms);
        assert!(lo.pre_first_glyph_delay_ms > def.pre_first_glyph_delay_ms);
    }

    #[test]
    fn terminal_is_fastest() {
        let term = ClientProfile::detect("foot");
        let def = ClientProfile::default();
        assert!(term.backspace_delay_ms <= def.backspace_delay_ms);
        assert!(term.glyph_delay_ms <= def.glyph_delay_ms);
        assert!(term.pre_first_glyph_delay_ms <= def.pre_first_glyph_delay_ms);
    }

    #[test]
    fn xwayland_browsers_have_fallback_flag() {
        for id in &["google-chrome", "chromium-browser", "brave", "vivaldi"] {
            let p = ClientProfile::detect(id);
            assert!(p.xwayland_fallback, "{id} must have xwayland_fallback=true");
            assert!(!p.is_legacy, "{id} must not be legacy");
        }
    }

    #[test]
    fn firefox_wayland_not_xwayland() {
        let p = ClientProfile::detect("firefox");
        assert!(!p.xwayland_fallback);
        assert!(p.batch_safe);
    }

    #[test]
    fn onlyoffice_needs_injector() {
        let p = ClientProfile::detect("onlyoffice-desktopeditors");
        assert!(p.needs_injector);
        assert!(p.is_legacy);
        assert!(p.is_slow);
    }

    #[test]
    fn profile_is_cloneable() {
        let p = ClientProfile::detect("kitty");
        let p2 = p.clone();
        assert_eq!(p2.backspace_delay_ms, p.backspace_delay_ms);
        assert_eq!(p2.label, p.label);
    }

    #[test]
    fn all_known_terminals_are_fast_and_batch_safe() {
        // Spot-check a subset of KNOWN_TERMINALS
        for id in &["kitty", "foot", "alacritty", "wezterm", "com.mitchellh.ghostty"] {
            let p = ClientProfile::detect(id);
            assert!(p.batch_safe, "{id} must be batch_safe");
            assert!(!p.is_slow, "{id} must not be slow");
            assert_eq!(p.backspace_delay_ms, 3, "{id} BS delay");
            assert_eq!(p.glyph_delay_ms, 3, "{id} glyph delay");
        }
    }

    #[test]
    fn profile_debug_format_includes_label() {
        let p = ClientProfile::detect("libreoffice");
        let debug = format!("{p:?}");
        assert!(debug.contains("LibreOffice"));
    }

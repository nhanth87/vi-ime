// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 vi-im contributors
//! Compositor IPC — track active window for per-app IME configuration.
//!
//! Provides a common trait for detecting the currently focused application
//! on different Wayland compositors (Hyprland, Niri, COSMIC).
//! Also classifies apps into categories for automatic IME mode selection.

mod niri;
pub mod probe;
mod wlr_toplevel;

pub use niri::spawn_niri_event_stream;
pub use wlr_toplevel::spawn_wlr_toplevel_stream;

/// The currently focused window: app_id plus title.
/// Title is used for per-site IME rules inside browsers.
/// `pid` (when the compositor IPC provides it — niri does, the wlr
/// foreign-toplevel protocol does not) enables /proc inspection
/// (Electron flag advisor).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FocusEvent {
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub pid: Option<i32>,
}

/// Terminal emulator app_ids — the SINGLE source of truth (lowercase, as
/// reported over Wayland `xdg_toplevel.app_id` / `wl_surface`). Consumed by
/// [`AppCategory::classify`] (this file), `plugin::TerminalPlugin`
/// (routes NonPreedit + swallows the ContentType=Terminal signal) and
/// `config::builtin::builtin_app_profile` (day-one NonPreedit default) —
/// keeping it in exactly one place means adding a terminal here is enough,
/// no need to touch three files in lockstep.
///
/// Comprehensive on purpose: every terminal here gets NonPreedit by
/// default, and terminals are consistently the app category where preedit
/// underline support is worst across the Wayland ecosystem (most either
/// don't implement text-input-v3 at all, or implement it without
/// `delete_surrounding_text`, which is what preedit-everywhere depends on
/// for the live-diff word). Getting a NEW terminal wrong just means an
/// underline the user didn't ask for — getting it right on day one avoids
/// the exact "chữ chồng nhau" class of bug this project has chased
/// repeatedly. Case-insensitive: `classify`/`builtin_app_profile` both
/// lowercase before matching, so list entries stay lowercase here.
pub const KNOWN_TERMINALS: &[&str] = &[
    // ── wlroots-native / GPU-accelerated ──
    "foot", "footclient",
    "kitty",
    "alacritty",
    "wezterm", "wezterm-gui", "org.wezfurlong.wezterm",
    "com.mitchellh.ghostty",
    "rio",
    "contour",
    "wayst",
    // ── DE-integrated ──
    "konsole", "org.kde.konsole",
    "gnome-terminal", "gnome-terminal-server", "org.gnome.terminal",
    "org.gnome.ptyxis",        // Ptyxis — new GNOME default, replacing gnome-terminal
    "xfce4-terminal",
    "mate-terminal",
    "lxterminal", "qterminal",
    "deepin-terminal",
    "io.elementary.terminal",
    "com.gexperts.tilix",
    "com.raggesilver.blackbox", // Blackbox
    "org.codeberg.dnkl.foot",   // foot, some distros package it under this id
    // ── Drop-down / quake-style ──
    "guake", "yakuake", "tilda",
    // ── Classic / X11 (via XWayland — still hit input-method-v2 through the
    //    compositor's Xwayland bridge on niri/Sway/Hyprland) ──
    "xterm", "uxterm", "urxvt", "rxvt", "st", "eterm",
    // ── Feature-rich / other ──
    "terminator",
    "terminology",
    "sakura",
    "termite",
    "tabby",
    "warp", "dev.warp.warp",
    "hyper",
    "cool-retro-term",
];

/// Application category for automatic IME mode selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppCategory {
    Terminal,
    Browser,
    Editor,
    Chat,
    Other,
}

impl AppCategory {
    /// Classify an app_id into a category.
    pub fn classify(app_id: &str) -> Self {
        let id = app_id.to_lowercase();
        if KNOWN_TERMINALS.contains(&id.as_str()) {
            return AppCategory::Terminal;
        }
        if matches!(id.as_str(),
            "chromium-browser" | "chromium" | "google-chrome" | "google-chrome-stable"
            | "firefox" | "firefoxdeveloperedition" | "firefox-esr" | "firefox-nightly"
            | "brave-browser" | "brave" | "microsoft-edge" | "edge" | "opera" | "vivaldi-stable"
            | "zen-browser" | "zen") {
            return AppCategory::Browser;
        }
        if matches!(id.as_str(),
            "code" | "code-oss" | "code-insiders" | "codium" | "vscode"
            | "jetbrains-idea" | "jetbrains-idea-ce" | "jetbrains-clion" | "jetbrains-pycharm"
            | "jetbrains-goland" | "jetbrains-rustrover" | "jetbrains-webstorm"
            | "sublime_text" | "subl" | "emacs" | "gedit" | "gnome-text-editor" | "kate"
            | "org.gnome.gedit" | "org.gnome.TextEditor") {
            return AppCategory::Editor;
        }
        if matches!(id.as_str(),
            "discord" | "discord-canary" | "slack" | "teams" | "microsoft teams"
            | "telegram-desktop" | "org.telegram.desktop" | "signal" | "element" | "ferdium" | "franz") {
            return AppCategory::Chat;
        }
        AppCategory::Other
    }
}



// SPDX-License-Identifier: GPL-3.0-or-later OR Commercial
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
        if matches!(id.as_str(),
            "foot" | "footclient" | "kitty" | "alacritty" | "wezterm" | "wezterm-gui"
            | "org.wezfurlong.wezterm" | "terminator" | "gnome-terminal" | "konsole"
            | "xfce4-terminal" | "com.mitchellh.ghostty" | "rio" | "warp" | "tabby") {
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



// SPDX-License-Identifier: GPL-3.0-or-later OR Commercial
// Copyright (c) 2024-2026 vi-im contributors
//! System-tray icon (StatusNotifierItem), fcitx-style. Runs in its own thread
//! (ksni spawns a DBus loop), shows the live method/on-off, and offers a menu:
//! switch method, toggle IME, open the QML config window, quit.
//!
//! Menu actions mutate `setting.conf` through `ConfigManager` — the daemon
//! already watches that file (inotify) and live-reloads, so the tray needs no
//! direct channel into the IME thread. The floating config window is the
//! separate `vi-settings` QML launcher (kept intact — the tray only spawns it).

use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Duration;

use tracing::{info, warn};

use crate::config::{ConfigManager, ImeMode, InputMethod};

/// Tray state source: reads/writes the on-disk config the daemon watches.
pub struct ViTray {
    config_path: PathBuf,
    /// Path to the `vi-settings` launcher (QML config window), if found.
    settings_exe: Option<PathBuf>,
    /// Live method — set in activate callback, ksni re-reads menu() after.
    current_method: InputMethod,
    current_mode: ImeMode,
    current_enabled: bool,
}

impl ViTray {
    /// Persist method to config and update live state.
    /// ksni re-reads menu() after the activate callback returns,
    /// so the ✓ mark updates immediately.
    fn set_method(&mut self, method: InputMethod) {
        self.current_method = method;
        if let Ok(mut m) = ConfigManager::new(Some(self.config_path.clone())) {
            m.setting_mut().input_method = method;
            let _ = m.save();
            info!("[TRAY] method → {method}");
        }
    }

    /// Persist display mode (Preedit/NonPreedit) — same flow as set_method.
    fn set_mode(&mut self, mode: ImeMode) {
        self.current_mode = mode;
        if let Ok(mut m) = ConfigManager::new(Some(self.config_path.clone())) {
            m.setting_mut().ime_mode = mode;
            let _ = m.save();
            info!("[TRAY] ime_mode → {mode:?}");
        }
    }

    fn toggle(&mut self) {
        self.current_enabled = !self.current_enabled;
        if let Ok(mut m) = ConfigManager::new(Some(self.config_path.clone())) {
            m.setting_mut().enabled = self.current_enabled;
            let _ = m.save();
            info!("[TRAY] IME {}", if self.current_enabled { "enabled" } else { "disabled" });
        }
    }

    fn open_settings(&self) {
        match &self.settings_exe {
            Some(exe) => {
                if let Err(e) = Command::new(exe).spawn() {
                    warn!("[TRAY] cannot launch settings {exe:?}: {e}");
                }
            }
            None => warn!("[TRAY] vi-settings launcher not found next to binary"),
        }
    }
}

impl ksni::Tray for ViTray {
    fn id(&self) -> String {
        "vi-im".into()
    }

    fn icon_name(&self) -> String {
        // Themed keyboard glyph; hosts that lack it fall back to a generic icon.
        "input-keyboard".into()
    }

    fn title(&self) -> String {
        format!("vi-im · {} · {}", self.current_method, if self.current_enabled { "Bật" } else { "Tắt" })
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: format!("vi-im · {}", self.current_method),
            description: if self.current_enabled { "Đang bật".into() } else { "Đang tắt".into() },
            icon_name: "input-keyboard".into(),
            icon_pixmap: Vec::new(),
        }
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::{MenuItem, StandardItem};
        // ksni re-reads menu() after an activate callback returns.
        // Mutating self.current_method in the callback propagates ✓.
        let cur = self.current_method;
        let cur_mode = self.current_mode;
        vec![
            method_item("  Telex", cur, InputMethod::Telex),
            method_item("  VNI",   cur, InputMethod::Vni),
            method_item("  Tự do", cur, InputMethod::Smart),
            MenuItem::Separator,
            mode_item("  Preedit (gạch chân)", cur_mode, ImeMode::Preedit),
            mode_item("  NonPreedit (gõ thẳng)", cur_mode, ImeMode::NonPreedit),
            MenuItem::Separator,
            StandardItem {
                label: if self.current_enabled {
                    "🟢 Đang bật  ·  nhấn để tắt".into()
                } else {
                    "🔴 Đang tắt  ·  nhấn để bật".into()
                },
                activate: Box::new(|t: &mut Self| t.toggle()),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Cài đặt…".into(),
                activate: Box::new(|t: &mut Self| t.open_settings()),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Thoát vi-im".into(),
                activate: Box::new(|_| std::process::exit(0)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

fn method_item(label: &str, cur: InputMethod, method: InputMethod) -> ksni::MenuItem<ViTray> {
    use ksni::menu::StandardItem;
    let checked = if cur == method { " ✓" } else { "" };
    StandardItem {
        label: format!("{label}{checked}"),
        activate: Box::new(move |t: &mut ViTray| t.set_method(method)),
        ..Default::default()
    }
    .into()
}

fn mode_item(label: &str, cur: ImeMode, mode: ImeMode) -> ksni::MenuItem<ViTray> {
    use ksni::menu::StandardItem;
    let checked = if cur == mode { " ✓" } else { "" };
    StandardItem {
        label: format!("{label}{checked}"),
        activate: Box::new(move |t: &mut ViTray| t.set_mode(mode)),
        ..Default::default()
    }
    .into()
}

/// Read the tray-relevant trio from disk.
fn read_state(path: &PathBuf) -> (InputMethod, ImeMode, bool) {
    ConfigManager::new(Some(path.clone()))
        .map(|m| {
            let s = m.setting();
            (s.input_method, s.ime_mode, s.enabled)
        })
        .unwrap_or((InputMethod::Telex, ImeMode::Preedit, true))
}

/// Register the tray (fcitx-style). Non-fatal: if no StatusNotifierHost is
/// running (bar without tray support), this simply shows no icon — the IME
/// keeps working. `settings_exe` is the vi-settings launcher for the menu.
pub fn spawn(config_path: PathBuf, settings_exe: Option<PathBuf>) {
    let (current_method, current_mode, current_enabled) = read_state(&config_path);
    let tray = ViTray {
        config_path: config_path.clone(),
        settings_exe,
        current_method,
        current_mode,
        current_enabled,
    };
    let service = ksni::TrayService::new(tray);
    let handle = service.handle();
    service.spawn();

    // Reverse sync: the tray's own clicks update it via ksni's re-read, but
    // changes from the settings GUI / CLI / file edits only land on disk.
    // Poll the config and push differences into ksni (handle.update emits
    // the DBus layout change → ✓ marks and title follow live).
    thread::spawn(move || {
        let mut last = read_state(&config_path);
        loop {
            thread::sleep(Duration::from_secs(2));
            let now = read_state(&config_path);
            if now != last {
                last = now;
                handle.update(|t: &mut ViTray| {
                    t.current_method = now.0;
                    t.current_mode = now.1;
                    t.current_enabled = now.2;
                });
            }
        }
    });
    info!("[TRAY] StatusNotifierItem registered (fcitx-style tray + menu)");
}

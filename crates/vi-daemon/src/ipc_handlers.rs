// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Daemon-side IPC command handlers (called from the main loop), split from
//! ipc.rs (R4). Read handlers build a response from live state; write
//! handlers mutate `ConfigManager` and persist.

use tracing::info;

use super::{AppEntry, IpcCommand, IpcResponse, LearnedEntry};
use crate::config::{ImeMode, InputMethod, OutputMode, ToneStyle};

/// Handle a read command — builds response from live state.
pub fn handle_read_command(
    cmd: &IpcCommand,
    setting: &crate::config::Setting,
    learned_store: &crate::config::LearnedStore,
    config_path: &std::path::Path,
) -> IpcResponse {
    match cmd {
        IpcCommand::GetConfig => IpcResponse {
            input_method: Some(setting.input_method.to_string()),
            tone_style: Some(tone_style_key(setting.tone_style).into()),
            enabled: Some(setting.enabled),
            ime_mode: Some(setting.ime_mode.to_string()),
            output_mode: Some(output_mode_key(setting.output_mode).into()),
            free_tone_placement: Some(setting.free_tone_placement),
            auto_detect_lang: Some(setting.auto_detect_lang),
            enable_per_app: Some(setting.enable_per_app),
            emoji: Some(setting.emoji),
            predict: Some(setting.predict),
            macros: Some(setting.macros.clone()),
            macro_hotkeys: Some(setting.macro_hotkeys.clone()),
            config_path: Some(config_path.display().to_string()),
            version: Some(env!("CARGO_PKG_VERSION").into()),
            ..IpcResponse::default()
        },
        IpcCommand::ListApps => {
            use crate::client_profile::ClientProfile;
            let entry = |id: &str, name: &str, builtin: bool| {
                let cfg = setting.app_configs.get(id).cloned().unwrap_or_default();
                let d = ClientProfile::detect(id);
                AppEntry {
                    app_id: id.to_string(),
                    app_name: name.to_string(),
                    method: cfg.input_method.map(|m| m.to_string())
                        .unwrap_or_else(|| "Mặc định".into()),
                    ime_mode: cfg.ime_mode.map(|m| m.to_string())
                        .unwrap_or_else(|| "Mặc định".into()),
                    enabled: cfg.enabled != Some(false),
                    icon: app_icon(id),
                    builtin,
                    profile: d.label.to_string(),
                    typing_delay_ms: cfg.typing_delay_ms,
                    bs_settle_ms: cfg.bs_settle_ms,
                    accent_settle_ms: cfg.accent_settle_ms,
                    default_typing_delay_ms: d.xtest_delay_ms,
                    default_bs_settle_ms: d.xtest_pre_glyph_ms,
                    default_accent_settle_ms: d.accent_settle_ms,
                }
            };
            // Built-in profiles first (always listed, with their defaults),
            // then every other app the user configured.
            let mut apps: Vec<AppEntry> = ClientProfile::BUILTIN_APPS
                .iter()
                .map(|(id, name)| entry(id, name, true))
                .collect();
            let mut user: Vec<AppEntry> = setting.app_configs.keys()
                .filter(|id| !ClientProfile::BUILTIN_APPS.iter().any(|(b, _)| b == id))
                .map(|id| entry(id, id, false))
                .collect();
            user.sort_by(|a, b| a.app_name.cmp(&b.app_name));
            apps.extend(user);
            IpcResponse { apps: Some(apps), ..IpcResponse::default() }
        }
        IpcCommand::GetLearned => {
            let learned: Vec<LearnedEntry> = learned_store.apps.iter()
                .map(|(id, p)| LearnedEntry {
                    app_id: id.clone(),
                    surrounding_text: p.surrounding_text,
                    ime_activated: p.ime_activated,
                }).collect();
            IpcResponse { learned: Some(learned), ..IpcResponse::default() }
        }
        _ => IpcResponse::error("Unexpected read command".into()),
    }
}

/// Handle a write command — mutates config and persists.
pub fn handle_write_command(
    cmd: &IpcCommand,
    config_manager: &mut crate::config::ConfigManager,
) -> IpcResponse {
    let s = config_manager.setting_mut();
    match cmd {
        IpcCommand::SetConfig {
            input_method, tone_style, enabled, ime_mode, output_mode,
            free_tone_placement, auto_detect_lang, enable_per_app, emoji, predict,
        } => {
            if let Some(m) = input_method {
                s.input_method = parse_method(m).unwrap_or(InputMethod::Telex);
            }
            if let Some(ts) = tone_style {
                s.tone_style = match ts.as_str() {
                    "modern" => ToneStyle::Modern,
                    _ => ToneStyle::Classic,
                };
            }
            if let Some(en) = *enabled { s.enabled = en; }
            if let Some(im) = ime_mode {
                s.ime_mode = parse_mode(im).unwrap_or(ImeMode::Preedit);
            }
            if let Some(om) = output_mode {
                s.output_mode = match om.as_str() {
                    "unicodetohop" => OutputMode::UnicodeToHop,
                    _ => OutputMode::UnicodeDungSan,
                };
            }
            if let Some(v) = *free_tone_placement { s.free_tone_placement = v; }
            if let Some(v) = *auto_detect_lang { s.auto_detect_lang = v; }
            if let Some(v) = *enable_per_app { s.enable_per_app = v; }
            if let Some(v) = *emoji { s.emoji = v; }
            if let Some(v) = *predict { s.predict = v; }
            info!("IPC: config updated via settings UI");
        }
        IpcCommand::AddApp {
            app_id, method, ime_mode, enabled, typing_delay_ms, bs_settle_ms, accent_settle_ms,
        } => {
            let entry = s.app_configs.entry(app_id.clone()).or_default();
            if let Some(m) = method { entry.input_method = parse_method(m); }
            if let Some(im) = ime_mode { entry.ime_mode = parse_mode(im); }
            entry.typing_delay_ms = *typing_delay_ms;
            entry.bs_settle_ms = *bs_settle_ms;
            entry.accent_settle_ms = *accent_settle_ms;
            // R18: only ever store an override that turns Vietnamese OFF.
            if let Some(en) = *enabled { entry.enabled = (!en).then_some(false); }
            if *entry == crate::config::AppConfig::default() {
                s.app_configs.remove(app_id);
            }
            info!("IPC: app config updated for {app_id}");
        }
        IpcCommand::RemoveApp { app_id } => {
            s.app_configs.remove(app_id);
            info!("IPC: app config removed for {app_id}");
        }
        IpcCommand::SetMacro { key, value } => {
            let key = key.trim();
            if key.is_empty() || value.is_empty() {
                return IpcResponse::error("Macro cần cả từ khoá và nội dung".into());
            }
            s.macros.insert(key.to_string(), value.clone());
            info!("IPC: macro set for {key:?}");
        }
        IpcCommand::RemoveMacro { key } => {
            s.macros.remove(key);
            info!("IPC: macro removed for {key:?}");
        }
        IpcCommand::SetMacroHotkey { key, value } => {
            // Stored lowercase + trimmed so the same chord can't appear twice
            // under different spellings ("Ctrl+Alt+M" vs "ctrl+alt+m").
            let key = key.trim().to_ascii_lowercase();
            if value.is_empty() {
                return IpcResponse::error("Phím tắt cần nội dung".into());
            }
            if crate::engine::macro_dict::parse_hotkey(&key).is_none() {
                return IpcResponse::error(format!(
                    "Tổ hợp {key:?} không hợp lệ — cần ctrl/alt/super + một phím, ví dụ ctrl+alt+m"
                ));
            }
            s.macro_hotkeys.insert(key.clone(), value.clone());
            info!("IPC: hotkey macro set for {key:?}");
        }
        IpcCommand::RemoveMacroHotkey { key } => {
            s.macro_hotkeys.remove(key);
            info!("IPC: hotkey macro removed for {key:?}");
        }
        _ => return IpcResponse::error("Unexpected write command".into()),
    }
    match config_manager.save() {
        Ok(()) => IpcResponse::reload(),
        Err(e) => IpcResponse::error(format!("Save: {e}")),
    }
}

/// Label → method. The GUI sends the display label ("Tự do"); without that
/// alias picking Tự do used to be saved as Telex. Unknown/"Mặc định" → None.
fn parse_method(m: &str) -> Option<InputMethod> {
    match m {
        "Telex" | "telex" => Some(InputMethod::Telex),
        "VNI" | "Vni" | "vni" => Some(InputMethod::Vni),
        "Tự do" | "tự do" | "Smart" | "smart" => Some(InputMethod::Smart),
        _ => None,
    }
}

fn parse_mode(m: &str) -> Option<ImeMode> {
    match m {
        "Preedit" | "preedit" => Some(ImeMode::Preedit),
        "NonPreedit" | "nonpreedit" => Some(ImeMode::NonPreedit),
        _ => None,
    }
}

fn tone_style_key(t: ToneStyle) -> &'static str {
    match t {
        ToneStyle::Classic => "classic",
        ToneStyle::Modern => "modern",
    }
}

/// Same spelling as `setting.conf` (serde lowercase).
fn output_mode_key(o: OutputMode) -> &'static str {
    match o {
        OutputMode::UnicodeDungSan => "unicodedungsan",
        OutputMode::UnicodeToHop => "unicodetohop",
    }
}

/// Heuristic icon for well-known app IDs.
#[allow(clippy::if_same_then_else)]
fn app_icon(app_id: &str) -> String {
    let lower = app_id.to_lowercase();
    if lower.contains("firefox") || lower.contains("zen") { "🦊".into() }
    else if lower.contains("chrome") || lower.contains("chromium") { "🌐".into() }
    else if lower.contains("code") || lower.contains("vscode") { "💻".into() }
    else if lower.contains("terminal") || lower.contains("kitty") { "⬛".into() }
    else if lower.contains("alacritty") { "⬛".into() }
    else if lower.contains("steam") { "🎮".into() }
    else if lower.contains("spotify") { "🎵".into() }
    else if lower.contains("slack") || lower.contains("discord") { "💬".into() }
    else { "📱".into() }
}

// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 vi-im contributors
//! Unix socket IPC server — phục vụ vi-settings QuickShell client.
//!
//! Giao thức: JSON lines qua Unix stream socket. Mỗi request là một dòng JSON,
//! response là một dòng JSON. Single-connection model.
//!
//! Tích hợp vào unified event loop (R15): IPC thread blocks trên `UnixListener`,
//! đẩy commands vào `DaemonEvent::Ipc*` channel. Không polling, không timer.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc::Sender;

use serde::{Deserialize, Serialize};
use tracing::{error, info};

use crate::config::{InputMethod, ImeMode, ToneStyle};
use crate::events::DaemonEvent;

// ── JSON Protocol types ─────────────────────────────────────────────────

/// Incoming command from vi-settings QML client.
#[derive(Debug, Deserialize)]
#[serde(tag = "cmd")]
pub enum IpcCommand {
    #[serde(rename = "get_config")]
    GetConfig,
    #[serde(rename = "set_config")]
    SetConfig {
        #[serde(default)]
        input_method: Option<String>,
        #[serde(default)]
        tone_style: Option<String>,
        #[serde(default)]
        enabled: Option<bool>,
        #[serde(default)]
        ime_mode: Option<String>,
    },
    #[serde(rename = "list_apps")]
    ListApps,
    #[serde(rename = "add_app")]
    AddApp {
        app_id: String,
        #[serde(default)]
        method: Option<String>,
        #[serde(default)]
        ime_mode: Option<String>,
    },
    #[serde(rename = "remove_app")]
    RemoveApp { app_id: String },
    #[serde(rename = "get_learned")]
    GetLearned,
}

/// Response gửi về QML client.
#[derive(Debug, Serialize, Clone)]
pub struct IpcResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tone_style: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ime_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apps: Option<Vec<AppEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub learned: Option<Vec<LearnedEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reload: Option<bool>,
}

#[derive(Debug, Serialize, Clone)]
pub struct AppEntry {
    pub app_id: String,
    pub app_name: String,
    pub method: String,
    pub ime_mode: String,
    pub icon: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct LearnedEntry {
    pub app_id: String,
    pub surrounding_text: Option<bool>,
    pub ime_activated: Option<bool>,
}

// ── IPC thread launcher ──────────────────────────────────────────────────

/// Đường dẫn mặc định cho IPC socket.
pub fn default_socket_path() -> PathBuf {
    let base = if let Ok(xdg) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(xdg)
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".local").join("share")
    } else {
        PathBuf::from("/tmp")
    };
    base.join("vi-ime").join("ipc.sock")
}

/// Spawn the IPC server thread. Blocks on UnixListener::incoming();
/// each accepted connection is handled inline (single-connection model).
/// Incoming commands are forwarded into the daemon's unified event bus.
pub fn spawn_ipc_server(
    tx: Sender<DaemonEvent>,
    socket_path: Option<PathBuf>,
) -> std::thread::JoinHandle<()> {
    let path = socket_path.unwrap_or_else(default_socket_path);

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    std::thread::Builder::new()
        .name("vi-im-ipc".into())
        .spawn(move || {
            let _ = std::fs::remove_file(&path);
            let listener = match UnixListener::bind(&path) {
                Ok(l) => l,
                Err(e) => {
                    error!("IPC server: bind({path:?}) failed: {e}");
                    return;
                }
            };
            info!("IPC server listening on {path:?}");

            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => handle_client(stream, &tx),
                    Err(e) => error!("IPC accept error: {e}"),
                }
            }
            let _ = std::fs::remove_file(&path);
        })
        .expect("Failed to spawn IPC thread")
}

/// Process all requests from one client connection.
fn handle_client(mut stream: UnixStream, tx: &Sender<DaemonEvent>) {
    let reader_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => { error!("IPC: clone stream: {e}"); return; }
    };
    let mut reader = BufReader::new(reader_stream);
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => { error!("IPC read: {e}"); break; }
        }
        let trimmed = line.trim();
        if trimmed.is_empty() { continue; }

        let cmd: IpcCommand = match serde_json::from_str(trimmed) {
            Ok(c) => c,
            Err(e) => {
                let resp = IpcResponse {
                    error: Some(format!("Parse: {e}")),
                    input_method: None, tone_style: None, enabled: None,
                    ime_mode: None, apps: None, learned: None, reload: None,
                };
                let _ = writeln!(stream, "{}", serde_json::to_string(&resp).unwrap());
                continue;
            }
        };

        match &cmd {
            IpcCommand::GetConfig | IpcCommand::ListApps | IpcCommand::GetLearned => {
                let (resp_tx, resp_rx) = std::sync::mpsc::channel();
                if tx.send(DaemonEvent::IpcRead { command: cmd, reply: resp_tx }).is_err() {
                    break;
                }
                match resp_rx.recv() {
                    Ok(resp) => {
                        let _ = writeln!(stream, "{}", serde_json::to_string(&resp).unwrap());
                    }
                    Err(_) => break,
                }
            }
            IpcCommand::SetConfig { .. } | IpcCommand::AddApp { .. } | IpcCommand::RemoveApp { .. } => {
                if tx.send(DaemonEvent::IpcWrite { command: cmd }).is_err() {
                    break;
                }
                let resp = IpcResponse {
                    reload: Some(true),
                    input_method: None, tone_style: None, enabled: None,
                    ime_mode: None, apps: None, learned: None, error: None,
                };
                let _ = writeln!(stream, "{}", serde_json::to_string(&resp).unwrap());
            }
        }
    }
}




// ── Daemon-side command handlers (called from main loop) ────────────────

/// Handle a read command — builds response from live state.
pub fn handle_read_command(
    cmd: &IpcCommand,
    setting: &crate::config::Setting,
    learned_store: &crate::config::LearnedStore,
) -> IpcResponse {
    let nil = || IpcResponse {
        input_method: None, tone_style: None, enabled: None, ime_mode: None,
        apps: None, learned: None, reload: None, error: None,
    };
    match cmd {
        IpcCommand::GetConfig => {
            let mut r = nil();
            r.input_method = Some(setting.input_method.to_string());
            r.tone_style = Some(match setting.tone_style {
                ToneStyle::Classic => "classic".into(),
                ToneStyle::Modern => "modern".into(),
            });
            r.enabled = Some(setting.enabled);
            r.ime_mode = Some(match setting.ime_mode {
                ImeMode::Preedit => "Preedit".into(),
                ImeMode::NonPreedit => "NonPreedit".into(),
            });
            r
        }
        IpcCommand::ListApps => {
            let mut apps: Vec<AppEntry> = setting.app_configs.iter()
                .map(|(id, cfg)| AppEntry {
                    app_id: id.clone(),
                    app_name: id.clone(),
                    method: cfg.input_method.map(|m| m.to_string())
                        .unwrap_or_else(|| "Mặc định".into()),
                    ime_mode: cfg.ime_mode.map(|m| m.to_string())
                        .unwrap_or_else(|| "Mặc định".into()),
                    icon: app_icon(id),
                }).collect();
            apps.sort_by(|a, b| a.app_name.cmp(&b.app_name));
            let mut r = nil();
            r.apps = Some(apps);
            r
        }
        IpcCommand::GetLearned => {
            let learned: Vec<LearnedEntry> = learned_store.apps.iter()
                .map(|(id, p)| LearnedEntry {
                    app_id: id.clone(),
                    surrounding_text: p.surrounding_text,
                    ime_activated: p.ime_activated,
                }).collect();
            let mut r = nil();
            r.learned = Some(learned);
            r
        }
        _ => {
            let mut r = nil();
            r.error = Some("Unexpected read command".into());
            r
        }
    }
}

/// Handle a write command — mutates config and persists.
pub fn handle_write_command(
    cmd: &IpcCommand,
    config_manager: &mut crate::config::ConfigManager,
) -> IpcResponse {
    let nil = || IpcResponse {
        input_method: None, tone_style: None, enabled: None, ime_mode: None,
        apps: None, learned: None, reload: None, error: None,
    };
    match cmd {
        IpcCommand::SetConfig { input_method, tone_style, enabled, ime_mode } => {
            let s = config_manager.setting_mut();
            if let Some(ref m) = *input_method {
                s.input_method = match m.as_str() {
                    "VNI" | "Vni" | "vni" => InputMethod::Vni,
                    // GUI gửi label hiển thị "Tự do" — thiếu nhánh này thì
                    // chọn Tự do bị lưu thành Telex (dấu chọn "không nhảy").
                    "Tự do" | "tự do" | "Smart" | "smart" => InputMethod::Smart,
                    _ => InputMethod::Telex,
                };
            }
            if let Some(ref ts) = *tone_style {
                s.tone_style = match ts.as_str() {
                    "modern" => ToneStyle::Modern,
                    _ => ToneStyle::Classic,
                };
            }
            if let Some(en) = *enabled { s.enabled = en; }
            if let Some(ref im) = *ime_mode {
                s.ime_mode = match im.as_str() {
                    "Preedit" | "preedit" => ImeMode::Preedit,
                    "NonPreedit" | "nonpreedit" => ImeMode::NonPreedit,
                    _ => ImeMode::Preedit,
                };
            }
            if let Err(e) = config_manager.save() {
                let mut r = nil();
                r.error = Some(format!("Save: {e}"));
                return r;
            }
            info!("IPC: config updated via settings UI");
            let mut r = nil();
            r.reload = Some(true);
            r
        }
        IpcCommand::AddApp { app_id, method, ime_mode } => {
            let s = config_manager.setting_mut();
            let entry = s.app_configs.entry(app_id.clone()).or_default();
            if let Some(ref m) = *method {
                entry.input_method = match m.as_str() {
                    "Telex" | "telex" => Some(InputMethod::Telex),
                    "VNI" | "Vni" | "vni" => Some(InputMethod::Vni),
                    "Tự do" | "tự do" | "Smart" | "smart" => Some(InputMethod::Smart),
                    _ => None,
                };
            }
            if let Some(ref im) = *ime_mode {
                entry.ime_mode = match im.as_str() {
                    "Preedit" | "preedit" => Some(ImeMode::Preedit),
                    "NonPreedit" | "nonpreedit" => Some(ImeMode::NonPreedit),
                    _ => None,
                };
            }
            if entry.input_method.is_none() && entry.ime_mode.is_none() && entry.enabled.is_none() {
                s.app_configs.remove(app_id);
            }
            if let Err(e) = config_manager.save() {
                let mut r = nil();
                r.error = Some(format!("Save: {e}"));
                return r;
            }
            info!("IPC: app config updated for {app_id}");
            let mut r = nil();
            r.reload = Some(true);
            r
        }
        IpcCommand::RemoveApp { app_id } => {
            config_manager.setting_mut().app_configs.remove(app_id);
            if let Err(e) = config_manager.save() {
                let mut r = nil();
                r.error = Some(format!("Save: {e}"));
                return r;
            }
            info!("IPC: app config removed for {app_id}");
            let mut r = nil();
            r.reload = Some(true);
            r
        }
        _ => {
            let mut r = nil();
            r.error = Some("Unexpected write command".into());
            r
        }
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

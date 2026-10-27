// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Unix socket IPC server — phục vụ vi-settings (cửa sổ cấu hình) và
//! vi-menu (popup nhanh từ tray), cả hai là client QuickShell.
//!
//! Giao thức: JSON lines qua Unix stream socket. Mỗi request là một dòng JSON,
//! response là một dòng JSON, theo đúng thứ tự request.
//!
//! Multi-client: mỗi kết nối một thread riêng, block trên `read_line` —
//! cửa sổ cấu hình đang mở KHÔNG được chặn popup tray (model cũ xử lý inline
//! từng client, client thứ hai treo tới khi client đầu đóng).
//!
//! Tích hợp vào unified event loop (R15): mọi thread IPC đều block (accept /
//! read), đẩy commands vào `DaemonEvent::Ipc*` channel. Không polling, không timer.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc::Sender;

use serde::{Deserialize, Serialize};
use tracing::{error, info};

use crate::events::DaemonEvent;

#[path = "ipc_handlers.rs"]
mod ipc_handlers;
pub use ipc_handlers::{handle_read_command, handle_write_command};

// ── JSON Protocol types ─────────────────────────────────────────────────

/// Incoming command from a QuickShell client.
#[derive(Debug, Deserialize)]
#[serde(tag = "cmd")]
pub enum IpcCommand {
    #[serde(rename = "get_config")]
    GetConfig,
    /// Every field optional: only the keys the client sends are changed.
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
        #[serde(default)]
        output_mode: Option<String>,
        #[serde(default)]
        free_tone_placement: Option<bool>,
        #[serde(default)]
        auto_detect_lang: Option<bool>,
        #[serde(default)]
        enable_per_app: Option<bool>,
        #[serde(default)]
        emoji: Option<bool>,
        #[serde(default)]
        predict: Option<bool>,
    },
    #[serde(rename = "list_apps")]
    ListApps,
    /// Upsert one per-app row. `method`/`ime_mode` "" (or "Mặc định") =
    /// inherit global. `enabled: false` turns Vietnamese off for that app;
    /// `true` clears the override — never stored as `Some(true)`, which would
    /// out-rank a global `enabled = false` (R18).
    /// Timings (`typing_delay_ms`, `bs_settle_ms`, `accent_settle_ms`) are
    /// written as sent: a number overrides, null/absent = built-in default.
    #[serde(rename = "add_app")]
    AddApp {
        app_id: String,
        #[serde(default)]
        method: Option<String>,
        #[serde(default)]
        ime_mode: Option<String>,
        #[serde(default)]
        enabled: Option<bool>,
        #[serde(default)]
        typing_delay_ms: Option<u64>,
        #[serde(default)]
        bs_settle_ms: Option<u64>,
        #[serde(default)]
        accent_settle_ms: Option<u64>,
    },
    #[serde(rename = "remove_app")]
    RemoveApp { app_id: String },
    #[serde(rename = "get_learned")]
    GetLearned,
    /// Upsert one keyword macro (gõ tắt).
    #[serde(rename = "set_macro")]
    SetMacro { key: String, value: String },
    #[serde(rename = "remove_macro")]
    RemoveMacro { key: String },
    /// Upsert one hotkey macro (`"ctrl+alt+m"` → text); the chord must
    /// include ctrl/alt/super (`macro_dict::parse_hotkey`).
    #[serde(rename = "set_macro_hotkey")]
    SetMacroHotkey { key: String, value: String },
    #[serde(rename = "remove_macro_hotkey")]
    RemoveMacroHotkey { key: String },
    /// Stream word suggestions: one `{"suggestions": [...]}` line now and
    /// one per change, for as long as the client stays connected (the
    /// suggestion bar). Blocks on a Condvar — no polling (R15).
    #[serde(rename = "predict_subscribe")]
    PredictSubscribe,
    /// Forget every learned word (Settings → "Xóa dữ liệu học").
    #[serde(rename = "clear_predict")]
    ClearPredict,
    /// Exit the daemon — same as the tray's "Thoát vi-im".
    #[serde(rename = "quit")]
    Quit,
}

/// Response gửi về QML client. Unset fields are omitted from the JSON.
#[derive(Debug, Serialize, Clone, Default)]
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
    pub output_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub free_tone_placement: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_detect_lang: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_per_app: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emoji: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub predict: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestions: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub macros: Option<HashMap<String, String>>,
    /// Hotkey macros (`[macro_hotkeys]`): chord → text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub macro_hotkeys: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apps: Option<Vec<AppEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub learned: Option<Vec<LearnedEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reload: Option<bool>,
}

impl IpcResponse {
    fn error(msg: String) -> Self {
        Self { error: Some(msg), ..Self::default() }
    }

    fn reload() -> Self {
        Self { reload: Some(true), ..Self::default() }
    }

    fn to_line(&self) -> String {
        serde_json::to_string(self)
            .unwrap_or_else(|e| format!("{{\"error\":\"Serialize: {e}\"}}"))
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct AppEntry {
    pub app_id: String,
    pub app_name: String,
    pub method: String,
    pub ime_mode: String,
    /// false = Vietnamese turned off for this app.
    pub enabled: bool,
    pub icon: String,
    /// Has a built-in timing profile (listed even without user overrides).
    pub builtin: bool,
    /// Profile name ("OnlyOffice (X11/Qt + CEF, …)").
    pub profile: String,
    /// User overrides (None = default) and the built-in defaults, in ms.
    pub typing_delay_ms: Option<u64>,
    pub bs_settle_ms: Option<u64>,
    pub accent_settle_ms: Option<u64>,
    pub default_typing_delay_ms: u64,
    pub default_bs_settle_ms: u64,
    pub default_accent_settle_ms: u64,
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
/// each accepted connection gets its own blocking reader thread.
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
                    Ok(stream) => {
                        let tx = tx.clone();
                        let spawned = std::thread::Builder::new()
                            .name("vi-im-ipc-client".into())
                            .spawn(move || handle_client(stream, &tx));
                        if let Err(e) = spawned {
                            error!("IPC: spawn client thread: {e}");
                        }
                    }
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
                let _ = writeln!(stream, "{}", IpcResponse::error(format!("Parse: {e}")).to_line());
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
                        let _ = writeln!(stream, "{}", resp.to_line());
                    }
                    Err(_) => break,
                }
            }
            // Writes reply before the main thread applies them, so a bad
            // chord must be refused here or the client never hears of it.
            IpcCommand::SetMacroHotkey { key, value }
                if value.is_empty() || crate::engine::macro_dict::parse_hotkey(key).is_none() =>
            {
                let msg = format!("Tổ hợp {key:?} không hợp lệ — cần ctrl/alt/super + một phím (ví dụ ctrl+alt+m) và nội dung");
                let _ = writeln!(stream, "{}", IpcResponse::error(msg).to_line());
            }
            IpcCommand::PredictSubscribe => {
                let (mut seen, mut words) = crate::engine::predict::current();
                loop {
                    let line = IpcResponse { suggestions: Some(words), ..IpcResponse::default() };
                    if writeln!(stream, "{}", line.to_line()).is_err() {
                        break; // bar closed
                    }
                    (seen, words) = crate::engine::predict::wait_change(seen);
                }
                break;
            }
            IpcCommand::ClearPredict => {
                crate::engine::predict::clear();
                info!("IPC: learned word suggestions cleared");
                let _ = writeln!(stream, "{}", IpcResponse::reload().to_line());
            }
            IpcCommand::Quit => {
                info!("IPC: quit requested");
                let _ = writeln!(stream, "{}", IpcResponse::reload().to_line());
                std::process::exit(0);
            }
            IpcCommand::SetConfig { .. }
            | IpcCommand::AddApp { .. }
            | IpcCommand::RemoveApp { .. }
            | IpcCommand::SetMacro { .. }
            | IpcCommand::RemoveMacro { .. }
            | IpcCommand::SetMacroHotkey { .. }
            | IpcCommand::RemoveMacroHotkey { .. } => {
                if tx.send(DaemonEvent::IpcWrite { command: cmd }).is_err() {
                    break;
                }
                let _ = writeln!(stream, "{}", IpcResponse::reload().to_line());
            }
        }
    }
}

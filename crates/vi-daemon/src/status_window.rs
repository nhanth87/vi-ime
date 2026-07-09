// SPDX-License-Identifier: GPL-3.0-or-later OR Commercial
// Copyright (c) 2024-2026 vi-im contributors
//! vi-status — QML floating status indicator.
//! Looks for `vi-status.qml` next to the binary or in installed assets dir,
//! then runs it via `qml` (Qt6 QML runner). Falls back to notify-send.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    let qml_file = find_qml_file();

    // Try qml runner (Qt6)
    if qml_file.exists() {
        if Command::new("qml").arg(&qml_file).spawn().is_ok() {
            return;
        }
        // Try qmlscene (Qt5)
        if Command::new("qmlscene").arg(&qml_file).spawn().is_ok() {
            return;
        }
    }

    eprintln!("vi-status: QML runner not available — install qt6-declarative or use CLI commands");
}

fn find_qml_file() -> PathBuf {
    // 1. Next to the binary
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent() {
            let sibling = dir.join("vi-status.qml");
            if sibling.exists() { return sibling; }
            // Also check ../assets/
            let assets = dir.join("../assets/vi-status.qml");
            if assets.exists() { return assets; }
        }
    // 2. Installed data dir
    let data = data_dir().join("vi-ime").join("vi-status.qml");
    if data.exists() { return data; }
    data
}

fn data_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        PathBuf::from(xdg)
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".local").join("share")
    } else {
        PathBuf::from("/tmp")
    }
}

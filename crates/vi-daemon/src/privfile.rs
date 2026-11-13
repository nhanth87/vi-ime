// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Private-file helper (R38): learned typing data and logs must not be
//! readable by other local users. All writers of user data go through
//! `write` (0700 parents, 0600 file); `harden_dir` fixes up files left
//! 0644 by older runs, once at startup. Best-effort everywhere: a
//! permission failure must never break typing.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

const MODE_FILE: u32 = 0o600;
const MODE_DIR: u32 = 0o700;

fn protect(path: &Path) {
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(MODE_FILE));
}

/// Write bytes privately: parents 0700, file 0600. Same shape as
/// `std::fs::write` so call sites change by one identifier.
pub(crate) fn write(path: &Path, bytes: impl AsRef<[u8]>) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(MODE_DIR));
    }
    std::fs::write(path, bytes)?;
    protect(path);
    Ok(())
}

/// `$XDG_DATA_HOME/vi-ime` (or `~/.local/share/vi-ime`): every learned
/// typing file lives here (predict.tsv, habit.tsv, diacritic_keep.txt,
/// learned.toml, telemetry.toml, vi-ime.log).
pub(crate) fn data_dir() -> std::path::PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME").map(std::path::PathBuf::from).or_else(|| {
        std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share"))
    });
    base.map(|b| b.join("vi-ime")).unwrap_or_else(|| std::path::PathBuf::from("/tmp/vi-ime"))
}

/// 0700 the dir and 0600 every file directly inside it.
pub(crate) fn harden_dir(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(MODE_DIR));
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        if e.file_type().is_ok_and(|t| t.is_file()) {
            protect(&e.path());
        }
    }
}

#[cfg(test)]
#[path = "privfile_tests.rs"]
mod privfile_tests;

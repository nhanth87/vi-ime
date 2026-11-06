// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Log file with a hard size cap (user 2026-11-06: "tự xóa log file khi lớn
//! hơn 10M"). One file, `vi-ime.log`; once it passes `MAX_BYTES` it is
//! deleted and started again. Writes come from the tracing worker thread
//! (`tracing_appender::non_blocking`), never from the typing path.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const MAX_BYTES: u64 = 10 * 1024 * 1024;

pub(crate) struct CappedLog {
    path: PathBuf,
    file: Option<File>,
    size: u64,
}

impl CappedLog {
    /// Opens `<dir>/vi-ime.log` and removes the old daily files
    /// (`vi-ime.<date>.log`) left by earlier versions.
    pub(crate) fn open(dir: &Path) -> Self {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                let name = e.file_name();
                let name = name.to_string_lossy();
                if name.starts_with("vi-ime.") && name.ends_with(".log") && name != "vi-ime.log" {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        let path = dir.join("vi-ime.log");
        let mut log = Self { path, file: None, size: 0 };
        log.reopen(false);
        if log.size > MAX_BYTES {
            log.reopen(true);
        }
        log
    }

    fn reopen(&mut self, fresh: bool) {
        if fresh {
            let _ = std::fs::remove_file(&self.path);
        }
        self.file = OpenOptions::new().create(true).append(true).open(&self.path).ok();
        self.size = self.file.as_ref().and_then(|f| f.metadata().ok()).map_or(0, |m| m.len());
    }
}

impl Write for CappedLog {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.size + buf.len() as u64 > MAX_BYTES {
            self.reopen(true);
        }
        let Some(f) = self.file.as_mut() else { return Ok(buf.len()) }; // no log, never fail the daemon
        let n = f.write(buf)?;
        self.size += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.as_mut().map_or(Ok(()), Write::flush)
    }
}


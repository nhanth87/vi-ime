// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! The X clipboard as a fast path for long rewrites in OnlyOffice (R37).
//! OnlyOffice drops an accented glyph when the next key comes sooner than
//! ~110 ms, so retyping a phrase costs 150 ms per accented glyph (~1 s for
//! "em trồng cần sa trước cửa nhà em,"). Pasting the fixed text is one key.
//! The user chose this (2026-11-15) knowing the clipboard is borrowed for a
//! moment and a clipboard manager may record the phrase.

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

/// The clipboard is handed back this long after the paste key: the app
/// reads it asynchronously, and handing it back too soon would paste the
/// user's old clipboard instead. Restoring runs on its own thread.
const RESTORE_AFTER: Duration = Duration::from_millis(1000);

fn xclip(display: &str) -> Command {
    let mut c = Command::new("xclip");
    c.args(["-selection", "clipboard"]).env("DISPLAY", display).stderr(Stdio::null());
    c
}

/// What the clipboard holds now: Ok(None) = empty, Ok(Some(text)) = text,
/// Err(()) = something we cannot put back exactly (an image, a file list):
/// then the clipboard is not touched.
fn save(display: &str) -> Result<Option<Vec<u8>>, ()> {
    let targets = xclip(display).args(["-o", "-t", "TARGETS"]).output().map_err(|_| ())?;
    if !targets.status.success() {
        return Ok(None); // nobody owns the clipboard
    }
    let list = String::from_utf8_lossy(&targets.stdout);
    let only_text = list
        .lines()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .all(|t| matches!(t, "TARGETS" | "TIMESTAMP" | "MULTIPLE" | "SAVE_TARGETS" | "UTF8_STRING" | "STRING" | "TEXT" | "COMPOUND_TEXT")
            || t.starts_with("text/plain"));
    if !only_text {
        return Err(());
    }
    let text = xclip(display).args(["-o", "-t", "UTF8_STRING"]).output().map_err(|_| ())?;
    if text.status.success() { Ok(Some(text.stdout)) } else { Err(()) }
}

fn set(display: &str, bytes: &[u8]) -> bool {
    // xclip -i forks a server that owns the selection; the parent exits once
    // it does, so `wait` returning means the clipboard is ours.
    let Ok(mut child) = xclip(display).args(["-i", "-t", "UTF8_STRING"]).stdin(Stdio::piped()).stdout(Stdio::null()).spawn() else {
        return false;
    };
    let wrote = child.stdin.take().is_some_and(|mut s| s.write_all(bytes).is_ok());
    child.wait().is_ok_and(|st| st.success()) && wrote
}

/// Put `text` on the clipboard, call `paste` (which sends Ctrl+V), then
/// give the old clipboard back on a background thread. False, with the
/// clipboard untouched, when this cannot be done safely.
pub(crate) fn paste_text(text: &str, paste: impl FnOnce() -> bool) -> bool {
    let display = crate::evdev_inject::resolve_x11_display()
        .or_else(|| std::env::var("DISPLAY").ok())
        .unwrap_or_else(|| ":0".into());
    let Ok(saved) = save(&display) else { return false };
    if !set(&display, text.as_bytes()) {
        return false;
    }
    let pasted = paste();
    std::thread::spawn(move || {
        std::thread::sleep(RESTORE_AFTER);
        if let Some(old) = saved {
            set(&display, &old);
        }
    });
    pasted
}

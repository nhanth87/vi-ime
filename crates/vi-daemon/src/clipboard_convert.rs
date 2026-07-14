// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Clipboard Vietnamese normalization ("📋 Chuyển đổi clipboard").
//!
//! Wired from the tray: read the current clipboard text, normalize Vietnamese
//! to precomposed Unicode (NFC / "dựng sẵn"), and write it back. This is the
//! safe, meaning-preserving conversion — it never changes WHICH characters a
//! word contains, only how they are encoded (combining marks → precomposed),
//! so pasting into apps that mishandle NFD (e.g. some web forms) shows the
//! text correctly. Idempotent: text already NFC round-trips unchanged.
//!
//! Design choice (2026-07-12): the earlier VNI/VIQR "tie^'ng" → "tiếng"
//! heuristic was rejected — it mangles legitimate ASCII (code, URLs) and there
//! is no reliable signal that pasted text is VNI vs plain. NFC normalization is
//! unambiguous and lossless.

use unicode_normalization::UnicodeNormalization;

/// Normalize a string's Vietnamese to precomposed NFC form. Pure function —
/// the actual GTK clipboard I/O lives in the tray thread (where a GDK display
/// is available). Returns `None` when the text is already NFC (nothing to do),
/// so the caller can skip a redundant clipboard write.
pub fn to_nfc(text: &str) -> Option<String> {
    let out: String = text.nfc().collect();
    if out == text { None } else { Some(out) }
}

// Manual test (policy: no automation tests): copy NFD Vietnamese (e.g. paste
// from a source that stores combining marks), click tray "📋 Chuẩn hoá
// clipboard (NFC)", paste — the text must be byte-identical visually but now
// precomposed. Copy plain ASCII / a URL / code → clicking must NOT change it.

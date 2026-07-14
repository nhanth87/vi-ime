// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! P1: Word matrix for the harness — the same hard cases that have caused
//! field bugs (R22). Double-tracked: both P0 atomic path and VietTyper path.

/// A single test case: type `keys` and expect the rendered buffer to be `want`.
pub struct HarnessWord {
    /// Telex key sequence (letters + tone marks).
    pub keys: &'static str,
    /// Expected rendered Vietnamese text after all keystrokes + final commit.
    pub want: &'static str,
}

/// Words that have historically caused field bugs (R22).
/// These MUST pass on BOTH the atomic P0 path (with surrounding_text)
/// and the VietTyper fallback path (without).
pub const HARD_MATRIX: &[HarnessWord] = &[
    // ── Core tone issues (field bugs 2026-07-10/13) ──
    HarnessWord { keys: "ngu7o7if",  want: "người" },    // R22 Bug A: ư mất dấu sừng
    HarnessWord { keys: "nguwowif",  want: "người" },    // same, uo→ươ path
    HarnessWord { keys: "chu74x",    want: "chữ" },      // R22 Bug A: dấu ngã + móc
    HarnessWord { keys: "chuwx",     want: "chữ" },      // same, uw path
    HarnessWord { keys: "quar",      want: "quả" },      // round-4: chỉ còn mất ó
    HarnessWord { keys: "cos",       want: "có" },       // round-4: "co"→"có" mất "ó"
    HarnessWord { keys: "as",        want: "á" },        // round-4: "a"→"á" mất trắng
    // ── oa/oe/uy tone placement ──
    HarnessWord { keys: "hoaf",      want: "hòa" },
    HarnessWord { keys: "hoas",      want: "hóa" },
    HarnessWord { keys: "khoef",     want: "khoè" },
    HarnessWord { keys: "thuys",     want: "thuý" },
    // ── uo→ươ complex ──
    HarnessWord { keys: "dduwowngs", want: "đường" },
    HarnessWord { keys: "buwowir",   want: "bưởi" },
    HarnessWord { keys: "ruwowuj",   want: "rượu" },
    // ── gi + qu onset ──
    HarnessWord { keys: "gis",       want: "gí" },
    HarnessWord { keys: "gif",       want: "gì" },
    HarnessWord { keys: "quoocs",    want: "quốc" },
    HarnessWord { keys: "quyeenr",   want: "quyển" },
    // ── Stacked quality + tone ──
    HarnessWord { keys: "nghieeng",  want: "nghiêng" },
    HarnessWord { keys: "cuwngs",    want: "cứng" },
    // ── Multi-word: field confirm whole sentence ──
    HarnessWord {
        keys: "mawts bof cas ger hix luj mood nees paws quaf rir sex vu7 xa6 yst kef tieemf uws daaus dduwowngs VIE6T5",
        want: "mất bò cá gẻ hĩ lụ mô nế pắ quà rỉ sẽ vư xâ ýt kẹ tiệm ừ dấu đường VIỆT",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hard_matrix_count() {
        assert_eq!(HARD_MATRIX.len(), 21, "update count when adding/removing");
    }
}

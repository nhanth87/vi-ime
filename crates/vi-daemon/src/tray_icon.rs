// SPDX-License-Identifier: GPL-3.0-or-later OR Commercial
// Copyright (c) 2024-2026 vi-im contributors
//! Raw ARGB32 tray icon rendering — the `icon_pixmap` fallback.
//!
//! `icon_name` (freedesktop theme lookup) is enough for GTK/Qt-based bars,
//! but several minimal wlroots-bar tray implementations (the ones people
//! actually run alongside niri/Sway) render ONLY `IconPixmap` and never do
//! a theme lookup by name at all — with no SVG library in the dependency
//! tree, we draw the icon ourselves: solid rounded-square background, one
//! hand-authored bitmap glyph ('V' or 'E'), nearest-neighbor scaled. No
//! font, no rasterizer, same visual as `assets/icons/*.svg`.

const BG: (u8, u8, u8) = (0x2E, 0x86, 0xDE); // sky blue, matches gen-icons.py
const FG: (u8, u8, u8) = (0xFF, 0xFF, 0xFF);

/// 9x9 bitmap glyphs — hand-authored, '1' = glyph pixel.
const GLYPH_V: [&str; 9] = [
    "1.......1",
    "1.......1",
    ".1.....1.",
    ".1.....1.",
    ".1.....1.",
    "..1...1..",
    "..1...1..",
    "...1.1...",
    "....1....",
];
const GLYPH_E: [&str; 9] = [
    "111111111",
    "1........",
    "1........",
    "1........",
    "1111111..",
    "1........",
    "1........",
    "1........",
    "111111111",
];

/// Render one size of the icon as ARGB32 (network byte order, per the SNI
/// spec: byte0=A, byte1=R, byte2=G, byte3=B for every pixel).
fn render(size: i32, glyph: &[&str; 9]) -> ksni::Icon {
    let n = size as usize;
    let mut data = vec![0u8; n * n * 4];
    let radius = size / 5;

    let grid = glyph.len() as i32;
    let target = (size * 7) / 10; // glyph fills ~70% of the canvas
    let scale = (target / grid).max(1);
    let glyph_px = grid * scale;
    let off = (size - glyph_px) / 2;

    for y in 0..size {
        for x in 0..size {
            let idx = ((y * size + x) * 4) as usize;
            let alpha = corner_alpha(x, y, size, radius);
            let (mut r, mut g, mut b) = BG;
            if x >= off && x < off + glyph_px && y >= off && y < off + glyph_px {
                let gx = ((x - off) / scale) as usize;
                let gy = ((y - off) / scale) as usize;
                if glyph[gy].as_bytes()[gx] == b'1' {
                    (r, g, b) = FG;
                }
            }
            data[idx] = alpha;
            data[idx + 1] = r;
            data[idx + 2] = g;
            data[idx + 3] = b;
        }
    }
    ksni::Icon { width: size, height: size, data }
}

/// 255 everywhere except the four corners outside the rounded-rect radius.
fn corner_alpha(x: i32, y: i32, size: i32, r: i32) -> u8 {
    let in_left = x < r;
    let in_right = x >= size - r;
    let in_top = y < r;
    let in_bottom = y >= size - r;
    if (in_left || in_right) && (in_top || in_bottom) {
        let cx = if in_left { r } else { size - 1 - r };
        let cy = if in_top { r } else { size - 1 - r };
        let (dx, dy) = (x - cx, y - cy);
        if dx * dx + dy * dy > r * r {
            return 0;
        }
    }
    255
}

/// Multi-size icon set — hosts pick whichever size fits their panel.
pub(crate) fn pixmap(enabled: bool) -> Vec<ksni::Icon> {
    let glyph = if enabled { &GLYPH_V } else { &GLYPH_E };
    [22, 32, 48].into_iter().map(|s| render(s, glyph)).collect()
}

#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later OR Commercial
# Copyright (c) 2024-2026 vi-im contributors
"""Generate the vi-im tray icons: a sky-blue rounded square with a bold
white glyph — 'V' when Vietnamese input is on, 'E' when it's off (raw
English passthrough). Pure stdlib (writes SVG XML directly), no
cairosvg/Pillow dependency — the icons only need to exist as .svg files
in a freedesktop icon theme, which every StatusNotifierHost can rasterize
itself.

Run: python3 scripts/gen-icons.py
Output: assets/icons/vi-im.svg (on) and assets/icons/vi-im-off.svg (off)
"""

from pathlib import Path

SIZE = 64
BG_ON = "#2E86DE"      # sky blue — Vietnamese input ON
BG_OFF = "#E04A4A"     # red — Vietnamese input OFF (raw English passthrough)
FG = "#FFFFFF"        # white glyph
CORNER_R = 14          # rounded-square corner radius

SVG_TEMPLATE = """<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 {size} {size}">
  <rect x="0" y="0" width="{size}" height="{size}" rx="{r}" ry="{r}" fill="{bg}"/>
  <text x="{cx}" y="{cy}" text-anchor="middle"
        font-family="DejaVu Sans, Noto Sans, Arial, sans-serif" font-weight="bold"
        font-size="{fs}" fill="{fg}">{glyph}</text>
</svg>
"""


def render(glyph: str, out_path: Path, bg: str) -> None:
    fs = SIZE * 0.58
    svg = SVG_TEMPLATE.format(
        size=SIZE,
        r=CORNER_R,
        bg=bg,
        fg=FG,
        cx=SIZE / 2,
        # No dominant-baseline: librsvg (what GTK/libappindicator actually
        # rasterizes with) supports it inconsistently and renders the
        # glyph noticeably high — confirmed live 2026-07-10, looked
        # centered in ImageMagick's preview (different SVG engine) but
        # sat near the top in the real tray. Default alphabetic baseline
        # + a manual cap-height offset is the portable fix: a capital
        # letter's cap-height is ~0.7 of font-size, so nudging the
        # baseline down by ~0.35*font-size centers the visible glyph
        # instead of the invisible em-box.
        cy=SIZE / 2 + fs * 0.35,
        fs=int(fs),
        glyph=glyph,
    )
    out_path.write_text(svg, encoding="utf-8")
    print(f"wrote {out_path} ({len(svg)} bytes)")


def main() -> None:
    out_dir = Path(__file__).resolve().parent.parent / "assets" / "icons"
    out_dir.mkdir(parents=True, exist_ok=True)
    render("V", out_dir / "vi-im.svg", BG_ON)       # Vietnamese input ON (blue 'V')
    render("E", out_dir / "vi-im-off.svg", BG_OFF)  # Vietnamese input OFF (red 'E')


if __name__ == "__main__":
    main()

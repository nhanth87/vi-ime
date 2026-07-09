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
BG = "#2E86DE"       # sky blue
FG = "#FFFFFF"        # white glyph
CORNER_R = 14          # rounded-square corner radius

SVG_TEMPLATE = """<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 {size} {size}">
  <rect x="0" y="0" width="{size}" height="{size}" rx="{r}" ry="{r}" fill="{bg}"/>
  <text x="{cx}" y="{cy}" text-anchor="middle" dominant-baseline="central"
        font-family="DejaVu Sans, Noto Sans, Arial, sans-serif" font-weight="bold"
        font-size="{fs}" fill="{fg}">{glyph}</text>
</svg>
"""


def render(glyph: str, out_path: Path) -> None:
    svg = SVG_TEMPLATE.format(
        size=SIZE,
        r=CORNER_R,
        bg=BG,
        fg=FG,
        cx=SIZE / 2,
        # Nudge baseline down slightly — dominant-baseline=central still
        # renders a hair high for capital letters in most renderers.
        cy=SIZE / 2 + 2,
        fs=int(SIZE * 0.58),
        glyph=glyph,
    )
    out_path.write_text(svg, encoding="utf-8")
    print(f"wrote {out_path} ({len(svg)} bytes)")


def main() -> None:
    out_dir = Path(__file__).resolve().parent.parent / "assets" / "icons"
    out_dir.mkdir(parents=True, exist_ok=True)
    render("V", out_dir / "vi-im.svg")       # Vietnamese input ON
    render("E", out_dir / "vi-im-off.svg")   # Vietnamese input OFF (raw English)


if __name__ == "__main__":
    main()

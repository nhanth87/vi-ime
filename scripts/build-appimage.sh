#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (c) 2024-2026 vi-im contributors
#
# Build vi-im-x86_64.AppImage: bundles vi-ime (daemon+tray) and vi-settings
# (QML config window launcher) + the QML source + icons into one portable
# file. Runs on any wlroots compositor (niri, Hyprland, Sway, COSMIC) —
# nothing here is compositor-specific.
#
# What is NOT bundled, by design: libwayland/libxkbcommon (every target
# system already has these — a Wayland IME with its own copy would be
# fighting the compositor's own libwayland), and Qt/quickshell (heavy,
# and the user's already-installed quickshell is what floats+centers the
# settings window via compositor IPC — bundling a second one would just
# not talk to the compositor). Both are documented runtime deps below.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUILD_DIR="$ROOT_DIR/target/appimage"
APPDIR="$BUILD_DIR/vi-im.AppDir"
CACHE_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/vi-im-build"
APPIMAGETOOL="$CACHE_DIR/appimagetool-x86_64.AppImage"
ARCH="${ARCH:-x86_64}"

GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'; NC='\033[0m'
info() { echo -e "  ${GREEN}✓${NC} $1"; }
warn() { echo -e "  ${YELLOW}⚠${NC} $1"; }
step() { echo -e "\n${CYAN}── $1 ──${NC}"; }

cd "$ROOT_DIR"

step "1/5 — Building release binaries"
cargo build --release
[ -f target/release/vi-ime ] || { echo "vi-ime binary missing — build failed"; exit 1; }
info "vi-ime, vi-settings built"

step "2/5 — Icons (regenerate from source, keeps AppImage in sync)"
if command -v python3 &>/dev/null; then
    python3 scripts/gen-icons.py
else
    warn "python3 not found — reusing assets/icons/*.svg as-is"
fi

step "3/5 — Assembling AppDir"
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/vi-im/qml" "$APPDIR/usr/share/icons/hicolor/scalable/apps"

cp target/release/vi-ime "$APPDIR/usr/bin/vi-ime"
[ -f target/release/vi-settings ] && cp target/release/vi-settings "$APPDIR/usr/bin/vi-settings"
cp vi-settings/main.qml "$APPDIR/usr/share/vi-im/qml/main.qml"
cp assets/icons/vi-im.svg "$APPDIR/usr/share/icons/hicolor/scalable/apps/vi-im.svg"
cp assets/icons/vi-im-off.svg "$APPDIR/usr/share/icons/hicolor/scalable/apps/vi-im-off.svg"
cp assets/icons/vi-im.svg "$APPDIR/vi-im.svg"

# Root-icon fallback: appimagetool / older thumbnailers expect a PNG next
# to a plain svg is fine on modern tooling, but rsvg-convert (if present)
# gives us a safety net that works everywhere.
if command -v rsvg-convert &>/dev/null; then
    rsvg-convert -w 256 -h 256 assets/icons/vi-im.svg -o "$APPDIR/vi-im.png"
fi

cat > "$APPDIR/vi-im.desktop" << 'DESKTOP'
[Desktop Entry]
Type=Application
Name=vi-im
Comment=Bộ gõ tiếng Việt cho Wayland (niri, Hyprland, Sway, COSMIC)
Exec=AppRun
Icon=vi-im
Terminal=false
Categories=Utility;Settings;
DESKTOP

cat > "$APPDIR/AppRun" << 'APPRUN'
#!/usr/bin/env bash
# vi-im AppImage entry point.
#   ./vi-im-x86_64.AppImage            → runs the daemon (vi-ime)
#   ./vi-im-x86_64.AppImage settings   → opens the settings window
#   ./vi-im-x86_64.AppImage --doctor   → any other vi-ime flag passes through
HERE="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")" && pwd)"
export PATH="$HERE/usr/bin:$PATH"
# vi-settings looks for VI_SETTINGS_QML first (see find_qml() in main.rs) —
# set it so it finds the QML bundled in THIS AppImage, not some path on
# the host that may not exist.
export VI_SETTINGS_QML="$HERE/usr/share/vi-im/qml/main.qml"

if [ "${1:-}" = "settings" ]; then
    shift
    exec "$HERE/usr/bin/vi-settings" "$@"
fi
exec "$HERE/usr/bin/vi-ime" "$@"
APPRUN
chmod +x "$APPDIR/AppRun"
info "AppDir ready at $APPDIR"

step "4/5 — Fetching appimagetool (cached under $CACHE_DIR)"
mkdir -p "$CACHE_DIR"
if [ ! -x "$APPIMAGETOOL" ]; then
    URL="https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-${ARCH}.AppImage"
    if command -v curl &>/dev/null; then
        curl -fsSL -o "$APPIMAGETOOL" "$URL"
    elif command -v wget &>/dev/null; then
        wget -qO "$APPIMAGETOOL" "$URL"
    else
        echo "Cần curl hoặc wget để tải appimagetool. Tải tay:"
        echo "  $URL"
        echo "Lưu vào: $APPIMAGETOOL rồi chạy lại script này."
        exit 1
    fi
    chmod +x "$APPIMAGETOOL"
fi
info "appimagetool sẵn sàng"

step "5/5 — Packaging AppImage"
OUT="$ROOT_DIR/vi-im-${ARCH}.AppImage"
# --appimage-extract-and-run: appimagetool is itself an AppImage that
# normally needs FUSE to mount; this flag extracts-and-runs instead, so
# the build works in containers/CI without a FUSE kernel module.
ARCH="$ARCH" "$APPIMAGETOOL" --appimage-extract-and-run "$APPDIR" "$OUT" 2>&1 | grep -v '^$' || true

if [ -f "$OUT" ]; then
    chmod +x "$OUT"
    info "Built: $OUT ($(du -h "$OUT" | cut -f1))"
    echo ""
    echo -e "  Chạy daemon:    ${CYAN}$OUT${NC}"
    echo -e "  Mở settings:    ${CYAN}$OUT settings${NC}"
else
    echo "appimagetool did not produce $OUT — see output above."
    exit 1
fi

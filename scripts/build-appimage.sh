#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
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
# Whole QML dir: main.qml (config window), menu.qml (tray quick menu) and
# the shared components they import via qmldir.
cp vi-settings/*.qml vi-settings/qmldir "$APPDIR/usr/share/vi-im/qml/"
# Helper scripts shipped inside: `AppImage install|uninstall|regression`.
mkdir -p "$APPDIR/usr/share/vi-im/scripts"
cp scripts/appimage-install.sh scripts/gen-icons.py "$APPDIR/usr/share/vi-im/scripts/"
cp -r scripts/vi-regression "$APPDIR/usr/share/vi-im/scripts/"
chmod +x "$APPDIR/usr/share/vi-im/scripts/"*.sh "$APPDIR/usr/share/vi-im/scripts/vi-regression/"*.sh
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

SCRIPTS="$HERE/usr/share/vi-im/scripts"
case "${1:-}" in
    install|uninstall)
        exec bash "$SCRIPTS/appimage-install.sh" "$@"
        ;;
    menu)
        shift
        exec "$HERE/usr/bin/vi-settings" --menu "$@"
        ;;
    regression)
        shift
        exec bash "$SCRIPTS/vi-regression/run.sh" "$@"
        ;;
    help|--help|-h)
        cat <<'HELP'
vi-im — bộ gõ tiếng Việt cho Wayland

  vi-im-x86_64.AppImage                  chạy bộ gõ (daemon + tray)
  vi-im-x86_64.AppImage install          cài vào ~/Applications + menu
  vi-im-x86_64.AppImage install --autostart
                                         ... và tự chạy khi đăng nhập
  vi-im-x86_64.AppImage uninstall        gỡ menu + tự khởi động
  vi-im-x86_64.AppImage settings         cửa sổ cấu hình
  vi-im-x86_64.AppImage menu             menu nhanh (như chuột giữa tray)
  vi-im-x86_64.AppImage --doctor         kiểm tra môi trường
  vi-im-x86_64.AppImage --toggle | --switch | --status
  vi-im-x86_64.AppImage regression       bộ test thực địa (xem scripts)
HELP
        exit 0
        ;;
esac

# One-shot commands that never touch the keyboard don't need /dev/input —
# skip the group dance entirely for them.
case "${1:-}" in
    --doctor|--take-over|--stop-rivals|--switch|--toggle|--mode|--status)
        exec "$HERE/usr/bin/vi-ime" "$@"
        ;;
esac

# evdev fallback (LibreOffice/OnlyOffice auto-grab, plus `--evdev`) needs
# read access to /dev/input/eventN — gated by the `input` group on every
# mainstream distro (deliberately: raw keyboard read = keylogging risk, so
# it's never auto-granted like joystick/webcam uaccess is). Without it the
# daemon still runs fine for every app reachable via zwp_input_method_v2 —
# this is a soft upgrade, never a hard requirement.
if command -v id >/dev/null 2>&1 && ! id -nG "$USER" 2>/dev/null | grep -qw input; then
    echo "vi-ime: chưa ở nhóm 'input' — LibreOffice/OnlyOffice sẽ không gõ được"
    echo "        (evdev fallback cần đọc /dev/input). Xin quyền một lần..."
    ADDED=1
    if command -v pkexec >/dev/null 2>&1; then
        pkexec usermod -aG input "$USER" || ADDED=0
    elif command -v sudo >/dev/null 2>&1; then
        sudo usermod -aG input "$USER" || ADDED=0
    else
        echo "vi-ime: không tìm thấy pkexec/sudo — bỏ qua, chạy như bình thường."
        ADDED=0
    fi
    if [ "$ADDED" = "1" ] && command -v sg >/dev/null 2>&1; then
        # usermod takes effect for NEW sessions only — `sg` re-execs this
        # process with the group active right now, no logout needed.
        echo "vi-ime: đã vào nhóm 'input' — áp dụng ngay, không cần đăng nhập lại."
        exec sg input -c "exec \"$HERE/usr/bin/vi-ime\" $(printf '%q ' "$@")"
    fi
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
# Remove the previous build first: otherwise a failed run still finds the
# OLD file below and reports "Built" (happened 2026-10-24).
rm -f "$OUT"
ARCH="$ARCH" "$APPIMAGETOOL" --appimage-extract-and-run "$APPDIR" "$OUT" 2>&1 | grep -av '^$' || true
# AppImageLauncher's binfmt hook breaks --appimage-extract-and-run
# ("appimagelauncher-binfmt-bypass ... execv failed"): unpack appimagetool's
# squashfs ourselves and run its AppRun directly.
if [ ! -f "$OUT" ] && command -v unsquashfs &>/dev/null; then
    TOOLROOT="$CACHE_DIR/appimagetool-root"
    if [ ! -x "$TOOLROOT/AppRun" ]; then
        for off in $(grep -abo hsqs "$APPIMAGETOOL" | cut -d: -f1); do
            rm -rf "$TOOLROOT"
            unsquashfs -q -o "$off" -d "$TOOLROOT" "$APPIMAGETOOL" >/dev/null 2>&1 && break
        done
    fi
    [ -x "$TOOLROOT/AppRun" ] && ARCH="$ARCH" "$TOOLROOT/AppRun" "$APPDIR" "$OUT" 2>&1 | grep -av '^$' || true
fi

if [ -f "$OUT" ]; then
    chmod +x "$OUT"
    info "Built: $OUT ($(du -h "$OUT" | cut -f1))"
    echo ""
    echo -e "  Chạy daemon:    ${CYAN}$OUT${NC}"
    echo -e "  Mở settings:    ${CYAN}$OUT settings${NC}"
    echo ""
    warn "Chưa bundle thư viện GTK3/libayatana-appindicator3 (tray icon)."
    echo "       AppImage này KHÔNG tự chứa — máy chạy nó vẫn cần sẵn:"
    echo "         gtk3, libayatana-appindicator3, libdbusmenu-gtk3"
    echo "       Thiếu 1 trong 3 → vi-ime KHÔNG khởi động được (kể cả gõ"
    echo "       tiếng Việt, không chỉ mất tray) — dynamic linker chặn"
    echo "       ngay khi nạp binary, trước khi main() chạy."
    echo "       Cố tình không bundle: GTK3 gần như luôn có sẵn trên desktop"
    echo "       Linux, và bundle cả stack GTK/X11 (~70 thư viện) sẽ làm"
    echo "       AppImage phình to + dễ vỡ theming trên máy đích — đi ngược"
    echo "       tinh thần 'siêu nhẹ' của vi-im. Muốn AppImage tự chứa hoàn"
    echo "       toàn (kể cả trên máy thiếu appindicator3): cần refactor"
    echo "       tray sang dlopen lúc chạy thay vì link tĩnh — chưa làm."
else
    echo "appimagetool did not produce $OUT — see output above."
    exit 1
fi

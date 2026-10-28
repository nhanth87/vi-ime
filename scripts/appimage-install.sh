#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
#
# Installer bundled INSIDE the AppImage (usr/share/vi-im/scripts/).
#   ./vi-im-x86_64.AppImage install [--autostart]
#   ./vi-im-x86_64.AppImage uninstall
#
# Per-user, no root: copies the AppImage to ~/Applications, adds the icon and
# a "vi-im Settings" menu entry. --autostart also installs a systemd user
# service bound to graphical-session.target (uwsm, Hyprland, niri, Sway,
# COSMIC sessions all start that target). Without --autostart nothing is
# started at login — useful when the compositor config already runs vi-ime.
set -euo pipefail

ACTION="${1:-install}"
AUTOSTART=0
[ "${2:-}" = "--autostart" ] && AUTOSTART=1

SRC="${APPIMAGE:-}"                    # set by the AppImage runtime
APPS="$HOME/Applications"
DEST="$APPS/vi-im-x86_64.AppImage"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
CONF="${XDG_CONFIG_HOME:-$HOME/.config}"
UNIT="$CONF/systemd/user/vi-ime.service"
DESKTOP="$DATA/applications/vi-im-settings.desktop"
ICON="$DATA/icons/hicolor/scalable/apps/vi-im.svg"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"   # AppDir root

say() { printf '  %s\n' "$*"; }

install_app() {
    if [ -z "$SRC" ] || [ ! -f "$SRC" ]; then
        echo "vi-im: chạy lệnh này từ file AppImage (./vi-im-x86_64.AppImage install)." >&2
        exit 1
    fi
    mkdir -p "$APPS" "$(dirname "$DESKTOP")" "$(dirname "$ICON")"
    if [ "$(readlink -f "$SRC")" != "$(readlink -f "$DEST" 2>/dev/null || true)" ]; then
        [ -f "$DEST" ] && cp -p "$DEST" "$DEST.bak" && say "Bản cũ giữ lại: $DEST.bak"
        cp -f "$SRC" "$DEST.tmp" && mv -f "$DEST.tmp" "$DEST"
    fi
    chmod +x "$DEST"
    say "AppImage: $DEST"

    cp -f "$HERE/usr/share/icons/hicolor/scalable/apps/vi-im.svg" "$ICON" 2>/dev/null || true
    cat > "$DESKTOP" <<EOF
[Desktop Entry]
Type=Application
Name=vi-im Settings
Name[vi]=Cấu hình vi-im
Comment=Bộ gõ tiếng Việt cho Wayland
Exec=$DEST settings
Icon=vi-im
Terminal=false
Categories=Settings;Utility;
Keywords=vietnamese;ime;telex;vni;bo go;
EOF
    say "Menu: vi-im Settings"

    if [ "$AUTOSTART" = 1 ]; then
        mkdir -p "$(dirname "$UNIT")"
        cat > "$UNIT" <<EOF
[Unit]
Description=vi-ime Vietnamese input method
PartOf=graphical-session.target
After=graphical-session.target

[Service]
ExecStart=$DEST
Restart=on-failure
RestartSec=2

[Install]
WantedBy=graphical-session.target
EOF
        systemctl --user daemon-reload
        systemctl --user enable vi-ime.service >/dev/null 2>&1 || true
        say "Tự khởi động: systemd --user vi-ime.service (bật từ lần đăng nhập sau)"
        say "Chạy ngay: systemctl --user start vi-ime.service"
    else
        say "Tự khởi động: chưa bật. Thêm '--autostart', hoặc tự gọi '$DEST'"
        say "trong cấu hình compositor (Hyprland exec-once, niri spawn-at-startup…)."
    fi

    if ! id -nG "$USER" 2>/dev/null | grep -qw input; then
        say "Lưu ý: chưa ở nhóm 'input' — LibreOffice/OnlyOffice/Chrome X11 cần nó."
        say "Lần chạy đầu vi-ime sẽ tự xin quyền (pkexec/sudo usermod -aG input)."
    fi
    echo "vi-im: cài xong."
}

uninstall_app() {
    if [ -f "$UNIT" ]; then
        systemctl --user disable --now vi-ime.service >/dev/null 2>&1 || true
        rm -f "$UNIT"
        systemctl --user daemon-reload || true
    fi
    rm -f "$DESKTOP" "$ICON"
    say "Đã gỡ menu, icon và tự khởi động."
    say "Giữ lại: $DEST (xoá tay nếu muốn) và cấu hình $CONF/vi-ime/."
}

case "$ACTION" in
    install) install_app ;;
    uninstall) uninstall_app ;;
    *) echo "dùng: install [--autostart] | uninstall" >&2; exit 2 ;;
esac

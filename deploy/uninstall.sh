#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later OR Commercial
# Copyright (c) 2024-2026 vi-im contributors
# ============================================================================
# uninstall.sh — Remove vi-ime completely
# ============================================================================
set -euo pipefail

BIN="${HOME}/.local/bin/vi-ime"
CONF="${XDG_CONFIG_HOME:-$HOME/.config}/vi-ime/setting.conf"
SYSTEMD="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/vi-ime.service"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}/vi-ime"

echo "============================================"
echo " vi-ime — Uninstall"
echo "============================================"

# Stop & disable service
if systemctl --user is-active vi-ime.service &>/dev/null; then
    echo "[1/5] Stopping service..."
    systemctl --user stop vi-ime.service
    echo "  ✓ Stopped"
fi

if systemctl --user is-enabled vi-ime.service &>/dev/null; then
    echo "[2/5] Disabling service..."
    systemctl --user disable vi-ime.service
    echo "  ✓ Disabled"
fi

# Remove files
echo "[3/5] Removing files..."
rm -f "$BIN" && echo "  ✓ $BIN" || echo "  - $BIN (not found)"
rm -f "$CONF" && echo "  ✓ $CONF" || echo "  - $CONF (not found)"
rm -f "$SYSTEMD" && echo "  ✓ $SYSTEMD" || echo "  - $SYSTEMD (not found)"
rm -rf "$DATA" && echo "  ✓ $DATA" || echo "  - $DATA (not found)"

# Reload systemd
echo "[4/5] Reloading systemd..."
systemctl --user daemon-reload 2>/dev/null || true
echo "  ✓ Reloaded"

# Remove udev (if root)
echo "[5/5] Removing udev rules..."
if [ "$EUID" -eq 0 ] || sudo -n true 2>/dev/null; then
    sudo rm -f /etc/udev/rules.d/99-vi-ime.rules
    sudo udevadm control --reload-rules
    echo "  ✓ Udev rules removed"
else
    echo "  ⚠ Not root — manually: sudo rm /etc/udev/rules.d/99-vi-ime.rules"
fi

# Restore rival IME autostart entries that install.sh shadowed with a
# Hidden=true override. We only delete overrides that are EXACTLY our shadow
# (a 2-line Hidden stub), never a real user autostart entry.
echo "[6/6] Restoring rival IME autostart..."
AUTOSTART="${XDG_CONFIG_HOME:-$HOME/.config}/autostart"
restored=0
if [ -d "$AUTOSTART" ]; then
    for f in "$AUTOSTART"/*.desktop; do
        [ -e "$f" ] || continue
        base=$(basename "$f")
        case "$base" in
            *[Ff]citx*|*[Ii][Bb]us*|*gcin*|*hime*|*nimf*|*uim*)
                # Our shadow is a tiny Hidden=true stub with no Exec line.
                if ! grep -q '^Exec=' "$f" && grep -q '^Hidden=true' "$f"; then
                    rm -f "$f" && restored=$((restored + 1))
                fi
                ;;
        esac
    done
fi
echo "  ✓ Restored $restored rival autostart entr$([ "$restored" -eq 1 ] && echo y || echo ies)"
echo "    (fcitx5/ibus sẽ tự chạy lại lần login sau nếu chúng vẫn được cài)"

echo ""
echo "============================================"
echo " Uninstall complete. Bye! 👋"
echo "============================================"

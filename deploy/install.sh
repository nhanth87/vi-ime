#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later OR Commercial
# Copyright (c) 2024-2026 vi-im contributors
# ============================================================================
# install.sh — vi-im install script (POSIX sh)
#
# Usage:
#   ./deploy/install.sh                        # full install
#   ./deploy/install.sh --systemd              # systemd service only
#   ./deploy/install.sh --autostart            # XDG autostart only
#   ./deploy/install.sh --systemd --autostart  # both (explicit)
#   ./deploy/install.sh --uninstall            # remove everything
#   ./deploy/install.sh --help                 # this help
#
# Default behavior when no flags given:
#   - systemd available  → install systemd user service
#   - systemd absent     → fall back to XDG autostart
#   - Always install binary + config
#
# Environment overrides:
#   VI_IM_BIN_DIR         binary destination  (default: ~/.local/bin)
#   VI_IM_CONFIG_DIR      config destination  (default: ~/.config/vi-ime)
#   VI_IM_SYSTEMD_DIR     systemd user dir    (default: ~/.config/systemd/user)
#   VI_IM_AUTOSTART_DIR   autostart dir       (default: ~/.config/autostart)
# ============================================================================
set -eu

# ── Paths ────────────────────────────────────────────────────────────────
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

BIN_SRC="${PROJECT_DIR}/target/release/vi-ime"
SETTINGS_SRC="${PROJECT_DIR}/target/release/vi-settings"

BIN_DIR="${VI_IM_BIN_DIR:-${HOME}/.local/bin}"
BIN_DST="${BIN_DIR}/vi-ime"
SETTINGS_DST="${BIN_DIR}/vi-settings"

CONFIG_DIR="${VI_IM_CONFIG_DIR:-${XDG_CONFIG_HOME:-${HOME}/.config}/vi-ime}"
CONFIG_DST="${CONFIG_DIR}/setting.conf"
CONFIG_SRC="${PROJECT_DIR}/setting.conf"

SYSTEMD_DIR="${VI_IM_SYSTEMD_DIR:-${XDG_CONFIG_HOME:-${HOME}/.config}/systemd/user}"
SYSTEMD_ENV_SRC="${SCRIPT_DIR}/systemd/vi-im-wayland-env.service"
SYSTEMD_ENV_DST="${SYSTEMD_DIR}/vi-im-wayland-env.service"
SYSTEMD_MAIN_SRC="${SCRIPT_DIR}/systemd/vi-im.service"
SYSTEMD_MAIN_DST="${SYSTEMD_DIR}/vi-im.service"

AUTOSTART_DIR="${VI_IM_AUTOSTART_DIR:-${XDG_CONFIG_HOME:-${HOME}/.config}/autostart}"
AUTOSTART_SRC="${SCRIPT_DIR}/autostart/vi-im.desktop"
AUTOSTART_DST="${AUTOSTART_DIR}/vi-im.desktop"

DATA_DIR="${XDG_DATA_HOME:-${HOME}/.local/share}/vi-ime"

# ── Output helpers (terminal-safe, POSIX) ───────────────────────────────
bold=""; red=""; green=""; yellow=""; blue=""; cyan=""; reset=""
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
    bold="$(printf '\033[1m')"
    red="$(printf '\033[0;31m')"
    green="$(printf '\033[0;32m')"
    yellow="$(printf '\033[1;33m')"
    blue="$(printf '\033[0;34m')"
    cyan="$(printf '\033[0;36m')"
    reset="$(printf '\033[0m')"
fi

log_info()    { printf "%s  →%s %s\n" "${blue}" "${reset}" "$*"; }
log_ok()      { printf "%s  ✓%s %s\n" "${green}" "${reset}" "$*"; }
log_warn()    { printf "%s  ⚠%s %s\n" "${yellow}" "${reset}" "$*"; }
log_err()     { printf "%s  ✗%s %s\n" "${red}" "${reset}" "$*"; }
log_hdr()     { printf "\n%s%s%s\n" "${bold}" "$*" "${reset}"; }
log_fatal()   { log_err "$*"; exit 1; }

# ── Flags ────────────────────────────────────────────────────────────────
FLAG_SYSTEMD=false
FLAG_AUTOSTART=false
FLAG_UNINSTALL=false
FLAG_HELP=false
HAS_EXPLICIT_MODE=false

for arg in "$@"; do
    case "$arg" in
        --systemd)   FLAG_SYSTEMD=true; HAS_EXPLICIT_MODE=true ;;
        --autostart) FLAG_AUTOSTART=true; HAS_EXPLICIT_MODE=true ;;
        --uninstall) FLAG_UNINSTALL=true ;;
        --help|-h)   FLAG_HELP=true ;;
        *)
            printf "Unknown flag: %s\n" "$arg"
            printf "Try: %s --help\n" "$0"
            exit 1
            ;;
    esac
done

if $FLAG_HELP; then
    printf '%s\n' \
    "vi-im install script" \
    "" \
    "Usage: $0 [FLAGS]" \
    "" \
    "Flags:" \
    "  --systemd     Install systemd user service" \
    "  --autostart   Install XDG autostart .desktop" \
    "  --uninstall   Remove everything installed" \
    "  --help        Show this help" \
    "" \
    "Default (no flags):" \
    "  - systemd available → install systemd service" \
    "  - systemd absent   → fall back to XDG autostart" \
    "  - Install binary  → ${BIN_DST}" \
    "  - Install config  → ${CONFIG_DST}" \
    "" \
    "Compositors automatically detected:" \
    "  niri, Hyprland, Sway, KWin (KDE), Cosmic, GNOME/Mutter"
    exit 0
fi

# ══════════════════════════════════════════════════════════════════════════
# UNINSTALL
# ══════════════════════════════════════════════════════════════════════════
if $FLAG_UNINSTALL; then
    log_hdr "vi-im — Uninstall"

    if command -v systemctl >/dev/null 2>&1; then
        systemctl --user stop vi-im.service 2>/dev/null || true
        systemctl --user disable vi-im.service 2>/dev/null || true
        systemctl --user stop vi-im-wayland-env.service 2>/dev/null || true
        systemctl --user disable vi-im-wayland-env.service 2>/dev/null || true
        systemctl --user daemon-reload 2>/dev/null || true
        log_ok "systemd services stopped and disabled"
    fi

    for f in "${SYSTEMD_MAIN_DST}" "${SYSTEMD_ENV_DST}"; do
        if [ -f "$f" ]; then
            rm -f "$f" && log_ok "Removed $f"
        fi
    done

    if [ -f "${AUTOSTART_DST}" ]; then
        rm -f "${AUTOSTART_DST}" && log_ok "Removed ${AUTOSTART_DST}"
    fi

    for f in "${BIN_DST}" "${SETTINGS_DST}"; do
        if [ -f "$f" ]; then
            rm -f "$f" && log_ok "Removed $f"
        fi
    done

    if [ -d "${CONFIG_DIR}" ]; then
        log_warn "Config preserved at ${CONFIG_DIR}/ (remove manually if desired)"
    fi
    if [ -d "${DATA_DIR}" ]; then
        log_warn "Data preserved at ${DATA_DIR}/ (remove manually if desired)"
    fi

    printf '\n%s%sUninstall complete.%s\n' "${green}" "${bold}" "${reset}"
    exit 0
fi

# ══════════════════════════════════════════════════════════════════════════
# INSTALL
# ══════════════════════════════════════════════════════════════════════════
log_hdr "vi-im — Install"

# ── Detect systemd ───────────────────────────────────────────────────────
HAS_SYSTEMD=false
if command -v systemctl >/dev/null 2>&1; then
    if systemctl --user >/dev/null 2>&1; then
        HAS_SYSTEMD=true
    fi
fi

DO_SYSTEMD=false
DO_AUTOSTART=false

if $HAS_EXPLICIT_MODE; then
    DO_SYSTEMD=$FLAG_SYSTEMD
    DO_AUTOSTART=$FLAG_AUTOSTART
else
    if $HAS_SYSTEMD; then
        DO_SYSTEMD=true
        log_info "systemd detected — will install systemd user service"
    else
        DO_AUTOSTART=true
        log_warn "systemd not available — falling back to XDG autostart"
    fi
fi

# ── 1. Install binary ────────────────────────────────────────────────────
log_hdr "[1/4] Installing binary"

if [ ! -f "${BIN_SRC}" ]; then
    log_fatal "Binary not found: ${BIN_SRC}. Run ./deploy/compile.sh first."
fi

mkdir -p "${BIN_DIR}"
cp "${BIN_SRC}" "${BIN_DST}"
chmod 755 "${BIN_DST}"
log_ok "${BIN_DST}"

if [ -f "${SETTINGS_SRC}" ]; then
    cp "${SETTINGS_SRC}" "${SETTINGS_DST}"
    chmod 755 "${SETTINGS_DST}"
    log_ok "${SETTINGS_DST}"
else
    log_warn "vi-settings binary not found — 'Open Settings' will not work"
fi

# ── 2. Install config ────────────────────────────────────────────────────
log_hdr "[2/4] Installing config"

mkdir -p "${CONFIG_DIR}"

if [ -f "${CONFIG_DST}" ]; then
    log_warn "Config exists, backing up to ${CONFIG_DST}.bak"
    cp "${CONFIG_DST}" "${CONFIG_DST}.bak"
fi

if [ -f "${CONFIG_SRC}" ]; then
    cp "${CONFIG_SRC}" "${CONFIG_DST}"
    log_ok "${CONFIG_DST}"
else
    cat > "${CONFIG_DST}" << 'TOML'
# vi-im configuration — edit and save; daemon reloads automatically
# Full docs: https://github.com/meodien/vi-im

input_method = "Telex"
enabled = true
output_mode = "UnicodeDungSan"
free_tone_placement = true
auto_detect_lang = true
enable_per_app = false
ime_mode = "Hybrid"
tone_style = "Classic"
TOML
    log_ok "${CONFIG_DST} (generated default)"
fi

# ── 3a. Install systemd ──────────────────────────────────────────────────
if $DO_SYSTEMD; then
    log_hdr "[3/4] Installing systemd user service"

    if ! command -v systemctl >/dev/null 2>&1; then
        log_fatal "systemctl not found — cannot install systemd service"
    fi

    mkdir -p "${SYSTEMD_DIR}"

    if [ -f "${SYSTEMD_ENV_SRC}" ]; then
        cp "${SYSTEMD_ENV_SRC}" "${SYSTEMD_ENV_DST}"
        log_ok "${SYSTEMD_ENV_DST}"
    else
        log_warn "Missing ${SYSTEMD_ENV_SRC} — skipping env service"
    fi

    if [ -f "${SYSTEMD_MAIN_SRC}" ]; then
        cp "${SYSTEMD_MAIN_SRC}" "${SYSTEMD_MAIN_DST}"
        log_ok "${SYSTEMD_MAIN_DST}"
    else
        log_fatal "Missing ${SYSTEMD_MAIN_SRC}"
    fi

    systemctl --user daemon-reload 2>/dev/null || true
    log_ok "systemd user daemon reloaded"

    if [ -f "${SYSTEMD_ENV_DST}" ]; then
        systemctl --user enable vi-im-wayland-env.service 2>/dev/null || true
        log_ok "Enabled vi-im-wayland-env.service"
    fi
    systemctl --user enable vi-im.service 2>/dev/null || true
    log_ok "Enabled vi-im.service"

    if [ -n "${WAYLAND_DISPLAY:-}" ]; then
        if [ -f "${SYSTEMD_ENV_DST}" ]; then
            systemctl --user start vi-im-wayland-env.service 2>/dev/null || true
        fi
        systemctl --user restart vi-im.service 2>/dev/null || true
        log_ok "Started vi-im.service (WAYLAND_DISPLAY=${WAYLAND_DISPLAY})"
    else
        log_warn "Not in Wayland — will start on next Wayland login"
    fi

    printf '\n'
    log_info "Useful commands:"
    printf "    Start:   systemctl --user start vi-im\n"
    printf "    Stop:    systemctl --user stop vi-im\n"
    printf "    Status:  systemctl --user status vi-im\n"
    printf "    Logs:    journalctl --user -u vi-im -f\n"
fi

# ── 3b. Install XDG autostart ────────────────────────────────────────────
if $DO_AUTOSTART; then
    log_hdr "[3/4] Installing XDG autostart"

    mkdir -p "${AUTOSTART_DIR}"

    if [ -f "${AUTOSTART_SRC}" ]; then
        if [ -f "${AUTOSTART_DST}" ]; then
            log_warn "Existing autostart file, backing up to ${AUTOSTART_DST}.bak"
            cp "${AUTOSTART_DST}" "${AUTOSTART_DST}.bak"
        fi
        cp "${AUTOSTART_SRC}" "${AUTOSTART_DST}"
        log_ok "${AUTOSTART_DST}"
    else
        log_fatal "Missing ${AUTOSTART_SRC}"
    fi

    log_info "vi-im will auto-start on next Wayland login via XDG autostart"
fi

# ── 4. Compositor detection + notes ──────────────────────────────────────
log_hdr "[4/4] Compositor setup notes"

compositor="Unknown"
if pgrep -x niri >/dev/null 2>&1; then
    compositor="Niri"
elif pgrep -x Hyprland >/dev/null 2>&1; then
    compositor="Hyprland"
elif pgrep -x sway >/dev/null 2>&1; then
    compositor="Sway"
elif pgrep -x kwin_wayland >/dev/null 2>&1; then
    compositor="KWin"
elif pgrep -x cosmic-comp >/dev/null 2>&1; then
    compositor="Cosmic"
elif pgrep -x gnome-shell >/dev/null 2>&1; then
    compositor="GNOME/Mutter"
elif pgrep -x labwc >/dev/null 2>&1; then
    compositor="Labwc"
elif pgrep -x river >/dev/null 2>&1; then
    compositor="River"
fi

printf '  Detected compositor: %s%s%s\n' "${bold}" "${compositor}" "${reset}"
printf '\n'

case "${compositor}" in
    Niri)
        log_info "Niri: ensure your config.kdl starts the IME:"
        printf '    %s\n' 'spawn-at-startup "systemctl" "--user" "start" "vi-im.service"'
        printf '\n'
        ;;
    Hyprland)
        log_info "Hyprland: add to hyprland.conf:"
        printf '    %s\n' 'exec-once = systemctl --user start vi-im.service'
        printf '\n'
        log_warn "Hyprland: ensure input-method-v2 is available (Hyprland ≥0.45)"
        printf '\n'
        ;;
    Sway)
        log_info "Sway: add to config (~/.config/sway/config):"
        printf '    %s\n' 'exec systemctl --user start vi-im.service'
        printf '\n'
        log_warn "Sway: text-input-v3 requires wlroots ≥0.17"
        printf '\n'
        ;;
    KWin)
        log_info "KWin/KDE: vi-im auto-starts via systemd."
        log_info "For manual: add as Login Script in System Settings."
        log_warn "KWin: virtual-keyboard may need manual activation:"
        printf '    %s\n' 'Settings → Input Devices → Virtual Keyboard → vi-im'
        printf '\n'
        ;;
    Cosmic)
        log_info "Cosmic: add to startup or enable systemd:"
        printf '    %s\n' 'systemctl --user enable --now vi-im.service'
        printf '\n'
        ;;
    GNOME/Mutter)
        log_info "GNOME: vi-im runs as systemd user service."
        log_warn "GNOME: may conflict with ibus. To disable ibus:"
        printf '    %s\n' 'gsettings set org.gnome.desktop.input-sources sources "[]"'
        printf '\n'
        ;;
    *)
        log_info "Unknown compositor — vi-im will start via chosen method."
        log_info "If the IME doesn't work, ensure compositor supports:"
        printf '    %s\n' '- zwp_input_method_v2'
        printf '    %s\n' '- zwp_text_input_v3'
        printf '    %s\n' '- zwp_virtual_keyboard_manager_v1'
        printf '\n'
        ;;
esac

# ── Summary ──────────────────────────────────────────────────────────────
echo ''
printf '%s%s══════════════════════════════════════════════════%s\n' "${green}" "${bold}" "${reset}"
printf '%s%s  vi-im installed successfully!%s\n' "${green}" "${bold}" "${reset}"
printf '%s%s══════════════════════════════════════════════════%s\n' "${green}" "${bold}" "${reset}"
echo ''

printf '  %sBinary:%s     %s\n' "${bold}" "${reset}" "${BIN_DST}"
printf '  %sConfig:%s     %s\n' "${bold}" "${reset}" "${CONFIG_DST}"

if $DO_SYSTEMD; then
    printf '  %sSystemd:%s   %s\n' "${bold}" "${reset}" "${SYSTEMD_MAIN_DST}"
    echo ''
    printf '  %sStart now:%s  systemctl --user start vi-im\n' "${bold}" "${reset}"
elif $DO_AUTOSTART; then
    printf '  %sAutostart:%s %s\n' "${bold}" "${reset}" "${AUTOSTART_DST}"
    echo ''
    printf '  %sStart now:%s  %s &\n' "${bold}" "${reset}" "${BIN_DST}"
fi

printf '  %sSettings:%s   vi-settings\n' "${bold}" "${reset}"
printf '  %sLogs:%s      journalctl --user -u vi-im -f\n' "${bold}" "${reset}"
echo ''
printf '%s%sDone!%s Restart your session or use the Start command above.\n' "${green}" "${bold}" "${reset}"

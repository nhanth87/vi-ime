#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later OR Commercial
# Copyright (c) 2024-2026 vi-im contributors
set -euo pipefail

# ─────────────────────────────────────────────────────────────────────
# vi-im — Vietnamese IME for Wayland Linux
# One-command install: curl -fsSL https://raw.github.../install.sh | bash
# ─────────────────────────────────────────────────────────────────────

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

INSTALL_DIR="${VI_IM_HOME:-$HOME/.local/share/vi-im}"
BIN_DIR="${VI_IM_BIN:-$HOME/.local/bin}"
CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/vi-ime"
SYSTEMD_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
REPO_URL="${VI_IM_REPO:-https://github.com/meodien/vi-im.git}"
BRANCH="${VI_IM_BRANCH:-main}"

TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

# ─────────────────────────────────────────────────────────────────────
# Banner
# ─────────────────────────────────────────────────────────────────────
banner() {
    echo ""
    echo -e "${CYAN}${BOLD}   ╔══════════════════════════════════════╗${NC}"
    echo -e "${CYAN}${BOLD}   ║          vi-im installer             ║${NC}"
    echo -e "${CYAN}${BOLD}   ║   Vietnamese IME for Wayland Linux   ║${NC}"
    echo -e "${CYAN}${BOLD}   ╚══════════════════════════════════════╝${NC}"
    echo ""
}

# ─────────────────────────────────────────────────────────────────────
# Helpers
# ─────────────────────────────────────────────────────────────────────
info()    { echo -e "  ${GREEN}✓${NC} $1"; }
warn()    { echo -e "  ${YELLOW}⚠${NC} $1"; }
error()   { echo -e "  ${RED}✗${NC} $1"; }
step()    { echo -e "\n${BOLD}── $1 ──${NC}"; }
fatal()   { error "$1"; exit 1; }

command_exists() { command -v "$1" &>/dev/null; }

# ─────────────────────────────────────────────────────────────────────
# Dependency checks
# ─────────────────────────────────────────────────────────────────────
check_deps() {
    step "Checking system dependencies"

    local missing=()

    # Rust toolchain
    if command_exists rustc && command_exists cargo; then
        local rust_ver
        rust_ver=$(rustc --version | grep -oP '\d+\.\d+' | head -1)
        if [ "$(echo "$rust_ver >= 1.80" | bc 2>/dev/null || echo 0)" = "0" ]; then
            warn "Rust $rust_ver detected — recommend ≥ 1.80. Will try anyway."
        else
            info "Rust $rust_ver"
        fi
    else
        missing+=("rustup (curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh)")
    fi

    # System deps
    for pkg in pkg-config gcc cmake; do
        if command_exists "$pkg"; then
            info "$pkg found"
        else
            missing+=("$pkg")
        fi
    done

    # DBus dev headers (needed to build the ksni system-tray icon)
    if pkg-config --exists dbus-1 2>/dev/null; then
        info "dbus-1 (tray icon)"
    else
        missing+=("libdbus-1-dev (or dbus development headers)")
    fi

    # quickshell — runs the QML config window (tray → Cài đặt). Optional: the
    # IME + tray work without it; only the settings window needs it.
    if command_exists quickshell; then
        info "quickshell (settings window)"
    else
        warn "quickshell not found — IME + tray vẫn chạy; chỉ cửa sổ Cài đặt cần nó."
    fi

    # systemd
    if command_exists systemctl; then
        info "systemd"
    else
        warn "systemd not found — autostart via other means (crontab, ~/.profile)"
    fi

    if [ ${#missing[@]} -gt 0 ]; then
        echo ""
        error "Missing dependencies. Install them first:"
        echo ""
        if command_exists apt; then
            echo "    sudo apt install ${missing[*]}"
        elif command_exists dnf; then
            echo "    sudo dnf install ${missing[*]}"
        elif command_exists pacman; then
            echo "    sudo pacman -S ${missing[*]}"
        else
            echo "    (unknown package manager — install: ${missing[*]})"
        fi
        echo ""
        fatal "Dependency check failed"
    fi

    echo ""
}

# ─────────────────────────────────────────────────────────────────────
# Clone & build
# ─────────────────────────────────────────────────────────────────────
build_project() {
    step "Cloning & building vi-im"

    cd "$TMP_DIR"

    if [ -d "$INSTALL_DIR/.git" ]; then
        info "Updating existing repo at $INSTALL_DIR"
        git -C "$INSTALL_DIR" pull --ff-only origin "$BRANCH" 2>/dev/null || {
            warn "Pull failed, doing fresh clone"
            rm -rf "$INSTALL_DIR"
            git clone --depth 1 --branch "$BRANCH" "$REPO_URL" "$INSTALL_DIR"
        }
    else
        info "Cloning $REPO_URL → $INSTALL_DIR"
        mkdir -p "$(dirname "$INSTALL_DIR")"
        git clone --depth 1 --branch "$BRANCH" "$REPO_URL" "$INSTALL_DIR"
    fi

    cd "$INSTALL_DIR"

    info "Building release binaries (this may take 2-5 minutes)..."
    cargo build --release 2>&1 | while IFS= read -r line; do
        case "$line" in
            *Compiling*vi-engine*|*Compiling*vi-wayland*|*Compiling*vi-ime*|*Compiling*vi-settings*|*Compiling*vi-tray*)
                echo -e "  ${CYAN}…${NC} ${line#*Compiling }"
                ;;
        esac
    done

    if [ ! -f "target/release/vi-ime" ]; then
        fatal "Build failed — vi-ime binary not found"
    fi
    info "Build complete"

    # Install binaries
    mkdir -p "$BIN_DIR"
    cp -f target/release/vi-ime "$BIN_DIR/vi-ime"
    if [ -f target/release/vi-settings ]; then
        cp -f target/release/vi-settings "$BIN_DIR/vi-settings"
    fi
    chmod +x "$BIN_DIR/vi-ime" "$BIN_DIR/vi-settings" 2>/dev/null || true

    # Ensure BIN_DIR in PATH
    if [[ ":$PATH:" != *":$BIN_DIR:"* ]]; then
        warn "$BIN_DIR is not in your PATH."
        echo "       Add this to your ~/.bashrc or ~/.zshrc:"
        echo "       export PATH=\"$BIN_DIR:\$PATH\""
    fi

    echo ""
}

# ─────────────────────────────────────────────────────────────────────
# Default config
# ─────────────────────────────────────────────────────────────────────
setup_config() {
    step "Setting up config"

    mkdir -p "$CONFIG_DIR"
    local CONFIG_FILE="$CONFIG_DIR/setting.conf"

    if [ -f "$CONFIG_FILE" ]; then
        info "Config exists at $CONFIG_FILE (skipping)"
        return
    fi

    cat > "$CONFIG_FILE" << 'TOML'
# vi-im configuration — edit and save; daemon reloads automatically
# Full docs: https://github.com/meodien/vi-im

# Input method: "Telex" or "Vni"
input_method = "Telex"
# Start IME enabled at login
enabled = true
# Unicode output: "UnicodeDungSan" (NFC) or "UnicodeToHop" (NFD)
output_mode = "UnicodeDungSan"
# Allow free tone placement
free_tone_placement = true
# Auto-detect English vs Vietnamese
auto_detect_lang = true
# Per-app auto switching
enable_per_app = false
# IME mode: "Preedit", "NonPreedit", or "Hybrid"
ime_mode = "Hybrid"
# Tone style: "Classic" (hòa) or "Modern" (hoà)
tone_style = "Classic"

# Per-app overrides (examples — uncomment to use)
# [app_configs."kitty"]
# method = "Telex"
# mode = "NonPreedit"
#
# [app_configs."chromium-browser"]
# method = "Telex"
# mode = "Hybrid"
TOML

    info "Default config written to $CONFIG_FILE"
}

# ─────────────────────────────────────────────────────────────────────
# Take over as the SOLE input method
# zwp_input_method_v2 is single-owner per seat — a running fcitx5/ibus grabs
# it first, so vi-ime would get nothing. We stop rivals + disable their
# autostart so vi-ime owns the seat. We NEVER set GTK_IM_MODULE/QT_IM_MODULE
# (project policy) — we only remove competitors.
# ─────────────────────────────────────────────────────────────────────
disable_rivals() {
    step "Taking over as the sole input method"

    # 1) systemd --user units for known rivals.
    for svc in fcitx5 fcitx ibus; do
        if systemctl --user cat "${svc}.service" &>/dev/null; then
            systemctl --user disable --now "${svc}.service" &>/dev/null \
                && info "disabled ${svc}.service"
        fi
    done

    # 2) Shadow rival XDG autostart entries with a Hidden=true override in the
    #    user autostart dir (wins over /etc/xdg/autostart). Never shadow ours.
    mkdir -p "$HOME/.config/autostart"
    local hidden=0
    for d in /etc/xdg/autostart/*.desktop "$HOME/.config/autostart/"*.desktop; do
        [ -e "$d" ] || continue
        local base; base=$(basename "$d")
        case "$base" in
            *vi-im*|*vi-ime*) continue ;;
            *[Ff]citx*|*[Ii][Bb]us*|*gcin*|*hime*|*nimf*|*uim*)
                printf '[Desktop Entry]\nHidden=true\n' \
                    > "$HOME/.config/autostart/$base"
                hidden=$((hidden + 1))
                ;;
        esac
    done
    if [ "$hidden" -gt 0 ]; then
        info "hid $hidden rival autostart entr$([ "$hidden" -eq 1 ] && echo y || echo ies)"
    else
        info "no rival autostart entries found"
    fi

    # 3) Stop any live rival now so vi-ime can grab the seat immediately.
    if [ -x "$BIN_DIR/vi-ime" ]; then
        "$BIN_DIR/vi-ime" --take-over 2>/dev/null || true
    fi

    # 4) Warn (not touch) about env-var IME modules that can misroute apps to a
    #    now-dead IME. We do not edit these (policy) — the user must.
    if [ -n "${GTK_IM_MODULE:-}" ] && [ "${GTK_IM_MODULE}" != "wayland" ]; then
        warn "GTK_IM_MODULE=$GTK_IM_MODULE — có thể ép app dùng IME cũ."
        echo  "       Bỏ nó trong ~/.config/environment.d/*.conf hoặc ~/.pam_environment rồi logout/login."
    fi
    if [ -n "${QT_IM_MODULE:-}" ] && [ "${QT_IM_MODULE}" != "wayland" ]; then
        warn "QT_IM_MODULE=$QT_IM_MODULE — tương tự, cân nhắc bỏ."
    fi
}

# ─────────────────────────────────────────────────────────────────────
# `input` group — optional, for the physical mouse-click watcher
# (drops a half-typed word the instant the mouse clicks, even in apps
# that report nothing over text-input-v3). Reads raw /dev/input/event*,
# which on most distros requires group membership. This is the ONE step
# in the whole installer that needs root, so it is asked for explicitly
# — never run silently, never assumed.
# ─────────────────────────────────────────────────────────────────────
setup_input_group() {
    step "Click-detect (optional): quyền đọc /dev/input"

    if ! command_exists id; then
        return
    fi
    if id -nG "$USER" 2>/dev/null | grep -qw input; then
        info "đã ở nhóm 'input' — click-detect sẵn sàng"
        return
    fi
    if ! command_exists sudo; then
        warn "không có sudo — bỏ qua click-detect (IME vẫn hoạt động bình thường)"
        return
    fi

    echo "  vi-im có thể tự phát hiện lúc bạn bấm chuột (kể cả trong app không"
    echo "  báo tín hiệu gì cho IME), để tránh chữ đang gõ dở bị chèn sai vị trí."
    echo "  Việc này cần quyền đọc /dev/input — tức là vào nhóm hệ thống 'input':"
    echo ""
    echo -e "      ${CYAN}sudo usermod -aG input \"$USER\"${NC}"
    echo ""
    read -r -p "  Chạy lệnh trên ngay bây giờ? [y/N] " reply
    case "$reply" in
        [yY]|[yY][eE][sS])
            if sudo usermod -aG input "$USER"; then
                info "đã thêm $USER vào nhóm 'input'"
                warn "cần ĐĂNG XUẤT / ĐĂNG NHẬP LẠI để quyền có hiệu lực"
            else
                warn "usermod thất bại — click-detect sẽ tắt, IME vẫn chạy bình thường"
            fi
            ;;
        *)
            info "bỏ qua — chạy lại bất cứ lúc nào: sudo usermod -aG input \"$USER\""
            ;;
    esac
}

# ─────────────────────────────────────────────────────────────────────
# Systemd user service
# ─────────────────────────────────────────────────────────────────────
setup_systemd() {
    step "Setting up systemd user service"

    if ! command_exists systemctl; then
        warn "No systemd — skipping service setup"
        echo "       Start manually: vi-ime &"
        return
    fi

    mkdir -p "$SYSTEMD_DIR"
    local SERVICE_FILE="$SYSTEMD_DIR/vi-ime.service"

    cat > "$SERVICE_FILE" << UNITFILE
[Unit]
Description=vi-im Vietnamese IME daemon
Documentation=https://github.com/meodien/vi-im
After=graphical-session.target
PartOf=graphical-session.target
# Sole-IME: never run alongside a rival that would own the seat first.
Conflicts=fcitx5.service fcitx.service ibus.service

[Service]
Type=simple
# Stop any rival that slipped in before we grab the input-method seat.
ExecStartPre=-$BIN_DIR/vi-ime --take-over
ExecStart=$BIN_DIR/vi-ime
ExecReload=/bin/kill -HUP \$MAINPID
Restart=on-failure
RestartSec=3
# WAYLAND_DISPLAY is intentionally NOT set here: this is a plain user
# service (no @instance), so a hardcoded value would be wrong for every
# session but the one that happened to write this file, and %i expands to
# nothing on a non-templated unit. niri/Hyprland/Sway session startup
# already runs \`systemctl --user import-environment WAYLAND_DISPLAY\`
# (or dbus-update-activation-environment) before graphical-session.target
# fires, so the variable is inherited correctly without us touching it.

# Security hardening
NoNewPrivileges=yes
PrivateTmp=yes
ProtectSystem=strict
ProtectHome=read-only
ReadWritePaths=$CONFIG_DIR
ReadOnlyPaths=$HOME/.local/state

[Install]
WantedBy=graphical-session.target
UNITFILE

    systemctl --user daemon-reload 2>/dev/null || true
    systemctl --user enable vi-ime.service 2>/dev/null || true

    info "systemd user service installed"
    echo "       Start:  systemctl --user start vi-ime"
    echo "       Stop:   systemctl --user stop vi-ime"
    echo "       Status: systemctl --user status vi-ime"
    echo "       Logs:   journalctl --user -u vi-ime -f"
}

# ─────────────────────────────────────────────────────────────────────
# Desktop entry — makes "vi-im Settings" show up in app launchers
# (wofi/rofi/GNOME overview/…), not just the tray. Icon itself is
# installed by the daemon at first run (crates/vi-daemon/src/tray.rs
# install_icons()) so it doesn't need duplicating here.
# ─────────────────────────────────────────────────────────────────────
install_desktop_entry() {
    step "Đăng ký vi-im Settings vào app launcher"

    local APPS_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
    mkdir -p "$APPS_DIR"
    cat > "$APPS_DIR/vi-im-settings.desktop" << DESKTOP
[Desktop Entry]
Type=Application
Name=vi-im Settings
Comment=Cấu hình bộ gõ tiếng Việt vi-im
Exec=$BIN_DIR/vi-settings
Icon=vi-im
Terminal=false
Categories=Settings;Utility;
NoDisplay=false
DESKTOP

    info "desktop entry: $APPS_DIR/vi-im-settings.desktop"
}

# ─────────────────────────────────────────────────────────────────────
# PATH reminder
# ─────────────────────────────────────────────────────────────────────
path_reminder() {
    local PROFILE_FILE=""
    for f in "$HOME/.zshrc" "$HOME/.bashrc" "$HOME/.profile"; do
        [ -f "$f" ] && { PROFILE_FILE="$f"; break; }
    done

    if [ -n "$PROFILE_FILE" ]; then
        if ! grep -q "$BIN_DIR" "$PROFILE_FILE" 2>/dev/null; then
            echo ""
            echo -e "${YELLOW}──────────────────────────────────────────────────────${NC}"
            echo -e "${BOLD}Add to PATH${NC} — append this to ${CYAN}$PROFILE_FILE${NC}:"
            echo ""
            echo -e "  ${GREEN}export PATH=\"$BIN_DIR:\$PATH\"${NC}"
            echo -e "${YELLOW}──────────────────────────────────────────────────────${NC}"
        fi
    fi
}

# ─────────────────────────────────────────────────────────────────────
# Summary
# ─────────────────────────────────────────────────────────────────────
summary() {
    echo ""
    echo -e "${GREEN}${BOLD}╔══════════════════════════════════════════════════╗${NC}"
    echo -e "${GREEN}${BOLD}║           vi-im installed successfully!          ║${NC}"
    echo -e "${GREEN}${BOLD}╚══════════════════════════════════════════════════╝${NC}"
    echo ""
    echo -e "  ${BOLD}Binaries:${NC}    $BIN_DIR/"
    echo -e "  ${BOLD}Config:${NC}      $CONFIG_DIR/setting.conf"
    echo -e "  ${BOLD}Source:${NC}      $INSTALL_DIR"
    echo ""
    echo -e "  ${BOLD}Start now:${NC}    systemctl --user start vi-ime"
    echo -e "  ${BOLD}Settings:${NC}     vi-settings"
    echo -e "  ${BOLD}Logs:${NC}        journalctl --user -u vi-ime -f"
    echo ""
}

# ─────────────────────────────────────────────────────────────────────
# Main
# ─────────────────────────────────────────────────────────────────────
banner
check_deps
build_project
setup_config
disable_rivals
setup_input_group
setup_systemd
install_desktop_entry
path_reminder
summary

echo -e "${GREEN}Done!${NC} Restart your session or run: ${BOLD}systemctl --user start vi-ime${NC}"

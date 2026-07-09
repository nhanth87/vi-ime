# 🚀 Deploy vi-im — Vietnamese Wayland IME

## Quick Start

```bash
# Build from source (~2-5 phút lần đầu)
./deploy/compile.sh

# Install (auto-detects systemd vs autostart)
./deploy/install.sh

# Or pick a specific method:
./deploy/install.sh --systemd       # systemd user service only
./deploy/install.sh --autostart     # XDG autostart only
./deploy/install.sh --uninstall     # remove everything
./deploy/install.sh --help          # show all options
```

## Files

| File | Purpose |
|------|---------|
| `compile.sh` | Build from source → `target/release/vi-ime` |
| `install.sh` | Install binary, config, systemd, autostart (POSIX sh) |
| `uninstall.sh` | Legacy uninstaller (use `install.sh --uninstall`) |
| `vi-ime.service` | Legacy systemd unit (replaced by `systemd/vi-im.service`) |
| `99-vi-ime.rules` | Udev rules (auto-restart on keyboard hotplug) |
| `systemd/vi-im.service` | **systemd user service** — main IME daemon |
| `systemd/vi-im-wayland-env.service` | **systemd env service** — propagate Wayland env vars |
| `autostart/vi-im.desktop` | **XDG autostart** — fallback for non-systemd setups |

## Installed paths

```
~/.local/bin/vi-ime                       ← Binary
~/.local/bin/vi-settings                     ← Settings window
~/.config/vi-ime/setting.conf                ← Config
~/.config/systemd/user/vi-im.service         ← Systemd (main)
~/.config/systemd/user/vi-im-wayland-env.service ← Systemd (env)
~/.config/autostart/vi-im.desktop            ← XDG autostart fallback
~/.local/share/vi-ime/godmod/                ← Debug logs (if enabled)
```

## Autostart decision flow

```
install.sh (no flags)
  │
  ├─ systemctl --user available?
  │   YES → install systemd units
  │          ├─ vi-im-wayland-env.service (oneshot, propagates env)
  │          └─ vi-im.service (simple, auto-restart on failure)
  │
  └─ NO  → fall back to XDG autostart
            └─ ~/.config/autostart/vi-im.desktop
```

## Useful commands

```bash
# Live logs
journalctl --user -u vi-im -f

# Restart after config change
systemctl --user restart vi-im

# Stop temporarily
systemctl --user stop vi-im

# Start
systemctl --user start vi-im

# Status
systemctl --user status vi-im

# Debug mode (verbose log)
RUST_LOG=debug vi-ime

# Godmod mode (log every keystroke)
VI_GODMOD=1 RUST_LOG=debug vi-ime
# Logs: ~/.local/share/vi-ime/godmod/
```

## Compositor-specific setup

### Niri
```kdl
// ~/.config/niri/config.kdl
spawn-at-startup "systemctl" "--user" "start" "vi-im.service"
```

### Hyprland
```
# ~/.config/hypr/hyprland.conf
exec-once = systemctl --user start vi-im.service
```

### Sway
```
# ~/.config/sway/config
exec systemctl --user start vi-im.service
```

### KDE / KWin
Installs via systemd. Enable virtual keyboard in:
Settings → Input Devices → Virtual Keyboard → vi-im

### GNOME / Mutter
```bash
# Disable ibus if it conflicts:
gsettings set org.gnome.desktop.input-sources sources "[]"
```

### Cosmic
```bash
systemctl --user enable --now vi-im.service
```

## System requirements

```bash
# Ubuntu/Debian
sudo apt install libxdo-dev libgtk-3-dev libwayland-dev

# Arch
sudo pacman -S xdotool gtk3 wayland

# Fedora
sudo dnf install xdotool gtk3 wayland-devel
```

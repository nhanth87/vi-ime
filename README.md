# vi-ime — Bộ gõ tiếng Việt cho Wayland

Bộ gõ tiếng Việt nhẹ, nhanh, chạy native trên Wayland (niri, Sway, Hyprland, COSMIC, river, labwc). Không dùng bảng Wolfe — dùng **Unicode algebra (NFD/NFC)** thuần thuật toán, table-free.

## Tính năng

- **3 kiểu gõ:** Telex, VNI, Tự do (chấp nhận cả Telex + VNI trong cùng từ)
- **Smart mode:** tự nhận diện English (test, user, sway, windows…) — không biến tiếng Anh thành tiếng Việt
- **Preedit-everywhere:** commit_string universal — hoạt động trên mọi app
- **Live mode (NonPreedit):** cho terminal, hiện chữ ngay không gạch chân
- **Per-app tự động:** terminal → NonPreedit, browser address bar → passthrough, password → off
- **Evdev fallback tự động:** Chrome/Chromium X11, LibreOffice, OnlyOffice — gõ được mà không cần config
- **Zero-CPU idle:** daemon ngủ hoàn toàn khi không gõ (blocking recv, không poll)
- **Autocorrect:** tự sửa lỗi chính tả phổ biến *(mặc định bật)*
- **Emoji shortcode:** gõ `:smile:` → 😄 *(mặc định bật)*
- **Clipboard convert:** chuyển đổi Unicode clipboard *(mặc định bật)*
- **Plugin system:** mở rộng cho gõ tắt, tiếng Hmong, 56 tiếng dân tộc Việt Nam
- **Tray icon:** quản lý qua StatusNotifierItem (KDE/GNOME/Niri panel)
- **Game mode:** auto-detect game → passthrough (Ctrl+Shift+G toggle)

## Compositor hỗ trợ

| Compositor | Focus tracking | Ghi chú |
|-----------|---------------|---------|
| niri | ✅ IPC event-stream (có PID) | Đầy đủ nhất |
| Sway | ✅ zwlr-foreign-toplevel | Không có PID |
| Hyprland | ✅ zwlr-foreign-toplevel | Không có PID |
| COSMIC | ✅ zwlr-foreign-toplevel | Không có PID |
| river / labwc / Wayfire | ✅ zwlr-foreign-toplevel | Generic wlroots |
| GNOME / KWin | ❌ | Dùng fcitx5/ibus thay thế |

## Cài đặt

### 1. Gỡ bỏ IME cũ (BẮT BUỘC)

vi-ime chiếm exclusive seat — không thể chạy song song fcitx5/ibus.

```bash
# Gỡ hoàn toàn fcitx (bao gồm fcitx-udev)
sudo apt remove --purge fcitx5 fcitx5-* fcitx-udev fcitx 2>/dev/null
sudo pacman -Rns fcitx5 fcitx5-configtool fcitx5-gtk fcitx5-qt 2>/dev/null
sudo dnf remove fcitx5* 2>/dev/null

# Gỡ hoàn toàn ibus
sudo apt remove --purge ibus ibus-* 2>/dev/null
sudo pacman -Rns ibus 2>/dev/null
sudo dnf remove ibus* 2>/dev/null

# Xóa biến môi trường cũ (trong ~/.bashrc, ~/.profile, /etc/environment)
# Xóa các dòng: GTK_IM_MODULE, QT_IM_MODULE, XMODIFIERS
```

### 2. Thêm user vào group input/uinput

Cần cho evdev fallback (LibreOffice, Chrome X11):

```bash
sudo usermod -aG input $USER
# Tạo udev rule cho uinput (nếu chưa có)
echo 'KERNEL=="uinput", GROUP="input", MODE="0660"' | sudo tee /etc/udev/rules.d/99-uinput.rules
sudo udevadm control --reload-rules
# Đăng xuất rồi đăng nhập lại để group có hiệu lực
```

### 3. Build & install

```bash
cargo build --release
# Copy binary
sudo cp target/release/vi-ime /usr/local/bin/
sudo cp target/release/vi-settings /usr/local/bin/
```

### 4. Autostart

```bash
mkdir -p ~/.config/autostart
cat > ~/.config/autostart/vi-ime.desktop << EOF
[Desktop Entry]
Name=vi-ime
Exec=/usr/local/bin/vi-ime
Type=Application
X-GNOME-Autostart-enabled=true
EOF
```

## Sử dụng

```bash
vi-ime              # Chạy daemon (tray icon)
vi-ime --toggle     # Bật/tắt IME
vi-ime --switch     # Chuyển Telex → VNI → Tự do
vi-ime --mode       # Chuyển Preedit ↔ NonPreedit
vi-ime --status     # Hiện trạng thái
vi-ime --doctor     # Chẩn đoán hệ thống
vi-ime --evdev      # Chế độ evdev thủ công (toàn bộ hệ thống)
vi-ime --take-over  # Dừng IME đối thủ (fcitx5/ibus)
```

### Phím tắt mặc định
- Tray icon middle-click: bật/tắt IME
- `Ctrl+Shift+G`: bật/tắt game mode

## Xử lý sự cố

### Bị kẹt ở ô password (IME nuốt phím)

Bộ gõ detect `ContentPurpose::Password` và tự tắt. Nếu app KHÔNG khai báo password field:

```bash
# Chuyển sang TTY khác
Ctrl+Alt+F3
# Kill vi-ime
pkill vi-ime
# Quay lại session
Ctrl+Alt+F1
```

### Chrome/Chromium không gõ được tiếng Việt

**Chrome chạy X11 (XWayland):** vi-ime tự detect và dùng evdev fallback (cần group `input`).

**Chrome chạy Wayland native:** Thêm flags:
```bash
# ~/.config/chrome-flags.conf hoặc command line:
--ozone-platform=wayland --enable-wayland-ime
```

### LibreOffice/OnlyOffice

Tự động dùng evdev fallback — chỉ cần user ở group `input`.

### Electron apps (Discord, VS Code, Slack…)

Thêm flags cho Wayland IME:
```bash
--ozone-platform=wayland --enable-wayland-ime
```

## Cấu trúc dự án

```
crates/
├── vi-daemon/         # Binary chính (IME engine + Wayland + evdev)
│   └── src/
│       ├── engine/    # Vietnamese engine (NFD algebra, table-free)
│       ├── wayland/   # zwp_input_method_v2 + virtual keyboard
│       ├── compositor/# Focus tracking (niri IPC, wlr-toplevel)
│       ├── config/    # 4-layer config resolution
│       ├── plugin/    # Plugin system (app plugins, abbreviation, languages)
│       └── data/      # Vietnamese syllables + English dictionary
└── vi-settings/       # QML settings UI (separate process)
```

## Thiết kế

- **Unicode algebra (R14):** Không dùng bảng Wolfe/VOWEL_CLUSTERS. Mỗi chữ Việt = `base × quality × tone`, tính toán qua NFC/NFD chuẩn Unicode.
- **One path (R14):** Một pipeline duy nhất cho Telex, VNI, và Tự do.
- **Preedit-everywhere (R2/R7):** `commit_string` thay thế preedit — universal.
- **Zero-CPU (R15):** Daemon ngủ hoàn toàn khi idle (no poll/timer).
- **Plugin middleware (R1):** Mọi mở rộng qua `pre_process_key` / `post_process_action`.

## License

GPL-3.0-only

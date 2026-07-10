<!--
SPDX-License-Identifier: GPL-3.0-or-later OR Commercial
Copyright (c) 2024-2026 vi-im contributors
-->

# vi-im — Bộ gõ tiếng Việt cho Wayland

Bộ gõ tiếng Việt viết mới hoàn toàn bằng Rust, nói chuyện thẳng với
`zwp_input_method_v2` — không qua IBus, không qua Fcitx. Zero-config: cài
là gõ được, tự nhận diện app đang focus để chọn cách hiển thị phù hợp.

Chạy trên mọi compositor **wlroots**: niri, Hyprland, Sway, COSMIC. (GNOME/KDE
không hỗ trợ đủ `input-method-v2` nên không nằm trong phạm vi dự án.)

## Tính năng

- **3 kiểu gõ**: Telex, VNI, và **Tự do** (trộn cả hai trong cùng một từ)
- **2 chế độ hiển thị**:
  - *Preedit* — gạch chân khi đang gõ, giống bộ gõ truyền thống
  - *NonPreedit* — gõ tới đâu hiện tới đó như Unikey trên Windows, không gạch
    chân, dùng bàn phím ảo với keymap sinh động theo từng từ (không phụ thuộc
    `delete_surrounding_text`, vốn nhiều app/terminal hỗ trợ không đầy đủ)
- **Tự thích ứng theo app**: terminal, trình duyệt, trang web (Facebook,
  Google Docs...) mỗi loại có cấu hình mặc định hợp lý sẵn — chỉnh tay khi
  cần qua `setting.conf` hoặc cửa sổ Cài đặt
- **Tray icon** (StatusNotifierItem qua libappindicator3/GTK): đổi kiểu gõ,
  chế độ hiển thị, bật/tắt ngay từ menu chuột phải
- **An toàn**: trường mật khẩu/PIN tắt hẳn engine, không log phím
- **Click chuột khi đang gõ dở**: tự phát hiện qua evdev, không để chữ commit
  nhầm vị trí con trỏ mới (cần user ở nhóm `input` — installer sẽ hỏi)

## Cài đặt

```bash
git clone https://github.com/nhanth87/vi-im.git
cd vi-im
./deploy/install.sh
```

Hoặc build tay + chạy AppImage (không cần cài vào hệ thống):

```bash
cargo build --release
./scripts/build-appimage.sh
./vi-im-x86_64.AppImage           # chạy daemon
./vi-im-x86_64.AppImage settings  # mở cửa sổ Cài đặt
```

## Điều khiển

```bash
vi-ime --switch    # đổi kiểu gõ: Telex ↔ VNI ↔ Tự do
vi-ime --toggle    # bật/tắt
vi-ime --mode      # đổi Preedit ↔ NonPreedit
vi-ime --status    # xem trạng thái hiện tại
vi-ime --doctor    # chẩn đoán cấu hình từng lớp
```

Hoặc dùng menu chuột phải trên tray icon — mọi thay đổi ghi thẳng vào
`setting.conf`, daemon tự reload (inotify), không cần restart.

## Yêu cầu hệ thống

- Compositor wlroots hỗ trợ `zwp_input_method_v2` + `zwp_virtual_keyboard_v1`
- Rust 1.80+, `libxkbcommon`, `libwayland-dev`, GTK3 + libappindicator3
  (cho tray icon)
- Cửa sổ Cài đặt cần Quickshell (module `Quickshell.Io`)

## Kiến trúc

Workspace 2 crate: `vi-daemon` (binary chính — engine, Wayland, tray, config,
telemetry) và `vi-settings` (launcher cửa sổ QML). Engine tiếng Việt là một
đường xử lý toán học duy nhất trên Unicode NFD/NFC cho cả ba kiểu gõ — không
bảng tra nguyên âm cứng.

## Giấy phép

Dual-license: **GPL v3.0** (mã nguồn mở) hoặc **giấy phép thương mại**
(liên hệ tác giả). Xem [LICENSE](./LICENSE).

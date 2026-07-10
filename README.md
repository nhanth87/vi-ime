<!--
SPDX-License-Identifier: GPL-3.0-only
Copyright (c) 2024-2026 vi-im contributors
-->

# vi-im — Bộ gõ tiếng Việt siêu nhẹ cho Wayland

Không phải fork của Unikey/fcitx. Viết mới hoàn toàn bằng Rust, nói chuyện
thẳng với `zwp_input_method_v2` của Wayland — không IBus, không Fcitx, không
daemon trung gian nào cả.

## Nhẹ đến mức nào?

| | vi-im | fcitx5 | IBus |
|---|---|---|---|
| Kích thước | **~1.9MB** (1 binary) | ~30MB+ (core + engine + plugin) | ~150MB (core + engine + gtk/qt module) |
| CPU lúc rảnh | **0%** — chặn trên 1 event, không poll | daemon nền liên tục | daemon nền liên tục |
| Phụ thuộc | libwayland, libxkbcommon | Qt/GTK, D-Bus, nhiều module | GTK, D-Bus |

Toàn bộ vòng lặp chính chỉ có **một** lệnh chặn (`rx.recv()`) — không timer,
không polling. Không gõ = không tốn CPU, không tốn pin.

## Engine: bỏ hẳn bảng tra từ kiểu Unikey

Các bộ gõ cũ (Unikey, fcitx5-unikey...) dùng **bảng tra nguyên âm cứng**
(vowel-cluster table) để quyết định dấu đặt ở đâu — đây chính là nguồn gốc
của hầu hết lỗi kinh điển mà ai gõ tiếng Việt cũng từng gặp: **chữ nhảy lung
tung khi gõ nhanh, dấu đặt sai nguyên âm, phụ âm bị dính/định sai vị trí**.

vi-im bỏ hoàn toàn bảng tra đó, thay bằng một **đường xử lý toán học duy
nhất trên Unicode NFD/NFC** (decompose → đặt dấu theo thuật toán ngữ âm →
compose lại) dùng chung cho cả Telex, VNI lẫn chế độ **Tự do** (trộn cả hai
kiểu trong cùng một từ). Mỗi phím gõ được re-parse lại toàn bộ âm tiết —
không có state cũ để mà "nhảy chữ".

Engine xử lý trong tầm **vài chục micro-giây mỗi phím** (đo thực tế: 17µs
từ lúc nhận phím tới lúc chữ tiếng Việt hiện ra) — nhanh hơn cả độ trễ mắt
người nhận biết được.

## Tự nhận diện ngữ cảnh

vi-im tự phát hiện app đang focus và đổi cấu hình phù hợp, không cần chỉnh tay:

- **Terminal** (foot, kitty, alacritty, konsole...) → gõ thẳng, không gạch chân
- **Trình duyệt / trang web** (Facebook, Google Docs...) → chế độ phù hợp với
  từng site
- **Game** → tự tắt engine, passthrough phím thô, không gõ nhầm tiếng Việt
  vào game
- **Trường mật khẩu/PIN** → tắt hẳn engine, không log phím

## Cài đặt

```bash
git clone https://github.com/nhanth87/vi-ime.git
cd vi-ime
./deploy/install.sh
```

Hoặc chạy AppImage không cần cài vào hệ thống:

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

Hoặc dùng menu chuột phải trên tray icon — đổi gì cũng ghi thẳng vào
`setting.conf`, daemon tự reload, không cần restart.

## Yêu cầu hệ thống

- Compositor **wlroots**: niri, Hyprland, Sway, COSMIC (hỗ trợ
  `zwp_input_method_v2` + `zwp_virtual_keyboard_v1`). GNOME/KDE chưa hỗ trợ
  đủ protocol nên nằm ngoài phạm vi dự án.
- Rust 1.80+, `libxkbcommon`, `libwayland-dev`, GTK3 + libappindicator3
  (cho tray icon)
- Cửa sổ Cài đặt cần Quickshell (module `Quickshell.Io`)

## Giấy phép

**GNU GPL v3.0.** Xem [LICENSE](./LICENSE).

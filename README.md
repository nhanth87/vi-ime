<!--
SPDX-License-Identifier: GPL-3.0-only
Copyright (c) 2024-2026 vi-im contributors
-->

# `VI-IME — Bộ gõ tiếng Việt siêu nhẹ cho Wayland`


https://github.com/user-attachments/assets/68af79c3-ae8c-4b50-9be9-1eaf87dfc53a




### ⬇️ [Tải bản mới nhất (AppImage)](https://github.com/nhanth87/vi-ime/releases)

> Tải file `vi-im-x86_64.AppImage` ở trang [Releases](https://github.com/nhanth87/vi-ime/releases), `chmod +x` rồi chạy — một file duy nhất, không cần cài đặt.

VI-IME là bộ gõ mới hoàn toàn trên nền linux, nó không dùng bảng VOWEL như fcit/ibus/unikey mà dùng **thuật toán đại số *NFD/C*** (*Algebra Normalization Form Decomposition/Composition* - file [**glyph.rs**](doc/nfc.md) - chỉ còn 20 locs so với hàng nghìn locs của VOWEL),  đây chính là bộ gõ tối giản của bộ gõ  chuẩn trên `win$ và appleè`. Được thiết kế lại  siêu gọn nhẹ, khả năng nhận diện các app văn phòng vượt trội, bộ từ điển mini tự sửa lỗi,  mục đính là perfect hoạt động với **tiếng việt quốc ngữ** một cách thuận tiện và tự nhiên cho người Việt mà không cần phải cấu hình lằng nhằng. Đặc biệt là cho dân VP đang di chuyển từ win$

- NFD/C là tiêu chuẩn của Unicode, còn **thuật toán Algebra NFD/C thì rất mới** (vi-ime là bộ gõ đầu tiên implement nó cho linux) so với bảng tra vowel đã thực chiến 21 năm có thể sẽ có lỗi. nếu gặp lỗi hoặc có thể là lỗi, hãy giúp chạy  `vi-ime --doctor`  và lấy log tạo issue nhé.
- notes: Hiện chỉ hỗ trợ ***wayland compositor***: niri, hyprland, sway, partial KDE plasma - steamdeck, không hỗ trợ Gnome

## Nhẹ đến mức nào?


|              | vi-im                                  | fcitx5                          | IBus                                   |
| ------------ | -------------------------------------- | ------------------------------- | -------------------------------------- |
| Kích thước   | **~1.9MB** (1 binary)                  | ~30MB+ (core + engine + plugin) | ~150MB (core + engine + gtk/qt module) |
| CPU lúc rảnh | **0%** — chặn trên 1 event, không poll | daemon nền liên tục             | daemon nền liên tục                    |
| Phụ thuộc    | libwayland, libxkbcommon               | Qt/GTK, D-Bus, nhiều module     | GTK, D-Bus                             |


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

- **Terminal** (foot, kitty, alacritty, konsole, wezterm, ghostty...) → gõ thẳng,
không gạch chân (NonPreedit)
- **Trình duyệt / trang web** (Facebook, Google Docs...) → chế độ phù hợp với
từng site. Address bar tự động passthrough tiếng Anh.
- **Game** → tự tắt engine, passthrough phím thô, không gõ nhầm tiếng Việt
vào game
- **Trường mật khẩu/PIN** → tắt hẳn engine, không log phím

## App hỗ trợ đặc biệt


| App                                                                                                                                                                                                                                                                    | Cơ chế                        | Ghi chú                                                                                                                                                      |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **Terminal** (kitty, foot, alacritty, wezterm, ghostty, konsole, gnome-terminal, ptyxis, xfce4-terminal, tilix, blackbox, guake, yakuake, tilda, xterm, urxvt, st, terminator, terminology, sakura, termite, tabby, warp, hyper, cool-retro-term, rio, contour, wayst) | NonPreedit (gõ thẳng)         | Live echo mode: phím thô forward trực tiếp, chữ hiện ngay không có gạch chân preedit. Không `delete_surrounding_text`.                                       |
| **Chrome / Chromium / Brave / Edge / Opera / Vivaldi / Zen**                                                                                                                                                                                                           | NonPreedit trên niri          | Tránh double-input trên niri (ChromiumNiriPlugin). Address bar tự động passthrough.                                                                          |
| **Firefox**                                                                                                                                                                                                                                                            | Preedit                       | Hỗ trợ `zwp_text_input_v3` đầy đủ.                                                                                                                           |
| **VS Code / VS Codium / JetBrains**                                                                                                                                                                                                                                    | Preedit / NonPreedit          | Tùy chọn trong setting.                                                                                                                                      |
| **Discord / Slack / Telegram / Signal / Element**                                                                                                                                                                                                                      | NonPreedit trên niri          | Electron app, tránh double-input.                                                                                                                            |
| **LibreOffice / OpenOffice**                                                                                                                                                                                                                                           | 🔧 evdev fallback (LIVE echo) | Không đi qua `zwp_input_method_v2` được (VCL/gtk3 bug: chỉ Activate 1 lần, không re-arm). Tự động grab keyboard + gõ qua virtual keyboard. Cần nhóm `input`. |
| **OnlyOffice Desktop Editors**                                                                                                                                                                                                                                         | 🔧 evdev fallback (LIVE echo) | Chạy XWayland (`QXcbConnection`) → không đến được text-input-v3. Tự động grab keyboard. Cần nhóm `input`.                                                    |
| **WPS Office / SoftMaker**                                                                                                                                                                                                                                             | 🔧 evdev fallback             | Tương tự LibreOffice nếu không gửi text-input.                                                                                                               |
| **App X11/XWayland khác**                                                                                                                                                                                                                                              | 🔧 evdev fallback (`--evdev`) | Bật toàn cục `vi-ime --evdev`.                                                                                                                               |
| **App Electron thiếu flag**                                                                                                                                                                                                                                            | Tự động detect                | `/proc/PID` advisor: thấy Electron thiếu `--enable-wayland-ime` → log cảnh báo.                                                                              |


## 🔧 LibreOffice / OnlyOffice / XWayland apps

Một số app **không đi qua `zwp_input_method_v2`** được:

- **LibreOffice** (VCL/gtk3): `text_input.enable()` chỉ gọi MỘT LẦN lúc focus
đầu, không bao giờ gọi lại → chỉ Activate 1 lần rồi Deactivate vĩnh viễn.
- **OnlyOffice Desktop Editors**: chạy XWayland → protocol Wayland không đến
được X11 client.
- **App X11/XWayland khác**: tương tự.

### Giải pháp: `evdev live-echo fallback`

vi-im tự động phát hiện các app này và chuyển sang **evdev fallback**:

- Grab trực tiếp bàn phím vật lý qua `/dev/input/event*`
- Gõ chữ qua **virtual keyboard bền vững** (MỘT `zwp_virtual_keyboard_v1` trên
connection riêng) — không spawn `wtype` mỗi phím, không race keymap
- Mỗi phím echo **trực tiếp** lên màn hình (live echo), không chờ word boundary
- Phím modifier (Super/Ctrl/Alt/Shift) forward 1:1 qua uinput + `SYN_REPORT`
- **Handshake:** nếu app bất ngờ Activate qua Wayland protocol → nhả grab,
protocol path xử lý

### Yêu cầu

```bash
# Thêm user vào nhóm input (cần cho evdev grab)
sudo usermod -aG input $USER
# Đăng xuất / đăng nhập lại, hoặc:
sg input -c vi-ime
```

AppImage tự xin quyền một lần qua `pkexec`/`sudo` ở lần chạy đầu.

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

### ⚠️ AppImage không chạy được?

AppImage có thể không hoạt động trên một số distro do khác biệt về
`libwayland`, `libfuse`, `libxkbcommon`, hoặc policy bảo mật (SELinux,
AppArmor). **Đừng bỏ cuộc — hãy mở Gemini, Claude, hoặc ChatGPT/Opus,**
paste nguyên dòng lỗi terminal vào và hỏi:

> "Tôi đang dùng [tên distro], appImage này báo lỗi: [paste lỗi].
> Làm sao để chạy được? Có cần cài thêm gói gì không?"

AI sẽ chỉ bạn cần cài gói gì (`libfuse2`, `libwayland-client`,
`libxkbcommon`, …) hoặc workaround nào (`--appimage-extract-and-run`,
`unsquashfs`, …) chỉ trong 1-2 phút. Mỗi distro mỗi khác — AI là
cách nhanh nhất để xử lý.

```bash
# Ví dụ lệnh thường gặp trên Ubuntu/Debian:
sudo apt install libfuse2 libwayland-client0 libxkbcommon0

# Hoặc chạy AppImage bypass FUSE:
./vi-im-*.AppImage --appimage-extract-and-run
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

- Compositor **wlroots**: niri, Hyprland, Sway, COSMIC, KDE plasma (hỗ trợ `zwp_input_method_v2` + `zwp_virtual_keyboard_v1`). GNOME chưa hỗ trợ đủ protocol nên nằm ngoài phạm vi dự án.
- Rust 1.80+, `libxkbcommon`, `libwayland-dev`, GTK3 + libappindicator3
(cho tray icon)
- Cửa sổ Cài đặt cần Quickshell (module `Quickshell.Io`)

## Giấy phép

**GNU GPL v3.0.** Xem [LICENSE](./LICENSE).

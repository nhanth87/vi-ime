# 🔬 DEBUG.md — Khoanh vùng lỗi theo tầng (vi-im edition)

> Nguyên tắc: đi từ dưới lên. Mỗi tầng có log riêng; so khớp **timestamp** giữa
> các log để biết phím "chết" ở đâu. Phần lớn trường hợp chỉ cần **bước 0**.

## Bước 0 — Luôn chạy trước: `vi-daemon --doctor`

```bash
vi-daemon --doctor
```

In ra 4 lớp chẩn đoán:
- **L0 env**: session Wayland, compositor, XWayland có mặt không
- **L1 globals**: compositor có `zwp_input_method_manager_v2` / `zwp_virtual_keyboard_manager_v1`
  / `zwp_text_input_manager_v3` không — đây là **sự thật duy nhất**, không phải env var
- **L2 learned**: từng app đã Activate chưa, có surrounding-text không, ack bao nhiêu µs
- **L3 blame**: phím kẹt ở chặng nào — `compositor transport` / `ack chain` /
  `vi-im engine` / `app · text-input-v3` — kèm ema/max/số lần stall

Blame này chạy **tự động khi đang gõ** (log tag `[BLAME]`) nhờ vi-telemetry đo 4 chặng
mỗi keystroke: Delivery (compositor→IME), QueueWait (kẹt buffer chờ ack),
Engine (vi-im xử lý), AckWait (delete→`done`, tức chặng compositor↔app qua text-input-v3).

## Tầng 0 — kernel/libinput (hiếm khi cần)

```bash
sudo libinput debug-events    # phím có lên khỏi kernel không?
```
Phím không xuất hiện ở đây → lỗi phần cứng/driver, không liên quan IME. Dừng.

## Tầng 1 — Dây protocol (quan trọng nhất khi nghi compositor relay sai)

vi-im là **một client Wayland duy nhất** nói cả input-method-v2 lẫn virtual-keyboard,
nên chỉ cần trace 2 process:

```bash
# Phía vi-im (input-method-v2 + virtual keyboard)
systemctl --user stop vi-ime
WAYLAND_DEBUG=1 vi-daemon 2>&1 | grep -E 'zwp_input_method|zwp_virtual_keyboard' | tee /tmp/vi-im.log

# Phía app (text-input-v3)
WAYLAND_DEBUG=1 <app> 2>&1 | grep -E 'zwp_text_input|wl_keyboard' | tee /tmp/app.log
```

So khớp:

| Thấy trong app.log | Thấy trong vi-im.log | Kết luận |
|---|---|---|
| `enable` + `commit` | **không có** `activate` | ❌ compositor không relay app→IME |
| — | `commit_string` gửi đi | app không nhận `commit_string` → ❌ compositor relay chiều IME→app |
| `delete_surrounding_text` đến | `done` không quay lại | ❌ compositor/app không ack (vi-im tự force sau 150ms + ghi DoneTimeout) |
| hai log khớp nhau mà chữ vẫn không hiện | | lỗi phía app/toolkit → tầng 3 |

## Tầng 2 — vi-im internal

```bash
RUST_LOG=debug vi-daemon          # tag: [KEY-IN] [COMMIT] [SCENARIO] [RECONFIG] [CONTENT-TYPE] [REORDER] [BLAME]
VI_GODMOD=1 vi-daemon             # ghi từng phím vào ~/.local/share/vi-ime/godmod/
```
Đường đi 1 phím: `[KEY-IN]` → engine → `[COMMIT] phase-1 (delete N bytes)` →
`done` → `phase-2 append`. Đứt ở đâu, tầng đó có lỗi.
Lưu ý: field password/PIN **không bao giờ** log ký tự (by design).

## Tầng 3 — Toolkit phía app

Không cần env var nào cho GTK3/4, Qt5.15+/Qt6 trên Wayland — chúng nói
text-input-v3 native, compositor tự nối tới vi-im. Nếu app hiện đại vẫn không
Activate (doctor L2 báo `activate ?` + notify "app chưa nhận bộ gõ"):
- **Electron**: daemon đã tự soi `/proc` và notify đúng cờ cần thêm
  (`--enable-wayland-ime --wayland-text-input-version=3`).
- App X11/XWayland (doctor L0 báo có DISPLAY): xem "App cũ" trong README —
  đường evdev-fallback (roadmap), KHÔNG dùng GTK_IM_MODULE/QT_IM_MODULE
  (đó là cơ chế của fcitx/ibus, vô nghĩa với vi-im).

## Tầng 4 — Compositor log

```bash
RUST_LOG=niri=debug niri 2> /tmp/niri.log      # niri (Smithay)
sway -d 2> /tmp/sway.log                        # Sway (wlroots)
# Hyprland: ~/.local/share/hyprland/hyprlandLog.log
```
Tìm các dòng quanh `input_method` / `text_input` tại đúng timestamp phím bị nuốt.

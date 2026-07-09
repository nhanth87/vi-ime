# vi-ime — Tổng kết tình hình (updated 2026-07-03)

## Cấu trúc workspace

```
vi-im/
├── Cargo.toml                          # workspace root
├── setting.conf                        # config mẫu
├── crates/
│   ├── vi-engine/          ✅ M1       # Core engine — Telex/VNI, 93 tests
│   ├── vi-wayland-im/      ✅ M2-M3   # Wayland IM protocol (đã fix offset + auto-commit)
│   ├── vi-compositor-ipc/  ✅ M5      # Niri IPC thật, Hyprland IPC thật
│   ├── vi-config/          ✅         # Đọc/ghi setting.conf, 5 tests
│   ├── vi-tray/            ✅         # System tray icon + menu
│   ├── vi-daemon/          ✅         # Binary chính (tray + Wayland IM thread)
│   └── vi-settings/        ✅         # egui settings window
```

## Trạng thái từng crate

### ✅ vi-engine — Hoàn thành (M1)
- `telex.rs` — Quy tắc Telex: tone_for_key, handle_vowel_or_special (aa→â, aw→ă, dd→đ, w→ư, ...)
- `vni.rs` — Quy tắc VNI: apply_diacritic (6→^, 7→horn, 8→breve, 9→đ)
- `tone_placement.rs` — Thuật toán đặt dấu thanh chuẩn tiếng Việt hiện đại
- `syllable.rs` — Struct lưu âm tiết
- `lib.rs` — Engine state machine + `is_word_boundary` cải tiến + `is_vietnamese_char`
- **93 tests pass**

### ✅ vi-wayland-im — Đã sửa (M2-M3)
- ✅ Kết nối Wayland, bind `zwp_input_method_manager_v2`
- ✅ Nhận key qua `zwp_input_method_keyboard_grab_v2`
- ✅ Xử lý xkb keymap mmap
- ✅ Engine integration: push_key → preedit/commit
- ✅ **FIXED**: Offset trong `set_preedit_string` dùng char index đúng (không còn `s.len()`)
- ✅ **FIXED**: Auto-commit preedit khi `Deactivate` (fix lỗi "chuột qua chỗ khác làm loạn văn bản")
- ✅ **FIXED**: `is_printable_keysym` không còn unreachable patterns
- ⏳ TODO: Popup candidate window (xdg_popup)
- ⏳ TODO: X11/XWayland XIM server (Phase 2)

### ✅ vi-config
- Đọc/ghi setting.conf (TOML)
- Per-app config
- **5 tests pass**

### ✅ vi-tray
- System tray icon với menu

### ✅ vi-compositor-ipc — Đã implement (M5)
- ✅ **NiriWatcher**: Poll `niri msg --json windows` để lấy active window
- ✅ **HyprlandWatcher**: Poll `hyprctl activewindow -j`
- ✅ **spawn_niri_event_stream()**: Real-time event stream listener (Phase 2 ready)
- ✅ **auto_detect_watcher()**: Tự detect compositor đang chạy
- ✅ CosmicWatcher: stub (chờ cosmic-comp API ổn định)

### ✅ vi-daemon
- Chạy tray icon + spawn Wayland IME thread

## 🔧 Các lỗi đã sửa (2026-07-03)

| # | Lỗi | Severity | Fix |
|---|-----|----------|-----|
| 1 | `set_preedit_string` dùng `s.len()` (byte) thay vì char index | CRITICAL | Đổi thành `s.chars().count()` |
| 2 | Deactivate không commit preedit → text corruption | CRITICAL | Auto-commit trước khi reset engine |
| 3 | `vi-compositor-ipc` toàn stub | HIGH | Implement NiriWatcher + HyprlandWatcher thật |
| 4 | `is_word_boundary` không xử lý control chars, CJK | MEDIUM | Viết lại với `is_vietnamese_char()` |
| 5 | `is_printable_keysym` unreachable patterns | LOW | Sửa match arms |

## Luồng xử lý gõ phím (hiện tại)

```
Phím được nhấn
   ↓
[1] Compositor nhận key event
   ↓
[2] zwp_input_method_keyboard_grab_v2 → key event
   ↓
[3] XkbState::keycode_to_char → char
   ↓
[4] Engine::push_key(char) → Action
   ↓
[5] set_preedit_string / commit_string → compositor → app
   ↓
[6] Khi Deactivate → auto-commit preedit (fix text corruption)
```

## Cần làm tiếp

1. **M4** — Popup candidate (xdg_popup):
   - Tạo `zwp_input_popup_surface_v2`
   - Dùng `xdg_popup` (KHÔNG dùng `xdg_toplevel`!)
   - Neo theo `cursor_rectangle` từ `text-input-v3`
   
2. **Phase 2** — Non-preedit mode (VMK-like):
   - Dùng Surrounding Text API
   - Gửi Backspace N lần → commit chuỗi mới
   - Loại bỏ hoàn toàn preedit cho tương thích cao nhất
   
3. **Chromium/Electron fix**:
   - Document flag `--enable-wayland-ime --wayland-text-input-version=3`

4. **Build fix**: Cần `libxdo-dev` để build vi-daemon (do tray-icon dependency)

## Biên dịch & Kiểm thử

```bash
cargo check                          # ✅ Pass
cargo test -p vi-engine              # ✅ 93 tests pass
cargo test -p vi-config              # ✅ 5 tests pass
cargo build                          # Cần libxdo-dev
```

## Ghi chú

- `vi-engine` KHÔNG phụ thuộc Wayland — test được bằng `cargo test` không cần compositor
- Chạy thử: `cargo run -p vi-daemon` → tray icon + Wayland IME
- Để test thật: cần chạy nested Niri session hoặc máy thật
- VMK tham khảo: non-preedit mode (Backspace + commit) cho tương thích cao nhất

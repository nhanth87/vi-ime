# 📖 Hướng dẫn đọc code vi-im

> **Dành cho:** Người mới, tay ngang, không cần kinh nghiệm lập trình.
> **Mục tiêu:** Hiểu cách vi-im hoạt động từ A-Z trong 30 phút.

---

## 🧠 Tổng quan

Bạn gõ "vieetj" → vi-im bắt phím qua Wayland → Engine xử lý thành "việt" → gửi vào app.

Khác Unikey: vi-im chạy trên Linux Wayland, chạy ngầm (daemon), không có cửa sổ.

---

## 🗺️ Đọc theo thứ tự

### 1. `vi-engine/src/types.rs` (124 dòng)

Từ vựng của vi-im: InputMethod (Telex/Vni), ImeMode (Preedit/NonPreedit/Hybrid),
Action (UpdatePreedit/Commit/PassThrough), NonPreeditAction.

### 2. `vi-engine/src/engine.rs` (302 dòng)

Engine struct + push_key(). Mỗi phím gọi push_key() 1 lần:
```
v→i→e→e→t→j→space = "việt"
```

### 3. `vi-engine/src/telex.rs` (243 dòng)

Tất cả quy tắc Telex: aa→â, ee→ê, oo→ô, aw→ă, ow→ơ, uw→ư, dd→đ,
s→sắc, f→huyền, r→hỏi, x→ngã, j→nặng. Tìm `match (prev, ch)`.

### 4. `vi-engine/src/vni.rs` (241 dòng)

VNI dùng số: 1=sắc, 2=huyền, 3=hỏi, 4=ngã, 5=nặng, 6=mũ, 7=râu, 8=trăng, 9=đ.

### 5. `vi-engine/src/tone_placement.rs` (403 dòng)

Quyết định đặt dấu vào chữ nào: "hoa"+sắc→"hoá", "thuy"+sắc→"thúy".

### 6. `vi-engine/src/fast_engine.rs` (337 dòng)

NonPreeditEngine: gõ ẩn cho terminal. Buffer → space → delete N ký tự → chèn chữ mới.
```rust
CommitWithBackspace { backspace_count: 6, text: "việt" }
// = xóa 6 ký tự "vieetj", chèn "việt"
```

### 7. `vi-plugin/src/lib.rs` (200 dòng)

AppPlugin trait: pre_process_key, post_process_action, focus_change.
Plugin có sẵn: GodmodPlugin, EnglishDetectPlugin.

---

## 🔌 Wayland layer

| File | Dòng | Vai trò |
|------|------|---------|
| `state.rs` | 292 | ImeAppState: engine + active state + serial |
| `dispatch.rs` | 238 | Xử lý activate/deactivate/key/done events |
| `xkb.rs` | 227 | keycode → keysym qua XKB |

| Event | Ý nghĩa |
|-------|---------|
| activate | User focus vào app → bắt đầu nhận phím |
| deactivate | User rời app → auto-commit + reset |
| key | User bấm phím → push_key() |
| done | App nhận xong → sẵn sàng nhận tiếp |

---

## 🔄 Data flow (10 bước)

```
Phím 'v' → Compositor → dispatch.rs → xkb.rs → godmod log
→ plugin.pre_process → engine.push_key → plugin.post_process
→ state.apply_action → set_preedit_string("v") → App hiện "v"
```

---

## 🧪 Test + Debug

```bash
cargo test -p vi-engine -- test_telex_aa
cargo test --workspace              # 191+ tests
VI_GODMOD=1 RUST_LOG=debug vi-daemon  # Debug mode
cat ~/.local/share/vi-ime/godmod/*.jsonl | jq .
```

---

## 📐 App Support Detection (thiết kế xong)

FocusMonitor Plugin: focus → timer 500ms → activate = supported, timeout = unsupported.
Thông báo: tray đổi màu (🟢⚫🟡) + DBus notify + log.

---

## 🎯 Lộ trình học (~4h)

| # | Đọc | Thời gian |
|---|-----|-----------|
| 1 | types.rs + engine.rs | 30' |
| 2 | telex.rs | 45' |
| 3 | vni.rs | 30' |
| 4 | fast_engine.rs | 30' |
| 5 | state.rs + dispatch.rs | 45' |
| 6 | main.rs | 30' |
| 7 | plugin.rs | 30' |

## ❓ FAQ

**Q: Crate là gì?** Lib crate → .rlib, Bin crate → executable. 8 lib + 1 bin → 1 binary.
**Q: main chạy gì?** Config → tray → spawn IME thread → event loop.
**Q: Plugin ở đâu?** vi-plugin/src/lib.rs — AppPlugin trait.
**Q: Sao ≤300 dòng?** AI agent context window. File > 300 bị từ chối merge.

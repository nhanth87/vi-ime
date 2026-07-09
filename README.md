<!--
SPDX-License-Identifier: GPL-3.0-or-later OR Commercial
Copyright (c) 2024-2026 vi-im contributors
-->

# 🇻🇳 vi-im — Bộ Gõ Tiếng Việt Thế Hệ Mới Cho Wayland

> **Không còn là Unikey/Vietkey.** vi-im dùng **NFD Engine** (Neutral Function
> Dispatching) — kiến trúc toán học thuần túy dựa trên Unicode Combining
> Diacritical Marks, không lookup table cứng nhắc, không phụ thuộc engine C++
> 20 năm tuổi.
>
> Built for **Niri · Hyprland · Sway · COSMIC · River**. Zero IBus. Zero Fcitx.

---

## ⚡ Tại Sao vi-im Nhanh Hơn Mọi Bộ Gõ Khác?


| Engine cũ (Unikey/Vietkey)          | vi-im (NFD Engine)                          |
| ----------------------------------- | ------------------------------------------- |
| Lookup table 55+ entries, O(n) scan | Unicode NFD math, O(1) composition          |
| 20 năm code C++ monolithic          | Rust 2024, zero-cost abstraction            |
| Phụ thuộc IBus/Fcitx daemon         | Native Wayland protocol, **zero middleman** |
| Không phân biệt được app Game/Code  | Auto-detect Game Mode, Terminal, Browser    |
| Phân biệt Anh-Việt thô sơ           | **Telemetry ML** tự học thói quen gõ        |


---

## 🧠 Core Engine: NFD Mathematical Dispatch

Bỏ hoàn toàn bảng tra cứu nguyên âm (`VOWEL_CLUSTERS`, 55 entries). Thay bằng
**MỘT path NFD toán học Unicode tổ hợp** cho MỌI kiểu gõ (Telex, VNI, Smart):

```
raw_keys → normalize (Telex/VNI + undo) → decompose (predicate ngữ âm)
             → tone placement (thuật toán) → NFC compose (glyph) → commit
                     │                              │
            onset/nucleus/coda              1 vowel → trên nó
            = danh sách category            coda → nguyên âm cuối
            (KHÔNG map char→char)           diphthong → nguyên âm chất lượng
            backtrack gi/qu                 hoặc oa/oe/uy theo ToneStyle
```

Vị trí dấu là **thuật toán thuần**, không phải data offset; mỗi dấu do Unicode
NFC compose sinh ra (không bảng char→char, ngoại lệ duy nhất: đ). Case được giữ
(Việt/VIỆT). Từ không tạo âm tiết Việt hợp lệ → commit RAW keys (windows→windows).

**Mỗi keystroke re-parse toàn bộ từ ≤12 chars trong nanosecond.** Parse, don't
mutate — raw keys là single source of truth.

---

## 🎯 3 Phương Thức + Smart Mode


| Mode         | Mô tả                                                           |
| ------------ | --------------------------------------------------------------- |
| **Telex**    | Gõ truyền thống: `toanf` → toàn, `vietj` → việt                 |
| **VNI**      | Gõ số: `to6an2` → toàn, `viet5` → việt                          |
| **Smart** 🔥 | **Tự detect VNI/Telex trong cùng một từ.** Gõ `to6ans` → engine |
|              | nhận ra `6` là VNI circumflex, `s` là Telex sắc → "toán".       |
|              | Conflict? Telex luôn thắng (người Việt quen hơn).               |


---

## 🛡️ Auto-Detect: Biết Bạn Đang Ở Đâu

vi-im **không gõ tiếng Việt vào game** hay terminal code:


| Context                   | Hành vi                                                      |
| ------------------------- | ------------------------------------------------------------ |
| 🎮 **Game**               | Tự tắt IME, passthrough toàn bộ phím (Ctrl+Shift+G override) |
| 💻 **Terminal / Code**    | Ép NonPreedit, không popup gây lag                           |
| 🔒 **Password field**     | Tắt engine, không log, không telemetry                       |
| 🌐 **Chrome / Firefox**   | Per-site config: gõ Việt trên Facebook, Anh trên GitHub      |
| 🐧 **X11 app (XWayland)** | Virtual keyboard bridge, hoạt động trong suốt                |


---

## 📊 Telemetry: Tự Phân Biệt Tiếng Anh / Tiếng Việt

Không dùng heuristic đếm phím thô sơ. vi-im dùng **phonotactic validation**:

- Từ không parse được thành âm tiết Việt hợp lệ → **commit nguyên văn** (English pass-through)
- Học theo app: gõ Anh trong VSCode? Engine tự giảm trigger Việt
- Protocol signal từ Wayland (`SurroundingText`, `DoneAck`, `ContentType`)
→ adaptation engine tự điều chỉnh

**Không bao giờ gửi telemetry ra ngoài.** Mọi thứ chạy local trong
`~/.local/share/vi-ime/`.

---

## 🏗️ Kiến Trúc 9 Crates, 0 Circular Deps

```
┌──────────────────────────────────────────────────────────┐
│  vi-daemon (root)                                        │
│  ┌─────────┐  ┌──────────┐  ┌───────────┐  ┌──────────┐  │
│  │vi-config│  │ vi-tray  │  │vi-compos- │  │vi-plugin │  │
│  │4-layer  │  │QML qs    │  │itor-ipc   │  │hooks     │  │
│  │config   │  │+ menu    │  │niri/hypr  │  │pre/post  │  │
│  └─────────┘  └──────────┘  └───────────┘  └──────────┘  │
│       ▲            ▲              ▲               ▲      │
│       └────────────┼──────────────┼───────────────┘      │
│                    │              │                      │
│  ┌─────────────────▼──────────────▼─────────────────────┐│
│  │ vi-wayland-im (Wayland thread)                       ││
│  │ zwp_input_method_v2 + keyboard_grab + virtual_kb     ││
│  └──────────────────────┬───────────────────────────────┘│
│                         │                                │
│  ┌──────────────────────▼───────────────────────────────┐│
│  │ vi-engine (leaf crate — zero deps)                   ││
│  │ NFD Engine + Parser + Smart Mode + Unicode algebra   ││
│  └──────────────────────────────────────────────────────┘│
└──────────────────────────────────────────────────────────┘
```

**Zero CPU idle:** daemon main loop = 1 `rx.recv()` blocking. Không poll, không
timer, không wakeup khi không gõ.

---

## 🚀 Quick Start

```bash
# 3 lệnh — xong
./deploy/compile.sh        # Build từ source
./deploy/install.sh         # Cài vào ~/.local/bin + systemd
systemctl --user start vi-ime  # Bắt đầu gõ tiếng Việt

# Gõ bình thường. Không cần bật app, không cần switch language.
# Tray icon sẽ xuất hiện — click phải để đổi method.
```

---

## 🎮 Điều Khiển

```bash
# Đổi phương thức gõ ngay lập tức (không cần restart)
vi-daemon --switch       # Telex ↔ VNI
vi-daemon --toggle        # Bật/tắt IME
vi-daemon --status        # Xem trạng thái hiện tại

# Debug mode — xem mọi phím
RUST_LOG=debug vi-daemon

# Godmod — ghi log từng keystroke
VI_GODMOD=1 vi-daemon
# Log: ~/.local/share/vi-ime/godmod/

# Restart sau khi sửa setting.conf
systemctl --user restart vi-ime
```

---

## 🧪 Test Suite

```bash
cargo test --workspace    # 198 tests, all pass
```

**Coverage:** Telex parser, VNI parser, Smart conflict resolution, NFD tone
placement, glide onset stripping, word boundary detection, NFC output
verification.

---

## 📦 Yêu Cầu

- **Compositor:** Niri, Hyprland, Sway, River, COSMIC (hỗ trợ `zwp_input_method_v2`), partial support KDE(KWIN) - tested steamdeck only
- **Không hỗ trợ:** GNOME (Mutter) compositor này không implement  
đầy đủ input-method protocol
- **Rust:** 1.80+
- **Thư viện hệ thống:** `libxkbcommon libwayland-dev` 
- **Cửa sổ Settings:** cần **Quickshell upstream** (có module `Quickshell.Ipc`) —
  Settings mở ra dạng **cửa sổ nổi** (FloatingWindow) tự float + center trên
  Niri/Hyprland/Sway (zero-config, không cần sửa config compositor). Một số fork
  (vd. noctalia-qs) thiếu `Quickshell.Ipc` nên `main.qml` không map được.

---

## 🗺️ Roadmap


| Phase | Nội dung                                     | Trạng thái     |
| ----- | -------------------------------------------- | -------------- |
| 1     | Unified binary `vi-im` + tray icon           | ✅ Done         |
| 2     | **Smart mode** (mixed VNI/Telex auto-detect) | ✅ Done         |
| 3     | Tray-only config menu                        | 🔨 In progress |
| 4     | **Burst commit** — ibus-style 300ms window   | 📋 Planned     |
| 5     | Test suite + AGENTS.md compliance            | ✅ Done         |
| 6     | **Game Mode** — auto-detect + Ctrl+Shift+G   | 📋 Planned     |


---

## 📁 Tài Liệu


| File                             | Nội dung                            |
| -------------------------------- | ----------------------------------- |
| `docs/VI_IM_DESIGN.md`           | Kiến trúc tổng thể, rules, file map |
| `docs/UNIFIED_SMART_IME_PLAN.md` | Kế hoạch 6 phases chi tiết          |
| `docs/smart-method.md`           | NFD Engine + Smart mode code specs  |
| `AGENTS.md`                      | Design contract cho AI agents       |


---



---
## 📜 License

vi-im is dual-licensed under **GPL v3.0** or a **commercial license**.

- **Commercial use:** Contact copyright holder for proprietary licensing
- **Open source users:** GPL v3.0 (see [LICENSE](./LICENSE))

```
SPDX-License-Identifier: GPL-3.0-or-later OR Commercial
Copyright (c) 2024-2026 vi-im contributors
```

---

## 🔧 Recent Fixes (2026-07-08)

- **Fix: `vi-settings` QML crash** — Quickshell parser rejects `;` after
  grouped property blocks (`font {...}; color`). Broke properties into
  separate lines on `Heading`, header `Text`, ComboBox `background`, and
  `RowLayout` blocks. Settings window launches correctly now.
- **Fix: Double characters when typing** — `set_preedit("")` before
  `commit_string()` caused doubling in terminals (foot, kitty, etc.) that
  auto-commit preedit on clear. Removed redundant `set_preedit("")` call
  in `apply_action` (`CommitWithBackspace`) and `finalize_word`. Per
  protocol spec, `commit_string` already replaces the current preedit.

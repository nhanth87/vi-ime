# APP_RULES.md — Master Control Table for vi-ime Engine

> **Mục đích:** Gom TẤT CẢ các quy tắc điều khiển engine theo app vào MỘT file.
> **Cập nhật:** Mỗi khi thêm app mới, sửa ClientProfile, cheat rule, hoặc
> path selection, PHẢI cập nhật file này.

---

## 🔀 Layer 0 — Path Selection (engine nào chạy?)

Quyết định engine xử lý qua đường **Wayland protocol** hay **evdev fallback**.

| App | Path | Lý do | File |
|-----|------|-------|------|
| **LibreOffice** (libreoffice-*, soffice) | `evdev` | VCL gtk3 gọi `enable()` 1 lần, không re-arm | `legacy_grab.rs:39-42` |
| **OnlyOffice** (onlyoffice-*) | `evdev` | Qt/X11 qua XWayland, không có `zwp_text_input_v3` | `legacy_grab.rs:42` |
| **Chrome/Chromium/Brave/Edge/Opera/Vivaldi** (XWayland) | `evdev` | Blink keymap lag làm sai `BackSpace` trên đường Wayland | `legacy_grab.rs:60-71` |
| **Firefox** (Wayland native) | `Wayland` | Hỗ trợ `zwp_text_input_v3` đầy đủ | *(default)* |
| **Terminal** (kitty, foot, alacritty, …) | `Wayland` | Hỗ trợ text-input-v3, preedit ok | *(default)* |
| **App khác** (gedit, VS Code, Slack, …) | `Wayland` | Mặc định | *(default)* |

### Cơ chế chuyển path

```
Focus app → Compositor báo app_id → main.rs check:
  1. is_legacy_app(app_id)?        → evdev ngay lập tức
  2. is_xwayland_fallback(app_id)? → probe 2s, nếu không Activate → evdev
  3. Re-arm detection (Phase 5)     → auto-detect one-shot app → evdev
```

---

## ⌨️ Layer 1 — Typer Selection (cách inject ký tự)

Khi đã ở đường **evdev**, chọn cách gửi phím vào app:

| App | Typer | Lý do |
|-----|-------|-------|
| **OnlyOffice** | `xdotool` (injector) | CEF child surface drop Mod3/Mod5 → static keymap sai level |
| **Các app khác** (LibreOffice, Chrome, …) | `Virtual keyboard` (native) | Bàn phím ảo tĩnh 8-level qua `zwp_virtual_keyboard_v1` |

File: `legacy_grab.rs:86`, `evdev_inject.rs:Typer::detect()`

---

## ⏱️ Layer 2 — Timing Profile (ClientProfile)

Sau khi có path + typer, quyết định **delay** giữa các ký tự:

| App | BS delay | Glyph delay | Pre-glyph | Batch? | Batch delay |
|-----|----------|-------------|-----------|--------|-------------|
| **LibreOffice** | 20ms | 20ms | 30ms | ❌ | 0 |
| **OnlyOffice** | 20ms | 20ms | 30ms | ❌ | 0 |
| **Chromium XWayland** | 8ms | 10ms | 20ms | ✅ | 15ms |
| **Firefox Wayland** | 5ms | 5ms | 15ms | ✅ | 10ms |
| **Terminal** | 3ms | 3ms | 5ms | ✅ | 5ms |
| **Default (unknown)** | 8ms | 8ms | 15ms | ✅ | 12ms |

### Phân loại app cho timing

| Nhóm | App pattern | Đặc điểm |
|------|------------|----------|
| **Slow** | `libreoffice*`, `soffice`, `onlyoffice*` | is_slow=true, batch_safe=false |
| **XWayland browsers** | `google-chrome*`, `chromium*`, `brave*`, `microsoft-edge`, `opera`, `vivaldi*` | xwayland_fallback=true |
| **Native browsers** | `firefox*`, `org.mozilla.firefox` | batch_safe=true |
| **Terminals** | `kitty`, `foot`, `alacritty`, `wezterm`, `ghostty`, … (45+ apps) | Nhanh nhất |
| **Default** | Mọi app khác | Conservative |

File: `client_profile.rs:ClientProfile::detect()`

---

## 📝 Layer 3 — Word Override (Cheat Module)

Trước khi engine compose, check xem từ có bị override thành English không:

### Luật global (áp dụng mọi app)

| Pattern | Ví dụ sai → đúng |
|---------|-------------------|
| `war*` | warp→ưảp → **warp** |
| `browser*` | browser→bởe → **browser** |
| `sw*` (swap/swift/swipe/sweep/sweet/swing/sword) | swap→sưap → **swap** |
| `dd` words (add/odd/address/sudden/middle/riddle) | add→ađ → **add** |
| `aw` words (award/aware/awake/draw/law/raw/saw/dawn/flaw/…) | draw→đră → **draw** |
| `sort/save/sound/south/source/space/speed/spell/split/…` | sort→sởt → **sort** |
| **Browser names** | chrome→chrome, firefox→firẽox → giữ nguyên |

### Luật per-app

| App | Word | Lý do |
|-----|------|-------|
| `chromium` | workspace, password | Address bar hay gõ |
| `firefox` | workspace, password | Address bar |

File: `engine/cheat.rs:CHEATS` (built-in) + `RUNTIME_CHEATS` (hot-reload)

### Cách thêm cheat mới (runtime)

```rust
// Từ IPC/config: thêm cheat rule không cần recompile
cheat::add_runtime_rule("chromium", "github");
```

---

## 🧠 Layer 4 — IME Mode (Preedit / NonPreedit)

| App category | Default mode | Lý do |
|-------------|-------------|-------|
| **Terminal** | `NonPreedit` | Hầu hết terminal không hỗ trợ preedit underline |
| **Browser** | `NonPreedit` (Hybrid) | Live-echo qua backspace-diff |
| **Editor** | `Preedit` | Hỗ trợ preedit tốt |
| **Chat** | `NonPreedit` | Ít hỗ trợ preedit |
| **Other** | `Hybrid` (config quyết định) | Mặc định |

File: `compositor/mod.rs:AppCategory`, `plugin/mod.rs`

---

## 🔤 Layer 5 — Content Type

| App | ContentType signal | Xử lý |
|-----|-------------------|-------|
| **Terminal** | `ContentType::Terminal` | Force NonPreedit, swallow signal |
| **Browser address bar** | *(không có signal)* | Trade-off: có thể compose Vietnamese trên URL |
| **Browser page** | `ContentType::Text` | Bình thường |

File: `wayland/dispatch.rs`, `plugin/mod.rs:TerminalPlugin`

---

## 🔄 Toàn bộ flow khi user gõ phím

```
Keystroke
  │
  ├─ Layer 0: Path Selection ─────────────────────────────┐
  │   ├─ Wayland path (default)                            │
  │   │   ├─ check ActivePath atomic (Phase 7)             │
  │   │   └─ → process_key() tiếp                         │
  │   └─ evdev path (legacy/XWayland app)                  │
  │       ├─ grab keyboard + reader thread (R20)           │
  │       └─ → evdev_mode::run_scoped()                    │
  │                                                        │
  ├─ Layer 1: Typer Selection (evdev only) ────────────────┤
  │   ├─ OnlyOffice → xdotool Injector                     │
  │   └─ Mọi app khác → Virtual Keyboard native            │
  │                                                        │
  ├─ Layer 2: Timing Profile (ClientProfile) ──────────────┤
  │   ├─ detect(app_id) → profile                          │
  │   ├─ BackSpace pacing: profile.backspace_delay_ms      │
  │   ├─ Glyph pacing: profile.glyph_delay_ms               │
  │   └─ Batch BS: profile.batch_safe → burst              │
  │                                                        │
  ├─ Layer 3: Cheat Module (word override) ────────────────┤
  │   ├─ should_force_english(app_id, raw_keys)            │
  │   ├─ Match → return raw keys as English                │
  │   └─ No match → continue                               │
  │                                                        │
  ├─ Layer 4: IME Mode ────────────────────────────────────┤
  │   ├─ Terminal → NonPreedit                             │
  │   └─ Other → config / plugin recommendation            │
  │                                                        │
  └─ Layer 5: Content Type ────────────────────────────────┘
      ├─ Terminal signal → swallow, force NonPreedit
      └─ Text → engine compose normally

  Sau tất cả: English Restore (R9)
      ├─ Dictionary check (is_english_word)
      └─ Validity check (is_viet_syllable)
```

---

## 📁 File map — tìm rule ở đâu

| Layer | File | Hàm / Const |
|-------|------|-------------|
| 0 — Path | `legacy_grab.rs` | `LEGACY_APP_PREFIXES`, `XWAYLAND_FALLBACK_PREFIXES`, `is_legacy_app()`, `is_xwayland_fallback_app()` |
| 0 — Path | `wayland/dispatch.rs` | Activate handler + re-arm detection |
| 1 — Typer | `legacy_grab.rs` | `INJECTOR_TYPER_PREFIXES`, `needs_injector_typer()` |
| 1 — Typer | `evdev_inject.rs` | `Typer::detect()` |
| 2 — Timing | `client_profile.rs` | `ClientProfile::detect()` |
| 3 — Cheat | `engine/cheat.rs` | `CHEATS`, `should_force_english()`, `add_runtime_rule()` |
| 3 — Dict | `data/english_common.txt` | 1070 English words |
| 4 — Mode | `compositor/mod.rs` | `AppCategory`, `KNOWN_TERMINALS` |
| 4 — Mode | `plugin/mod.rs` | PluginManager, per-app recommendations |
| 5 — Content | `wayland/dispatch.rs` | ContentType handling |
| — Atomic | `wayland/runtime.rs` | `ActivePath` atomic |

---

## 🧪 Cách test khi thêm app mới

```bash
# 1. Unit test cheat rules
cargo test -p vi-daemon cheat

# 2. Unit test client profile
cargo test -p vi-daemon client_profile

# 3. Stress test engine
cargo test -p vi-daemon stress_fast_typing

# 4. Full integration (Wayland session thật)
bash scripts/vi-regression/run.sh LO=1 OO=1

# 5. Kiểm tra log file
RUST_LOG=debug ./deploy/vi-ime-7.3.1-x86_64.AppImage
# → ~/.local/share/vi-ime/vi-ime.log
```

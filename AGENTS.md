# AGENTS.md — vi-ime AI Agent Design Contract

> **Mục đích:** Mọi AI agent khi sửa code PHẢI tuân theo file này.
> **Vi phạm:** Merge bị từ chối. Phải revert.

---

## 🔒 DESIGN RULES — KHÔNG ĐƯỢC PHÁ VỠ

### R1. Data Flow Pipeline (Immutable)
```
KeyEvent → [vi-godmod log] → vi-wayland-im (keyboard grab)
  → vi-plugin.pre_process_key()   ← short-circuit hook
  → vi-engine.push_key()          ← Telex/VNI processing
  → NonPreeditAction              ← result
  → vi-plugin.post_process_action() ← modify hook
  → vi-wayland-im.apply_action()  → Wayland commit
```
**Cấm:** Thêm bước vào pipeline không qua plugin.

### R2. ImeMode Contract (Preedit-everywhere)
- Mọi mode dùng **preedit path**: `set_preedit_string` cho hiển thị,
  `commit_string` cho commit. KHÔNG `delete_surrounding_text` ở đâu cả.
- Preedit mode: luôn hiện preedit. Hybrid: hiện preedit khi đang gõ (`has_pending`).
  NonPreedit: buffer âm thầm, commit ở word boundary.
- Virtual keyboard (zwp_virtual_keyboard_v1) CHỈ dùng cho phím passthrough:
  shortcut (Ctrl/Alt/Super), navigation, Enter/Tab/Esc, boundary key replay.
  Mirror keymap + modifiers từ grab sang vk. release_all() khi deactivate/disable.

### R3. NonPreeditAction Variants (Extend-only)
Chỉ được THÊM variant, KHÔNG xóa/đổi tên/đổi kiểu variant hiện có.

### R4. File Size: ≤300 dòng (src), ≤600 dòng (tests)

### R5. Workspace = 2 crate: `vi-daemon` (binary, chứa TẤT CẢ: engine + config + wayland + plugin + godmod + compositor + telemetry) và `vi-settings` (binary QML). Engine là module `crate::engine` (self-contained, `#![allow(dead_code)]` để giữ ngữ nghĩa lib — API test-covered qua `tests/vi-engine/`, không phải dead). Không cycle.

### R6. Godmod: chỉ chạy khi RUST_LOG=debug hoặc --godmod. Ghi vào ~/.local/share/vi-ime/godmod/

### R7. Commit: preedit-everywhere — commit_string cho mọi mode
Không còn `delete_surrounding_text`. Mọi commit dùng `commit_string` (protocol
`zwp_input_method_v2` tự replace preedit). Không hai pha, không chờ Done.
`sync_word` + `Phase2` đã bị deprecated — giữ lại làm library API.

### R8. Deactivate: Drop, Don't Commit
- Deactivate đến SAU khi cursor đã di chuyển (click chuột, compositor xử lý trước).
- **KHÔNG** commit pending text — text sẽ bị đặt sai vị trí.
- Chỉ `engine.reset()`. Compositor tự clear preedit ở vị trí cũ.
- Với NonPreedit mode, raw keys đã forward → không thể undo, chấp nhận mất.
- Boundary keys (Tab/Enter/Space) đã commit text TRƯỚC Deactivate, nên không mất.
- Muốn giữ text đang gở → Space trước khi click.

### R9. English Restore: theo VALIDITY, không đếm phím.
Từ không parse được thành âm tiết Việt hợp lệ (qua predicate ngữ âm trong
syllable.rs: onset/coda hợp lệ + nucleus có nguyên âm) → hiển thị + commit RAW
KEYS nguyên văn (windows→windows, html→html).
Ngoại lệ đã biết: residue hợp lệ ngẫu nhiên (if→ì, expr→ẻp) — cần syllable
dictionary mới bắt được, KHÔNG quay lại cách đếm threshold.

### R10. Test Coverage: mỗi pub fn ≥1 test, mỗi bug fix có regression test.

### R11. App Support Detection (IMPLEMENTED — tín hiệu protocol, không heuristic)
```
IME thread ──ImeFeedback──▶ daemon (Adaptation, learning.rs)
  Activated / SurroundingTextSeen / DoneAck{µs} / DoneTimeout
  / Unavailable / KeyReorder / KeyChatter

Probe: focus đổi → probe thread (1.5s sleep, thread riêng — main loop vẫn
pure recv) → ProbeTimeout → chưa từng Activate? → notify 1 lần/app/session
  + /proc/PID advisor: app Electron thiếu --enable-wayland-ime → advice cụ thể.
```
**Cấm:** Spam notify. **Cấm:** persist "unsupported" vào learned store —
app không có text field cũng không Activate (chỉ persist capability DƯƠNG).
**Cấm:** log ký tự phím khi field_sensitivity == Secure (password/PIN).

### R11b. Per-field ContentType (transient, không persist)
- Password/Pin → engine OFF + passthrough + không log (đè MỌI layer).
- Terminal → ép hidden/live, TRỪ khi mode là lựa chọn user (mode_from_user).
- Digits/Number/Phone/Date/Time → passthrough.
- Reset về Normal ở mỗi Activate/Deactivate.

### R12. Live Reconfiguration (RuntimeConfig)
```
daemon ──store()──▶ Arc<RuntimeConfig> (atomics + generation)
                         │ snapshot() khi generation đổi
IME thread: maybe_reconfigure() ở process_key + Activate
```
- Field ghi TRƯỚC, generation bump SAU (Release); đọc generation Acquire trước field.
- maybe_reconfigure PHẢI commit pending trước khi apply (R8).
- enabled→disabled PHẢI release keyboard grab (không release = nuốt hết phím).
- disabled→enabled re-grab ở Activate (nơi duy nhất có QueueHandle).
- Commit path PHẢI dùng `Engine::preedit_output()` (áp NFC/NFD), KHÔNG dùng
  `preedit_string()` raw — raw chỉ dành cho hiển thị preedit.

### R13. Config resolution 4 LỚP (user > learned > builtin > global)
- Entry point: `Setting::effective_config_layered(app_id, title, learned)`.
  Thứ tự đè: user site > user app > learned > builtin site > builtin app > global.
  `ResolvedConfig` mang `mode_source`/`origin` (badge UI: bạn chỉnh/tự học/mặc định).
- Builtin = bảng tĩnh `builtin.rs` (DATA, không ghi vào setting.conf).
- Learned = `learned.toml` (~/.local/share/vi-ime/), CHỈ suggest ime_mode,
  từ tín hiệu protocol (surrounding_text, done_timeouts). Daemon là chủ sở hữu.
- `title` chỉ truyền khi app là Browser (daemon quyết định, vi-config dep-free).
- Site key = substring lowercase của window title; key dài hơn thắng.
- `tone_style` là global-only (không override per app/site).
- Settings UI là process riêng (`vi-settings` bin) ghi setting.conf;
  daemon nhận qua inotify → `ConfigManager::reload_if_changed()`.
  KHÔNG thêm IPC daemon↔settings. Settings đọc learned.toml chỉ để hiển thị.

### R14. Engine: "Parse, don't mutate" + NFD Unicode Algebra (table-free)
```
raw_keys (source of truth) ─mỗi phím─▶ normalize (Telex/VNI + undo thống nhất)
        ─▶ syllable::decompose (onset/nucleus/coda, PREDICATE ngữ âm)
        ─▶ tone placement (THUẬT TOÁN) ─▶ NFC compose (glyph.rs) ─▶ String
```
- MỘT path NFD duy nhất cho MỌI kiểu gõ (Telex, VNI, Smart). CẤM bảng
  VOWEL_CLUSTERS / bảng tra cứu nguyên âm char→char (kiểu 'a'→'á').
- Decompose bằng cấu trúc: nguyên âm = run vowel (predicate `is_vowel_char` qua
  NFD base), onset/coda check bằng danh sách category `&[&str]` (KHÔNG phải map).
  Backtracking initial cho gi/qu (gì=g+i, già=gi+a). Validity R9 = onset hợp lệ +
  coda hợp lệ + nucleus có nguyên âm; từ không hợp lệ → commit RAW keys.
- Vị trí dấu là THUẬT TOÁN, không phải data offset:
  1 nguyên âm → trên nó; có coda → nguyên âm cuối nucleus;
  triphthong (không coda) → nguyên âm giữa;
  diphthong (không coda) → nguyên âm chất lượng (â/ê/ô/ơ/ư/ă) nếu có; nếu không,
  oa/oe/uy tách theo ToneStyle (Classic=lướt, Modern=chính); còn lại → thứ nhất.
- Dấu = Unicode combining codepoint + NFC composition (glyph.rs). Ngoại lệ duy
  nhất: đ (không NFC-composable). Giữ case (Việt/VIỆT) khi render.
- Undo thống nhất (normalize.rs): merge span S + key k → M; nhấn k lần nữa →
  S + k literal, từ đó word ở literal mode. Tone key ×2 → hủy dấu + literal key.

### R15. Zero-CPU Idle
- Daemon main loop = MỘT `rx.recv()` blocking trên unified DaemonEvent channel.
  CẤM recv_timeout/try_recv/sleep trong main loop.
- Mọi feeder là blocking thread: niri pipe (tự reconnect nội bộ, backoff capped),
  inotify fd (config watch), tray callback. Không timer, không poll.
- Keyboard grab chỉ khi Activate + enabled (context-gated), release khi disable.
- Hot path engine không alloc mới sau warmup (display buffer tái dùng).

---

## 📁 File Map

| File | Lines | Role |
|------|-------|------|
| `vi-engine/src/types.rs` | ~136 | Core enums (incl. AppSupport) |
| `vi-engine/src/style.rs` | ~16 | ToneStyle (stable crate-root type, R14) |
| `vi-engine/src/engine.rs` | ~150 | Engine facade — MỘT path NFD (R14) |
| `vi-engine/src/syllable.rs` | ~300 | NFD path: decompose (predicate) + tone thuật toán + render + case + validity |
| `vi-engine/src/normalize.rs` | ~275 | Telex/VNI modifiers + undo thống nhất |
| `vi-engine/src/glyph.rs` | ~78 | Unicode algebra (NFC composition) |
| `vi-engine/src/fast_engine.rs` | ~270 | NonPreeditEngine (Done-ack R7) |
| `vi-wayland-im/src/xkb.rs` | ~270 | XKB keyboard + modifier queries |
| `vi-wayland-im/src/state.rs` | ~290 | ImeAppState + buffer + reconfigure + FieldSensitivity |
| `vi-wayland-im/src/commit.rs` | ~142 | Two-phase sync_word (diff suffix, R7) |
| `vi-wayland-im/src/actions.rs` | ~130 | process_key + apply_action (live model) + engine stage |
| `vi-wayland-im/src/feedback.rs` | ~60 | ImeFeedback + PipelineStage (R11) |
| `vi-wayland-im/src/virtual_keyboard.rs` | ~134 | VkForwarder (passthrough keys only, R2) |
| `vi-wayland-im/src/runtime.rs` | ~180 | RuntimeConfig (live reconfig, R12) + mode_from_user |
| `vi-wayland-im/src/dispatch.rs` | ~270 | IM + grab dispatch (ContentType/Surrounding/stages) |
| `vi-wayland-im/src/dispatch_stubs.rs` | ~77 | Event-less Dispatch stubs |
| `vi-config/src/builtin.rs` | ~93 | Builtin profile tables (R13 layer 3, DATA) |
| `vi-config/src/learned.rs` | ~134 | LearnedStore/LearnedProfile (R13 layer 2) |
| `vi-config/src/effective_fields.rs` | ~85 | Legacy per-field getters |
| `vi-compositor-ipc/src/wlr_toplevel.rs` | ~204 | zwlr-foreign-toplevel focus stream |
| `vi-telemetry/src/lib.rs` | ~290 | Per-app metrics + pipeline blame |
| `vi-daemon/src/learning.rs` | ~190 | Adaptation (feedback→learned/telemetry/notify) |
| `vi-daemon/src/advisor.rs` | ~57 | /proc Electron flag advisor |
| `vi-compositor-ipc/src/lib.rs` | ~110 | FocusEvent + AppCategory + trait |
| `vi-compositor-ipc/src/niri.rs` | ~130 | Niri IPC + event stream |
| `vi-compositor-ipc/src/hyprland.rs` | ~50 | Hyprland IPC (poll only) |
| `vi-config/src/lib.rs` | ~136 | ConfigManager + reload_if_changed |
| `vi-config/src/types.rs` | ~146 | Setting/AppConfig/site_configs |
| `vi-config/src/effective.rs` | ~177 | effective_config (R13) + recommended() |
| `vi-daemon/src/main.rs` | ~200 | Entry + unified blocking loop (R15) |
| `vi-daemon/src/events.rs` | ~95 | DaemonEvent bus + inotify watch |
| `vi-daemon/src/sync.rs` | ~40 | vi-config → RuntimeSnapshot mapping |
| `vi-daemon/src/settings_launcher.rs` | ~68 | Spawn vi-settings process |
| `vi-settings/src/app.rs` | ~108 | Settings window shell |
| `vi-settings/src/model.rs` | ~141 | UI model thuần (KieuGo, rows, presets) |
| `vi-settings/src/tabs/*.rs` | ≤120 mỗi file | Chung/Đầu ra/Ứng dụng/Website |
| `vi-settings/src/main.rs` | ~31 | vi-settings bin |
| `vi-plugin/src/lib.rs` | 201 | Plugin trait |
| `vi-godmod/src/` | 315 | Debug telemetry |
| `utils/src/` | 343 | NFD, log, timestamp |

---

## 🚫 VIOLATIONS

| Hành động | Mức | Hậu quả |
|-----------|-----|---------|
| `unwrap()` ngoài test | 🔴 | Crash |
| Đổi NonPreeditAction | 🔴 | Break dispatch |
| Bỏ Deactivate auto-commit | 🔴 | Mất chữ |
| File > 300 dòng | 🟡 | AI context |
| Circular dep | 🔴 | Không compile |
| Thiếu test | 🟡 | Regression |

---

## ✅ Merge Checklist

- [ ] `cargo check` clean
- [ ] `cargo test` all pass
- [ ] No file > 300 dòng
- [ ] No `unwrap()` in production
- [ ] New code has tests
- [ ] No circular deps
- [ ] AGENTS.md updated

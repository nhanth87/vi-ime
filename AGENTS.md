# AGENTS.md — vi-ime AI Agent Design Contract

> **Mục đích:** Mọi AI agent khi sửa code PHẢI tuân theo file này.
> **Vi phạm:** Merge bị từ chối. Phải revert.

---

## 🗺️ Codebase Knowledge — /understand

- **Trước khi làm việc:** chạy `/understand` hoặc đọc `.understand-anything/knowledge-graph.json`.
- **Sau khi thay đổi:** chạy `/understand --full` để cập nhật knowledge graph.

---

## 🔒 DESIGN RULES — KHÔNG ĐƯỢC PHÁ VỠ

> Đọc chi tiết: [`.agents/R18-design-rules.md`](.agents/R18-design-rules.md)

**Tóm tắt 19 rules:**
- R1. Pipeline immutable (qua plugin only)
- R2. Preedit-everywhere (commit_string, KHÔNG delete_surrounding_text)
- R3. NonPreeditAction extend-only
- R4. File ≤300 dòng
- R5. 2 crate: vi-daemon + vi-settings
- R6. Godmod = debug only
- R7. commit_string cho mọi mode
- R8. Deactivate = Drop (KHÔNG commit)
- R9. English restore = validity + dictionary
- R10. Test coverage (pub fn ≥1 test)
- R11/R11b. App detection (protocol signals) + ContentType
- R12. Live reconfig (generation-gated atomics)
- R13. 4-layer config (user > learned > builtin > global)
- R14. Engine = NFD algebra, table-free
- R15. Zero-CPU idle (blocking recv only)

### R16. Focus Pipeline & Preedit-Jump Guard
> ⚠️ **ĐỌC:** [`.agents/R16-focus-pipeline.md`](.agents/R16-focus-pipeline.md)
> trước khi sửa focus, reconfigure, set_preedit, hoặc evdev fallback.

### R17. Bản đồ tính năng hay vỡ
> ⚠️ **ĐỌC:** [`.agents/R17-feature-map.md`](.agents/R17-feature-map.md)
> trước khi sửa typing path, address bar, terminal mode, hoặc click guard.

### R18. TẮT BỘ GÕ = MỆNH LỆNH TỐI CAO — KHÔNG BAO GIỜ OVERRIDE
> ⚠️ Khi `enabled = false` (setting.conf / tray / CLI `--toggle`), TUYỆT ĐỐI
> không có đường nào tạo ra tiếng Việt. Không plugin, không app-config,
> không mode, không fallback nào được override.
>
> **Bẫy đã sập (2026-07-12):** evdev fallback (`legacy_grab` + `evdev_mode`)
> là THREAD RIÊNG grab bàn phím vật lý, compose ĐỘC LẬP với đường Wayland.
> Đường Wayland tự tôn trọng `enabled` (nhả grab qua snapshot), nhưng evdev
> thì KHÔNG — tắt bộ gõ xong vào Chrome/Electron X11 vẫn ra tiếng Việt.
>
> **Bất biến bắt buộc (mọi đường ra chữ phải thoả):**
> 1. **Không engage khi tắt:** mọi chỗ `LegacyGrab::start` phải gate
>    `config_manager.setting().enabled` (focus-change, ProbeTimeout).
> 2. **Drop ngay khi tắt:** `ConfigChanged` + `IpcWrite` phải `legacy_grab =
>    None` khi `!enabled` (đường Wayland đã tự lo qua snapshot).
> 3. **Defense-in-depth:** `evdev_mode::run_loop` tự `break` (→ ungrab) khi
>    `runtime.snapshot().enabled == false`, phòng khi tầng 1 chưa kịp drop.
>
> Thêm BẤT KỲ đường ra chữ mới nào (fallback, injector, path) → phải kiểm
> `enabled` TRƯỚC TIÊN, coi như bất biến số 0.

### R19. ONLYOFFICE evdev typer = `xdotool` riêng, KHÔNG dùng bàn phím ảo tĩnh chung
> ⚠️ Đọc trước khi sửa `legacy_grab.rs`, `evdev_inject.rs`, `evdev_typer.rs`,
> hoặc thêm app mới vào evdev fallback.
>
> **Bẫy đã sập (2026-07-12):** ONLYOFFICE Desktop Editors là app Qt/X11 (qua
> XWayland) nhưng vùng soạn thảo là cửa sổ con CEF (Chromium) nhúng bên
> trong. Bàn phím ảo tĩnh 8-level dùng chung cho mọi app evdev-fallback
> (`viet_typer.rs`/`evdev_typer.rs`, chọn ký tự có dấu bằng Mod3/Mod5) chạy
> tốt với Chrome/LibreOffice thật, nhưng cú nhảy Qt-shell → CEF-child của
> ONLYOFFICE làm rớt trạng thái modifier giả đó — mọi ký tự bị giải mã sai
> level (gõ "cửa hàng á phi âu" ra toàn dấu câu). Repro/fix xem
> `legacy_grab::needs_injector_typer` (hiện chỉ match `"onlyoffice"`) và
> `evdev_inject::Typer::detect`'s `force_xdotool` doc.
>
> **Bất biến bắt buộc khi dùng `xdotool` làm typer cho app evdev-fallback:**
> 1. **Tự resolve `DISPLAY`, không tin biến kế thừa:** `vi-daemon` là daemon
>    dài hạn, `DISPLAY` nó kế thừa lúc khởi động có thể trỏ sai socket X11
>    (giống lý do wrapper `onlyoffice-desktopeditors` tồn tại) → `xdotool`
>    lỗi "Failed creating new xdo instance", gõ im lặng thất bại không log.
>    Xem `evdev_inject::resolve_x11_display` — probe `/tmp/.X11-unix` mỗi
>    lần spawn, chỉ override cho tiến trình con, không đụng daemon.
> 2. **PHẢI giãn nhịp, dù `xdotool` cũng "keymap động":** `xdotool
>    type`/`key` với ký tự ngoài ASCII remap tạm 1 keycode → gõ → unmap —
>    ĐÚNG kiểu "keymap động" mà R14/viet_typer.rs đã cấm cho bàn phím ảo
>    riêng, vì app xử lý trễ giải mã nhầm bảng cũ. Không giãn nhịp → rớt
>    ký tự NGẪU NHIÊN ở vị trí khác nhau mỗi lần gõ cùng 1 chuỗi (field
>    2026-07-12: "Cửa"→"Ửa", "áo"→"o", "sắt"→"st" — 3 lần chạy, 3 ký tự
>    khác nhau rớt). Bắt buộc `--delay 15` trên CẢ subcommand `key` và
>    `type` (xdotool không có cờ global khi chain hai subcommand), CỘNG
>    thêm `sleep(15ms)` SAU khi mỗi tiến trình `xdotool` thoát (live-echo
>    gọi `backspace_then_type` gần như mỗi keystroke → mỗi lần là 1 process
>    mới, `cmd.status()` chỉ xác nhận xdotool đã QUEUE xong X11 event, không
>    xác nhận CEF-child đã render trước khi process kế tiếp remap keycode).
> 3. **Luôn kiểm exit status, không nuốt lỗi:** `Injector::backspace_then_type`
>    trả `bool` — nếu discard bằng `let _ = cmd.status()` như code cũ, mọi
>    lần gõ thất bại sẽ im lặng, không có evidence trail để debug (R17: xác
>    định mechanism trước khi vá).
>
> **Chỉ ONLYOFFICE cần route này** — ĐỪNG mở rộng `needs_injector_typer`
> cho cả `XWAYLAND_FALLBACK_PREFIXES` (Chrome/Chromium): Chrome dùng bàn
> phím ảo tĩnh vẫn gõ đúng (field-proven), lỗi chỉ do lớp nhúng CEF-trong-Qt
> riêng của ONLYOFFICE. Thêm app mới vào route `xdotool` chỉ khi có repro
> field xác nhận bàn phím ảo tĩnh sai với chính app đó.

---

## 📁 File Map

> Xem chi tiết: [`.agents/file-map.md`](.agents/file-map.md)

---

## 🚫 VIOLATIONS

| Hành động | Mức | Hậu quả |
|-----------|-----|---------|
| Ra chữ khi `enabled=false` (R18) | 🔴 | Mất niềm tin — tắt là phải tắt |
| `unwrap()` ngoài test | 🔴 | Crash |
| Đổi NonPreeditAction | 🔴 | Break dispatch |
| Bỏ Deactivate auto-commit | 🔴 | Mất chữ |
| File > 300 dòng | 🟡 | AI context |
| Circular dep | 🔴 | Không compile |
| Thiếu test | 🟡 | Regression |

---

## 🧠 Task Workflow — Supememory + Regression

> Áp dụng cho MỌI task, không ngoại lệ.

### Trước khi bắt đầu task
1. **Recall từ Supememory:** query context liên quan đến task.
2. Đọc lại Rules liên quan (đặc biệt R16/R17 nếu đụng typing/focus/preedit).

### Sau mỗi lần cập nhật code
3. **Chạy regression test:**
   ```bash
   cargo test -p vi-daemon
   # Nếu thay đổi typing path / live echo / evdev:
   ./scripts/vi-regression/run.sh
   ```
   KHÔNG được coi task hoàn thành nếu regression fail.

### Sau khi kết thúc task
4. **Update Supememory:** ghi tóm tắt task (file, rule, decision).
5. **Update `/understand`** nếu có thay đổi kiến trúc.

---

## ✅ Merge Checklist

- [ ] `cargo check` clean
- [ ] `cargo test` all pass
- [ ] No file > 300 dòng
- [ ] No `unwrap()` in production
- [ ] New code has tests
- [ ] No circular deps
- [ ] Supememory updated
- [ ] AGENTS.md updated (nếu có rule/file map thay đổi)

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
>
> **Field report 2026-07-12 (round 2):** flat `--delay 15` + 15ms settle
> KHÔNG đủ khi gõ liên tục thật (khác với repro 3-từ ban đầu) — user phải
> tự nghỉ tay 1-2s giữa các ký tự để tránh mất chữ. Vá lần đó: `settle_ms`
> tuỳ theo `text` có non-ASCII hay không (20ms/120ms) — NHƯNG đặt sai vị trí
> (xem field report round 3 dưới).
>
> **Field report 2026-07-13 (round 3) — VỊ TRÍ settle SAI, không phải giá trị
> sai:** debug log `[EVDEV-SYNC]` đối chiếu với chữ thật trên màn hình xác
> nhận: round 2 gộp `key` (backspace) và `type` (chữ mới) vào MỘT process
> xdotool (`xdotool key --delay 15 BackSpace type --delay 15 -- "ươ"`).
> `--delay 15` chỉ áp dụng GIỮA các tap TRONG một subcommand — không có
> khoảng nghỉ nào ở đúng điểm nối `key` kết thúc → `type` bắt đầu remap
> scratch-keycode. `settle_ms` chạy SAU KHI CẢ process thoát, không chèn vào
> khe hở thật. Mọi lệnh có CẢ backspace VÀ ký tự có dấu (rất phổ biến — mọi
> lần sửa tone/quality) trúng đúng khe hở này, xdotool luôn exit 0 (không
> WARN) vì nó không biết CEF đã rớt gì. Đối chiếu cụ thể (gõ Kiều, câu
> "trăm năm trong cõi người ta..."): "người"→"ngời" tại bước `bs=2
> suffix="ươ"`, "chữ"→"cữ" tại `bs=1 suffix="ư"`, "khéo"→"kho" tại `bs=2
> suffix="éo"` — luôn là ký tự ĐẦU TIÊN ngay sau backspace bị rớt.
>
> **Fix (round 3):** `Injector::backspace_then_type` tách `key` và `type`
> thành 2 PROCESS xdotool riêng — settle (20ms sau `key`, 20/120ms sau
> `type` theo round 2) giờ chèn ĐÚNG vào giữa hai bước, không chỉ sau cùng.
> ĐỪNG gộp lại thành 1 process "cho gọn" — đây chính là bug đã sập 2 lần.
>
> **Field report 2026-07-13 (round 4) — bug gộp process đã hết, còn sót
> flaky KHÔNG TẤT ĐỊNH:** sau round 3, câu test dài (~9 lần sửa dấu qua
> backspace) chỉ còn 2/9 lần bị nuốt hẳn ký tự type ("a"→"á" mất trắng,
> "co"→"có" mất "ó") — không lặp lại cùng vị trí giữa các lần chạy khác
> nhau, xdotool vẫn exit 0. Khác round 3 (sai VỊ TRÍ nghỉ — đã sửa dứt
> điểm), đây là CEF render không có thời hạn đảm bảo — chỉ GIẢM xác suất
> bằng settle dài hơn, không loại bỏ hoàn toàn được bằng cách này. Đã tăng
> `settle_ms` nhánh non-ASCII 120ms→200ms. Nếu vẫn còn flaky: tăng tiếp
> (ĐỪNG hạ xuống — field-confirmed không đủ ở round 2 VÀ round 4); nếu tăng
> settle mãi vẫn không hết, cân nhắc đổi cơ chế inject cho ONLYOFFICE hẳn
> (không còn là vấn đề timing nữa) — hỏi user trước khi đổi hướng lớn này.
>
> **Field report 2026-07-13 (round 5) — lỗi CÓ QUY LUẬT, không phải flaky
> ngẫu nhiên:** đối chiếu log thật xác nhận "người"→"ngời" xảy ra đúng tại
> bước `bs=2 suffix="ươ"` (mất "ư" — ký tự ĐẦU của suffix 2 ký tự). Khác
> round 4 (đơn ký tự, random vị trí giữa các lần chạy) — đây LUÔN là ký tự
> đầu tiên của một `suffix` ≥2 ký tự có dấu bị CEF nuốt, ký tự sau trong
> cùng suffix luôn vào đúng. Giả thuyết: gộp cả cụm vào một lệnh
> `xdotool type -- "ươ"` khiến CEF chưa kịp ổn định sau backspace/remap
> trước khi tap ký tự đầu của lệnh `type` mới bắn ra.
>
> **Fix (round 5, làm CẢ HAI theo yêu cầu user):**
> 1. Tách MỖI ký tự của `text` thành 1 process `xdotool type` riêng —
>    không còn gõ cả cụm "ươ" trong 1 lệnh, mỗi ký tự luôn là "ký tự đầu"
>    của lệnh chứa nó, loại bỏ vị trí rủi ro trong log.
> 2. Thêm settle 30ms TRƯỚC lệnh type đầu tiên khi có backspace (không chỉ
>    20ms SAU backspace như round 3) — dự phòng trường hợp nhiều backspace
>    cần thêm thời gian ổn định.
> ĐỪNG gộp nhiều ký tự vào 1 lệnh `type` trở lại — đây chính là bug đã sập.
> Nếu vẫn còn mất chữ sau round 5: kiểm tra có còn đúng PATTERN (luôn ký tự
> đầu của cụm) hay chuyển sang random (flaky timing như round 4) để biết
> đang gặp lỗi nào trước khi vá tiếp.
>
> Nếu gặp lỗi MỚI (không phải flaky ngẫu nhiên mà lặp lại có quy luật):
> đọc lại `[EVDEV-SYNC]` ở mức `RUST_LOG=debug` và đối chiếu TỪNG ký tự mất
> với `bs`/`suffix` trước khi vá — đừng vá settle_ms mù, tìm đúng vị trí/cơ
> chế trước (R17).

### R20. evdev fallback = reader thread riêng + processor thread riêng, KHÔNG BAO GIỜ gộp lại
> ⚠️ Đọc trước khi sửa `evdev_mode.rs` (reader_loop/consumer_loop/run_loop).
>
> **Bẫy đã sập (2026-07-13):** trước đây MỘT thread vừa `poll`+`fetch_events`
> bàn phím vật lý vừa gọi `composer.handle()` → `typer.backspace_then_type()`.
> Typer block 20-120ms/lần (settle pacing R19, để CEF/VCL kịp render trước
> lần remap kế). Trong lúc block, thread không quay lại `poll()` → hàng đợi
> phím TRONG KERNEL (kích thước cố định của thiết bị evdev đã grab) tràn khi
> gõ nhanh liên tục thật → kernel tự rớt phím TRƯỚC KHI tới engine. Không có
> fix ở tầng ứng dụng (engine/keymap/typer) cứu được — phím đã mất trước khi
> code thấy nó. Field bug: mất chữ ngẫu nhiên khi gõ nhanh trong
> LibreOffice/OnlyOffice.
>
> **Fix:** tách thành 2 vai trên 2 thread, nối bằng `std::sync::mpsc`
> UNBOUNDED (`run_loop` dùng `std::thread::scope`):
> 1. `reader_loop` (1 thread/bàn phím): CHỈ `poll` + `fetch_events` + đẩy
>    `(KeyCode, i32)` vào channel. Không được làm gì có thể block quá
>    poll timeout — nếu cần thêm việc gì ở đây, tự hỏi "việc này có thể
>    tốn >200ms không", nếu có thì KHÔNG được thêm.
> 2. `consumer_loop` (1 thread, thay cho toàn bộ `run_loop` cũ): tiêu thụ
>    channel bằng `recv_timeout(200ms)` (giữ đúng cadence cũ để check
>    enabled/click), gọi `composer.handle()` như trước — mọi pacing/settle
>    của typer vẫn ở đây, không đổi.
>
> **Bất biến bắt buộc:**
> - Channel PHẢI unbounded (`mpsc::channel`, KHÔNG `sync_channel`). Bounded
>   channel khi đầy làm `send()` phía reader BLOCK — tái tạo đúng lỗi gốc
>   (reader không kịp quay lại `poll`, kernel tràn queue lần nữa).
> - `queued: AtomicUsize` chỉ là cảnh báo mềm (log 1 dòng khi backlog ≥
>   `QUEUE_WARN_THRESHOLD`), KHÔNG dùng để chặn hay drop bất cứ gì.
> - `dead: AtomicBool` là cờ chia sẻ 2 chiều: reader lỗi fatal → set `dead`
>   để consumer thoát cùng; consumer thoát (disabled/stop) → set `dead` để
>   reader còn sống (bàn phím khác) thoát theo, tránh treo `Grabbed` (ungrab
>   trễ = bàn phím vẫn bị chiếm).
> - ĐỪNG gộp reader+processor lại "cho đơn giản" hay "giảm 1 thread" dưới
>   bất kỳ danh nghĩa tối ưu nào — đây chính là cấu trúc đã gây bug.

### R21. VietTyper (wayland/viet_typer.rs) có kết nối Wayland RIÊNG — ĐỪNG gộp lại với event queue chính
> ⚠️ Đọc trước khi sửa `wayland/viet_typer.rs`, `wayland/state.rs::ImeAppState::new`,
> hoặc `wayland/mod.rs`'s virtual-keyboard setup.
>
> **Bẫy đã sập (2026-07-13):** gõ "chữ" trong LibreOffice (đường Wayland gốc
> qua `VietTyper`, KHÔNG phải evdev fallback) ra "chu" — mất cả dấu ngã VÀ
> dấu móc, dừng lại đúng ở trạng thái TRƯỚC 2 lần sửa dấu liên tiếp
> (`chu→chư` rồi `chư→chữ`, mỗi lần cách nhau ~90ms). Không phải mất 1 ký
> tự — CẢ HAI lần sửa bị nuốt trọn. `[LIVE-SYNC]` log xác nhận engine tính
> diff đúng 100%; `backspace_then_type` báo `true` (thành công) — lỗi nằm ở
> chỗ `pace()` (bản cũ) chỉ gọi `flush()` (đẩy byte ra socket) + sleep 15ms,
> KHÔNG xác nhận VCL/gtk3 đã render xong trước khi tap kế tiếp tới.
>
> Đây CHÍNH LÀ bug "VCL/gtk3 swallows BS+char bursts whole" mà
> `evdev_typer.rs` đã ghi nhận và vá cho đường evdev fallback (2026-07-10,
> dùng `queue.roundtrip()` — chờ xác nhận thật, không chỉ flush) — nhưng
> `wayland/viet_typer.rs` (đường Wayland gốc, dùng cho app KHÔNG cần evdev
> fallback như LibreOffice khi Activate được qua protocol) chưa từng được
> vá tương tự vì `VietTyper` dùng CHUNG connection/`EventQueue` với vòng lặp
> Wayland chính — gọi `roundtrip()` ở đó từ giữa xử lý phím (bên trong
> `dispatch_pending` của vòng lặp chính) là re-entrant dispatch, rủi ro.
>
> **Fix gốc đã làm (2026-07-13):** `VietTyper` giờ tự mở connection +
> `EventQueue` RIÊNG (`VietTyper::new()` không còn nhận `vk` từ ngoài —
> tự gọi `Connection::connect_to_env()`, bind seat + virtual-keyboard-manager,
> upload keymap, `roundtrip()` xác nhận xong mới trả về), giống HỆT pattern
> `EvdevTyper` (`evdev_typer.rs`) đã field-proven. Độc lập hoàn toàn với
> event queue chính (`wayland/mod.rs`) nên `roundtrip()` bên trong
> `backspace_then_type` không còn re-entrant. Mỗi BackSpace giờ có
> `roundtrip()` thật (không chỉ flush) + sleep 15ms trước khi ký tự mới tới
> — đúng scheme `evdev_typer.rs` đã field-confirm.
>
> `wayland/mod.rs` không còn tạo `viet_keyboard` (2 virtual keyboard trên
> connection chính) — chỉ còn `virtual_keyboard` (passthrough forwarder).
> `ImeAppState::new()` mất tham số `viet_keyboard`. ĐỪNG khôi phục lại việc
> truyền `vk` từ ngoài vào `VietTyper` — đây chính là cấu trúc đã gây bug.
>
> Nếu gặp lỗi mất dấu MỚI ở đường Wayland gốc: kiểm tra `[LIVE-SYNC]` log ở
> `RUST_LOG=debug` trước — nếu vẫn đúng pattern "2 lần sửa liên tiếp mất cả
> hai", nghi ngờ `roundtrip()` không đủ (thử tăng sleep sau BackSpace,
> hiện 15ms) trước khi nghĩ tới việc khác.

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

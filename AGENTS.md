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

### R16. Focus Pipeline & Preedit-Jump Guard (sự cố 2026-07-10 — ĐỌC TRƯỚC KHI SỬA)

**Bối cảnh:** `compositor/niri.rs`'s `follow_stream` gọi `niri msg event-stream`
KHÔNG có `--json` trong nhiều tháng → output là text người-đọc
("Windows changed: ..."), không khớp substring code tìm ("WindowsChanged")
→ `parse_focused` fail 100% lần → `current_app_id` ở main.rs LUÔN LUÔN
`None`. Per-app config theo focus thực tế (R13) và mọi logic dựa vào
`rt.app_id()` coi như CHẾT LÂM SÀNG suốt thời gian đó, dù build xanh/test
xanh — vì dự án CẤM automation test (xem policy), bug này chỉ lộ ra khi
chạy tay và grep log. Đã fix: thêm `--json` vào cả `event-stream` lẫn
`niri msg --json windows`; sửa luôn `NiriWindows` struct — lệnh `windows`
trả về **mảng JSON trần** `[...]`, không phải `{"Windows":[...]}` (đó là
hình dạng của event-stream tag `WindowsChanged`, không phải command output).

**Bài học 1 — `maybe_reconfigure` KHÔNG được cố "commit an toàn" nữa, chỉ
Drop, giống mọi nhánh ngắt-ngang khác. Lịch sử 3 lần vá liên tiếp trong
CÙNG MỘT BUỔI (2026-07-10), đọc kỹ trước khi thêm lại bất kỳ điều kiện nào
vào block này:**
1. Bản gốc: LUÔN `commit_pending_then` trước khi áp config mới, bất kể
   generation đổi vì app-switch hay chỉ đổi setting cùng app. Nằm im vì
   `current_app_id` luôn `None` (bug niri `--json` ở trên) nên nhánh
   app-switch coi như chết. Fix niri.rs xong → đánh thức bug này: commit
   pending text vào app MỚI (cursor app cũ không liên quan gì) → "nhảy
   theo con trỏ".
2. Vá lần 1: tách `app_switched` — app đổi thì Drop (giống Deactivate),
   cùng app thì vẫn Commit. Ý đúng, nhưng nhánh Drop gọi
   `set_preedit(&im, "")` **vô điều kiện** — với NonPreedit/terminal, app
   chưa từng nhận `set_preedit_string` thật nên đây là message thừa →
   **y hệt triệu chứng "nhảy chữ"**, xác nhận lại trên kitty trong vòng
   1 tiếng.
3. Vá lần 2: thêm check `live = engine.mode()==NonPreedit && viet.ready()`
   trước khi gọi `set_preedit`, verify sống trên kitty (chữ thô giữ
   nguyên, không nhân đôi, không nhảy) — **nhưng user vẫn báo lỗi lặp
   lại sau đó.**
4. **Quyết định cuối (đang áp dụng):** bỏ hẳn phân biệt app-switch-vs-
   same-app, bỏ hẳn `commit_pending_then` (đã xóa khỏi commit.rs — không
   còn nơi nào gọi). `maybe_reconfigure` giờ LUÔN Drop khi có pending,
   y hệt `Deactivate`/`on_physical_click`/`Event::Done`'s external_change
   — GIỐNG NHAU cả 4 chỗ, không có case đặc biệt nào. Lý do: mỗi lần thêm
   điều kiện "commit an toàn khi X" là một chỗ mới để sai; bỏ hẳn câu hỏi
   đó là cách duy nhất để 4 chỗ này hết lệch nhau. Giá phải trả: đổi
   Telex/VNI giữa chừng một từ sẽ làm mất từ đó thay vì hoàn tất nó —
   chấp nhận được, vì mọi ngắt-ngang khác trong file này đã hành xử y hệt
   từ trước và không ai phàn nàn.

**Bài học 2 — MỌI nơi gọi `set_preedit()` sau một sự kiện "ngắt ngang"
(reconfigure, external cursor change, click) PHẢI mode-aware:**
```rust
let live = self.engine.mode() == ImeMode::NonPreedit && self.viet.ready();
if !live { /* mới được gọi set_preedit(&im, "") */ }
```
NonPreedit/live mode (terminal — kitty/foot/alacritty) không BAO GIỜ gọi
`set_preedit_string` thật — phím thô forward trực tiếp, chữ trên màn ĐÃ
LÀ text thật. Gửi `set_preedit_string("",0,0)` + `commit()` cho app không
hề yêu cầu là một protocol message thừa — xác nhận trực tiếp trên kitty: y
hệt triệu chứng "nhảy chữ theo con trỏ" (xem Bài học 1, vá lần 1). Ba chỗ
hiện đúng pattern này: `finalize_word`/`on_physical_click` (commit.rs,
actions.rs), `maybe_reconfigure` (state.rs), `Event::Done`'s
external_change (dispatch.rs). Thêm chỗ gọi `set_preedit` mới ở bất kỳ
nhánh "ngắt ngang" nào → bắt buộc check `live` y hệt.

**Notify "app chưa nhận bộ gõ" đã bị GỠ BỎ (2026-07-10)** — user thấy phiền,
và về bản chất nó noisy: app được focus nhưng không có ô nhập liệu (chỉ
bấm nút toolbar) cũng never-Activate giống hệt app thật sự lỗi, không có
tín hiệu nào để phân biệt hai case. Đừng thêm lại popup này ở `notify.rs`
trừ khi tìm được cách phân biệt hai case trên; log `[UNSUPPORTED]` trong
`learning.rs::probe_timeout` vẫn còn, dùng cho `--doctor`.

**Bài học 3 — `field_sensitivity` (Terminal ép NonPreedit, Url passthrough
cho Chrome/Firefox address bar, Secure cho password) reset về `Normal` mỗi
lần app đổi thật (`Activate` handler, dispatch.rs) — nhưng điều kiện đổi
(`state.current_app_id != prev_app`) phụ thuộc `current_app_id` được
`maybe_reconfigure` cập nhật từ `rt.app_id()`, tức từ MAIN THREAD (niri
focus) qua một channel riêng, KHÁC với Wayland thread tự nó nhận biết
Activate/ContentType. Hai luồng phải đồng bộ đúng thời điểm; sửa focus
pipeline (niri.rs, main.rs) mà không test lại address-bar Chrome VÀ
terminal cùng lúc dễ vỡ một trong hai mà không nhận ra (chỉ vỡ 1 app,
app kia vẫn ổn → dễ tưởng nhầm là app-specific bug).

**Bài học 4 — LibreOffice/OnlyOffice không đi qua `zwp_input_method_v2`
được:** LibreOffice (VCL/gtk3) chỉ gọi `text_input.enable()` MỘT LẦN lúc
focus đầu tiên, không bao giờ gọi lại khi refocus (xác nhận sống — ACTIVATE
đúng 1 lần, sau đó DEACTIVATE mãi mãi dù đổi focus qua lại nhiều lần).
OnlyOffice Desktop Editors chạy qua XWayland (`QXcbConnection`) — protocol
Wayland-thuần không bao giờ chạm X11 client. Cả hai KHÔNG sửa được từ phía
IME. Giải pháp: `legacy_grab.rs` — tự động evdev-grab bàn phím + gõ qua
`wtype` khi focus vào app tiền tố `libreoffice*/soffice*/onlyoffice*`,
chạy song song với luồng Wayland (không độc quyền như cờ `--evdev` cũ).
Cần user ở nhóm `input` (`sudo usermod -aG input $USER`) — AppRun của
AppImage tự xin quyền một lần qua `pkexec`/`sudo` rồi `sg input -c` để áp
dụng ngay, không cần đăng xuất lại.

**Mô hình evdev fallback = LIVE echo (sửa 2026-07-10, 2 vòng):** bản đầu
là buffer-and-commit (nuốt phím chữ, chỉ gõ cả từ qua wtype ở word
boundary) → user báo "LibreOffice phải commit mới hiện chữ, cả predict
lẫn nonpredict" — đúng, vì trong lúc soạn KHÔNG có gì trên màn hình, và
mode setting không liên quan (legacy path luôn dùng NonPreeditEngine).
Fix vòng 1: echo theo từng phím — mỗi keystroke diff `shown` với
`engine.preedit_output()` rồi gửi `BackSpace × k + suffix` qua wtype.
**Vòng 1 FAIL trên field ("mèo"→"mèe"):** mỗi lần spawn wtype = một
virtual keyboard + keymap MỚI (keysym→keycode phụ thuộc nội dung suffix
từng lần gọi) → seat keymap của compositor nhảy real↔wtype MỖI PHÍM;
client áp keymap trễ một nhịp là render SAI GLYPH (keycode của 'o' lần
này = keycode của 'e' lần trước). Fix vòng 2 (đang áp dụng):
`evdev_typer.rs` — MỘT `zwp_virtual_keyboard_v1` bền vững trên connection
riêng của daemon, mỗi sync upload keymap tí hon (BackSpace + suffix,
keycode 2..=33 — tái dùng builder của `viet_typer.rs`) + tap trên CÙNG
object nên keymap-trước-key được protocol bảo đảm; kết thúc bằng
roundtrip để event sau của uinput mirror không vượt mặt (cross-channel).
xdotool chỉ còn là fallback X11 (`evdev_inject.rs`). Composer ở
`evdev_compose.rs`, dùng chung cho `run` (--evdev) lẫn `run_scoped`.
Fix kèm cùng ngày: (a) digit đầu từ (VNI) / digit boundary (Telex) bị
NUỐT MẤT (PassThrough không emit) — giờ replay qua uinput tap;
(b) Backspace giữa từ vào engine (diff-erase) thay vì forward thô;
(c) **Ctrl+A bị engine ăn mất** — Composer thiếu gate system-modifier:
giờ Ctrl/Alt/Super track riêng (transparent như MODIFIER_KEYS Wayland
path), phím bấm khi đang giữ chord → finish_word + forward VERBATIM;
(d) release của phím chữ giờ forward luôn (release không press là no-op,
nhưng nếu press đã forward theo chord mà release bị nuốt = KẸT PHÍM).

### R17. BẢN ĐỒ 3 TÍNH NĂNG HAY VỠ + TẠI SAO SỬA HOÀI KHÔNG HẾT (phân tích 2026-07-10)

> Đây là kết quả trace toàn bộ code sau chuỗi regression trong R16. Agent nào
> định sửa MỘT trong ba tính năng dưới đây: đọc HẾT mục này trước, vì chúng
> chia sẻ state và điểm ngắt — sửa một cái mà không nhìn hai cái kia là
> nguồn regression chính của dự án.

**Tính năng 1 — Address bar → tiếng Anh (Url passthrough), chuỗi 3 bước:**
1. `dispatch.rs` `sensitivity_of()`: `ContentPurpose::Url → FieldSensitivity::Url`
   (app tự khai loại field qua text-input-v3).
2. `dispatch.rs` `Event::ContentType`: ghi `state.field_sensitivity = sens`;
   kèm flush phím ĐẦU đã lỡ vào engine trước khi ContentType đến
   (`should_finalize` — thiếu nó là "mất chữ đầu trong address bar").
3. `actions.rs` `process_key`, gate: `field_sensitivity ∈ {Secure, NumericRaw,
   Url}` → `vk.press()` passthrough thẳng, không qua engine. Đây là CỔNG THẬT.

**Tính năng 2 — Terminal → NonPreedit, hai đường ghi đè lẫn nhau:**
1. `dispatch.rs` `Event::ContentType`: `Terminal` → `engine.set_mode(NonPreedit)`
   ngay (trừ khi `mode_from_user`).
2. `state.rs` `maybe_reconfigure`: SAU MỖI `apply_snapshot` (vốn ghi đè mode
   về giá trị config) phải ép lại Terminal → NonPreedit. Quên bước này =
   config reload là terminal rơi về Preedit.

**Tính năng 3 — chống "nhảy theo con trỏ" trong terminal: BA lớp phòng thủ,
không phải một chỗ:**
| Lớp | Code | Khi nào hoạt động |
|---|---|---|
| Deactivate | `dispatch.rs` Event::Deactivate | CHỈ khi click sang cửa sổ KHÁC |
| text_change_cause=Other | `dispatch.rs` Event::Done + external_change | app phải report surrounding — **terminal không bao giờ gửi** |
| evdev click watch | `click_watch.rs` → `on_physical_click` + per-key guard (`actions.rs`) | CẦN nhóm `input`; không có quyền = lớp này TẮT |

**Ba lý do cấu trúc khiến "sửa lỗi này ra lỗi khác":**

1. **Một triệu chứng, nhiều cơ chế.** "Nhảy chữ theo con trỏ" = ít nhất 3 bug
   khác nhau cùng biểu hiện: (A) maybe_reconfigure commit khi đổi app — đã
   sửa (drop vô điều kiện, R16); (B) `set_preedit("")` thừa cho live mode —
   đã sửa (check `live`); (C) **click trong CÙNG cửa sổ terminal**: cả 3 lớp
   phòng thủ đều mù (không Deactivate vì cùng surface, không
   text_change_cause vì terminal không gửi, evdev tắt vì thiếu quyền) →
   engine giữ từ dở → phím kế tiếp vào `sync_shown` (`actions.rs`) gõ
   `Backspace × k` để diff — cursor đã ở chỗ mới nên backspace ĂN CHỮ TẠI
   VỊ TRÍ MỚI rồi gõ phần còn lại vào đó. **Cơ chế C chưa từng được sửa và
   KHÔNG sửa được thuần code khi thiếu quyền `input`.** User test lại sau
   khi A/B đã fix, gặp C, tưởng "lỗi quay lại" — thực ra là lỗi khác chưa
   từng đi. Khi nhận bug report "nhảy chữ": XÁC ĐỊNH CƠ CHẾ trước (xem log:
   có RECONFIG? có Deactivate? click watch có chạy?), đừng vá mù.

2. **Một biến, năm người ghi, hai luồng đua.** `field_sensitivity` bị ghi từ
   4 chỗ (ContentType / reset ở Activate / reset ở Deactivate / default);
   `engine.mode()` bị ghi từ 3 chỗ (apply_snapshot theo R13 / ContentType-
   Terminal / reconfigure-Terminal). Ai ghi SAU CÙNG thắng, thứ tự phụ thuộc
   timing từng app. Reset ở Activate so sánh `current_app_id` (main thread
   đút qua RuntimeConfig, nguồn niri IPC) với `prev_app` (Wayland thread tự
   nhớ) — hai luồng biết về CÙNG một lần đổi focus ở hai thời điểm khác
   nhau; main thread chậm → reset không bắn → Url/Terminal của app cũ dính
   sang app mới.

3. **Ba tính năng đòi ba câu trả lời TRÁI NGƯỢC tại cùng điểm ngắt.** Khi có
   chữ dở mà bị ngắt ngang: Url cần **COMMIT** (flush phím đầu kẻo mất);
   Terminal cần **TIẾP TỤC COMPOSE** (không finalize); click/reconfigure/
   deactivate cần **DROP** (R8). Ba đáp án × nhiều điểm ngắt × 2 mode
   (Preedit/live) = ma trận mà chỉnh một ô là lệch ô bên cạnh. Bất kỳ thay
   đổi nào ở MỘT điểm ngắt phải rà lại cả ma trận: address bar Chrome,
   terminal kitty, đổi setting giữa chừng từ, click giữa chừng từ.

**Tính năng 4 (bổ sung 2026-07-10) — engine "đ" đứng một mình:** gõ
`dd`/`d9` + boundary từng commit RAW ("dd"/"d9") ở MỌI app: normalize ra
`['đ']` nhưng `decompose` đòi nucleus có nguyên âm → fail → R9 restore
raw. Fix ở `syllable.rs::process`: decompose fail NHƯNG chars là đúng MỘT
initial hợp lệ (`is_onset_only`: "đ", "ngh", "nh"…) → render dạng chuẩn
hoá (`đ`), không restore raw. KHÔNG nới thêm điều kiện này (vd "chấp nhận
mọi prefix") — R9 tồn tại để "windows"→"ưindows" không xảy ra; từ tiếng
Anh thật không bao giờ là một onset Việt trần có modifier bị nuốt.

**Tính năng 5 (2026-07-10 tối) — RIVAL Ở TẦNG EVDEV + double-path LibreOffice.**
Chuỗi regression "sửa một ra ba" chiều 2026-07-10 hoá ra có một thủ phạm
ngoài code: máy dev chạy sẵn **`/usr/local/bin/fcitx5_uinput_server`**
(service HỆ THỐNG `fcitx5-uinput-meodien.service`, enabled, chạy từ boot,
quyền root) — một injector bàn phím cấp evdev của setup fcitx5 cũ. Ba hệ quả:
1. `rivals.rs` KHÔNG bắt được nó: so comm CHÍNH XÁC với "fcitx5" trong khi
   /proc comm bị cắt 15 ký tự thành "fcitx5_uinput_s" → đã đổi sang
   `starts_with`. Khi nhận bug "nuốt phím/mất dấu/shortcut chết toàn hệ
   thống": chạy `vi-ime --doctor` xem mục rival TRƯỚC khi nghi code.
2. `grab_all_keyboards` từng grab MỌI thiết bị có phím A-Z — kể cả uinput
   device của rival (`Fcitx5_Uinput_Server`) → vi-ime xử lý lại phím do
   rival tiêm = chữ đôi/loạn. Đã thêm `IGNORE_DEVICE_MARKERS` (vi-ime/
   fcitx/ibus/uinput/ydotool/wtype) — evdev fallback CHỈ grab bàn phím
   vật lý.
3. **LibreOffice text-input KHÔNG chết hẳn**: nó Activate thật ở lần focus
   ĐẦU (learned.toml ghi surrounding_text=true cho libreoffice-writer!) —
   tức là có khoảng thời gian cả Wayland path LẪN evdev legacy grab cùng
   gõ vào một cửa sổ (space đi vòng qua IM-grab replay trễ → "d ân trí").
   Fix: handshake trong main.rs — nhận `ImeFeedback::Activated` khi đang
   có legacy grab → NHẢ grab ngay (protocol path là chủ); focus lại lần
   sau không có Activate → grab engage lại như cũ. KHÔNG thêm điều kiện
   khác vào handshake này.

**Tính năng 6 (2026-07-10 tối, PROBE-VERIFIED) — VCL/gtk3 nuốt burst
BackSpace+ký-tự:** LibreOffice "nonpredict mất dấu" đã được TÁI HIỆN CÓ ĐO
ĐẠC bằng `scripts/vk-probe` (virtual keyboard thật + zenity/LibreOffice +
grim đọc kết quả). Ma trận kết quả trên LibreOffice Writer (gtk3):
| Chuỗi tap trong MỘT burst | Kết quả |
|---|---|
| 1 phím bất kỳ (kể cả ê/ệ, kể cả BackSpace trần) | ✅ ăn |
| 2+ ký tự thường (xy) | ✅ ăn |
| BackSpace + ký tự khác (BS,ệ hoặc BS,x) | ❌ NUỐT TRỌN GÓI (cả BS lẫn chữ) |
| BS → roundtrip + 15ms → ký tự | ✅ hoàn hảo ("việt" đủ dấu) |
Timestamp đơn điệu KHÔNG cứu. zenity (GTK4) không dính. kitty không dính.
→ Fix THỐNG NHẤT một pattern cho cả hai đường gõ (bản đầu từng ép builtin
LibreOffice→Preedit để né — user bác ngay vì preedit = gạch chân, đã revert):
- `viet_typer.rs::backspace_then_type`: BackSpace vào CHUNG keymap per-word
  (keycode FIRST_CODE), tap trên CÙNG object, sau mỗi BS thì flush + 15ms.
  `sync_shown` (actions.rs, Wayland live) giờ gọi hàm này — vk1 KHÔNG còn
  gõ BackSpace hộ live path nữa (một kênh duy nhất, hết cả câu hỏi
  cross-object). kitty chịu thêm 15ms MỖI phím-có-diff-lùi (phím dấu) —
  chấp nhận, đổi lấy MỘT hành vi cho mọi app.
- `evdev_typer.rs` (fallback path) cùng pattern, pace sau mỗi BS.
~~Chromium-family trên niri resolve NonPreedit ở layer builtin~~ — **KIẾN
TRÚC CUỐI (2026-07-10 khuya, sau 3 vòng trong MỘT buổi tối — đọc hết
trước khi đụng vào mode resolution/live path):**

Chuỗi sự kiện: (1) flip builtin Chromium→NonPreedit đẩy Chrome vào live
path; Blink áp `wl_keyboard.keymap` trễ VÔ HẠN ĐỊNH — không pacing nào
cứu nổi ("tu72 dau61 tie6m5" → "phò từ gâu gâu6m5", repro textarea
file:// + uinput). (2) Vá bằng builtin Chromium→Preedit → user bác ngay
("gõ trong chrome bị gạch đít cho non-preedit") vì builtin app ĐÈ toggle
global (R13). (3) Kiến trúc cuối:
- **`live_echo()` (state.rs) là predicate DUY NHẤT** cho live mode, 6 chỗ
  từng inline predicate giờ đều gọi nó: live = NonPreedit && viet.ready()
  && app ∈ KNOWN_TERMINALS. Blink/Electron/mọi app thường KHÔNG BAO GIỜ
  live-echo.
- **NonPreedit ngoài terminal = buffer ÂM THẦM** (đúng chữ R2): không
  set_preedit (không gạch chân), không viet_typer; từ hiện nguyên khối
  qua `commit_string` ở word boundary. An toàn trên mọi app có
  text-input; idle-commit (1.5s) vẫn arm để giảm mất chữ khi click.
- **BUILTIN_APPS đã GỠ hết entry Preedit** (browsers/chat): builtin đè
  global nên mỗi entry là một app mà toggle user chết; giờ silent-
  NonPreedit an toàn mọi nơi thì "Preedit cho đẹp" không còn là lý do
  đúng đắn. Chỉ giữ NonPreedit cho editor/IDE (underline đánh nhau với
  autocomplete). Nghi án "Preedit double-input dưới niri" khi xưa gần
  như chắc là rival `fcitx5_uinput_server` (Tính năng 5).
- **`SAFE_CODES` (viet_typer.rs) loại 14/15/28/29** (BS/Tab/Enter/Ctrl)
  khỏi dải keycode gán cho keymap: cache grow-only từng dồn 'ấ' lên
  code 28 = KEY_ENTER → app hiểu là Enter → "gõ 'mất' là nó tự commit
  thành dấu enter" (tự gửi message giữa từ — field 2026-07-10 khuya).
  Per-word keymap cũ không bao giờ chạm code 28 nên lỗi chỉ lộ sau khi
  có cache. KHÔNG BAO GIỜ gán ký tự vào 4 code đó dù keymap có remap.
ChromiumNiriPlugin chỉ advisory (R13) nên dòng log "forcing NonPreedit"
của nó là noise, không phải hành động.

**Tính năng 7 (2026-07-10 tối) — chống "click là mất chữ đang gõ":**
composition chỉ-là-preedit bị DROP khi click (R8, đúng và giữ nguyên —
commit lúc click là race đã tử nạn nhiều lần, R16). Giảm đau bằng 2 lớp:
1. **Idle auto-commit** (`state.rs::idle_commit`, `IDLE_COMMIT_MS=1500`):
   đang soạn preedit mà 1.5s không gõ → `finalize_word` khi cursor CHẮC
   CHẮN còn tại chỗ (an toàn tuyệt đối — y hệt user tự bấm boundary, không
   race). Cơ chế: poll timeout trong loop wayland/mod.rs, CHỈ arm khi có
   pending dạng preedit (live mode không arm — text đã thật; idle = block
   vô hạn như cũ, R15 giữ nguyên). Trade-off: nghỉ giữa từ >1.5s là từ bị
   chốt, phím dấu muộn rơi vào từ mới — hiếm, chấp nhận.
2. **Click-guard cho evdev Composer** (`click_reset`, đọc cùng counter
   click_watch qua RuntimeConfig truyền vào `LegacyGrab::start`): click là
   drop tracking, KHÔNG đụng màn hình (text live là thật, diff tiếp sẽ
   backspace nhầm chỗ mới — cơ chế C của R17).

**Tính năng 8 (2026-07-10 tối) — KẸT PHÍM SUPER/CTRL sau khi đổi cửa sổ:**
user giữ Super rồi bấm phím chuyển cửa sổ → grab forward Super PRESS qua
vk1 VÀ mirror `vk.modifiers(depressed=Super)`; Deactivate đến trước khi
user nhả Super → release không bao giờ tới IME. `release_all()` đã nhả
KEY 125 nhưng KHÔNG clear modifiers-state đã mirror — trạng thái
`vk.modifiers()` là EXPLICIT, key-release không tự xoá nó → seat giữ
Super vĩnh viễn. Fix: `release_all()` giờ gửi thêm `vk.modifiers(0,0,0,0)`.
Cùng class ở evdev path: uinput mirror forward modifier press, disengage
trước khi release → `Composer::release_mods()` nhả hết modifier còn giữ ở
teardown của `run_loop`. LƯU Ý khi review đề xuất "thiếu SYN_REPORT":
evdev crate 0.13 `VirtualDevice::emit()` TỰ ĐỘNG kết batch bằng
SYN_REPORT (đã đọc source registry) — đừng thêm SYN thủ công theo chẩn
đoán đó, kẹt phím là do modifiers-state ở trên.
`viet_typer` giờ có `cached_map` (skip keymap rebuild per phím, fix
SLOW-KEY 15-30ms) + eviction khi tràn 32 slot (không có eviction thì lần
tràn đầu tiên làm MỌI từ có ký tự mới fail vĩnh viễn); pacing BS giờ theo
tham số `paced` — `sync_shown` chỉ bật cho app family libreoffice/soffice,
terminal giữ burst-fast.

**Cơ chế kẹt phím THỨ BA (cùng ngày, dai dẳng nhất): evdev grab GIỮA LÚC
phím đang đè.** User giữ Super + chuyển cửa sổ sang LibreOffice → legacy
grab EVIOCGRAB bàn phím vật lý ~300ms sau focus, khi Super CÒN ĐANG ĐÈ →
từ đó release của Super chỉ đến vi-ime, libinput/niri không bao giờ thấy
→ với compositor phím đó đè VĨNH VIỄN. Không cứu được bằng uinput
(libinput lọc release của phím chưa từng press trên device đó) và không
liên quan hai fix modifiers ở trên. Fix chuẩn (keyd/xremap cùng dùng):
`wait_keys_clear` — poll `EVIOCGKEY` (`Device::get_key_state`) 20ms/lần,
CHỜ mọi phím nhả hết rồi mới grab (trong lúc chờ event vẫn chảy về
compositor nên release rơi đúng chỗ; stop-flag thoát chờ khi focus rời
app). Bonus: `--evdev` gõ từ terminal không còn dính phím Enter lúc launch.
KHÔNG bao giờ grab evdev mà không qua wait_keys_clear.

**Đính chính pacing (field "cua73"→màn hình "cưử", 2026-07-10 muộn):**
pace-chỉ-sau-BS KHÔNG đủ — burst 2 ký tự NGAY SAU chuỗi BS vẫn mất ký tự
thứ hai trên VCL (op `BS→pause→"ửa"` chỉ ra 'ử'). Chế độ probe pass là
pace SAU MỌI TAP → `viet_typer` (khi `paced=true`) và `evdev_typer` (luôn,
vì chỉ nhắm legacy app) giờ flush+15ms sau TỪNG tap. Đừng "tối ưu" bỏ bớt
nhịp nào khi target là VCL — mọi biến thể ít-pace-hơn đều đã fail thực địa.

**Đính chính pacing LẦN 2 (regression "commit xong mất" VNI/Tự do,
2026-07-10 khuya — REPRO CÓ ĐO ĐẠC):** commit `6b2f357` từng thu hẹp paced
về whitelist libreoffice/soffice ("chỉ VCL cần pace") — SAI, đúng lời cảnh
báo ở trên. Repro bằng uinput rollover 20ms/phím vào Electron (orca-ide)
ép NonPreedit live: từ có ký tự dấu MỚI (chưa có trong keymap cache của
`viet_typer`) ngay sau khi keymap đổi bị ăn mất đuôi — "quà"→"q",
"kẹ"→"k", "từ"→"t", "tiệm"→"tieệm". Cơ chế: client (Electron/Chromium,
nghi cả GTK cũ) áp `wl_keyboard.keymap` TRỄ MỘT NHỊP; tap keycode mới
nằm CÙNG burst với keymap upload bị giải mã theo keymap CŨ → unmapped
(mất chữ) hoặc trúng BS cũ (ăn ngược từ). Chữ ≥2 dấu dính nhiều nhất vì
mỗi lần đổi/thêm dấu là một composed char MỚI → một lần đổi keymap.
Quy tắc hiện hành (3 điểm, đừng đảo lại):
1. `actions.rs::sync_shown`: paced MẶC ĐỊNH BẬT; chỉ tắt cho
   `compositor::KNOWN_TERMINALS` (kitty/foot… đã probe burst-safe).
   `current_app_id == None` → paced (phía an toàn).
2. `viet_typer`/`evdev_typer`: sau MỖI lần `vk.keymap()` upload phải có
   nhịp flush+15ms TRƯỚC tap đầu tiên (keymap-apply beat) — pace-sau-tap
   không cứu được tap ĐẦU.
3. `evdev_compose::sync_shown`: diff theo byte phải lùi `common_bytes` về
   char-boundary của CẢ shown lẫn target — mọi chữ 2-dấu (U+1EA0..U+1EF9)
   chung prefix E1 BA/BB, đổi dấu (ứ→ừ, ề→ệ) làm byte-compare dừng GIỮA
   ký tự → slice panic giết thread legacy-grab.

**Key-repeat kẹt app-side (Chrome Ctrl+T → "tttt…", 2026-07-10 muộn):**
chuỗi: Ctrl+T forward qua vk1 → tab mới Deactivate → release_all drain
`held` → grab nhả → niri gửi `wl_keyboard.enter` kèm mảng phím ĐANG ĐÈ
(t thật sự còn đè) → app bắt đầu client-side repeat → ACTIVATE re-grab
NUỐT release thật → `vk.release()` cũ bỏ qua vì `held` không còn biết phím
→ app repeat vô hạn tới key event kế. Fix: `VkForwarder::release` giờ
LUÔN forward release (release phím chưa press là no-op với app; nuốt nó
mới là thứ gây bug). `held` chỉ còn là bookkeeping cho release_all.

**Trạng thái máy dev (2026-07-10 tối):** user ĐÃ ở nhóm `input` (click-watch
+ legacy_grab hoạt động). Rival `fcitx5-uinput-meodien.service` cần disable
thủ công (system service, cần sudo):
`sudo systemctl disable --now fcitx5-uinput-meodien.service`.
Phương án dự phòng chưa làm: heuristic "quá N giây không gõ → coi như từ
mới" (`last_key_time` hiện chỉ dùng cho telemetry REORDER, chưa dùng ngắt
từ) — chỉ giảm tần suất cơ chế C, không diệt gốc.

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

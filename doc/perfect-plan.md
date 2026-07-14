<!--
SPDX-License-Identifier: GPL-3.0-only
Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
-->

# VI-IME — Kế hoạch "Perfect" (roadmap chi tiết cho dev)

> Tài liệu này viết đủ chi tiết để **một dev mới vào dự án** cũng code được từng
> hạng mục mà không phá vỡ các bất biến đã trả giá bằng bug thực địa. Đọc hết
> **Mục 0** trước khi động vào bất kỳ task card nào.
>
> Ký hiệu ưu tiên: **P0** = làm trước, rủi ro thấp, đòn bẩy cao → **P4** = dài hạn.
> Mỗi task card có: *Mục tiêu · Nguyên nhân gốc · Đã có sẵn · Các bước · Code
> skeleton · ĐỪNG làm · Định nghĩa Done*.

---

## 0. Đọc trước khi code — kiến trúc & bất biến

### 0.1 Hai kết nối Wayland (KHÔNG được gộp)

vi-ime mở **hai** kết nối Wayland độc lập:

1. **Kết nối chính** (`wayland/mod.rs::run_ime_internal`): giữ
   `zwp_input_method_v2` + keyboard grab + một `VkForwarder`
   (`wayland/virtual_keyboard.rs`) để **re-inject** mọi phím mà IME không nuốt
   (mũi tên, Ctrl+C, Enter, phím raw của từ NonPreedit…). Vòng lặp event chặn
   trên `poll()` → **0% CPU lúc rảnh** (bất biến R15).
2. **Kết nối riêng của `VietTyper`** (`wayland/viet_typer.rs`): một
   `zwp_virtual_keyboard_v1` thứ hai, mang **keymap tĩnh 8-level** để "gõ" ra
   chữ Việt đã dựng. Nó có connection riêng vì cần gọi `roundtrip()` — gọi
   `roundtrip()` trên event queue **chính** từ trong callback của chính nó là
   re-entrant dispatch (nguy hiểm).

> ⚠️ Bất biến: **không** gọi `roundtrip()` trên queue chính từ trong key
> handler. Mọi thứ cần confirmation phải ở connection riêng.

### 0.2 Ba đường output (hiện tại)

| Đường | Khi nào | Cơ chế | File |
|---|---|---|---|
| **Preedit** | `ImeMode::Preedit` | `set_preedit_string` → `commit_string` ở word boundary | `commit.rs::set_preedit`, `finalize_word` |
| **NonPreedit-silent** | `NonPreedit` + không có VietTyper | buffer ÂM THẦM, `commit_string` nguyên khối ở boundary | `actions.rs::apply_action` (nhánh `silent`) |
| **NonPreedit-live-echo** | `NonPreedit` + `viet.ready()` | `sync_shown` → `backspace_then_type` diff trên VietTyper | `actions.rs::sync_shown`, `viet_typer.rs` |

Predicate chọn đường sống: **luôn** gọi `ImeAppState::live_echo()`
(`state.rs`) — đừng inline lại (R16 bài học: 6 chỗ từng lệch nhau).

### 0.3 Hai thiết kế đã bị loại — ĐỪNG hồi sinh vô điều kiện

- **`commit_string` sau khi vk backspace** → hai channel (input-method vs
  virtual-keyboard) **không giữ thứ tự** với nhau → reorder. Chết.
- **`delete_surrounding_text` cho MỌI app** → **một số** app phớt lờ (terminal,
  Electron cũ, XWayland) → chữ không xoá. Vì vậy hiện tại dùng mẫu số chung
  (VietTyper backspace) cho tất cả.

> **P0 dưới đây KHÔNG vi phạm điều này.** Nó chỉ bật lại
> `delete_surrounding_text` cho **đúng những app đã tự chứng minh** là honor nó
> (qua tín hiệu `surrounding_text`), chứ không dùng đại trà.

### 0.4 Các R-rule hay bị nhắc

- **R2**: NonPreedit ngoài terminal = buffer âm thầm, không gạch chân.
- **R8 — Drop, Don't Commit**: khi cursor bị dời (click chuột, đổi focus,
  external change) → **vứt** từ đang soạn, KHÔNG commit ở vị trí mới. Mọi điểm
  gián đoạn (`Deactivate`, `on_physical_click`, `external_change`,
  reconfigure) đều theo cùng một luật.
- **R13**: config 4 lớp (user override > learned > builtin > global) là nguồn
  chân lý duy nhất cho `ime_mode`; plugin chỉ *gợi ý*.
- **R14**: đường engine **cấm** bảng tra chữ literal — chữ sinh bằng algebra
  (`glyph.rs`). Bảng literal chỉ được nằm trong test (làm oracle).
- **R15**: 0% CPU lúc rảnh — chỉ chặn trên `poll()`, không polling/timer.
- **R16/R17**: mọi biến thể keymap-động / ít-pace-hơn đều **đã fail thực địa**
  (Blink áp keymap trễ vô hạn định). Keymap của VietTyper phải TĨNH, upload
  một lần.

### 0.5 Bản đồ file (những file task card đụng tới)

```
crates/vi-daemon/src/
  wayland/
    actions.rs        process_key, apply_action, sync_shown, on_physical_click
    dispatch.rs       Event::{Activate,Deactivate,Done,SurroundingText,ContentType,...}
    commit.rs         finalize_word, set_preedit, reset_word_state
    state.rs          ImeAppState, live_echo(), shown_word, FieldSensitivity
    viet_typer.rs     VietTyper, backspace_then_type (đường sleep-based)
    virtual_keyboard.rs  VkForwarder (passthrough)
    runtime.rs        RuntimeConfig, RuntimeSnapshot
  engine/
    fast_engine.rs    NonPreeditEngine, NonPreeditAction, CompositorKind
    types.rs          enum NonPreeditAction { CommitWithBackspace{backspace_count,text}, ... }
    engine.rs         Engine core (reparse-every-key, undo stack), WordTest
    syllable.rs       đặt dấu (tone placement)
    normalize.rs      quality marks (â/ư/đ...)
    viet_dict.rs      smart-English restore (86 LOC)
  config/
    learned.rs        LearnedProfile{ surrounding_text: Option<bool>, ... } (đã persist)
  compositor/
    mod.rs            KNOWN_TERMINALS, detect
  evdev_inject.rs     đường fallback (uinput/xdotool/virtual-keyboard)
```

---

## P0 — Commit phân tầng theo capability (bỏ "sleep-dance" cho app ngoan)

### Mục tiêu
App **có** báo `surrounding_text` → dùng đường `delete_surrounding_text +
commit_string` **atomic** (một message, không backspace, không `sleep`). App
**không** báo → giữ nguyên đường VietTyper hiện tại. Kết quả: xoá bỏ lớp bug
"chữ→chu / ư mất dấu sừng" cho đa số app (Firefox, GTK4, Qt 6.8) và giảm độ trễ
cảm nhận.

### Nguyên nhân gốc
`actions.rs::sync_shown` + `viet_typer.rs::backspace_then_type(paced=true)` dựa
vào `roundtrip()` + `sleep(15ms)`/`sleep(20ms)` cứng cho **mỗi glyph**. Đó là
"ack giả" cho việc app render xong — luôn racy trên app chậm, và block đúng
thread key-handler. Đây là gốc của ~5 vòng field-bug ghi trong comment
`viet_typer.rs`/`actions.rs`.

### Đã có sẵn (tận dụng, không viết lại)
- `NonPreeditAction::CommitWithBackspace { backspace_count, text }`
  (`engine/types.rs`) — **doc của enum đã ghi đúng flow cần làm**:
  `delete_surrounding_text(-N, N)` → `commit_string(text)` → `commit(serial)`.
- `LearnedProfile.surrounding_text: Option<bool>` (`config/learned.rs`) — **đã
  persist** ra `~/.local/share/vi-ime/learned.toml`, fed bởi
  `ImeFeedback::SurroundingTextSeen`. `Some(true)` = app từng báo surrounding.
- `ImeAppState.surrounding_seen: bool` (`state.rs`) — cờ per-activation, set ở
  `dispatch.rs::Event::SurroundingText`.

### Các bước

1. **Thêm capability helper** vào `state.rs`:
   - Trả `true` nếu app hiện tại honor surrounding-text, dựa trên (a) cờ
     per-activation `surrounding_seen`, HOẶC (b) profile persist (`Some(true)`)
     để **từ phím đầu tiên** của session sau đã đúng.
2. **Nối profile persist vào snapshot** (`runtime.rs`): thêm field
   `surrounding_capable: bool` vào `RuntimeSnapshot`; daemon điền nó từ
   `LearnedStore` theo `app_id` đang focus. (v1 tối giản có thể bỏ qua bước này
   và chỉ dùng `surrounding_seen` — chấp nhận từ đầu tiên của mỗi app trong
   session dùng fallback tới khi `surrounding_text` tới.)
3. **Thêm state theo dõi độ dài đã commit** (byte): `committed_bytes: usize`
   trong `ImeAppState` — cần vì `delete_surrounding_text` tính theo **BYTE**,
   chữ Việt là multi-byte UTF-8.
4. **Rẽ nhánh trong `apply_action`**: khi `live` **và** app capable → gọi đường
   mới `live_commit(...)`; ngược lại giữ `sync_shown(...)` như cũ.
5. **Cài `live_commit`**: xoá toàn bộ byte đã commit của từ + commit lại nguyên
   từ mới, **atomic trong một `commit(serial)`**. (Suffix-diff để tối ưu là
   refinement sau; từ ≤7 ký tự thì xoá-cả-commit-lại là quá đủ.)
6. **Cập nhật các điểm reset R8**: `on_physical_click`, `Deactivate`,
   `external_change`, reconfigure — khi ở đường live-commit, reset
   `committed_bytes = 0` cùng `reset_word_state()`. Không cần đụng app (chữ đã
   là text thật, để nguyên tại chỗ — đúng R8).

### Code skeleton

```rust
// state.rs — thêm field
pub(crate) committed_bytes: usize,   // byte đã commit_string cho từ hiện tại (đường live-commit)

impl ImeAppState {
    /// App hiện tại có honor surrounding-text (⇒ delete_surrounding_text an toàn)?
    pub(crate) fn app_surrounding_capable(&self) -> bool {
        if self.surrounding_seen {
            return true; // đã thấy trong activation này
        }
        // Persist từ session trước (điền vào snapshot bởi daemon từ learned.toml).
        self.runtime
            .as_ref()
            .map(|rt| rt.snapshot().surrounding_capable)
            .unwrap_or(false)
    }

    /// Đường live KHÔNG-underline, atomic, cho app capable.
    /// Thay thế sync_shown (VietTyper sleep-dance) cho nhóm app này.
    fn live_commit(&mut self, im: &ZwpInputMethodV2, target: &str) {
        // Xoá đúng số byte đã commit trước đó cho từ này, rồi commit từ mới.
        // before_length = byte trước cursor cần xoá; after_length = 0.
        if self.committed_bytes > 0 {
            im.delete_surrounding_text(self.committed_bytes as u32, 0);
        }
        if !target.is_empty() {
            im.commit_string(target.to_string());
        }
        im.commit(self.serial);          // ← MỘT commit ⇒ delete+insert atomic
        self.committed_bytes = target.len(); // len() = số BYTE UTF-8
    }
}
```

```rust
// actions.rs — trong apply_action, thay các nhánh live:
let live = self.live_echo();
let cap  = live && self.app_surrounding_capable();
// ...
NonPreeditAction::Buffer | NonPreeditAction::UpdatePreedit(_) => {
    let target = self.engine.inner().preedit_output();
    if cap {
        self.live_commit(im, &target);          // ← đường mới, atomic
    } else if live {
        self.sync_shown(&target);               // ← VietTyper cũ (fallback)
    } else if !silent {
        let s = self.engine.inner().preedit_string().to_string();
        self.set_preedit(im, &s);
    }
}
NonPreeditAction::CommitWithBackspace { text, .. } => {
    if cap {
        self.live_commit(im, &text);            // chữ đã thành text thật
    } else if live {
        self.sync_shown(&text);
    } else {
        im.commit_string(text.clone());
        im.commit(self.serial);
    }
    self.engine.reset();
    self.reset_word_state();                    // nhớ reset committed_bytes (xem dưới)
    self.vk.tap(keycode);                       // replay phím boundary
}
NonPreeditAction::ClearPreedit => {
    if cap {
        self.live_commit(im, "");               // xoá phần đã commit
    } else if live {
        self.sync_shown("");
    } else if !silent {
        self.set_preedit(im, "");
    }
}
```

```rust
// commit.rs — reset_word_state cũng phải clear committed_bytes
pub(crate) fn reset_word_state(&mut self) {
    self.shown_word.clear();
    self.committed_bytes = 0;   // ← thêm dòng này
}
```

### ĐỪNG làm
- ĐỪNG gọi `delete_surrounding_text` khi `app_surrounding_capable()` là false —
  đó chính là thiết kế đã bị loại ở **0.3**.
- ĐỪNG tách `delete_surrounding_text` và `commit_string` sang hai `commit()`
  khác nhau — phải cùng **một** `commit(serial)` để atomic.
- ĐỪNG đo `committed_bytes` bằng `.chars().count()` — phải `.len()` (byte).
- ĐỪNG bỏ `live_echo_pending` guard khi vẫn còn app dùng đường VietTyper — nhánh
  fallback vẫn cần nó (xem `dispatch.rs::TextChangeCause`).

### Định nghĩa Done
- [ ] Firefox / một app GTK4 / một app Qt 6.8: gõ "người", "chữ", "quả",
      "nghiêng" — **không** còn "nguời/chu/q", không sleep cảm nhận được.
- [ ] Terminal (kitty/foot) và LibreOffice: vẫn đi đường cũ, **không** hồi quy.
- [ ] Click chuột giữa từ ở app capable: từ đang soạn biến mất tại chỗ, không
      "nhảy theo con trỏ" (R8).
- [ ] `committed_bytes` về 0 sau mọi commit/cancel/deactivate (thêm assert log).
- [ ] Harness P1 (khi có) xanh cho cả đường live-commit và VietTyper.

---

## P0b — Tách VietTyper ra thread riêng (bỏ `sleep` khỏi event loop)

### Mục tiêu
Kể cả khi vẫn cần VietTyper cho app không-capable, **không** `std::thread::sleep`
trong key handler đồng bộ. Tách "độ mượt của IME" khỏi "nhịp render của app".

### Nguyên nhân gốc
`backspace_then_type(paced=true)` `sleep` 15–20ms mỗi glyph **ngay trong**
`sync_shown` ← `apply_action` ← `process_key`, tức trên thread event loop chính.
Một từ 6 ký tự ⇒ ~90ms daemon đứng hình, phím dồn `key_buffer`.

### Các bước
1. Cho `VietTyper` sở hữu một thread riêng + `mpsc::Sender<TypeCmd>`.
2. `TypeCmd { backspaces: usize, suffix: String, paced: bool }`.
3. Thread owner giữ connection riêng của VietTyper, thực hiện `roundtrip()` +
   pacing trên thread đó. Main thread chỉ `sender.send(cmd)` rồi trả về ngay.
4. Đồng bộ ngược: dùng một counter/oneshot để `Done`/`live_echo_pending` vẫn
   biết khi nào batch xong (giữ đúng logic suppress `TextChangeCause::Other`).

### Code skeleton
```rust
enum TypeCmd { Type { backspaces: usize, suffix: String, paced: bool }, Shutdown }

pub(crate) struct VietTyper {
    tx: Option<std::sync::mpsc::Sender<TypeCmd>>,
    ready: bool,
    map_covers: std::sync::Arc<...>,   // để type_str kiểm tra coverage không cần thread
}

impl VietTyper {
    pub(crate) fn new() -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<TypeCmd>();
        std::thread::spawn(move || {
            // mở connection riêng + keymap tĩnh Ở TRONG thread này
            // vòng while rx.recv(): thực hiện backspace_then_type_blocking(...)
        });
        // ...
    }
    /// Non-blocking: đẩy lệnh, KHÔNG sleep.
    pub(crate) fn enqueue(&self, backspaces: usize, suffix: &str, paced: bool) -> bool { /* ... */ }
}
```

### ĐỪNG làm
- ĐỪNG để coverage-check (`map.contains_key`) chạy sau khi gửi cmd — kiểm tra
  all-or-nothing **trước** khi enqueue để giữ semantics "false = chưa gõ gì".
- ĐỪNG giữ keymap ở hai nơi — thread owner là chủ duy nhất của connection.

### Định nghĩa Done
- [ ] Gõ liên tục 200 ký tự vào LibreOffice: main loop không block, `--doctor`
      cho thấy stage QueueWait không phình.
- [ ] Không hồi quy lớp "chữ→chu" trên app fallback (thread vẫn `roundtrip` +
      pace như cũ, chỉ khác chỗ chạy).

---

## P1 — Harness integration headless (đòn bẩy verify lớn nhất)

### Mục tiêu
Bắt lớp bug **render-race theo app** trong CI, TRƯỚC khi ship — thứ mà unit test
không thể bắt (đó là lý do có 5 vòng field-bug dù test coverage tốt).

### Nguyên nhân gốc
100% failure của dự án là **integration** (app render sai do race), nhưng test
hiện tại chỉ ở mức đơn vị (keymap oracle, WordTest engine).

### Các bước
1. Dựng một compositor headless test:
   - Ưu tiên `anvil` (sample compositor của Smithay) HOẶC Sway/wlroots chạy
     `WLR_BACKENDS=headless`.
2. Viết một **client text-input-v3** script được (Rust, `wayland-client`) đóng
   vai app: enable text-input, nhận `commit_string`/`preedit`/
   `delete_surrounding_text`, dựng lại buffer văn bản của chính nó.
3. Kịch bản: spawn vi-ime trỏ vào compositor headless, "gõ" một **ma trận từ
   khó** (danh sách dưới), rồi assert **buffer render của client** == kỳ vọng.
4. Chạy hai biến thể: đường live-commit (client báo surrounding_text) và đường
   VietTyper (client KHÔNG báo surrounding_text) → phủ cả hai path của P0.
5. Cắm vào CI (GitHub Actions) chạy headless.

### Ma trận từ khó (tối thiểu)
```
người nguời chữ quả khoẻ thuý thúy hoà hòa nghiêng nghịch
quốc gì giờ gịn đường dương ưu bưởi cứng rượu
Việt Nam ĐẤT nƯỚC   (hoa/thường trộn)
xin chào123 email@test  (biên giới English/số/ký hiệu)
```

### Code skeleton (client giả)
```rust
// tests/harness/fake_app.rs
struct FakeApp { buffer: String, cursor: usize, announce_surrounding: bool }
// impl Dispatch cho zwp_text_input_v3:
//   Event::CommitString{text}        => chèn tại cursor
//   Event::DeleteSurroundingText{before,after} => xoá byte quanh cursor
//   Event::PreeditString{..}         => giữ preedit riêng, không vào buffer
//   Event::Done                      => áp batch, nếu announce_surrounding thì gửi set_surrounding_text
fn assert_types(word_keys: &str, expect: &str, announce: bool) { /* spawn + drive + assert */ }
```

### Định nghĩa Done
- [ ] CI job `integration` chạy headless, xanh trên toàn ma trận, cả 2 path.
- [ ] Cố tình revert P0 → harness **đỏ** (chứng minh nó bắt được lớp bug đó).

---

## P2a — Phủ GNOME (đường uinput/XWayland) — khó, làm sau P0/P1

### Mục tiêu
Đưa vi-ime lên GNOME (desktop phổ biến nhất) ở mức tối thiểu.

### Nguyên nhân gốc (đọc kỹ trước khi hứa hẹn)
Mutter **không** hỗ trợ `zwp_input_method_v2` **và cũng không** hỗ trợ
`zwp_virtual_keyboard_v1`. Đường evdev fallback hiện tại vẫn **output qua
virtual-keyboard** (`evdev_inject.rs` cảnh báo khi thiếu, fallback `xdotool`).
Mấu chốt: **uinput không phát được Unicode tuỳ ý** (`evdev_inject.rs:108`).

### Hệ quả lựa chọn
- **GNOME Wayland-native**: không có đường sạch (không protocol, không
  virtual-keyboard, uinput không gõ được Unicode). Thực tế phải chờ Mutter hỗ
  trợ protocol, hoặc chấp nhận không hỗ trợ.
- **GNOME + XWayland app**: khả thi qua **XTEST/xdotool** (gõ Unicode qua X).
  Đây là phạm vi thực tế nên nhắm: một chế độ `--x11` dùng XTEST cho session
  X/XWayland.

### Các bước (bản X11/XWayland-only)
1. Thêm backend output `x11_xtest` song song với `virtual-keyboard`/`xdotool`.
2. Capture qua evdev grab (đã có), inject qua XTEST `XSendEvent`/keysym remap.
3. Cổng phát hiện: nếu `XDG_SESSION_TYPE=x11` hoặc app là XWayland → chọn
   backend này.

### ĐỪNG làm
- ĐỪNG hứa "GNOME Wayland-native chạy được" trong README — không có đường sạch.
  Ghi rõ giới hạn (đúng tinh thần README hiện tại đã trung thực về Mutter).

### Định nghĩa Done
- [ ] Trên session X/XWayland: gõ tiếng Việt vào một app X11 (vd gedit trên X)
      hoạt động; ghi rõ trong README "GNOME: chỉ app X/XWayland qua `--x11`".

---

## P2b — Profile capability bền vững thay hardcoded app-name list

### Mục tiêu
Router chiến lược (live/silent/preedit, fallback) dựa trên **capability đã quan
sát**, không dựa vào danh sách tên app cứng (mau lỗi thời).

### Nguyên nhân gốc
Hiện router dựa `KNOWN_TERMINALS`, danh sách browser, `ChromiumNiriPlugin`… App
mới ra là miss. Nhưng `LearnedStore` (`config/learned.rs`) đã đi 70% đường.

### Đã có sẵn
`LearnedProfile { surrounding_text, ime_activated, updated_at }` persist ra
`learned.toml`, resolve 4 lớp (user > learned > builtin > global).

### Các bước
1. Mở rộng `LearnedProfile` thêm các **capability là protocol-fact**:
   - `honors_delete_surrounding: Option<bool>` (suy ra: sau khi
     `delete_surrounding_text`, `surrounding_text` kế tiếp phản ánh xoá đúng?).
   - `is_xwayland: Option<bool>` (từ advisor `/proc/PID` — đã có
     `ElectronFlagAdvisorPlugin`).
   - `rearms_enable: Option<bool>` (LibreOffice VCL chỉ Activate 1 lần → cần
     evdev fallback).
2. Để **capability làm router chính**; danh sách tên chỉ là *seed* cho lần gặp
   đầu.
3. `--doctor` in profile thật từng app thay vì đoán theo tên.

### ĐỪNG làm
- ĐỪNG lưu heuristic đoán mò vào learned cache — chỉ lưu **protocol-fact** (đúng
  triết lý hiện tại của `learned.rs`).

### Định nghĩa Done
- [ ] Một app "lạ" (không có trong mọi danh sách) tự được route đúng sau 1–2 lần
      focus; profile ghi vào `learned.toml`.
- [ ] `--doctor` in bảng capability per-app.

---

## P3 — Đánh bóng engine ngôn ngữ (ma trận đặt dấu + smart-English)

### Mục tiêu
Khoá vĩnh viễn các ca đặt dấu khó, và giảm false positive/negative của
smart-English restore.

### Nền tảng (giữ nguyên)
Reparse-every-key + undo stack (`engine.rs`) là thiết kế **đúng**, chống nhảy
chữ tận gốc. NFC/NFD algebra (`glyph.rs`) đẹp. **Không** đụng lõi này.

### Các bước
1. **Dựng ma trận `WordTest` lớn** (`engine.rs` đã có struct `WordTest`) cho các
   ca khó, mỗi ca test cả Telex/VNI/Tự do:
   - Đặt dấu `oa/oe/uy`: `hoà`↔`hòa`, `khoẻ`, `thuý`↔`thúy`, `quả` (kiểm cả hai
     `ToneStyle`).
   - Onset đặc biệt: `gì`, `giờ`, `gịn` ('i' sau 'gi'); `quốc`, `quyển` ('u' sau
     'q' là bán âm).
   - Cặp `uo→ươ`: `đường`, `dương`, `bưởi`, `rượu` (đã xử ở `normalize.rs` — test
     để khoá regression).
   - Undo kép: `aa→â→aa`, `dd→đ→dd`, `uww→uw`, `ww→w` (đã có luật ở
     `normalize.rs`, cần test bao phủ).
2. **Tinh chỉnh `viet_dict.rs`** (86 LOC — quá nhỏ): cân nhắc dict theo **tần
   suất** thay vì set cứng, để "and/the/git" không bị Việt-hoá nhưng "gì/vì" thì
   giữ. Đo bằng một corpus nhỏ tiếng Việt + tiếng Anh trộn.

### ĐỪNG làm
- ĐỪNG thêm bảng tra chữ literal vào đường engine (R14). Bảng tham chiếu chỉ
  nằm trong test làm oracle (như `viet_typer.rs` đã làm với `REF`).

### Định nghĩa Done
- [ ] Toàn bộ ma trận `WordTest` xanh trên Telex + VNI + Tự do.
- [ ] Corpus trộn: tỉ lệ Việt-hoá nhầm từ tiếng Anh < ngưỡng đặt ra (vd < 2%),
      và không bỏ sót từ Việt hợp lệ.

---

## P4 — Đóng góp upstream (de-risk tận gốc)

### Mục tiêu
Gốc của mọi sleep-race là **protocol không cho IME biết app đã render xong** sau
delete+commit. Đây đúng là lỗ hổng phía compositor mà cả ecosystem thiếu người
làm.

### Các bước
1. Dùng harness P1 làm **repro tối giản** cho race.
2. Mở issue/MR ở **Smithay** (Rust, lợi luôn COSMIC + anvil) hoặc wlroots quanh
   ordering/confirmation của `input-method-v2` ↔ `text-input-v3`.
3. Bám issue #39 của `wayland-protocols` (chuẩn hoá input-method) và các bản
   experimental (`text-input-v3` rework, `xx-input-method-v2`).

### Vì sao đáng
Nút thắt đã được người duy trì mảng này nói thẳng: "phần lớn thời gian không có
expert làm phần compositor". Bạn là Rust + rành Wayland + đang tự viết IME →
đúng người. Và mảng này đang có nguồn tài trợ (NLnet) để xin.

---

## Thứ tự khuyến nghị

```
P0  (capability-tiered commit)   ─┐  ~80% nỗi đau (race) + ít rủi ro
P0b (tách thread VietTyper)       │  bỏ sleep khỏi event loop
P1  (harness headless)           ─┘  khoá regression cho mọi P sau
   ↓
P2b (capability profile)  →  P3 (ngôn ngữ)  →  P2a (GNOME)  →  P4 (upstream)
```

P0 + P1 xử lý race + "không bắt được regression" — hai vấn đề lớn nhất — mà
**không đụng lõi algebra** vốn đã đẹp. Làm P1 **trước hoặc song song** P0 để có
lưới an toàn khi refactor đường commit.

---

## Phụ lục A — Bất biến tuyệt đối (đọc lại trước mỗi PR)

1. Không `roundtrip()` trên queue chính từ trong key handler (0.1).
2. Không `delete_surrounding_text` cho app chưa chứng minh capability (0.3, P0).
3. Không commit khi cursor bị dời — Drop, Don't Commit (R8).
4. Không bảng tra chữ literal ở đường engine (R14).
5. Không polling/timer ở vòng lặp chính — 0% CPU idle (R15).
6. Keymap VietTyper phải TĨNH, upload một lần (R16/R17).
7. Không log phím trong field Secure (password/PIN).
8. Mọi nơi phân biệt live/preedit phải gọi `live_echo()`, không inline lại.

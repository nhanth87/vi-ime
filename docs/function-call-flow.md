# 🇻🇳 vi-im Function Call Flow — Từ Wayland đến Tiếng Việt

> **Trace đầy đủ**: Khi một từ tiếng Việt được gõ, nó đi qua những hàm nào, bảng gì.

---

## 📐 Tổng quan kiến trúc

```
┌──────────┐      ┌──────────────┐      ┌───────────────┐      ┌──────────┐      ┌──────────┐
│ Wayland  │      │ vi-daemon    │      │ vi-wayland-im │      │ vi-engine │      │  App     │
│ Keyboard │─────▶│ (main loop)  │─────▶│ (IME thread)  │─────▶│ (core)    │─────▶│ (commit) │
└──────────┘      └──────────────┘      └───────────────┘      └──────────┘      └──────────┘
```

---

## 🔄 Flow tổng thể (ví dụ: gõ "tiếng")

### Giai đoạn 0: Khởi tạo

```
main()  [vi-daemon/src/main.rs:30]
├── ConfigManager::new()          → load setting.conf
├── CompositorKind::detect()       → phát hiện Niri/Hyprland/...
├── RuntimeConfig::new()           → tạo shared config (atomic, lock-free)
├── spawn_niri_event_stream()      → bắt sự kiện focus từ compositor
├── spawn_config_watch()           → inotify theo dõi file config
├── TrayIcon::with_callback()      → system tray icon
└── thread::spawn(run_ime_shared)  → thread Wayland IME
```

### Giai đoạn 1: Bắt phím từ Wayland

```
run_ime_shared()  [vi-wayland-im/src/lib.rs:114]
└── run_ime_internal()
    ├── Connection::connect_to_env()     → kết nối Wayland socket
    ├── registry_queue_init()            → đăng ký global objects
    ├── bind(zwp_input_method_manager_v2) → protocol IME
    ├── bind(wl_seat)
    ├── get_input_method(seat)           → lấy zwp_input_method_v2
    ├── ImeAppState::new(engine)         → khởi tạo state
    └── event_queue.blocking_dispatch()  → VÒNG LẶP CHÍNH (blocking)
```

### Giai đoạn 2: Nhận key event → Dispatch

Khi người dùng gõ phím, compositor gửi event qua Wayland socket:

```
blocking_dispatch()  [vi-wayland-im/src/lib.rs:85]
└── Dispatch<ZwpInputMethodKeyboardGrabV2>::event()  [dispatch.rs:166]
    ├── Event::Keymap { fd, size }  → XkbState::set_keymap()
    │                                   └── mmap + xkb_keymap_new_from_string()
    ├── Event::Modifiers { ... }    → XkbState::update_modifiers()
    │
    └── Event::Key { key, state }  ← 👈 PHÍM ĐƯỢC NHẬN
        ├── state.buffer_key(key)           [state.rs:142]
        │   ├── XkbState::keycode_to_char()  → chuyển keycode → char
        │   ├── Coalesce check (<20ms gap)   → bỏ qua key repeat
        │   └── key_buffer.push_back()       → đẩy vào hàng đợi
        │
        └── if !waiting_for_done:
            └── state.flush_key_buffer()     [state.rs:183]
                └── while key_buffer có key:
                    └── state.process_key()  👈 XỬ LÝ TỪNG PHÍM
```

### Giai đoạn 3: Xử lý từng phím — `process_key()`

```
process_key(keycode, conn)  [vi-wayland-im/src/state.rs:197]
│
├── maybe_reconfigure()           [state.rs:106]
│   ├── RuntimeConfig::snapshot() → đọc config mới (atomic)
│   ├── Check generation counter  → bỏ qua nếu không đổi
│   ├── Auto-commit pending text  → nếu config thay đổi giữa chừng
│   └── apply_snapshot()          → cập nhật engine (method, mode, ...)
│
├── is_system_modifier_active()?  [xkb.rs:222]
│   → Nếu Ctrl/Alt/Super: forward thẳng app, không xử lý
│
├── keycode_to_char(keycode)      [xkb.rs:161]
│   ├── xkb_state_key_get_one_sym() → lấy keysym từ XKB
│   └── xkb_state_key_get_utf8()    → convert sang UTF-8 char
│
├── should_forward_key()?         [state.rs:295]
│   ├── NonPreedit: luôn forward
│   ├── Hybrid: forward nếu chưa có preedit
│   └── Preedit: không forward
│
├── [Special keys]
│   ├── Backspace → handle_backspace()
│   ├── Enter     → commit nếu có pending
│   ├── Escape    → reset engine
│   └── Delete    → commit + reset
│
├── Plugin pipeline:
│   ├── plugin_manager.pre_process_key()   → plugin chặn/thay đổi phím
│   ├── engine.push_key(ch)               → 👈 CORE: đẩy vào engine
│   └── plugin_manager.post_process_action() → plugin hậu xử lý
│
└── apply_action(action)           [actions.rs:12]
    ├── CommitWithBackspace → delete_surrounding + commit_string
    ├── UpdatePreedit       → set_preedit_string
    ├── Buffer              → không làm gì (non-preedit)
    └── PassThrough         → không làm gì
```

### Giai đoạn 4: Engine core — `NonPreeditEngine::push_key()`

```
NonPreeditEngine::push_key(ch)  [vi-engine/src/fast_engine.rs:53]
│
├── [Pass-through] nếu là control char (trừ Backspace)
│   └── CommitWithBackspace nếu có pending
│
├── [Backspace] → handle_backspace()
│
├── [Word boundary] (space, dấu câu, số)
│   └── CommitWithBackspace nếu có pending
│       ├── backspace_count = raw_count  (số ký tự raw đã gõ)
│       └── text = inner.preedit_output()
│
└── [Ký tự thường]  ← 👈 PATH CHÍNH
    ├── raw_count += 1
    └── inner.push_key(ch)  → Engine::push_key()
```

### Giai đoạn 5: Engine cơ bản — `Engine::push_key()`

```
Engine::push_key(ch)  [vi-engine/src/engine.rs:86]
│
├── is_word_boundary(ch)? 
│   ├── Whitespace, punctuation, control → YES
│   ├── Digit + Telex → YES (số là boundary)
│   ├── Digit + VNI   → NO  (số là MODIFIER)
│   └── Non-VN char   → YES
│
├── [Chữ đầu phải là letter] 
│   └── raw_keys rỗng và ch không ASCII alpha → PassThrough
│
└── [XỬ LÝ CHÍNH]
    ├── raw_keys.push(ch)          → lưu vào buffer raw
    ├── reparse()                  → 👈 PARSE LẠI TOÀN BỘ TỪ
    │   └── parser::parse(&raw_keys, method)
    └── Action::UpdatePreedit(preedit_output())
```

### Giai đoạn 6: Parser — "Parse, don't mutate"

```
parser::parse(raw_keys, method)  [vi-engine/src/parser/mod.rs:85]
│
├── detect_case(raw_keys)         → Lower / Upper / Capitalized
├── normalize::normalize(&lower, method)  👈 BƯỚC 1: NORMALIZE
│   │
│   │  Duyệt từng ký tự trong raw_keys:
│   │
│   ├── [Tone keys]
│   │   ├── Telex: s/f/r/x/j  → Acute/Grave/Hook/Tilde/Dot
│   │   ├── VNI:   1/2/3/4/5  → Acute/Grave/Hook/Tilde/Dot
│   │   └── z (Telex)          → xóa dấu
│   │
│   ├── [Double-key undo] (ass→as, ddd→dd)
│   │   ├── Tone key 2 lần  → hủy dấu, literal mode
│   │   └── Merge key 2 lần → hoàn tác merge, literal mode
│   │
│   ├── [Telex doubling] (quality marks)
│   │   ├── aa→â, ee→ê, oo→ô  (CIRCUMFLEX ◌̂)
│   │   └── dd→đ              (STROKE, special case)
│   │
│   ├── [Telex w-modifier]
│   │   ├── aw→ă  (BREVE ◌̆)
│   │   ├── ow→ơ, uw→ư  (HORN ◌̛)
│   │   ├── uow→ươ  (cặp: ư + ơ)
│   │   └── w standalone → ư
│   │
│   ├── [VNI quality digits] 6/7/8/9
│   │   ├── 6 → CIRCUMFLEX  (a6→â, e6→ê, o6→ô)
│   │   ├── 7 → HORN        (o7→ơ, u7→ư)
│   │   ├── 8 → BREVE       (a8→ă)
│   │   └── 9 → STROKE      (d9→đ)
│   │
│   └── [Plain char] → đẩy thẳng vào output
│
└── analyze::analyze(&norm.chars)  👈 BƯỚC 2: PHÂN TÍCH ÂM VỊ
    │
    ├── Duyệt INITIALS (dài→ngắn):
    │   ├── "ngh","ng","gh","gi","kh","nh","ph","qu","th","tr","ch"
    │   ├── "b","c","d","đ","g","h","k","l","m","n","p","q","r","s","t","v","x"
    │   └── "" (không có phụ âm đầu)
    │
    ├── Với mỗi initial, thử match_cluster():
    │   └── Duyệt VOWEL_CLUSTERS (dài→ngắn):
    │       ├── Triphthongs: iêu,yêu,oai,oay,oeo,uây,uôi,uya,uyê,uyu,ươi,ươu
    │       ├── Diphthongs: ai,ao,au,ay,âu,ây,eo,êu,ia,iê,iu,oa,oă,
    │       │               oe,oi,oo,ôi,ơi,ua,uâ,uê,ui,uô,uơ,uy,ưa,ưi,ươ,ưu,yê
    │       └── Monophthongs: a,ă,â,e,ê,i,o,ô,ơ,u,ư,y
    │
    └── Với mỗi cluster match, thử match_coda_exact():
        └── Duyệt CODAS: "ng","nh","ch","c","m","n","p","t"
```

### Giai đoạn 7: Render — Tạo chuỗi Unicode cuối cùng

```
render::render_into()  [vi-engine/src/parser/render.rs:26]
│
├── tone_offset(cluster_idx, has_coda, style)
│   ├── Có coda: tone trên vowel CUỐI của cluster
│   └── Không coda: dùng offset từ bảng VOWEL_CLUSTERS
│       ├── Classic: "hòa", "thúy"
│       └── Modern:  "hoà", "thuý"
│
├── push_cased(initial, case, 0)          → âm đầu với case
├── for mỗi vowel trong cluster:
│   ├── Nếu là target position → toned(ch, tone)  → áp dấu
│   │   └── glyph::compose(base, tone_mark) → NFC composition
│   └── push_cased_char(ch, case, pos)              → case
└── push_cased(coda, case, vowel_end)     → âm cuối với case
```

### Giai đoạn 8: Commit ra app

```
apply_action()  [vi-wayland-im/src/actions.rs:12]
│
├── CommitWithBackspace { backspace_count, text }:
│   ├── delete_surrounding_text(backspace_count, 0)  → xóa raw chars
│   ├── commit(serial)                                → gửi serial
│   ├── waiting_for_done = true
│   └── pending_commit = Some(text)                   → đợi "done"
│
├── [Compositor gửi Event::Done]  [dispatch.rs:117]
│   ├── serial += 1
│   ├── commit_string(pending_text)   → 👈 GỬI CHỮ TIẾNG VIỆT
│   ├── commit(serial)
│   └── flush_key_buffer()            → xử lý key tiếp theo
│
├── UpdatePreedit(s):
│   ├── set_preedit_string(s, 0, char_count)  → hiển thị underline
│   └── commit(serial)
│
├── Buffer:   không làm gì (đợi word boundary)
└── PassThrough: không làm gì
```

---

## 📊 Các bảng dữ liệu chính (DATA, not logic)

### `tables.rs` — Bảng âm vị

| Bảng | Mô tả | Số phần tử | File |
|------|-------|-----------|------|
| `INITIALS` | Phụ âm đầu (sorted dài→ngắn) | 27 | `crates/vi-engine/src/parser/tables.rs:9` |
| `VOWEL_CLUSTERS` | Cụm nguyên âm + tone offset (classic/modern) | 55 | `crates/vi-engine/src/parser/tables.rs:36` |
| `CODAS` | Phụ âm cuối | 8 | `crates/vi-engine/src/parser/tables.rs:16` |

### `glyph.rs` — Đại số Unicode (NFC composition)

| Hàm | Input | Output |
|-----|-------|--------|
| `tone_mark(tone)` | `Tone::Acute` | `'\u{0301}'` (◌́) |
| `compose(base, mark)` | `'e' + '\u{0301}'` | `'é'` (qua NFC) |
| `apply_quality(base, mark)` | `'a' + CIRCUMFLEX` | `'â'` |
| `base_of(ch)` | `'ệ'` | `'e'` (qua NFD) |

### Tone map (trong `normalize.rs:159-188`)

| Tone | Telex | VNI | Unicode Mark |
|------|-------|-----|--------------|
| Sắc | `s` | `1` | U+0301 ◌́ |
| Huyền | `f` | `2` | U+0300 ◌̀ |
| Hỏi | `r` | `3` | U+0309 ◌̉ |
| Ngã | `x` | `4` | U+0303 ◌̃ |
| Nặng | `j` | `5` | U+0323 ◌̣ |
| Ngang | (none) | (none) | (none) |

### Telex quality modifiers (trong `normalize.rs:190-206`)

| Input | Output | Cơ chế |
|-------|--------|--------|
| `aa` | `â` | CIRCUMFLEX (◌̂) |
| `ee` | `ê` | CIRCUMFLEX (◌̂) |
| `oo` | `ô` | CIRCUMFLEX (◌̂) |
| `dd` | `đ` | STROKE (special case, không có NFC) |
| `aw` | `ă` | BREVE (◌̆) |
| `ow` | `ơ` | HORN (◌̛) |
| `uw` | `ư` | HORN (◌̛) |
| `uow` | `ươ` | Cặp ư+ơ (2 ký tự cùng biến đổi) |

### VNI quality digits (trong `normalize.rs:216-242`)

| Digit | Mark | Target chars |
|-------|------|-------------|
| `6` | CIRCUMFLEX | a, e, o |
| `7` | HORN | o, u |
| `8` | BREVE | a |
| `9` | STROKE | d |

### Case handling (trong `render.rs:63-76`)

| CaseHint | Quy tắc |
|----------|---------|
| `Lower` | Tất cả lowercase |
| `Upper` | Tất cả uppercase (qua `.to_uppercase()`) |
| `Capitalized` | Chỉ ký tự đầu tiên uppercase |

---

## 🔄 Ví dụ cụ thể: Gõ `tieengs` (Telex) → "tiếng"

```
Phím gõ    | raw_keys      | Sau normalize          | Sau analyze                    | Display
-----------|---------------|------------------------|--------------------------------|---------
t          | [t]           | [t]                    | Invalid (chưa có vowel)        | "t"
i          | [t,i]         | [t,i]                  | Invalid (i ko phải cluster)    | "ti"
e          | [t,i,e]       | [t,i,ê] (ee→ê)        | Valid: t+iê, no coda           | "tiê"
e   (undo) | [t,i,e,e]     | literal mode [t,i,e,e]| Literal (double-key undo)      | "tiee"
n          | [t,i,e,n]     | [t,i,ê,n]              | t + iê + n  → "tiên"          | "tiên"
g          | [t,i,e,n,g]   | [t,i,ê,n,g]            | t + iê + ng → "tiêng"         | "tiêng"
s          | [t,i,e,n,g,s] | tone=Acute, [t,i,ê,n,g]| t + iê + ng, Acute → "tiếng"  | "tiếng"
[space]    | commit        |                        |                                | "tiếng "
```

**Tại space:**
1. `NonPreeditEngine::push_key(' ')` → word boundary
2. `CommitWithBackspace { backspace_count: 7, text: "tiếng" }`
3. `delete_surrounding_text(7, 0)` → xóa "tieengs"
4. [Đợi Done từ compositor]
5. `commit_string("tiếng")` → app nhận chữ Việt
6. `commit(serial)` → hoàn tất chu kỳ

---

## 🧵 Mô hình threading

```
Thread 1 (Main — vi-daemon)
  ├── recv(DaemonEvent)        ← blocking, zero CPU khi idle
  ├── Focus event → cập nhật RuntimeConfig
  ├── Tray event → toggle IME, switch method
  └── ConfigChanged → reload setting.conf

Thread 2 (IME — vi-wayland-im)
  ├── blocking_dispatch()      ← blocking trên Wayland socket
  ├── [Key event] → process_key() → engine.push_key()
  ├── [Done event] → commit_string() + flush_key_buffer()
  └── [Activate/Deactivate] → grab/release keyboard

Thread 3 (Focus — niri event stream) [chỉ khi Niri]
  └── spawn_niri_event_stream() → self-reconnecting pipe read

Thread 4 (Config watch)
  └── inotify.read_events_blocking() → watch setting.conf
```

---

## ⚡ Key Design Decisions

| Quyết định | Lý do |
|------------|-------|
| **Parse, don't mutate** | Mỗi keystroke re-parse toàn bộ từ → không state lỗi |
| **Zero-CPU idle** | Blocking recv + blocking dispatch → không polling |
| **Lock-free config** | Atomics + generation counter → không cần mutex giữa daemon & IME |
| **Tone placement = DATA** | `VOWEL_CLUSTERS[].classic/modern` offsets → không logic rẽ nhánh |
| **NFC = tone table** | `compose(base, tone_mark)` → Unicode DB làm thay ta |
| **Key buffer + rollover** | VecDeque 16 slots → xử lý gõ nhanh không mất phím |
| **AdaptiveDelay** | EMA tuning per-compositor → tối ưu backspace latency |
| **Plugin pipeline** | pre_process_key + post_process_action → mở rộng không sửa core |
| **Double-key undo** | Tone/merge key 2 lần → literal mode (giống Unikey) |
| **NonPreedit default** | Buffer ẩn + backspace+commit → ~0ms latency, >90% app compat |

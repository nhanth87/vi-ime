# 🚀 Hướng dẫn đọc code vi-ime — Dành cho người mới bắt đầu

> **Mục tiêu:** Sau khi đọc xong guide này, bạn sẽ hiểu được từng dòng code trong dự án.
> **Thời gian đọc:** ~30-45 phút (đọc chậm, vừa đọc vừa mở code xem)
> **Yêu cầu:** Không cần biết gì về lập trình. Chỉ cần biết tiếng Việt và tiếng Anh cơ bản.

---

## Mục lục

1. [Dự án này làm gì?](#1-dự-án-này-làm-gì)
2. [Kiến trúc tổng quan — Nhìn từ trên cao](#2-kiến-trúc-tổng-quan--nhìn-từ-trên-cao)
3. [Các "crate" (thư viện con) — Mỗi crate làm gì?](#3-các-crate-thư-viện-con--mỗi-crate-làm-gì)
4. [Đọc code từng crate — Chi tiết từng file](#4-đọc-code-từng-crate--chi-tiết-từng-file)
5. [Luồng dữ liệu — Khi bạn gõ 1 phím thì chuyện gì xảy ra?](#5-luồng-dữ-liệu--khi-bạn-gõ-1-phím-thì-chuyện-gì-xảy-ra)
6. [Các khái niệm quan trọng cần biết](#6-các-khái-niệm-quan-trọng-cần-biết)
7. [Tự sửa code — Các chỗ dễ thay đổi](#7-tự-sửa-code--các-chỗ-dễ-thay-đổi)
8. [FAQ — Các câu hỏi thường gặp](#8-faq--các-câu-hỏi-thường-gặp)

---

## 1. Dự án này làm gì?

**vi-ime** là **bộ gõ tiếng Việt chạy trên Linux**, cụ thể là trên **Wayland** (hệ thống đồ họa mới của Linux).

Nó giống như **Unikey** trên Windows vậy — bạn gõ `vieetj` thì nó tự động biến thành `việt`.

### Điểm đặc biệt của vi-ime:

- **Chạy trên Niri** (một compositor Wayland dạng lưới/tiling, scroll được)
- **Không bị trượt chữ** khi chuyển cửa sổ nhanh (vấn đề muôn thuở của IME trên Linux)
- **Hỗ trợ 3 chế độ gõ**:
  - `Preedit`: Gõ dở thì hiện chữ gạch chân (giống Unikey)
  - `NonPreedit`: Gõ ẩn trong bộ nhớ, chỉ hiện kết quả cuối cùng (nhanh nhất, dùng cho terminal)
  - `Hybrid`: Kết hợp cả 2 — bình thường ẩn, chỉ hiện khi đang phân vân (mặc định)

---

## 2. Kiến trúc tổng quan — Nhìn từ trên cao

Hãy tưởng tượng dự án như một **nhà máy sản xuất chữ Việt**:

```
┌──────────────────────────────────────────────────────────────────┐
│                        BẠN GÕ PHÍM                               │
│                    (trên bàn phím thật)                           │
└─────────────────────┬────────────────────────────────────────────┘
                      │
                      ▼
┌──────────────────────────────────────────────────────────────────┐
│                   WAYLAND COMPOSITOR (Niri)                      │
│     Nhận tín hiệu bàn phím → gửi cho chương trình IME            │
└─────────────────────┬────────────────────────────────────────────┘
                      │
                      ▼
┌──────────────────────────────────────────────────────────────────┐
│                 vi-wayland-im (CỔNG VÀO)                         │
│     Nhận phím từ Wayland, chuyển thành ký tự (qua xkb)           │
└─────────────────────┬────────────────────────────────────────────┘
                      │
                      ▼
┌──────────────────────────────────────────────────────────────────┐
│                   vi-engine (BỘ NÃO)                             │
│     - Nhận ký tự (a, a, s, w, ...)                               │
│     - Biến đổi theo luật Telex/VNI                               │
│     - aa → â, aw → ă, as → á, ...                                 │
│     - Trả về: "UpdatePreedit(\"ấ\")" hoặc "Commit(\"việt\")"  │
└─────────────────────┬────────────────────────────────────────────┘
                      │
                      ▼
┌──────────────────────────────────────────────────────────────────┐
│                   vi-wayland-im (CỔNG RA)                        │
│     Gửi kết quả ngược lại cho Wayland → hiện lên màn hình         │
└──────────────────────────────────────────────────────────────────┘
                      │
                      ▼
┌──────────────────────────────────────────────────────────────────┐
│                   ỨNG DỤNG (Firefox, Terminal, VS Code...)       │
│     Hiển thị chữ Việt đã được gõ xong                            │
└──────────────────────────────────────────────────────────────────┘
```

### Sơ đồ các crate (thư viện con) và quan hệ:

```
vi-daemon (chương trình chính)
  ├── vi-config (đọc/sửa file cấu hình setting.conf)
  ├── vi-tray (icon khay hệ thống, menu bật/tắt)
  ├── vi-compositor-ipc (giao tiếp với Niri/Hyprland để biết cửa sổ nào đang active)
  ├── vi-wayland-im (xử lý giao thức Wayland — cổng vào/ra)
  │     └── vi-engine (bộ não xử lý Telex/VNI — thuần logic, không phụ thuộc gì)
  └── vi-settings (cửa sổ cài đặt GUI — chưa hoàn thiện)
```

---

## 3. Các "crate" (thư viện con) — Mỗi crate làm gì?

Trong Rust, mỗi thư mục con trong `crates/` là một **crate** (gói code độc lập).

| Crate | Vai trò | File chính |
|-------|---------|-----------|
| **vi-engine** | 🧠 Bộ não: xử lý luật gõ Telex + VNI | `lib.rs` (~1170 dòng) |
| **vi-wayland-im** | 🔌 Cổng Wayland: nhận phím, gửi kết quả | `lib.rs` (~510 dòng) |
| **vi-daemon** | 🏠 Chương trình chính: khởi động tất cả | `main.rs` (~135 dòng) |
| **vi-config** | ⚙️ Đọc/ghi file cài đặt `setting.conf` | `lib.rs` (~400 dòng) |
| **vi-compositor-ipc** | 🔍 Phát hiện app đang dùng (Niri/Hyprland) | `lib.rs` (~225 dòng) |
| **vi-tray** | 📌 Icon khay hệ thống | `lib.rs` |
| **vi-settings** | 🪟 Cửa sổ cài đặt GUI | `lib.rs` |

---

## 4. Đọc code từng crate — Chi tiết từng file

### 4.1 `vi-engine` — Bộ não xử lý tiếng Việt ⭐ QUAN TRỌNG NHẤT

Đây là **trái tim của dự án**. File quan trọng nhất để đọc.

#### Cấu trúc thư mục:
```
crates/vi-engine/src/
├── lib.rs            ← File CHÍNH (1170 dòng) — Engine, các test
├── fast_engine.rs    ← Engine tốc độ cao (NonPreedit mode)
├── telex.rs          ← Luật gõ Telex
├── vni.rs            ← Luật gõ VNI
├── tone.rs           ← Định nghĩa các thanh điệu (sắc, huyền, hỏi, ngã, nặng)
├── syllable.rs       ← Cấu trúc 1 âm tiết tiếng Việt
└── tone_placement.rs ← Thuật toán đặt dấu thanh vào đúng nguyên âm
```

#### Cách đọc `lib.rs` (file chính):

**Bước 1: Hiểu các enum (kiểu dữ liệu) được định nghĩa**

Mở file `crates/vi-engine/src/lib.rs`, bắt đầu từ dòng 1-120. Bạn sẽ thấy:

```rust
// Dòng 20-24: Có 2 kiểu gõ
pub enum InputMethod {
    Telex,  // Gõ telex: aa=â, aw=ă, ee=ê, ...
    Vni,    // Gõ VNI: a6=â, a8=ă, e6=ê, ...
}

// Dòng 30-40: Có 3 chế độ gõ
pub enum ImeMode {
    Preedit,     // Luôn hiện chữ gạch chân khi gõ dở
    NonPreedit,  // Ẩn hoàn toàn, chỉ hiện kết quả cuối
    Hybrid,      // Bình thường ẩn, chỉ hiện khi phân vân
}

// Dòng 116-123: Engine trả về 3 loại Action
pub enum Action {
    UpdatePreedit(String),  // "Cập nhật chữ đang gõ dở"
    Commit(String),         // "Xuất chữ ra màn hình"
    PassThrough,            // "Bỏ qua, không phải chữ Việt"
}
```

**Bước 2: Hiểu struct Engine (dòng 129-148)**

Đây là "cỗ máy" chính. Nó chứa:

```rust
pub struct Engine {
    method: InputMethod,       // Đang dùng Telex hay VNI?
    buffer: Syllable,          // Chữ đang gõ dở, ví dụ "viê"
    raw_keys: Vec<char>,       // Các phím đã gõ, ví dụ ['v','i','e','e']
    has_preedit: bool,         // Đang có chữ dở dang không?
    lang_mode: LanguageMode,   // Đang detect tiếng Anh/Việt?
    auto_detect: bool,         // Có tự động phát hiện tiếng Anh?
    free_tone: bool,           // Cho phép đặt dấu tự do?
    output_mode: OutputMode,   // Xuất Unicode dựng sẵn hay tổ hợp?
    english_key_count: u32,    // Đếm số phím tiếng Anh liên tiếp
    is_english_word: bool,     // Đã phát hiện đang gõ tiếng Anh?
}
```

**Bước 3: Hiểu hàm `push_key()` (dòng 262-272) — HÀM QUAN TRỌNG NHẤT**

Đây là hàm được gọi MỖI LẦN bạn gõ 1 phím:

```rust
pub fn push_key(&mut self, ch: char) -> Action {
    // 1. Kiểm tra: có phải đang gõ tiếng Anh không?
    if self.should_pass_through(ch) {
        return Action::PassThrough;  // Bỏ qua
    }

    // 2. Nếu là tiếng Việt, xử lý theo Telex hoặc VNI
    match self.method {
        InputMethod::Telex => self.push_telex(ch),
        InputMethod::Vni => self.push_vni(ch),
    }
}
```

Ví dụ: bạn gõ `v`, `i`, `e`, `e`, `t`, `j`, ` ` (space)

| Lần | Ký tự | Hàm gọi | Kết quả |
|------|-------|---------|---------|
| 1 | `v` | push_telex | UpdatePreedit("v") |
| 2 | `i` | push_telex | UpdatePreedit("vi") |
| 3 | `e` | push_telex | UpdatePreedit("vie") |
| 4 | `e` | push_telex | UpdatePreedit("viê") — ee→ê |
| 5 | `t` | push_telex | UpdatePreedit("viêt") |
| 6 | `j` | push_telex | UpdatePreedit("việt") — j=nặng |
| 7 | ` ` | push_telex | **Commit("việt")** — Xuất ra! |

**Bước 4: Đọc `should_pass_through()` (dòng 221-259)**

Hàm này quyết định: "Phím này có phải tiếng Việt không?"

```rust
fn should_pass_through(&mut self, ch: char) -> bool {
    // Nếu tắt auto-detect → luôn coi là tiếng Việt
    if !self.auto_detect { return false; }

    // Kiểm tra: ký tự này có phải phím tiếng Việt?
    let is_vn_key = /* ch là nguyên âm (a,ă,â,e,ê,...) hoặc phím dấu (s,f,r,x,j) */;

    if is_vn_key {
        self.english_key_count = 0;  // Reset bộ đếm tiếng Anh
        return false;                // Là tiếng Việt, xử lý tiếp
    }

    // Không phải phím Việt → tăng bộ đếm tiếng Anh
    self.english_key_count += 1;

    // Nếu gõ 4 phím tiếng Anh liên tiếp → phát hiện đang gõ tiếng Anh!
    if self.english_key_count >= 4 {
        self.is_english_word = true;
        return true;  // Bỏ qua, không xử lý nữa
    }

    false
}
```

**Bước 5: Đọc các file con**

- **`telex.rs`** (244 dòng): Định nghĩa luật Telex
  - `tone_for_key('s')` → trả về `Tone::Acute` (dấu sắc)
  - `tone_for_key('f')` → trả về `Tone::Grave` (dấu huyền)
  - `tone_for_key('r')` → trả về `Tone::Hook` (dấu hỏi)
  - `tone_for_key('x')` → trả về `Tone::Tilde` (dấu ngã)
  - `tone_for_key('j')` → trả về `Tone::Dot` (dấu nặng)
  - `handle_vowel_or_special()`: Xử lý aa→â, aw→ă, ee→ê, oo→ô, ow→ơ, uw→ư, dd→đ, w→ư

- **`vni.rs`** (242 dòng): Định nghĩa luật VNI
  - 1=sắc, 2=huyền, 3=hỏi, 4=ngã, 5=nặng
  - 6=^ (mũ), 7=horn (râu ơ/ư), 8=breve (trăng ă), 9=đ

- **`tone_placement.rs`** (404 dòng): THUẬT TOÁN QUAN TRỌNG — Đặt dấu vào đâu?
  - Ví dụ: chữ `hoa` + sắc → dấu đặt ở `a` cuối → `hoá`
  - Ví dụ: chữ `tiên` + sắc → iê là cặp đôi → dấu đặt ở `ê` → `tiến`
  - Quy tắc: ưu tiên nguyên âm có sẵn dấu (â,ê,ô,ơ,ư), sau đó mới đến nguyên âm cuối

- **`fast_engine.rs`** (560 dòng): Engine nhanh — dùng cho chế độ NonPreedit
  - Thay vì gửi preedit mỗi lần gõ, nó buffer âm thầm rồi xóa+chèn 1 lần

- **`syllable.rs`** (73 dòng): Cấu trúc 1 âm tiết
  - `initial`: phụ âm đầu (ngh, tr, ph...)
  - `vowel`: nguyên âm
  - `final_`: phụ âm cuối (ng, t, nh...)
  - `tone`: thanh điệu

- **`tone.rs`** (18 dòng): Enum đơn giản
  ```rust
  pub enum Tone {
      Level,  // Không dấu
      Acute,  // Sắc
      Grave,  // Huyền
      Hook,   // Hỏi
      Tilde,  // Ngã
      Dot,    // Nặng
  }
  ```

---

### 4.2 `vi-wayland-im` — Cổng giao tiếp với Wayland

File duy nhất: `crates/vi-wayland-im/src/lib.rs` (~510 dòng)

#### Cấu trúc:

```
1-77:    FFI — Gọi thư viện C (libxkbcommon) để đọc phím
78-230:  XkbState — Quản lý bàn phím (layout, keymap)
231-240: Wayland protocol types
241-380: ImeAppState — Trạng thái chính của IME
381-500: Dispatch — Xử lý sự kiện Wayland (Activate, Deactivate, Key, Done)
501-510: Public API — Hàm run_ime() để khởi động
```

#### Các phần quan trọng:

**1. XkbState (dòng 82-230):**
- Nhận keymap từ compositor (qua `mmap`)
- `keycode_to_char()`: Biến mã phím (số) thành ký tự (chữ)
  - Ví dụ: keycode 38 → 'a', keycode 56 → 'b'

**2. ImeAppState (dòng 252-380):**
```rust
pub struct ImeAppState {
    pub engine: NonPreeditEngine,  // Bộ não xử lý chữ Việt
    pub active: bool,              // Đang active không?
    pub input_method: ...,         // Kết nối Wayland
    pub keyboard_grab: ...,        // Bắt bàn phím
    pub xkb: XkbState,             // Đọc phím
    pub serial: u32,               // Số thứ tự sự kiện
    pub ime_enabled: bool,         // Bật/tắt IME
    delay: AdaptiveDelay,          // Độ trễ thông minh
    key_buffer: VecDeque<KeyEvent>,// Hàng đợi phím (xử lý gõ nhanh)
    waiting_for_done: bool,        // Đang chờ compositor xác nhận
    pending_commit: Option<String>,// Chữ đang chờ xuất ra
}
```

**3. Cách xử lý 1 phím (hàm `process_key`, dòng 248):**

```
Phím từ bàn phím
  → XkbState.keycode_to_char() → ký tự (vd: 'a')
  → Nếu là Backspace/Enter/Escape/Delete → xử lý đặc biệt
  → Không thì: engine.push_key(ch)
  → Nhận Action từ engine:
      - CommitWithBackspace: gọi delete_surrounding_text + commit_string
      - UpdatePreedit: gửi preedit lên màn hình
      - Buffer: không làm gì (non-preedit mode)
      - PassThrough: bỏ qua
```

**4. Dispatch events (dòng 381-500):**

Đây là nơi nhận sự kiện từ Wayland:
- `Event::Activate` (dòng 433): Khi click vào ô nhập liệu → bắt bàn phím
- `Event::Deactivate` (dòng 443): Khi rời khỏi ô nhập liệu → **TỰ ĐỘNG COMMIT** chữ đang gõ dở (tránh mất chữ!)
- `Event::Done` (dòng 461): Compositor xác nhận đã xử lý xong → flush buffer
- `Event::Key` (dòng 504): Có phím mới → `buffer_key()` rồi `flush_key_buffer()`

---

### 4.3 `vi-daemon` — Chương trình chính

File: `crates/vi-daemon/src/main.rs` (~135 dòng)

Đây là nơi mọi thứ bắt đầu. Khi bạn chạy `vi-daemon`:

```rust
fn main() {
    // 1. Khởi tạo logging (in ra terminal để debug)
    tracing_subscriber::fmt().init();

    // 2. Đọc file cấu hình setting.conf
    let mut config_manager = ConfigManager::new(...);

    // 3. Phát hiện compositor (Niri? Hyprland?)
    let compositor = CompositorKind::detect();

    // 4. Khởi động theo dõi cửa sổ Niri (real-time)
    vi_compositor_ipc::spawn_niri_event_stream(focus_tx);

    // 5. Tạo icon khay hệ thống
    let (tray, tray_rx) = TrayIcon::new(ime_status);

    // 6. Chạy Wayland IME trong 1 luồng riêng (thread)
    thread::spawn(move || {
        vi_wayland_im::run_ime(method, mode);
    });

    // 7. Vòng lặp chính: xử lý click chuột vào tray icon + focus change
    loop {
        // Nhận thông báo từ tray (bật/tắt, đổi phương thức, thoát)
        // Nhận thông báo focus change từ Niri
    }
}
```

---

### 4.4 `vi-config` — Quản lý cấu hình

File: `crates/vi-config/src/lib.rs` (~400 dòng)

#### Cấu trúc file cấu hình (`setting.conf`):

```toml
input_method = "telex"          # Gõ Telex hay VNI?
enabled = true                  # Bật IME?
output_mode = "unicodedungsan"  # Xuất dựng sẵn hay tổ hợp?
free_tone_placement = true      # Cho đặt dấu tự do?
auto_detect_lang = true         # Tự phát hiện tiếng Anh?
ime_mode = "hybrid"             # Chế độ gõ: preedit/nonpreedit/hybrid

[app_configs]
foot = { ime_mode = "nonpreedit" }       # Terminal foot → nonpreedit
kitty = { ime_mode = "nonpreedit" }      # Terminal kitty → nonpreedit
"chromium-browser" = { ime_mode = "hybrid" }  # Chrome → hybrid
firefox = { ime_mode = "hybrid" }        # Firefox → hybrid
code = { ime_mode = "nonpreedit" }       # VS Code → nonpreedit
```

#### Các hàm quan trọng:

- `ConfigManager::new(path)`: Đọc file config, nếu không có thì tạo mặc định
- `setting.effective_method(app_id)`: Lấy phương thức gõ cho app cụ thể
- `setting.effective_ime_mode(app_id)`: Lấy chế độ gõ cho app cụ thể
- `config_manager.save()`: Lưu thay đổi xuống file

---

### 4.5 `vi-compositor-ipc` — Giao tiếp với Niri/Hyprland

File: `crates/vi-compositor-ipc/src/lib.rs` (~225 dòng)

#### Chức năng:

1. **Phát hiện app đang active**: Gọi lệnh `niri msg --json windows` để lấy danh sách cửa sổ
2. **Phân loại app**: `AppCategory::classify("foot")` → `Terminal`
3. **Theo dõi real-time**: `spawn_niri_event_stream()` lắng nghe sự kiện focus change từ Niri

#### AppCategory (phân loại app):

```rust
pub enum AppCategory {
    Terminal,  // foot, kitty, alacritty, wezterm, ghostty...
    Browser,   // chromium, firefox, brave, edge, opera...
    Editor,    // code, vscode, jetbrains, emacs, sublime...
    Chat,      // discord, slack, telegram, signal...
    Other,     // Các app khác
}
```

Mỗi loại app có chế độ IME khuyên dùng:
- Terminal → NonPreedit (terminal xử lý preedit kém)
- Browser → Hybrid (cần visual feedback khi điền form)
- Editor → NonPreedit (code editor không cần preedit)

---

### 4.6 `vi-tray` — Icon khay hệ thống

File: `crates/vi-tray/src/lib.rs`

Chức năng đơn giản:
- Hiện icon V (xanh lá = bật, xám = tắt)
- Menu chuột phải: Bật/Tắt, Đổi Telex↔VNI, Cài đặt, Thoát
- Gửi message qua channel khi người dùng click

---

### 4.7 `vi-settings` — Cửa sổ cài đặt GUI

File: `crates/vi-settings/src/lib.rs`

Hiện tại còn đơn giản, dùng egui (thư viện GUI Rust) để hiện cửa sổ cài đặt.
Chưa hoàn thiện đầy đủ.

---

## 5. Luồng dữ liệu — Khi bạn gõ 1 phím thì chuyện gì xảy ra?

### Ví dụ: Bạn muốn gõ chữ "việt" (Telex: v-i-e-e-t-j-space)

```
Bước 1: Bạn gõ 'v' trên bàn phím
  → Niri nhận tín hiệu → gửi keycode 55 cho vi-wayland-im
  → XkbState.keycode_to_char(55) → 'v'
  → engine.push_key('v')
  → Engine: chưa có preedit, 'v' là chữ cái, thêm vào buffer "v"
  → Trả về: Action::UpdatePreedit("v")
  → Wayland: gửi set_preedit_string("v") → màn hình hiện 'v' gạch chân

Bước 2: Bạn gõ 'i'
  → engine.push_key('i')
  → buffer đang có "v", thêm 'i' → "vi"
  → Trả về: Action::UpdatePreedit("vi")
  → Màn hình hiện 'vi' gạch chân

Bước 3: Bạn gõ 'e'
  → engine.push_key('e')
  → buffer: "vie"
  → Trả về: Action::UpdatePreedit("vie")

Bước 4: Bạn gõ 'e' (lần 2)
  → engine.push_key('e')
  → Telex: ee → ê!
  → buffer: "viê"
  → Trả về: Action::UpdatePreedit("viê")

Bước 5: Bạn gõ 't'
  → engine.push_key('t')
  → buffer: "viêt"
  → Trả về: Action::UpdatePreedit("viêt")

Bước 6: Bạn gõ 'j' (dấu nặng)
  → engine.push_key('j')
  → Telex: j → Tone::Dot (dấu nặng)
  → apply_tone_to_vowel(): đặt dấu nặng vào 'ê' → "việt"
  → Trả về: Action::UpdatePreedit("việt")

Bước 7: Bạn gõ SPACE
  → engine.push_key(' ')
  → Space là word boundary → COMMIT!
  → Trả về: Action::Commit("việt")
  → Wayland: gửi commit_string("việt") → chữ 'việt' xuất hiện trên màn hình
  → Engine reset: buffer rỗng, sẵn sàng cho từ tiếp theo
```

### Trong chế độ NonPreedit (terminal):

```
Bước 1-6: Giống hệt trên, nhưng Wayland không hiện preedit
  → Các Action::UpdatePreedit → chuyển thành Buffer (giữ im trong bộ nhớ)

Bước 7: Gõ SPACE
  → Action::CommitWithBackspace { backspace_count: 6, text: "việt" }
  → Wayland:
    1. delete_surrounding_text(6, 0)  ← Xóa 6 ký tự "vieetj" khỏi màn hình
    2. Đợi compositor Done
    3. commit_string("việt")          ← Chèn chữ "việt" vào
  → Kết quả: người dùng thấy "việt" xuất hiện, không thấy bước trung gian
```

---

## 6. Các khái niệm quan trọng cần biết

### 6.1 Wayland là gì?

Wayland là **hệ thống đồ họa** (display server) mới trên Linux. Nó thay thế X11 cũ.
- Trên Wayland, mỗi ứng dụng vẽ trực tiếp lên màn hình
- Compositor (như Niri) quản lý vị trí, kích thước cửa sổ

### 6.2 IME Protocol (input-method-v2) là gì?

Là giao thức chuẩn để viết bộ gõ trên Wayland:

- `zwp_input_method_manager_v2`: Quản lý IME — "ai là bộ gõ?"
- `zwp_input_method_v2`: Một IME instance — "bộ gõ đang chạy"
- `zwp_input_method_keyboard_grab_v2`: Bắt bàn phím — "cho tôi nhận phím thay vì app"

Các hàm quan trọng:
- `commit_string("việt")`: Xuất chữ ra app
- `set_preedit_string("viê", 0, 3)`: Hiện chữ đang gõ dở
- `delete_surrounding_text(6, 0)`: Xóa 6 ký tự bên trái con trỏ
- `commit(serial)`: Gửi tất cả thay đổi lên compositor

### 6.3 Preedit là gì?

**Preedit** = chữ đang gõ dở, hiện lên với gạch chân:
- Ví dụ bạn gõ `vieetj` → màn hình hiện `việt` (có gạch chân)
- Gõ space → gạch chân biến mất, chữ được "commit"
- Preedit giúp bạn thấy mình đang gõ gì, nhưng 1 số app (terminal) không hỗ trợ

### 6.4 NonPreedit là gì?

**NonPreedit** = KHÔNG hiện chữ gạch chân:
- Khi gõ `vieetj`, màn hình vẫn hiện `vieetj` (không có gì thay đổi)
- Gõ space → **xóa** `vieetj`, **chèn** `việt`
- Nhanh hơn, tương thích với mọi app (kể cả terminal)
- Nhược điểm: không thấy visual feedback

### 6.5 Hybrid là gì?

**Hybrid** = kết hợp:
- Bình thường: non-preedit (không hiện gì)
- Khi ambiguous (phân vân Anh/Việt): hiện preedit
- Tốt nhất cho hầu hết trường hợp

### 6.6 Deactivate auto-commit là gì?

Khi bạn chuyển cửa sổ (Alt+Tab hoặc Niri scroll) lúc đang gõ dở:
- IME nhận sự kiện `Deactivate`
- Nếu có chữ đang gõ dở → **TỰ ĐỘNG COMMIT** chữ đó
- Tránh mất chữ! (Đây là bug phổ biến trên IME Linux)

---

## 7. Tự sửa code — Các chỗ dễ thay đổi

### 7.1 Đổi ngưỡng phát hiện tiếng Anh

Mở `crates/vi-engine/src/lib.rs`, dòng ~253:
```rust
if self.english_key_count >= 4 {  // Đổi số này: 3 = nhanh hơn, 5 = chậm hơn
    self.is_english_word = true;
    return true;
}
```

### 7.2 Thêm app mới vào danh sách phân loại

Mở `crates/vi-compositor-ipc/src/lib.rs`, dòng ~40:
```rust
if matches!(id.as_str(),
    "foot" | "footclient" | "kitty" | ... | "YOUR_APP_ID_HERE") {
    return AppCategory::Terminal;
}
```

### 7.3 Đổi chế độ gõ mặc định cho 1 app

Mở `setting.conf`:
```toml
[app_configs]
"your-app-id" = { ime_mode = "nonpreedit" }
```

### 7.4 Đổi delay cho compositor

Mở `crates/vi-engine/src/fast_engine.rs`, dòng ~250:
```rust
CompositorKind::Niri => 1500.0,  // Microseconds — tăng nếu bị lỗi commit
```

### 7.5 Thêm luật Telex mới (nếu cần)

Mở `crates/vi-engine/src/telex.rs`, thêm vào `handle_vowel_or_special()`:
```rust
// Ví dụ: thêm luật "uu → ư"
if last == 'u' && ch == 'u' {
    let mut s = build_string_from_raw(raw_keys, raw_keys.len() - 1);
    s.push('ư');
    return Some(TelexResult::UpdatePreedit(s));
}
```

---

## 8. FAQ — Các câu hỏi thường gặp

### Q: Tại sao phải dùng Rust mà không phải Python/C++?

Rust an toàn về bộ nhớ (không bị crash như C++) nhưng vẫn nhanh như C++.
IME cần tốc độ cao (xử lý hàng ngàn phím/giây) và không được crash.

### Q: `cargo check` / `cargo test` / `cargo build` khác gì nhau?

- `cargo check`: Kiểm tra code có lỗi không (nhanh, không tạo file chạy)
- `cargo build`: Biên dịch ra file chạy được (chậm hơn)
- `cargo test`: Chạy tất cả unit test

### Q: Làm sao để chạy thử?

```bash
cd ~/Desktop/github-vui/vi-im
cargo build --release
./target/release/vi-daemon path/to/setting.conf
```

### Q: Làm sao để debug (xem log)?

```bash
RUST_LOG=debug ./target/release/vi-daemon
```

### Q: Tại sao trên terminal gõ tiếng Việt bị lỗi?

Vì terminal (foot, kitty, alacritty) xử lý preedit kém. Giải pháp:
- Dùng chế độ `nonpreedit` cho terminal (đã cấu hình sẵn trong setting.conf)
- Hoặc dùng terminal hỗ trợ tốt: foot, kitty

### Q: Làm sao để tắt IME tạm thời?

- Click chuột phải vào icon V ở khay hệ thống → chọn "Toggle IME"
- Hoặc sửa `setting.conf`: `enabled = false`

---

## 📚 Tài liệu tham khảo thêm

- [Wayland Input Method Protocol](https://wayland.app/protocols/input-method-unstable-v2) — Giao thức IME
- [Niri IPC](https://github.com/YaLTeR/niri/wiki/IPC) — Giao tiếp với Niri
- [Rust Book](https://doc.rust-lang.org/book/) — Học Rust từ đầu (tiếng Anh)
- [Telex typing rules](https://en.wikipedia.org/wiki/Telex_(input_method)) — Luật gõ Telex
- [VNI typing rules](https://en.wikipedia.org/wiki/VNI) — Luật gõ VNI

---

> **Chúc bạn đọc code vui vẻ!** Nếu có chỗ nào không hiểu, cứ hỏi — code này được viết để người mới cũng đọc được. 🎉

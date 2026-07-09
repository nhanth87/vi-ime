# vi-ime — Phân Tích & Sửa Lỗi cho Niri / Tiling DE

> Ngày: 2026-07-03
> Mục tiêu: Đưa bộ gõ tiếng Việt vi-ime từ trạng thái prototype → test được thực tế trên Niri và các tiling Wayland compositor.

---

## 1. Tổng quan dự án

vi-ime là bộ gõ tiếng Việt viết bằng Rust, giảm lược từ fcitx5, chạy trực tiếp trên Wayland protocol (`input-method-v2`). Không phụ thuộc fcitx5/ibus.

### Cấu trúc

```
vi-im/
├── crates/
│   ├── vi-engine/          # Core Telex/VNI engine (93 tests)
│   ├── vi-wayland-im/      # Wayland input-method-v2 protocol
│   ├── vi-compositor-ipc/  # Niri/Hyprland IPC tracker
│   ├── vi-config/          # Config manager (TOML)
│   ├── vi-tray/            # System tray icon
│   ├── vi-daemon/          # Main binary
│   └── vi-settings/        # egui settings window
```

---

## 2. Các lỗi đã phát hiện & sửa

### 🔴 CRITICAL #1: Offset sai trong `set_preedit_string`

**File:** `crates/vi-wayland-im/src/lib.rs`

**Mô tả:**
Protocol `input-method-v2` yêu cầu `cursor_begin` và `cursor_end` trong `set_preedit_string`
là **UTF-8 byte offset**. Nhưng code cũ dùng `s.len()` để tính offset — với chuỗi ASCII
thì `len()` = số byte = số ký tự. Với tiếng Việt, mỗi ký tự có dấu chiếm 2-3 byte,
dẫn đến cursor hiển thị sai vị trí.

**Code cũ (BUG):**
```rust
let cursor_end = s.len() as i32;  // SAI: byte length != char count
input_method.set_preedit_string(s, cursor_end, cursor_end);
```

**Code mới (FIX):**
```rust
// cursor_begin=0, cursor_end = số ký tự (char count)
let char_count = s.chars().count() as i32;
input_method.set_preedit_string(s, 0, char_count);
```

---

### 🔴 CRITICAL #2: Deactivate không commit — text corruption

**File:** `crates/vi-wayland-im/src/lib.rs` (hàm `Dispatch<ZwpInputMethodV2>`)

**Mô tả:**
Khi đang gở dang dở (có preedit) mà người dùng click chuột sang cửa sổ khác,
compositor gửi `Event::Deactivate`. Code cũ chỉ `engine.reset()` — preedit bị xóa
mà không hề commit. Văn bản đang gở bị mất. Trên tiling DE (Niri) — nơi chuyển
cửa sổ xảy ra liên tục — đây là lỗi nghiêm trọng.

**Code cũ (BUG):**
```rust
Event::Deactivate => {
    state.active = false;
    state.keyboard_grab.take().map(|g| g.release());
    state.engine.reset();  // Mất hết preedit!
}
```

**Code mới (FIX):**
```rust
Event::Deactivate => {
    info!("IME deactivated");
    // CRITICAL: Commit pending preedit before deactivating
    if state.engine.has_preedit() {
        let committed = state.engine.preedit_string().to_string();
        info!("Auto-committing preedit on deactivate: \"{}\"", committed);
        state.engine.reset();
        proxy.commit_string(committed);
        proxy.commit(state.serial);
    }
    state.active = false;
    if let Some(grab) = state.keyboard_grab.take() {
        grab.release();
    }
}
```

---

### 🟡 HIGH #3: vi-compositor-ipc toàn stub

**File:** `crates/vi-compositor-ipc/src/lib.rs`

**Mô tả:**
Trait `ActiveWindowWatcher` chỉ có stub. `NiriWatcher::current_app_id()` luôn trả về `None`.
Không thể track active window để per-app config hoặc auto-commit khi chuyển cửa sổ.

**Đã implement:**

#### NiriWatcher (polling `niri msg --json windows`)
```rust
pub struct NiriWatcher {
    niri_binary: String,
    available: bool,
}

impl ActiveWindowWatcher for NiriWatcher {
    fn current_app_id(&mut self) -> Option<String> {
        let windows = self.query_windows()?;
        for w in &windows.windows {
            if w.is_focused == Some(true) {
                return w.app_id.clone();
            }
        }
        windows.windows.first().and_then(|w| w.app_id.clone())
    }
}
```

#### Niri event-stream (real-time, Phase 2)
```rust
pub fn spawn_niri_event_stream(tx: std::sync::mpsc::Sender<Option<String>>) {
    // Spawn thread lắng nghe "niri msg event-stream"
    // Khi WindowFocusChanged → re-query windows → send app_id
}
```

#### HyprlandWatcher (polling `hyprctl activewindow -j`)
```rust
impl ActiveWindowWatcher for HyprlandWatcher {
    fn current_app_id(&mut self) -> Option<String> {
        let output = Command::new("hyprctl").arg("activewindow").arg("-j").output().ok()?;
        let w: HyprWindow = serde_json::from_str(&stdout).ok()?;
        w.class
    }
}
```

#### Auto-detect
```rust
pub fn auto_detect_watcher() -> Box<dyn ActiveWindowWatcher> {
    let niri = NiriWatcher::new();
    if niri.is_available() { return Box::new(niri); }
    let hypr = HyprlandWatcher::new();
    if hypr.is_available() { return Box::new(hypr); }
    Box::new(/* no-op */)
}
```

---

### 🟡 MEDIUM #4: `is_word_boundary` không xử lý edge cases

**File:** `crates/vi-engine/src/lib.rs`

**Mô tả:**
Hàm cũ không nhận diện control characters, CJK, emoji làm word boundary.
Khi user paste text có ký tự đặc biệt hoặc gõ trong môi trường có unicode
phức tạp, engine có thể không commit đúng lúc.

**Code cũ:**
```rust
fn is_word_boundary(ch: char) -> bool {
    ch.is_ascii_punctuation() || ch.is_ascii_whitespace() 
    || ch.is_ascii_digit()
    || !ch.is_ascii_alphabetic() && !ch.is_lowercase() && ch > '\u{00FF}'
}
```

**Code mới:**
```rust
fn is_word_boundary(ch: char) -> bool {
    if ch.is_ascii_whitespace() || ch.is_ascii_punctuation() || ch.is_ascii_digit() {
        return true;
    }
    if ch.is_ascii_control() {
        return true;
    }
    if !ch.is_ascii() && !is_vietnamese_char(ch) {
        return true;
    }
    false
}

fn is_vietnamese_char(ch: char) -> bool {
    matches!(ch,
        'a'..='z' | 'A'..='Z'
        | 'à'..='ạ' | 'À'..='Ạ'
        | 'á'..='ặ' | 'Á'..='Ặ'
        | 'â' | 'Â' | 'ê' | 'Ê' | 'ô' | 'Ô'
        | 'ơ' | 'Ơ' | 'ư' | 'Ư' | 'ă' | 'Ă'
        | 'đ' | 'Đ'
    )
}
```

---

### 🟢 LOW #5: `is_printable_keysym` unreachable patterns

**File:** `crates/vi-wayland-im/src/lib.rs`

**Mô tả:**
Pattern `0x0100..=0x10FFFF` match tất cả Unicode codepoints, khiến các pattern
sau nó (BackSpace, Return, Tab...) không bao giờ reach được.

**Fix:** Tách thành 2 range riêng và exclude private use area.

---

## 3. Kết quả kiểm thử

```bash
$ cargo check
    Checking vi-engine ...
    Checking vi-compositor-ipc ...
    Checking vi-wayland-im ...
    Checking vi-daemon ...
    Finished ✅

$ cargo test -p vi-engine -p vi-config
running 93 tests (vi-engine) ... 93 passed ✅
running 5 tests (vi-config)  ... 5 passed  ✅
```

---

## 4. Chiến lược để perfect trên Niri & tiling DE

### 4.1 Non-preedit mode (VMK-like) — ƯU TIÊN CAO NHẤT

**Tại sao cần:**
- Preedit gây rắc rối trên tiling DE: cửa sổ resize/rearrange liên tục
- Một số app (Electron, Chrome) không hỗ trợ preedit tốt
- VMK đã chứng minh non-preedit hoạt động ổn định trên Wayland

**Cơ chế (từ VMK):**
```
1. Nhận key input → buffer trong engine
2. Khi hoàn thành 1 từ:
   a. Đọc surrounding text từ compositor (text-input-v3)
   b. Tính toán số ký tự cần xóa
   c. Gửi Backspace N lần (qua delete_surrounding_text)
   d. Commit chuỗi tiếng Việt hoàn chỉnh
```

**Implement trong vi-ime:**
```rust
// Thêm vào Engine
pub enum ImeMode {
    Preedit,      // Chuẩn: dùng set_preedit_string
    NonPreedit,   // VMK-like: backspace + commit
}

// Trong vi-wayland-im
fn handle_key_press_non_preedit(&mut self, ch: char) {
    let action = self.engine.push_key(ch);
    match action {
        Action::Commit(s) => {
            // Tính số ký tự gốc đã gõ
            let raw_len = self.engine.raw_key_count();
            // Xóa raw keys cũ
            input_method.delete_surrounding_text(-(raw_len as i32), raw_len as u32);
            // Commit chuỗi mới
            input_method.commit_string(s);
            input_method.commit(self.serial);
        }
        Action::UpdatePreedit(s) => {
            // Trong non-preedit mode: không gửi preedit, giữ trong buffer
        }
        _ => {}
    }
}
```

### 4.2 Niri event-stream integration

```rust
// Trong vi-daemon main():
let (focus_tx, focus_rx) = std::sync::mpsc::channel();
vi_compositor_ipc::spawn_niri_event_stream(focus_tx);

// Trong event loop:
if let Ok(Some(app_id)) = focus_rx.try_recv() {
    // Đổi phương thức gõ nếu cần
    // Hoặc auto-commit nếu đang gõ dở
}
```

### 4.3 Popup candidate window

**Yêu cầu khắt khe:**
- Dùng `zwp_input_popup_surface_v2`
- Surface role: `xdg_popup` (KHÔNG `xdg_toplevel`!)
- Neo theo `cursor_rectangle` từ `text-input-v3`
- Trên Niri: nếu dùng `xdg_toplevel`, popup sẽ bị coi là cửa sổ tiling → chiếm nửa màn hình

```rust
// Tạo popup surface
let surface = compositor.create_surface(&qh, ());
let popup_surface = input_method.get_input_popup_surface(&surface, &qh, ());
let xdg_surface = xdg_wm_base.get_xdg_surface(&surface, &qh, ());
let xdg_popup = xdg_surface.get_popup(None, parent, &positioner, &qh, ());
// Gán role cho popup
xdg_popup.grab(seat, serial);
```

### 4.4 App compatibility matrix

| App | Status | Ghi chú |
|-----|--------|---------|
| **foot** (terminal) | ✅ Hỗ trợ text-input-v3 | Nên test đầu tiên |
| **kitty** | ✅ Hỗ trợ text-input-v3 tốt | |
| **Alacritty** | ⚠️ Hạn chế | Chưa hỗ trợ đầy đủ |
| **vi/vim/helix** (trong terminal) | ✅ Nếu terminal hỗ trợ | Phụ thuộc terminal |
| **Chrome/Chromium** | ⚠️ Cần flag | `--enable-wayland-ime --wayland-text-input-version=3` |
| **Firefox** | ✅ Hỗ trợ text-input-v3 | Từ Firefox 121+ |
| **Electron app** (VS Code, Discord) | ⚠️ Cần flag | `--enable-wayland-ime --wayland-text-input-version=3` |
| **GTK app** | ⚠️ Bỏ qua | Không trong scope |
| **KDE/Qt app** | ⚠️ Bỏ qua | Không trong scope |

### 4.5 Timer/debounce cho Niri window switch

Khi Niri chuyển cửa sổ (scroll), event-stream có thể spam nhiều event.
Cần debounce để tránh engine reset liên tục:

```rust
use std::time::{Instant, Duration};

struct DebouncedFocus {
    last_focus_change: Instant,
    pending_app_id: Option<String>,
}

impl DebouncedFocus {
    fn handle_focus_change(&mut self, app_id: Option<String>, engine: &mut Engine) {
        let now = Instant::now();
        if now - self.last_focus_change < Duration::from_millis(100) {
            // Debounce: lưu pending, xử lý sau
            self.pending_app_id = app_id;
            return;
        }
        self.last_focus_change = now;
        // Commit preedit nếu đang gõ dở
        if engine.has_preedit() {
            // ...
        }
    }
}
```

---

## 5. So sánh với VMK

| Tiêu chí | VMK | vi-ime |
|----------|-----|--------|
| **Nền tảng** | C++ plugin cho fcitx5 | Rust standalone |
| **Cơ chế chính** | Non-preedit (Backspace + commit) | Preedit chuẩn |
| **Wayland** | Qua fcitx5 frontend | Trực tiếp input-method-v2 |
| **Tiling DE** | Ổn (VMK1HC cho IDE) | Cần thêm non-preedit mode |
| **Popup** | Qua fcitx5 | Chưa có (M4) |
| **Per-app config** | Thủ công qua sconfig | Tự động qua compositor IPC |
| **Build** | Script cài .so vào fcitx5 | `cargo build` |
| **Phụ thuộc** | fcitx5 ≥ 5.1.7 | Chỉ cần Wayland compositor |

**Bài học từ VMK:**
1. Non-preedit mode cho tương thích cao nhất (>90% app)
2. Lưu state vào /tmp (RAM) để chống mất engine khi XIM lỗi
3. Backspace ảo + delay logic = mô phỏng chính xác UniKey
4. VMK1HC cho IDE: lưu state vào RAM, không mất khi fcitx5 mất kết nối

---

## 6. Roadmap tiếp theo

### Ngắn hạn (1-2 tuần)
- [ ] **Non-preedit mode** (quan trọng nhất)
- [ ] Test thực tế trên Niri nested session
- [ ] Sửa `cargo build` (cài `libxdo-dev`)

### Trung hạn (2-4 tuần)
- [ ] **Popup candidate** với `xdg_popup`
- [ ] Chromium/Electron auto-flag wrapper
- [ ] Niri event-stream real-time focus tracking

### Dài hạn (Phase 2)
- [ ] X11/XWayland XIM server
- [ ] Dictionary/gõ tắt
- [ ] Plugin system (`.so` plugins)

---

## 7. Cách test thực tế

```bash
# 1. Cài dependencies
sudo apt install libxdo-dev  # cho tray-icon

# 2. Build
cd ~/Desktop/github-vui/vi-im
cargo build

# 3. Chạy nested Niri session (an toàn, không phá session chính)
niri --session

# 4. Trong Niri session:
#    - Mở terminal foot/kitty
#    - Chạy IME với WAYLAND_DEBUG để xem log protocol
WAYLAND_DEBUG=1 cargo run -p vi-daemon 2>&1 | tee debug.log

# 5. Test các case:
#    - Gõ "vieetj nam" → phải ra "việt nam"
#    - Đang gõ dở "việ" → click sang cửa sổ khác → phải commit "việ"
#    - Chuyển cửa sổ bằng Niri scroll → không mất chữ
#    - Mở Chrome với flag IME → gõ được
#    - Gõ trong terminal + vim → hoạt động
```

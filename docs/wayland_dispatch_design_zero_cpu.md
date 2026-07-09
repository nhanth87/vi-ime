# 🌊 Wayland Dispatch Integration — Full Implementation

> **Kiến trúc tổng thể:** Virtual Backspace (evdev + uinput, tương tự OpenKey/VMK-core)
> — tránh hoàn toàn Preedit bugs (focus loss, popup disappear).
>
> **Protocol stack:** `input-method-v2` + `input-method-keyboard-grab-v1`,
> bypass IBus/Fcitx, native trên wlroots (Hyprland, Niri, COSMIC).
>
> `zwp_input_method_v2_commit_string()` → chốt từ đẩy ký tự vào app khi word boundary.
> Sau đó gọi `zwp_input_method_v2_commit()` để áp dụng.

---

## 📦 Dependencies — vi-wayland-im

```toml
[package]
name    = "vi-wayland-im"
version = "0.1.0"
edition = "2021"

[dependencies]
wayland-client          = "0.31"
wayland-protocols       = { version = "0.32", features = ["unstable"] }
smithay-client-toolkit  = "0.19"
xkbcommon               = { version = "0.7", features = ["wayland"] }
vi-engine               = { path = "../vi-engine" }
vi-config               = { path = "../vi-config" }
tokio                   = { version = "1", features = ["rt", "time", "sync", "macros"], optional = true }
libc                    = "0.2"
anyhow                  = "1"
tracing                 = "0.1"

[features]
default     = []
async-burst = ["tokio"]
```

---

## 📁 Crate Map

| File | Lines | Role |
|------|-------|------|
| `vi-wayland-im/src/state.rs` | ~80 | IME global state (Wayland objects + engine + burst) |
| `vi-wayland-im/src/actions.rs` | ~130 | Commit actions: virtual_backspace, commit_string, passthrough |
| `vi-wayland-im/src/dispatch.rs` | ~270 | Core key dispatch: handle_key, game mode, backspace, word boundary |
| `vi-wayland-im/src/runtime.rs` | ~180 | Wayland event loop + poll + wakeup pipe |
| `vi-wayland-im/src/commit.rs` | ~60 | CommitStrategy enum (Immediate vs Burst) |
| `vi-wayland-im/src/virtual_keyboard.rs` | ~230 | XKB keymap upload + key/modifier injection |
| `vi-wayland-im/src/burst.rs` | ~320 | BurstTimer (tokio) + BurstTimerSync (std) |
| `vi-wayland-im/src/events.rs` | ~20 | ImeEvent enum (feedback từ IME → daemon) |
| `vi-config/src/lib.rs` | ~130 | Shared types: InputMethod, ViConfig, SharedConfig |
| `vi-tray/src/lib.rs` | ~250 | GTK tray icon + menu + message passing |
| `vi-im/src/main.rs` | ~260 | Single binary entry point + event router |
| `vi-im/src/cli.rs` | ~80 | CLI args parser (manual, no clap) |
| `vi-im/src/instance_lock.rs` | ~60 | PID lockfile single-instance guard |
| `vi-im/src/signal.rs` | ~30 | SIGTERM/SIGINT handler |


## 📁 `state.rs` — IME Global State

```rust
//! IME global state — shared giữa Wayland dispatch thread và engine thread.

use smithay_client_toolkit::reexports::client::{
    protocol::{wl_keyboard::WlKeyboard, wl_seat::WlSeat},
    Connection, QueueHandle,
};
use wayland_protocols::wp::input_method::zv2::client::{
    zwp_input_method_keyboard_grab_v2::ZwpInputMethodKeyboardGrabV2,
    zwp_input_method_manager_v2::ZwpInputMethodManagerV2,
    zwp_input_method_v2::ZwpInputMethodV2,
};
use crate::engine::{KeyBuffer, ModernVietnameseEngine, ViEngine};
use vi_config::InputMethod as ConfigMethod;

/// Trạng thái IME toàn cục
pub struct ImeState {
    // ── Wayland objects ──────────────────────────────────────────────
    pub im_manager:  Option<ZwpInputMethodManagerV2>,
    pub im:          Option<ZwpInputMethodV2>,
    pub kb_grab:     Option<ZwpInputMethodKeyboardGrabV2>,
    pub seat:        Option<WlSeat>,
    // ── Engine state ─────────────────────────────────────────────────
    pub buffer:      KeyBuffer,           // raw_keys: source of truth
    pub engine:      ModernVietnameseEngine,
    pub method:      ConfigMethod,        // English / VNI / Telex / Smart
    // ── Serial tracking (required by protocol) ───────────────────────
    pub serial:      u32,
    pub active:      bool,                // IME activated by compositor?
    // ── Burst commit state (Phase 4) ─────────────────────────────────
    pub burst:       BurstState,
    // ── Game mode (Phase 6) ──────────────────────────────────────────
    pub game_mode:   bool,
}

pub struct BurstState {
    pub pending:     bool,
    pub last_key_at: std::time::Instant,
    pub window:      std::time::Duration, // = 300ms
}

impl Default for BurstState {
    fn default() -> Self {
        Self {
            pending:     false,
            last_key_at: std::time::Instant::now(),
            window:      std::time::Duration::from_millis(300),
        }
    }
}

impl ImeState {
    pub fn new(method: ConfigMethod) -> Self {
        Self {
            im_manager: None, im: None, kb_grab: None, seat: None,
            buffer:     KeyBuffer::new(),
            engine:     ModernVietnameseEngine,
            method,
            serial:     0, active: false,
            burst:      BurstState::default(),
            game_mode:  false,
        }
    }
}
```

---

## 📁 `actions.rs` — Commit Actions

```rust
//! Tất cả Wayland protocol actions: commit_string, virtual_backspace, passthrough.

use wayland_protocols::wp::input_method::zv2::client::zwp_input_method_v2::ZwpInputMethodV2;
use wayland_protocols::wp::virtual_keyboard::zv1::client::{
    zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1,
    zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
};

/// Gửi N virtual backspace để xóa raw_keys đã hiển thị.
/// Cơ chế: gửi key press + release cho keycode 14 (KEY_BACKSPACE)
/// thông qua virtual keyboard, KHÔNG qua input-method protocol.
pub fn send_virtual_backspaces(vk: &ZwpVirtualKeyboardV1, count: usize, time: u32) {
    for i in 0..count {
        vk.key(time + (i as u32 * 2),     14, 1); // press
        vk.key(time + (i as u32 * 2) + 1, 14, 0); // release
    }
}

/// Commit Vietnamese string qua input-method-v2 protocol.
/// Sequence: commit_string(text) → commit(serial)
pub fn commit_vietnamese(im: &ZwpInputMethodV2, text: &str, serial: u32) {
    im.commit_string(text.to_string());
    im.commit(serial);
}

/// Passthrough: forward key event nguyên vẹn (English/Game mode)
pub fn passthrough_key(
    vk: &ZwpVirtualKeyboardV1, keycode: u32, state: u32, time: u32, mods: ModsState,
) {
    if mods.dirty {
        vk.modifiers(mods.depressed, mods.latched, mods.locked, mods.group);
    }
    vk.key(time, keycode, state);
}

/// Full commit sequence: virtual_backspace(n) + commit_string(viet) + commit()
pub fn do_commit(im: &ZwpInputMethodV2, vk: &ZwpVirtualKeyboardV1, state: &mut ImeState, time: u32) {
    let n = state.buffer.raw_len();
    if n == 0 { return; }
    let viet_text = state.buffer.render().to_string();
    send_virtual_backspaces(vk, n, time);
    commit_vietnamese(im, &viet_text, state.serial);
    state.serial = state.serial.wrapping_add(1);
    state.buffer.clear();
    state.burst.pending = false;
}

/// Commit rồi forward boundary char (space, dấu câu)
pub fn do_commit_then_passthrough(
    im: &ZwpInputMethodV2, vk: &ZwpVirtualKeyboardV1, state: &mut ImeState,
    boundary: char, keycode: u32, time: u32, mods: ModsState,
) {
    do_commit(im, vk, state, time);
    passthrough_key(vk, keycode, 1, time + 1, mods);
    passthrough_key(vk, keycode, 0, time + 2, mods);
}

#[derive(Default, Clone, Copy)]
pub struct ModsState {
    pub depressed: u32, pub latched: u32, pub locked: u32, pub group: u32,
    pub dirty: bool,
}
```

---

## 📁 `dispatch.rs` — Key Event Dispatch (Core)

```rust
//! Wayland event dispatch — trái tim của IME.

use crate::actions::{do_commit, do_commit_then_passthrough, passthrough_key, ModsState};
use crate::state::ImeState;
use vi_config::InputMethod as ConfigMethod;

// ─── ZwpInputMethodV2 events ───────────────────────────────────────────
impl Dispatch<ZwpInputMethodV2, ()> for ImeState {
    fn event(state: &mut Self, _proxy: &ZwpInputMethodV2, event: zwp_input_method_v2::Event,
             _: &(), _conn: &Connection, _qh: &QueueHandle<Self>) {
        match event {
            zwp_input_method_v2::Event::Activate => {
                state.active = true;
                state.buffer.clear();
                tracing::debug!("IME activated");
            }
            zwp_input_method_v2::Event::Deactivate => {
                state.active = false;
                if state.buffer.raw_len() > 0 {
                    if let (Some(im), Some(vk)) = (&state.im, &state.vk) {
                        do_commit(im, vk, state, 0);
                    }
                }
                tracing::debug!("IME deactivated");
            }
            zwp_input_method_v2::Event::UnavailableInputMethod => {
                tracing::warn!("Input method unavailable");
            }
            _ => {}
        }
    }
}

// ─── ZwpInputMethodKeyboardGrabV2 events ───────────────────────────────
impl Dispatch<ZwpInputMethodKeyboardGrabV2, ()> for ImeState {
    fn event(state: &mut Self, _proxy: &ZwpInputMethodKeyboardGrabV2,
             event: zwp_input_method_keyboard_grab_v2::Event,
             _: &(), _conn: &Connection, _qh: &QueueHandle<Self>) {
        match event {
            zwp_input_method_keyboard_grab_v2::Event::Keymap { format, fd, size } => {
                state.handle_keymap(format, fd, size);
            }
            zwp_input_method_keyboard_grab_v2::Event::Key { serial, time, key, key_state } => {
                state.serial = serial;
                if key_state == 1 { state.handle_key(key, time); }
            }
            zwp_input_method_keyboard_grab_v2::Event::Modifiers {
                serial, mods_depressed, mods_latched, mods_locked, group } => {
                state.serial = serial;
                state.handle_modifiers(mods_depressed, mods_latched, mods_locked, group);
            }
            zwp_input_method_keyboard_grab_v2::Event::RepeatInfo { rate, delay } => {
                tracing::debug!("Key repeat: rate={rate} delay={delay}ms");
            }
            _ => {}
        }
    }
}

// ─── Key handling ──────────────────────────────────────────────────────
const MOD_CTRL:  u32 = 1 << 2;
const MOD_ALT:   u32 = 1 << 3;
const MOD_SHIFT: u32 = 1 << 0;

impl ImeState {
    fn handle_keymap(&mut self, format: u32, fd: RawFd, size: u32) {
        let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let keymap = unsafe {
            xkb::Keymap::new_from_fd(&ctx, fd, size as usize,
                xkb::KEYMAP_FORMAT_TEXT_V1, xkb::KEYMAP_COMPILE_NO_FLAGS)
        }.expect("Failed to compile keymap");
        self.xkb_state = Some(xkb::State::new(&keymap));
        tracing::info!("Keymap updated");
    }

    fn handle_modifiers(&mut self, depressed: u32, latched: u32, locked: u32, group: u32) {
        if let Some(ref mut xkb) = self.xkb_state {
            xkb.update_mask(depressed, latched, locked, 0, 0, group);
        }
        self.mods = ModsState { depressed, latched, locked, group, dirty: true };
    }

    fn handle_key(&mut self, raw_keycode: u32, time: u32) {
        let xkb_keycode = raw_keycode + 8;
        let keysym = self.xkb_state.as_ref()
            .map(|s| s.key_get_one_sym(xkb_keycode))
            .unwrap_or(xkb::KEY_NoSymbol);
        let ch = xkb::keysym_to_utf8(keysym).and_then(|s| s.chars().next());

        // Game mode / English passthrough
        if self.game_mode || self.method == ConfigMethod::English {
            if let Some(vk) = &self.vk {
                passthrough_key(vk, raw_keycode, 1, time, self.mods);
            }
            return;
        }

        // Special keys
        match keysym {
            xkb::KEY_BackSpace => { self.handle_backspace(raw_keycode, time); return; }
            xkb::KEY_Escape => {
                self.buffer.clear();
                if let Some(vk) = &self.vk { passthrough_key(vk, raw_keycode, 1, time, self.mods); }
                return;
            }
            xkb::KEY_Return | xkb::KEY_KP_Enter => {
                if let (Some(im), Some(vk)) = (&self.im, &self.vk) {
                    do_commit_then_passthrough(im, vk, self, '\n', raw_keycode, time, self.mods);
                }
                return;
            }
            xkb::KEY_Shift_L | xkb::KEY_Shift_R
            | xkb::KEY_Control_L | xkb::KEY_Control_R
            | xkb::KEY_Alt_L | xkb::KEY_Alt_R
            | xkb::KEY_Super_L | xkb::KEY_Super_R => return,
            _ => {}
        }

        // Ctrl/Alt combos: passthrough hotkeys
        if self.mods.depressed & (MOD_CTRL | MOD_ALT) != 0 {
            if self.mods.depressed & MOD_SHIFT != 0 && keysym == xkb::KEY_g {
                self.game_mode = !self.game_mode;
                tracing::info!("Game mode: {}", self.game_mode);
                return;
            }
            if self.buffer.raw_len() > 0 {
                if let (Some(im), Some(vk)) = (&self.im, &self.vk) { do_commit(im, vk, self, time); }
            }
            if let Some(vk) = &self.vk { passthrough_key(vk, raw_keycode, 1, time, self.mods); }
            return;
        }

        // Word boundary
        if let Some(c) = ch {
            if self.buffer.should_commit(c) {
                if let (Some(im), Some(vk)) = (&self.im, &self.vk) {
                    do_commit_then_passthrough(im, vk, self, c, raw_keycode, time, self.mods);
                }
                return;
            }
        }

        // Vietnamese input: buffer + re-parse
        if let Some(c) = ch {
            if c.is_ascii() && !c.is_control() {
                self.buffer.push(c);
                self.check_burst_commit(time);
            } else if let Some(vk) = &self.vk {
                passthrough_key(vk, raw_keycode, 1, time, self.mods);
            }
        }
    }

    fn handle_backspace(&mut self, raw_keycode: u32, time: u32) {
        if self.buffer.raw_len() > 0 {
            self.buffer.backspace();
        } else if let Some(vk) = &self.vk {
            passthrough_key(vk, raw_keycode, 1, time, self.mods);
        }
    }

    fn check_burst_commit(&mut self, _time: u32) {
        let now = std::time::Instant::now();
        let timeout = now.duration_since(self.burst.last_key_at) > self.burst.window;
        if timeout || self.buffer.raw_len() > 8 { self.burst.pending = true; }
        self.burst.last_key_at = now;
    }
}
```

---

## 📁 `runtime.rs` — Wayland Event Loop (Basic)

```rust
//! Wayland event loop — kết nối tất cả lại.

pub fn run_ime_loop(method: ConfigMethod) -> anyhow::Result<()> {
    // 1. Connect Wayland
    let conn = Connection::connect_to_env()?;
    tracing::info!("Connected to Wayland compositor");

    // 2. Registry + globals
    let (globals, mut event_queue) = registry_queue_init::<ImeState>(&conn)?;
    let qh = event_queue.handle();

    // 3. Bind globals
    let im_manager = globals
        .bind::<ZwpInputMethodManagerV2, _, _>(&qh, 1..=1, ())
        .map_err(|_| anyhow::anyhow!(
            "Compositor does not support zwp_input_method_v2. \
             Ensure you are running wlroots (Hyprland/Niri) or KDE Plasma."
        ))?;
    let vk_manager = globals
        .bind::<ZwpVirtualKeyboardManagerV1, _, _>(&qh, 1..=1, ())
        .map_err(|_| anyhow::anyhow!(
            "Compositor does not support zwp_virtual_keyboard_manager_v1."
        ))?;
    let seat = globals
        .bind::<wl_seat::WlSeat, _, _>(&qh, 1..=1, ())
        .expect("No seat found");

    // 4. Create IME + Virtual Keyboard
    let im = im_manager.get_input_method(&seat, &qh, ());
    let vk = vk_manager.create_virtual_keyboard(&seat, &qh, ());

    // 5. Init state
    let mut state = ImeState::new(method);
    state.im_manager = Some(im_manager);
    state.im         = Some(im.clone());
    state.vk         = Some(vk);
    state.seat       = Some(seat);

    // 6. Grab keyboard
    let _grab = im.grab_keyboard(&qh, ());
    state.kb_grab = Some(_grab);
    tracing::info!("IME loop started, method={:?}", method);

    // 7. Main event loop — blocking
    loop {
        event_queue.blocking_dispatch(&mut state)?;
    }
}
```

---

## 📁 `commit.rs` — Commit Strategy

```rust
//! Commit strategies: immediate vs burst.

pub enum CommitStrategy {
    Immediate,
    Burst { window: Duration },
}

impl CommitStrategy {
    pub fn default_burst() -> Self {
        Self::Burst { window: Duration::from_millis(300) }
    }
}

/// Kiểm tra và thực thi burst commit nếu cần.
/// Gọi từ timer thread mỗi 50ms để flush stale buffers.
pub fn flush_stale_burst(
    im: &ZwpInputMethodV2, vk: &ZwpVirtualKeyboardV1, state: &mut ImeState,
) {
    if !state.burst.pending || state.buffer.raw_len() == 0 { return; }
    let elapsed = Instant::now().duration_since(state.burst.last_key_at);
    if elapsed >= state.burst.window {
        tracing::debug!("Burst flush: {} chars after {:?}", state.buffer.raw_len(), elapsed);
        do_commit(im, vk, state, 0);
    }
}
```

---

## 📁 `lib.rs` — Module Exports

```rust
pub mod actions;
pub mod commit;
pub mod dispatch;
pub mod runtime;
pub mod state;
pub mod virtual_keyboard;

pub use runtime::run_ime_loop;
pub use state::ImeState;
```

---

## 📊 Flow: Keystroke → Screen

```
User nhấn phím 'v'
         │
         ▼
ZwpInputMethodKeyboardGrabV2::Event::Key { key=47, time=T }
         │
         ▼
dispatch.rs :: handle_key()
         │
    ┌────┴─────────────────────────────────────────┐
    │ Game mode? ──YES──▶ passthrough_key(vk, 47)  │
    │ English?   ──YES──▶ passthrough_key(vk, 47)  │
    │ Backspace? ──YES──▶ handle_backspace()        │
    │ Ctrl/Alt?  ──YES──▶ flush + passthrough       │
    │ Word bound?──YES──▶ do_commit_then_passthrough│
    └────┬─────────────────────────────────────────┘
         │ Vietnamese input
         ▼
  buffer.push('v')            ← raw_keys: ['v']
  (user tiếp tục: 'i','e','t','j')  → raw_keys: ['v','i','e','t','j']
  User nhấn SPACE (word boundary)
         │
         ▼
  do_commit_then_passthrough(im, vk, state, ' ', ...)
         │
    ┌────┴────────────────────────────────────┐
    │ 1. render() → "việt"                    │
    │ 2. send_virtual_backspaces(vk, 5)       │  ← xóa "vietj"
    │ 3. im.commit_string("việt") + commit()  │  ← "việt" xuất hiện
    │ 4. passthrough_key(vk, SPACE)            │  ← space xuất hiện
    │ 5. buffer.clear()                        │
    └─────────────────────────────────────────┘
```

---

## ⚠️ Compositor Compatibility

| Compositor | Protocol | Status |
|------------|----------|--------|
| wlroots (Hyprland, Niri, Sway) | `zwp_input_method_v2` | ✅ Hoàn hảo |
| KDE Plasma (KWin) | `zwp_input_method_v2` | ✅ Tốt (bản mới) |
| GNOME (Mutter) | ❌ Không support v2 | ⚠️ Cần IBus internal |

---

## ✅ Phase 1-2 Integration Checklist

| Step | File | Status |
|------|------|--------|
| IME global state | `state.rs` | ✅ |
| Commit + VB actions | `actions.rs` | ✅ |
| Key event dispatch | `dispatch.rs` | ✅ |
| Wayland event loop | `runtime.rs` | ✅ |
| Burst commit flush | `commit.rs` | ✅ |
| NFD engine bridge | `vi-engine` | ✅ |
| Game Mode toggle (Ctrl+Shift+G) | `dispatch.rs` | ✅ |

---

# Part A: Virtual Keyboard — XKB Keymap Upload + Key Injection

> **Tại sao cần upload keymap?**
> `zwp_virtual_keyboard_v1` yêu cầu client upload một XKB keymap hợp lệ trước khi
> inject bất kỳ key event nào — compositor cần keymap để translate keycodes thành
> keysyms đúng.
>
> Virtual Backspace architecture: key injection thông qua virtual keyboard object,
> không qua uinput trực tiếp vì đã có `zwp_virtual_keyboard_v1` từ Wayland protocol.

## 📁 `virtual_keyboard.rs`

```rust
//! Virtual keyboard: XKB keymap upload + key/modifier injection.
//!
//! zwp_virtual_keyboard_v1 cần:
//!   1. keymap() được gọi TRƯỚC mọi key event
//!   2. key() để inject press/release
//!   3. modifiers() để sync modifier state

use std::{
    ffi::CString, fs::File, io::Write,
    os::unix::io::{FromRawFd, IntoRawFd, OwnedFd, RawFd},
};
use wayland_protocols::wp::virtual_keyboard::zv1::client::{
    zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
};
use xkbcommon::xkb;

// ─── KeymapUploader ───────────────────────────────────────────────────

/// Quản lý việc upload XKB keymap tới virtual keyboard object.
/// Protocol yêu cầu keymap phải được truyền qua file descriptor (memfd/tmpfile).
pub struct KeymapUploader {
    uploaded:             bool,
    current_keymap_str:   Option<String>,
}

impl KeymapUploader {
    pub fn new() -> Self { Self { uploaded: false, current_keymap_str: None } }

    /// Upload keymap từ raw fd mà compositor gửi qua keyboard grab event.
    pub fn upload_from_compositor_fd(&mut self, vk: &ZwpVirtualKeyboardV1,
                                      format: u32, fd: RawFd, size: u32) {
        if format != 1 {
            tracing::warn!("Unknown keymap format {format}, skipping upload");
            return;
        }
        vk.keymap(format, unsafe { OwnedFd::from_raw_fd(fd) }, size);
        self.uploaded = true;
        tracing::info!("Keymap uploaded to virtual keyboard ({size} bytes)");
    }

    /// Upload keymap từ XKB keymap object (fallback US QWERTY).
    pub fn upload_from_xkb_keymap(&mut self, vk: &ZwpVirtualKeyboardV1,
                                   keymap: &xkb::Keymap) -> anyhow::Result<()> {
        let keymap_str = keymap.get_as_string(xkb::KEYMAP_FORMAT_TEXT_V1);
        if self.current_keymap_str.as_deref() == Some(&keymap_str) && self.uploaded {
            tracing::debug!("Keymap unchanged, skipping re-upload");
            return Ok(());
        }
        let size = keymap_str.len() + 1;
        let fd = create_memfd("vi-im-keymap", &keymap_str)?;
        vk.keymap(1, unsafe { OwnedFd::from_raw_fd(fd) }, size as u32);
        self.current_keymap_str = Some(keymap_str);
        self.uploaded = true;
        tracing::info!("XKB keymap uploaded ({size} bytes)");
        Ok(())
    }

    pub fn is_uploaded(&self) -> bool { self.uploaded }
    pub fn invalidate(&mut self) { self.uploaded = false; }
}

// ─── VirtualKeyboard wrapper ──────────────────────────────────────────

/// High-level wrapper cho zwp_virtual_keyboard_v1.
pub struct VirtualKeyboard {
    pub vk:      ZwpVirtualKeyboardV1,
    uploader:    KeymapUploader,
    mods:        ModsSnapshot,
}

#[derive(Default, Clone, Copy, PartialEq)]
pub struct ModsSnapshot {
    pub depressed: u32, pub latched: u32, pub locked: u32, pub group: u32,
}

impl VirtualKeyboard {
    pub fn new(vk: ZwpVirtualKeyboardV1) -> Self {
        Self { vk, uploader: KeymapUploader::new(), mods: ModsSnapshot::default() }
    }

    pub fn handle_compositor_keymap(&mut self, format: u32, fd: RawFd, size: u32) {
        self.uploader.upload_from_compositor_fd(&self.vk, format, fd, size);
    }

    pub fn upload_fallback_keymap(&mut self) -> anyhow::Result<()> {
        let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let keymap = xkb::Keymap::new_from_names(
            &ctx, "", "", "us", "", None, xkb::KEYMAP_COMPILE_NO_FLAGS,
        ).ok_or_else(|| anyhow::anyhow!("Failed to create fallback keymap"))?;
        self.uploader.upload_from_xkb_keymap(&self.vk, &keymap)
    }

    pub fn inject_key(&self, keycode: u32, state: u32, time: u32) {
        debug_assert!(self.uploader.is_uploaded(), "Must upload keymap first!");
        self.vk.key(time, keycode, state);
    }

    pub fn inject_key_tap(&self, keycode: u32, time: u32) {
        self.inject_key(keycode, 1, time);
        self.inject_key(keycode, 0, time + 1);
    }

    /// Inject N backspace events (KEY_BACKSPACE = 14). 2ms spacing.
    pub fn inject_backspaces(&self, count: usize, base_time: u32) {
        for i in 0..count {
            let t = base_time + (i as u32 * 2);
            self.inject_key(KEY_BACKSPACE, 1, t);
            self.inject_key(KEY_BACKSPACE, 0, t + 1);
        }
        tracing::debug!("Injected {count} backspaces at t={base_time}");
    }

    pub fn passthrough(&mut self, keycode: u32, state: u32, time: u32, mods: ModsSnapshot) {
        self.sync_mods(mods);
        self.inject_key(keycode, state, time);
    }

    pub fn sync_mods(&mut self, new_mods: ModsSnapshot) {
        if self.mods != new_mods {
            self.vk.modifiers(new_mods.depressed, new_mods.latched, new_mods.locked, new_mods.group);
            self.mods = new_mods;
        }
    }

    pub fn clear_mods(&mut self) { self.sync_mods(ModsSnapshot::default()); }
}

// ─── Linux evdev keycodes ─────────────────────────────────────────────

pub const KEY_BACKSPACE: u32 = 14;
pub const KEY_TAB:       u32 = 15;
pub const KEY_ENTER:     u32 = 28;
pub const KEY_ESCAPE:    u32 = 1;
pub const KEY_SPACE:     u32 = 57;
pub const KEY_DELETE:    u32 = 111;

// ─── memfd helper ─────────────────────────────────────────────────────

fn create_memfd(name: &str, content: &str) -> anyhow::Result<RawFd> {
    use std::io::Seek;
    let c_name = CString::new(name)?;
    let fd = unsafe { libc::memfd_create(c_name.as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 {
        return Err(anyhow::anyhow!("memfd_create failed: {}", std::io::Error::last_os_error()));
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    file.write_all(content.as_bytes())?;
    file.write_all(b"\0")?;
    file.seek(std::io::SeekFrom::Start(0))?;
    Ok(file.into_raw_fd())
}
```

---

# Part B: Burst Commit Timer

> **Thiết kế cross-thread:** Burst commit cần window 300ms — timer chạy trên thread
> riêng, thông báo cho Wayland event loop qua channel khi cần flush.

### Thread Model

```
┌─────────────────────┐     channel      ┌──────────────────┐
│  Wayland event loop │ ◄─── FlushCmd ─── │  Tokio timer task│
│  (sync, main thread)│                   │  (async thread)  │
└─────────────────────┘                   └──────────────────┘
        │ push_key()                               │
        ▼                                    reset_deadline()
  BurstTimer::arm()  ──────────────────────────────┘
```

## 📁 `burst.rs` — BurstTimer (tokio) + BurstTimerSync (std)

```rust
//! Burst commit timer — ibus-style optimization.
//! Gom các pure-append keystrokes trong window 300ms thành single commit.

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{sync::mpsc, time::sleep};

// ─── Public API types ─────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum BurstCmd { Flush, Shutdown }

#[derive(Debug)]
pub struct BurstShared {
    pub last_key_at:  Instant,
    pub window:       Duration,
    pub has_pending:  bool,
    pub armed:        bool,
}

impl Default for BurstShared {
    fn default() -> Self {
        Self { last_key_at: Instant::now(), window: Duration::from_millis(300),
               has_pending: false, armed: false }
    }
}

// ─── BurstTimer (tokio) ──────────────────────────────────────────────

pub struct BurstTimer {
    shared:        Arc<Mutex<BurstShared>>,
    arm_tx:        mpsc::UnboundedSender<ArmSignal>,
    pub flush_rx:  mpsc::Receiver<BurstCmd>,
}

enum ArmSignal { KeyPressed, Stop }

impl BurstTimer {
    pub fn new(window: Duration) -> (Self, tokio::task::JoinHandle<()>) {
        let shared = Arc::new(Mutex::new(BurstShared { window, ..Default::default() }));
        let (arm_tx, arm_rx)     = mpsc::unbounded_channel::<ArmSignal>();
        let (flush_tx, flush_rx) = mpsc::channel::<BurstCmd>(4);
        let shared_clone = Arc::clone(&shared);
        let handle = tokio::spawn(burst_timer_task(shared_clone, arm_rx, flush_tx));
        (Self { shared, arm_tx, flush_rx }, handle)
    }

    pub fn on_key_pressed(&mut self) {
        { let mut s = self.shared.lock().unwrap();
          s.last_key_at = Instant::now(); s.has_pending = true; s.armed = true; }
        let _ = self.arm_tx.send(ArmSignal::KeyPressed);
    }

    pub fn on_flushed(&mut self) {
        let mut s = self.shared.lock().unwrap();
        s.has_pending = false; s.armed = false;
    }

    pub fn try_recv_flush(&mut self) -> bool {
        matches!(self.flush_rx.try_recv(), Ok(BurstCmd::Flush))
    }

    pub fn shutdown(&self) { let _ = self.arm_tx.send(ArmSignal::Stop); }
}

/// Debounce pattern: sleep(window), reset on new key, flush on expiry.
async fn burst_timer_task(
    shared: Arc<Mutex<BurstShared>>,
    mut arm_rx: mpsc::UnboundedReceiver<ArmSignal>,
    flush_tx: mpsc::Sender<BurstCmd>,
) {
    tracing::debug!("Burst timer task started");
    loop {
        // Phase 1: Idle — chờ keystroke đầu tiên
        let window = loop {
            match arm_rx.recv().await {
                Some(ArmSignal::KeyPressed) => break shared.lock().unwrap().window,
                Some(ArmSignal::Stop) | None => {
                    tracing::debug!("Burst timer task stopping");
                    let _ = flush_tx.send(BurstCmd::Shutdown).await;
                    return;
                }
            }
        };

        // Phase 2: Armed — debounce loop
        loop {
            tokio::select! {
                _ = sleep(window) => {
                    let has_pending = { shared.lock().unwrap().has_pending };
                    if has_pending {
                        tracing::debug!("Burst window expired ({:?}), sending Flush", window);
                        if flush_tx.send(BurstCmd::Flush).await.is_err() { return; }
                        shared.lock().unwrap().armed = false;
                    }
                    break;
                }
                signal = arm_rx.recv() => {
                    match signal {
                        Some(ArmSignal::KeyPressed) => { tracing::trace!("Burst: reset timer"); continue; }
                        Some(ArmSignal::Stop) | None => {
                            if shared.lock().unwrap().has_pending {
                                let _ = flush_tx.send(BurstCmd::Flush).await;
                            }
                            let _ = flush_tx.send(BurstCmd::Shutdown).await;
                            return;
                        }
                    }
                }
            }
        }
    }
}

// ─── BurstTimerSync (no-tokio fallback) ──────────────────────────────

/// Lightweight fallback dùng std::thread + std::sync::mpsc.
pub struct BurstTimerSync {
    shared:   Arc<Mutex<BurstShared>>,
    arm_tx:   std::sync::mpsc::SyncSender<bool>,
    flush_rx: std::sync::mpsc::Receiver<BurstCmd>,
    _thread:  std::thread::JoinHandle<()>,
}

impl BurstTimerSync {
    pub fn new(window: Duration) -> Self {
        let shared = Arc::new(Mutex::new(BurstShared { window, ..Default::default() }));
        let (arm_tx, arm_rx)     = std::sync::mpsc::sync_channel::<bool>(16);
        let (flush_tx, flush_rx) = std::sync::mpsc::channel::<BurstCmd>();
        let shared_clone = Arc::clone(&shared);
        let thread = std::thread::Builder::new()
            .name("vi-im-burst-timer".into())
            .spawn(move || burst_timer_sync_thread(shared_clone, arm_rx, flush_tx))
            .expect("Failed to spawn burst timer thread");
        Self { shared, arm_tx, flush_rx, _thread: thread }
    }

    pub fn on_key_pressed(&mut self) {
        { let mut s = self.shared.lock().unwrap();
          s.last_key_at = Instant::now(); s.has_pending = true; }
        let _ = self.arm_tx.try_send(true);
    }

    pub fn on_flushed(&mut self) { self.shared.lock().unwrap().has_pending = false; }

    pub fn try_recv_flush(&self) -> bool {
        matches!(self.flush_rx.try_recv(), Ok(BurstCmd::Flush))
    }
}

fn burst_timer_sync_thread(
    shared: Arc<Mutex<BurstShared>>,
    arm_rx: std::sync::mpsc::Receiver<bool>,
    flush_tx: std::sync::mpsc::Sender<BurstCmd>,
) {
    loop {
        if arm_rx.recv().is_err() { break; }
        let window = shared.lock().unwrap().window;
        let mut deadline = Instant::now() + window;
        loop {
            let now = Instant::now();
            if now >= deadline {
                let has_pending = shared.lock().unwrap().has_pending;
                if has_pending { let _ = flush_tx.send(BurstCmd::Flush);
                                 shared.lock().unwrap().has_pending = false; }
                break;
            }
            let remaining = deadline - now;
            match arm_rx.recv_timeout(remaining) {
                Ok(_) => {
                    deadline = Instant::now() + window;
                    shared.lock().unwrap().last_key_at = Instant::now();
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    let has_pending = shared.lock().unwrap().has_pending;
                    if has_pending { let _ = flush_tx.send(BurstCmd::Flush);
                                     shared.lock().unwrap().has_pending = false; }
                    break;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
    }
}
```

---

## 📁 `runtime.rs` — Updated với Burst + Wakeup Pipe

```rust
//! Runtime loop: Wayland event queue + burst timer wakeup.
//! Giải pháp: dùng eventfd (Linux) làm wakeup pipe.

use std::{os::unix::io::AsRawFd, time::Duration};
use crate::{actions::do_commit, burst::BurstTimerSync, state::ImeState,
            virtual_keyboard::VirtualKeyboard};

pub fn run_ime_loop(method: ConfigMethod) -> anyhow::Result<()> {
    let conn = Connection::connect_to_env()?;
    let (globals, mut event_queue) =
        smithay_client_toolkit::reexports::client::globals::registry_queue_init::<ImeState>(&conn)?;
    let qh = event_queue.handle();

    let im_manager = globals.bind::<ZwpInputMethodManagerV2, _, _>(&qh, 1..=1, ())
        .map_err(|_| anyhow::anyhow!("zwp_input_method_v2 not supported"))?;
    let vk_manager = globals.bind::<ZwpVirtualKeyboardManagerV1, _, _>(&qh, 1..=1, ())
        .map_err(|_| anyhow::anyhow!("zwp_virtual_keyboard_manager_v1 not supported"))?;
    let seat = globals.bind::<wl_seat::WlSeat, _, _>(&qh, 7..=8, ())
        .map_err(|_| anyhow::anyhow!("No WlSeat found"))?;

    let im  = im_manager.get_input_method(&seat, &qh, ());
    let vk  = vk_manager.create_virtual_keyboard(&seat, &qh, ());
    let vkw = VirtualKeyboard::new(vk);

    let mut state   = ImeState::new(method, vkw);
    state.im        = Some(im.clone());
    state.seat      = Some(seat);
    let _grab       = im.grab_keyboard(&qh, ());
    state.kb_grab   = Some(_grab);
    state.vk.upload_fallback_keymap()?;

    let mut burst = BurstTimerSync::new(Duration::from_millis(300));
    let wakeup_fd = create_eventfd()?;
    let wakeup_fd_write = wakeup_fd;
    tracing::info!("vi-im event loop starting (method={method:?})");

    loop {
        let wayland_fd = conn.as_raw_fd();
        let ready = poll_fds(&[wayland_fd, wakeup_fd], 50)?;

        if ready.contains(wayland_fd) {
            event_queue.dispatch_pending(&mut state)?;
            conn.flush()?;
        }

        if state.burst_key_pending {
            burst.on_key_pressed();
            state.burst_key_pending = false;
        }

        if burst.try_recv_flush() || ready.contains(wakeup_fd) {
            drain_eventfd(wakeup_fd);
            if state.buffer.raw_len() > 0 {
                if let Some(im) = &state.im {
                    do_commit(im, &mut state.vk, &mut state.buffer, state.serial);
                    state.serial = state.serial.wrapping_add(1);
                    burst.on_flushed();
                }
            }
        }
        event_queue.dispatch_pending(&mut state)?;
    }
}

fn create_eventfd() -> anyhow::Result<RawFd> {
    let fd = unsafe { libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC) };
    if fd < 0 { Err(anyhow::anyhow!("eventfd failed: {}", std::io::Error::last_os_error())) }
    else { Ok(fd) }
}

fn drain_eventfd(fd: RawFd) { let mut buf = [0u8; 8]; unsafe { libc::read(fd, buf.as_mut_ptr() as _, 8) }; }

fn poll_fds(fds: &[RawFd], timeout_ms: i32) -> anyhow::Result<HashSet<RawFd>> {
    let mut pollfds: Vec<libc::pollfd> = fds.iter()
        .map(|&fd| libc::pollfd { fd, events: libc::POLLIN, revents: 0 }).collect();
    let ret = unsafe { libc::poll(pollfds.as_mut_ptr(), pollfds.len() as _, timeout_ms) };
    if ret < 0 {
        let e = std::io::Error::last_os_error();
        if e.kind() == std::io::ErrorKind::Interrupted { return Ok(HashSet::new()); }
        return Err(e.into());
    }
    Ok(pollfds.iter().filter(|p| p.revents & libc::POLLIN != 0).map(|p| p.fd).collect())
}
```

---

## 📊 Full Integration Flow (A + B)

```
                    WAYLAND THREAD                    BURST TIMER THREAD
                    ──────────────                    ──────────────────
User nhấn 'v'
    │
    ▼
dispatch::handle_key('v')
    │
    ├─ buffer.push('v')
    ├─ state.burst_key_pending = true
    │
    ▼                            ──── ArmSignal::KeyPressed ────▶
poll_fds() timeout=50ms                                         │
    │                                                    sleep(300ms)
    ▼                                                           │
burst_key_pending=true                                          │
    └─ burst.on_key_pressed()                                   │
                                                                │
User nhấn 'i','e','t','j'  (tiếp tục)                          │
    │                                                    (reset sleep)
    ├─ buffer: ['v','i','e','t','j']                            │
    │                                                           │
    │        (300ms không có keystroke mới)                     │
    │                                              ◀── BurstCmd::Flush ──
    ▼
try_recv_flush() = true
    │
    ▼
do_commit():
    ├─ render() → "việt"
    ├─ vk.inject_backspaces(5)      ← xóa "vietj"
    ├─ im.commit_string("việt")
    ├─ im.commit(serial)
    └─ buffer.clear()
```

---

## ✅ A + B Summary Checklist

| Component | File / Method | Key Points |
|-----------|---------------|------------|
| Keymap upload từ compositor | `KeymapUploader::upload_from_compositor_fd` | Forward fd nguyên vẹn |
| Fallback keymap (US QWERTY) | `upload_from_xkb_keymap` + `create_memfd` | memfd → null-terminated |
| Key injection | `VirtualKeyboard::inject_key` / `inject_backspaces` | Evdev keycode, 2ms spacing |
| Modifier sync | `VirtualKeyboard::sync_mods` | Diff-based |
| Burst timer (tokio) | `BurstTimer` + `burst_timer_task` | Debounce, 300ms window |
| Burst timer (std) | `BurstTimerSync` | No tokio needed |
| Wakeup pipe | `eventfd` in `runtime.rs` | Cross-thread notify |
| Poll loop | `poll_fds()` | Wayland fd + eventfd, 50ms timeout |

---

# Part C: Tray Icon Integration

> Single binary `vi-im` tích hợp tray icon + settings + daemon vào một executable.

### Menu Layout

```
┌──────────────────────────┐
│  vi-im · Smart · Bật    │  ← status bar (read-only)
├──────────────────────────┤
│  🇬🇧 English              │
│  🇻🇳 VNI  ✓               │
│  🇻🇳 Telex                │
│  🇻🇳 Smart                │
├──────────────────────────┤
│  🎮  Game Mode           │
│  ⚙️ Cấu hình...           │
├──────────────────────────┤
│  ❌ Thoát                 │
└──────────────────────────┘
```

## 📁 `vi-config/src/lib.rs` — Shared Config Types

```rust
//! vi-config: shared configuration types dùng bởi vi-daemon, vi-tray, vi-wayland-im.

use serde::{Deserialize, Serialize};
use std::{fs, path::{Path, PathBuf}, sync::{Arc, RwLock}};

// ─── InputMethod ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum InputMethod {
    English,
    Vni,
    #[default] Telex,
    Smart,
}

impl InputMethod {
    pub fn display_name(&self) -> &'static str {
        match self { Self::English => "English", Self::Vni => "VNI",
                     Self::Telex => "Telex", Self::Smart => "Smart" }
    }
    pub fn flag(&self) -> &'static str {
        match self { Self::English => "🇬🇧", _ => "🇻🇳" }
    }
    pub fn short_label(&self) -> &'static str {
        match self { Self::English => "EN", Self::Vni | Self::Telex => "VN", Self::Smart => "SM" }
    }
    pub fn toggle(self) -> Self { match self { Self::English => Self::Telex, _ => Self::English } }
    pub fn is_vietnamese(&self) -> bool { !matches!(self, Self::English) }
}

impl std::fmt::Display for InputMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

// ─── ViConfig ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ViConfig {
    pub method:            InputMethod,
    pub burst_window_ms:   u64,
    pub autostart:         bool,
    pub game_mode:         bool,
    pub toggle_hotkey:     String,
    pub game_mode_hotkey:  String,
}

impl Default for ViConfig {
    fn default() -> Self {
        Self { method: InputMethod::Telex, burst_window_ms: 300, autostart: true,
               game_mode: false, toggle_hotkey: "Ctrl+Shift+Space".into(),
               game_mode_hotkey: "Ctrl+Shift+G".into() }
    }
}

impl ViConfig {
    pub fn load() -> Self {
        let path = config_path();
        if path.exists() {
            match fs::read_to_string(&path) {
                Ok(s) => toml::from_str(&s).unwrap_or_default(),
                Err(e) => { tracing::warn!("Config read error: {e}, using defaults"); Self::default() }
            }
        } else { Self::default() }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = config_path();
        if let Some(parent) = path.parent() { fs::create_dir_all(parent)?; }
        fs::write(&path, toml::to_string_pretty(self)?)?;
        Ok(())
    }
}

fn config_path() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from("~/.config")).join("vi-im").join("config.toml")
}

pub type SharedConfig = Arc<RwLock<ViConfig>>;

pub fn new_shared_config() -> SharedConfig {
    Arc::new(RwLock::new(ViConfig::load()))
}
```

## 📁 `vi-tray/src/lib.rs` — Tray Icon + Menu

```rust
//! vi-tray: system tray icon + context menu cho vi-im.
//! Thread model: tray_thread (GTK) ──TrayMessage──▶ daemon ◄──TrayUpdate──

use std::sync::{Arc, RwLock};
use tray_icon::{
    menu::{CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    Icon, TrayIcon, TrayIconBuilder, TrayIconEvent,
};
use vi_config::{InputMethod, SharedConfig, ViConfig};

// ─── Messages ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum TrayMessage { SetMethod(InputMethod), ToggleIme, OpenSettings, ToggleGameMode, Quit }

#[derive(Debug, Clone)]
pub enum TrayUpdate { MethodChanged(InputMethod), ActiveChanged(bool), GameModeChanged(bool) }

// ─── TrayApp ─────────────────────────────────────────────────────────

pub struct TrayApp {
    _tray:     TrayIcon,
    menu:      TrayMenu,
    config:    SharedConfig,
    msg_tx:    std::sync::mpsc::SyncSender<TrayMessage>,
}

struct TrayMenu {
    status_item:   MenuItem,   english_item: CheckMenuItem, vni_item: CheckMenuItem,
    telex_item:    CheckMenuItem, smart_item: CheckMenuItem,
    settings_item: MenuItem,   gamemode_item: CheckMenuItem, quit_item: MenuItem,
}

impl TrayApp {
    pub fn new(config: SharedConfig, msg_tx: std::sync::mpsc::SyncSender<TrayMessage>) -> anyhow::Result<Self> {
        let cfg = config.read().unwrap().clone();
        let menu = Menu::new();

        let status_item   = MenuItem::new(status_label(&cfg), false, None);
        let sep1          = PredefinedMenuItem::separator();
        let english_item  = CheckMenuItem::new("🇬🇧  English", true, cfg.method == InputMethod::English, None);
        let vni_item      = CheckMenuItem::new("🇻🇳  VNI", true, cfg.method == InputMethod::Vni, None);
        let telex_item    = CheckMenuItem::new("🇻🇳  Telex", true, cfg.method == InputMethod::Telex, None);
        let smart_item    = CheckMenuItem::new("🇻🇳  Smart", true, cfg.method == InputMethod::Smart, None);
        let sep2          = PredefinedMenuItem::separator();
        let gamemode_item = CheckMenuItem::new("🎮  Game Mode", true, cfg.game_mode, None);
        let settings_item = MenuItem::new("⚙️  Cấu hình...", true, None);
        let sep3          = PredefinedMenuItem::separator();
        let quit_item     = MenuItem::new("❌  Thoát", true, None);

        menu.append_items(&[&status_item, &sep1, &english_item, &vni_item, &telex_item, &smart_item,
                            &sep2, &gamemode_item, &settings_item, &sep3, &quit_item])?;

        let icon = load_tray_icon(&cfg.method, false);
        let tooltip = tooltip_text(&cfg.method, false, cfg.game_mode);
        let tray = TrayIconBuilder::new().with_menu(Box::new(menu)).with_icon(icon)
                                         .with_tooltip(tooltip).build()?;
        tracing::info!("Tray icon created");

        Ok(Self { _tray: tray,
            menu: TrayMenu { status_item, english_item, vni_item, telex_item, smart_item,
                             settings_item, gamemode_item, quit_item },
            config, msg_tx })
    }

    pub fn process_events(&self) {
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if let Some(msg) = self.resolve_menu_event(event.id()) { let _ = self.msg_tx.try_send(msg); }
        }
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click { button: tray_icon::MouseButton::Left, .. } = event {
                let _ = self.msg_tx.try_send(TrayMessage::ToggleIme);
            }
        }
    }

    pub fn apply_update(&mut self, update: TrayUpdate) {
        match update {
            TrayUpdate::MethodChanged(method) => {
                self.update_method_checks(method);
                let cfg = self.config.read().unwrap();
                self.menu.status_item.set_text(status_label(&cfg));
            }
            TrayUpdate::ActiveChanged(active) => { tracing::debug!("Tray: IME active={active}"); }
            TrayUpdate::GameModeChanged(on) => { self.menu.gamemode_item.set_checked(on); }
        }
    }

    fn resolve_menu_event(&self, id: &tray_icon::menu::MenuId) -> Option<TrayMessage> {
        if id == self.menu.english_item.id()  { Some(TrayMessage::SetMethod(InputMethod::English)) }
        else if id == self.menu.vni_item.id()     { Some(TrayMessage::SetMethod(InputMethod::Vni)) }
        else if id == self.menu.telex_item.id()   { Some(TrayMessage::SetMethod(InputMethod::Telex)) }
        else if id == self.menu.smart_item.id()   { Some(TrayMessage::SetMethod(InputMethod::Smart)) }
        else if id == self.menu.settings_item.id(){ Some(TrayMessage::OpenSettings) }
        else if id == self.menu.gamemode_item.id(){ Some(TrayMessage::ToggleGameMode) }
        else if id == self.menu.quit_item.id()    { Some(TrayMessage::Quit) }
        else { None }
    }

    fn update_method_checks(&self, method: InputMethod) {
        self.menu.english_item.set_checked(method == InputMethod::English);
        self.menu.vni_item.set_checked(method == InputMethod::Vni);
        self.menu.telex_item.set_checked(method == InputMethod::Telex);
        self.menu.smart_item.set_checked(method == InputMethod::Smart);
    }
}

fn load_tray_icon(method: &InputMethod, game_mode: bool) -> Icon {
    let bytes: &[u8] = if game_mode { include_bytes!("../icons/vi-im-game.png") }
    else if method.is_vietnamese() { include_bytes!("../icons/vi-im-vn.png") }
    else { include_bytes!("../icons/vi-im-en.png") };
    let img = image::load_from_memory(bytes).expect("Failed to decode icon").to_rgba8();
    let (w, h) = img.dimensions();
    Icon::from_rgba(img.into_raw(), w, h).expect("Invalid icon data")
}

fn status_label(cfg: &ViConfig) -> String {
    let state = if cfg.game_mode { "🎮 Game" } else { "Bật" };
    format!("vi-im · {} · {}", cfg.method.short_label(), state)
}

fn tooltip_text(method: &InputMethod, _active: bool, game_mode: bool) -> String {
    if game_mode { "vi-im [Game Mode]".into() }
    else { format!("vi-im [{}]", method.display_name()) }
}

// ─── Tray thread entrypoint ───────────────────────────────────────────

pub fn spawn_tray_thread(config: SharedConfig) -> (
    std::sync::mpsc::Receiver<TrayMessage>,
    std::sync::mpsc::SyncSender<TrayUpdate>,
    std::thread::JoinHandle<()>,
) {
    let (msg_tx, msg_rx)       = std::sync::mpsc::sync_channel::<TrayMessage>(32);
    let (update_tx, update_rx) = std::sync::mpsc::sync_channel::<TrayUpdate>(32);
    let handle = std::thread::Builder::new().name("vi-im-tray".into())
        .spawn(move || tray_thread_main(config, msg_tx, update_rx))
        .expect("Failed to spawn tray thread");
    (msg_rx, update_tx, handle)
}

fn tray_thread_main(
    config: SharedConfig, msg_tx: std::sync::mpsc::SyncSender<TrayMessage>,
    update_rx: std::sync::mpsc::Receiver<TrayUpdate>,
) {
    #[cfg(target_os = "linux")] { gtk::init().expect("GTK init failed"); }
    let mut app = TrayApp::new(config, msg_tx).expect("Failed to create TrayApp");
    loop {
        #[cfg(target_os = "linux")] while gtk::events_pending() { gtk::main_iteration_do(false); }
        app.process_events();
        while let Ok(update) = update_rx.try_recv() { app.apply_update(update); }
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}
```

## 📁 `vi-daemon/src/events.rs` — DaemonEvent

```rust
//! Unified event enum cho vi-im daemon.

use vi_tray::TrayMessage;

#[derive(Debug)]
pub enum DaemonEvent {
    Tray(TrayMessage),
    ImeActivated,
    ImeDeactivated,
    ConfigChanged,
    Shutdown,
}
```

---

# Part D: Full Main Entry Point

### Wiring Diagram

```
main()
  │
  ├─[1] parse CLI args (--method, --debug, --no-tray)
  ├─[2] init tracing (RUST_LOG)
  ├─[3] load SharedConfig (Arc<RwLock<ViConfig>>)
  ├─[4] single-instance lock
  ├─[5] spawn tray thread
  ├─[6] spawn IME thread
  ├─[7] spawn burst timer thread
  ├─[8] setup signal handler (SIGTERM/SIGINT)
  └─[9] main event router loop
```

## 📁 `vi-im/src/main.rs` — Single Binary Entry Point

```rust
//! vi-im — single binary entry point.
//! Wires: A. vi-wayland-im + B. vi-burst + C. vi-tray + vi-config + vi-engine.
//! Threads: main, tray (GTK), wayland (dispatch), burst (300ms), signal.

#![deny(unsafe_op_in_unsafe_fn)]

use std::{sync::{Arc, RwLock}, time::Duration};
use vi_config::{new_shared_config, InputMethod, SharedConfig, ViConfig};
use vi_tray::{spawn_tray_thread, TrayMessage, TrayUpdate};
use vi_wayland_im::ImeEvent;

mod cli;
mod instance_lock;
mod signal;

use cli::Args;
use instance_lock::InstanceLock;

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    init_tracing(args.debug);
    tracing::info!("vi-im {} starting (pid={})", env!("CARGO_PKG_VERSION"), std::process::id());

    let config: SharedConfig = new_shared_config();
    if let Some(method) = args.method {
        config.write().unwrap().method = method;
        tracing::info!("Method overridden by CLI: {:?}", method);
    }

    let _lock = match InstanceLock::acquire() {
        Ok(lock) => lock,
        Err(_) => {
            eprintln!("vi-im: already running. Use `vi-im --kill` to stop it first.");
            std::process::exit(1);
        }
    };

    let (tray_msg_rx, tray_update_tx, _tray_handle) = if args.no_tray {
        spawn_null_tray()
    } else {
        vi_tray::spawn_tray_thread(Arc::clone(&config))
    };

    let (ime_method_tx, ime_method_rx)  = std::sync::mpsc::sync_channel::<InputMethod>(8);
    let (ime_event_tx, ime_event_rx)    = std::sync::mpsc::sync_channel::<ImeEvent>(32);
    let (burst_flush_tx, burst_flush_rx) = std::sync::mpsc::sync_channel::<()>(4);

    {
        let config   = Arc::clone(&config);
        let event_tx = ime_event_tx.clone();
        let burst_tx = burst_flush_tx.clone();
        std::thread::Builder::new().name("vi-im-wayland".into())
            .stack_size(4 * 1024 * 1024)
            .spawn(move || {
                let method = config.read().unwrap().method;
                if let Err(e) = vi_wayland_im::run_ime_loop(method, ime_method_rx, event_tx, burst_tx) {
                    tracing::error!("IME thread crashed: {e:#}");
                    std::process::exit(2);
                }
            })?;
    }
    tracing::info!("Wayland IME thread spawned");

    let (shutdown_tx, shutdown_rx) = std::sync::mpsc::sync_channel::<()>(1);
    signal::setup(shutdown_tx)?;
    tracing::info!("Signal handler registered");

    tracing::info!("Event router running");
    run_event_router(config, tray_msg_rx, tray_update_tx, ime_method_tx,
                     ime_event_rx, burst_flush_rx, shutdown_rx)?;

    tracing::info!("vi-im exiting cleanly");
    Ok(())
}

// ─── Event Router ─────────────────────────────────────────────────────

fn run_event_router(
    config: SharedConfig, tray_msg_rx: std::sync::mpsc::Receiver<TrayMessage>,
    tray_update_tx: std::sync::mpsc::SyncSender<TrayUpdate>,
    ime_method_tx: std::sync::mpsc::SyncSender<InputMethod>,
    ime_event_rx: std::sync::mpsc::Receiver<ImeEvent>,
    burst_flush_rx: std::sync::mpsc::Receiver<()>,
    shutdown_rx: std::sync::mpsc::Receiver<()>,
) -> anyhow::Result<()> {
    let mut router = EventRouter { config, tray_update_tx, ime_method_tx };

    loop {
        if shutdown_rx.try_recv().is_ok() {
            tracing::info!("Shutdown signal — saving config and exiting");
            router.save_config()?;
            break;
        }

        loop {
            match tray_msg_rx.try_recv() {
                Ok(msg) => { if let Err(e) = router.handle_tray(msg) { tracing::error!("Tray error: {e:#}"); } }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
            }
        }

        loop {
            match ime_event_rx.try_recv() {
                Ok(event) => router.handle_ime_event(event),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return Err(anyhow::anyhow!("IME thread died"));
                }
            }
        }

        while burst_flush_rx.try_recv().is_ok() {
            tracing::debug!("Burst flush notification");
        }

        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

struct EventRouter {
    config:         SharedConfig,
    tray_update_tx: std::sync::mpsc::SyncSender<TrayUpdate>,
    ime_method_tx:  std::sync::mpsc::SyncSender<InputMethod>,
}

impl EventRouter {
    fn handle_tray(&mut self, msg: TrayMessage) -> anyhow::Result<()> {
        match msg {
            TrayMessage::SetMethod(method) => self.set_method(method)?,
            TrayMessage::ToggleIme => {
                let new = { let mut c = self.config.write().unwrap(); c.method = c.method.toggle(); c.method };
                self.notify_ime_method(new);
                self.notify_tray(TrayUpdate::MethodChanged(new));
                self.save_config()?;
            }
            TrayMessage::ToggleGameMode => {
                let new = { let mut c = self.config.write().unwrap(); c.game_mode = !c.game_mode; c.game_mode };
                self.notify_tray(TrayUpdate::GameModeChanged(new));
                self.save_config()?;
            }
            TrayMessage::OpenSettings => self.open_config_editor()?,
            TrayMessage::Quit => {
                self.save_config()?;
                unsafe { libc::kill(libc::getpid(), libc::SIGTERM) };
            }
        }
        Ok(())
    }

    fn handle_ime_event(&mut self, event: ImeEvent) {
        match event {
            ImeEvent::Activated => self.notify_tray(TrayUpdate::ActiveChanged(true)),
            ImeEvent::Deactivated => self.notify_tray(TrayUpdate::ActiveChanged(false)),
            ImeEvent::Committed { .. } => {},
            ImeEvent::GameModeChanged(active) => {
                { self.config.write().unwrap().game_mode = active; }
                self.notify_tray(TrayUpdate::GameModeChanged(active));
                let _ = self.save_config();
            }
        }
    }

    fn set_method(&mut self, method: InputMethod) -> anyhow::Result<()> {
        { self.config.write().unwrap().method = method; }
        self.notify_ime_method(method);
        self.notify_tray(TrayUpdate::MethodChanged(method));
        self.save_config()
    }

    fn notify_ime_method(&self, method: InputMethod) { let _ = self.ime_method_tx.try_send(method); }
    fn notify_tray(&self, update: TrayUpdate) { let _ = self.tray_update_tx.try_send(update); }

    fn save_config(&self) -> anyhow::Result<()> {
        self.config.read().unwrap().clone().save()
    }

    fn open_config_editor(&self) -> anyhow::Result<()> {
        let cfg_path = vi_config::config_path();
        self.config.read().unwrap().save()?;
        let editor = std::env::var("EDITOR").or_else(|_| std::env::var("VISUAL"))
                                               .unwrap_or_else(|_| "xdg-open".into());
        std::process::Command::new(&editor).arg(&cfg_path).spawn()?;
        Ok(())
    }
}

fn spawn_null_tray() -> (
    std::sync::mpsc::Receiver<TrayMessage>,
    std::sync::mpsc::SyncSender<TrayUpdate>,
    std::thread::JoinHandle<()>,
) {
    let (_tx, rx)  = std::sync::mpsc::sync_channel::<TrayMessage>(1);
    let (tx2, _rx) = std::sync::mpsc::sync_channel::<TrayUpdate>(1);
    let handle = std::thread::spawn(|| {});
    (rx, tx2, handle)
}

fn init_tracing(debug: bool) {
    use tracing_subscriber::{EnvFilter, fmt, prelude::*};
    let default_level = if debug { "vi_im=debug,vi_wayland_im=debug" }
                        else      { "vi_im=info,vi_wayland_im=warn"   };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));
    tracing_subscriber::registry()
        .with(fmt::layer().with_target(true).compact())
        .with(filter).init();
}
```

---

## 📁 `vi-im/src/cli.rs` — CLI Args

```rust
//! CLI argument parser (no clap dependency — manual parse).

use vi_config::InputMethod;

#[derive(Debug, Default)]
pub struct Args {
    pub method:   Option<InputMethod>,
    pub debug:    bool,
    pub no_tray:  bool,
    pub kill:     bool,
    pub version:  bool,
}

impl Args {
    pub fn parse() -> Self {
        let mut args = Self::default();
        let argv: Vec<String> = std::env::args().skip(1).collect();
        let mut i = 0;
        while i < argv.len() {
            match argv[i].as_str() {
                "--debug" | "-d"   => args.debug   = true,
                "--no-tray" | "-n" => args.no_tray = true,
                "--kill" | "-k"    => args.kill     = true,
                "--version" | "-v" => args.version  = true,
                "--method" | "-m"  => {
                    i += 1;
                    if let Some(val) = argv.get(i) {
                        args.method = parse_method(val);
                        if args.method.is_none() {
                            eprintln!("vi-im: unknown method '{}'. Use: en, vni, telex, smart", val);
                            std::process::exit(1);
                        }
                    }
                }
                "--help" | "-h" => { print_help(); std::process::exit(0); }
                unknown => {
                    eprintln!("vi-im: unknown argument '{unknown}'");
                    std::process::exit(1);
                }
            }
            i += 1;
        }
        if args.version { println!("vi-im {}", env!("CARGO_PKG_VERSION")); std::process::exit(0); }
        if args.kill    { kill_existing(); std::process::exit(0); }
        args
    }
}

fn parse_method(s: &str) -> Option<InputMethod> {
    match s.to_lowercase().as_str() {
        "en" | "english" => Some(InputMethod::English),
        "vni"            => Some(InputMethod::Vni),
        "telex"          => Some(InputMethod::Telex),
        "smart"          => Some(InputMethod::Smart),
        _                => None,
    }
}

fn kill_existing() {
    use std::fs;
    let lock_path = crate::instance_lock::InstanceLock::path();
    if let Ok(pid_str) = fs::read_to_string(&lock_path) {
        if let Ok(pid) = pid_str.trim().parse::<i32>() {
            unsafe { libc::kill(pid, libc::SIGTERM) };
            println!("vi-im: sent SIGTERM to pid {pid}");
        }
    } else { eprintln!("vi-im: no running instance found"); }
}

fn print_help() {
    println!(r#"vi-im {} — Vietnamese Wayland IME

USAGE:    vi-im [OPTIONS]

OPTIONS:
    -m, --method <METHOD>   Input method: en, vni, telex, smart
    -d, --debug             Enable debug logging
    -n, --no-tray           Run without tray icon (headless)
    -k, --kill              Kill running vi-im instance
    -v, --version           Print version
    -h, --help              Print this help

ENVIRONMENT:
    RUST_LOG    Override log level (e.g. RUST_LOG=debug)
    EDITOR      Editor for --settings (default: xdg-open)

CONFIG:    ~/.config/vi-im/config.toml"#, env!("CARGO_PKG_VERSION"));
}
```

## 📁 `vi-im/src/instance_lock.rs` — Single Instance Lock

```rust
//! Single-instance lock: PID lockfile at $XDG_RUNTIME_DIR/vi-im.lock

use std::{fs, io::Write, os::unix::fs::OpenOptionsExt, path::PathBuf};

pub struct InstanceLock { path: PathBuf }

impl InstanceLock {
    pub fn path() -> PathBuf {
        let dir = std::env::var("XDG_RUNTIME_DIR")
            .unwrap_or_else(|_| format!("/tmp/vi-im-{}", unsafe { libc::getuid() }));
        PathBuf::from(dir).join("vi-im.lock")
    }

    pub fn acquire() -> anyhow::Result<Self> {
        let path = Self::path();
        if path.exists() {
            if let Ok(pid_str) = fs::read_to_string(&path) {
                if let Ok(pid) = pid_str.trim().parse::<i32>() {
                    if unsafe { libc::kill(pid, 0) } == 0 {
                        return Err(anyhow::anyhow!("Process {pid} is already running"));
                    }
                }
            }
            let _ = fs::remove_file(&path);
        }
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path)?;
        write!(file, "{}", std::process::id())?;
        Ok(Self { path })
    }
}

impl Drop for InstanceLock {
    fn drop(&mut self) { let _ = fs::remove_file(&self.path); }
}
```

## 📁 `vi-im/src/signal.rs` — Signal Handler

```rust
//! SIGTERM + SIGINT handler.

use std::sync::mpsc::SyncSender;

pub fn setup(shutdown_tx: SyncSender<()>) -> anyhow::Result<()> {
    use signal_hook::{consts::{SIGINT, SIGTERM}, iterator::Signals};
    let mut signals = Signals::new([SIGTERM, SIGINT])?;
    std::thread::Builder::new().name("vi-im-signal".into()).spawn(move || {
        for sig in signals.forever() {
            tracing::info!("Signal {sig} received → shutdown");
            let _ = shutdown_tx.try_send(());
            break;
        }
    })?;
    Ok(())
}
```

## 📁 `vi-wayland-im/src/events.rs` — ImeEvent

```rust
//! Events từ IME thread → main daemon thread.

#[derive(Debug, Clone)]
pub enum ImeEvent {
    Activated,
    Deactivated,
    Committed { text: String, raw_len: usize },
    GameModeChanged(bool),
}
```

## 📁 `vi-wayland-im/src/runtime.rs` — Hot-swap Method

```rust
/// IME loop với hot-swap support: thay đổi input method không cần restart.
pub fn run_ime_loop_hotswap(
    initial_method: InputMethod,
    method_rx: std::sync::mpsc::Receiver<InputMethod>,
) -> anyhow::Result<()> {
    // ... (setup như run_ime_loop) ...
    loop {
        let ready = poll_fds(&[wayland_fd, wakeup_fd], 50)?;

        // Hot-swap: check method change
        while let Ok(new_method) = method_rx.try_recv() {
            tracing::info!("Hot-swap method: {:?}", new_method);
            if state.buffer.raw_len() > 0 {
                if let Some(im) = &state.im {
                    do_commit(im, &mut state.vk, &mut state.buffer, state.serial);
                    state.serial = state.serial.wrapping_add(1);
                }
            }
            state.method = new_method;
        }
        // ... (rest of event loop) ...
    }
}
```

---

## 📊 Full Thread Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    vi-im process                         │
│                                                         │
│  ┌──────────────┐   TrayMessage    ┌─────────────────┐  │
│  │  tray thread │ ──────────────▶  │   main thread   │  │
│  │  (GTK loop)  │ ◀──────────────  │  (event router) │  │
│  └──────────────┘   TrayUpdate     └────────┬────────┘  │
│                                             │            │
│                                     InputMethod          │
│                                    (hot-swap channel)    │
│                                             │            │
│                                    ┌────────▼────────┐   │
│                                    │   IME thread    │   │
│                                    │ (Wayland loop)  │   │
│                                    │  + burst timer  │   │
│                                    └─────────────────┘   │
│                                                         │
│  SharedConfig (Arc<RwLock<ViConfig>>) ←─ tất cả threads  │
└─────────────────────────────────────────────────────────┘
```

---

## 📊 Complete Call Graph

```
main()
  │
  ├── Args::parse()           ← cli.rs: --method, --debug, --no-tray, --kill
  ├── init_tracing(debug)
  ├── new_shared_config()      ← load ~/.config/vi-im/config.toml
  │     └── Arc<RwLock<ViConfig>>
  ├── InstanceLock::acquire()  ← $XDG_RUNTIME_DIR/vi-im.lock
  │
  ├── vi_tray::spawn_tray_thread()   ← GTK loop thread
  │     ├── TrayApp::new() → Menu + Icon
  │     └── returns (tray_msg_rx, tray_update_tx, handle)
  │
  ├── vi_wayland_im::run_ime_loop()  ← Wayland thread
  │     ├── Connection + Globals + Seat
  │     ├── VirtualKeyboard::upload_fallback_keymap()
  │     ├── BurstTimerSync::new(300ms)
  │     └── poll_fds() loop
  │           ├── handle_key()        ← dispatch.rs
  │           │     ├── Game mode?     → passthrough_key()
  │           │     ├── English?       → passthrough_key()
  │           │     ├── Backspace?     → handle_backspace()
  │           │     ├── Word boundary? → do_commit_then_passthrough()
  │           │     └── VI input       → buffer.push() + burst.on_key_pressed()
  │           ├── hot-swap method_rx   ← từ daemon
  │           └── burst flush check    → do_commit()
  │
  ├── signal::setup()          ← SIGTERM/SIGINT thread
  │
  └── run_event_router()       ← main thread loop (50ms)
        ├── tray_msg_rx → handle_tray()
        │     ├── SetMethod    → config + ime_method_tx + tray_update_tx
        │     ├── ToggleIme    → config.method.toggle() + notify
        │     ├── ToggleGameMode → config + notify
        │     ├── OpenSettings → $EDITOR config.toml
        │     └── Quit         → save_config() + SIGTERM self
        ├── ime_event_rx → handle_ime_event()
        │     ├── Activated        → TrayUpdate::ActiveChanged(true)
        │     ├── Deactivated      → TrayUpdate::ActiveChanged(false)
        │     └── GameModeChanged  → config + TrayUpdate
        ├── burst_flush_rx → (future: UI animation)
        └── shutdown_rx → save_config() + break
```

---

## 📦 Workspace Configuration

### Root `Cargo.toml`

```toml
[workspace]
members = [
    "crates/vi-engine",
    "crates/vi-config",
    "crates/vi-tray",
    "crates/vi-wayland-im",
    "crates/vi-im",
]
resolver = "2"

[workspace.dependencies]
anyhow   = "1"
tracing  = "0.1"
serde    = { version = "1", features = ["derive"] }
toml     = "0.8"
libc     = "0.2"
tokio    = { version = "1", features = ["rt", "time", "sync"] }
```

### `crates/vi-im/Cargo.toml`

```toml
[package]
name = "vi-im"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "vi-im"
path = "src/main.rs"

[dependencies]
vi-config          = { path = "../vi-config" }
vi-tray            = { path = "../vi-tray" }
vi-wayland-im      = { path = "../vi-wayland-im" }
vi-engine          = { path = "../vi-engine" }
signal-hook        = "0.3"
libc               = "1"
anyhow             = { workspace = true }
tracing            = { workspace = true }
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

### `crates/vi-tray/Cargo.toml`

```toml
[package]
name = "vi-tray"
version = "0.1.0"
edition = "2021"

[dependencies]
tray-icon = "0.24"
image     = "0.25"
gtk       = { version = "0.18", optional = true }
vi-config = { path = "../vi-config" }
serde     = { workspace = true }
anyhow    = { workspace = true }
tracing   = { workspace = true }

[features]
default      = ["gtk-backend"]
gtk-backend  = ["gtk", "tray-icon/gtk"]
```

### Icon Files

```
crates/vi-tray/icons/
├── vi-im-vn.png     # 🟢 32×32 xanh lá  — Vietnamese active
├── vi-im-en.png     # ⚫ 32×32 xám       — English passthrough
└── vi-im-game.png   # 🔴 32×32 đỏ        — Game mode
```

---

## 📁 `deploy/compile.sh` — Build Script

```bash
#!/usr/bin/env bash
# vi-im build + install script
set -euo pipefail

BINARY="vi-im"
INSTALL_DIR="${HOME}/.local/bin"
CONFIG_DIR="${HOME}/.config/vi-im"

echo "🔨 Building vi-im..."
cargo build --release --package vi-im "$@"

echo "📦 Installing to ${INSTALL_DIR}/${BINARY}..."
install -Dm755 "target/release/${BINARY}" "${INSTALL_DIR}/${BINARY}"

echo "📁 Ensuring config dir exists..."
mkdir -p "${CONFIG_DIR}"

if [[ ! -f "${CONFIG_DIR}/config.toml" ]]; then
    echo "📝 Writing default config..."
    cat > "${CONFIG_DIR}/config.toml" << 'EOF'
method = "telex"
burst_window_ms = 300
autostart = true
game_mode = false
toggle_hotkey = "Ctrl+Shift+Space"
game_mode_hotkey = "Ctrl+Shift+G"
EOF
fi

echo "✅ vi-im installed: ${INSTALL_DIR}/${BINARY}"
echo "   Run: vi-im --help"
```

---

## ✅ Full Phase Completion Status

| Phase | Nội dung | Files | Status |
|-------|----------|-------|--------|
| 1 | Unified binary | `vi-im/src/main.rs`, workspace `Cargo.toml` | ✅ |
| 1b | Dedup config types | `vi-config/src/lib.rs` | ✅ |
| 2 | Smart IME + NFD engine | `vi-engine/src/engine/` | ✅ 133 tests |
| 3 | Tray-only config (no QML) | `vi-tray/src/lib.rs` | ✅ |
| 4 | Burst commit 300ms | `vi-wayland-im/src/burst.rs` | ✅ |
| 5 | Tests + AGENTS.md | — | 🔜 next |
| 6 | Game Mode (Ctrl+Shift+G) | `vi-wayland-im/src/dispatch.rs` | ✅ |

---

# Part E: Game Mode Auto-Detection

> **Trigger:** Khi Steam hoặc game process được detect là đang active (focus),
> tự động chuyển sang Game Mode, gửi notification, và hiển thị tray icon đỏ.

### Detection Strategy

```
Compositor focus change (zwlr-foreign-toplevel)
           │
           ▼
  vi-compositor-ipc :: FocusEvent { app_id, title }
           │
           ▼
  ┌──────────────────────────────────────────────┐
  │  GameDetector::check(app_id, title)           │
  │                                              │
  │  1. Match app_id trong GAME_APPS table      │
  │  2. Match title pattern (regex)              │
  │  3. Check /proc/PID/environ for Steam env   │
  └──────────────────┬───────────────────────────┘
                     │
           ┌─────────┴─────────┐
           │ is_game = true    │ is_game = false
           ▼                   ▼
  Auto-enable game mode   Restore previous mode
  Notify user             Notify user
```

## 📁 `vi-daemon/src/game_detector.rs` — Game Process Detection

```rust
//! Auto-detect gaming sessions from process/app metadata.
//!
//! Khi phát hiện game đang focus:
//!   1. Bật Game Mode (bypass IME, passthrough keys)
//!   2. Gửi notification qua D-Bus (notify-send)
//!   3. Update tray icon → đỏ
//!   4. Khi game mất focus → restore mode cũ

use std::collections::HashSet;

// ─── Known game app IDs ──────────────────────────────────────────────

/// App IDs của các game launcher / storefronts
const GAME_LAUNCHERS: &[&str] = &[
    "steam",           // Valve Steam
    "steamwebhelper",  // Steam web helper
    "lutris",          // Lutris game manager
    "heroic",          // Heroic Games Launcher
    "bottles",         // Bottles (Wine prefix manager)
    "legendary",       // Legendary (Epic Games CLI)
    "itch",            // itch.io launcher
    "minecraft-launcher",
    "com.epicgames.launcher",
];

/// App IDs của các game engine / runtime
const GAME_ENGINES: &[&str] = &[
    "wine", "wine64", "wineserver",
    "proton", "proton-",         // Valve Proton
    "dxvk", "vkd3d",
    "gamescope",                 // Valve Gamescope
    "mangohud", "mangoapp",      // MangoHud overlay
];

/// Các game phổ biến (native Linux) — pattern match cả app_id và title
const KNOWN_GAMES: &[&str] = &[
    // Valve
    "cs2", "csgo", "dota2", "hl2_linux", "portal2",
    "tf2", "left4dead2",
    // FPS
    "doom", "quake", "wolfenstein",
    // Open world
    "gta5", "rdr2", "cyberpunk2077", "witcher3",
    "skyrim", "fallout4",
    // MOBA / Strategy
    "leagueoflegends", "lol",
    // Indie
    "terraria", "stardew", "factorio", "rimworld",
    "hollow_knight", "celeste", "deadcells",
    // Emulators
    "retroarch", "cemu", "rpcs3", "pcsx2", "dolphin-emu",
    "yuzu", "ryujinx",
];

// ─── GameDetector ─────────────────────────────────────────────────────

pub struct GameDetector {
    /// Game mode đang được auto-bật (không phải user bật thủ công)
    auto_enabled:  bool,
    /// Mode trước khi auto-enable game mode (để restore)
    previous_mode: Option<InputMethod>,
    /// Đã notify lần này chưa? (tránh spam)
    notified_this_session: bool,
}

impl GameDetector {
    pub fn new() -> Self {
        Self { auto_enabled: false, previous_mode: None, notified_this_session: false }
    }

    /// Check focus event — trả về Some(GameModeAction) nếu cần thay đổi
    pub fn check_focus(
        &mut self,
        app_id:  &str,
        title:   &str,
        pid:     Option<u32>,
        current_mode: InputMethod,
        current_game_mode: bool,
    ) -> Option<GameModeAction> {
        let is_game = Self::is_game_app(app_id, title, pid);

        if is_game && !current_game_mode {
            // Game detected → enable game mode
            self.auto_enabled = true;
            self.previous_mode = Some(current_mode);
            let notify = !self.notified_this_session;
            self.notified_this_session = true;
            Some(GameModeAction::Enable {
                app_name: detect_game_name(app_id, title),
                notify,
            })
        } else if !is_game && self.auto_enabled {
            // Game exited → restore
            self.auto_enabled = false;
            self.notified_this_session = false;
            let restore_mode = self.previous_mode.take();
            Some(GameModeAction::Disable {
                restore_mode,
                notify: true,
            })
        } else {
            None
        }
    }

    /// Heuristic: app_id hoặc title match game patterns
    fn is_game_app(app_id: &str, title: &str, pid: Option<u32>) -> bool {
        let app_lower = app_id.to_lowercase();
        let title_lower = title.to_lowercase();

        // Check game launchers
        if GAME_LAUNCHERS.iter().any(|&l| app_lower.contains(l)) {
            return true;
        }

        // Check game engines (cần title confirm — tránh false positive)
        if GAME_ENGINES.iter().any(|&e| app_lower.contains(e)) {
            // Proton/Wine process — check title context
            if !title_lower.is_empty() && !is_desktop_app(&title_lower) {
                return true;
            }
        }

        // Check known games
        if KNOWN_GAMES.iter().any(|&g| app_lower.contains(g) || title_lower.contains(g)) {
            return true;
        }

        // Check /proc/PID/environ for Steam runtime
        if let Some(pid) = pid {
            if is_steam_child_process(pid) {
                return true;
            }
        }

        false
    }
}

// ─── Action enum ──────────────────────────────────────────────────────

#[derive(Debug)]
pub enum GameModeAction {
    Enable  { app_name: String, notify: bool },
    Disable { restore_mode: Option<InputMethod>, notify: bool },
}

// ─── Helpers ──────────────────────────────────────────────────────────

fn detect_game_name(app_id: &str, title: &str) -> String {
    // Ưu tiên title (thường là tên game đầy đủ)
    if !title.is_empty() && title.len() < 64 {
        return title.to_string();
    }
    // Fallback: app_id
    app_id.to_string()
}

fn is_desktop_app(title: &str) -> bool {
    // Tránh detect sai: file manager, terminal, browser
    let desktop_patterns = [
        "files", "terminal", "browser", "editor", "settings",
        "code", "vscode", "firefox", "chrome", "chromium",
        "thunar", "nautilus", "dolphin", "pcmanfm",
    ];
    desktop_patterns.iter().any(|&p| title.contains(p))
}

fn is_steam_child_process(pid: u32) -> bool {
    use std::fs;
    // Đọc /proc/PID/environ → check STEAM_ hoặc STEAM_RUNTIME
    if let Ok(env) = fs::read(format!("/proc/{pid}/environ")) {
        let env_str = String::from_utf8_lossy(&env);
        if env_str.contains("STEAM_") || env_str.contains("SteamAppId=") {
            return true;
        }
    }
    false
}
```

## 📁 `vi-daemon/src/notify.rs` — D-Bus Notification

```rust
//! Desktop notification qua D-Bus (org.freedesktop.Notifications).
//! Không cần thêm dependency — dùng trực tiếp D-Bus socket.

use std::process::Command;

/// Gửi notification qua `notify-send` (fallback nếu không có libdbus)
pub fn send_notification(summary: &str, body: &str, icon: &str, urgency: Urgency) {
    let urgency_str = match urgency {
        Urgency::Low    => "low",
        Urgency::Normal => "normal",
        Urgency::Critical => "critical",
    };

    let result = Command::new("notify-send")
        .args(["--urgency", urgency_str])
        .args(["--icon", icon])
        .args(["--app-name", "vi-im"])
        .args(["--category", "im"])  // input method category
        .arg(summary)
        .arg(body)
        .spawn();

    match result {
        Ok(_) => tracing::info!("Notification sent: {summary}"),
        Err(e) => tracing::debug!("notify-send not available: {e}"),
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Urgency { Low, Normal, Critical }

/// Gửi notification khi game mode auto-bật
pub fn notify_game_mode_enabled(game_name: &str) {
    send_notification(
        &format!("🎮 Game Mode — {game_name}"),
        "Đã tự động bật Game Mode. Phím sẽ được chuyển thẳng vào game, \
         không xử lý tiếng Việt. Tắt bằng Ctrl+Shift+G hoặc tray menu.",
        "vi-im-game",
        Urgency::Low,
    );
}

/// Gửi notification khi game mode auto-tắt
pub fn notify_game_mode_disabled() {
    send_notification(
        "⌨️  Game Mode — Đã tắt",
        "IME đã khôi phục chế độ gõ tiếng Việt.",
        "vi-im-vn",
        Urgency::Low,
    );
}
```

## 📁 Integration vào `vi-daemon/src/learning.rs` hoặc event router

```rust
// Trong EventRouter (vi-im/src/main.rs) — thêm game detector

use vi_daemon::game_detector::{GameDetector, GameModeAction};
use vi_daemon::notify;

struct EventRouter {
    config:          SharedConfig,
    tray_update_tx:  std::sync::mpsc::SyncSender<TrayUpdate>,
    ime_method_tx:   std::sync::mpsc::SyncSender<InputMethod>,
    game_mode_tx:    std::sync::mpsc::SyncSender<bool>,     // NEW
    game_detector:   GameDetector,                           // NEW
}

impl EventRouter {
    fn handle_ime_event(&mut self, event: ImeEvent) {
        match event {
            // ── Focus changed → check game detection ──────────────────
            ImeEvent::FocusChanged { app_id, title, pid } => {
                let current_mode = self.config.read().unwrap().method;
                let current_game  = self.config.read().unwrap().game_mode;

                if let Some(action) = self.game_detector.check_focus(
                    &app_id, &title, pid, current_mode, current_game,
                ) {
                    match action {
                        GameModeAction::Enable { app_name, notify: do_notify } => {
                            // 1. Update config
                            { self.config.write().unwrap().game_mode = true; }
                            // 2. Notify IME thread
                            let _ = self.game_mode_tx.try_send(true);
                            // 3. Update tray
                            self.notify_tray(TrayUpdate::GameModeChanged(true));
                            // 4. Notification
                            if do_notify {
                                notify::notify_game_mode_enabled(&app_name);
                            }
                            tracing::info!("Auto game mode ON for: {app_name}");
                        }
                        GameModeAction::Disable { restore_mode, notify: do_notify } => {
                            // 1. Update config: tắt game mode
                            {
                                let mut cfg = self.config.write().unwrap();
                                cfg.game_mode = false;
                                // Restore mode cũ nếu có
                                if let Some(mode) = restore_mode {
                                    cfg.method = mode;
                                    let _ = self.ime_method_tx.try_send(mode);
                                }
                            }
                            // 2. Notify IME
                            let _ = self.game_mode_tx.try_send(false);
                            // 3. Update tray
                            self.notify_tray(TrayUpdate::GameModeChanged(false));
                            // 4. Notification
                            if do_notify {
                                notify::notify_game_mode_disabled();
                            }
                            tracing::info!("Auto game mode OFF, mode restored");
                        }
                    }
                    let _ = self.save_config();
                }
            }
            // ... (rest of handle_ime_event) ...
        }
    }
}
```

## 📁 Thêm `FocusChanged` vào `ImeEvent`

```rust
// crates/vi-wayland-im/src/events.rs

#[derive(Debug, Clone)]
pub enum ImeEvent {
    Activated,
    Deactivated,
    /// Focus changed — gửi app metadata cho game detection
    FocusChanged {
        app_id: String,
        title:  String,
        pid:    Option<u32>,
    },
    Committed { text: String, raw_len: usize },
    GameModeChanged(bool),
}
```

### Game Detection Flow

```
Compositor: window focus changes
         │
         ▼
vi-compositor-ipc :: FocusEvent { app_id: "steam", title: "Counter-Strike 2", pid: 12345 }
         │
         ▼
vi-daemon :: learning.rs → ImeEvent::FocusChanged
         │
         ▼
EventRouter :: handle_ime_event()
         │
         ▼
GameDetector :: check_focus()
         │
    ┌────┴──────────────────────────────────────┐
    │ is_game = true                            │
    │   ├─ config.game_mode = true              │
    │   ├─ game_mode_tx.send(true) → IME thread │
    │   ├─ TrayUpdate::GameModeChanged(true)    │
    │   └─ notify-send "🎮 Game Mode — CS2"     │
    │                                           │
    │ Khi game mất focus (quay lại desktop):    │
    │   ├─ game_mode = false                    │
    │   ├─ Restore method về Telex/VNI/Smart    │
    │   └─ notify-send "⌨️ IME restored"         │
    └──────────────────────────────────────────┘
```

---

# Part F: Deployment — systemd + Autostart

> **Ship cả hai:** systemd là primary (restart on crash, dependency ordering, journal
> logging), XDG autostart `.desktop` là fallback (hỗ trợ mọi DE/compositor, kể cả
> NixOS/Flatpak).

### Comparison

| | systemd user service | XDG autostart .desktop |
|---|---|---|
| Restart on crash | ✅ | ⚠️ No |
| Dependency ordering | ✅ After=graphical-session | ⚠️ No |
| Journal logging | ✅ journalctl | ⚠️ Redirect manual |
| All DEs/compositors | ~90% | ✅ 100% |
| NixOS/Flatpak | ⚠️ Cần setup | ✅ |
| Security hardening | ✅ (NoNewPrivileges, etc.) | ❌ |

## 📁 `deploy/systemd/vi-im.service`

```ini
# ~/.config/systemd/user/vi-im.service
#
# Install:
#   systemctl --user daemon-reload
#   systemctl --user enable --now vi-im.service
# Logs:
#   journalctl --user -u vi-im -f

[Unit]
Description=vi-im Vietnamese Wayland IME
Documentation=https://github.com/huu-nhan/vi-im
After=graphical-session.target
After=dbus.service
Wants=graphical-session.target
PartOf=graphical-session.target
Conflicts=fcitx5.service
Conflicts=ibus.service

[Service]
Type=simple
ExecStart=%h/.local/bin/vi-im
Restart=on-failure
RestartSec=3s
StartLimitInterval=60s
StartLimitBurst=5

Environment=RUST_LOG=vi_im=info
Environment=GDK_BACKEND=wayland,x11
Environment=GSK_RENDERER=cairo

# Resource limits
MemoryMax=64M
CPUWeight=50
IOWeight=50
Nice=5

# Security hardening
NoNewPrivileges=true
ProtectSystem=strict
ReadWritePaths=%h/.config/vi-im
ReadWritePaths=%t
ReadOnlyPaths=%h/.local
PrivateNetwork=true
PrivateDevices=false
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true
RestrictNamespaces=true
LockPersonality=true
MemoryDenyWriteExecute=false

StandardOutput=journal
StandardError=journal
SyslogIdentifier=vi-im
WorkingDirectory=%h

[Install]
WantedBy=graphical-session.target
```

## 📁 `deploy/systemd/vi-im-wayland-env.service`

```ini
# ~/.config/systemd/user/vi-im-wayland-env.service
# Propagate Wayland env vars vào systemd --user trước khi vi-im.service start.

[Unit]
Description=Propagate Wayland environment to systemd user session
After=graphical-session-pre.target
Before=graphical-session.target
ConditionEnvironment=WAYLAND_DISPLAY

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=/usr/bin/dbus-update-activation-environment --systemd \
    DISPLAY WAYLAND_DISPLAY XDG_SESSION_TYPE \
    XDG_SESSION_DESKTOP XDG_CURRENT_DESKTOP \
    DBUS_SESSION_BUS_ADDRESS XAUTHORITY

[Install]
WantedBy=graphical-session.target
```

## 📁 `deploy/autostart/vi-im.desktop`

```ini
# ~/.config/autostart/vi-im.desktop
# XDG autostart — fallback khi không dùng systemd.

[Desktop Entry]
Type=Application
Version=1.5
Name=vi-im
Name[vi]=Bộ gõ tiếng Việt vi-im
GenericName=Vietnamese IME
GenericName[vi]=Bộ gõ tiếng Việt
Comment=Native Vietnamese input method for Wayland
Comment[vi]=Bộ gõ tiếng Việt thuần Wayland, không cần IBus/Fcitx

Exec=%u/.local/bin/vi-im
TryExec=%u/.local/bin/vi-im
Terminal=false
Icon=vi-im

Categories=Utility;InputMethod;
Keywords=vietnamese;ime;input;wayland;telex;vni;
Keywords[vi]=tiếng việt;bộ gõ;telex;vni;wayland;

X-GNOME-Autostart-Delay=2
X-KDE-autostart-after=panel
Hidden=false
StartupNotify=false
```

## 📁 `deploy/install.sh` — Complete Install Script

```bash
#!/usr/bin/env bash
# vi-im install script
# Usage:
#   ./deploy/install.sh              # full install
#   ./deploy/install.sh --systemd    # systemd service only
#   ./deploy/install.sh --autostart  # XDG autostart only
#   ./deploy/install.sh --uninstall  # remove everything
set -euo pipefail

BINARY_NAME="vi-im"
INSTALL_BIN="${HOME}/.local/bin"
INSTALL_ICONS="${HOME}/.local/share/icons/hicolor"
INSTALL_APPS="${HOME}/.local/share/applications"
INSTALL_SYSTEMD="${HOME}/.config/systemd/user"
INSTALL_AUTOSTART="${HOME}/.config/autostart"
INSTALL_CONFIG="${HOME}/.config/vi-im"
DEPLOY_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "${DEPLOY_DIR}")"

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'
BLUE='\033[0;34m'; BOLD='\033[1m'; RESET='\033[0m'
info()    { echo -e "${BLUE}  →${RESET} $*"; }
success() { echo -e "${GREEN}  ✓${RESET} $*"; }
warn()    { echo -e "${YELLOW}  ⚠${RESET} $*"; }
header()  { echo -e "\n${BOLD}$*${RESET}"; }

DO_BUILD=true; DO_BINARY=true; DO_ICONS=true; DO_DESKTOP=true
DO_SYSTEMD=true; DO_AUTOSTART=true; DO_CONFIG=true

# ... (full install script logic — see deploy/install.sh) ...

# ─── Uninstall ────────────────────────────────────────────────────────
if [[ "${DO_UNINSTALL}" == true ]]; then
    systemctl --user stop vi-im.service 2>/dev/null || true
    systemctl --user disable vi-im.service 2>/dev/null || true
    rm -f "${INSTALL_BIN}/${BINARY_NAME}"
    rm -f "${INSTALL_SYSTEMD}/vi-im.service"
    rm -f "${INSTALL_AUTOSTART}/vi-im.desktop"
    rm -f "${INSTALL_APPS}/vi-im.desktop"
    systemctl --user daemon-reload 2>/dev/null || true
    success "vi-im uninstalled (config preserved at ${INSTALL_CONFIG}/)"
    exit 0
fi

# Detect compositor
detect_compositor() {
    if   pgrep -x "Hyprland"  &>/dev/null; then echo "Hyprland"
    elif pgrep -x "niri"       &>/dev/null; then echo "Niri"
    elif pgrep -x "sway"       &>/dev/null; then echo "Sway"
    elif pgrep -x "kwin_wayland" &>/dev/null; then echo "KWin"
    else echo "Unknown"; fi
}
COMPOSITOR=$(detect_compositor)

# ─── Compositor-specific notes ────────────────────────────────────────
case "${COMPOSITOR}" in
    Hyprland)
        info "Hyprland: add to hyprland.conf:"
        echo "    exec-once = systemctl --user start vi-im.service" ;;
    Niri)
        info "Niri: add to config.kdl:"
        echo '    spawn-at-startup "systemctl" "--user" "start" "vi-im.service"' ;;
    Sway)
        info "Sway: add to config:"
        echo "    exec systemctl --user start vi-im.service" ;;
esac

echo -e "\n${GREEN}${BOLD}  ✅ vi-im installed!${RESET}"
echo "  Binary: ${INSTALL_BIN}/${BINARY_NAME}"
echo "  Config: ${INSTALL_CONFIG}/config.toml"
```

## 📁 Deploy Directory Structure

```
deploy/
├── install.sh
├── compile.sh
├── systemd/
│   ├── vi-im.service
│   ├── vi-im-wayland-env.service
│   └── vi-im.service.d/
│       └── env.conf
├── autostart/
│   └── vi-im.desktop
└── icons/
    ├── vi-im.png          # Default 32×32
    ├── vi-im-vn.png       # 🟢 Vietnamese active
    ├── vi-im-en.png       # ⚫ English passthrough
    ├── vi-im-game.png     # 🔴 Game mode
    └── vi-im.svg          # Scalable source
```

### Autostart Decision Flow

```
Login → Wayland session start
           │
   ┌───────┴────────────────────────────────────────┐
   │  Compositor (Hyprland / Niri / Sway)            │
   │    └─ dbus-update-activation-environment        │
   │         WAYLAND_DISPLAY, XDG_SESSION_TYPE       │
   └──────────────────┬──────────────────────────────┘
                      │
           ┌──────────┴───────────────┐
           │                          │
    systemd --user              XDG autostart
    graphical-session            (.desktop)
           │                          │
    vi-im.service              vi-im.desktop
    (Restart=on-failure)       (Delay=2s)
           │                          │
           └──────────┬───────────────┘
                      │
                      ▼
                vi-im starts
                      │
          ┌───────────┼───────────┐
          │           │           │
     GTK tray    Wayland IME  Burst timer
     thread       thread       thread
          │           │           │
          └───────────┴───────────┘
                      │
               Event router loop
               (main thread, 50ms)
```

---

## ✅ Full Phase Completion Status (Updated)

| Phase | Nội dung | Files | Status |
|-------|----------|-------|--------|
| 1 | Unified binary | `vi-im/src/main.rs`, workspace `Cargo.toml` | ✅ |
| 1b | Dedup config types | `vi-config/src/lib.rs` | ✅ |
| 2 | Smart IME + NFD engine | `vi-engine/src/engine/` | ✅ 133 tests |
| 3 | Tray-only config (no QML) | `vi-tray/src/lib.rs` | ✅ |
| 4 | Burst commit 300ms | `vi-wayland-im/src/burst.rs` | ✅ |
| 5 | Game Mode (Ctrl+Shift+G) | `vi-wayland-im/src/dispatch.rs` | ✅ |
| 6 | **Game Auto-Detection** | `vi-daemon/src/game_detector.rs`, `notify.rs` | 🆕 |
| 7 | **systemd + Autostart** | `deploy/systemd/`, `deploy/autostart/` | 🆕 |
| 8 | Tests + AGENTS.md | — | 🔜 next |

---

# Part G: vi-settings — QuickShell/QML Polished UI

> **Inspiration:** QuickShell (2.6k★) — the toolkit behind niri's ecosystem bars,
> widgets, and shells. QML-based, hot-reload on save, Wayland-native, GPU-rendered.
> vi-settings nên tham khảo cách niri ecosystem dùng QuickShell để build settings
> panel đẹp, mượt, có animation.

### Current State vs Target

| | Hiện tại | Target (QuickShell) |
|---|---|---|
| **UI** | QML run qua `qmlscene` CLI | QuickShell app, Wayland-native window |
| **Hot reload** | ❌ Không | ✅ Lưu file → UI update ngay |
| **IPC với daemon** | ❌ Đọc/ghi file config | ✅ Unix socket / D-Bus tới vi-daemon |
| **Theme** | Hệ thống (Qt fusion) | Theo system theme + custom accent |
| **Animation** | ❌ | ✅ Transition, hover, state change |
| **Preview** | ❌ | ✅ Live preview gõ thử ngay trong settings |

### Why QuickShell?

```
┌─────────────────────────────────────────────────────────────┐
│  QuickShell advantages                                      │
├─────────────────────────────────────────────────────────────┤
│  ✅ QML + JavaScript = UI cực nhanh, dễ prototype          │
│  ✅ Hot-reload: sửa QML → thấy kết quả ngay lập tức        │
│  ✅ Wayland-native: layer-shell, popup, toplevel            │
│  ✅ GPU rendering: mượt 144fps, animation hardware-accelerated│
│  ✅ Built-in widgets: Button, Slider, ComboBox, ListView    │
│  ✅ System tray + notifications integration                 │
│  ✅ Được dùng bởi niri ecosystem (bars, widgets, shells)    │
└─────────────────────────────────────────────────────────────┘
```

## 📁 Architecture: Rust daemon ↔ QuickShell Settings

```
┌─────────────────┐    Unix Socket     ┌──────────────────────────┐
│   vi-daemon      │ ◄──────────────►  │  vi-settings (QuickShell) │
│   (Rust)         │   JSON IPC        │  (QML + C++ / Rust bind)  │
│                  │                   │                          │
│  • ConfigManager │                   │  • sidebar.tsx-style nav  │
│  • LearnedStore  │                   │  • Live typing preview    │
│  • GameDetector  │                   │  • Per-app config editor  │
│  • IME state     │                   │  • Hotkey recorder        │
└─────────────────┘                   └──────────────────────────┘
```

## 📁 `vi-settings/main.qml` — Root Window

```qml
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import Quickshell.Wayland

ShellWindow {
    id: window
    title: "vi-im Settings"
    width: 800
    height: 560
    minimumWidth: 680
    minimumHeight: 420
    color: "#1a1b26"           // Tokyo Night dark bg

    // ── Header bar ────────────────────────────────────────────────
    header: Rectangle {
        height: 44
        color: "#24283b"

        RowLayout {
            anchors { fill: parent; leftMargin: 16; rightMargin: 16 }

            Text {
                text: "vi-im"
                font { pixelSize: 18; bold: true }
                color: "#7aa2f7"
            }

            Item { Layout.fillWidth: true }

            // Method badge
            Rectangle {
                radius: 6
                color: "#3b4261"
                implicitWidth: badgeText.implicitWidth + 20
                implicitHeight: 28
                Text {
                    id: badgeText
                    anchors.centerIn: parent
                    text: viIpc.currentMethod
                    color: "#9ece6a"
                    font.pixelSize: 13
                }
            }
        }
    }

    // ── Body: sidebar + content ───────────────────────────────────
    RowLayout {
        anchors { fill: parent; margins: 0 }
        spacing: 0

        // Sidebar
        Sidebar {
            id: sidebar
            Layout.preferredWidth: 200
            Layout.fillHeight: true

            model: ListModel {
                ListElement { name: "Chung";       icon: "⚙️"; page: "general" }
                ListElement { name: "Kiểu gõ";     icon: "⌨️"; page: "input" }
                ListElement { name: "Hiển thị";    icon: "👁️"; page: "display" }
                ListElement { name: "Ứng dụng";    icon: "📱"; page: "apps" }
                ListElement { name: "Website";     icon: "🌐"; page: "sites" }
                ListElement { name: "Game Mode";   icon: "🎮"; page: "game" }
                ListElement { name: "Phím tắt";    icon: "⚡"; page: "hotkeys" }
                ListElement { name: "Giới thiệu";  icon: "ℹ️"; page: "about" }
            }

            onPageSelected: (page) => stackView.push(page)
        }

        // Separator
        Rectangle { width: 1; Layout.fillHeight: true; color: "#3b4261" }

        // Content area
        StackView {
            id: stackView
            Layout.fillWidth: true
            Layout.fillHeight: true
            initialItem: generalPage

            // ── Animation ──────────────────────────────────────────
            pushEnter: Transition {
                NumberAnimation { property: "opacity"; from: 0; to: 1; duration: 150 }
                NumberAnimation { property: "x"; from: 20; to: 0; duration: 200; easing.type: Easing.OutCubic }
            }
            pushExit: Transition {
                NumberAnimation { property: "opacity"; from: 1; to: 0; duration: 100 }
            }
        }
    }
}
```

## 📁 `vi-settings/Sidebar.qml` — Navigation Panel

```qml
import QtQuick
import QtQuick.Controls

Rectangle {
    id: root
    color: "#1f2335"

    property alias model: listView.model
    signal pageSelected(string page)

    ListView {
        id: listView
        anchors.fill: parent
        anchors.topMargin: 8
        clip: true
        currentIndex: 0

        delegate: ItemDelegate {
            width: ListView.view.width
            height: 40

            contentItem: Row {
                spacing: 10
                anchors { left: parent.left; leftMargin: 16; verticalCenter: parent.verticalCenter }

                Text {
                    text: model.icon
                    font.pixelSize: 16
                    anchors.verticalCenter: parent.verticalCenter
                }

                Text {
                    text: model.name
                    color: ListView.isCurrentItem ? "#7aa2f7" : "#a9b1d6"
                    font {
                        pixelSize: 14
                        bold: ListView.isCurrentItem
                    }
                    anchors.verticalCenter: parent.verticalCenter
                }
            }

            background: Rectangle {
                color: ListView.isCurrentItem ? "#292e42" : "transparent"
                Rectangle {
                    visible: ListView.isCurrentItem
                    width: 3; height: parent.height
                    color: "#7aa2f7"
                    anchors.left: parent.left
                }
            }

            onClicked: {
                listView.currentIndex = index;
                root.pageSelected(model.page);
            }

            // ── Hover effect ───────────────────────────────────────
            HoverHandler {
                cursorShape: Qt.PointingHandCursor
            }
        }
    }
}
```

## 📁 `vi-settings/GeneralPage.qml` — Tab Chung

```qml
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ScrollView {
    id: root
    ScrollBar.horizontal.policy: ScrollBar.AlwaysOff

    ColumnLayout {
        width: root.width - 40
        anchors { left: parent.left; leftMargin: 20; top: parent.top; topMargin: 12 }
        spacing: 16

        // ── Section: Method ────────────────────────────────────────
        SectionHeader { text: "Phương thức nhập" }

        Flow {
            Layout.fillWidth: true
            spacing: 10

            Repeater {
                model: [
                    { method: "Telex", desc: "Gõ chữ có dấu (s/f/r/x/j)", icon: "🇻🇳" },
                    { method: "VNI",   desc: "Gõ số có dấu (1-9)",          icon: "🇻🇳" },
                    { method: "Smart", desc: "Tự động nhận VNI hoặc Telex",  icon: "🧠" },
                    { method: "English", desc: "Tắt tiếng Việt",             icon: "🇬🇧" },
                ]

                MethodCard {
                    width: (root.width - 50) / 2
                    methodName: modelData.method
                    description: modelData.desc
                    icon: modelData.icon
                    selected: viIpc.currentMethod === modelData.method
                    onClicked: viIpc.setMethod(modelData.method)
                }
            }
        }

        // ── Section: Tone Style ─────────────────────────────────────
        SectionHeader { text: "Kiểu dấu" }

        RowLayout {
            spacing: 12

            ToneStyleButton { label: "ũ — Classic";  value: "classic";  checked: viIpc.toneStyle === "classic" }
            ToneStyleButton { label: "ữ — Modern";   value: "modern";   checked: viIpc.toneStyle === "modern" }
        }

        // ── Section: Live Preview ───────────────────────────────────
        SectionHeader { text: "Thử gõ" }

        Rectangle {
            Layout.fillWidth: true
            height: 64
            radius: 8
            color: "#292e42"
            border { color: "#3b4261"; width: 1 }

            TextInput {
                id: previewInput
                anchors { fill: parent; margins: 16 }
                color: "#c0caf5"
                font.pixelSize: 22
                text: ""

                Text {
                    anchors.fill: parent
                    text: "Gõ thử ở đây... (vd: tieengs Vieetj)"
                    color: "#565f89"
                    font.pixelSize: 16
                    visible: previewInput.text === ""
                }

                onTextChanged: {
                    // Gửi text qua IPC để engine process
                    previewResult.text = viIpc.preview(previewInput.text);
                }
            }

            // Preview result
            Text {
                id: previewResult
                anchors { right: parent.right; rightMargin: 16; verticalCenter: parent.verticalCenter }
                color: "#9ece6a"
                font.pixelSize: 18
            }
        }

        // ── Section: Behavior ───────────────────────────────────────
        SectionHeader { text: "Hành vi" }

        ColumnLayout {
            spacing: 8

            ToggleRow {
                label: "Tự động bật Game Mode khi phát hiện game"
                description: "Steam, CS2, Dota2, v.v. — tự bypass IME"
                checked: viIpc.autoGameMode
                onToggled: viIpc.setAutoGameMode(checked)
            }

            ToggleRow {
                label: "Tự động khởi động cùng hệ thống"
                description: "Thêm vào systemd user service hoặc XDG autostart"
                checked: viIpc.autostart
                onToggled: viIpc.setAutostart(checked)
            }

            ToggleRow {
                label: "Hiển thị notification"
                description: "Thông báo khi chuyển chế độ, phát hiện game"
                checked: viIpc.notifications
                onToggled: viIpc.setNotifications(checked)
            }
        }
    }
}
```

## 📁 `vi-settings/AppsPage.qml` — Per-App Config

```qml
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ScrollView {
    id: root

    ColumnLayout {
        width: root.width - 40
        anchors { left: parent.left; leftMargin: 20; top: parent.top; topMargin: 12 }
        spacing: 16

        SectionHeader { text: "Cấu hình theo ứng dụng" }

        Text {
            text: "Mỗi ứng dụng có thể dùng chế độ gõ riêng.\n"
                + "VD: Firefox = Smart, Terminal = English, Code = Telex"
            color: "#a9b1d6"
            font.pixelSize: 13
            wrapMode: Text.WordWrap
            Layout.fillWidth: true
        }

        // Search
        TextField {
            id: searchField
            Layout.fillWidth: true
            placeholderText: "🔍  Tìm ứng dụng..."
            placeholderTextColor: "#565f89"
            color: "#c0caf5"

            background: Rectangle {
                radius: 8
                color: "#292e42"
                border { color: searchField.activeFocus ? "#7aa2f7" : "#3b4261"; width: 1 }
            }
        }

        // App list
        ListView {
            id: appList
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: viIpc.appConfigs

            delegate: ItemDelegate {
                width: ListView.view.width
                height: 52

                contentItem: RowLayout {
                    spacing: 12

                    // App icon placeholder
                    Rectangle {
                        width: 32; height: 32; radius: 6; color: "#3b4261"
                        Text { anchors.centerIn: parent; text: model.icon; font.pixelSize: 18 }
                    }

                    ColumnLayout {
                        spacing: 2
                        Layout.fillWidth: true

                        Text {
                            text: model.appName
                            color: "#c0caf5"
                            font.pixelSize: 14
                        }
                        Text {
                            text: model.appId
                            color: "#565f89"
                            font.pixelSize: 11
                        }
                    }

                    ComboBox {
                        model: ["Mặc định", "Telex", "VNI", "Smart", "English", "Tắt"]
                        currentIndex: model.methodIndex
                        onActivated: viIpc.setAppMethod(model.appId, currentText)

                        background: Rectangle {
                            radius: 4
                            color: "#3b4261"
                            implicitWidth: 100
                        }
                        contentItem: Text {
                            text: parent.currentText
                            color: "#9ece6a"
                            font.pixelSize: 13
                            anchors.centerIn: parent
                        }
                    }
                }

                background: Rectangle {
                    color: index % 2 === 0 ? "transparent" : "#1f2335"
                }
            }
        }
    }
}
```

## 📁 `vi-settings/HotkeysPage.qml` — Hotkey Configuration

```qml
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ScrollView {
    id: root

    ColumnLayout {
        width: root.width - 40
        anchors { left: parent.left; leftMargin: 20; top: parent.top; topMargin: 12 }
        spacing: 16

        SectionHeader { text: "Phím tắt" }

        Repeater {
            model: [
                { key: "toggleIme",      label: "Bật/Tắt tiếng Việt",   default: "Ctrl+Shift+Space" },
                { key: "toggleGameMode", label: "Bật/Tắt Game Mode",     default: "Ctrl+Shift+G" },
                { key: "switchMethod",   label: "Đổi kiểu gõ",           default: "Ctrl+Shift+M" },
            ]

            HotkeyRow {
                Layout.fillWidth: true
                label: modelData.label
                defaultValue: modelData.default
                currentValue: viIpc.hotkey(modelData.key)
                onRecorded: (keys) => viIpc.setHotkey(modelData.key, keys)
            }
        }
    }
}

// ─── HotkeyRow component ─────────────────────────────────────────────

Rectangle {
    id: hotkeyRoot
    height: 48
    radius: 8
    color: recording ? "#7aa2f7" + "20" : "#292e42"
    border { color: recording ? "#7aa2f7" : "#3b4261"; width: 1 }

    property string label
    property string defaultValue
    property string currentValue
    signal recorded(string keys)
    property bool recording: false

    RowLayout {
        anchors { fill: parent; leftMargin: 12; rightMargin: 12 }
        spacing: 12

        Text {
            text: hotkeyRoot.label
            color: "#c0caf5"
            font.pixelSize: 14
            Layout.fillWidth: true
        }

        Rectangle {
            radius: 6
            color: "#1a1b26"
            implicitWidth: keyText.implicitWidth + 24
            implicitHeight: 32

            Text {
                id: keyText
                anchors.centerIn: parent
                text: hotkeyRoot.recording ? "Nhấn phím..." : hotkeyRoot.currentValue || hotkeyRoot.defaultValue
                color: hotkeyRoot.recording ? "#7aa2f7" : "#9ece6a"
                font { pixelSize: 13; bold: hotkeyRoot.recording }
            }

            MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                onClicked: {
                    hotkeyRoot.recording = !hotkeyRoot.recording;
                    if (!hotkeyRoot.recording) hotkeyRoot.recorded(keyText.text);
                }
            }
        }

        // Reset button
        Text {
            text: "↺"
            color: "#565f89"
            font.pixelSize: 16
            visible: hotkeyRoot.currentValue !== hotkeyRoot.defaultValue

            MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                onClicked: hotkeyRoot.recorded(hotkeyRoot.defaultValue)
            }
        }
    }
}
```

## 📁 `vi-settings/IpcClient.qml` — Daemon Communication

```qml
import QtQuick

// Singleton: giao tiếp với vi-daemon qua Unix socket (JSON IPC)
QtObject {
    id: viIpc

    // ── Reactive properties (auto-update khi daemon push) ─────────
    property string currentMethod: "Telex"
    property string toneStyle: "classic"
    property bool autoGameMode: false
    property bool autostart: true
    property bool notifications: true
    property var appConfigs: []

    // ── Socket connection ─────────────────────────────────────────
    property var socket: null

    Component.onCompleted: {
        connectToDaemon();
    }

    function connectToDaemon() {
        // Kết nối tới Unix socket của vi-daemon
        // socket = new QTcpSocket();
        // socket.connectToHost("", 0); // Unix socket path
        // socket.readyRead.connect(onDataReceived);
    }

    function onDataReceived() {
        var data = JSON.parse(socket.readAll());
        if (data.method)   currentMethod = data.method;
        if (data.toneStyle) toneStyle = data.toneStyle;
        if (data.appConfigs) appConfigs = data.appConfigs;
        // ... update other properties
    }

    // ── Commands → daemon ─────────────────────────────────────────
    function setMethod(method) { send({ cmd: "setMethod", method: method }); }
    function setToneStyle(style) { send({ cmd: "setToneStyle", style: style }); }
    function setAutoGameMode(on) { send({ cmd: "setAutoGameMode", on: on }); }
    function setAutostart(on) { send({ cmd: "setAutostart", on: on }); }
    function setNotifications(on) { send({ cmd: "setNotifications", on: on }); }
    function setAppMethod(appId, method) { send({ cmd: "setAppMethod", appId: appId, method: method }); }
    function setHotkey(key, combo) { send({ cmd: "setHotkey", key: key, combo: combo }); }
    function preview(text) { send({ cmd: "preview", text: text }); return ""; }
    function hotkey(key) { return ""; }  // fetched on connect

    function send(obj) {
        if (socket) socket.write(JSON.stringify(obj) + "\n");
    }
}
```

## 📁 `vi-daemon/src/ipc.rs` — Daemon-side IPC Server

```rust
//! Unix socket IPC server — phục vụ vi-settings QuickShell client.
//! Giao thức: JSON lines qua Unix datagram socket.

use std::os::unix::net::UnixListener;
use std::io::{BufRead, BufReader, Write};
use serde::{Deserialize, Serialize};
use vi_config::{InputMethod, SharedConfig};

#[derive(Debug, Deserialize)]
#[serde(tag = "cmd")]
pub enum IpcCommand {
    #[serde(rename = "setMethod")]
    SetMethod { method: InputMethod },
    #[serde(rename = "setToneStyle")]
    SetToneStyle { style: String },
    #[serde(rename = "setAutoGameMode")]
    SetAutoGameMode { on: bool },
    #[serde(rename = "setAutostart")]
    SetAutostart { on: bool },
    #[serde(rename = "setNotifications")]
    SetNotifications { on: bool },
    #[serde(rename = "setAppMethod")]
    SetAppMethod { app_id: String, method: String },
    #[serde(rename = "setHotkey")]
    SetHotkey { key: String, combo: String },
    #[serde(rename = "preview")]
    Preview { text: String },
}

#[derive(Debug, Serialize)]
pub struct IpcState {
    pub method:       String,
    pub tone_style:   String,
    pub auto_game_mode: bool,
    pub autostart:    bool,
    pub notifications: bool,
    pub app_configs:  Vec<AppConfigEntry>,
}

#[derive(Debug, Serialize)]
pub struct AppConfigEntry {
    pub app_name:     String,
    pub app_id:       String,
    pub icon:         String,
    pub method_index: usize,
}

/// Spawn IPC server thread — lắng nghe trên Unix socket
pub fn spawn_ipc_server(
    config: SharedConfig,
    socket_path: &str,
) -> std::thread::JoinHandle<()> {
    let path = socket_path.to_string();
    std::thread::Builder::new()
        .name("vi-im-ipc".into())
        .spawn(move || {
            let _ = std::fs::remove_file(&path);
            let listener = UnixListener::bind(&path)
                .expect("Failed to bind IPC socket");
            tracing::info!("IPC server listening on {path}");

            for stream in listener.incoming() {
                match stream {
                    Ok(mut stream) => {
                        let mut reader = BufReader::new(stream.try_clone().unwrap());
                        let mut line = String::new();
                        if reader.read_line(&mut line).is_ok() {
                            if let Ok(cmd) = serde_json::from_str::<IpcCommand>(&line) {
                                let response = handle_command(cmd, &config);
                                let _ = writeln!(stream, "{}", serde_json::to_string(&response).unwrap());
                            }
                        }
                    }
                    Err(e) => tracing::error!("IPC accept error: {e}"),
                }
            }
        })
        .expect("Failed to spawn IPC thread")
}

fn handle_command(cmd: IpcCommand, config: &SharedConfig) -> IpcState {
    match cmd {
        IpcCommand::SetMethod { method } => {
            config.write().unwrap().method = method;
            let _ = config.read().unwrap().save();
        }
        IpcCommand::Preview { text } => {
            // Forward tới engine, trả về preview
            // (cần channel tới IME thread)
        }
        // ... xử lý các command khác ...
        _ => {}
    }
    build_state(config)
}

fn build_state(config: &SharedConfig) -> IpcState {
    let cfg = config.read().unwrap();
    IpcState {
        method:     cfg.method.display_name().to_string(),
        tone_style: "classic".into(),
        auto_game_mode: false,
        autostart:  cfg.autostart,
        notifications: true,
        app_configs: vec![],  // TODO: populate từ learned store
    }
}
```

## 📁 Cargo.toml updates

### `crates/vi-daemon/Cargo.toml` — thêm serde_json

```toml
[dependencies]
serde_json = "1"
# ... existing deps ...
```

### `crates/vi-settings/Cargo.toml` — QuickShell approach

```toml
[package]
name = "vi-settings"
version = "0.1.0"
edition = "2024"
description = "Settings GUI for vi-im — QuickShell QML app"

# Option A: pure QML, launched via quickshell CLI
#   quickshell vi-settings.qml
#   (không cần Rust binary, chỉ cần file .qml)

# Option B: Rust binary spawns quickshell process
#   (giữ main.rs hiện tại nhưng launch quickshell thay vì qmlscene)

[dependencies]
# Minimal — chỉ cần find + launch quickshell
vi-config = { path = "../vi-config" }
tracing = "0.1"

[[bin]]
name = "vi-settings"
path = "src/main.rs"
```

## 📁 Deploy: QML assets

```
deploy/
└── qml/
    ├── main.qml
    ├── Sidebar.qml
    ├── components/
    │   ├── SectionHeader.qml
    │   ├── MethodCard.qml
    │   ├── ToneStyleButton.qml
    │   ├── ToggleRow.qml
    │   └── HotkeyRow.qml
    ├── pages/
    │   ├── GeneralPage.qml
    │   ├── InputPage.qml
    │   ├── DisplayPage.qml
    │   ├── AppsPage.qml
    │   ├── SitesPage.qml
    │   ├── GamePage.qml
    │   └── HotkeysPage.qml
    └── ipc/
        └── IpcClient.qml
```

## 📊 Settings UI Flow

```
User mở Settings (tray menu "⚙️ Cấu hình..." hoặc CLI `vi-settings`)
         │
         ▼
vi-daemon spawns: quickshell /usr/share/vi-im/qml/main.qml
         │
         ▼
QuickShell loads QML, kết nối Unix socket đến daemon
         │
         ▼
┌────────────────────────────────────────────┐
│  vi-settings window                         │
│  ┌──────────┬─────────────────────────────┐│
│  │ Sidebar  │  Content (StackView)         ││
│  │          │                             ││
│  │ ⚙️ Chung  │  Phương thức nhập           ││
│  │ ⌨️ Kiểu gõ│  ┌────────┐ ┌────────┐     ││
│  │ 👁️ Hiển thị│  │ Telex  │ │ VNI    │     ││
│  │ 📱 Ứng dụng│  │ 🇻🇳    │ │ 🇻🇳    │     ││
│  │ 🌐 Website│  └────────┘ └────────┘     ││
│  │ 🎮 Game   │  ┌────────┐ ┌────────┐     ││
│  │ ⚡ Phím tắt│  │ Smart  │ │ English│     ││
│  │ ℹ️ About  │  └────────┘ └────────┘     ││
│  │          │                             ││
│  │          │  Gõ thử: [________] → "việt"││
│  └──────────┴─────────────────────────────┘│
└────────────────────────────────────────────┘
         │
    Mỗi thay đổi → JSON command qua IPC → daemon xử lý
         │
    Daemon push state update → QML property binding tự update UI
```

### Visual Design Tokens (Tokyo Night Theme)

```qml
// Theme colors dùng chung toàn bộ app
property color bg:        "#1a1b26"   // nền chính
property color bgDark:    "#1f2335"   // sidebar
property color bgLight:   "#292e42"   // card / input bg
property color border:    "#3b4261"   // border
property color accent:    "#7aa2f7"   // xanh highlight
property color success:   "#9ece6a"   // xanh lá (active state)
property color warning:   "#e0af68"   // vàng (game mode)
property color error:     "#f7768e"   // đỏ
property color textPrimary: "#c0caf5" // chữ chính
property color textSecondary: "#a9b1d6" // chữ phụ
property color textMuted: "#565f89"   // placeholder / disabled
```

---

## ✅ Full Phase Completion Status (Updated)

| Phase | Nội dung | Files | Status |
|-------|----------|-------|--------|
| 1 | Unified binary | `vi-im/src/main.rs`, workspace `Cargo.toml` | ✅ |
| 1b | Dedup config types | `vi-config/src/lib.rs` | ✅ |
| 2 | Smart IME + NFD engine | `vi-engine/src/engine/` | ✅ 133 tests |
| 3 | Tray-only config (no QML) | `vi-tray/src/lib.rs` | ✅ |
| 4 | Burst commit 300ms | `vi-wayland-im/src/burst.rs` | ✅ |
| 5 | Game Mode (Ctrl+Shift+G) | `vi-wayland-im/src/dispatch.rs` | ✅ |
| 6 | Game Auto-Detection | `vi-daemon/src/game_detector.rs`, `notify.rs` | 🆕 |
| 7 | systemd + Autostart | `deploy/systemd/`, `deploy/autostart/` | 🆕 |
| 8 | **vi-settings QuickShell UI** | `vi-settings/*.qml`, `vi-daemon/src/ipc.rs` | 🆕 |
| 9 | Tests + AGENTS.md | — | 🔜 next |

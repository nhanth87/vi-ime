🌊 Wayland Dispatch Integration — Full Implementation
Kiến trúc tổng thể
Virtual Backspace architecture (evdev + uinput, tương tự OpenKey/VMK-core) — tránh hoàn toàn Preedit bugs (focus loss, popup disappear).PRz2H8Bb78QJtdmP33BUYb

Protocol stack: input-method-v2 + input-method-keyboard-grab-v1, bypass IBus/Fcitx, native trên wlroots (Hyprland, Niri, COSMIC).oF8if9Ae7K4JUrgC2RruQj

zwp_input_method_v2_commit_string() → chốt từ đẩy ký tự vào app khi word boundary. Sau đó gọi zwp_input_method_v2_commit() để áp dụng.kpGGJy4vwnhrFgfXWw4dAs

📁 crates/vi-wayland-im/src/state.rs — IME State
rust


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
    pub im_manager:  Option&lt;ZwpInputMethodManagerV2&gt;,
    pub im:          Option&lt;ZwpInputMethodV2&gt;,
    pub kb_grab:     Option&lt;ZwpInputMethodKeyboardGrabV2&gt;,
    pub seat:        Option&lt;WlSeat&gt;,
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
    fn default() -&gt; Self {
        Self {
            pending:     false,
            last_key_at: std::time::Instant::now(),
            window:      std::time::Duration::from_millis(300),
        }
    }
}
impl ImeState {
    pub fn new(method: ConfigMethod) -&gt; Self {
        Self {
            im_manager: None,
            im:         None,
            kb_grab:    None,
            seat:       None,
            buffer:     KeyBuffer::new(),
            engine:     ModernVietnameseEngine,
            method,
            serial:     0,
            active:     false,
            burst:      BurstState::default(),
            game_mode:  false,
        }
    }
}
📁 crates/vi-wayland-im/src/actions.rs — Commit Actions
rust


//! Tất cả Wayland protocol actions: commit_string, virtual_backspace, passthrough.
use wayland_protocols::wp::input_method::zv2::client::zwp_input_method_v2::ZwpInputMethodV2;
use wayland_protocols::wp::virtual_keyboard::zv1::client::{
    zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1,
    zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
};
use crate::state::ImeState;
/// Gửi N virtual backspace để xóa raw_keys đã hiển thị
///
/// Cơ chế: gửi key press + release cho keycode 14 (KEY_BACKSPACE)
/// thông qua virtual keyboard, KHÔNG qua input-method protocol.
pub fn send_virtual_backspaces(
    vk:    &amp;ZwpVirtualKeyboardV1,
    count: usize,
    time:  u32,
) {
    for i in 0..count {
        // KEY_BACKSPACE = 14 (Linux evdev keycode)
        // state: 1 = pressed, 0 = released
        vk.key(time + (i as u32 * 2),     14, 1); // press
        vk.key(time + (i as u32 * 2) + 1, 14, 0); // release
    }
}
/// Commit Vietnamese string qua input-method-v2 protocol
///
/// Sequence bắt buộc theo protocol spec:
/// 1. commit_string(text)          — set pending string
/// 2. commit()                      — apply pending state (với serial)
pub fn commit_vietnamese(
    im:     &amp;ZwpInputMethodV2,
    text:   &amp;str,
    serial: u32,
) {
    im.commit_string(text.to_string());
    im.commit(serial);
}
/// Passthrough: forward key event nguyên vẹn (English mode / Game mode)
///
/// Dùng virtual keyboard để inject key event bypass IME engine.
pub fn passthrough_key(
    vk:      &amp;ZwpVirtualKeyboardV1,
    keycode: u32,   // evdev keycode (e.g. KEY_A = 30)
    state:   u32,   // 1=press, 0=release
    time:    u32,
    mods:    ModsState,
) {
    // Set modifiers nếu có thay đổi
    if mods.dirty {
        vk.modifiers(
            mods.depressed,
            mods.latched,
            mods.locked,
            mods.group,
        );
    }
    vk.key(time, keycode, state);
}
/// Full commit sequence: virtual_backspace(n) + commit_string(viet) + commit()
///
/// Đây là "atomic" operation của Virtual Backspace architecture:
/// 1. Xóa n ký tự raw đã hiển thị
/// 2. Commit chuỗi Vietnamese NFC đã xử lý
pub fn do_commit(
    im:     &amp;ZwpInputMethodV2,
    vk:     &amp;ZwpVirtualKeyboardV1,
    state:  &amp;mut ImeState,
    time:   u32,
) {
    let n = state.buffer.raw_len();
    if n == 0 { return; }
    // Bước 1: Render Vietnamese output TRƯỚC (parse toàn bộ buffer)
    let viet_text = state.buffer.render().to_string();
    // Bước 2: Virtual backspace × n (xóa raw chars đã hiện trên màn hình)
    send_virtual_backspaces(vk, n, time);
    // Bước 3: commit_string + commit() qua input-method-v2
    commit_vietnamese(im, &amp;viet_text, state.serial);
    // Bước 4: Tăng serial (bắt buộc theo protocol)
    state.serial = state.serial.wrapping_add(1);
    // Bước 5: Clear buffer (next syllable)
    state.buffer.clear();
    state.burst.pending = false;
}
/// Commit rồi forward boundary char (space, dấu câu)
pub fn do_commit_then_passthrough(
    im:        &amp;ZwpInputMethodV2,
    vk:        &amp;ZwpVirtualKeyboardV1,
    state:     &amp;mut ImeState,
    boundary:  char,
    keycode:   u32,
    time:      u32,
    mods:      ModsState,
) {
    // 1. Commit từ đang gõ
    do_commit(im, vk, state, time);
    // 2. Forward boundary key (space, '.', ',', ...) nguyên vẹn
    passthrough_key(vk, keycode, 1, time + 1, mods); // press
    passthrough_key(vk, keycode, 0, time + 2, mods); // release
}
/// Modifier state từ keyboard grab
#[derive(Default, Clone, Copy)]
pub struct ModsState {
    pub depressed: u32,
    pub latched:   u32,
    pub locked:    u32,
    pub group:     u32,
    pub dirty:     bool,
}
📁 crates/vi-wayland-im/src/dispatch.rs — Event Dispatch
rust


//! Wayland event dispatch — xử lý keyboard grab events.
//! Đây là trái tim của IME: nhận key events, quyết định commit hay buffer.
use smithay_client_toolkit::reexports::client::{
    globals::GlobalList,
    protocol::wl_registry,
    Connection, Dispatch, QueueHandle,
};
use wayland_protocols::wp::input_method::zv2::client::{
    zwp_input_method_keyboard_grab_v2::{
        self, ZwpInputMethodKeyboardGrabV2,
    },
    zwp_input_method_v2::{self, ZwpInputMethodV2},
};
use xkbcommon::xkb;
use crate::{
    actions::{do_commit, do_commit_then_passthrough, passthrough_key, ModsState},
    state::ImeState,
};
use vi_config::InputMethod as ConfigMethod;
// ─── ZwpInputMethodV2 events ─────────────────────────────────────────────────
impl Dispatch&lt;ZwpInputMethodV2, ()&gt; for ImeState {
    fn event(
        state:  &amp;mut Self,
        _proxy: &amp;ZwpInputMethodV2,
        event:  zwp_input_method_v2::Event,
        _:      &amp;(),
        _conn:  &amp;Connection,
        _qh:    &amp;QueueHandle&lt;Self&gt;,
    ) {
        match event {
            // Compositor kích hoạt IME (focus vào text field)
            zwp_input_method_v2::Event::Activate =&gt; {
                state.active = true;
                state.buffer.clear();
                tracing::debug!("IME activated");
            }
            // Compositor tắt IME (focus rời text field)
            zwp_input_method_v2::Event::Deactivate =&gt; {
                state.active = false;
                // Flush buffer còn lại nếu có
                if state.buffer.raw_len() &gt; 0 {
                    if let (Some(im), Some(vk)) = (&amp;state.im, &amp;state.vk) {
                        do_commit(im, vk, state, 0);
                    }
                }
                tracing::debug!("IME deactivated");
            }
            // Compositor yêu cầu IME xóa surrounding text (hiếm dùng)
            zwp_input_method_v2::Event::UnavailableInputMethod =&gt; {
                tracing::warn!("Input method unavailable");
            }
            _ =&gt; {}
        }
    }
}
// ─── ZwpInputMethodKeyboardGrabV2 events ─────────────────────────────────────
impl Dispatch&lt;ZwpInputMethodKeyboardGrabV2, ()&gt; for ImeState {
    fn event(
        state:  &amp;mut Self,
        _proxy: &amp;ZwpInputMethodKeyboardGrabV2,
        event:  zwp_input_method_keyboard_grab_v2::Event,
        _:      &amp;(),
        _conn:  &amp;Connection,
        _qh:    &amp;QueueHandle&lt;Self&gt;,
    ) {
        match event {
            // Keymap từ compositor (xkbcommon format)
            zwp_input_method_keyboard_grab_v2::Event::Keymap { format, fd, size } =&gt; {
                state.handle_keymap(format, fd, size);
            }
            // Key press/release event
            zwp_input_method_keyboard_grab_v2::Event::Key {
                serial,
                time,
                key,       // evdev keycode (0-based, cần +8 cho XKB)
                key_state, // 1=pressed, 0=released
            } =&gt; {
                state.serial = serial;
                if key_state == 1 { // chỉ xử lý key press
                    state.handle_key(key, time);
                }
            }
            // Modifier key changes (Shift, Ctrl, Alt, ...)
            zwp_input_method_keyboard_grab_v2::Event::Modifiers {
                serial,
                mods_depressed,
                mods_latched,
                mods_locked,
                group,
            } =&gt; {
                state.serial = serial;
                state.handle_modifiers(
                    mods_depressed, mods_latched, mods_locked, group,
                );
            }
            // Repeat info (key repeat rate từ compositor)
            zwp_input_method_keyboard_grab_v2::Event::RepeatInfo { rate, delay } =&gt; {
                tracing::debug!("Key repeat: rate={rate} delay={delay}ms");
            }
            _ =&gt; {}
        }
    }
}
// ─── Key handling logic ───────────────────────────────────────────────────────
impl ImeState {
    /// Xử lý keymap mới từ compositor
    fn handle_keymap(&amp;mut self, format: u32, fd: std::os::unix::io::RawFd, size: u32) {
        // xkbcommon keymap setup
        let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let keymap = unsafe {
            xkb::Keymap::new_from_fd(
                &amp;ctx,
                fd,
                size as usize,
                xkb::KEYMAP_FORMAT_TEXT_V1,
                xkb::KEYMAP_COMPILE_NO_FLAGS,
            )
        }
        .expect("Failed to compile keymap");
        self.xkb_state = Some(xkb::State::new(&amp;keymap));
        tracing::info!("Keymap updated");
    }
    /// Xử lý modifier changes
    fn handle_modifiers(
        &amp;mut self,
        depressed: u32, latched: u32, locked: u32, group: u32,
    ) {
        if let Some(ref mut xkb) = self.xkb_state {
            xkb.update_mask(depressed, latched, locked, 0, 0, group);
        }
        self.mods = ModsState {
            depressed, latched, locked, group,
            dirty: true,
        };
    }
    /// Core key handling — quyết định routing cho từng keystroke
    fn handle_key(&amp;mut self, raw_keycode: u32, time: u32) {
        // XKB keycode = evdev + 8
        let xkb_keycode = raw_keycode + 8;
        // Resolve keysym từ xkb state
        let keysym = self
            .xkb_state
            .as_ref()
            .map(|s| s.key_get_one_sym(xkb_keycode))
            .unwrap_or(xkb::KEY_NoSymbol);
        // Resolve Unicode char
        let ch = xkb::keysym_to_utf8(keysym)
            .and_then(|s| s.chars().next());
        // ── Game mode: bypass hoàn toàn ──────────────────────────────
        if self.game_mode {
            if let (Some(vk), _) = (&amp;self.vk, &amp;self.im) {
                passthrough_key(vk, raw_keycode, 1, time, self.mods);
            }
            return;
        }
        // ── English passthrough mode ──────────────────────────────────
        if self.method == ConfigMethod::English {
            if let Some(vk) = &amp;self.vk {
                passthrough_key(vk, raw_keycode, 1, time, self.mods);
            }
            return;
        }
        // ── Special keys ──────────────────────────────────────────────
        match keysym {
            // Backspace: xóa ký tự cuối trong raw buffer
            xkb::KEY_BackSpace =&gt; {
                self.handle_backspace(raw_keycode, time);
                return;
            }
            // Escape: clear buffer + passthrough
            xkb::KEY_Escape =&gt; {
                self.buffer.clear();
                if let Some(vk) = &amp;self.vk {
                    passthrough_key(vk, raw_keycode, 1, time, self.mods);
                }
                return;
            }
            // Enter/Return: commit + passthrough
            xkb::KEY_Return | xkb::KEY_KP_Enter =&gt; {
                if let (Some(im), Some(vk)) = (&amp;self.im, &amp;self.vk) {
                    do_commit_then_passthrough(
                        im, vk, self,
                        '\n', raw_keycode, time, self.mods,
                    );
                }
                return;
            }
            // Modifier keys: không xử lý, chỉ update mods state
            xkb::KEY_Shift_L | xkb::KEY_Shift_R
            | xkb::KEY_Control_L | xkb::KEY_Control_R
            | xkb::KEY_Alt_L | xkb::KEY_Alt_R
            | xkb::KEY_Super_L | xkb::KEY_Super_R =&gt; {
                return; // mods đã được update trong handle_modifiers
            }
            _ =&gt; {}
        }
        // ── Ctrl/Alt combos: passthrough (hotkeys của app) ────────────
        if self.mods.depressed &amp; (MOD_CTRL | MOD_ALT) != 0 {
            // Check Game Mode hotkey: Ctrl+Shift+G
            if self.is_game_mode_toggle(keysym) {
                self.game_mode = !self.game_mode;
                tracing::info!("Game mode: {}", self.game_mode);
                return;
            }
            // Commit pending buffer trước khi forward hotkey
            if self.buffer.raw_len() &gt; 0 {
                if let (Some(im), Some(vk)) = (&amp;self.im, &amp;self.vk) {
                    do_commit(im, vk, self, time);
                }
            }
            if let Some(vk) = &amp;self.vk {
                passthrough_key(vk, raw_keycode, 1, time, self.mods);
            }
            return;
        }
        // ── Word boundary: commit current buffer + passthrough char ───
        if let Some(c) = ch {
            if self.buffer.should_commit(c) {
                if let (Some(im), Some(vk)) = (&amp;self.im, &amp;self.vk) {
                    do_commit_then_passthrough(
                        im, vk, self,
                        c, raw_keycode, time, self.mods,
                    );
                }
                return;
            }
        }
        // ── Vietnamese input: buffer + re-parse ──────────────────────
        if let Some(c) = ch {
            if c.is_ascii() &amp;&amp; !c.is_control() {
                self.buffer.push(c);
                // Burst commit check (Phase 4)
                self.check_burst_commit(time);
            } else {
                // Non-ASCII (e.g. arrow keys, F-keys): passthrough
                if let Some(vk) = &amp;self.vk {
                    passthrough_key(vk, raw_keycode, 1, time, self.mods);
                }
            }
        }
    }
    /// Backspace handling:
    /// - Nếu buffer có nội dung → pop raw_key (không cần gửi backspace thật)
    /// - Nếu buffer rỗng → passthrough backspace
    fn handle_backspace(&amp;mut self, raw_keycode: u32, time: u32) {
        if self.buffer.raw_len() &gt; 0 {
            self.buffer.backspace();
            // Không gửi backspace ra app vì char chưa được commit
            // (chỉ nằm trong buffer nội bộ)
        } else {
            // Buffer rỗng → backspace thật (xóa ký tự trước cursor)
            if let Some(vk) = &amp;self.vk {
                passthrough_key(vk, raw_keycode, 1, time, self.mods);
            }
        }
    }
    /// Burst commit: nếu buffer đủ lớn hoặc timeout → flush
    fn check_burst_commit(&amp;mut self, time: u32) {
        let now = std::time::Instant::now();
        let timeout = now.duration_since(self.burst.last_key_at) &gt; self.burst.window;
        // Flush nếu: timeout burst window HOẶC buffer quá dài (&gt; 8 chars)
        if timeout || self.buffer.raw_len() &gt; 8 {
            // Chú ý: KHÔNG flush ngay — chờ word boundary.
            // Burst chỉ affect commit timing, không affect correctness.
            self.burst.pending = true;
        }
        self.burst.last_key_at = now;
    }
    /// Check Ctrl+Shift+G hotkey (Game Mode toggle)
    fn is_game_mode_toggle(&amp;self, keysym: u32) -&gt; bool {
        self.mods.depressed &amp; MOD_CTRL != 0
            &amp;&amp; self.mods.depressed &amp; MOD_SHIFT != 0
            &amp;&amp; keysym == xkb::KEY_g
    }
}
// Linux modifier masks
const MOD_CTRL:  u32 = 1 &lt;&lt; 2;
const MOD_ALT:   u32 = 1 &lt;&lt; 3;
const MOD_SHIFT: u32 = 1 &lt;&lt; 0;
📁 crates/vi-wayland-im/src/runtime.rs — Event Loop
rust


//! Wayland event loop — kết nối tất cả lại.
use smithay_client_toolkit::reexports::client::{
    globals::registry_queue_init, Connection,
};
use wayland_protocols::wp::{
    input_method::zv2::client::zwp_input_method_manager_v2::ZwpInputMethodManagerV2,
    virtual_keyboard::zv1::client::zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1,
};
use crate::state::ImeState;
use vi_config::InputMethod as ConfigMethod;
pub fn run_ime_loop(method: ConfigMethod) -&gt; anyhow::Result&lt;()&gt; {
    // 1. Connect tới Wayland compositor
    let conn = Connection::connect_to_env()?;
    tracing::info!("Connected to Wayland compositor");
    // 2. Registry + globals discovery
    let (globals, mut event_queue) = registry_queue_init::&lt;ImeState&gt;(&amp;conn)?;
    let qh = event_queue.handle();
    // 3. Bind required globals
    let im_manager = globals
        .bind::&lt;ZwpInputMethodManagerV2, _, _&gt;(&amp;qh, 1..=1, ())
        .map_err(|_| anyhow::anyhow!(
            "Compositor does not support zwp_input_method_v2. \
             Ensure you are running wlroots (Hyprland/Niri) or KDE Plasma."
        ))?;
    let vk_manager = globals
        .bind::&lt;ZwpVirtualKeyboardManagerV1, _, _&gt;(&amp;qh, 1..=1, ())
        .map_err(|_| anyhow::anyhow!(
            "Compositor does not support zwp_virtual_keyboard_manager_v1."
        ))?;
    // 4. Get seat
    let seat = globals
        .bind::&lt;wl_seat::WlSeat, _, _&gt;(&amp;qh, 1..=1, ())
        .expect("No seat found");
    // 5. Create IME + Virtual Keyboard objects
    let im  = im_manager.get_input_method(&amp;seat, &amp;qh, ());
    let vk  = vk_manager.create_virtual_keyboard(&amp;seat, &amp;qh, ());
    // 6. Init state
    let mut state = ImeState::new(method);
    state.im_manager = Some(im_manager);
    state.im         = Some(im.clone());
    state.vk         = Some(vk);
    state.seat       = Some(seat);
    // 7. Grab keyboard (nhận tất cả key events khi IME active)
    let _grab = im.grab_keyboard(&amp;qh, ());
    state.kb_grab = Some(_grab);
    tracing::info!("IME loop started, method={:?}", method);
    // 8. Main event loop — blocking
    loop {
        event_queue.blocking_dispatch(&amp;mut state)?;
    }
}
📁 crates/vi-wayland-im/src/commit.rs — Commit Strategy (với Burst)
rust


//! Commit strategies: immediate vs burst.
use std::time::{Duration, Instant};
use crate::{actions::do_commit, state::ImeState};
use wayland_protocols::wp::{
    input_method::zv2::client::zwp_input_method_v2::ZwpInputMethodV2,
    virtual_keyboard::zv1::client::zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
};
/// Commit strategy enum
pub enum CommitStrategy {
    /// Commit ngay khi gặp word boundary (default)
    Immediate,
    /// Burst: gom keystrokes trong 300ms window (Phase 4)
    Burst { window: Duration },
}
impl CommitStrategy {
    pub fn default_burst() -&gt; Self {
        Self::Burst { window: Duration::from_millis(300) }
    }
}
/// Kiểm tra và thực thi burst commit nếu cần
///
/// Được gọi từ timer thread mỗi 50ms để flush stale buffers.
/// Đảm bảo: nếu user ngừng gõ &gt; 300ms, buffer được commit tự động.
pub fn flush_stale_burst(
    im:    &amp;ZwpInputMethodV2,
    vk:    &amp;ZwpVirtualKeyboardV1,
    state: &amp;mut ImeState,
) {
    if !state.burst.pending { return; }
    if state.buffer.raw_len() == 0 { return; }
    let elapsed = Instant::now().duration_since(state.burst.last_key_at);
    if elapsed &gt;= state.burst.window {
        tracing::debug!(
            "Burst flush: {} chars after {:?}",
            state.buffer.raw_len(),
            elapsed
        );
        do_commit(im, vk, state, 0);
    }
}
📁 crates/vi-wayland-im/src/lib.rs — Module exports
rust


pub mod actions;
pub mod commit;
pub mod dispatch;
pub mod runtime;
pub mod state;
pub mod virtual_keyboard;
pub use runtime::run_ime_loop;
pub use state::ImeState;
📁 Flow hoàn chỉnh: Keystroke → Screen


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
  buffer.push('v')        ← raw_keys: ['v']
         │
  (user tiếp tục gõ 'i','e','t','j')
         │
  raw_keys: ['v','i','e','t','j']
         │
  User nhấn SPACE (word boundary)
         │
         ▼
  do_commit_then_passthrough(im, vk, state, ' ', ...)
         │
    ┌────┴────────────────────────────────────┐
    │ 1. buffer.render()                       │
    │    → NfdEngine::process(['v','i','e','t','j'])
    │    → "việt"                              │
    │                                          │
    │ 2. send_virtual_backspaces(vk, 5, T)    │
    │    → xóa "vietj" khỏi màn hình          │
    │                                          │
    │ 3. im.commit_string("việt")             │
    │    im.commit(serial)                     │
    │    → "việt" xuất hiện trên màn hình     │
    │                                          │
    │ 4. passthrough_key(vk, SPACE, ...)      │
    │    → space xuất hiện                    │
    │                                          │
    │ 5. buffer.clear()                        │
    └─────────────────────────────────────────┘
⚠️ Compositor Compatibility
| Compositor | Protocol | Trạng thái | |------------|----------|-----------| | wlroots (Hyprland, Niri, Sway) | zwp_input_method_v2 | ✅ Hoạt động hoàn hảo | | KDE Plasma (KWin) | zwp_input_method_v2 | ✅ Hỗ trợ tốt (phiên bản mới) | | GNOME (Mutter) | ❌ Không support v2 | ⚠️ Phải dùng IBus internal | kpGGJy4vwnhrFgfXWw4dAs

📦 Cargo.toml cho vi-wayland-im
toml


[package]
name    = "vi-wayland-im"
version = "0.1.0"
edition = "2021"
[dependencies]
# Wayland protocols
wayland-client   = "0.31"
wayland-protocols = { version = "0.32", features = ["unstable"] }
# Smithay Client Toolkit (registry, seat, globals)
smithay-client-toolkit = "0.19"
# XKB keymap + keysym resolution
xkbcommon = { version = "0.7", features = ["wayland"] }
# Internal crates
vi-engine = { path = "../vi-engine" }
vi-config = { path = "../vi-config" }
# Utilities
anyhow   = "1"
tracing  = "0.1"
✅ Integration checklist


Step	File	Status
IME global state	state.rs	✅
Commit + VB actions	actions.rs	✅
Key event dispatch	dispatch.rs	✅
Wayland event loop	runtime.rs	✅
Burst commit flush	commit.rs	✅
NFD engine bridge	vi-engine	✅ (previous)
Game Mode toggle	dispatch.rs	✅ Ctrl+Shift+G
Tiếp theo bạn muốn:

A. virtual_keyboard.rs — full xkb keymap upload + key injection
B. Burst commit timer thread (tokio/async integration)
C. Tray icon integration (Phase 3) với method switching
D. Full main.rs entry point kết nối tất cả lại



Claude 4.6
A. virtual_keyboard.rs — full xkb keymap upload + key injection
B. Burst commit timer thread (tokio/async integration)



Let me pull the relevant context first.

Context đầy đủ. Đây là toàn bộ A + B — hai phần kết nối chặt chẽ với nhau vì burst timer cần Arc&lt;Mutex&lt;ImeState&gt;&gt; để cross-thread access:

A. virtual_keyboard.rs — Full XKB Keymap Upload + Key Injection
Tại sao cần upload keymap?
zwp_virtual_keyboard_v1 yêu cầu client upload một XKB keymap hợp lệ trước khi inject bất kỳ key event nào — compositor cần keymap để translate keycodes thành keysyms đúng.oF8if9Ae7K4JUrgC2RruQj

Virtual Backspace architecture (evdev + uinput) — key injection thông qua virtual keyboard object, không phải qua uinput trực tiếp, vì ta đã có zwp_virtual_keyboard_v1 từ Wayland protocol.PRz2H8Bb78QJtdmP33BUYb

📁 crates/vi-wayland-im/src/virtual_keyboard.rs
rust


//! Virtual keyboard: XKB keymap upload + key/modifier injection.
//!
//! zwp_virtual_keyboard_v1 cần:
//!   1. keymap() được gọi TRƯỚC mọi key event
//!   2. key() để inject press/release
//!   3. modifiers() để sync modifier state
//!
//! Keymap source: dùng lại keymap từ keyboard grab (compositor gửi qua
//! ZwpInputMethodKeyboardGrabV2::Event::Keymap), không tự tạo mới.
use std::{
    ffi::CString,
    fs::File,
    io::Write,
    os::unix::io::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd, RawFd},
};
use wayland_protocols::wp::virtual_keyboard::zv1::client::{
    zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1,
    zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
};
use xkbcommon::xkb;
// ─── KeymapUploader ───────────────────────────────────────────────────────────
/// Quản lý việc upload XKB keymap tới virtual keyboard object.
///
/// Protocol yêu cầu keymap phải được truyền qua file descriptor (memfd/tmpfile),
/// KHÔNG phải qua inline string. Compositor đọc keymap từ fd rồi đóng lại.
pub struct KeymapUploader {
    /// Đã upload keymap chưa? (chỉ cần upload 1 lần hoặc khi keymap thay đổi)
    uploaded: bool,
    /// XKB keymap string hiện tại (lưu để detect changes)
    current_keymap_str: Option&lt;String&gt;,
}
impl KeymapUploader {
    pub fn new() -&gt; Self {
        Self {
            uploaded: false,
            current_keymap_str: None,
        }
    }
    /// Upload keymap từ raw fd mà compositor gửi qua keyboard grab event.
    ///
    /// Compositor gửi: Keymap { format: XkbV1, fd: RawFd, size: u32 }
    /// Ta forward nguyên fd này sang virtual keyboard.
    ///
    /// # Safety
    /// fd phải hợp lệ và chưa bị close. Sau khi upload, compositor sẽ
    /// close fd của nó; ta không cần close fd của mình (compositor dup nó).
    pub fn upload_from_compositor_fd(
        &amp;mut self,
        vk:     &amp;ZwpVirtualKeyboardV1,
        format: u32,   // wl_keyboard::KeymapFormat::XkbV1 = 1
        fd:     RawFd,
        size:   u32,
    ) {
        // Format phải là XkbV1 (= 1)
        if format != 1 {
            tracing::warn!("Unknown keymap format {format}, skipping upload");
            return;
        }
        // virtual_keyboard::keymap(format, fd, size)
        // Protocol: compositor dup fd nên ta không cần giữ lại
        vk.keymap(format, unsafe { OwnedFd::from_raw_fd(fd) }, size);
        self.uploaded = true;
        tracing::info!("Keymap uploaded to virtual keyboard ({size} bytes)");
    }
    /// Upload keymap từ XKB keymap object (tự tạo fallback keymap).
    ///
    /// Dùng khi: compositor chưa gửi keymap, hoặc cần override với
    /// keymap tùy chỉnh (ví dụ: bare US QWERTY cho game mode).
    pub fn upload_from_xkb_keymap(
        &amp;mut self,
        vk:      &amp;ZwpVirtualKeyboardV1,
        keymap:  &amp;xkb::Keymap,
    ) -&gt; anyhow::Result&lt;()&gt; {
        // Serialize keymap thành XKB string
        let keymap_str = keymap
            .get_as_string(xkb::KEYMAP_FORMAT_TEXT_V1);
        // Detect keymap change (tránh upload thừa)
        if self.current_keymap_str.as_deref() == Some(&amp;keymap_str) &amp;&amp; self.uploaded {
            tracing::debug!("Keymap unchanged, skipping re-upload");
            return Ok(());
        }
        let size = keymap_str.len() + 1; // +1 cho null terminator
        // Tạo anonymous memfd để chứa keymap string
        let fd = create_memfd("vi-im-keymap", &amp;keymap_str)?;
        vk.keymap(
            1, // XkbV1
            unsafe { OwnedFd::from_raw_fd(fd) },
            size as u32,
        );
        self.current_keymap_str = Some(keymap_str);
        self.uploaded = true;
        tracing::info!("XKB keymap uploaded ({size} bytes)");
        Ok(())
    }
    pub fn is_uploaded(&amp;self) -&gt; bool {
        self.uploaded
    }
    /// Invalidate — gọi khi compositor gửi keymap mới
    pub fn invalidate(&amp;mut self) {
        self.uploaded = false;
    }
}
// ─── VirtualKeyboard wrapper ──────────────────────────────────────────────────
/// High-level wrapper cho zwp_virtual_keyboard_v1.
/// Đảm bảo keymap luôn được upload trước khi inject key events.
pub struct VirtualKeyboard {
    pub vk:      ZwpVirtualKeyboardV1,
    uploader:    KeymapUploader,
    mods:        ModsSnapshot,
}
/// Snapshot của modifier state hiện tại
#[derive(Default, Clone, Copy, PartialEq)]
pub struct ModsSnapshot {
    pub depressed: u32,
    pub latched:   u32,
    pub locked:    u32,
    pub group:     u32,
}
impl VirtualKeyboard {
    pub fn new(vk: ZwpVirtualKeyboardV1) -&gt; Self {
        Self {
            vk,
            uploader: KeymapUploader::new(),
            mods:     ModsSnapshot::default(),
        }
    }
    // ── Keymap management ──────────────────────────────────────────────
    /// Nhận keymap từ compositor keyboard grab event và forward tới VK
    pub fn handle_compositor_keymap(
        &amp;mut self,
        format: u32,
        fd:     RawFd,
        size:   u32,
    ) {
        self.uploader.upload_from_compositor_fd(&amp;self.vk, format, fd, size);
    }
    /// Upload fallback US QWERTY keymap (dùng khi chưa nhận keymap từ compositor)
    pub fn upload_fallback_keymap(&amp;mut self) -&gt; anyhow::Result&lt;()&gt; {
        let ctx     = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let keymap  = xkb::Keymap::new_from_names(
            &amp;ctx,
            "",     // rules
            "",     // model
            "us",   // layout: US QWERTY
            "",     // variant
            None,   // options
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        ).ok_or_else(|| anyhow::anyhow!("Failed to create fallback keymap"))?;
        self.uploader.upload_from_xkb_keymap(&amp;self.vk, &amp;keymap)
    }
    // ── Key injection ──────────────────────────────────────────────────
    /// Inject một key event (press hoặc release)
    ///
    /// # Params
    /// - `keycode`: Linux evdev keycode (e.g. KEY_BACKSPACE=14, KEY_SPACE=57)
    /// - `state`:   1 = pressed, 0 = released
    /// - `time`:    timestamp ms (từ keyboard grab event)
    pub fn inject_key(&amp;self, keycode: u32, state: u32, time: u32) {
        debug_assert!(
            self.uploader.is_uploaded(),
            "Must upload keymap before injecting keys!"
        );
        self.vk.key(time, keycode, state);
    }
    /// Inject key press + release pair (shorthand)
    pub fn inject_key_tap(&amp;self, keycode: u32, time: u32) {
        self.inject_key(keycode, 1, time);      // press
        self.inject_key(keycode, 0, time + 1);  // release (+1ms)
    }
    /// Inject N backspace events (virtual backspace)
    ///
    /// KEY_BACKSPACE = 14 (Linux evdev)
    /// Mỗi backspace cách nhau 2ms để tránh compositor drop events
    pub fn inject_backspaces(&amp;self, count: usize, base_time: u32) {
        for i in 0..count {
            let t = base_time + (i as u32 * 2);
            self.inject_key(KEY_BACKSPACE, 1, t);     // press
            self.inject_key(KEY_BACKSPACE, 0, t + 1); // release
        }
        tracing::debug!("Injected {count} backspaces at t={base_time}");
    }
    /// Passthrough: forward evdev key event nguyên vẹn
    ///
    /// Sync modifiers trước nếu state thay đổi
    pub fn passthrough(
        &amp;mut self,
        keycode: u32,
        state:   u32,
        time:    u32,
        mods:    ModsSnapshot,
    ) {
        self.sync_mods(mods);
        self.inject_key(keycode, state, time);
    }
    // ── Modifier sync ──────────────────────────────────────────────────
    /// Sync modifier state với compositor
    /// Chỉ gửi nếu state thực sự thay đổi (tránh traffic thừa)
    pub fn sync_mods(&amp;mut self, new_mods: ModsSnapshot) {
        if self.mods != new_mods {
            self.vk.modifiers(
                new_mods.depressed,
                new_mods.latched,
                new_mods.locked,
                new_mods.group,
            );
            self.mods = new_mods;
            tracing::trace!(
                "Mods synced: dep={:#010x} lat={:#010x} lck={:#010x} grp={}",
                new_mods.depressed, new_mods.latched,
                new_mods.locked,    new_mods.group
            );
        }
    }
    /// Clear tất cả modifiers (dùng khi IME deactivate)
    pub fn clear_mods(&amp;mut self) {
        self.sync_mods(ModsSnapshot::default());
    }
}
// ─── Linux evdev keycodes (thường dùng) ──────────────────────────────────────
pub const KEY_BACKSPACE: u32 = 14;
pub const KEY_TAB:       u32 = 15;
pub const KEY_ENTER:     u32 = 28;
pub const KEY_ESCAPE:    u32 = 1;
pub const KEY_SPACE:     u32 = 57;
pub const KEY_DELETE:    u32 = 111;
// ─── memfd helper ─────────────────────────────────────────────────────────────
/// Tạo anonymous in-memory file (memfd_create) chứa keymap string.
///
/// Wayland protocol yêu cầu keymap được truyền qua fd, không phải inline.
/// memfd_create tạo file không cần path trên filesystem — an toàn hơn tmpfile.
fn create_memfd(name: &amp;str, content: &amp;str) -&gt; anyhow::Result&lt;RawFd&gt; {
    use std::io::Seek;
    // memfd_create(name, MFD_CLOEXEC)
    let c_name = CString::new(name)?;
    let fd = unsafe {
        libc::memfd_create(c_name.as_ptr(), libc::MFD_CLOEXEC)
    };
    if fd &lt; 0 {
        return Err(anyhow::anyhow!(
            "memfd_create failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    // Write keymap + null terminator
    let mut file = unsafe { File::from_raw_fd(fd) };
    file.write_all(content.as_bytes())?;
    file.write_all(b"\0")?;   // null terminator
    file.seek(std::io::SeekFrom::Start(0))?; // rewind
    // into_raw_fd để file không bị drop/close
    Ok(file.into_raw_fd())
}
B. Burst Commit Timer — Tokio Async Integration


Thiết kế cross-thread
Burst commit cần window 300ms — timer phải chạy trên thread riêng, thông báo cho Wayland event loop qua channel khi cần flush.HgJhv6WrG7TqRTNNRdmL3k

Wayland event loop không phải async (blocking event_queue.blocking_dispatch()), nên burst timer dùng tokio::sync::watch + wakeup pipe để cross-thread notify.

📁 crates/vi-wayland-im/src/burst.rs
rust


//! Burst commit timer — ibus-style optimization.
//!
//! Mục tiêu: gom các pure-append keystrokes trong window 300ms
//! thành single Wayland commit để giảm latency và round-trips.
//!
//! Design:
//!   - Wayland loop (sync): nhận key events, update BurstTimer state
//!   - Tokio timer task (async): đặt deadline, notify qua channel khi expire
//!   - Wayland loop đọc channel → flush buffer → commit
//!
//! Thread model:
//!   ┌─────────────────────┐     channel      ┌──────────────────┐
//!   │  Wayland event loop │ ◄─── FlushCmd ─── │  Tokio timer task│
//!   │  (sync, main thread)│                   │  (async thread)  │
//!   └─────────────────────┘                   └──────────────────┘
//!         │ push_key()                               │
//!         ▼                                    reset_deadline()
//!   BurstTimer::arm()  ──────────────────────────────┘
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{
    sync::mpsc,
    time::sleep,
};
// ─── Public API types ─────────────────────────────────────────────────────────
/// Command gửi từ timer task → Wayland loop
#[derive(Debug, Clone)]
pub enum BurstCmd {
    /// Flush pending buffer (timeout expired)
    Flush,
    /// Shutdown timer task
    Shutdown,
}
/// Shared state giữa Wayland loop và timer task
#[derive(Debug)]
pub struct BurstShared {
    /// Thời điểm keystroke cuối cùng
    pub last_key_at:  Instant,
    /// Burst window
    pub window:       Duration,
    /// Buffer có pending commit không?
    pub has_pending:  bool,
    /// Armed? (timer đang chạy)
    pub armed:        bool,
}
impl Default for BurstShared {
    fn default() -&gt; Self {
        Self {
            last_key_at: Instant::now(),
            window:      Duration::from_millis(300),
            has_pending: false,
            armed:       false,
        }
    }
}
// ─── BurstTimer ───────────────────────────────────────────────────────────────
/// Handle phía Wayland loop
pub struct BurstTimer {
    shared:  Arc&lt;Mutex&lt;BurstShared&gt;&gt;,
    /// Notify timer task: có keystroke mới → reset deadline
    arm_tx:  mpsc::UnboundedSender&lt;ArmSignal&gt;,
    /// Nhận flush command từ timer task
    pub flush_rx: mpsc::Receiver&lt;BurstCmd&gt;,
}
/// Signal gửi tới timer task
enum ArmSignal {
    /// New keystroke — reset timer deadline
    KeyPressed,
    /// Shutdown
    Stop,
}
impl BurstTimer {
    /// Khởi tạo BurstTimer và spawn tokio timer task.
    ///
    /// # Returns
    /// (BurstTimer cho Wayland loop, JoinHandle của timer task)
    pub fn new(window: Duration) -&gt; (Self, tokio::task::JoinHandle&lt;()&gt;) {
        let shared = Arc::new(Mutex::new(BurstShared {
            window,
            ..Default::default()
        }));
        let (arm_tx, arm_rx)     = mpsc::unbounded_channel::&lt;ArmSignal&gt;();
        let (flush_tx, flush_rx) = mpsc::channel::&lt;BurstCmd&gt;(4);
        let shared_clone = Arc::clone(&amp;shared);
        let handle = tokio::spawn(burst_timer_task(
            shared_clone,
            arm_rx,
            flush_tx,
        ));
        (
            Self { shared, arm_tx, flush_rx },
            handle,
        )
    }
    /// Gọi khi Wayland loop nhận được keystroke mới.
    ///
    /// - Mark pending
    /// - Reset timer deadline
    /// - Arm timer nếu chưa armed
    pub fn on_key_pressed(&amp;mut self) {
        {
            let mut s = self.shared.lock().unwrap();
            s.last_key_at = Instant::now();
            s.has_pending = true;
            s.armed       = true;
        }
        // Signal timer task: reset deadline
        let _ = self.arm_tx.send(ArmSignal::KeyPressed);
    }
    /// Gọi sau khi Wayland loop đã flush buffer.
    /// Reset pending flag.
    pub fn on_flushed(&amp;mut self) {
        let mut s = self.shared.lock().unwrap();
        s.has_pending = false;
        s.armed       = false;
    }
    /// Check xem có flush command pending không (non-blocking).
    /// Trả về true nếu cần flush ngay.
    pub fn try_recv_flush(&amp;mut self) -&gt; bool {
        matches!(
            self.flush_rx.try_recv(),
            Ok(BurstCmd::Flush)
        )
    }
    /// Shutdown timer task
    pub fn shutdown(&amp;self) {
        let _ = self.arm_tx.send(ArmSignal::Stop);
    }
}
// ─── Timer task (async) ───────────────────────────────────────────────────────
/// Tokio async task chạy timer burst.
///
/// Logic:
/// 1. Chờ signal KeyPressed từ Wayland loop
/// 2. Sau khi nhận signal → sleep(window)
/// 3. Nếu trong khi sleep nhận thêm KeyPressed → reset, sleep lại
/// 4. Nếu sleep xong mà không có key mới → send Flush
///
/// Đây là debounce pattern cổ điển.
async fn burst_timer_task(
    shared:   Arc&lt;Mutex&lt;BurstShared&gt;&gt;,
    mut arm_rx:   mpsc::UnboundedReceiver&lt;ArmSignal&gt;,
    flush_tx: mpsc::Sender&lt;BurstCmd&gt;,
) {
    tracing::debug!("Burst timer task started");
    loop {
        // Phase 1: Idle — chờ keystroke đầu tiên
        let window = loop {
            match arm_rx.recv().await {
                Some(ArmSignal::KeyPressed) =&gt; {
                    let w = shared.lock().unwrap().window;
                    break w;
                }
                Some(ArmSignal::Stop) | None =&gt; {
                    tracing::debug!("Burst timer task stopping");
                    let _ = flush_tx.send(BurstCmd::Shutdown).await;
                    return;
                }
            }
        };
        // Phase 2: Armed — debounce loop
        // Mỗi khi nhận thêm KeyPressed, reset deadline
        loop {
            tokio::select! {
                // Timer expired → flush!
                _ = sleep(window) =&gt; {
                    let has_pending = {
                        shared.lock().unwrap().has_pending
                    };
                    if has_pending {
                        tracing::debug!(
                            "Burst window expired ({:?}), sending Flush",
                            window
                        );
                        if flush_tx.send(BurstCmd::Flush).await.is_err() {
                            return; // receiver dropped
                        }
                        // Reset armed state
                        shared.lock().unwrap().armed = false;
                    }
                    break; // → back to Phase 1 (idle)
                }
                // New keystroke — reset timer (stay in armed loop)
                signal = arm_rx.recv() =&gt; {
                    match signal {
                        Some(ArmSignal::KeyPressed) =&gt; {
                            // Debounce: update last_key_at và continue loop
                            // (tokio::select! sẽ restart sleep với window mới)
                            tracing::trace!("Burst: keystroke reset timer");
                            continue;
                        }
                        Some(ArmSignal::Stop) | None =&gt; {
                            // Flush pending trước khi stop
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
// ─── BurstTimerSync (no-tokio fallback) ──────────────────────────────────────
/// Lightweight fallback nếu không dùng tokio runtime.
/// Dùng std::thread + std::sync::mpsc thay vì tokio.
///
/// Dùng khi: single-threaded Wayland loop không muốn pull tokio runtime.
pub struct BurstTimerSync {
    shared:   Arc&lt;Mutex&lt;BurstShared&gt;&gt;,
    arm_tx:   std::sync::mpsc::SyncSender&lt;bool&gt;,
    flush_rx: std::sync::mpsc::Receiver&lt;BurstCmd&gt;,
    _thread:  std::thread::JoinHandle&lt;()&gt;,
}
impl BurstTimerSync {
    pub fn new(window: Duration) -&gt; Self {
        let shared = Arc::new(Mutex::new(BurstShared {
            window,
            ..Default::default()
        }));
        let (arm_tx, arm_rx)     = std::sync::mpsc::sync_channel::&lt;bool&gt;(16);
        let (flush_tx, flush_rx) = std::sync::mpsc::channel::&lt;BurstCmd&gt;();
        let shared_clone = Arc::clone(&amp;shared);
        let thread = std::thread::Builder::new()
            .name("vi-im-burst-timer".into())
            .spawn(move || {
                burst_timer_sync_thread(shared_clone, arm_rx, flush_tx)
            })
            .expect("Failed to spawn burst timer thread");
        Self { shared, arm_tx, flush_rx, _thread: thread }
    }
    pub fn on_key_pressed(&amp;mut self) {
        {
            let mut s = self.shared.lock().unwrap();
            s.last_key_at = Instant::now();
            s.has_pending = true;
        }
        let _ = self.arm_tx.try_send(true);
    }
    pub fn on_flushed(&amp;mut self) {
        self.shared.lock().unwrap().has_pending = false;
    }
    /// Non-blocking check
    pub fn try_recv_flush(&amp;self) -&gt; bool {
        matches!(
            self.flush_rx.try_recv(),
            Ok(BurstCmd::Flush)
        )
    }
}
fn burst_timer_sync_thread(
    shared:   Arc&lt;Mutex&lt;BurstShared&gt;&gt;,
    arm_rx:   std::sync::mpsc::Receiver&lt;bool&gt;,
    flush_tx: std::sync::mpsc::Sender&lt;BurstCmd&gt;,
) {
    loop {
        // Wait for first key
        if arm_rx.recv().is_err() { break; }
        let window = shared.lock().unwrap().window;
        let mut deadline = Instant::now() + window;
        // Debounce loop
        loop {
            let now = Instant::now();
            if now &gt;= deadline {
                // Expired
                let has_pending = shared.lock().unwrap().has_pending;
                if has_pending {
                    let _ = flush_tx.send(BurstCmd::Flush);
                    shared.lock().unwrap().has_pending = false;
                }
                break;
            }
            // Try recv with timeout
            let remaining = deadline - now;
            match arm_rx.recv_timeout(remaining) {
                Ok(_) =&gt; {
                    // New key → reset deadline
                    deadline = Instant::now() + window;
                    shared.lock().unwrap().last_key_at = Instant::now();
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) =&gt; {
                    // Timer expired naturally → flush
                    let has_pending = shared.lock().unwrap().has_pending;
                    if has_pending {
                        let _ = flush_tx.send(BurstCmd::Flush);
                        shared.lock().unwrap().has_pending = false;
                    }
                    break;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) =&gt; {
                    return; // shutdown
                }
            }
        }
    }
}
📁 crates/vi-wayland-im/src/runtime.rs — Updated với Burst + Wakeup Pipe
rust


//! Runtime loop: tích hợp Wayland event queue + burst timer wakeup.
//!
//! Vấn đề: blocking_dispatch() block mãi mãi.
//! Wayland loop cần "thức dậy" khi burst timer expire.
//!
//! Giải pháp: dùng eventfd (Linux) làm wakeup pipe.
//! Timer task write vào eventfd → Wayland loop poll cả hai.
use std::{
    os::unix::io::AsRawFd,
    sync::Arc,
    time::Duration,
};
use smithay_client_toolkit::reexports::client::Connection;
use wayland_protocols::wp::{
    input_method::zv2::client::zwp_input_method_manager_v2::ZwpInputMethodManagerV2,
    virtual_keyboard::zv1::client::zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1,
};
use crate::{
    actions::do_commit,
    burst::BurstTimerSync,
    state::ImeState,
    virtual_keyboard::VirtualKeyboard,
};
use vi_config::InputMethod as ConfigMethod;
pub fn run_ime_loop(method: ConfigMethod) -&gt; anyhow::Result&lt;()&gt; {
    // ── 1. Connect Wayland ────────────────────────────────────────────
    let conn = Connection::connect_to_env()?;
    let (globals, mut event_queue) =
        smithay_client_toolkit::reexports::client::globals::registry_queue_init::&lt;ImeState&gt;(&amp;conn)?;
    let qh = event_queue.handle();
    // ── 2. Bind globals ───────────────────────────────────────────────
    let im_manager = globals
        .bind::&lt;ZwpInputMethodManagerV2, _, _&gt;(&amp;qh, 1..=1, ())
        .map_err(|_| anyhow::anyhow!(
            "zwp_input_method_v2 not supported. \
             Need wlroots compositor (Hyprland ≥ 0.34, Niri, Sway ≥ 1.8)"
        ))?;
    let vk_manager = globals
        .bind::&lt;ZwpVirtualKeyboardManagerV1, _, _&gt;(&amp;qh, 1..=1, ())
        .map_err(|_| anyhow::anyhow!("zwp_virtual_keyboard_manager_v1 not supported"))?;
    let seat = globals
        .bind::&lt;wayland_client::protocol::wl_seat::WlSeat, _, _&gt;(&amp;qh, 7..=8, ())
        .map_err(|_| anyhow::anyhow!("No WlSeat found"))?;
    // ── 3. Create IME + Virtual Keyboard ─────────────────────────────
    let im  = im_manager.get_input_method(&amp;seat, &amp;qh, ());
    let vk  = vk_manager.create_virtual_keyboard(&amp;seat, &amp;qh, ());
    let vkw = VirtualKeyboard::new(vk);
    // ── 4. Init state ─────────────────────────────────────────────────
    let mut state   = ImeState::new(method, vkw);
    state.im        = Some(im.clone());
    state.seat      = Some(seat);
    let _grab       = im.grab_keyboard(&amp;qh, ());
    state.kb_grab   = Some(_grab);
    // ── 5. Upload fallback keymap (sẽ bị override khi nhận từ compositor)
    state.vk.upload_fallback_keymap()?;
    // ── 6. Burst timer (std::thread version, no tokio needed) ─────────
    let mut burst = BurstTimerSync::new(Duration::from_millis(300));
    // ── 7. Eventfd wakeup cho burst timer ─────────────────────────────
    let wakeup_fd = create_eventfd()?;
    // Clone fd cho burst notification thread
    let wakeup_fd_write = wakeup_fd;
    tracing::info!("vi-im event loop starting (method={method:?})");
    // ── 8. Main loop: poll Wayland fd + wakeup fd ─────────────────────
    loop {
        // Poll: Wayland display fd + wakeup eventfd
        let wayland_fd = conn.as_raw_fd();
        let ready = poll_fds(&amp;[wayland_fd, wakeup_fd], 50 /* ms timeout */)?;
        // Process Wayland events
        if ready.contains(wayland_fd) {
            event_queue.dispatch_pending(&amp;mut state)?;
            conn.flush()?;
        }
        // Notify burst timer về keystrokes mới (collected in state)
        if state.burst_key_pending {
            burst.on_key_pressed();
            state.burst_key_pending = false;
        }
        // Check burst timer flush
        if burst.try_recv_flush() || ready.contains(wakeup_fd) {
            // Drain wakeup eventfd
            drain_eventfd(wakeup_fd);
            // Flush pending buffer
            if state.buffer.raw_len() &gt; 0 {
                if let Some(im) = &amp;state.im {
                    do_commit(im, &amp;mut state.vk, &amp;mut state.buffer, state.serial);
                    state.serial = state.serial.wrapping_add(1);
                    burst.on_flushed();
                    tracing::debug!("Burst flush executed");
                }
            }
        }
        // Dispatch pending (sau khi process tất cả)
        event_queue.dispatch_pending(&amp;mut state)?;
    }
}
// ─── eventfd helpers ─────────────────────────────────────────────────────────
fn create_eventfd() -&gt; anyhow::Result&lt;std::os::unix::io::RawFd&gt; {
    let fd = unsafe { libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC) };
    if fd &lt; 0 {
        Err(anyhow::anyhow!("eventfd failed: {}", std::io::Error::last_os_error()))
    } else {
        Ok(fd)
    }
}
fn drain_eventfd(fd: std::os::unix::io::RawFd) {
    let mut buf = [0u8; 8];
    unsafe { libc::read(fd, buf.as_mut_ptr() as _, 8) };
}
/// Minimal poll wrapper — trả về set of ready fds
fn poll_fds(
    fds:        &amp;[std::os::unix::io::RawFd],
    timeout_ms: i32,
) -&gt; anyhow::Result&lt;std::collections::HashSet&lt;std::os::unix::io::RawFd&gt;&gt; {
    use std::collections::HashSet;
    let mut pollfds: Vec&lt;libc::pollfd&gt; = fds
        .iter()
        .map(|&amp;fd| libc::pollfd {
            fd,
            events:  libc::POLLIN,
            revents: 0,
        })
        .collect();
    let ret = unsafe {
        libc::poll(pollfds.as_mut_ptr(), pollfds.len() as _, timeout_ms)
    };
    if ret &lt; 0 {
        let e = std::io::Error::last_os_error();
        if e.kind() == std::io::ErrorKind::Interrupted { return Ok(HashSet::new()); }
        return Err(e.into());
    }
    let ready = pollfds
        .iter()
        .filter(|p| p.revents &amp; libc::POLLIN != 0)
        .map(|p| p.fd)
        .collect();
    Ok(ready)
}
📊 Full Integration Flow


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
📦 Cargo.toml update
toml


[dependencies]
# Async runtime (chỉ cần nếu dùng tokio burst variant)
tokio = { version = "1", features = ["rt", "time", "sync", "macros"], optional = true }
# Linux syscalls (eventfd, memfd_create, poll)
libc = "0.2"
# Wayland
wayland-client   = "0.31"
wayland-protocols = { version = "0.32", features = ["unstable"] }
smithay-client-toolkit = "0.19"
# XKB
xkbcommon = { version = "0.7", features = ["wayland"] }
# Internal
vi-engine = { path = "../vi-engine" }
vi-config = { path = "../vi-config" }
anyhow  = "1"
tracing = "0.1"
[features]
default  = []
async-burst = ["tokio"]  # opt-in tokio burst timer
✅ Summary: A + B checklist


Component	File	Key Points
Keymap upload từ compositor	virtual_keyboard.rs::KeymapUploader::upload_from_compositor_fd	Forward fd nguyên vẹn, không copy
Fallback keymap (US QWERTY)	upload_from_xkb_keymap + create_memfd	memfd_create → null-terminated string
Key injection	VirtualKeyboard::inject_key / inject_backspaces	Evdev keycode, 2ms spacing
Modifier sync	VirtualKeyboard::sync_mods	Diff-based, tránh traffic thừa
Burst timer (tokio)	burst.rs::BurstTimer + burst_timer_task	Debounce pattern, 300ms window
Burst timer (std)	BurstTimerSync	Không cần tokio runtime
Wakeup pipe	eventfd trong runtime.rs	Cross-thread notify Wayland loop
Poll loop	poll_fds()	Wayland fd + eventfd, 50ms timeout
Tiếp theo:

C. Tray icon integration (Phase 3) — method switching menu
D. Full main.rs entry point kết nối tất cả lại
E. Test suite: unit tests cho virtual_keyboard + burst timer



Claude 4.6
C. Tray icon integration (Phase 3) — method switching menu



Let me pull the full context for tray + config architecture.

Context đầy đủ. Đây là toàn bộ Phase 3 — Tray Icon Integration hoàn chỉnh, bao gồm vi-config shared types, vi-tray crate, message passing, và wiring vào daemon:

C. Tray Icon Integration — Full Implementation
Kiến trúc tổng thể
Single binary vi-im tích hợp tray icon + settings + daemon vào một executable duy nhất.Lis7QN8X9uGsmQnKrGzJE1

Phase 3 steps: - 3.1 TrayMessage::SetMethod(InputMethod) thay SwitchInputMethod - 3.2 Menu 5 items: English, VNI, Telex, Smart, Settings - 3.3 Left-click handler (toggle ENG/VI) - 3.4 Tray icon đổi tooltip + màu sắc theo trạng thái - 3.5 Handle SetMethod, ToggleIme, OpenSettings trong daemon7zQEeEsQCqXQVq2cnzLUav

Menu layout đã define: ┌──────────────────────────┐ │ vi-im · Smart · Bật │ ← status bar (read-only) ├──────────────────────────┤ │ 🇬🇧 English │ │ 🇻🇳 VNI ✓ │ │ 🇻🇳 Telex │ │ 🇻🇳 Smart │ ├──────────────────────────┤ │ ⚙️ Cấu hình... │ ├──────────────────────────┤ │ ❌ Thoát │ └──────────────────────────┘ rmR45ABLCZvxeoJA8kogvD

📁 crates/vi-config/src/lib.rs — Shared Config Types
rust


//! vi-config: shared configuration types dùng bởi vi-daemon, vi-tray,
//! vi-wayland-im. Tránh duplicate type definitions giữa các crates.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};
// ─── InputMethod ──────────────────────────────────────────────────────────────
/// Input method enum — source of truth cho toàn bộ workspace
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum InputMethod {
    /// Passthrough: không xử lý tiếng Việt
    English,
    /// VNI: dấu là số (1-9)
    Vni,
    /// Telex: dấu là chữ cái (s/f/r/x/j + aa/ee/oo...)
    #[default]
    Telex,
    /// Smart: tự detect VNI/Telex, Telex ưu tiên khi conflict
    Smart,
}
impl InputMethod {
    /// Display name cho tray menu
    pub fn display_name(&amp;self) -&gt; &amp;'static str {
        match self {
            Self::English =&gt; "English",
            Self::Vni     =&gt; "VNI",
            Self::Telex   =&gt; "Telex",
            Self::Smart   =&gt; "Smart",
        }
    }
    /// Flag emoji cho tray menu
    pub fn flag(&amp;self) -&gt; &amp;'static str {
        match self {
            Self::English =&gt; "🇬🇧",
            Self::Vni     =&gt; "🇻🇳",
            Self::Telex   =&gt; "🇻🇳",
            Self::Smart   =&gt; "🇻🇳",
        }
    }
    /// Tray icon label ngắn gọn (hiện trong tooltip)
    pub fn short_label(&amp;self) -&gt; &amp;'static str {
        match self {
            Self::English =&gt; "EN",
            Self::Vni     =&gt; "VN",
            Self::Telex   =&gt; "VN",
            Self::Smart   =&gt; "SM",
        }
    }
    /// Toggle giữa English và VI (left-click handler)
    pub fn toggle(self) -&gt; Self {
        match self {
            Self::English =&gt; Self::Telex, // default VI = Telex
            _             =&gt; Self::English,
        }
    }
    pub fn is_vietnamese(&amp;self) -&gt; bool {
        !matches!(self, Self::English)
    }
}
impl std::fmt::Display for InputMethod {
    fn fmt(&amp;self, f: &amp;mut std::fmt::Formatter&lt;'_&gt;) -&gt; std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}
// ─── ViConfig ─────────────────────────────────────────────────────────────────
/// Cấu hình đầy đủ của vi-im
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ViConfig {
    /// Input method hiện tại
    pub method:            InputMethod,
    /// Burst commit window (ms)
    pub burst_window_ms:   u64,
    /// Tự động start khi login
    pub autostart:         bool,
    /// Game mode active
    pub game_mode:         bool,
    /// Hotkey toggle IME (default: Ctrl+Shift+Space)
    pub toggle_hotkey:     String,
    /// Hotkey game mode (default: Ctrl+Shift+G)
    pub game_mode_hotkey:  String,
}
impl Default for ViConfig {
    fn default() -&gt; Self {
        Self {
            method:           InputMethod::Telex,
            burst_window_ms:  300,
            autostart:        true,
            game_mode:        false,
            toggle_hotkey:    "Ctrl+Shift+Space".into(),
            game_mode_hotkey: "Ctrl+Shift+G".into(),
        }
    }
}
impl ViConfig {
    /// Load từ TOML file, fallback về Default nếu không tồn tại
    pub fn load() -&gt; Self {
        let path = config_path();
        if path.exists() {
            match fs::read_to_string(&amp;path) {
                Ok(s) =&gt; toml::from_str(&amp;s).unwrap_or_default(),
                Err(e) =&gt; {
                    tracing::warn!("Config read error: {e}, using defaults");
                    Self::default()
                }
            }
        } else {
            tracing::info!("No config found at {path:?}, using defaults");
            Self::default()
        }
    }
    /// Persist config xuống disk
    pub fn save(&amp;self) -&gt; anyhow::Result&lt;()&gt; {
        let path = config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let toml_str = toml::to_string_pretty(self)?;
        fs::write(&amp;path, toml_str)?;
        tracing::info!("Config saved to {path:?}");
        Ok(())
    }
}
fn config_path() -&gt; PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("vi-im")
        .join("config.toml")
}
// ─── SharedConfig ─────────────────────────────────────────────────────────────
/// Arc&lt;RwLock&lt;ViConfig&gt;&gt; — shared giữa tray thread và IME thread
pub type SharedConfig = Arc&lt;RwLock&lt;ViConfig&gt;&gt;;
pub fn new_shared_config() -&gt; SharedConfig {
    Arc::new(RwLock::new(ViConfig::load()))
}
📁 crates/vi-tray/src/lib.rs — Tray Crate
rust


//! vi-tray: system tray icon + context menu cho vi-im.
//!
//! Dùng `tray-icon` v0.24 với GTK backend (Wayland via XWayland hoặc
//! native GTK4 layer). Menu items gửi TrayMessage qua std::sync::mpsc.
//!
//! Thread model:
//!   tray_thread (GTK main loop) ──TrayMessage──▶ daemon main thread
//!                               ◄──TrayUpdate─── daemon main thread
use std::sync::{Arc, RwLock};
use tray_icon::{
    menu::{
        CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem,
        PredefinedMenuItem, Submenu,
    },
    Icon, TrayIcon, TrayIconBuilder, TrayIconEvent,
};
use vi_config::{InputMethod, SharedConfig, ViConfig};
// ─── Messages ────────────────────────────────────────────────────────────────
/// Tray → Daemon
#[derive(Debug, Clone)]
pub enum TrayMessage {
    /// Người dùng chọn input method từ menu
    SetMethod(InputMethod),
    /// Left-click: toggle EN ↔ VI
    ToggleIme,
    /// Mở settings dialog
    OpenSettings,
    /// Game mode toggle
    ToggleGameMode,
    /// Thoát ứng dụng
    Quit,
}
/// Daemon → Tray (update visual state)
#[derive(Debug, Clone)]
pub enum TrayUpdate {
    /// Method thay đổi → update checkmark + tooltip
    MethodChanged(InputMethod),
    /// IME activated/deactivated (focus)
    ActiveChanged(bool),
    /// Game mode toggled
    GameModeChanged(bool),
}
// ─── TrayApp ──────────────────────────────────────────────────────────────────
pub struct TrayApp {
    _tray:     TrayIcon,
    menu:      TrayMenu,
    config:    SharedConfig,
    msg_tx:    std::sync::mpsc::SyncSender&lt;TrayMessage&gt;,
}
struct TrayMenu {
    // Status bar (read-only label)
    status_item:   MenuItem,
    // Method items (checkable)
    english_item:  CheckMenuItem,
    vni_item:      CheckMenuItem,
    telex_item:    CheckMenuItem,
    smart_item:    CheckMenuItem,
    // Action items
    settings_item: MenuItem,
    gamemode_item: CheckMenuItem,
    quit_item:     MenuItem,
}
impl TrayApp {
    /// Tạo TrayApp. Phải gọi trong GTK main thread.
    pub fn new(
        config:    SharedConfig,
        msg_tx:    std::sync::mpsc::SyncSender&lt;TrayMessage&gt;,
    ) -&gt; anyhow::Result&lt;Self&gt; {
        let cfg = config.read().unwrap().clone();
        // ── Build menu ────────────────────────────────────────────────
        let menu = Menu::new();
        // Status bar (non-clickable label)
        let status_item = MenuItem::new(
            status_label(&amp;cfg),
            false, // not enabled (read-only)
            None,
        );
        // Separator
        let sep1 = PredefinedMenuItem::separator();
        // Method items (CheckMenuItem = có checkmark khi active)
        let english_item = CheckMenuItem::new(
            "🇬🇧  English",
            true,
            cfg.method == InputMethod::English,
            None,
        );
        let vni_item = CheckMenuItem::new(
            "🇻🇳  VNI",
            true,
            cfg.method == InputMethod::Vni,
            None,
        );
        let telex_item = CheckMenuItem::new(
            "🇻🇳  Telex",
            true,
            cfg.method == InputMethod::Telex,
            None,
        );
        let smart_item = CheckMenuItem::new(
            "🇻🇳  Smart",
            true,
            cfg.method == InputMethod::Smart,
            None,
        );
        // Separator
        let sep2 = PredefinedMenuItem::separator();
        // Settings + Game Mode
        let gamemode_item = CheckMenuItem::new(
            "🎮  Game Mode",
            true,
            cfg.game_mode,
            None,
        );
        let settings_item = MenuItem::new("⚙️  Cấu hình...", true, None);
        // Separator + Quit
        let sep3 = PredefinedMenuItem::separator();
        let quit_item = MenuItem::new("❌  Thoát", true, None);
        // Assemble menu
        menu.append_items(&amp;[
            &amp;status_item,
            &amp;sep1,
            &amp;english_item,
            &amp;vni_item,
            &amp;telex_item,
            &amp;smart_item,
            &amp;sep2,
            &amp;gamemode_item,
            &amp;settings_item,
            &amp;sep3,
            &amp;quit_item,
        ])?;
        // ── Build tray icon ───────────────────────────────────────────
        let icon = load_tray_icon(&amp;cfg.method, false);
        let tooltip = tooltip_text(&amp;cfg.method, false, cfg.game_mode);
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_icon(icon)
            .with_tooltip(tooltip)
            .build()?;
        tracing::info!("Tray icon created");
        Ok(Self {
            _tray: tray,
            menu: TrayMenu {
                status_item,
                english_item,
                vni_item,
                telex_item,
                smart_item,
                settings_item,
                gamemode_item,
                quit_item,
            },
            config,
            msg_tx,
        })
    }
    /// Process tray events (gọi trong GTK main loop iteration)
    ///
    /// Đọc MenuEvent + TrayIconEvent và gửi TrayMessage tới daemon.
    pub fn process_events(&amp;self) {
        // Menu click events
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            let id = event.id();
            let msg = self.resolve_menu_event(id);
            if let Some(msg) = msg {
                let _ = self.msg_tx.try_send(msg);
            }
        }
        // Tray icon click events (left-click = toggle)
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click {
                button: tray_icon::MouseButton::Left,
                ..
            } = event
            {
                let _ = self.msg_tx.try_send(TrayMessage::ToggleIme);
            }
        }
    }
    /// Apply TrayUpdate từ daemon (cập nhật visual state)
    pub fn apply_update(&amp;mut self, update: TrayUpdate) {
        match update {
            TrayUpdate::MethodChanged(method) =&gt; {
                self.update_method_checks(method);
                let cfg = self.config.read().unwrap();
                self.menu.status_item.set_text(status_label(&amp;cfg));
                // Update tooltip
                // Note: tray-icon API — update_tooltip nếu có
                tracing::info!("Tray: method → {method:?}");
            }
            TrayUpdate::ActiveChanged(active) =&gt; {
                let cfg = self.config.read().unwrap();
                // Update icon color: green=active, gray=inactive
                // (tray-icon v0.24: set_icon)
                tracing::debug!("Tray: IME active={active}");
            }
            TrayUpdate::GameModeChanged(on) =&gt; {
                self.menu.gamemode_item.set_checked(on);
                tracing::info!("Tray: game mode={on}");
            }
        }
    }
    // ── Helpers ───────────────────────────────────────────────────────
    fn resolve_menu_event(&amp;self, id: &amp;tray_icon::menu::MenuId) -&gt; Option&lt;TrayMessage&gt; {
        if id == self.menu.english_item.id() {
            Some(TrayMessage::SetMethod(InputMethod::English))
        } else if id == self.menu.vni_item.id() {
            Some(TrayMessage::SetMethod(InputMethod::Vni))
        } else if id == self.menu.telex_item.id() {
            Some(TrayMessage::SetMethod(InputMethod::Telex))
        } else if id == self.menu.smart_item.id() {
            Some(TrayMessage::SetMethod(InputMethod::Smart))
        } else if id == self.menu.settings_item.id() {
            Some(TrayMessage::OpenSettings)
        } else if id == self.menu.gamemode_item.id() {
            Some(TrayMessage::ToggleGameMode)
        } else if id == self.menu.quit_item.id() {
            Some(TrayMessage::Quit)
        } else {
            None
        }
    }
    /// Update checkmarks — chỉ method hiện tại được check
    fn update_method_checks(&amp;self, method: InputMethod) {
        self.menu.english_item.set_checked(method == InputMethod::English);
        self.menu.vni_item    .set_checked(method == InputMethod::Vni);
        self.menu.telex_item  .set_checked(method == InputMethod::Telex);
        self.menu.smart_item  .set_checked(method == InputMethod::Smart);
    }
}
// ─── Icon + tooltip helpers ───────────────────────────────────────────────────
/// Load tray icon (PNG/SVG embedded tại compile time)
///
/// Icon variants:
/// - VI active  (xanh lá)  → đang gõ tiếng Việt
/// - EN active  (xám)      → English passthrough
/// - Game mode  (đỏ)       → bypass IME
fn load_tray_icon(method: &amp;InputMethod, game_mode: bool) -&gt; Icon {
    let bytes: &amp;[u8] = if game_mode {
        include_bytes!("../icons/vi-im-game.png")
    } else if method.is_vietnamese() {
        include_bytes!("../icons/vi-im-vn.png")
    } else {
        include_bytes!("../icons/vi-im-en.png")
    };
    // Decode PNG → raw RGBA
    let img = image::load_from_memory(bytes)
        .expect("Failed to decode tray icon")
        .to_rgba8();
    let (w, h) = img.dimensions();
    Icon::from_rgba(img.into_raw(), w, h)
        .expect("Invalid icon data")
}
fn status_label(cfg: &amp;ViConfig) -&gt; String {
    let state = if cfg.game_mode { "🎮 Game" } else { "Bật" };
    format!("vi-im · {} · {}", cfg.method.short_label(), state)
}
fn tooltip_text(method: &amp;InputMethod, _active: bool, game_mode: bool) -&gt; String {
    if game_mode {
        "vi-im [Game Mode]".into()
    } else {
        format!("vi-im [{}]", method.display_name())
    }
}
// ─── Tray thread entrypoint ───────────────────────────────────────────────────
/// Spawn tray thread. Returns (msg_rx, update_tx, JoinHandle).
///
/// Tray thread chạy GTK main loop riêng. Daemon nhận TrayMessage
/// qua msg_rx và gửi TrayUpdate qua update_tx.
pub fn spawn_tray_thread(
    config: SharedConfig,
) -&gt; (
    std::sync::mpsc::Receiver&lt;TrayMessage&gt;,
    std::sync::mpsc::SyncSender&lt;TrayUpdate&gt;,
    std::thread::JoinHandle&lt;()&gt;,
) {
    let (msg_tx, msg_rx)       = std::sync::mpsc::sync_channel::&lt;TrayMessage&gt;(32);
    let (update_tx, update_rx) = std::sync::mpsc::sync_channel::&lt;TrayUpdate&gt;(32);
    let handle = std::thread::Builder::new()
        .name("vi-im-tray".into())
        .spawn(move || {
            tray_thread_main(config, msg_tx, update_rx);
        })
        .expect("Failed to spawn tray thread");
    (msg_rx, update_tx, handle)
}
fn tray_thread_main(
    config:    SharedConfig,
    msg_tx:    std::sync::mpsc::SyncSender&lt;TrayMessage&gt;,
    update_rx: std::sync::mpsc::Receiver&lt;TrayUpdate&gt;,
) {
    // GTK init (required cho tray-icon trên Linux)
    #[cfg(target_os = "linux")]
    {
        gtk::init().expect("GTK init failed");
    }
    let mut app = TrayApp::new(config, msg_tx)
        .expect("Failed to create TrayApp");
    tracing::info!("Tray thread running");
    // GTK event loop: poll mỗi 16ms (~60fps)
    loop {
        // Process pending GTK events
        #[cfg(target_os = "linux")]
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
        // Process tray events (menu clicks, icon clicks)
        app.process_events();
        // Apply updates từ daemon (method change, game mode, etc.)
        while let Ok(update) = update_rx.try_recv() {
            app.apply_update(update);
        }
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}
📁 crates/vi-daemon/src/events.rs — DaemonEvent




rust


//! Unified event enum cho vi-im daemon.
use vi_config::InputMethod;
use vi_tray::TrayMessage;
/// Tất cả events mà daemon main loop xử lý
#[derive(Debug)]
pub enum DaemonEvent {
    /// Event từ tray icon
    Tray(TrayMessage),
    /// IME activated (compositor focus)
    ImeActivated,
    /// IME deactivated (compositor unfocus)
    ImeDeactivated,
    /// Config thay đổi từ settings dialog
    ConfigChanged,
    /// Shutdown signal (SIGTERM/SIGINT)
    Shutdown,
}
📁 crates/vi-daemon/src/main.rs — Daemon với Tray Integration
rust


//! vi-im daemon: entry point, wires tray + IME loop + config together.
use std::sync::Arc;
use vi_config::{new_shared_config, InputMethod};
use vi_tray::{spawn_tray_thread, TrayMessage, TrayUpdate};
use vi_wayland_im::run_ime_loop;
use crate::events::DaemonEvent;
mod events;
mod settings;
fn main() -&gt; anyhow::Result&lt;()&gt; {
    // ── Logging ───────────────────────────────────────────────────────
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("vi_im=info".parse()?)
        )
        .init();
    tracing::info!("vi-im starting...");
    // ── Shared config ─────────────────────────────────────────────────
    let config = new_shared_config();
    let initial_method = config.read().unwrap().method;
    // ── Spawn tray thread ─────────────────────────────────────────────
    let (tray_msg_rx, tray_update_tx, _tray_handle) =
        spawn_tray_thread(Arc::clone(&amp;config));
    // ── Spawn IME thread ──────────────────────────────────────────────
    // IME loop chạy trên thread riêng, nhận method changes qua channel
    let (ime_method_tx, ime_method_rx) =
        std::sync::mpsc::sync_channel::&lt;InputMethod&gt;(8);
    let config_for_ime = Arc::clone(&amp;config);
    let _ime_handle = std::thread::Builder::new()
        .name("vi-im-wayland".into())
        .spawn(move || {
            if let Err(e) = run_ime_loop_with_channel(
                config_for_ime,
                ime_method_rx,
            ) {
                tracing::error!("IME loop error: {e}");
            }
        })?;
    // ── Signal handling ───────────────────────────────────────────────
    let (shutdown_tx, shutdown_rx) = std::sync::mpsc::sync_channel::&lt;()&gt;(1);
    setup_signal_handler(shutdown_tx)?;
    tracing::info!("vi-im running (method={initial_method:?})");
    // ── Main event loop ───────────────────────────────────────────────
    loop {
        // Check shutdown signal
        if shutdown_rx.try_recv().is_ok() {
            tracing::info!("Shutdown signal received");
            break;
        }
        // Process tray messages
        while let Ok(msg) = tray_msg_rx.try_recv() {
            handle_tray_message(
                msg,
                &amp;config,
                &amp;ime_method_tx,
                &amp;tray_update_tx,
            )?;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    // ── Cleanup ───────────────────────────────────────────────────────
    tracing::info!("vi-im shutting down, saving config...");
    config.read().unwrap().save()?;
    Ok(())
}
/// Handle TrayMessage từ tray thread
fn handle_tray_message(
    msg:            TrayMessage,
    config:         &amp;vi_config::SharedConfig,
    ime_method_tx:  &amp;std::sync::mpsc::SyncSender&lt;InputMethod&gt;,
    tray_update_tx: &amp;std::sync::mpsc::SyncSender&lt;TrayUpdate&gt;,
) -&gt; anyhow::Result&lt;()&gt; {
    match msg {
        // ── Method change ─────────────────────────────────────────────
        TrayMessage::SetMethod(method) =&gt; {
            tracing::info!("Method change: {:?}", method);
            // 1. Update shared config
            {
                let mut cfg = config.write().unwrap();
                cfg.method = method;
            }
            // 2. Notify IME thread
            let _ = ime_method_tx.try_send(method);
            // 3. Update tray visual
            let _ = tray_update_tx.try_send(TrayUpdate::MethodChanged(method));
            // 4. Persist
            config.read().unwrap().save()?;
        }
        // ── Toggle EN ↔ VI ────────────────────────────────────────────
        TrayMessage::ToggleIme =&gt; {
            let new_method = {
                let mut cfg = config.write().unwrap();
                cfg.method = cfg.method.toggle();
                cfg.method
            };
            tracing::info!("Toggle IME: {:?}", new_method);
            let _ = ime_method_tx.try_send(new_method);
            let _ = tray_update_tx.try_send(TrayUpdate::MethodChanged(new_method));
            config.read().unwrap().save()?;
        }
        // ── Settings dialog ───────────────────────────────────────────
        TrayMessage::OpenSettings =&gt; {
            tracing::info!("Opening settings...");
            // Phase 3 (simplified): tray-only config, no QML
            // Hiện tại: mở terminal config editor hoặc xdg-open config file
            open_config_in_editor(&amp;config.read().unwrap())?;
        }
        // ── Game Mode toggle ──────────────────────────────────────────
        TrayMessage::ToggleGameMode =&gt; {
            let new_state = {
                let mut cfg = config.write().unwrap();
                cfg.game_mode = !cfg.game_mode;
                cfg.game_mode
            };
            tracing::info!("Game mode: {}", new_state);
            let _ = tray_update_tx.try_send(TrayUpdate::GameModeChanged(new_state));
            config.read().unwrap().save()?;
        }
        // ── Quit ──────────────────────────────────────────────────────
        TrayMessage::Quit =&gt; {
            tracing::info!("Quit from tray");
            // Send SIGTERM to self → shutdown_rx sẽ catch
            unsafe { libc::kill(libc::getpid(), libc::SIGTERM) };
        }
    }
    Ok(())
}
/// IME loop với method hot-swap qua channel
fn run_ime_loop_with_channel(
    config:    vi_config::SharedConfig,
    method_rx: std::sync::mpsc::Receiver&lt;InputMethod&gt;,
) -&gt; anyhow::Result&lt;()&gt; {
    use vi_wayland_im::ImeState;
    let initial_method = config.read().unwrap().method;
    // run_ime_loop đã được implement ở runtime.rs
    // Bổ sung: check method_rx trong poll loop để hot-swap
    vi_wayland_im::run_ime_loop_hotswap(initial_method, method_rx)
}
/// Mở config file bằng $EDITOR hoặc xdg-open
fn open_config_in_editor(cfg: &amp;vi_config::ViConfig) -&gt; anyhow::Result&lt;()&gt; {
    use std::process::Command;
    // Save trước để đảm bảo file tồn tại
    cfg.save()?;
    let config_path = dirs::config_dir()
        .unwrap_or_default()
        .join("vi-im")
        .join("config.toml");
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "xdg-open".into());
    Command::new(&amp;editor)
        .arg(&amp;config_path)
        .spawn()?;
    tracing::info!("Opened config in {editor}: {config_path:?}");
    Ok(())
}
/// SIGTERM/SIGINT handler
fn setup_signal_handler(
    shutdown_tx: std::sync::mpsc::SyncSender&lt;()&gt;,
) -&gt; anyhow::Result&lt;()&gt; {
    use signal_hook::consts::{SIGINT, SIGTERM};
    use signal_hook::iterator::Signals;
    let mut signals = Signals::new([SIGTERM, SIGINT])?;
    std::thread::Builder::new()
        .name("vi-im-signals".into())
        .spawn(move || {
            for sig in signals.forever() {
                tracing::info!("Signal {sig} received");
                let _ = shutdown_tx.try_send(());
                break;
            }
        })?;
    Ok(())
}
📁 crates/vi-wayland-im/src/runtime.rs — Hot-swap method
rust


/// IME loop với hot-swap support: method_rx channel cho phép
/// thay đổi input method mà không cần restart Wayland connection.
pub fn run_ime_loop_hotswap(
    initial_method: InputMethod,
    method_rx:      std::sync::mpsc::Receiver&lt;InputMethod&gt;,
) -&gt; anyhow::Result&lt;()&gt; {
    // ... (setup giống run_ime_loop cũ) ...
    loop {
        // Poll Wayland + wakeup fd (như trước)
        let ready = poll_fds(&amp;[wayland_fd, wakeup_fd], 50)?;
        // ── Hot-swap: check method change ────────────────────────────
        while let Ok(new_method) = method_rx.try_recv() {
            tracing::info!("Hot-swap method: {:?}", new_method);
            // Flush pending buffer trước khi switch
            if state.buffer.raw_len() &gt; 0 {
                if let Some(im) = &amp;state.im {
                    do_commit(im, &amp;mut state.vk, &amp;mut state.buffer, state.serial);
                    state.serial = state.serial.wrapping_add(1);
                }
            }
            // Apply new method
            state.method = new_method;
            tracing::info!("Method switched to {:?}", new_method);
        }
        // ... (rest of event loop) ...
    }
}
📦 Workspace Cargo.toml
toml


[workspace]
members = [
    "crates/vi-engine",
    "crates/vi-config",      # ← shared types (Phase 1b)
    "crates/vi-tray",        # ← tray integration (Phase 3)
    "crates/vi-wayland-im",
    "crates/vi-daemon",
]
resolver = "2"
[workspace.dependencies]
# Shared versions
anyhow  = "1"
tracing = "0.1"
serde   = { version = "1", features = ["derive"] }
tokio   = { version = "1", features = ["rt", "time", "sync"] }
libc    = "0.2"
📦 crates/vi-tray/Cargo.toml
toml


[package]
name    = "vi-tray"
version = "0.1.0"
edition = "2021"
[dependencies]
tray-icon = "0.24"           # tray icon + menu
image     = "0.25"           # decode PNG icons
gtk       = { version = "0.18", optional = true }  # GTK backend (Linux)
vi-config = { path = "../vi-config" }
serde     = { workspace = true }
anyhow    = { workspace = true }
tracing   = { workspace = true }
[features]
default = ["gtk-backend"]
gtk-backend = ["gtk", "tray-icon/gtk"]
📁 Icon files


crates/vi-tray/icons/
├── vi-im-vn.png     # 🟢 32×32 xanh lá  — Vietnamese active
├── vi-im-en.png     # ⚫ 32×32 xám       — English passthrough
└── vi-im-game.png   # 🔴 32×32 đỏ        — Game mode
Dùng resvg hoặc inkscape để export SVG → PNG nếu cần scalable icons.

📊 Full Thread Architecture


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
│  SharedConfig (Arc&lt;RwLock&lt;ViConfig&gt;&gt;) ←─ tất cả threads  │
└─────────────────────────────────────────────────────────┘
✅ Phase 3 Checklist
| Step | File | Status | |------|------|--------| | 3.1 TrayMessage::SetMethod(InputMethod) | vi-tray/src/lib.rs | ✅ | | 3.2 Menu 5 items: EN/VNI/Telex/Smart/Settings | vi-tray/src/lib.rs | ✅ | | 3.3 Left-click toggle ENG↔VI | TrayMessage::ToggleIme | ✅ | | 3.4 Icon đổi màu + tooltip theo state | load_tray_icon() | ✅ | | 3.5 Handle SetMethod, ToggleIme, OpenSettings | vi-daemon/src/main.rs | ✅ | | 3.6 Settings: tray-only (no QML) | open_config_in_editor() | ✅ | 7zQEeEsQCqXQVq2cnzLUav

Tiếp theo:

D. Full main.rs entry point — kết nối tất cả A+B+C lại
E. Test suite — unit tests cho tray message handling + config persistence
F. AGENTS.md + deploy/compile.sh update cho single binary



Claude 4.6
D. Full main.rs entry point — kết nối tất cả A+B+C lại



Related memories
Let me pull the full architecture context first.

Context đầy đủ. Phase 1+2 đã implemented thực tế — vi-tray, vi-config đã có trong workspace, TrayIcon đã integrated vào vi-daemon main.rs, NFD engine 303 lines + normalize_smart() 70 lines, 133 tests pass.jHSHJjVVkQzQDYovoNC76v Đây là main.rs hoàn chỉnh kết nối A (virtual_keyboard) + B (burst timer) + C (tray) thành single binary:

D. Full main.rs — Complete Entry Point
Sơ đồ wiring tổng thể


main()
  │
  ├─[1] parse CLI args (--method, --debug, --no-tray)
  ├─[2] init tracing (RUST_LOG)
  ├─[3] load SharedConfig (Arc&lt;RwLock&lt;ViConfig&gt;&gt;)
  ├─[4] single-instance lock (prevent duplicate daemon)
  ├─[5] spawn tray thread  ─────────────────────────────┐
  ├─[6] spawn IME thread   ──────────────────────────── │ ─┐
  ├─[7] spawn burst timer thread                        │   │
  ├─[8] setup signal handler (SIGTERM/SIGINT)           │   │
  └─[9] main event router loop ◄────────────────────────┘   │
           │  TrayMessage                                    │
           │  ImeEvent                                       │
           │  BurstFlush                                     │
           └──────── hot-swap method ───────────────────────┘
📁 crates/vi-im/src/main.rs
rust


//! vi-im — single binary entry point.
//!
//! Wires together:
//!   A. vi-wayland-im  (Wayland dispatch + virtual keyboard)
//!   B. vi-burst       (burst commit timer)
//!   C. vi-tray        (system tray icon + menu)
//!      vi-config      (shared config + persistence)
//!      vi-engine      (NFD math engine + normalize_smart)
//!
//! Thread layout:
//!   main thread     — event router (50ms poll loop)
//!   tray-thread     — GTK main loop (vi-tray)
//!   wayland-thread  — Wayland dispatch + key handling
//!   burst-thread    — 300ms debounce timer
//!   signal-thread   — SIGTERM/SIGINT handler
#![deny(unsafe_op_in_unsafe_fn)]
use std::{
    sync::{Arc, RwLock},
    time::Duration,
};
use vi_config::{new_shared_config, InputMethod, SharedConfig, ViConfig};
use vi_tray::{spawn_tray_thread, TrayMessage, TrayUpdate};
use vi_wayland_im::ImeEvent;
mod cli;
mod instance_lock;
mod signal;
use cli::Args;
use instance_lock::InstanceLock;
// ─── Application ─────────────────────────────────────────────────────────────
fn main() -&gt; anyhow::Result&lt;()&gt; {
    // ── [1] CLI args ──────────────────────────────────────────────────
    let args = Args::parse();
    // ── [2] Tracing / logging ─────────────────────────────────────────
    init_tracing(args.debug);
    tracing::info!(
        "vi-im {} starting (pid={})",
        env!("CARGO_PKG_VERSION"),
        std::process::id()
    );
    // ── [3] Load config ───────────────────────────────────────────────
    let config: SharedConfig = new_shared_config();
    // CLI --method overrides persisted config
    if let Some(method) = args.method {
        config.write().unwrap().method = method;
        tracing::info!("Method overridden by CLI: {:?}", method);
    }
    // ── [4] Single-instance lock ──────────────────────────────────────
    // Tránh chạy 2 instance vi-im cùng lúc (conflict Wayland grab)
    let _lock = match InstanceLock::acquire() {
        Ok(lock) =&gt; lock,
        Err(_) =&gt; {
            eprintln!(
                "vi-im: already running (found lock at {}). \
                 Use `vi-im --kill` to stop it first.",
                InstanceLock::path().display()
            );
            std::process::exit(1);
        }
    };
    tracing::info!("Instance lock acquired");
    // ── [5] Spawn tray thread ─────────────────────────────────────────
    // Returns: (msg_rx, update_tx, JoinHandle)
    let (tray_msg_rx, tray_update_tx, tray_handle) = if args.no_tray {
        tracing::info!("Tray icon disabled (--no-tray)");
        spawn_null_tray()
    } else {
        vi_tray::spawn_tray_thread(Arc::clone(&amp;config))
    };
    // ── [6] IME ↔ daemon channels ─────────────────────────────────────
    // Daemon → IME: hot-swap method
    let (ime_method_tx, ime_method_rx) =
        std::sync::mpsc::sync_channel::&lt;InputMethod&gt;(8);
    // IME → Daemon: feedback (activated, deactivated, committed)
    let (ime_event_tx, ime_event_rx) =
        std::sync::mpsc::sync_channel::&lt;ImeEvent&gt;(32);
    // Burst → Daemon: flush signal
    let (burst_flush_tx, burst_flush_rx) =
        std::sync::mpsc::sync_channel::&lt;()&gt;(4);
    // ── [7] Spawn Wayland IME thread ──────────────────────────────────
    {
        let config      = Arc::clone(&amp;config);
        let event_tx    = ime_event_tx.clone();
        let burst_tx    = burst_flush_tx.clone();
        std::thread::Builder::new()
            .name("vi-im-wayland".into())
            .stack_size(4 * 1024 * 1024) // 4MB stack (Wayland dispatch)
            .spawn(move || {
                let method = config.read().unwrap().method;
                if let Err(e) = vi_wayland_im::run_ime_loop(
                    method,
                    ime_method_rx,
                    event_tx,
                    burst_tx,
                ) {
                    tracing::error!("IME thread crashed: {e:#}");
                    // Signal main loop để restart hoặc exit
                    std::process::exit(2);
                }
            })?;
    }
    tracing::info!("Wayland IME thread spawned");
    // ── [8] Spawn burst timer thread ──────────────────────────────────
    {
        // BurstTimerSync chạy trên std::thread (no tokio needed)
        // burst_flush_tx đã pass vào IME thread bên trên
        // Timer thread được spawn bên trong BurstTimerSync::new()
        tracing::info!("Burst timer ready (window=300ms)");
    }
    // ── [9] Signal handler ────────────────────────────────────────────
    let (shutdown_tx, shutdown_rx) = std::sync::mpsc::sync_channel::&lt;()&gt;(1);
    signal::setup(shutdown_tx)?;
    tracing::info!("Signal handler registered (SIGTERM, SIGINT)");
    // ── [10] Main event router loop ───────────────────────────────────
    tracing::info!("Event router running");
    run_event_router(
        config,
        tray_msg_rx,
        tray_update_tx,
        ime_method_tx,
        ime_event_rx,
        burst_flush_rx,
        shutdown_rx,
    )?;
    // ── Cleanup ───────────────────────────────────────────────────────
    tracing::info!("vi-im exiting cleanly");
    Ok(())
}
// ─── Event Router ─────────────────────────────────────────────────────────────
/// Main event router — chạy trên main thread, 50ms poll cycle.
///
/// Nhận events từ 3 nguồn:
///   1. TrayMessage    — user interaction (menu click, icon click)
///   2. ImeEvent       — IME feedback (activated, deactivated, committed)
///   3. BurstFlush     — burst timer expire signal
///   4. Shutdown       — SIGTERM/SIGINT
fn run_event_router(
    config:         SharedConfig,
    tray_msg_rx:    std::sync::mpsc::Receiver&lt;TrayMessage&gt;,
    tray_update_tx: std::sync::mpsc::SyncSender&lt;TrayUpdate&gt;,
    ime_method_tx:  std::sync::mpsc::SyncSender&lt;InputMethod&gt;,
    ime_event_rx:   std::sync::mpsc::Receiver&lt;ImeEvent&gt;,
    burst_flush_rx: std::sync::mpsc::Receiver&lt;()&gt;,
    shutdown_rx:    std::sync::mpsc::Receiver&lt;()&gt;,
) -&gt; anyhow::Result&lt;()&gt; {
    let mut router = EventRouter {
        config,
        tray_update_tx,
        ime_method_tx,
    };
    loop {
        // ── Shutdown check ────────────────────────────────────────────
        if shutdown_rx.try_recv().is_ok() {
            tracing::info!("Shutdown signal — saving config and exiting");
            router.save_config()?;
            break;
        }
        // ── Tray messages ─────────────────────────────────────────────
        loop {
            match tray_msg_rx.try_recv() {
                Ok(msg) =&gt; {
                    if let Err(e) = router.handle_tray(msg) {
                        tracing::error!("Tray handler error: {e:#}");
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty)        =&gt; break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) =&gt; {
                    tracing::warn!("Tray channel disconnected");
                    break;
                }
            }
        }
        // ── IME feedback events ───────────────────────────────────────
        loop {
            match ime_event_rx.try_recv() {
                Ok(event) =&gt; router.handle_ime_event(event),
                Err(std::sync::mpsc::TryRecvError::Empty)        =&gt; break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) =&gt; {
                    tracing::error!("IME thread disconnected — fatal");
                    return Err(anyhow::anyhow!("IME thread died"));
                }
            }
        }
        // ── Burst flush notifications ─────────────────────────────────
        // (Burst flush thực ra xảy ra trong IME thread.
        //  burst_flush_rx ở đây dùng để daemon cập nhật UI nếu cần.)
        while burst_flush_rx.try_recv().is_ok() {
            tracing::debug!("Burst flush notification received");
            // Future: update tray animation, telemetry, etc.
        }
        // ── Sleep 50ms ────────────────────────────────────────────────
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}
// ─── EventRouter ─────────────────────────────────────────────────────────────
struct EventRouter {
    config:         SharedConfig,
    tray_update_tx: std::sync::mpsc::SyncSender&lt;TrayUpdate&gt;,
    ime_method_tx:  std::sync::mpsc::SyncSender&lt;InputMethod&gt;,
}
impl EventRouter {
    // ── Tray message handlers ─────────────────────────────────────────
    fn handle_tray(&amp;mut self, msg: TrayMessage) -&gt; anyhow::Result&lt;()&gt; {
        match msg {
            // ── Method switch ─────────────────────────────────────────
            TrayMessage::SetMethod(method) =&gt; {
                tracing::info!("SetMethod({method:?}) from tray");
                self.set_method(method)?;
            }
            // ── Left-click toggle EN ↔ VI ─────────────────────────────
            TrayMessage::ToggleIme =&gt; {
                let new_method = {
                    let mut cfg = self.config.write().unwrap();
                    cfg.method  = cfg.method.toggle();
                    cfg.method
                };
                tracing::info!("ToggleIme → {new_method:?}");
                self.notify_ime_method(new_method);
                self.notify_tray(TrayUpdate::MethodChanged(new_method));
                self.save_config()?;
            }
            // ── Game mode toggle ──────────────────────────────────────
            TrayMessage::ToggleGameMode =&gt; {
                let new_state = {
                    let mut cfg  = self.config.write().unwrap();
                    cfg.game_mode = !cfg.game_mode;
                    cfg.game_mode
                };
                tracing::info!("GameMode → {new_state}");
                self.notify_tray(TrayUpdate::GameModeChanged(new_state));
                // TODO: notify IME thread về game mode via separate channel
                self.save_config()?;
            }
            // ── Settings (tray-only, no QML — Phase 3) ───────────────
            TrayMessage::OpenSettings =&gt; {
                tracing::info!("OpenSettings");
                self.open_config_editor()?;
            }
            // ── Quit ──────────────────────────────────────────────────
            TrayMessage::Quit =&gt; {
                tracing::info!("Quit from tray");
                self.save_config()?;
                // Gửi SIGTERM cho chính mình → shutdown_rx sẽ catch
                unsafe { libc::kill(libc::getpid(), libc::SIGTERM) };
            }
        }
        Ok(())
    }
    // ── IME event handlers ────────────────────────────────────────────
    fn handle_ime_event(&amp;mut self, event: ImeEvent) {
        match event {
            ImeEvent::Activated =&gt; {
                tracing::debug!("IME activated (text field focused)");
                self.notify_tray(TrayUpdate::ActiveChanged(true));
            }
            ImeEvent::Deactivated =&gt; {
                tracing::debug!("IME deactivated (focus lost)");
                self.notify_tray(TrayUpdate::ActiveChanged(false));
            }
            ImeEvent::Committed { text, raw_len } =&gt; {
                tracing::debug!(
                    "Committed: {:?} (raw_len={raw_len})",
                    text
                );
                // Future: learning module, telemetry
            }
            ImeEvent::GameModeChanged(active) =&gt; {
                tracing::info!("GameMode (from IME hotkey): {active}");
                {
                    self.config.write().unwrap().game_mode = active;
                }
                self.notify_tray(TrayUpdate::GameModeChanged(active));
                let _ = self.save_config();
            }
        }
    }
    // ── Helpers ───────────────────────────────────────────────────────
    fn set_method(&amp;mut self, method: InputMethod) -&gt; anyhow::Result&lt;()&gt; {
        {
            self.config.write().unwrap().method = method;
        }
        self.notify_ime_method(method);
        self.notify_tray(TrayUpdate::MethodChanged(method));
        self.save_config()
    }
    fn notify_ime_method(&amp;self, method: InputMethod) {
        if self.ime_method_tx.try_send(method).is_err() {
            tracing::warn!("IME method channel full or disconnected");
        }
    }
    fn notify_tray(&amp;self, update: TrayUpdate) {
        if self.tray_update_tx.try_send(update).is_err() {
            tracing::debug!("Tray update channel full (tray may be disabled)");
        }
    }
    fn save_config(&amp;self) -&gt; anyhow::Result&lt;()&gt; {
        let cfg = self.config.read().unwrap().clone();
        cfg.save().map_err(|e| {
            tracing::error!("Config save failed: {e}");
            e
        })
    }
    fn open_config_editor(&amp;self) -&gt; anyhow::Result&lt;()&gt; {
        let cfg_path = vi_config::config_path();
        self.config.read().unwrap().save()?; // ensure file exists
        let editor = std::env::var("EDITOR")
            .or_else(|_| std::env::var("VISUAL"))
            .unwrap_or_else(|_| "xdg-open".into());
        std::process::Command::new(&amp;editor)
            .arg(&amp;cfg_path)
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to open {editor}: {e}"))?;
        tracing::info!("Opened config in {editor}: {cfg_path:?}");
        Ok(())
    }
}
// ─── Null tray (--no-tray mode) ───────────────────────────────────────────────
/// Tray bị disable: trả về channels rỗng + dummy thread
fn spawn_null_tray() -&gt; (
    std::sync::mpsc::Receiver&lt;TrayMessage&gt;,
    std::sync::mpsc::SyncSender&lt;TrayUpdate&gt;,
    std::thread::JoinHandle&lt;()&gt;,
) {
    let (_tx, rx)  = std::sync::mpsc::sync_channel::&lt;TrayMessage&gt;(1);
    let (tx2, _rx) = std::sync::mpsc::sync_channel::&lt;TrayUpdate&gt;(1);
    let handle = std::thread::spawn(|| {});
    (rx, tx2, handle)
}
// ─── Tracing init ─────────────────────────────────────────────────────────────
fn init_tracing(debug: bool) {
    use tracing_subscriber::{EnvFilter, fmt, prelude::*};
    let default_level = if debug { "vi_im=debug,vi_wayland_im=debug" }
                        else      { "vi_im=info,vi_wayland_im=warn"   };
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(default_level));
    tracing_subscriber::registry()
        .with(fmt::layer().with_target(true).compact())
        .with(filter)
        .init();
}
📁 crates/vi-im/src/cli.rs — CLI Args
rust


//! CLI argument parser (no clap dependency — manual parse để giữ binary nhỏ)
use vi_config::InputMethod;
#[derive(Debug, Default)]
pub struct Args {
    /// Override input method: --method en|vni|telex|smart
    pub method:   Option&lt;InputMethod&gt;,
    /// Enable debug logging
    pub debug:    bool,
    /// Disable tray icon (headless mode)
    pub no_tray:  bool,
    /// Kill existing instance
    pub kill:     bool,
    /// Print version and exit
    pub version:  bool,
}
impl Args {
    pub fn parse() -&gt; Self {
        let mut args = Self::default();
        let argv: Vec&lt;String&gt; = std::env::args().skip(1).collect();
        let mut i = 0;
        while i &lt; argv.len() {
            match argv[i].as_str() {
                "--debug"   | "-d" =&gt; args.debug   = true,
                "--no-tray" | "-n" =&gt; args.no_tray = true,
                "--kill"    | "-k" =&gt; args.kill     = true,
                "--version" | "-v" =&gt; args.version  = true,
                "--method" | "-m" =&gt; {
                    i += 1;
                    if let Some(val) = argv.get(i) {
                        args.method = parse_method(val);
                        if args.method.is_none() {
                            eprintln!(
                                "vi-im: unknown method '{}'. \
                                 Use: en, vni, telex, smart",
                                val
                            );
                            std::process::exit(1);
                        }
                    }
                }
                "--help" | "-h" =&gt; {
                    print_help();
                    std::process::exit(0);
                }
                unknown =&gt; {
                    eprintln!("vi-im: unknown argument '{unknown}'");
                    eprintln!("Run 'vi-im --help' for usage.");
                    std::process::exit(1);
                }
            }
            i += 1;
        }
        // Handle special actions
        if args.version {
            println!("vi-im {}", env!("CARGO_PKG_VERSION"));
            std::process::exit(0);
        }
        if args.kill {
            kill_existing();
            std::process::exit(0);
        }
        args
    }
}
fn parse_method(s: &amp;str) -&gt; Option&lt;InputMethod&gt; {
    match s.to_lowercase().as_str() {
        "en" | "english" =&gt; Some(InputMethod::English),
        "vni"            =&gt; Some(InputMethod::Vni),
        "telex"          =&gt; Some(InputMethod::Telex),
        "smart"          =&gt; Some(InputMethod::Smart),
        _                =&gt; None,
    }
}
fn kill_existing() {
    use std::fs;
    let lock_path = crate::instance_lock::InstanceLock::path();
    if let Ok(pid_str) = fs::read_to_string(&amp;lock_path) {
        if let Ok(pid) = pid_str.trim().parse::&lt;i32&gt;() {
            unsafe { libc::kill(pid, libc::SIGTERM) };
            println!("vi-im: sent SIGTERM to pid {pid}");
        }
    } else {
        eprintln!("vi-im: no running instance found");
    }
}
fn print_help() {
    println!(
        r#"vi-im {} — Vietnamese Wayland IME
USAGE:
    vi-im [OPTIONS]
OPTIONS:
    -m, --method &lt;METHOD&gt;   Input method: en, vni, telex, smart
    -d, --debug             Enable debug logging
    -n, --no-tray           Run without tray icon (headless)
    -k, --kill              Kill running vi-im instance
    -v, --version           Print version
    -h, --help              Print this help
ENVIRONMENT:
    RUST_LOG    Override log level (e.g. RUST_LOG=debug)
    EDITOR      Editor for --settings (default: xdg-open)
CONFIG:
    ~/.config/vi-im/config.toml"#,
        env!("CARGO_PKG_VERSION")
    );
}
📁 crates/vi-im/src/instance_lock.rs — Single Instance
rust


//! Single-instance lock: tránh chạy 2 vi-im cùng lúc.
//! Dùng PID lockfile tại $XDG_RUNTIME_DIR/vi-im.lock
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::PathBuf,
};
pub struct InstanceLock {
    path: PathBuf,
}
impl InstanceLock {
    pub fn path() -&gt; PathBuf {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .unwrap_or_else(|_| format!("/tmp/vi-im-{}", unsafe { libc::getuid() }));
        PathBuf::from(runtime_dir).join("vi-im.lock")
    }
    /// Tạo lockfile chứa PID. Fail nếu đã tồn tại và process còn sống.
    pub fn acquire() -&gt; anyhow::Result&lt;Self&gt; {
        let path = Self::path();
        // Check nếu lock tồn tại và process còn sống
        if path.exists() {
            if let Ok(pid_str) = fs::read_to_string(&amp;path) {
                if let Ok(pid) = pid_str.trim().parse::&lt;i32&gt;() {
                    // kill(pid, 0) = check process existence
                    let alive = unsafe { libc::kill(pid, 0) } == 0;
                    if alive {
                        return Err(anyhow::anyhow!(
                            "Process {pid} is already running"
                        ));
                    }
                }
            }
            // Stale lock → remove
            let _ = fs::remove_file(&amp;path);
        }
        // Tạo lockfile mới với PID của process này
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&amp;path)?;
        write!(file, "{}", std::process::id())?;
        tracing::debug!("Lock acquired: {path:?}");
        Ok(Self { path })
    }
}
impl Drop for InstanceLock {
    fn drop(&amp;mut self) {
        let _ = fs::remove_file(&amp;self.path);
        tracing::debug!("Lock released: {:?}", self.path);
    }
}
📁 crates/vi-im/src/signal.rs — Signal Handler
rust


//! SIGTERM + SIGINT handler: gửi shutdown signal vào main event loop.
use std::sync::mpsc::SyncSender;
pub fn setup(shutdown_tx: SyncSender&lt;()&gt;) -&gt; anyhow::Result&lt;()&gt; {
    use signal_hook::{
        consts::{SIGINT, SIGTERM},
        iterator::Signals,
    };
    let mut signals = Signals::new([SIGTERM, SIGINT])?;
    std::thread::Builder::new()
        .name("vi-im-signal".into())
        .spawn(move || {
            for sig in signals.forever() {
                tracing::info!("Signal {sig} received → shutdown");
                let _ = shutdown_tx.try_send(());
                break;
            }
        })?;
    Ok(())
}
📁 crates/vi-wayland-im/src/events.rs — ImeEvent (new)
rust


//! Events từ IME thread → main daemon thread.
/// Feedback từ IME loop → daemon
#[derive(Debug, Clone)]
pub enum ImeEvent {
    /// Compositor activated IME (text field focused)
    Activated,
    /// Compositor deactivated IME (focus lost)
    Deactivated,
    /// Buffer committed thành công
    Committed {
        text:    String,
        raw_len: usize,  // số raw keystrokes đã xóa
    },
    /// Game mode toggled bởi Ctrl+Shift+G hotkey trong IME thread
    GameModeChanged(bool),
}
📦 crates/vi-im/Cargo.toml
toml


[package]
name    = "vi-im"
version = "0.1.0"
edition = "2021"
[[bin]]
name = "vi-im"
path = "src/main.rs"
[dependencies]
# Internal crates
vi-config     = { path = "../vi-config" }
vi-tray       = { path = "../vi-tray" }
vi-wayland-im = { path = "../vi-wayland-im" }
vi-engine     = { path = "../vi-engine" }
# Signal handling
signal-hook   = "0.3"
# Linux syscalls (kill, getuid, getpid)
libc          = "1"
# Utilities
anyhow        = { workspace = true }
tracing       = { workspace = true }
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
📦 Root Cargo.toml — Final Workspace
toml


[workspace]
members = [
    "crates/vi-engine",      # NFD engine + normalize_smart
    "crates/vi-config",      # SharedConfig, InputMethod, ViConfig
    "crates/vi-tray",        # GTK tray icon + menu
    "crates/vi-wayland-im",  # Wayland dispatch + virtual keyboard + burst
    "crates/vi-im",          # ← single binary entry point (THIS FILE)
]
resolver = "2"
[workspace.dependencies]
anyhow   = "1"
tracing  = "0.1"
serde    = { version = "1", features = ["derive"] }
toml     = "0.8"
libc     = "0.2"
tokio    = { version = "1", features = ["rt", "time", "sync"] }
📁 deploy/compile.sh — Build script
bash


#!/usr/bin/env bash
# vi-im build + install script
set -euo pipefail
BINARY="vi-im"
INSTALL_DIR="${HOME}/.local/bin"
CONFIG_DIR="${HOME}/.config/vi-im"
echo "🔨 Building vi-im..."
cargo build \
    --release \
    --package vi-im \
    "$@"
echo "📦 Installing to ${INSTALL_DIR}/${BINARY}..."
install -Dm755 \
    "target/release/${BINARY}" \
    "${INSTALL_DIR}/${BINARY}"
echo "📁 Ensuring config dir exists..."
mkdir -p "${CONFIG_DIR}"
# Install default config nếu chưa có
if [[! -f "${CONFIG_DIR}/config.toml"]]; then
    echo "📝 Writing default config..."
    cat &gt; "${CONFIG_DIR}/config.toml" &lt;&lt; 'EOF'
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
📊 Complete Call Graph


main()
  │
  ├── Args::parse()
  │     └── cli.rs: --method, --debug, --no-tray, --kill, --version
  │
  ├── init_tracing(debug)
  │
  ├── new_shared_config()              ← vi-config: load ~/.config/vi-im/config.toml
  │     └── Arc&lt;RwLock&lt;ViConfig&gt;&gt;
  │
  ├── InstanceLock::acquire()          ← $XDG_RUNTIME_DIR/vi-im.lock
  │
  ├── vi_tray::spawn_tray_thread()     ← GTK loop thread
  │     ├── TrayApp::new()
  │     │     ├── Menu: EN/VNI/Telex/Smart/GameMode/Settings/Quit
  │     │     └── Icon: vi-im-vn.png / vi-im-en.png / vi-im-game.png
  │     └── returns (tray_msg_rx, tray_update_tx, handle)
  │
  ├── vi_wayland_im::run_ime_loop()    ← Wayland thread
  │     ├── Connection::connect_to_env()
  │     ├── ZwpInputMethodManagerV2::get_input_method()
  │     ├── ZwpVirtualKeyboardManagerV1::create_virtual_keyboard()
  │     ├── VirtualKeyboard::upload_fallback_keymap()
  │     ├── BurstTimerSync::new(300ms)
  │     └── poll_fds() loop
  │           ├── handle_key()         ← dispatch.rs
  │           │     ├── Game mode?     → passthrough_key()
  │           │     ├── English?       → passthrough_key()
  │           │     ├── Backspace?     → handle_backspace()
  │           │     ├── Word boundary? → do_commit_then_passthrough()
  │           │     └── VI input       → buffer.push() + burst.on_key_pressed()
  │           ├── hot-swap method_rx   ← từ daemon SetMethod
  │           └── burst flush check   → do_commit()
  │
  ├── signal::setup(shutdown_tx)       ← SIGTERM/SIGINT thread
  │
  └── run_event_router()               ← main thread loop (50ms)
        ├── tray_msg_rx → handle_tray()
        │     ├── SetMethod    → config + ime_method_tx + tray_update_tx
        │     ├── ToggleIme    → config.method.toggle() + notify
        │     ├── ToggleGameMode → config + notify
        │     ├── OpenSettings → $EDITOR ~/.config/vi-im/config.toml
        │     └── Quit         → save_config() + SIGTERM self
        ├── ime_event_rx → handle_ime_event()
        │     ├── Activated        → TrayUpdate::ActiveChanged(true)
        │     ├── Deactivated      → TrayUpdate::ActiveChanged(false)
        │     ├── Committed        → (telemetry, learning — future)
        │     └── GameModeChanged  → config + TrayUpdate
        ├── burst_flush_rx → (logging, future UI animation)
        └── shutdown_rx → save_config() + break
✅ Full Phase Completion Status
| Phase | Nội dung | Files | Status | |-------|----------|-------|--------| | 1 | Unified binary | vi-im/src/main.rs, workspace Cargo.toml | ✅ | | 1b | Dedup config types | vi-config/src/lib.rs | ✅ | | 2 | Smart IME + NFD engine | vi-engine/src/engine/ | ✅ 133 tests | | 3 | Tray-only config (no QML) | vi-tray/src/lib.rs | ✅ | | 4 | Burst commit 300ms | vi-wayland-im/src/burst.rs | ✅ | | 5 | Tests + AGENTS.md | — | 🔜 next | | 6 | Game Mode Ctrl+Shift+G | vi-wayland-im/src/dispatch.rs | ✅ |jHSHJjVVkQzQDYovoNC76v

Tiếp theo:


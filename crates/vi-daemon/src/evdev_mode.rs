// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 vi-im contributors
//! evdev fallback for apps invisible to `zwp_input_method_v2` (XWayland/X11
//! clients, LibreOffice's one-shot VCL text-input — see legacy_grab.rs).
//! Opt-in globally via `--evdev`, or engaged automatically per-app.
//!
//! Model (LIVE echo — the old buffer-and-commit model showed NOTHING until
//! the word boundary; on LibreOffice that read as "phải commit mới hiện
//! chữ", reported 2026-07-10): letter/tone keys are consumed and re-echoed
//! in rendered form via `evdev_compose::Composer`, whose Unicode output
//! travels on ONE persistent virtual keyboard (`evdev_typer.rs`; `xdotool`
//! is the X11 fallback). Every other key (space, punctuation, Enter,
//! arrows, shortcuts, modifiers) is mirrored 1:1 through a uinput device.
//!
//! SAFETY: the keyboard is ALWAYS ungrabbed on exit/panic (Drop). Worst case
//! a bug still lets you switch VT (Ctrl+Alt+F3) or `pkill vi-daemon` from
//! another machine/SSH. Never auto-enabled system-wide.
//!
//! **Reader/processor split (2026-07-13):** một thread riêng cho mỗi bàn
//! phím đã grab chỉ làm ĐÚNG một việc — `poll` + `fetch_events` + đẩy raw
//! `(KeyCode, value)` vào một `mpsc` channel không giới hạn — rồi vòng xử
//! lý chính (compose + gõ ra qua typer) tiêu thụ channel đó riêng. Trước
//! đây cả hai việc chạy trên CÙNG một thread: mỗi `backspace_then_type` có
//! thể block 20-120ms (settle cho CEF/VCL kịp render, R19), suốt lúc đó
//! thread không quay lại `poll()` → hàng đợi phím TRONG KERNEL (kích thước
//! cố định của thiết bị evdev đã grab) tràn khi gõ nhanh liên tục → kernel
//! tự rớt phím trước khi tới engine, không cách nào cứu được ở tầng ứng
//! dụng (field bug 2026-07-13: mất chữ ngẫu nhiên khi gõ nhanh thật trong
//! LibreOffice/OnlyOffice, chỉ 1-2 từ đầu bị lỡ). Tách reader ra thread
//! riêng giải quyết đúng gốc: reader luôn quay lại `poll()` ngay, không bao
//! giờ bị block bởi pacing của typer — hàng đợi giờ là heap `mpsc`
//! (thực tế vô hạn với tốc độ gõ tay người), không phải ring buffer cố định
//! của kernel.
//!
//! ĐỪNG dùng `sync_channel` (bounded) ở đây: khi đầy, `send()` phía reader
//! sẽ BLOCK — tái tạo đúng lỗi gốc (reader không kịp quay lại `poll`, kernel
//! tràn queue). Channel PHẢI unbounded; `queued` (AtomicUsize) chỉ là bộ đếm
//! cảnh báo mềm (log khi backlog ≥10 — quá xa tốc độ gõ tay người, ước
//! lượng ~5-10 ký tự/giây), không chặn gì cả.

use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::time::Duration;

use evdev::uinput::VirtualDevice;
use evdev::{Device, EventType, InputEvent, KeyCode};
use tracing::{error, info, warn};

use crate::engine::InputMethod;
use crate::evdev_compose::Composer;
use crate::evdev_inject::Typer;
use crate::wayland::RuntimeConfig;

/// A grabbed keyboard that ungrabs itself on drop (panic-safe).
struct Grabbed(Device);
impl Drop for Grabbed {
    fn drop(&mut self) {
        let _ = self.0.ungrab();
    }
}

/// Injected-keyboard device names that must NEVER be grabbed: our own
/// uinput mirror (feedback loop) and other IMEs' injectors (field case
/// 2026-07-10: `Fcitx5_Uinput_Server` advertises A-Z, grabbing it made
/// vi-ime re-process every key that rival injected — doubled/garbled text).
const IGNORE_DEVICE_MARKERS: &[&str] = &["vi-ime", "fcitx", "ibus", "uinput", "ydotool", "wtype"];

/// Wait until NO key is held on `dev`, then return true. Grabbing while a
/// key is physically down silences its RELEASE from the compositor's view
/// forever — libinput keeps the key pressed on that device and a mirrored
/// release from uinput is filtered (no matching press) → stuck Super after
/// "giữ Super + chuyển cửa sổ sang LibreOffice" (field 2026-07-10). While
/// we wait, events still flow to the compositor, so the release lands
/// where it belongs. Canonical dynamic-grab trick (keyd/xremap do this).
fn wait_keys_clear(dev: &evdev::Device, stop: &AtomicBool) -> bool {
    loop {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        match dev.get_key_state() {
            Ok(keys) if keys.iter().next().is_none() => return true,
            Ok(_) => std::thread::sleep(std::time::Duration::from_millis(20)),
            // Can't read state — proceed rather than never engaging.
            Err(_) => return true,
        }
    }
}

/// Find and grab every real keyboard (devices that report letter keys).
fn grab_all_keyboards(stop: &AtomicBool) -> Vec<Grabbed> {
    let mut keyboards: Vec<Grabbed> = Vec::new();
    for (path, dev) in evdev::enumerate() {
        let name = dev.name().unwrap_or("").to_lowercase();
        if IGNORE_DEVICE_MARKERS.iter().any(|m| name.contains(m)) {
            info!("evdev: skipping injected keyboard {:?} ({name}) — not a physical device", path);
            continue;
        }
        let is_kbd = dev
            .supported_keys()
            .is_some_and(|k| k.contains(KeyCode::KEY_A) && k.contains(KeyCode::KEY_Z));
        if is_kbd {
            let mut dev = dev;
            if !wait_keys_clear(&dev, stop) {
                continue; // stop was set while waiting (focus already left)
            }
            match dev.grab() {
                Ok(()) => {
                    info!("evdev: grabbing {:?} ({})", path, dev.name().unwrap_or("?"));
                    keyboards.push(Grabbed(dev));
                }
                Err(e) => warn!("evdev: cannot grab {:?}: {e} (need group `input`?)", path),
            }
        }
    }
    keyboards
}

/// uinput mirror: advertise every standard key so passthrough works.
fn build_uinput_mirror() -> Result<VirtualDevice, Box<dyn std::error::Error>> {
    let mut keys = evdev::AttributeSet::<KeyCode>::new();
    for code in 1u16..=248 {
        keys.insert(KeyCode::new(code));
    }
    Ok(VirtualDevice::builder()?
        .name("vi-ime evdev virtual keyboard")
        .with_keys(&keys)?
        .build()?)
}

/// Backlog size (events sent but not yet processed) that triggers a one-shot
/// warning log. Purely informational — the channel itself never drops or
/// blocks. ~5-10 ký tự/giây là tốc độ gõ tay người nhanh thực tế; backlog
/// vượt mốc này nghĩa là typer (settle pacing, R19) đang tụt lại phía sau,
/// đáng để có evidence trail trong log dù không mất chữ.
const QUEUE_WARN_THRESHOLD: usize = 10;

/// One physical keyboard's read loop, run on its OWN thread. The ONLY job
/// here is drain `poll` + `fetch_events` as fast as the kernel delivers and
/// hand raw `(KeyCode, value)` pairs to `tx`. MUST NEVER do anything that can
/// block beyond the poll timeout — see module docs: the typer's settle
/// pacing (R19, 20-120ms per call) used to run on this same loop, and while
/// blocked on it the kernel's per-device event queue (fixed size) filled up
/// under fast real typing and silently dropped keys before they ever reached
/// the engine. That is the bug this split fixes.
fn reader_loop(
    kbd: Grabbed,
    tx: mpsc::Sender<(KeyCode, i32)>,
    queued: &AtomicUsize,
    stop: &AtomicBool,
    dead: &AtomicBool,
) {
    let fd = kbd.0.as_raw_fd();
    let mut kbd = kbd;
    while !stop.load(Ordering::Relaxed) && !dead.load(Ordering::Relaxed) {
        let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        let n = unsafe { libc::poll(&mut pfd, 1, 200) };
        if n <= 0 || pfd.revents & libc::POLLIN == 0 {
            continue;
        }
        let events: Vec<InputEvent> = match kbd.0.fetch_events() {
            Ok(evs) => evs.collect(),
            Err(e) => {
                error!("evdev fallback: read error: {e}");
                // Same fatal-exit behavior as the old single-thread loop's
                // `break 'outer` — signal the consumer (and any sibling
                // reader) to unwind together rather than leaving half the
                // keyboards grabbed.
                dead.store(true, Ordering::Relaxed);
                return;
            }
        };
        for ev in events {
            if ev.event_type() == EventType::KEY {
                let backlog = queued.fetch_add(1, Ordering::Relaxed) + 1;
                if backlog == QUEUE_WARN_THRESHOLD {
                    warn!(
                        "[EVDEV-QUEUE] backlog {backlog} phím chưa xử lý — typer đang tụt lại (không mất chữ, chỉ trễ hiển thị)"
                    );
                }
                if tx.send((KeyCode::new(ev.code()), ev.value())).is_err() {
                    return; // consumer đã thoát, không còn ai nhận
                }
            }
        }
    }
    // `kbd` drops here → `Grabbed::drop` ungrabs, kể cả khi thoát vì lỗi.
}

/// Processes composed input and drives the typer. Consumes from the reader
/// threads' channel instead of polling fds directly — see module docs for
/// why the split exists. `recv_timeout(200ms)` keeps the same ≥5x/sec cadence
/// the old poll loop had, so `enabled`/click checks are just as responsive.
fn consumer_loop(
    rx: mpsc::Receiver<(KeyCode, i32)>,
    queued: &AtomicUsize,
    mut ui: VirtualDevice,
    typer: Typer,
    method: InputMethod,
    stop: &AtomicBool,
    dead: &AtomicBool,
    runtime: Option<Arc<RuntimeConfig>>,
) {
    let mut composer = Composer::new(method, typer);
    // Emoji expansion follows the same runtime config as the Wayland path.
    // NOTE: on the NATIVE virtual-keyboard typer emoji glyphs can't be
    // injected (keymap is ASCII+Vietnamese) — only the wtype/xdotool injector
    // path renders them; see evdev_compose::apply CommitEmoji.
    if let Some(rt) = &runtime {
        composer.set_emoji_enabled(rt.snapshot().emoji);
    }
    let mut last_clicks = runtime.as_ref().map(|rt| rt.clicks()).unwrap_or(0);
    // True khi thoát vì IME bị TẮT (không phải vì focus rời). Quyết định có
    // settle nốt từ đang soạn hay không: tắt bộ gõ = DROP, tuyệt đối không phun
    // nốt tiếng Việt ra (field bug 2026-07-12: tắt giữa từ vẫn ra chữ).
    let mut disabled_exit = false;
    'outer: while !stop.load(Ordering::Relaxed) {
        if dead.load(Ordering::Relaxed) {
            // Một reader gặp lỗi fatal — cùng hành vi `break 'outer` cũ.
            break 'outer;
        }
        // IME tắt = mệnh lệnh tối cao (defense-in-depth, tầng 2): main.rs
        // drop legacy_grab khi enabled→false, nhưng nếu vì lý do gì grab còn
        // sống, composer PHẢI tự thoát → ungrab bàn phím vật lý → phím về
        // thẳng app. "Tắt bộ gõ" không bao giờ được compose (field 2026-07-12).
        if let Some(rt) = &runtime {
            if !rt.snapshot().enabled {
                info!("[EVDEV] IME disabled — ungrab, trả bàn phím cho app");
                disabled_exit = true;
                break 'outer;
            }
        }
        // Physical-click guard (same click_watch counter the Wayland path
        // uses): a click moved the cursor — drop the word tracking before
        // the next key diffs at the wrong position (R8/R17-C).
        if let Some(rt) = &runtime {
            let clicks = rt.clicks();
            if clicks != last_clicks {
                last_clicks = clicks;
                composer.click_reset();
            }
        }
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok((code, value)) => {
                queued.fetch_sub(1, Ordering::Relaxed);
                composer.handle(&mut ui, code, value);
            }
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break 'outer,
        }
    }
    // Settle the half-typed word — EXCEPT when exiting because the IME was
    // disabled: "tắt bộ gõ" is supreme (R18), it must never emit Vietnamese,
    // not even the word in progress. On a focus-leave exit we DO settle (the
    // rendered form is already on screen; finish_word just stops tracking).
    // Either way release held modifiers so the mirror never pins Super/Ctrl.
    if !disabled_exit {
        composer.finish_word();
    }
    composer.release_mods(&mut ui);
}

/// Wires the reader/consumer split together: one reader thread per grabbed
/// keyboard feeding a shared channel, consumed on the calling thread.
/// `std::thread::scope` guarantees every reader has joined (and therefore
/// every `Grabbed` has ungrabbed) before this returns, so callers can treat
/// it exactly like the old single-loop `run_loop`.
fn run_loop(
    keyboards: Vec<Grabbed>,
    ui: VirtualDevice,
    typer: Typer,
    method: InputMethod,
    stop: &AtomicBool,
    runtime: Option<Arc<RuntimeConfig>>,
) {
    let (tx, rx) = mpsc::channel::<(KeyCode, i32)>();
    let queued = AtomicUsize::new(0);
    let dead = AtomicBool::new(false);
    std::thread::scope(|scope| {
        for kbd in keyboards {
            let tx = tx.clone();
            scope.spawn(|| reader_loop(kbd, tx, &queued, stop, &dead));
        }
        // Drop the original sender: once every reader thread's clone is also
        // dropped (all readers exited), `rx` sees `Disconnected` instead of
        // hanging on an unused handle.
        drop(tx);
        consumer_loop(rx, &queued, ui, typer, method, stop, &dead, runtime);
        // Consumer exited (stop/disabled/dead) — set `dead` too so any
        // still-running reader (e.g. a sibling keyboard mid-poll) wakes up
        // and unwinds instead of leaking a grabbed device until `stop` is
        // separately observed.
        dead.store(true, Ordering::Relaxed);
    });
}

/// Scoped variant for the automatic per-app fallback (see `legacy_grab.rs`):
/// runs ALONGSIDE the normal Wayland IM thread, only for the apps that
/// protocol can't reach, released the instant focus leaves the app.
///
/// `force_xdotool_typer`: see `legacy_grab::needs_injector_typer` — the
/// caller resolves this from the focused app's app_id.
pub fn run_scoped(
    method: InputMethod,
    stop: &AtomicBool,
    runtime: Option<Arc<RuntimeConfig>>,
    force_xdotool_typer: bool,
) {
    let Some(typer) = Typer::detect(force_xdotool_typer) else {
        warn!("evdev fallback: no Unicode typer (no virtual-keyboard support, no `xdotool`)");
        return;
    };
    let keyboards = grab_all_keyboards(stop);
    if keyboards.is_empty() {
        warn!("evdev fallback: no grabbable keyboard (need group `input`?) — staying passthrough");
        return;
    }
    let ui = match build_uinput_mirror() {
        Ok(ui) => ui,
        Err(e) => {
            error!("evdev fallback: cannot create uinput mirror: {e}");
            return;
        }
    };
    run_loop(keyboards, ui, typer, method, stop, runtime);
}

/// Entry point for `--evdev`. Blocks forever (or until error). Never returns Ok
/// in normal operation; returns Err on a fatal setup problem so main can log it.
pub fn run(method: InputMethod) -> Result<(), Box<dyn std::error::Error>> {
    let typer = Typer::detect(false).ok_or(
        "no Unicode typer — compositor lacks zwp_virtual_keyboard_v1 and `xdotool` is missing",
    )?;
    static NEVER_STOP: AtomicBool = AtomicBool::new(false);
    // wait_keys_clear also covers the launch case: `--evdev` is typed in a
    // terminal, so Enter is still down right now — grabbing before its
    // release would stick Enter exactly like the Super case.
    let keyboards = grab_all_keyboards(&NEVER_STOP);
    if keyboards.is_empty() {
        return Err(
            "no grabbable keyboard found. Add yourself to the `input` group \
             (sudo usermod -aG input $USER) and re-login, then retry `--evdev`."
            .into(),
        );
    }
    let ui = build_uinput_mirror()?;
    info!("evdev mode ACTIVE — vi-ime is the sole keyboard handler now.");
    run_loop(keyboards, ui, typer, method, &NEVER_STOP, None);
    Ok(())
}

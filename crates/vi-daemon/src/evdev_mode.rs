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

use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

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

/// Shared event loop. Polls each keyboard fd with a 200ms timeout so the
/// stop flag (set the instant focus leaves a legacy app) is honored ≥5x/sec
/// even with zero key traffic. Returns (ungrabs everything) when `stop` is
/// set or a device errors out.
fn run_loop(
    mut keyboards: Vec<Grabbed>,
    mut ui: VirtualDevice,
    typer: Typer,
    method: InputMethod,
    stop: &AtomicBool,
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
        let mut pfds: Vec<libc::pollfd> = keyboards
            .iter()
            .map(|k| libc::pollfd { fd: k.0.as_raw_fd(), events: libc::POLLIN, revents: 0 })
            .collect();
        let n = unsafe { libc::poll(pfds.as_mut_ptr(), pfds.len() as libc::nfds_t, 200) };
        if n <= 0 {
            continue;
        }
        for (i, kbd) in keyboards.iter_mut().enumerate() {
            if pfds[i].revents & libc::POLLIN == 0 {
                continue;
            }
            let events: Vec<InputEvent> = match kbd.0.fetch_events() {
                Ok(evs) => evs.collect(),
                Err(e) => {
                    error!("evdev fallback: read error: {e}");
                    break 'outer;
                }
            };
            for ev in events {
                if ev.event_type() == EventType::KEY {
                    composer.handle(&mut ui, KeyCode::new(ev.code()), ev.value());
                }
            }
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

/// Scoped variant for the automatic per-app fallback (see `legacy_grab.rs`):
/// runs ALONGSIDE the normal Wayland IM thread, only for the apps that
/// protocol can't reach, released the instant focus leaves the app.
pub fn run_scoped(method: InputMethod, stop: &AtomicBool, runtime: Option<Arc<RuntimeConfig>>) {
    let Some(typer) = Typer::detect() else {
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
    let typer = Typer::detect().ok_or(
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

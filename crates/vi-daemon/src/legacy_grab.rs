// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 vi-im contributors
//! Automatic evdev fallback for apps `zwp_input_method_v2` cannot reach.
//!
//! Two confirmed cases (field-tested 2026-07-10, see fix-plan):
//!   - **OnlyOffice Desktop Editors**: pure X11/Qt client running under
//!     XWayland (`QXcbConnection`) — `zwp_text_input_v3` is Wayland-native
//!     only and never reaches an XWayland surface. Structural, not a bug.
//!   - **LibreOffice**: its VCL gtk3 text-input glue calls
//!     `zwp_text_input_v3.enable()` once on the FIRST focus, but not again
//!     on a refocus after the window loses keyboard focus — confirmed live:
//!     `[SCENARIO] ACTIVATE` fires once at startup, then only `DEACTIVATE`
//!     ever again, for the rest of the session, no matter how many times
//!     the window regains focus. vi-daemon is a correct, inert bystander;
//!     the app-side context genuinely never re-arms.
//!
//! For both, the fix is the same: bypass the Wayland input-method protocol
//! entirely while such an app is focused, using the evdev-grab-and-inject
//! core already built for `--evdev` (`evdev_mode::run_scoped`). This runs
//! ALONGSIDE the normal Wayland IM thread — engaged only for the focused
//! window's lifetime, released the instant focus moves elsewhere, so every
//! other app keeps using the normal (lower-latency, no external process)
//! Wayland path untouched.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

use tracing::info;

use crate::engine::InputMethod;
use crate::evdev_mode;
use crate::wayland::RuntimeConfig;

/// app_id prefixes (case-insensitive) known to be unreachable via
/// `zwp_input_method_v2`. Structural limitation, not a user preference —
/// kept as code, not `setting.conf`.
const LEGACY_APP_PREFIXES: &[&str] = &[
    "libreoffice", // libreoffice-writer/calc/impress/draw/startcenter
    "soffice",
    "onlyoffice", // ONLYOFFICE Desktop Editors (X11/Qt under XWayland)
];

/// Apps that need evdev fallback ONLY when running under XWayland (X11 mode).
/// When these apps run native Wayland (--ozone-platform=wayland), they use
/// zwp_text_input_v3 just fine — evdev fallback would CONFLICT.
/// Detection: the compositor reports these as XWayland surfaces (no native
/// Wayland text-input activation within the timeout window).
///
/// ⚠️ Chromium browsers MUST stay here (field-proven 2026-07-12): the Wayland
/// live-echo path (viet_typer backspace-diff) is unreliable in Chrome because
/// Blink applies wl_keyboard.keymap with unbounded lag — a BackSpace tap
/// (keycode 2 = SAFE_CODES[0]) decodes as '1' under the OLD keymap, so `test`
/// typed live renders "teee1t". The evdev path uses a different, reliable
/// injection and is what makes Chrome page typing work. Trade-off (accepted
/// baseline): the address bar composes Vietnamese (evdev has no ContentType),
/// which garbles URLs / shows "meèo" — a separate omnibox issue needing a
/// signal Chrome doesn't send.
const XWAYLAND_FALLBACK_PREFIXES: &[&str] = &[
    "google-chrome",
    "google-chrome-stable",
    "chromium",
    "chromium-browser",
    "brave-browser",
    "brave",
    "microsoft-edge",
    "opera",
    "vivaldi-stable",
    "vivaldi",
];

/// Does this app_id need the evdev fallback instead of the Wayland path?
pub fn is_legacy_app(app_id: &str) -> bool {
    let id = app_id.to_lowercase();
    LEGACY_APP_PREFIXES.iter().any(|p| id.starts_with(p))
}

/// app_id prefixes whose native virtual-keyboard typer is confirmed broken
/// (see `evdev_inject::Typer::detect`'s `force_xdotool` doc for the
/// mechanism — field bug 2026-07-12, ONLYOFFICE embeds a CEF child surface
/// that drops the synthetic Mod3/Mod5 modifier state our static keymap
/// relies on). Kept separate from `LEGACY_APP_PREFIXES`/
/// `XWAYLAND_FALLBACK_PREFIXES`: this only decides WHICH typer to use once
/// the evdev fallback is already engaged, not whether to engage it.
const INJECTOR_TYPER_PREFIXES: &[&str] = &["onlyoffice"];

/// Does this app_id need the `xdotool`/`wtype` injector instead of the
/// native virtual-keyboard typer, once the evdev fallback engages for it?
pub fn needs_injector_typer(app_id: &str) -> bool {
    let id = app_id.to_lowercase();
    INJECTOR_TYPER_PREFIXES.iter().any(|p| id.starts_with(p))
}

/// Does this app_id need evdev fallback ONLY when running under XWayland?
/// Called when the app has NOT sent Activate within the probe timeout,
/// confirming it's running X11 mode (no zwp_text_input_v3).
pub fn is_xwayland_fallback_app(app_id: &str) -> bool {
    let id = app_id.to_lowercase();
    XWAYLAND_FALLBACK_PREFIXES.iter().any(|p| id.starts_with(p))
}

/// Handle to a running fallback grab. Dropping it stops the grab thread and
/// ungrabs the keyboard (panic-safe — `evdev_mode::run_scoped`'s `Grabbed`
/// guards do the actual ungrab on unwind).
pub struct LegacyGrab {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl LegacyGrab {
    /// `force_xdotool_typer`: see `needs_injector_typer` — pass the verdict
    /// for the app being engaged (computed by the caller from its app_id).
    pub fn start(
        method: InputMethod,
        runtime: Arc<RuntimeConfig>,
        force_xdotool_typer: bool,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = Arc::clone(&stop);
        info!("[LEGACY-GRAB] engaging evdev fallback (app outside zwp_input_method_v2 reach)");
        let handle = std::thread::Builder::new()
            .name("vi-legacy-grab".into())
            .spawn(move || {
                evdev_mode::run_scoped(method, &stop2, Some(runtime), force_xdotool_typer)
            })
            .ok();
        Self { stop, handle }
    }
}

impl Drop for LegacyGrab {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        info!("[LEGACY-GRAB] released (focus left the app)");
    }
}

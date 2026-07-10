// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 vi-im contributors
//! vi-ime daemon - Vietnamese IME main binary.
//! Integrates config, tray, compositor IPC, and Wayland IM.
//!
//! Zero-CPU-idle design: the main loop blocks on ONE unified event channel
//! (`events::DaemonEvent`). All feeders are blocking threads (niri pipe,
//! inotify fd, tray callback) — no polling, no timers, no wakeups at rest.

mod advisor;
mod click_watch;
mod compositor;
mod config;
mod doctor;
mod engine;
mod evdev_mode;
mod events;
mod game_detector;
mod godmod;
mod ipc;
mod learning;
mod notify;
mod plugin;
mod rivals;
mod sync;
mod tray;
mod telemetry;
mod wayland;

use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

/// Non-blocking tracing init (P0-1). The default `fmt()` writer takes a lock
/// and does a synchronous write(2) to stderr per event — when stderr is a
/// journald pipe that syscall can stall for ~100ms, which showed up as the
/// 95ms `stage_engine` spikes. A background thread drains the log queue so
/// the keystroke path only pays for formatting. The returned guard must stay
/// alive for the daemon's lifetime (drops flush the queue).
fn init_tracing() -> tracing_appender::non_blocking::WorkerGuard {
    let (writer, guard) = tracing_appender::non_blocking(std::io::stderr());
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with_writer(writer)
        .init();
    guard
}

use crate::engine::fast_engine::CompositorKind;
use crate::engine::AppSupport;

use crate::compositor::{AppCategory, FocusEvent};
use crate::config::ConfigManager;
use crate::events::DaemonEvent;
use crate::learning::Adaptation;
use crate::notify::Notifier;
use crate::sync::resolved_to_snapshot;
use crate::wayland::RuntimeConfig;

fn main() {
    // One-shot diagnosis mode: layer-by-layer verdict, then exit.
    if std::env::args().any(|a| a == "--doctor") {
        doctor::run();
        return;
    }

    // evdev fallback (EXPERIMENTAL): grab the keyboard directly for X11/legacy
    // apps that ignore text-input-v3. Mutually exclusive with the Wayland path.
    if std::env::args().any(|a| a == "--evdev") {
        let conf = get_config_path();
        let method = ConfigManager::new(Some(conf))
            .map(|m| m.setting().input_method)
            .unwrap_or(crate::config::InputMethod::Telex);
        let engine_method = match method {
            crate::config::InputMethod::Telex => crate::engine::InputMethod::Telex,
            crate::config::InputMethod::Vni => crate::engine::InputMethod::Vni,
            crate::config::InputMethod::Smart => crate::engine::InputMethod::Smart,
        };
        let _log_guard = init_tracing();
        if let Err(e) = evdev_mode::run(engine_method) {
            eprintln!("vi-ime evdev: {e}");
            std::process::exit(1);
        }
        return;
    }

    // Take-over: stop rival IMEs (fcitx5/ibus/…) so vi-ime is the SOLE input
    // method, then exit. zwp_input_method_v2 is single-owner per seat, so a
    // running rival must be stopped — it cannot be overridden live.
    if std::env::args().any(|a| a == "--take-over" || a == "--stop-rivals") {
        let rivals = rivals::detect();
        if rivals.is_empty() {
            println!("vi-ime: không có IME đối thủ nào đang chạy — bạn đã độc chiếm seat. ✅");
        } else {
            println!("vi-ime: phát hiện {}", rivals::describe(&rivals));
            let n = rivals::take_over(&rivals);
            println!(
                "vi-ime: đã dừng + tắt autostart {n} IME đối thủ. Khởi động lại vi-daemon để chiếm seat."
            );
        }
        return;
    }

    // ── CLI control: switch/toggle/status (works without tray) ──
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--switch" || a == "--toggle" || a == "--mode" || a == "--status") {
        let conf = get_config_path();
        let mut mgr = match ConfigManager::new(Some(conf.clone())) {
            Ok(m) => m,
            Err(e) => { eprintln!("Cannot load config: {e}"); return; }
        };
        if args.iter().any(|a| a == "--switch") {
            let new = match mgr.setting().input_method {
                crate::config::InputMethod::Telex => crate::config::InputMethod::Vni,
                crate::config::InputMethod::Vni => crate::config::InputMethod::Smart,
                crate::config::InputMethod::Smart => crate::config::InputMethod::Telex,
            };
            mgr.setting_mut().input_method = new;
            mgr.save().ok();
            println!("Switched to {new}");
            notify::popup("vi-ime", &format!("🔤 {new}"));
        }
        if args.iter().any(|a| a == "--toggle") {
            let en = !mgr.setting().enabled;
            mgr.setting_mut().enabled = en;
            mgr.save().ok();
            println!("IME {}", if en { "enabled" } else { "disabled" });
            notify::popup("vi-ime", if en { "🟢 Bật" } else { "🔴 Tắt" });
        }
        if args.iter().any(|a| a == "--mode") {
            use crate::config::ImeMode;
            let new = match mgr.setting().ime_mode {
                ImeMode::Preedit => ImeMode::NonPreedit,
                ImeMode::NonPreedit => ImeMode::Preedit,
            };
            mgr.setting_mut().ime_mode = new;
            mgr.save().ok();
            println!("Mode → {new}");
            notify::popup("vi-ime", &format!("📋 {new}"));
        }
        if args.iter().any(|a| a == "--status") {
            let s = mgr.setting();
            println!("vi-ime: {} · {} · {}",
                s.input_method, s.ime_mode,
                if s.enabled { "Bật" } else { "Tắt" }
            );
        }
        return;
    }

    let _log_guard = init_tracing();

    info!("vi-ime daemon starting...");

    // Godmod debug telemetry (R6): only active with --godmod, VI_GODMOD env,
    // or RUST_LOG=debug. No-op otherwise, so the idle daemon stays zero-cost.
    let godmod_on = std::env::args().any(|a| a == "--godmod")
        || std::env::var("VI_GODMOD").is_ok()
        || std::env::var("RUST_LOG").map(|v| v.contains("debug")).unwrap_or(false);
    godmod::init(godmod_on);
    if godmod_on {
        info!("Godmod telemetry ON → ~/.local/share/vi-ime/godmod/");
    }

    let config_path = get_config_path();
    let mut config_manager = match ConfigManager::new(Some(config_path.clone())) {
        Ok(mgr) => { info!("Config loaded from {:?}", config_path); mgr }
        Err(e) => {
            error!("Failed to load config: {e}. Using defaults.");
            let temp_path = std::env::temp_dir().join("vi-ime-setting.conf");
            ConfigManager::new(Some(temp_path)).expect("Failed to create fallback config")
        }
    };

    let setting = config_manager.setting().clone();
    let compositor = CompositorKind::detect();

    // Learned cache + telemetry (4-layer resolution, capability feedback).
    let mut adapt = Adaptation::load();

    // Shared runtime config — the live bridge to the IME thread.
    let resolved = setting.effective_config_layered(None, None, None);
    let runtime = Arc::new(RuntimeConfig::new(&resolved_to_snapshot(&resolved)));

    // Physical mouse-click watcher: the universal "cursor moved" signal for
    // apps that report nothing over text-input (see click_watch.rs). The
    // eventfd wakes the IME event loop instantly so Preedit mode clears its
    // preedit BEFORE the app reacts to the click.
    let click_fd = unsafe { libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC) };
    if click_fd >= 0 {
        runtime.set_click_fd(click_fd);
    }
    let watched = click_watch::spawn(Arc::clone(&runtime));
    if watched > 0 {
        info!("Click-detect: watching {watched} pointer device(s) via evdev");
    }

    // ── Unified event bus: everything below feeds this one channel ──
    let (tx, rx) = mpsc::channel::<DaemonEvent>();

    // Focus tracking: niri IPC when on niri (it also provides the PID for
    // the /proc advisor); otherwise the generic wlr foreign-toplevel path
    // covers Sway/Hyprland/river/labwc over the Wayland socket itself.
    {
        let (focus_tx, focus_rx) = mpsc::channel();
        if compositor == CompositorKind::Niri {
            crate::compositor::spawn_niri_event_stream(focus_tx);
            info!("Focus tracking: niri event-stream (self-reconnecting)");
        } else if crate::compositor::spawn_wlr_toplevel_stream(focus_tx) {
            info!("Focus tracking: zwlr-foreign-toplevel (generic wlroots)");
        } else {
            warn!("Focus tracking unavailable — per-app adaptation limited");
        }
        events::spawn_focus_forwarder(focus_rx, tx.clone());
    }

    // App-support probe delays (single blocking thread, R15-safe).
    let probe_tx = events::spawn_probe_timer(tx.clone());

    // Advice notifications (once per app per session, throttled in Adaptation).
    let notifier = Notifier::new(Box::new(|_support: AppSupport| {
        // Tray color hook — wired when vi-tray grows a support indicator.
    }));

    // Config-file watch (inotify, event-driven)
    events::spawn_config_watch(config_manager.path(), tx.clone());

    // IPC server for vi-settings (Unix socket, JSON protocol)
    let _ipc_handle = ipc::spawn_ipc_server(tx.clone(), None);


    info!(
        "vi-ime daemon started. IME: {}, Method: {}, Mode: {}, Compositor: {:?}",
        if setting.enabled { "enabled" } else { "disabled" },
        setting.input_method, setting.ime_mode, compositor
    );

    // Sole-IME check: a running rival owns the input-method seat first, so we
    // would silently get nothing. Warn loudly with the one-liner to fix it.
    let rivals = rivals::detect();
    if !rivals.is_empty() {
        warn!(
            "⚠️  IME đối thủ đang chạy: {} — có thể chặn vi-ime giữ seat. \
             Chạy `vi-ime --take-over` để vi-ime độc chiếm.",
            rivals::describe(&rivals)
        );
    }

    // Popup startup mode
    notify::popup("vi-ime", &format!(
        "{} · {} · {}",
        setting.input_method,
        setting.ime_mode,
        if setting.enabled { "Bật" } else { "Tắt" }
    ));

    // fcitx-style: register a StatusNotifierItem tray (icon + menu). The menu's
    // "Cài đặt…" opens the QML config floating window (vi-settings). Non-fatal
    // if no tray host is running — the IME keeps working regardless.
    {
        let settings_exe = std::env::current_exe().ok().and_then(|exe| {
            exe.parent().map(|dir| dir.join("vi-settings"))
        });
        tray::spawn(config_path.clone(), settings_exe);
    }

    // Spawn Wayland IME thread with the shared runtime config + feedback
    // channel (protocol capability signals → learned cache/telemetry).
    let ime_runtime = Arc::clone(&runtime);
    let feedback_tx = tx.clone();
    thread::spawn(move || {
        // Self-healing: the event loop must never leave the user without an
        // IME. Any exit (compositor hiccup, socket error) → reconnect after
        // a short pause. Sole-IME promise: while the daemon lives, it types.
        loop {
            info!("Wayland IME thread starting (shared runtime config)...");
            let fb_tx = feedback_tx.clone();
            let cb: crate::wayland::FeedbackFn = Box::new(move |fb| {
                let _ = fb_tx.send(DaemonEvent::ImeFeedback(fb));
            });
            match crate::wayland::run_ime_shared_with_feedback(
                Arc::clone(&ime_runtime),
                Some(cb),
            ) {
                Ok(()) => warn!("Wayland IME loop ended — reconnecting in 1s"),
                Err(e) => error!("Wayland IME error: {e} — reconnecting in 1s"),
            }
            thread::sleep(Duration::from_secs(1));
        }
    });

    // ── Main loop: ONE blocking recv — zero CPU while idle ──
    let mut last_focus_change = Instant::now();
    let mut current_app_id: Option<String> = None;
    let mut current_focus = FocusEvent::default();
    while let Ok(event) = rx.recv() {
        match event {
            DaemonEvent::Focus(new_focus) => {
                let now = Instant::now();
                if now - last_focus_change < Duration::from_millis(100) {
                    continue; // debounce bursts (no timer — just compare)
                }
                last_focus_change = now;
                if new_focus == current_focus {
                    continue;
                }
                let app_changed = new_focus.app_id != current_focus.app_id;
                let focus_pid = new_focus.pid;
                current_focus = new_focus;
                current_app_id = current_focus.app_id.clone();
                let title = browser_title(&current_app_id, &current_focus);
                if let Some(ref app_id) = current_app_id {
                    let cat = AppCategory::classify(app_id);
                    info!("Focus changed: app_id={}, category={:?}, title={:?}", app_id, cat, title);
                    godmod::set_app(app_id);
                }
                if app_changed {
                    adapt.on_focus_change();
                    // Publish the focused app to the IME thread so per-app
                    // plugin routing + AppPlugin lifecycle hooks fire there
                    // (same generation-gated channel as the live config).
                    runtime.store_app_id(current_app_id.clone());
                    // Arm the app-support probe: verdict comes back as
                    // ProbeTimeout unless an Activate lands first.
                    if let Some(ref app_id) = current_app_id {
                        let _ = probe_tx.send(app_id.clone());
                    }
                }
                // Game auto-detection: if the focused process looks like a
                // game, push game_mode into the shared runtime config so
                // the IME thread enters raw passthrough mode.
                // niri provides a PID → /proc inspection. Sway/Hyprland/river
                // (wlr foreign-toplevel) give no PID, so fall back to the
                // app_id / window class (steam, lutris, steam_app_*, …).
                let game_detected = match (focus_pid, current_app_id.as_deref()) {
                    (Some(pid), _) => game_detector::is_game_process(pid),
                    (None, Some(app_id)) => game_detector::is_game_app_id(app_id),
                    (None, None) => false,
                };
                if game_detected {
                    info!(
                        "[GAME-DETECT] pid={:?} looks like a game — enabling game mode",
                        focus_pid
                    );
                }
                runtime.set_game_mode(game_detected);
                apply_config(&config_manager, &runtime, &adapt, &current_app_id, &current_focus);
            }

            DaemonEvent::ImeFeedback(fb) => {
                let changed = adapt.handle_feedback(current_app_id.as_deref(), fb);
                if changed {
                    info!("learned suggestion changed for {:?} — re-resolving", current_app_id);
                    apply_config(&config_manager, &runtime, &adapt, &current_app_id, &current_focus);
                }
            }

            DaemonEvent::ProbeTimeout(app_id) => {
                if let Some(advice) =
                    adapt.probe_timeout(&app_id, current_app_id.as_deref(), current_focus.pid)
                {
                    notifier.notify_advice(&app_id, &advice);
                }
            }

            DaemonEvent::ConfigChanged => {
                match config_manager.reload_if_changed() {
                    Ok(true) => {
                        info!("setting.conf changed on disk — applying live");
                        apply_config(&config_manager, &runtime, &adapt, &current_app_id, &current_focus);
                    }
                    Ok(false) => {} // our own save() — mtime already tracked
                    Err(e) => warn!("Config reload error: {e}"),
                }
            }

            DaemonEvent::IpcRead { command, reply } => {
                let resp = ipc::handle_read_command(
                    &command,
                    config_manager.setting(),
                    adapt.learned_store(),
                );
                let _ = reply.send(resp);
            }

            DaemonEvent::IpcWrite { command } => {
                let resp = ipc::handle_write_command(&command, &mut config_manager);
                if resp.error.is_none() {
                    apply_config(&config_manager, &runtime, &adapt, &current_app_id, &current_focus);
                }
            }

        }
    }
    adapt.persist_now();
    if let Some(summary) = godmod::finish() {
        info!(
            "Godmod session: {} keys, {} commits ({} VN / {} EN), max latency {}µs",
            summary.total_keystrokes, summary.commits,
            summary.vietnamese_words, summary.english_words, summary.max_latency_us,
        );
    }
    info!("vi-ime daemon stopped");
}

/// Recompute the effective config (4-layer: user > learned > builtin >
/// global) for the current focus and push it live to the IME thread.
fn apply_config(
    config_manager: &ConfigManager,
    runtime: &RuntimeConfig,
    adapt: &Adaptation,
    current_app_id: &Option<String>,
    current_focus: &FocusEvent,
) {
    let setting = config_manager.setting();
    let title = browser_title(current_app_id, current_focus);
    let learned = adapt.learned_config(current_app_id.as_deref());
    let resolved = setting.effective_config_layered(
        current_app_id.as_deref(),
        title,
        learned.as_ref(),
    );
    runtime.store(&resolved_to_snapshot(&resolved));
}

/// Per-site rules apply only inside browsers (title = tab name).
fn browser_title<'a>(app_id: &Option<String>, focus: &'a FocusEvent) -> Option<&'a str> {
    app_id
        .as_deref()
        .filter(|id| AppCategory::classify(id) == AppCategory::Browser)
        .and(focus.title.as_deref())
}

fn get_config_path() -> PathBuf {
    let args: Vec<String> = std::env::args().collect();
    // First non-flag arg = explicit config path
    let config_arg = args.iter().skip(1).find(|a| !a.starts_with("--"));
    if let Some(path) = config_arg {
        PathBuf::from(path)
    } else {
        let local = std::env::current_dir().unwrap_or_default().join("setting.conf");
        if local.exists() { local } else { ConfigManager::default_path() }
    }
}

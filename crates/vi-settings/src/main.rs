// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! vi-settings — QML settings window launcher with auto-float.
//!
//! Two modes:
//!   `vi-settings`         → the configuration window (main.qml).
//!   `vi-settings --menu`  → the tray quick menu (menu.qml), a layer-shell
//!                           popup the tray opens on middle-click. A second
//!                           `--menu` while it is open closes it (toggle).
//!
//! Architecture:
//!   1. Finds the single `main.qml` (sibling dir, repo layout, or system share).
//!   2. Detects the wlroots compositor from env the COMPOSITOR itself sets
//!      (never asks the user to export anything — see project policy).
//!   3. Launches `quickshell` and asks the compositor to float + center the
//!      "vi-im Settings" toplevel, so it drops as a compact floating window
//!      instead of a big tiled one.
//!
//! All compositor IPC is best-effort: failures are ignored, never block the
//! main path, never panic. Unknown compositor => window opens tiled (graceful).

use std::env;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Duration;

/// Stable window identity the QML sets (title) and we match against.
const WIN_TITLE: &str = "vi-im Settings";

/// Detected wlroots compositor, from env only the compositor sets.
enum Compositor {
    Hyprland,
    Sway,
    Niri,
    Unknown,
}

fn main() {
    let menu_mode = env::args().any(|a| a == "--menu");
    // Word suggestion bar, started by the daemon (one instance per daemon).
    let bar_mode = env::args().any(|a| a == "--bar");
    let main_qml = find_qml();
    let qml = if menu_mode {
        main_qml.with_file_name("menu.qml")
    } else if bar_mode {
        main_qml.with_file_name("bar.qml")
    } else {
        main_qml
    };
    if !qml.exists() {
        eprintln!("vi-settings: QML file {:?} not found", qml);
        std::process::exit(1);
    }

    // Quickshell is single-instance per config file: a stale process (old
    // window hidden instead of quit, crash…) holds the lock and the new
    // launch dies with "already running" — the tray button then appears
    // dead. The user asked for a settings window NOW: replace any stale
    // instance of OUR qml (match on the qml path tail, so an unrelated
    // quickshell desktop bar is never touched).
    let replaced = kill_stale_instance(&qml);

    if menu_mode {
        // Middle-click on an open menu = close it, like any popup menu.
        if !replaced {
            spawn_quickshell(&qml);
        }
        return;
    }
    if bar_mode {
        // A restarted daemon replaces the old bar (its socket died anyway).
        spawn_quickshell(&qml);
        return;
    }

    let comp = detect_compositor();

    // Rule-based compositors must have the float+center rule installed BEFORE
    // the window maps, so set them prior to spawning the QML process.
    apply_pre_spawn_rules(&comp);

    // Only `quickshell` can run this QML: it imports Quickshell.Io (unix-socket
    // IPC to the daemon), which plain `qml`/`qmlscene` cannot resolve. Modern
    // quickshell selects a QML file via `-p <path>`.
    spawn_quickshell(&qml);

    // Action-based compositors (niri) float the window after it maps; this call
    // waits (bounded) for the window to appear, then floats + centers it.
    apply_post_spawn_float(&comp);
}

/// Start quickshell on `qml` and detach (dropping the handle does not kill
/// the child on Unix). The menu's "Cấu hình…" relaunches us, so tell the QML
/// where this binary is instead of trusting PATH.
fn spawn_quickshell(qml: &PathBuf) {
    let mut cmd = Command::new("quickshell");
    cmd.arg("-p").arg(qml);
    if let Ok(exe) = env::current_exe() {
        cmd.env("VI_SETTINGS_EXE", exe);
    }
    if cmd.spawn().is_err() {
        eprintln!(
            "vi-settings: `quickshell` not found. Install it (AUR: quickshell),\n\
             or control the daemon via CLI: vi-ime --switch / --toggle / --status"
        );
        std::process::exit(1);
    }
}

/// Kill a stale quickshell instance running OUR qml (see call site). The
/// pattern is `quickshell.*<last-two-path-components>` on the canonical
/// path, e.g. "quickshell.*vi-settings/main.qml" — specific enough to
/// never match somebody's quickshell bar. Returns true if one was killed.
fn kill_stale_instance(qml: &PathBuf) -> bool {
    let canon = qml.canonicalize().unwrap_or_else(|_| qml.clone());
    let mut parts: Vec<String> = canon
        .iter()
        .rev()
        .take(2)
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    parts.reverse();
    let pattern = format!("quickshell.*{}", parts.join("/"));
    if Command::new("pkill")
        .args(["-f", &pattern])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        eprintln!("vi-settings: đã thay instance cũ đang giữ lock");
        // Give the old process a beat to release the single-instance lock.
        thread::sleep(Duration::from_millis(150));
        return true;
    }
    false
}

/// Detect the compositor purely from env the COMPOSITOR sets — never requires
/// the user to export anything (project policy).
fn detect_compositor() -> Compositor {
    if env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        Compositor::Hyprland
    } else if env::var_os("SWAYSOCK").is_some() {
        Compositor::Sway
    } else if env::var_os("NIRI_SOCKET").is_some() {
        Compositor::Niri
    } else {
        Compositor::Unknown
    }
}

/// Install float+center window rules that apply when the window maps.
fn apply_pre_spawn_rules(comp: &Compositor) {
    match comp {
        Compositor::Hyprland => {
            // Hyprland ≥0.55 is configured in Lua and rejects `keyword`
            // ("can't work with non-legacy parsers"); older ones only know
            // `keyword`. Try Lua first, fall back when it isn't understood.
            let lua = format!(
                "hl.window_rule({{ name = \"vi-im-settings\", \
                 match = {{ title = \"^({})$\" }}, float = true, center = true }})",
                WIN_TITLE
            );
            let lua_ok = Command::new("hyprctl")
                .args(["eval", &lua])
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "ok")
                .unwrap_or(false);
            if !lua_ok {
                let title_rule = format!("float,title:^({})$", WIN_TITLE);
                let center_rule = format!("center,title:^({})$", WIN_TITLE);
                run_quiet("hyprctl", &["keyword", "windowrulev2", &title_rule]);
                run_quiet("hyprctl", &["keyword", "windowrulev2", &center_rule]);
            }
        }
        Compositor::Sway => {
            let rule = format!(
                "for_window [title=\"{}\"] floating enable, move position center",
                WIN_TITLE
            );
            run_quiet("swaymsg", &[&rule]);
        }
        _ => {}
    }
}

/// For action-based compositors (niri), find the freshly mapped window by title
/// and move it to the floating layout + center it. Bounded retry: the window
/// may not exist yet the instant the process starts.
fn apply_post_spawn_float(comp: &Compositor) {
    match comp {
        Compositor::Niri => float_niri(),
        // Rule-based / unknown compositors need no post-spawn action; give the
        // process a moment to come up before we detach.
        _ => thread::sleep(Duration::from_millis(300)),
    }
}

fn float_niri() {
    for _ in 0..15 {
        if let Some(id) = find_niri_window_id(WIN_TITLE) {
            let id = id.to_string();
            run_quiet(
                "niri",
                &["msg", "action", "move-window-to-floating", "--id", &id],
            );
            run_quiet("niri", &["msg", "action", "center-window", "--id", &id]);
            return;
        }
        thread::sleep(Duration::from_millis(150));
    }
}

/// Query `niri msg -j windows` and return the id of the window whose title
/// matches `title`. Lightweight scan (no JSON dep): the window's `"id":` is the
/// first field of its object, so the last `"id":` before the title marker is it.
fn find_niri_window_id(title: &str) -> Option<u64> {
    let out = Command::new("niri")
        .args(["msg", "-j", "windows"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let marker = format!("\"title\":\"{}\"", title);
    let idx = text.find(&marker)?;
    let id_pos = text[..idx].rfind("\"id\":")?;
    let after = &text[id_pos + "\"id\":".len()..];
    let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// Run a command, ignoring all failures (missing binary, non-zero exit, IPC
/// error). Best-effort: never blocks meaningfully, never panics.
fn run_quiet(program: &str, args: &[&str]) {
    let _ = Command::new(program).args(args).status();
}

fn find_qml() -> PathBuf {
    if let Ok(path) = env::var("VI_SETTINGS_QML") {
        let p = PathBuf::from(&path);
        if p.exists() {
            return p;
        }
    }

    if let Ok(exe) = env::current_exe()
        && let Some(dir) = exe.parent() {
            for name in &[
                "vi-settings/main.qml",
                "../../vi-settings/main.qml",
                "../vi-settings/main.qml",
            ] {
                let p = dir.join(name);
                if p.exists() {
                    return p;
                }
            }
        }

    // install.sh puts the QML here (per-user install, no root).
    if let Some(data) = dirs_data_home() {
        let p = data.join("vi-im/qml/main.qml");
        if p.exists() {
            return p;
        }
    }

    for path in &[
        "/usr/share/vi-im/qml/main.qml",
        "/usr/local/share/vi-im/qml/main.qml",
    ] {
        let p = PathBuf::from(path);
        if p.exists() {
            return p;
        }
    }

    PathBuf::from("vi-settings/main.qml")
}

/// $XDG_DATA_HOME, falling back to ~/.local/share.
fn dirs_data_home() -> Option<PathBuf> {
    env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
}

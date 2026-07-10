// SPDX-License-Identifier: GPL-3.0-or-later OR Commercial
// Copyright (c) 2024-2026 vi-im contributors
//! Game process auto-detection via /proc.
//!
//! Called on focus change when the compositor IPC provides a PID (niri does).
//! Reads /proc/PID/comm and /proc/PID/exe to decide whether the focused
//! application is a game — where IME processing is unwanted. Best-effort:
//! any I/O error simply means "not a game".
//!
//! # Detection signals (all case-insensitive):
//! - `comm` matches known game runtimes/launchers
//! - `exe` path contains game-platform directories (Steam, Proton, Wine, etc.)
//! - `comm` or `exe` contains well-known Linux-native game binaries

use std::fs;

// ── Known game process names (comm field, 15-char limit on Linux) ──────────

/// Processes that indicate the focused window IS a game or game runtime.
const GAME_COMMS: &[&str] = &[
    // Runtimes & launchers
    "steam", "wineserver", "wine", "wine-preloader", "wine64",
    "wine64-preload", "proton", "gamescope", "gamescope-wl",
    "lutris", "heroic", "bottles", "mangohud",
    // Valve / Source engine games
    "cs2", "csgo_linux64", "hl2_linux", "dota2", "dota",
    "tf_linux64", "portal2_linux", "l4d2_linux",
    // Common native Linux games
    "runescape", "osu-lazer", "minecraft", "terraria", "factorio",
    "stardew_valley", "celeste", "hollow_knight", "deadcells",
    "hades", "hades2", "slay_the_spire", "darkest_dungeon",
    "rimworld", "baldurs_gate3", "bg3", "eldenring", "cyberpunk2077",
    "witcher3", "skyrim", "fallout4", "fallout76", "warframe",
    "warframe.x64", "rocketleague", "overwatch", "wow",
    "wowclassic", "diablo4", "pathofexile", "poe2",
    "leagueoflegend", "valorant", "apex_legend",
    "pubg", "fortnite", "rainbowsix", "doom", "quake",
    "borderlands", "bioshock", "gta5", "reddead",
    "fifa", "nba2k", "callofduty", "battlefield",
    "destiny2", "splitgate", "thefinals", "marvelrivals",
    "helldivers2", "palworld", "enshrouded", "vrising",
    "valheim", "satisfactory", "subnautica", "no_mans_sky",
    "starfield", "star_citizen", "elite_danger", "x4",
    "total_war", "civilization", "civ6", "civ7",
    "age_of_empire", "anno", "tropico",
    "cities_skylin", "planet_zoo", "planet_coaste",
    "jurassic_worl", "farming_simul", "euro_truck",
    "american_truc", "beamng", "assetto_cors",
    "iracing", "dirt", "f1", "forza", "trackmania",
];

/// Substrings of exe path that strongly indicate a game.
const GAME_EXE_SUBSTRINGS: &[&str] = &[
    "steamapps/common", "/Steam/", "Proton ",
    "/Proton/", "Proton-", "/wine/", "wine-",
    "wine64", "lutris", "bottles", "heroic",
    "dosdevices", "drive_c", "GOG Games", "Epic Games",
    "Ubisoft", "EA Games", "Rockstar Games",
    "Battle.net", "Riot Games", "Bethesda",
];

// ── Public API ──────────────────────────────────────────────────────────────

/// Check whether the process with the given PID looks like a game.
pub fn is_game_process(pid: i32) -> bool {
    let comm = read_comm(pid);
    if comm.is_empty() {
        return false;
    }
    let comm_lower = comm.to_lowercase();

    if check_by_comm(&comm_lower) {
        return true;
    }

    let exe = read_exe(pid);
    let exe_lower = exe.to_lowercase();
    check_by_exe(&exe_lower)
}

/// Detect a game by its `app_id` / window class alone — for compositors whose
/// focus IPC does not provide a PID (wlr foreign-toplevel on Sway/Hyprland/
/// river). The class string is the only signal we get there, so we match the
/// same game-platform substrings used for the exe path (`check_by_exe`) plus
/// the launcher/store reverse-DNS and Steam's per-game window ids.
///
/// Examples that resolve true:
/// - `com.valvesoftware.Steam`, `org.lutris.Lutris`, `com.heroicgameslauncher.hgl`
/// - `steam_app_1234560` (a running Steam game window)
/// - `wine`, `proton`, `gamescope` (any prefix)
pub fn is_game_app_id(app_id: &str) -> bool {
    let a = app_id.to_lowercase();
    if a.is_empty() {
        return false;
    }
    // Same platform substrings as the exe-path check (steam, lutris, heroic,
    // bottles, proton, wine, gamescope, Epic, GOG, Ubisoft, Battle.net, …).
    if check_by_exe(&a) {
        return true;
    }
    // Steam's per-game windows are named `steam_app_<appid>`.
    if a.starts_with("steam_app_") {
        return true;
    }
    // Any known game comm name appearing as a substring of the class
    // (e.g. a game that advertises its binary name as the app_id).
    GAME_COMMS.iter().any(|g| a == *g || a.contains(g))
}

/// Check by `comm` name alone (unit-testable without /proc).
pub fn check_by_comm(comm_lower: &str) -> bool {
    for name in GAME_COMMS {
        if comm_lower == *name {
            return true;
        }
    }
    comm_lower.starts_with("wine-")
        || comm_lower.contains("proton")
        || comm_lower.contains("gamescope")
}

/// Check by exe path substring (unit-testable without /proc).
pub fn check_by_exe(exe_lower: &str) -> bool {
    for sub in GAME_EXE_SUBSTRINGS {
        if exe_lower.contains(sub) {
            return true;
        }
    }
    false
}

// ── /proc readers ───────────────────────────────────────────────────────────

fn read_comm(pid: i32) -> String {
    fs::read_to_string(format!("/proc/{pid}/comm"))
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn read_exe(pid: i32) -> String {
    fs::read_link(format!("/proc/{pid}/exe"))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_id_launcher_reverse_dns() {
        assert!(is_game_app_id("com.valvesoftware.Steam"));
        assert!(is_game_app_id("org.lutris.Lutris"));
        assert!(is_game_app_id("com.heroicgameslauncher.hgl"));
    }

    #[test]
    fn app_id_steam_game_window() {
        assert!(is_game_app_id("steam_app_1234560"));
        assert!(is_game_app_id("STEAM_APP_999"));
    }

    #[test]
    fn app_id_plain_comm() {
        assert!(is_game_app_id("cs2"));
        assert!(is_game_app_id("eldenring"));
        assert!(is_game_app_id("wine-preloader"));
    }

    #[test]
    fn app_id_non_game() {
        assert!(!is_game_app_id(""));
        assert!(!is_game_app_id("org.wezfurlong.wezterm"));
        assert!(!is_game_app_id("foot"));
        assert!(!is_game_app_id("firefox"));
    }

    #[test]
    fn pid_comm_game() {
        // check_by_comm matches the known comm list.
        assert!(check_by_comm("cs2"));
        assert!(check_by_comm("wineserver"));
        assert!(!check_by_comm("kitty"));
    }

    #[test]
    fn exe_substring_game() {
        assert!(check_by_exe("/home/user/.steam/steam/steamapps/common/Game/Game.x86_64"));
        assert!(check_by_exe("/mnt/games/Proton 9/proton"));
        assert!(!check_by_exe("/usr/bin/foot"));
    }
}


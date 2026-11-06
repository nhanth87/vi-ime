# Lessons learned — vi-ime v7.4.x (2026-10-24 → 2026-11-04)

Each entry: what the user saw → the real cause → the fix → the lesson.
Rules distilled from these live in `AGENTS.md` (R24–R34).

## Method

- **Read the log before patching (R22).** Every fix below that stuck began with
  `[LIVE-SYNC]` / `[EVDEV-SYNC]` / `[KEY-IN]` lines from the moment of the bug.
  Two guessed fixes in July had to be reverted.
- **Reproduce in the real engine, then delete the probe.** A temporary
  `engine/zz_probe.rs` (or the scratch `xprobe` crate) replaying the exact keys
  from the log settled "sẵ" → "sa84", "core" → "cỏe", "H" → "I" in minutes.
  Probes that `#[path]`-include daemon code are not `cfg(test)` — point
  `XDG_DATA_HOME`/`HOME` at a scratch dir or they overwrite user data.
- **Sweep the logs, not one example.** Checking every committed word against the
  system vi_VN dictionary found the only real Tự do error ("cỏe") among ~500
  words; scanning all words with stray digits showed "sa84" was a pattern.
- **Separate the IME from its environment.** Several "regressions" were the
  IME switched off from the tray, machine load ~8, or the user typing during a
  field test. Check `enabled`, load and focus before trusting a result.
- **Don't loop.** Re-arming test runs and re-reading logs with nothing new
  changed burned the user's tokens without finding bugs. Stop and wait for a
  report once the open items need the user.

## Typing-path bugs

| User saw | Cause | Fix |
|---|---|---|
| OnlyOffice "thủ tướng" → "thủ tớng" at fast typing | Coalesced typer commands put the next key 4 ms after an accented glyph; CEF drops it | Accent settle per key and before every uinput forward (R30b) |
| OnlyOffice still wrong after switching from Chrome | Grab kept Chrome's profile | Re-grab with the new app's profile on evdev→evdev focus change (R30) |
| "sẵ" → "sa84", "kiể" → "kie63" | Incomplete word restored to raw VNI keys | Raw keys with mark digits are never restored (R28) |
| "khó" → "kho1" | Kernel repeat (250 ms) counted as a keystroke under the grab | Ignore value-2 events for composing keys |
| Reconnected keyboard types raw "a1" | Hotplug watcher never forgot a removed node; kernel reuses eventN | Watch DELETE (R24b) |
| Daemon at 95 % CPU after unplugging | Reader looped on POLLHUP | Reader exits on hangup (R24b) |
| "Press the Windows key to type" / 5–10 min wait after start | Hyprland handed the grab vi-ime's own keymap (typer) or none | Ignore own keymap, build the configured layout (R33) |
| Same, on Chrome | X11 fallback engaged on Wayland Chrome (cmdline guess) | Ask Hyprland whether the window is XWayland (R33) |
| "core" → "cỏe" after a Vietnamese sentence | Context overrode the dictionary check | Fake Vietnamese never beats an English word |
| "test" → "tét", "bar" → "bả" in Vietnamese sentences | Context always won between two real words | Common English beats rare Vietnamese; prefer-list for everyday words |
| Capital "H" → "I" in Chrome | Typer keymap packed ASCII in order; Chrome added the held Shift | Keymap in (lower, Shift) pairs (R33) |
| No Vietnamese in the address bar | URL fields were raw passthrough | Live echo with Delete before BackSpace (R33) |
| "hút" → "hhuhuthúthút" in a YouTube comment box | The field sent surrounding text, so vi-ime used delete_surrounding + commit; Chrome ignored the delete | delete_surrounding path disabled for good (R33) |
| "độc" → "đô5c" on some sites | Page re-render made Chrome deactivate/reactivate after every edit; each blink dropped the word | Blink tolerance: same app, no click, < 150 ms keeps the word (R33) |
| "ví" → "vií" in the New Tab page search box | It inline-completes from history like the address bar, but is a normal field | Delete-before-BackSpace there too (title "New Tab") |
| Address bar words appear after 2–3 s | Silent mode waited for boundary / 1.5 s idle commit | Back to live echo |

## Footprint

- vi-ime showed 28 MB / 22 threads. Measured: 21 MB of that was shared libraries,
  the real share (PSS) ~9 MB — but GTK, used only for the tray icon, linked 74
  libraries and 9 threads, and a missing appindicator3 stopped the daemon from
  starting at all. Replaced by `ksni` (pure-Rust D-Bus tray): 5 libraries.

## Not resolved (evidence logging in place)

- Same key arriving twice 55–67 ms apart on the evdev path ("chơ" → "cho7"):
  `[EVDEV-DUP]` logs whether a release came between the presses.
- A word ending one key early ("sẵ" before its "n"): `[EVDEV-END]` names the
  key that ended it.

# 🇻🇳 vi-im Design Document

> **Source:** Recalled from Supermemory
> **Date:** 2026-07-07

---

## 1. Vision

vi-im is a **native Vietnamese Input Method Editor for Wayland**, built in Rust.
It bypasses IBus/Fcitx, using `input-method-v2` + `input-method-keyboard-grab-v1`
protocols to run natively on wlroots-based compositors (Niri, Hyprland, COSMIC).

### Core Principles

- **"Statistic & Structure" approach** — syllable-level Vietnamese tone placement
  and diacritic restoration using efficient algorithms, avoiding heavy AI/LLM.
- **"Parse, don't mutate"** — raw keys are source of truth; every keystroke
  re-parses the entire word.
- **Zero-CPU Idle** — daemon main loop is a single blocking `rx.recv()`, no polling.

---

## 2. Unified Binary Architecture

Gộp 3 binary (`vi-daemon`, `vi-settings`, `vi-status`) → 1 binary `vi-im`.

```
┌──────────────────────────────────────────────────────┐
│ vi-im (single binary)                                │
│                                                      │
│ main()                                               │
│  ├── ConfigManager (vi-config)                       │
│  ├── TrayIcon (vi-tray, GTK)                         │
│  │    ├── left-click  → ToggleIme                    │
│  │    └── right-click → Menu (EN/VNI/Telex/Smart/⚙️) │
│  ├── DaemonEvent bus (mpsc channel)                  │
│  │    ├── Focus events (niri/wlr IPC)                │
│  │    ├── Tray(TrayMessage)                          │
│  │    ├── ImeFeedback (Wayland IME → daemon)         │
│  │    ├── ConfigChanged (inotify)                    │
│  │    └── probe_timeout (app-support detection)      │
│  ├── Wayland IME thread (vi-wayland-im + vi-engine)  │
│  │    └── shared Arc<RuntimeConfig>                  │
│  └── Settings window (QML, launched on demand)       │
└──────────────────────────────────────────────────────┘
```

---

## 3. Data Flow Pipeline (Immutable)

```
KeyEvent → [vi-godmod log] → vi-wayland-im (keyboard grab)
  → vi-plugin.pre_process_key()           ← short-circuit hook
  → vi-engine.push_key()                  ← Telex/VNI processing
  → NonPreeditAction                      ← result
  → vi-plugin.post_process_action()       ← modify hook
  → vi-wayland-im.apply_action()          → Wayland commit
```

---

## 4. Input Methods

| Method | Description |
|--------|-------------|
| Telex | Traditional Telex typing |
| VNI | VNI typing |
| **Smart** | Auto-detect mixed VNI + Telex within same word |

### Smart IME
- Auto-detects and allows mixed VNI and Telex typing within the same word.
- Uses phonotactic validation (R9: English Restore by validity, not key counting).
- Syllable-level parsing via phonotactic tables.

---

## 5. Unicode Algebra (Engine)

```
raw_keys (source of truth) → normalize (Telex/VNI + undo)
  → analyze (phonotactic tables: initial/cluster/coda)
  → render (tone = data offset from VOWEL_CLUSTERS table)
```

- Tone placement is **data-driven** (2 columns classic/modern in `VOWEL_CLUSTERS`).
- Tone = Unicode combining codepoint + NFC composition (`glyph.rs`).
- Only exception: `đ` (not NFC-composable, special-cased).
- Undo: unified merge span; tone key ×2 → cancel tone + literal key.
- Backtracking initial for `gi`/`qu` (gì=g+i, già=gi+a).

---

## 6. ImeMode Contract (Live Model)

### Preedit
- `set_preedit` per keystroke (only for apps supporting preedit well).

### NonPreedit / Hybrid (live surrounding-text)
- App always shows current render of the word.
- Per keystroke → diff suffix with `committed_word`:
  `delete_surrounding_text(delta_bytes)` + `commit_string(suffix)`.
- Does NOT forward raw key chars through virtual keyboard.

### Virtual Keyboard
- `zwp_virtual_keyboard_v1` only for passthrough keys:
  shortcuts (Ctrl/Alt/Super), navigation, Enter/Tab/Esc, boundary key replay.

---

## 7. Two-Phase Commit (R7)

```
delete_surrounding_text (BYTES) → wait Done (timeout 150ms force)
  → commit_string
```

- Key buffer holds keys while waiting for Done ack.

---

## 8. Config Resolution (4 Layers — R13)

```
user > learned > builtin > global
```

- `Setting::effective_config_layered(app_id, title, learned)`
- `ResolvedConfig` carries `mode_source`/`origin` (badge: user/learned/default).
- `tone_style` is global-only.
- Settings UI = separate process (`vi-settings`) writes `setting.conf`;
  daemon picks up via inotify → `ConfigManager::reload_if_changed()`.

---

## 9. App Support Detection (R11)

```
IME thread ──ImeFeedback──▶ daemon (Adaptation)
  Activated / SurroundingTextSeen / DoneAck{μs} / DoneTimeout
  / Unavailable / KeyReorder / KeyChatter
```

- Probe: focus change → probe thread (1.5s sleep, separate thread — main loop
  stays pure `recv`) → ProbeTimeout → if never Activated: notify 1x/app/session.

---

## 10. Per-field ContentType (R11b)

| ContentType | Behavior |
|-------------|----------|
| Password/Pin | Engine OFF + passthrough + no logging |
| Terminal | Force hidden/live (unless user mode override) |
| Digits/Number/Phone/Date/Time | Passthrough |
| Normal | Reset on every Activate/Deactivate |

---

## 11. Live Reconfiguration (R12)

```
daemon ──store()──▶ Arc<RuntimeConfig> (atomics + generation)
                         │ snapshot() on generation change
IME thread: maybe_reconfigure() at process_key + Activate
```

- Commit pending before apply (R8).
- enabled→disabled: release keyboard grab.
- disabled→enabled: re-grab at Activate.

---

## 12. Crate DAG

```
vi-engine (leaf: unicode-normalization only)
  ↑
vi-wayland-im
  ↑
vi-daemon (root)
```

No cycles. `vi-engine` is the only leaf.

---

## 13. Key Files

| File | Lines | Role |
|------|-------|------|
| `vi-engine/src/engine.rs` | ~170 | Engine facade over parser |
| `vi-engine/src/parser/tables.rs` | ~95 | Phonotactic tables + tone offsets |
| `vi-engine/src/parser/normalize.rs` | ~290 | Telex/VNI modifiers + undo |
| `vi-engine/src/parser/analyze.rs` | ~90 | Initial/cluster/coda + backtrack |
| `vi-engine/src/parser/render.rs` | ~90 | Render + case; tone via glyph |
| `vi-engine/src/parser/glyph.rs` | ~76 | Unicode algebra (NFC composition) |
| `vi-wayland-im/src/state.rs` | ~290 | ImeAppState + buffer + reconfigure |
| `vi-wayland-im/src/commit.rs` | ~142 | Two-phase sync_word |
| `vi-wayland-im/src/dispatch.rs` | ~270 | IM + grab dispatch |
| `vi-daemon/src/main.rs` | ~200 | Entry + blocking loop |
| `vi-daemon/src/learning.rs` | ~190 | Adaptation (feedback → learned) |
| `vi-config/src/effective.rs` | ~177 | effective_config (4-layer) |
| `vi-config/src/builtin.rs` | ~93 | Builtin profile tables |
| `vi-config/src/learned.rs` | ~134 | LearnedStore |

---

*Generated by `vi-supermemory recall` from Supermemory.*

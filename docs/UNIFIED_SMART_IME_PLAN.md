# 🇻🇳 vi-im: Unified Binary + Smart IME — Implementation Plan

> **Date:** 2026-07-07
> **Status:** Draft — awaiting approval
> **Estimated total:** 22–32 hours
> **Source:** Recalled from Supermemory

---

## 1. Mục tiêu

### 1.1 Gộp 3 binary → 1 binary duy nhất

Hiện tại có 3 binary riêng biệt:
- `vi-daemon` — daemon chính (Wayland IME, CLI controls)
- `vi-settings` — launcher QML settings (30 dòng Rust)
- `vi-status` — cửa sổ trạng thái nổi QML (cùng crate với daemon)

**Mục tiêu:** Gộp thành 1 binary `vi-im`, chạy là có tray icon ngay.

### 1.2 Tray icon với menu đầy đủ

- **Click trái** → Toggle English ↔ Vietnamese (bật/tắt IME)
- **Click phải** → Menu:
  - 🇬🇧 English (tắt IME, passthrough)
  - 🇻🇳 VNI
  - 🇻🇳 Telex
  - 🇻🇳 Smart (VNI + Telex lẫn lộn, auto-detect)
  - ⚙️ Cấu hình (mở settings QML)
  - ❌ Thoát

---

## 2. Current Architecture Analysis

### 2.1 Workspace hiện tại

```
vi-im/
├── Cargo.toml          (workspace: vi-engine, vi-daemon, vi-settings)
├── crates/
│   ├── vi-engine/      → leaf crate, engine Telex/VNI processing
│   ├── vi-daemon/      → daemon chính + config types
│   ├── vi-settings/    → QML settings launcher
│   ├── vi-tray/        → [chưa trong workspace] tray icon GTK
│   ├── vi-config/      → [chưa trong workspace] config types
│   └── ...
```

### 2.2 Crate mồ côi (chưa trong workspace)

| Crate | Vị trí | Trạng thái |
|-------|--------|-----------|
| `vi-tray` | `crates/vi-tray/` | Library tray icon (`tray-icon` v0.24, GTK). Có sẵn `TrayIcon`, `TrayMessage`, `ImeStatus`. **Chưa wired vào daemon.** |
| `vi-config` | `crates/vi-config/` | Config types (`InputMethod`, `ImeMode`, `Setting`). **Trùng lặp** với `vi-daemon/src/config/`. |

### 2.3 Engine hiện tại

- `InputMethod`: chỉ có `Telex`, `Vni` (chưa có Smart)
- `auto_detect_lang`: phonotactic validation (R9)
- Two-phase commit: `delete_surrounding_text` + `commit_string` (R7)
- Live model (NonPreedit/Hybrid): diff suffix mỗi phím
- **Bug đã fix 07/07**: `fast_engine::is_word_boundary` thiếu tham số `method`

### 2.4 Luồng dữ liệu hiện tại

```
KeyEvent → pre_process_key() → engine.push_key() → NonPreeditAction
  → post_process_action() → apply_action() → Wayland commit
```

### 2.5 Config flow

```
setting.conf → ConfigManager → Arc<RuntimeConfig> → IME thread
                    ↑ inotify watch (auto-reload)
```

---

## 3. Kiến trúc đề xuất

### 3.1 Unified Binary

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

### 3.2 Tray Menu Structure

```
┌──────────────────────────┐
│ vi-im · Smart · Bật     │ ← status bar (read-only)
├──────────────────────────┤
│ 🇬🇧 English              │ ← disable IME, passthrough
│ 🇻🇳 VNI           ✓      │ ← InputMethod::Vni
│ 🇻🇳 Telex                │ ← InputMethod::Telex
│ 🇻🇳 Smart                │ ← InputMethod::Smart
├──────────────────────────┤
│ ⚙️ Cấu hình...           │ ← launch QML settings
├──────────────────────────┤
│ ❌ Thoát                 │ ← graceful shutdown
└──────────────────────────┘
```

---

## 4. Implementation Phases

### Phase 1: Unified Binary (Foundation) — 4–6h

**Dependencies:** Không

| Step | File(s) | Changes |
|------|---------|---------|
| 1.1 | `Cargo.toml` (root) | Thêm `vi-tray`, `vi-config` vào workspace members |
| 1.2 | `vi-daemon/Cargo.toml` | Thêm `vi-tray` (optional, feature `tray`), `vi-config` (required) |
| 1.3 | `vi-daemon/src/events.rs` | Thêm `DaemonEvent::Tray(TrayMessage)` variant |
| 1.4 | `vi-daemon/src/main.rs` | Tích hợp `TrayIcon::with_callback()`, xử lý tray events |
| 1.5 | `deploy/compile.sh` | Cập nhật build command: `cargo build -p vi-daemon --features tray` |

### Phase 1b: Dedup Config Types — 1–2h

**Dependencies:** Phase 1

| Step | File(s) | Changes |
|------|---------|---------|
| 1b.1 | `vi-config/src/types.rs` | Thêm `InputMethod::Smart` + serde |
| 1b.2 | `vi-daemon/Cargo.toml` | Thêm `vi-config` dependency |
| 1b.3 | `vi-daemon/src/config/mod.rs` | Xóa `types.rs` trùng lặp, re-export từ `vi-config` |
| 1b.4 | All daemon files | Sửa imports `crate::config::*` → `vi_config::*` |

### Phase 2: InputMethod::Smart (Core Engine) — 8–12h

**Dependencies:** Phase 1b

| Step | File(s) | Changes |
|------|---------|---------|
| 2.1 | `vi-engine/src/types.rs` | Thêm `InputMethod::Smart` variant |
| 2.2 | `vi-engine/src/parser/normalize.rs` | **Hàm mới `normalize_smart()`**: mixed Telex/VNI processing |
| 2.3 | `vi-engine/src/parser/mod.rs` | Gọi `normalize_smart` khi method == Smart |
| 2.4 | `vi-engine/src/engine.rs` | Cập nhật `is_word_boundary` cho Smart |
| 2.5 | `vi-engine/src/fast_engine.rs` | Cập nhật `is_word_boundary` helper (đã fix) |
| 2.6 | `vi-wayland-im/src/runtime.rs` | Encode/decode Smart (giá trị = 2) |
| 2.7 | `vi-daemon/src/sync.rs` | Map Smart trong `resolved_to_snapshot()` |

### Phase 3: Tray-Only Config Menu — 3–4h

**Dependencies:** Phase 1
**Changed:** Thay QML settings UI bằng tray-only config menu.

| Step | File(s) | Changes |
|------|---------|---------|
| 3.1 | `vi-tray/src/lib.rs` | `TrayMessage::SetMethod(InputMethod)` thay `SwitchInputMethod` |
| 3.2 | `vi-tray/src/lib.rs` | Menu 5 items: English, VNI, Telex, Smart, Settings |
| 3.3 | `vi-tray/src/lib.rs` | Left-click handler (toggle ENG/VI) |
| 3.4 | `vi-tray/src/lib.rs` | Tray icon đổi tooltip + màu sắc theo trạng thái |
| 3.5 | `vi-daemon/src/main.rs` | Handle `SetMethod`, `ToggleIme`, `OpenSettings` |
| 3.6 | `vi-daemon/src/main.rs` | Settings: tray-only dialog (không cần QML) |

### Phase 4: ibus-style Commit Optimization — 4–6h

**Dependencies:** Phase 2

| Step | File(s) | Changes |
|------|---------|---------|
| 4.1 | `vi-wayland-im/src/commit.rs` | Burst commit: merge pure-append phase-2 |
| 4.2 | `vi-wayland-im/src/state.rs` | Thêm `COMMIT_BURST_WINDOW` (300ms) |
| 4.3 | Tests | Smart mode commit test cases |

### Phase 5: Testing & Polish — 3–4h

**Dependencies:** Phase 1–4

| Step | File(s) | Changes |
|------|---------|---------|
| 5.1 | `tests/vi-engine/smart_tests.rs` | 20+ test cases cho Smart normalize |
| 5.2 | `AGENTS.md` | Cập nhật types table, R9 cho Smart |
| 5.3 | `README.md` | Hướng dẫn Smart mode, build mới |
| 5.4 | Full regression | `cargo test --workspace` — 200+ tests pass |

---

## 5. Risks & Mitigations

### R1: Smart normalize ambiguity

**Risk:** Phím `s`, `f`, `r`, `x`, `j` vừa là tone key (Telex) vừa là chữ cái (VNI).

**Mitigation:**
- Context-based: sau vowel → tone, đầu/cuối từ → literal
- Fallback: cả 2 hợp lệ → ưu tiên Telex (quen thuộc hơn)
- User ép literal bằng double-key undo (R14)

### R2: Config type duplication

**Risk:** Merge `vi-config` vào `vi-daemon` gây breakage.

**Mitigation:** `vi-config` là leaf crate (không dep vào ai), an toàn để merge.

### R3: GTK tray trên Wayland

**Risk:** `tray-icon` dùng GTK có thể không hoạt động trên pure Wayland.

**Mitigation:** `tray-icon` v0.24 hỗ trợ `zwp_status_notifier` (Wayland-native). Fallback: daemon vẫn chạy nếu tray fail.

### R4: Burst commit mất chữ

**Risk:** Merge phase-2 pending làm mất text.

**Mitigation:** Chỉ merge pure-append (del=0). Vẫn giữ 150ms DONE_TIMEOUT (R7).

### R5: Performance

**Risk:** Smart mode thử 2 interpretations → chậm hơn.

**Mitigation:** Engine re-parse toàn bộ từ (≤12 chars) đã ở nanosecond scale. Smart chỉ ảnh hưởng normalize (O(n)).

---

## 6. AGENTS.md Compliance

- [x] R1: Data Flow Pipeline không đổi (Smart là thay đổi bên trong normalize)
- [x] R2: ImeMode Contract không đổi (Smart là method mới, không phải mode)
- [x] R4: File ≤300 dòng (src), ≤600 dòng (tests)
- [x] R5: Crate DAG không cycle (vi-config → leaf, vi-tray → leaf)
- [x] R10: Mỗi pub fn ≥1 test, mỗi bug fix có regression test

---

## 7. File Map Changes

| File | Action |
|------|--------|
| `Cargo.toml` (root) | + `vi-tray`, `vi-config` vào members |
| `vi-daemon/Cargo.toml` | + `vi-tray` (optional), `vi-config` |
| `vi-daemon/src/main.rs` | + TrayIcon integration, tray event handling |
| `vi-daemon/src/events.rs` | + `DaemonEvent::Tray(TrayMessage)` |
| `vi-daemon/src/config/mod.rs` | - `types.rs`, re-export từ `vi-config` |
| `vi-engine/src/types.rs` | + `InputMethod::Smart` |
| `vi-config/src/types.rs` | + `InputMethod::Smart` + serde |
| `vi-engine/src/parser/normalize.rs` | + `normalize_smart()` ~150 dòng |
| `vi-engine/src/parser/mod.rs` | + Smart dispatch trong `parse()` |
| `vi-wayland-im/src/runtime.rs` | + Smart encode/decode |
| `vi-daemon/src/sync.rs` | + Smart mapping |
| `vi-tray/src/lib.rs` | Restructure menu, `SetMethod` |
| `vi-wayland-im/src/commit.rs` | + Burst commit logic |
| `tests/vi-engine/smart_tests.rs` | **NEW** — 20+ test cases |
| `AGENTS.md` | + Smart mode docs |
| `README.md` | + Smart mode hướng dẫn |
| `vi-engine/src/engine/smart.rs` | **NEW** — `normalize_smart()` + glide onset |
| `vi-im/src/burst.rs` | **NEW** — `BurstCommitter` 300ms window |
| `vi-im/src/game_mode.rs` | **NEW** — `GameModeDetector` Ctrl+Shift+G |

---

## 8. Phase 6: Game Mode — 2–3h

**Dependencies:** Phase 1

Bypass IME hoàn toàn khi detect game app. Dùng hybrid approach:
- **Auto-detect** qua `text-input-v3` monitoring (app không bind → game)
- **Manual override** hotkey `Ctrl+Shift+G`

| Step | File(s) | Changes |
|------|---------|---------|
| 6.1 | `vi-im/src/game_mode.rs` | `GameModeDetector` struct: auto-detect + manual toggle |
| 6.2 | `vi-wayland-im/src/dispatch.rs` | Check `game_mode.is_active()` trước khi process key |
| 6.3 | `vi-daemon/src/events.rs` | Hotkey handler `Ctrl+Shift+G` → `ToggleGameMode` |
| 6.4 | Tests | Game mode unit tests |

---

## 9. Code Skeletons

### 9.1 normalize_smart() — NFD Math Engine

Thay thế bảng `VOWEL_CLUSTERS` (55 entries) bằng Unicode Combining Diacritical
Marks để tính toán dynamic, giảm memory, fix edge cases `qu`/`gi`.

```rust
/// vi-engine/src/engine/smart.rs
pub fn normalize_smart(raw_keys: &[char]) -> String {
    // 1. Strip glide onsets trước
    let (onset, rest) = strip_glide_onset(raw_keys); // "qu", "gi"

    // 2. Parse phần còn lại, Telex preferred
    let syllable = parse_mixed(rest);

    // 3. Apply tone placement (NFD math)
    let nfd = place_tone_nfd(&syllable);

    // 4. Reassemble onset + normalize to NFC
    onset.to_string() + &to_nfc(&nfd)
}

fn strip_glide_onset(keys: &[char]) -> (&str, &[char]) {
    if keys.starts_with(&['q', 'u']) { return ("qu", &keys[2..]); }
    if keys.starts_with(&['g', 'i']) { return ("gi", &keys[2..]); }
    ("", keys)
}
```

**Pipeline:** Tokenize → NFD units → Parse ASCII syllabic structure →
Calculate tone placement per vowel clusters → Normalize NFC before commit.

### 9.2 BurstCommitter — 300ms Window

```rust
/// vi-im/src/burst.rs
pub struct BurstCommitter {
    raw_keys: Vec<char>,        // source of truth
    last_key_at: Instant,
    window: Duration,           // = 300ms
}

impl BurstCommitter {
    pub fn push_key(&mut self, k: char) -> Option<String> {
        let now = Instant::now();
        let is_pure_append = !is_tone_modifier(k);

        if is_pure_append && now.duration_since(self.last_key_at) < self.window {
            self.raw_keys.push(k);
            self.last_key_at = now;
            None // defer commit
        } else {
            // Flush: re-parse full buffer (Parse, don't mutate!)
            let result = normalize_smart(&self.raw_keys);
            self.raw_keys.clear();
            self.raw_keys.push(k);
            self.last_key_at = now;
            Some(result)
        }
    }
}
```

### 9.3 GameModeDetector — Ctrl+Shift+G

```rust
/// vi-im/src/game_mode.rs
pub enum GameModeState { Active, Inactive }

pub struct GameModeDetector {
    state: GameModeState,
}

impl GameModeDetector {
    /// Hotkey override: Ctrl+Shift+G
    pub fn toggle_manual(&mut self) {
        self.state = match self.state {
            GameModeState::Active   => GameModeState::Inactive,
            GameModeState::Inactive => GameModeState::Active,
        };
    }

    /// Auto-detect: nếu app không bind text-input-v3 → game
    pub fn detect_from_wl(&mut self, has_text_input: bool) {
        if !has_text_input {
            self.state = GameModeState::Active;
        }
    }

    /// Forward raw events, bypass IME hoàn toàn
    pub fn is_active(&self) -> bool {
        matches!(self.state, GameModeState::Active)
    }
}
```

---

## 10. Architecture Checklist

| Quyết định | Trạng thái |
|------------|-----------|
| Single binary vi-im + tray | ✅ Phase 1 |
| normalize_smart() + Telex priority | ✅ Phase 2 |
| Tray-only config (no QML) | ✅ Phase 3 |
| Burst commit 300ms | ✅ Phase 4 |
| Game Mode Ctrl+Shift+G | ✅ Phase 6 |
| Parse, don't mutate | ✅ Core principle |
| NFD math engine (no lookup table) | ✅ Design decision |
| Virtual Backspace (no preedit) | ✅ Core architecture |
| Separate threads: daemon + IME loop | ✅ Architecture |

---

*Reconstructed from Supermemory chunks by `vi-supermemory recall`.*

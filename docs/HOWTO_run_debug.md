# HOWTO — Chạy & Debug vi-ime (godmod + plugin focus wiring)

> Áp dụng cho đợt thay đổi 2026-07-07: godmod telemetry đã wire (R6), và
> plugin system giờ nhận app_id qua channel focus→IME thread (on_focus_change
> + per-app routing). Doc này hướng dẫn chạy, xác nhận từng phần hoạt động,
> và cách đọc log/telemetry.

---

## 1. Build

```bash
# Debug build (có log, chạy nhanh khi dev)
cargo build -p vi-daemon --features tray

# hoặc release
./deploy/compile.sh --release tray      # → target/release/vi-daemon
```

Binary: `target/debug/vi-daemon` (hoặc `target/release/vi-daemon`).

Sanity trước khi chạy thật:
```bash
cargo test --workspace          # 188 tests, phải xanh hết
cargo build --workspace 2>&1 | grep -c '^warning:'   # phải = 0
```

---

## 2. Chạy daemon (2 chế độ debug)

vi-ime là input-method của Wayland — **phải chạy trong 1 session wlroots**
(niri/Hyprland/Sway) có `zwp_input_method_v2`. Đừng chạy từ TTY thuần.

```bash
# A. Log info + godmod telemetry (ghi jsonl từng phím)
VI_GODMOD=1 RUST_LOG=info ./target/debug/vi-daemon

# B. Full debug (thấy [PLUGIN] on_focus, keymap, grab, two-phase commit…)
VI_GODMOD=1 RUST_LOG=debug ./target/debug/vi-daemon

# C. Chỉ --godmod flag (không cần env)
./target/debug/vi-daemon --godmod
```

godmod bật khi **bất kỳ**: cờ `--godmod`, env `VI_GODMOD=1`, hoặc
`RUST_LOG=debug`. Tắt hoàn toàn (no-op, zero-cost) nếu không có gì → giữ R15.

> **Nếu không có session wlroots rảnh:** chạy nested để test an toàn:
> ```bash
> # cửa sổ niri lồng trong session hiện tại
> niri --session &        # hoặc: sway
> # rồi trong niri lồng đó, mở terminal và chạy vi-daemon như trên
> ```

---

## 3. Xác nhận từng phần hoạt động (grep log)

Chạy daemon ở chế độ B (`RUST_LOG=debug`), rồi **click/focus qua lại giữa 2
app khác nhau** (vd terminal ↔ browser) và **gõ vài từ tiếng Việt**.

### 3.1 godmod bật
```
Godmod telemetry ON → ~/.local/share/vi-ime/godmod/
```

### 3.2 Focus → IME thread (wiring mới)
Mỗi lần đổi app, daemon publish app_id; IME thread nhận ở `maybe_reconfigure`:
```
Focus changed: app_id=foot, category=Terminal, title=...     ← daemon main thread
[RECONFIG] gen=<n> enabled=true method=Telex mode=... output=...   ← IME thread nhận
[PLUGIN] foot on_focus(foot)                                 ← AppPlugin lifecycle CHẠY (debug)
```
`[PLUGIN] … on_focus(…)` là bằng chứng `on_focus_change` đã fire — trước khi
wire dòng này **không bao giờ xuất hiện** (current_app_id luôn None).

### 3.3 recommended_mode = advisory (không override R13)
Khi plugin gợi ý mode khác config đã resolve:
```
[PLUGIN] code: plugin suggests NonPreedit, config resolved Hybrid (config wins, R13)
```
→ đúng thiết kế: **config 4-layer thắng**, plugin chỉ là gợi ý ghi log.

### 3.4 Per-app routing đã đúng
Các plugin gate theo `handles_app` giờ mới chạy đúng app. Ví dụ mở app
Electron thiếu cờ IME:
```
[ElectronFlagAdvisor] discord is Electron/Chromium. Add flags: --ozone-platform=wayland --enable-wayland-ime
```
(trước khi wire, app_id=None → `handles_app` sai → advisor không bắn.)

### 3.5 Two-phase commit (gõ tiếng Việt)
```
[COMMIT] word done: "việt" + replay code=<n>
```

---

## 4. Đọc godmod telemetry

Mỗi session ghi 1 file jsonl + summary:
```bash
ls -t ~/.local/share/vi-ime/godmod/            # file mới nhất: <YYYYMMDD_HHMMSS_ms>.jsonl
```

Mỗi phím = 1 dòng JSON:
```bash
tail -f ~/.local/share/vi-ime/godmod/*.jsonl | jq .
# { "keycode":..., "character":"v", "app_id":"foot", "ime_mode":"NonPreedit",
#   "action":"key", "latency_us":..., "buffer_depth":.., "has_pending":..,
#   "preedit_text":"vi" }
```

Khi daemon dừng (Ctrl-C / SIGTERM / thoát tray), summary in ra log **và** ghi
cuối file:
```
Godmod session: 142 keys, 20 commits (17 VN / 3 EN), max latency 890µs
```
+ file `<session>_apps.json` = thống kê per-app (keystrokes/commits/latency).

> **R11b — bảo mật:** godmod **không** log phím trong field password/PIN
> (`field_sensitivity == Secure`). Muốn kiểm: focus vào ô password, gõ →
> không có dòng nào trong jsonl.

Lọc nhanh:
```bash
# tất cả phím ở 1 app
jq 'select(.app_id=="foot")' ~/.local/share/vi-ime/godmod/*.jsonl
# phím chậm >1ms (nghi engine bug)
jq 'select(.latency_us > 1000)' ~/.local/share/vi-ime/godmod/*.jsonl
```

---

## 5. Kiến trúc wiring (để debug sâu)

```
[daemon main thread]                         [IME thread (wayland)]
 FocusEvent (niri/wlr)                         event_queue.blocking_dispatch
   │ app_changed                                  │ mỗi key / Activate
   ├─ runtime.store_app_id(app_id) ──┐            ├─ maybe_reconfigure():
   │  (+bump generation, Release)     │  Arc<RuntimeConfig>  │   snap = rt.snapshot()
   ├─ godmod::set_app(app_id)         │  app_id: Mutex<..>   │   if gen đổi:
   └─ apply_new_focus_config          │  generation: Atomic  │     new_app_id = rt.app_id()
      → runtime.store(snapshot)  ─────┘  (generation-gated)  │     plugin_manager.on_focus_change()
                                                             │     self.current_app_id = new_app_id
                                                             │     (→ pre/post_process_key route đúng)
```
- Không thêm channel/timer mới → **giữ R15** (IME loop vẫn chỉ blocking_dispatch).
- `app_id` qua Mutex (String không atomic được), nhưng **chỉ đọc khi generation
  đổi** (đổi app) nên hot-path từng phím vẫn lock-free.
- **R13 vẫn là single source of `ime_mode`** — plugin `recommended_mode` chỉ log.

File liên quan:
| File | Vai trò |
|------|---------|
| `crates/vi-daemon/src/wayland/runtime.rs` | `store_app_id`/`app_id` + generation |
| `crates/vi-daemon/src/main.rs` | focus handler publish app_id + godmod init/set_app/finish |
| `crates/vi-daemon/src/wayland/state.rs` | `maybe_reconfigure` gọi on_focus_change |
| `crates/vi-daemon/src/plugin/mod.rs` | AppPlugin trait + PluginManager + 5 builtin plugins |
| `crates/vi-daemon/src/godmod/` | recorder (session.rs) + models |

---

## 6. Troubleshooting

| Triệu chứng | Nguyên nhân / cách xử |
|-------------|------------------------|
| Không có `[PLUGIN] … on_focus` | Compositor không gửi focus event → check `Focus changed:` có xuất hiện không. Nếu không: focus tracking fail (niri IPC / wlr-toplevel). |
| `zwp_input_method_manager_v2 not available` | Compositor không phải wlroots hoặc thiếu protocol → dùng niri/Hyprland/Sway. |
| godmod jsonl rỗng | Chưa bật (thiếu `--godmod`/`VI_GODMOD`/`RUST_LOG=debug`), hoặc chỉ gõ trong field Secure (R11b), hoặc chưa gõ phím nào. |
| Gõ không ra tiếng Việt | Check `[RECONFIG] … method=` đúng Telex/VNI/Smart; check `enabled=true`; nếu game_mode=true → passthrough. |
| Muốn tắt godmod hẳn | Chạy không cờ, `RUST_LOG=info` (mặc định) → godmod = no-op. |

Dừng sạch để lấy summary:
```bash
pkill -TERM vi-daemon        # SIGTERM → finalize + in "Godmod session: …"
```

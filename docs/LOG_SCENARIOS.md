# 🎬 Các Scenario & Cách đọc Log

> Chạy: `RUST_LOG=info cargo run` để xem log.
> Chạy: `RUST_LOG=debug cargo run` để xem log chi tiết hơn.

---

## Bảng tóm tắt các tag log

| Tag | Ý nghĩa |
|-----|---------|
| `[KEY-IN]` | Nhận 1 phím từ bàn phím |
| `[KEY-BUF]` | Phím được đưa vào hàng đợi (obsolete) |
| `[ROLLOVER]` | Phát hiện key-repeat hoặc gõ quá nhanh |
| `[SCENARIO]` | Sự kiện quan trọng (activate/deactivate/commit) |
| `[LATENCY]` | Đo độ trễ (sẽ thêm sau) |

---

## Scenario 1: Gõ tiếng Việt bình thường (Telex, Preedit mode)

**Hành động:** Gõ `v`, `i`, `e`, `e`, `t`, `j`, `SPACE`

**Log mong đợi:**
```
[SCENARIO] ✅ ACTIVATE — IME=Preedit composer attached, grabbing keyboard...
[SCENARIO] ⌨️  Keyboard GRABBED (all keys go to IME now)
[KEY-IN] code=55 char='v' mode=Preedit queue=1/16
  → engine: UpdatePreedit("v") → set_preedit_string("v", 0, 1)
[KEY-IN] code=31 char='i' mode=Preedit buf="v" queue=1/16
  → engine: UpdatePreedit("vi") → set_preedit_string("vi", 0, 2)
[KEY-IN] code=26 char='e' mode=Preedit buf="vi" queue=1/16
  → engine: UpdatePreedit("vie")
[KEY-IN] code=26 char='e' mode=Preedit buf="vie" queue=1/16
  → engine: ee→ê, UpdatePreedit("viê")
[KEY-IN] code=28 char='t' mode=Preedit buf="viê" queue=1/16
  → engine: UpdatePreedit("viêt")
[KEY-IN] code=44 char='j' mode=Preedit buf="viêt" queue=1/16
  → engine: j→nặng, UpdatePreedit("việt")
[KEY-IN] code=65 char=' ' mode=Preedit buf="việt" queue=1/16
  → engine: Commit("việt")
  → Wayland: commit_string("việt") → chữ hiện ra màn hình!
```

**Đánh giá:**
- Mỗi phím → 1 lần `set_preedit_string` (có độ trễ vì phải roundtrip Wayland)
- Visual feedback tốt: thấy chữ gạch chân khi gõ dở
- Phù hợp: Firefox, Chrome, GTK apps

---

## Scenario 2: Gõ tiếng Việt trong terminal (NonPreedit mode)

**Hành động:** Gõ `v`, `i`, `e`, `e`, `t`, `j`, `SPACE`

**Log mong đợi:**
```
[SCENARIO] ✅ ACTIVATE — IME=NonPreedit composer attached, grabbing keyboard...
[SCENARIO] ⌨️  Keyboard GRABBED
[KEY-IN] code=55 char='v' mode=NonPreedit queue=1/16
  → engine: Buffer (không gửi gì lên Wayland)
[KEY-IN] code=31 char='i' mode=NonPreedit buf="v" queue=1/16
  → engine: Buffer
[KEY-IN] code=26 char='e' mode=NonPreedit buf="vi" queue=1/16
  → engine: Buffer
[KEY-IN] code=26 char='e' mode=NonPreedit buf="vie" queue=1/16
  → engine: ee→ê, Buffer
[KEY-IN] code=28 char='t' mode=NonPreedit buf="viê" queue=1/16
  → engine: Buffer
[KEY-IN] code=44 char='j' mode=NonPreedit buf="viêt" queue=1/16
  → engine: j→nặng, Buffer
[KEY-IN] code=65 char=' ' mode=NonPreedit buf="việt" queue=1/16
  → engine: CommitWithBackspace { backspace_count: 6, text: "việt" }
  → [SCENARIO] 📝 COMMIT phase-1: delete_surrounding_text(6, 0)
  → [SCENARIO] 🔄 DONE received
  → [SCENARIO] 📝 COMMIT phase-2: "việt" (4 chars)
```

**Đánh giá:**
- Các phím bị nuốt (Buffer) → không có roundtrip Wayland mỗi phím
- Chỉ 1 lần commit khi gõ space → nhanh!
- Phù hợp: terminal (foot, kitty, alacritty), VS Code

---

## Scenario 3: Gõ tiếng Anh xen kẽ (auto-detect)

**Hành động:** Gõ `h`, `e`, `l`, `l`, `o` (hello)

**Log mong đợi:**
```
[KEY-IN] code=43 char='h' mode=Hybrid queue=1/16
  → should_pass_through: 'h' không phải phím VN → english_count=1
  → engine: UpdatePreedit("h")
[KEY-IN] code=26 char='e' mode=Hybrid buf="h" queue=1/16
  → should_pass_through: 'e' LÀ nguyên âm VN! → english_count=0 (reset!)
  → engine: UpdatePreedit("he")
[KEY-IN] code=46 char='l' mode=Hybrid buf="he" queue=1/16
  → should_pass_through: 'l' không phải phím VN → english_count=1
  → engine: UpdatePreedit("hel")
[KEY-IN] code=46 char='l' mode=Hybrid buf="hel" queue=1/16
  → should_pass_through: english_count=2 < 4 → chưa kích hoạt
  → engine: UpdatePreedit("hell")
[KEY-IN] code=32 char='o' mode=Hybrid buf="hell" queue=1/16
  → should_pass_through: 'o' LÀ nguyên âm VN! → english_count=0 (reset!) ✅
  → engine: UpdatePreedit("hello")
```

**Đánh giá:**
- 'e' và 'o' reset bộ đếm tiếng Anh → không bị false positive
- Nếu gõ "bgklm" (toàn phụ âm): sau 4 phím → pass-through (đúng là tiếng Anh)

---

## Scenario 4: Gõ nhanh — Rollover detection

**Hành động:** Gõ `a`, `a` trong vòng <5ms (ultra-fast)

**Log mong đợi:**
```
[KEY-IN] code=38 char='a' mode=NonPreedit queue=1/16
[ROLLOVER] ⚡ULTRA-FAST code=38 char='a' gap=3200µs   ← CẢNH BÁO!
[KEY-IN] code=38 char='a' mode=NonPreedit buf="a" queue=2/16
```

**Hành động:** Giữ phím `a` (key repeat)

**Log mong đợi:**
```
[KEY-IN] code=38 char='a' mode=NonPreedit queue=1/16
[ROLLOVER] SKIP key-repeat code=38 char='a' gap=18000µs (coalesced)  ← BỎ QUA
[ROLLOVER] SKIP key-repeat code=38 char='a' gap=17500µs (coalesced)
[ROLLOVER] SKIP key-repeat code=38 char='a' gap=17200µs (coalesced)
```

---

## Scenario 5: Chuyển cửa sổ khi đang gõ dở (Deactivate Auto-Commit)

**Hành động:** Gõ `v`, `i`, `e`, `e` (đang gõ "viê") → Niri scroll sang cửa sổ khác

**Log mong đợi:**
```
[KEY-IN] code=38 char='v' mode=NonPreedit queue=1/16
[KEY-IN] code=31 char='i' mode=NonPreedit buf="v" queue=1/16
[KEY-IN] code=26 char='e' mode=NonPreedit buf="vi" queue=1/16
[KEY-IN] code=26 char='e' mode=NonPreedit buf="vie" queue=1/16
  → ee→ê, Buffer("viê")
  
[Niri scroll → focus changes]

[SCENARIO] ❌ DEACTIVATE — focus lost, had_pending=true, text="viê"
[SCENARIO] 🔒 AUTO-COMMIT on deactivate: "viê" → saved!      ← CỨU CHỮ!
[SCENARIO] 🔓 Keyboard RELEASED

[Niri focus on new window]

[SCENARIO] ✅ ACTIVATE — IME=NonPreedit composer attached...
[SCENARIO] ⌨️  Keyboard GRABBED
```

**Đánh giá:**
- Chữ "viê" không bị mất! Được auto-commit khi mất focus
- Đây là cơ chế quan trọng chống trượt chữ trên Niri

---

## Scenario 6: Backspace trong lúc gõ dở

**Hành động:** Gõ `v`, `i`, BACKSPACE, `e`, `e`, `t`, `j`, SPACE

**Log mong đợi:**
```
[KEY-IN] code=55 char='v' mode=Telex queue=1/16
  → UpdatePreedit("v")
[KEY-IN] code=31 char='i' mode=Telex buf="v" queue=1/16
  → UpdatePreedit("vi")
[KEY-IN] code=22 char='␈' mode=Telex buf="vi" queue=1/16
  → handle_backspace: engine.backspace()
  → buffer còn "v"
  → UpdatePreedit("v")
[KEY-IN] code=26 char='e' mode=Telex buf="v" queue=1/16
  → UpdatePreedit("ve")
  ...
```

---

## Scenario 7: Latency Benchmark (đo độ trễ)

**Cách đo:**
```bash
# Mở 2 terminal:
# Terminal 1: Chạy IME với log
RUST_LOG=info ./target/release/vi-daemon 2>&1 | ts '[%H:%M:%.S]'

# Terminal 2: Gõ và quan sát timestamp
```

**Metric cần quan sát:**
1. **Key-in → Action**: thời gian giữa `[KEY-IN]` và action log (nên <1ms)
2. **Commit phase-1 → Done**: thời gian compositor xử lý delete_surrounding_text (nên <5ms)
3. **Done → Commit phase-2**: thời gian commit text (nên <1ms)
4. **Total roundtrip**: từ key-in đến khi chữ hiện ra (mục tiêu <10ms)

---

## Bảng tổng kết performance

| Mode | Mỗi phím (latency) | Commit (latency) | Visual feedback | App compat |
|------|-------------------|------------------|-----------------|------------|
| **Preedit** | ~2-5ms (roundtrip) | ~1ms | ✅ Có | ~70% |
| **NonPreedit** | ~0ms (nuốt phím) | ~5-10ms (2 roundtrips) | ❌ Không | ~95% |
| **Hybrid** | ~0ms (cho đến khi detect VN) | ~5-10ms | ⚠️ Khi cần | ~85% |

---

> **Tip:** Khi debug, thêm `| ts` vào cuối command để có timestamp cho mỗi dòng log.
> Cài `ts`: `sudo apt install moreutils` (Ubuntu) hoặc `sudo pacman -S moreutils` (Arch)

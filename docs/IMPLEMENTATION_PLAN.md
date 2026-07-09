# 🛠️ vi-im Implementation Plan — NFD Engine + qu/gi Fixes + Roadmap

> **Mục tiêu**: Củng cố engine, fix edge cases, tăng test coverage.
> **Bỏ qua**: Game Mode (sẽ làm riêng phase sau).
> **Status**: 🔴 DRAFT — chờ team review trước khi implement.

---

## 📋 Mục lục

1. [Recall: NFD Engine + 8 Hằng số Unicode](#1-recall-nfd-engine--8-hằng-số-unicode)
2. [Fix qu/gi Edge Cases](#2-fix-qugi-edge-cases)
3. [State Machine + Latency Table](#3-state-machine--latency-table)
4. [4-Phase Roadmap](#4-4-phase-roadmap)
5. [Full Test Cases + File Structure](#5-full-test-cases--file-structure)
6. [Implementation Checklist](#6-implementation-checklist)

---

## 1. Recall: NFD Engine + 8 Hằng Số Unicode

### 1.1 Kiến trúc hiện tại

```
┌──────────────────────────────────────────────────────────┐
│                      GLYPH LAYER                         │
│  glyph.rs — Unicode Algebra (quality + tone marks)       │
│                                                          │
│  3 QUALITY MARKS (pub const):                            │
│    CIRCUMFLEX = U+0302  →  â, ê, ô                      │
│    BREVE      = U+0306  →  ă                             │
│    HORN       = U+031B  →  ơ, ư                          │
│                                                          │
│  5 TONE MARKS (inside tone_mark()):                      │
│    U+0301 = Acute  (sắc)    U+0300 = Grave  (huyền)     │
│    U+0309 = Hook   (hỏi)    U+0303 = Tilde  (ngã)       │
│    U+0323 = Dot    (nặng)                                │
│                                                          │
│  1 PSEUDO-MARK:                                          │
│    STROKE = U+0335 → sentinel for đ/Đ special case       │
│                                                          │
│  compose(base, mark) → NFC → precomposed char            │
│  base_of(ch)         → NFD → strips all marks to ASCII   │
└──────────────────────────────────────────────────────────┘
                            │
                            ▼
┌──────────────────────────────────────────────────────────┐
│                     ENGINE LAYER                         │
│  engine.rs:69-75 — preedit_output()                      │
│                                                          │
│  display (NFC) ──┬── UnicodeDungSan → clone()            │
│                  └── UnicodeToHop   → .nfd().collect()   │
│                                                          │
│  KEY: display luôn là NFC. NFD chỉ áp tại commit boundary│
└──────────────────────────────────────────────────────────┘
```

### 1.2 Output Mode Decision Tree

```
              ┌─────────────────┐
              │  OutputMode?    │
              └────────┬────────┘
                       │
         ┌─────────────┴─────────────┐
         │                           │
   UnicodeDungSan               UnicodeToHop
   (NFC - dựng sẵn)             (NFD - tổ hợp)
         │                           │
  "tiếng" → "tiếng"          "tiếng" → "t i ê ́ . n g"
                                        (base chars +
                                         combining marks)
```

### 1.3 Test hiện có cho NFD

| Test | File | Status |
|------|------|--------|
| `test_output_mode_nfd` | `engine_tests.rs:249` | ✅ Passing |
| `test_nfd_output_on_word_boundary_commit` | `fast_engine_tests.rs:207` | ✅ Passing |
| `test_nfc_output_stays_precomposed` | `fast_engine_tests.rs:228` | ✅ Passing |
| `test_preedit_output_formats_buffer` | `fast_engine_tests.rs:243` | ✅ Passing |

### 1.4 Không cần thay đổi gì cho NFD Engine

NFD engine đã hoàn chỉnh. **KHÔNG implement thêm**.

---

## 2. Fix qu/gi Edge Cases

### 2.1 Phân tích hiện trạng

Backtracking trong `analyze.rs:28-50` hoạt động đúng cho hầu hết trường hợp, nhưng có 4 gaps:

| # | Edge Case | Input | Hiện tại | Should Be | Severity |
|---|-----------|-------|----------|-----------|----------|
| 1 | "gii" false positive | `gi` + `i` | qu+oa → "quoa" ✓ | "gii" (invalid) | Low |
| 2 | "qu" standalone | `q`+`u` | "qu" accepted | REJECT (need vowel) | Low |
| 3 | "quoa" false positive | `qu`+`oa` | "quoa" accepted | REJECT (phontactic) | Low |
| 4 | English tone residue | `expr`, `express` | ẻp, ẻpress | expr, express (raw) | **Medium** |

### 2.2 Giải pháp đề xuất

#### Fix #1 & #2 & #3: Phonotactic Validation Layer

Thêm một module mới `validator.rs` kiểm tra quy tắc âm vị học sau khi `analyze()` thành công:

```rust
// File mới: crates/vi-engine/src/parser/validator.rs

/// Kiểm tra phonotactic rules sau khi analyze thành công.
/// Returns true nếu syllable hợp lệ.
pub fn is_valid_syllable(decoded: &Decomposed) -> bool {
    // Rule 1: initial "qu" PHẢI có vowel sau nó (không phải chỉ u)
    if decoded.initial == "qu" {
        let cluster = decoded.cluster();
        // qu + u = không hợp lệ (u đã nằm trong initial qu)
        if cluster == "u" { return false; }
        // qu + oa = không hợp lệ (glide o đã nằm trong qu)
        if cluster.starts_with('o') { return false; }
    }

    // Rule 2: initial "gi" + "i" = false positive
    // gi đã chứa i, thêm i nữa → gii không phải tiếng Việt
    if decoded.initial == "gi" && decoded.cluster() == "i" {
        return false;
    }

    true
}
```

**Vị trí gọi**: trong `parser::parse()` (`mod.rs:98-101`), sau khi `analyze()` trả về Some:

```rust
// mod.rs:98 — thêm dòng này
match analyze::analyze(&norm.chars) {
    Some(d) if validator::is_valid_syllable(&d) => ParseOutcome::Valid(Parsed { ... }),
    Some(_) => ParseOutcome::Invalid,  // ← mới: phonotactic invalid
    None => ParseOutcome::Invalid,
}
```

#### Fix #4: English Tone Residue — Pre-normalize Guard

Vấn đề: "expr" → normalize đã biến 'e'+'x'+'p'+'r' thành ['ẻ','p'] trước khi analyzer chạy.

Giải pháp: Thêm check trong normalize để phát hiện tone key trên từ không có vowel hợp lệ:

```rust
// Trong normalize.rs, sau khi xử lý tone key:
// Nếu chữ cái trước tone key không nằm trong tập vowel cơ bản
// → revert tone key, đánh dấu literal mode
```

**Cụ thể**: Trong `normalize()`, sau khi `tone_for_key()` trả về Some, kiểm tra:
- Nếu `has_vowel == false` → không áp dụng tone
- Nếu ký tự LIỀN TRƯỚC tone key là consonant → revert

```rust
// normalize.rs:48 — sửa logic tone key
if let Some(t) = tone_for_key(ch, method, has_vowel) {
    // NEW: check if the char immediately before tone key is a vowel
    let prev_is_vowel = out.last().map_or(false, |&c| tables::is_vowel_char(c));
    if !prev_is_vowel && tone != Tone::Level {
        // Tone key on consonant → don't apply, emit literally
        out.push(ch);
        continue;
    }
    // ... existing logic
}
```

### 2.3 Files cần sửa

| File | Thay đổi |
|------|----------|
| `crates/vi-engine/src/parser/validator.rs` | **NEW** — phonotactic rules |
| `crates/vi-engine/src/parser/mod.rs` | Thêm `pub mod validator;` + gọi `is_valid_syllable()` |
| `crates/vi-engine/src/parser/normalize.rs` | Pre-normalize guard cho tone residue |

### 2.4 Test cases mới

```rust
// tests/vi-engine/validator_tests.rs (NEW)

#[test]
fn test_reject_qu_standalone() {
    // "qu" alone → phonotactically invalid
    assert!(parse_telex("qu").is_err());
}

#[test]
fn test_reject_quoa() {
    // "quoa" → glide conflict (o in qu + oa)
    assert!(parse_telex("quoa").is_err());
}

#[test]
fn test_reject_gii() {
    // "gii" → gi + i = false positive
    assert!(parse_telex("gii").is_err());
}

#[test]
fn test_accept_qua() {
    // "quá" → OK: qu + a
    assert!(parse_telex("quas").is_ok());
}

#[test]
fn test_accept_gia() {
    // "già" → OK: gi + a
    assert!(parse_telex("giaf").is_ok());
}

#[test]
fn test_english_tone_residue_expr() {
    // UNIGNORE: sửa `#[ignore]` test hiện tại
    assert_eq!(commit_telex("expr"), "expr");
}

#[test]
fn test_english_tone_residue_express() {
    assert_eq!(commit_telex("express"), "express");
}
```

---

## 3. State Machine + Latency Table

### 3.1 Current State Machine

```
                    ┌──────────┐
          ┌────────▶│   IDLE   │◀────────┐
          │         └────┬─────┘         │
          │              │ key press      │
          │              ▼                │
          │         ┌──────────┐         │
          │         │COMPOSING │         │
          │         └────┬─────┘         │
          │              │                │
          │    ┌─────────┼─────────┐     │
          │    │         │         │     │
          │    ▼         ▼         ▼     │
          │  word    backspace   escape  │
          │  boundary           /delete  │
          │    │         │         │     │
          │    ▼         ▼         └─────┘
          │ ┌──────┐ ┌──────┐
          │ │COMMIT│ │ POP  │
          │ └──┬───┘ └──┬───┘
          │    │         │
          └────┘         │
                          │
         ┌────────────────┘
         │
    ┌────▼─────┐     ┌──────────┐     ┌─────────┐
    │ raw_keys │────▶│ normalize│────▶│ analyze │
    │  (truth) │     │ (2-pass) │     │(phonot.)│
    └──────────┘     └──────────┘     └────┬────┘
                                           │
                                    ┌──────▼──────┐
                                    │   render    │
                                    │ (NFC output)│
                                    └─────────────┘
```

### 3.2 Action Flow (per IME mode)

| Step | NonPreedit | Hybrid | Preedit |
|------|-----------|--------|---------|
| Key press | `Buffer` (silent) | `UpdatePreedit` if ambiguous, else `Buffer` | `UpdatePreedit` |
| Backspace | `Buffer` / `ClearPreedit` | `UpdatePreedit` | `UpdatePreedit` |
| Word boundary | `CommitWithBackspace` | `CommitWithBackspace` | `Commit` |
| Enter | `CommitWithBackspace` | `CommitWithBackspace` | `Commit` |
| Escape | `ClearPreedit` | `ClearPreedit` | `ClearPreedit` |

### 3.3 Latency Table (Compositor-specific)

| Compositor | Base Delay (µs) | Per-char EMA | App Compat |
|------------|-----------------|--------------|------------|
| **Niri** | 1500 (1.5ms) | 0.3 α factor | 95%+ |
| **Hyprland** | 2500 (2.5ms) | 0.3 α factor | 90%+ |
| **GNOME Mutter** | 3000 (3.0ms) | 0.3 α factor | 85%+ |
| **KDE KWin** | 3500 (3.5ms) | 0.3 α factor | 85%+ |
| **COSMIC** | 2000 (2.0ms) | 0.3 α factor | 90%+ |
| **Unknown** | 4000 (4.0ms) | 0.3 α factor | 80%+ |

**Công thức**: `delay = ema_roundtrip_us × raw_len + 5000µs` (5ms safety buffer)

**Ghi chú**: AdaptiveDelay đã được implement trong `fast_engine.rs:220-294`. Không cần thay đổi.

---

## 4. 4-Phase Roadmap

```
Phase 1: Edge Case Fixes          Phase 2: Validation Layer
┌─────────────────────────┐       ┌─────────────────────────┐
│ ✅ Fix qu standalone     │       │ ✅ validator.rs module   │
│ ✅ Fix quoa glide        │  ───▶ │ ✅ Phonotactic rules     │
│ ✅ Fix gii false positive│       │ ✅ English tone guard    │
│ ✅ Fix expr/express      │       │ ✅ Unignore tests        │
│ ✅ 10+ new tests         │       │                         │
└─────────────────────────┘       └─────────────────────────┘
         │                                   │
         └───────────────┬───────────────────┘
                         ▼
Phase 3: Test Coverage             Phase 4: Documentation
┌─────────────────────────┐       ┌─────────────────────────┐
│ ✅ Golden corpus: 200+  │       │ ✅ IMPLEMENTATION_PLAN   │
│ ✅ Modern tone golden   │  ───▶ │ ✅ CODE_GUIDE updates    │
│ ✅ Fuzz testing setup   │       │ ✅ Function call flow    │
│ ✅ Edge case matrix     │       │ ✅ Perf benchmarks       │
│ ✅ NFD round-trip tests │       │                         │
└─────────────────────────┘       └─────────────────────────┘
```

### Phase 1: Edge Case Fixes (2-3 ngày)

- [ ] Implement `validator.rs` với 3 phonotactic rules
- [ ] Implement pre-normalize guard trong `normalize.rs`
- [ ] Unignore `test_english_tone_residue_restores_raw`
- [ ] 10+ test cases cho edge cases

### Phase 2: Validation Layer (1-2 ngày)

- [ ] Tách validator thành module riêng
- [ ] Thêm rule: coda-only check (chỉ m/n/ng/nh/p/t/ch/c được làm coda)
- [ ] Thêm rule: vowel harmony check (ơ+ê, u+ô, ...)
- [ ] Document tất cả phonotactic rules

### Phase 3: Test Coverage (2-3 ngày)

- [ ] Mở rộng golden corpus: 87+16 → 200+ cases
- [ ] Thêm Modern tone style golden tests
- [ ] Property-based/fuzz testing với `proptest`
- [ ] NFD round-trip tests (NFC→NFD→NFC)
- [ ] Latency benchmark tests

### Phase 4: Documentation (1 ngày)

- [ ] Cập nhật CODE_GUIDE.md
- [ ] Cập nhật AGENTS.md
- [ ] Tạo edge case matrix doc
- [ ] Benchmark report

---

## 5. Full Test Cases + File Structure

### 5.1 File Structure Mới

```
vi-im/
├── crates/
│   └── vi-engine/
│       └── src/
│           └── parser/
│               ├── mod.rs          ← [EDIT] Thêm validator call
│               ├── analyze.rs       (no change)
│               ├── normalize.rs    ← [EDIT] Pre-normalize guard
│               ├── glyph.rs         (no change)
│               ├── render.rs        (no change)
│               ├── tables.rs        (no change)
│               └── validator.rs    ← [NEW] Phonotactic rules
│
├── tests/
│   └── vi-engine/
│       ├── engine_tests.rs         ← [EDIT] Unignore test
│       ├── fast_engine_tests.rs     (no change)
│       ├── golden_tests.rs         ← [EDIT] +Modern tone +edge cases
│       ├── hybrid_tests.rs          (no change)
│       ├── rollover_tests.rs        (no change)
│       ├── parser_tests.rs         ← [EDIT] +qu/gi edge cases
│       └── validator_tests.rs      ← [NEW] Phonotactic tests
│
└── docs/
    ├── function-call-flow.md       (đã tạo)
    ├── analysis-for-roadmap.md     (đã tạo)
    └── IMPLEMENTATION_PLAN.md      ← [THIS FILE]
```

### 5.2 Test Case Matrix (đầy đủ)

#### A. validator_tests.rs (NEW — 12 tests)

| # | Test Name | Input | Expected |
|---|-----------|-------|----------|
| 1 | `test_reject_qu_standalone` | `parse("qu")` | Invalid |
| 2 | `test_reject_quoa` | `parse("quoa")` | Invalid |
| 3 | `test_reject_quoe` | `parse("quoe")` | Invalid |
| 4 | `test_reject_quoi` | `parse("quoi")` | Invalid |
| 5 | `test_reject_gii` | `parse("gii")` | Invalid |
| 6 | `test_accept_qua` | `parse("qua")` | Valid (qu+a) |
| 7 | `test_accept_que` | `parse("que")` | Valid (qu+e) |
| 8 | `test_accept_qui` | `parse("qui")` | Valid (qu+i) |
| 9 | `test_accept_quy` | `parse("quy")` | Valid (qu+y) |
| 10 | `test_accept_quoc` | `parse("quôc")` | Valid (qu+ô+c) |
| 11 | `test_accept_gia` | `parse("gia")` | Valid (gi+a) |
| 12 | `test_accept_giuong` | `parse("giương")` | Valid (gi+ươ+ng) |

#### B. normalize tests (EDIT — 5 tests)

| # | Test Name | Input | Expected |
|---|-----------|-------|----------|
| 1 | `test_expr_not_tone_residue` | `normalize("expr", Telex)` | chars=[e,x,p,r], tone=Level |
| 2 | `test_express_not_tone_residue` | `normalize("express", Telex)` | chars=[e,x,p,r,e,s,s], tone=Level |
| 3 | `test_tone_on_vowel_still_works` | `normalize("as", Telex)` | chars=[a], tone=Acute |
| 4 | `test_tone_after_consonant_kept` | `normalize("ems", Telex)` | chars=[e,m], tone=Acute |
| 5 | `test_double_tone_on_consonant` | `normalize("prr", Telex)` | chars=[p,r,r], undo=true |

#### C. parser_tests.rs (EDIT — 4 tests mới)

| # | Test Name | Input | Expected |
|---|-----------|-------|----------|
| 1 | `test_parse_qu_standalone_rejected` | `parse([q,u], Telex)` | Invalid |
| 2 | `test_parse_quoa_rejected` | `parse([q,u,o,a], Telex)` | Invalid |
| 3 | `test_parse_gii_rejected` | `parse([g,i,i], Telex)` | Invalid |
| 4 | `test_parse_quoc_valid` | `parse([q,u,ô,c], Telex)` | Valid (qu+ô+c) |

#### D. engine_tests.rs (EDIT — 2 tests unignore)

| # | Test Name | Input | Expected |
|---|-----------|-------|----------|
| 1 | `test_english_tone_residue_restores_raw` | `commit_telex("expr")` | "expr" |
| 2 | `test_english_tone_residue_restores_raw` | `commit_telex("express")` | "express" |

#### E. golden_tests.rs (EDIT — thêm 30+ cases)

| # | Category | Count |
|---|----------|-------|
| 1 | Modern tone style (hoà, thuý, ...) | 15 cases |
| 2 | qu- words edge cases | 8 cases |
| 3 | gi- words edge cases | 5 cases |
| 4 | NFD output golden | 5 cases |

### 5.3 Tổng số test cases sau implement

| File | Hiện tại | Mới | Tổng |
|------|---------|-----|------|
| `validator_tests.rs` | 0 | 12 | **12** |
| `engine_tests.rs` | ~20 | +2 (unignore) | **22** |
| `fast_engine_tests.rs` | ~15 | 0 | **15** |
| `golden_tests.rs` | 103 | +30 | **133** |
| `hybrid_tests.rs` | ~8 | 0 | **8** |
| `rollover_tests.rs` | ~6 | 0 | **6** |
| `parser_tests.rs` | ~30 | +4 | **34** |
| **TOTAL** | **~182** | **+48** | **~230** |

---

## 6. Implementation Checklist

### 🔴 Phase 1: Edge Case Fixes

- [ ] **1.1** Tạo `crates/vi-engine/src/parser/validator.rs`
  ```rust
  pub fn is_valid_syllable(decoded: &Decomposed) -> bool
  // Rule: qu + u | qu + o* | gi + i → Invalid
  ```
- [ ] **1.2** Sửa `crates/vi-engine/src/parser/mod.rs`:
  - Thêm `pub mod validator;` (sau dòng `pub mod tables;`)
  - Import `validator::is_valid_syllable` trong `parse()`
  - Gọi `is_valid_syllable()` trong match `analyze()`
- [ ] **1.3** Sửa `crates/vi-engine/src/parser/normalize.rs`:
  - Trong `tone_for_key()` block (dòng 48): thêm check `prev_is_vowel`
  - Nếu tone key đứng sau consonant → emit literal, không áp tone
- [ ] **1.4** Tạo `tests/vi-engine/validator_tests.rs` (12 tests)
- [ ] **1.5** Sửa `tests/vi-engine/engine_tests.rs`: bỏ `#[ignore]` dòng 222
- [ ] **1.6** Sửa `tests/vi-engine/parser_tests.rs`: thêm 4 edge case tests
- [ ] **1.7** Chạy `cargo test --workspace` → tất cả pass

### 🟡 Phase 2: Validation Layer

- [ ] **2.1** Mở rộng `validator.rs`:
  - Coda validation (chỉ 8 coda hợp lệ)
  - Vowel harmony basic check
- [ ] **2.2** Tài liệu hóa tất cả phonotactic rules trong validator.rs
- [ ] **2.3** Chạy full test suite

### 🟢 Phase 3: Test Coverage

- [ ] **3.1** Mở rộng `golden_tests.rs` → 200+ cases
  - 15 Modern tone style cases
  - 8 qu- edge cases
  - 5 gi- edge cases
  - 5 NFD output cases
- [ ] **3.2** Setup property-based testing (optional)
- [ ] **3.3** NFD round-trip tests
- [ ] **3.4** Chạy `cargo test --workspace` → tất cả pass

### 🔵 Phase 4: Documentation

- [ ] **4.1** Cập nhật `CODE_GUIDE.md` — thêm validator section
- [ ] **4.2** Cập nhật `AGENTS.md` — thêm phonotactic rules
- [ ] **4.3** Tạo benchmark report
- [ ] **4.4** Final review

---

## ✅ Review Checklist (trước khi implement)

- [ ] Xác nhận NFD engine không cần thay đổi
- [ ] Xác nhận validator.rs approach là đúng (không dictionary, chỉ phonotactic)
- [ ] Xác nhận pre-normalize guard không break Telex/VNI hợp lệ
- [ ] Xác nhận test case matrix đầy đủ
- [ ] Xác nhận 4-phase timeline hợp lý
- [ ] Xác nhận Game Mode bỏ qua (sẽ implement riêng)

---

> **Next step**: Team review document này → approve → bắt đầu implement Phase 1.

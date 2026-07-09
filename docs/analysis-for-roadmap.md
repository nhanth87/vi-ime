# vi-im Codebase Analysis for Roadmap Planning

> Generated: 2025-07-15
> Scope: `crates/vi-engine/` + `tests/vi-engine/`

---

## 1. NFD Engine — Unicode Combining Constants

### 1.1 Architecture

The NFD output path is a two-tier design:

| Tier | Location | Purpose |
|------|----------|---------|
| **Render** | `glyph.rs` | Unicode algebra: quality marks + tone marks composed via NFC |
| **Output** | `engine.rs:69–75` | `preedit_output()` applies NFD at commit boundary |

The key insight (documented in `glyph.rs:1–13`) is:

> A Vietnamese letter is `base × quality × tone`. Internally we only ever combine a base/quality-marked char with ONE combining codepoint and let the Unicode canonical-composition algorithm (NFC) produce the precomposed character.

### 1.2 The 8 Combining Constants

There are **8 real combining codepoints** used in the pipeline (plus one pseudo-mark for đ):

#### Quality Marks (3 constants — `pub const`)

| # | Constant | Codepoint | Unicode Name | Vietnamese Name | Applies To |
|---|----------|-----------|--------------|-----------------|------------|
| 1 | `CIRCUMFLEX` | `U+0302` | COMBINING CIRCUMFLEX ACCENT | mũ | a→â, e→ê, o→ô |
| 2 | `BREVE` | `U+0306` | COMBINING BREVE | trăng | a→ă |
| 3 | `HORN` | `U+031B` | COMBINING HORN | móc | o→ơ, u→ư |

**Line references** — `glyph.rs`:
- `CIRCUMFLEX`: line 22 (`pub const CIRCUMFLEX: char = '\u{0302}';`)
- `BREVE`: line 24 (`pub const BREVE: char = '\u{0306}';`)
- `HORN`: line 26 (`pub const HORN: char = '\u{031B}';`)

#### Tone Marks (5 constants — inside `tone_mark()`)

| # | Codepoint | Unicode Name | Vietnamese Name | Tone Variant |
|---|-----------|--------------|-----------------|--------------|
| 4 | `U+0301` | COMBINING ACUTE ACCENT | sắc | `Tone::Acute` |
| 5 | `U+0300` | COMBINING GRAVE ACCENT | huyền | `Tone::Grave` |
| 6 | `U+0309` | COMBINING HOOK ABOVE | hỏi | `Tone::Hook` |
| 7 | `U+0303` | COMBINING TILDE | ngã | `Tone::Tilde` |
| 8 | `U+0323` | COMBINING DOT BELOW | nặng | `Tone::Dot` |

**Line references** — `glyph.rs:29–38` (the `tone_mark()` function).

#### Pseudo-mark (1 additional constant — special case)

| Constant | Codepoint | Unicode Name | Purpose |
|----------|-----------|--------------|---------|
| `STROKE` | `U+0335` | COMBINING SHORT STROKE OVERLAY | đ/Đ — not a real combining char in pipeline |

**Line reference** — `glyph.rs:76`: `pub const STROKE: char = '\u{0335}';`

This is NOT a real combining codepoint in the NFD pipeline. It is used only as a sentinel value for `apply_quality()`. The đ/Đ characters have no canonical composition in Unicode, so they are handled as the lone special case (`glyph.rs:53–54`, `glyph.rs:60–68`).

### 1.3 NFD Output Path

```
engine.rs:70-75 — preedit_output()
    │
    ├── OutputMode::UnicodeDungSan (NFC) → self.display.clone()  (precomposed)
    │
    └── OutputMode::UnicodeToHop  (NFD) → self.display.nfd().collect()
                                           uses unicode_normalization crate
                                           decomposes all precomposed chars
```

**Key detail** (`engine.rs:69–75`): The `display` field always stores NFC (precomposed) text. The NFD decomposition happens only at the output boundary in `preedit_output()`. This means:
- `preedit_string()` always returns NFC (for display rendering)
- `preedit_output()` may return NFD depending on `output_mode`

The `NonPreeditEngine` wrapper (`fast_engine.rs:78`, line 210–217) also calls `inner.preedit_output()` at commit time, so NFD is honored in both code paths.

**Test coverage for NFD**:
- `engine_tests.rs:249–260` — `test_output_mode_nfd`: verifies NFD output contains combining marks and display stays NFC
- `fast_engine_tests.rs:207–256` — `test_nfd_output_on_word_boundary_commit`, `test_nfc_output_stays_precomposed`, `test_preedit_output_formats_buffer`

---

## 2. qu/gi Edge Cases — Backtracking Analysis

### 2.1 Algorithm

The backtracking logic lives in `analyze.rs:28–50` (`analyze()`). The key design:

```
for initial in initial_candidates(chars) {  // longest-first, ending with ""
    let rest = &chars[initial.chars().count()..];
    if rest.is_empty() { continue; }         // ← BACKTRACK trigger
    if let Some((cluster_idx, after)) = match_cluster(rest) {
        // check coda, return if valid
    }
}
```

`initial_candidates()` (`analyze.rs:53–59`) returns all `INITIALS` that prefix `chars` (longest-first from `tables.rs:9–13`), then falls through to the empty initial `""`.

### 2.2 Words That WORK

| Raw Keys | Normalized | Initial Tried | Rest | Result | Rendered |
|----------|------------|---------------|------|--------|----------|
| `gif` | `['g','i']` | `"gi"` → rest `[]` → **continue** | — | `g + i` | **gì** ✓ |
| `giaf` | `['g','i','a']` | `"gi"` → rest `['a']` → match | — | `gi + a` | **già** ✓ |
| `quas` | `['q','u','a']` | `"qu"` → rest `['a']` → match | — | `qu + a` | **quá** ✓ |
| `quoocs` | `['q','u','ô','c']` | `"qu"` → rest `['ô','c']` → match | `"c"` coda | `qu + ô + c` | **quốc** ✓ |
| `giangr` | `['g','i','a','ng']` | `"gi"` → rest `['a','ng']` → match | `"ng"` coda | `gi + a + ng` | **giảng** ✓ |
| `giuowngf` | `['g','i','ươ','ng']` | `"gi"` → rest `['ươ','ng']` → match | `"ng"` coda | `gi + ươ + ng` | **giường** ✓ |
| `gioongs` | `['g','i','ô','ng']` | `"gi"` → rest `['ô','ng']` → match | `"ng"` coda | `gi + ô + ng` | **giống** ✓ |

**Tests**: `parser_tests.rs:49–68` (`test_parse_gi_backtracking`, `test_parse_qu`), `golden_tests.rs:53–55`.

### 2.3 Words That FAIL (Known Limitations)

#### 2.3.1 "gii" — false positive for invalid syllables

`analyze(['g','i','i'])`: Try `"gi"` → rest `['i']` → **NOT empty** → `match_cluster(['i'])` succeeds (monophthong "i") → returns `initial="gi", cluster="i"`. This produces the rendered output **"gii"**, which is not a valid Vietnamese syllable.

**Root cause**: The backtracking only triggers when `rest.is_empty()` (line 37). When `rest` is non-empty and matches a vowel cluster, the initial is kept even if the resulting syllable is nonsense. There is no dictionary/lexical validation.

**Real-world impact**: Low. Users would rarely type "gii" intending Vietnamese.

#### 2.3.2 "qu" alone — accepted as a syllable

`analyze(['q','u'])`: Try `"qu"` → rest `[]` → empty → continue. Try `"q"` → rest `['u']` → match cluster "u" → return `initial="q", cluster="u", coda=""`. Rendered as **"qu"**.

**Is this valid?** Standard Vietnamese phonotactics requires `qu` to be followed by a vowel. "qu" alone is not a Vietnamese word. However, it passes structural validation.

#### 2.3.3 "quoa" — accepted as qu+oa

`analyze(['q','u','o','a'])`: Try `"qu"` → rest `['o','a']` → match diphthong "oa" → return `initial="qu", cluster="oa"`. Rendered as **"quoa"**, which is not standard Vietnamese (should be "qua" = qu+a, but "quoa" with the glide "o" already in the initial is phonotactically impossible).

#### 2.3.4 English tone residue (documented `#[ignore]`)

`engine_tests.rs:222–228` — `test_english_tone_residue_restores_raw`:

```rust
#[ignore = "known limitation: tone-key residue forms a structurally valid \
    syllable (ẻp/epres) — needs a real-syllable dictionary to catch; planned"]
fn test_english_tone_residue_restores_raw() {
    assert_eq!(commit_telex("expr"), "expr");     // FAILS
    assert_eq!(commit_telex("express"), "express"); // FAILS
}
```

**Root cause**: "expr" → the normalizer already produces `['ẻ', 'p']` because 'r' is a Telex tone key (hook). The analyzer sees `['ẻ','p']` which passes phonotactic validation as initial="", cluster="e", coda="p" — even though 'ẻ' already carries tone. The normalized chars contain a Vietnamese vowel before the analyzer can reject the English word.

### 2.4 Summary of Backtracking Gaps

| Gap | Severity | Fix Complexity |
|-----|----------|----------------|
| "gii" false positive | Low (unlikely input) | Medium (dictionary) |
| "qu" standalone | Low | Low (reject q+u without vowel) |
| "quoa" false positive | Low | Medium (phonotactic rules) |
| English tone residue | **Medium** (common English words) | High (dictionary or syllable validation) |

---

## 3. Current State Machine Design

### 3.1 `Action` Enum (`types.rs:117–124`)

```rust
pub enum Action {
    UpdatePreedit(String),  // Update the preedit string (in-progress composition)
    Commit(String),         // Commit the final composed string to the application
    PassThrough,            // Let the key pass through unchanged
}
```

This is the **core engine action** — returned by `Engine::push_key()` and `Engine::backspace()`.

### 3.2 `NonPreeditAction` Enum (`types.rs:52–71`)

```rust
pub enum NonPreeditAction {
    CommitWithBackspace { backspace_count: usize, text: String },
    UpdatePreedit(String),
    Buffer,
    PassThrough,
    ClearPreedit,
}
```

This is returned by `NonPreeditEngine::push_key()` for the Wayland layer. Key difference from `Action`:
- `CommitWithBackspace` — the Wayland layer must: (1) `delete_surrounding_text(-N, N)`, (2) `commit_string(text)`, (3) `commit(serial)`
- `Buffer` — non-preedit mode silently buffers with zero compositor roundtrips
- `ClearPreedit` — clear preedit display

### 3.3 `Engine` State Machine (`engine.rs`)

**Fields** (line 14–26):
- `raw_keys: Vec<char>` — single source of truth
- `display: String` — rendered NFC preedit (cached)
- `last_valid: bool` — drives `is_ambiguous()` for Hybrid mode
- `method: InputMethod` — Telex / Vni
- `output_mode: OutputMode` — UnicodeDungSan (NFC) / UnicodeToHop (NFD)
- `tone_style: ToneStyle` — Classic / Modern
- `auto_detect: bool` — English auto-restore
- `free_tone: bool` — kept for config compat, subsumed by phonotactic validation

**State transitions — `push_key(ch)`** (line 86–105):

```
                 ┌──────────────────────────────┐
                 │     is_word_boundary(ch)?     │
                 └──────────────┬───────────────┘
                      Yes │                │ No
                 ┌────────▼────────┐  ┌────▼────────────────────┐
                 │ has_preedit()?   │  │ raw_keys empty AND      │
                 └───┬──────────┬───┘  │ !ch.is_ascii_alphabetic?│
              Yes    │          │ No   └────┬────────────────┬────┘
         ┌───────────▼──┐  ┌───▼────┐  Yes │                │ No
         │ Commit(       │  │PassThrough│──▼────────┐  ┌─────▼──────────┐
         │ preedit_output│  └──────────┘│PassThrough│  │ raw_keys.push  │
         │ ()) + reset   │              └───────────┘  │ + reparse()    │
         └───────────────┘                              │ → UpdatePreedit│
                                                        └────────────────┘
```

**State transitions — `backspace()`** (line 107–119):

```
                 ┌──────────────────────┐
                 │ raw_keys.is_empty()?  │
                 └──────────┬───────────┘
                      Yes │         │ No
                 ┌────────▼──┐  ┌────▼──────────┐
                 │PassThrough│  │ raw_keys.pop() │
                 └───────────┘  └────┬───────────┘
                                     │
                           ┌─────────▼──────────┐
                           │ raw_keys empty now? │
                           └────┬──────────┬─────┘
                          Yes   │          │ No
                    ┌───────────▼──┐  ┌─────▼──────────┐
                    │ display.clear│  │ reparse()       │
                    │ last_valid=F │  │ → UpdatePreedit │
                    │ → UpdatePreedit(│ └────────────────┘
                    │   String::new())│
                    └────────────────┘
```

**`reparse()` transitions** (line 128–145):

```
parser::parse(&raw_keys, method)
    │
    ├── ParseOutcome::Valid(p)
    │       → p.render_into(&mut display, tone_style)
    │       → last_valid = true
    │
    ├── ParseOutcome::Literal(chars, case)
    │       → render_literal_into(&mut display, &chars, case)
    │       → last_valid = true   // user-forced literal is not "ambiguous"
    │
    └── ParseOutcome::Invalid
            → display.clear()
            → display.extend(raw_keys.iter())  // restore raw keys verbatim
            → last_valid = false
```

### 3.4 `NonPreeditEngine` State Machine (`fast_engine.rs`)

This is a **wrapper** over `Engine` that implements VMK1-style non-preedit typing.

**Fields** (line 24–34):
- `inner: Engine` — core engine
- `mode: ImeMode` — NonPreedit / Preedit / Hybrid
- `raw_count: usize` — number of raw keys buffered
- `preedit_strategy: PreeditStrategy` — Always / Never / OnAmbiguous / OnDemand

**`push_key()` transitions** (line 53–130):

```
┌── control char (not backspace)?
│   └── commit pending if any → CommitWithBackspace, else PassThrough
├── backspace?
│   └── handle_backspace() → ClearPreedit / UpdatePreedit / Buffer / PassThrough
├── word boundary?
│   └── commit pending if any → CommitWithBackspace, else PassThrough
└── process key
    ├── raw_count += 1
    ├── inner.push_key(ch)
    └── match inner action:
        ├── Action::Commit(s) → CommitWithBackspace { raw_count, text: s }
        ├── Action::UpdatePreedit(_)
        │   └── should_show_preedit()?
        │       ├── Yes → UpdatePreedit(preedit_string)
        │       └── No  → Buffer
        └── Action::PassThrough
            └── has_preedit? → flush as CommitWithBackspace, else PassThrough
```

**`should_show_preedit()`** (line 160–167):
- `PreeditStrategy::Always` → true (standard preedit mode)
- `PreeditStrategy::Never` → false (non-preedit mode)
- `PreeditStrategy::OnAmbiguous` → `inner.is_ambiguous()` (hybrid mode)
- `PreeditStrategy::OnDemand` → false (user toggle, TODO — not yet implemented)

### 3.5 `ImeMode` Enum (`types.rs:12–22`)

```rust
pub enum ImeMode {
    Preedit,      // Standard preedit mode — ~60-70% app compat
    NonPreedit,   // VMK1-style silent buffer — >90% app compat
    Hybrid,       // Preedit only when ambiguous — ~75-80% app compat
}
```

### 3.6 `PreeditStrategy` Enum (`types.rs:39–49`)

```rust
pub enum PreeditStrategy {
    Always,       // Standard mode
    OnAmbiguous,  // Hybrid mode
    OnDemand,     // User toggle (TODO — not yet implemented)
    Never,        // Non-preedit mode
}
```

**Gap**: `PreeditStrategy::OnDemand` exists in the enum but is stubbed to `false` in `should_show_preedit()` (`fast_engine.rs:165`).

---

## 4. Test Coverage Analysis

### 4.1 Test Files (6 files, ~95 test functions)

| File | # Tests | Focus |
|------|---------|-------|
| `tests/vi-engine/parser_tests.rs` | 24 | Parser internals: structure, tone placement, qu/gi, undo, case, VNI, glyph algebra |
| `tests/vi-engine/engine_tests.rs` | 38 | Engine facade: Telex/VNI quality marks, tones, words, backspace, boundaries, NFD, English restore |
| `tests/vi-engine/golden_tests.rs` | 2 | Golden corpus: ~87 Telex cases + ~16 VNI cases covering all vần groups |
| `tests/vi-engine/hybrid_tests.rs` | 14 | Smoke/regression: defaults, mode switching, backspace, reset, rapid typing |
| `tests/vi-engine/rollover_tests.rs` | 15 | NonPreeditEngine: buffer, commit, backspace, mode switching, VNI |
| `tests/vi-engine/fast_engine_tests.rs` | 14 | NonPreeditEngine + AdaptiveDelay: commits, backspace, hybrid, NFD, benchmarks |

### 4.2 Complete Test Case Inventory

#### parser_tests.rs (24 tests)

| # | Test Name | What It Covers | Status |
|---|-----------|----------------|--------|
| 1 | `test_parse_tieng_structure` | initial/cluster/coda/tone decomposition | ✅ |
| 2 | `test_parse_gi_backtracking` | gi→g backtrack (gì) AND gi kept (già) | ✅ |
| 3 | `test_parse_qu` | qu initial (quá, quốc) | ✅ |
| 4 | `test_parse_ngh` | ngh initial (nghiêng) | ✅ |
| 5 | `test_tone_ua_ia_eo_au_ao` | Tone on first vowel of diphthongs | ✅ |
| 6 | `test_tone_with_coda_always_last_vowel` | hoán, huỳnh, tuần, hoặc, nguyễn | ✅ |
| 7 | `test_tone_style_glide_clusters` | Classic vs Modern (hòa/hoà, thúy/thuý) | ✅ |
| 8 | `test_uo_horn_pair` | ươ via uow/uw+ow (thương, đường, giường, người, rượu) | ✅ |
| 9 | `test_w_scan_back` | w after coda modifies vowel (thuongw→thương) | ✅ |
| 10 | `test_double_tone_key_undo` | ass→as (undo semantics) | ✅ |
| 11 | `test_double_merge_undo` | ddd→dd, aaa→aa, uww→uw, www→ww, xooong→xoong | ✅ |
| 12 | `test_tone_change` | afs→á, asf→à (tone toggle) | ✅ |
| 13 | `test_z_removes_tone` | asz→a, vieejtz→viêt | ✅ |
| 14 | `test_case_upper_dd` | Đại, Ấn, Việt, VIỆT | ✅ |
| 15 | `test_english_words_invalid` | express, windows, html, crush, thanks → Invalid | ✅ |
| 16 | `test_invalid_consonant_cluster` | csn, zzz → Invalid | ✅ |
| 17 | `test_vni_basic` | tiếng, thuộc, đường, việt via VNI | ✅ |
| 18 | `test_vni_tone_toggle` | ma11→ma1, ma12 tone change | ✅ |
| 19 | `test_parse_tone_toggle_literal_mode` | tiss→tis, tissa→tisa (literal after undo) | ✅ |
| 20 | `test_empty_and_tone_only` | "" and "s" → Invalid | ✅ |
| 21 | `test_glyph_compose_nfc` | Unicode algebra: compose, quality marks | ✅ |
| 22 | `test_glyph_apply_quality` | đ/Đ stroke, horn | ✅ |
| 23 | `test_glyph_base_of` | ệ→e, ư→u, đ→d | ✅ |
| 24 | `test_glyph_tone_marks_cover_all_tones` | All 5 tones have non-None marks | ✅ |

#### engine_tests.rs (38 tests)

| # | Test Name | Status |
|---|-----------|--------|
| 1 | `test_telex_aa` (â) | ✅ |
| 2 | `test_telex_aw` (ă) | ✅ |
| 3 | `test_telex_ee` (ê) | ✅ |
| 4 | `test_telex_oo` (ô) | ✅ |
| 5 | `test_telex_ow` (ơ) | ✅ |
| 6 | `test_telex_dd` (đi) | ✅ |
| 7 | `test_telex_w_as_u_horn` (ư) | ✅ |
| 8 | `test_telex_tones` (á,à,ả,ã,ạ) | ✅ |
| 9 | `test_telex_tone_change` (afs, asf) | ✅ |
| 10 | `test_telex_tone_undo` (ass→as) | ✅ |
| 11 | `test_telex_z_removes_tone` (asz→a) | ✅ |
| 12 | `test_telex_viet` (việt) | ✅ |
| 13 | `test_telex_nam` (năm) | ✅ |
| 14 | `test_telex_nghieng` (nghiêng) | ✅ |
| 15 | `test_telex_uong` (uống) | ✅ |
| 16 | `test_telex_chuyen` (chuyển) | ✅ |
| 17 | `test_telex_huynh` (huỳnh) | ✅ |
| 18 | `test_telex_thuong` (thương) | ✅ |
| 19 | `test_telex_giang` (giảng) | ✅ |
| 20 | `test_telex_mua_cua` (mùa, của) | ✅ |
| 21 | `test_telex_reset` | ✅ |
| 22 | `test_backspace_empty_passes_through` | ✅ |
| 23 | `test_backspace_rederives` | ✅ |
| 24 | `test_backspace_single` | ✅ |
| 25 | `test_vni_tones` | ✅ |
| 26 | `test_vni_words` (tiếng, đường, việt) | ✅ |
| 27 | `test_space_commits` | ✅ |
| 28 | `test_digit_is_boundary_in_telex` | ✅ |
| 29 | `test_non_vietnamese_chars_pass_through` | ✅ |
| 30 | `test_english_word_restores_raw` (html, csn) | ✅ |
| 31 | `test_english_windows_thanks` (windows, thanks, crush) | ✅ |
| 32 | `test_english_tone_residue_restores_raw` (expr, express) | ❌ `#[ignore]` |
| 33 | `test_set_method` | ✅ |
| 34 | `test_has_preedit` | ✅ |
| 35 | `test_output_mode_nfd` | ✅ |
| 36 | `test_tone_style` (hòa/hoà) | ✅ |
| 37 | `test_is_ambiguous` | ✅ |
| 38 | `test_uppercase` (Đại, Ấn, Việt) | ✅ |

#### golden_tests.rs (2 tests with ~103 sub-cases)

- `golden_telex`: 87 key→expected pairs covering all vần groups
- `golden_vni`: 16 key→expected pairs

#### hybrid_tests.rs (14 tests)

All smoke/regression: `engine_new_defaults`, `set_output_mode`, `switch_input_method`, `auto_detect_works`, `backspace_in_preedit`, `backspace_empty_buffer`, `backspace_then_retype`, `engine_reset_clears_preedit`, `deactivate_then_new_typing`, `raw_key_count`, `telex_word_commits`, `rapid_typing_no_crash`, `many_tone_marks_no_crash`, `empty_input_no_crash`, `very_long_word`, `preedit_string_tracks_buffer`.

#### rollover_tests.rs (15 tests)

All NonPreeditEngine smoke tests covering buffer/commit/mode-switching/reset/VNI.

#### fast_engine_tests.rs (14 tests)

NonPreeditEngine detailed tests + AdaptiveDelay + benchmarks + NFD regression.

### 4.3 Coverage Gaps

| Gap | Severity | Description |
|-----|----------|-------------|
| **No Modern tone style tests in golden** | Medium | Golden tests only use `ToneStyle::Classic` (default). No golden for Modern style ("hoà", "thuý"). |
| **No qu backtracking failure tests** | Medium | No tests for "q"+"u" standalone, "quoa", or "gii" edge cases. |
| **English tone residue ignored** | High | `test_english_tone_residue_restores_raw` is `#[ignore]`d — "expr" and "express" fail. |
| **No VNI golden for qu/gi backtracking** | Low | VNI golden has `("qua1", "quá")` and `("gi2", "gì")` but no "gi+a" or "qu+ôc" VNI cases. |
| **No digraph/ph/th/tr/ch/nh stress tests** | Low | These initials appear in golden tests but only 1-2 cases each. No systematic coverage. |
| **No tone-on-glide Modern style without coda** | Medium | Only `test_tone_style_glide_clusters` tests oa/oe/uy. Missing: uyê, oai, oay, oeo with Modern style. |
| **No coda-only (no initial) tests** | Low | Words like "anh", "em", "im", "yên" are not explicitly tested. |
| **No fuzz/property tests** | Medium | No random input fuzzing to catch crashes or invalid Vietnamese output. |
| **No regression tests for specific old bugs** | Low | No tests referencing specific GitHub issues or known regressions. |
| **`PreeditStrategy::OnDemand` untested** | Low | Stubbed to `false`, no test exercising the OnDemand code path. |
| **No VNI digit-as-modifier backtracking tests** | Low | VNI digits scan back for targets — no test for edge cases like digit-after-coda. |
| **No upper/lowercase mixed for qu/gi** | Low | "QUa", "GIa" etc. not tested. |
| **No AdaptiveDelay convergence test** | Low | AdaptiveDelay observe/calculate tested but no long-sequence convergence test. |

### 4.4 Test Statistics

| Metric | Count |
|--------|-------|
| Total test functions | ~95 |
| Passing | ~93 |
| Ignored | 1 (`test_english_tone_residue_restores_raw`) |
| Golden corpus cases (Telex) | 87 |
| Golden corpus cases (VNI) | 16 |
| Benchmark tests | 2 |
| Test files | 6 |

---

## Appendix: File Reference Map

```
crates/vi-engine/src/
├── lib.rs              — crate root, re-exports, test module registration
├── tone.rs             — Tone enum (Level, Acute, Grave, Hook, Tilde, Dot)
├── types.rs            — Action, NonPreeditAction, InputMethod, ImeMode,
│                         OutputMode, LanguageMode, PreeditStrategy, AppSupport
├── engine.rs           — Engine state machine (push_key, backspace, reparse)
├── fast_engine.rs      — NonPreeditEngine wrapper + AdaptiveDelay
└── parser/
    ├── mod.rs          — ParseOutcome, Parsed, ToneStyle, CaseHint, parse(), detect_case()
    ├── glyph.rs        — Unicode combining constants + compose/base_of/apply_quality
    ├── normalize.rs    — Telex/VNI → quality-marked chars + tone + undo semantics
    ├── analyze.rs      — Phonotactic analysis + initial backtracking
    ├── tables.rs       — INITIALS, CODAS, VOWEL_CLUSTERS (const tables)
    └── render.rs       — Render decomposed→string with tone placement

tests/vi-engine/
├── parser_tests.rs     — 24 tests: normalization, phonotactics, undo, glyph algebra
├── engine_tests.rs     — 38 tests: Engine facade, tones, words, backspace, NFD
├── golden_tests.rs     — 2 tests: 87 Telex + 16 VNI golden corpus cases
├── hybrid_tests.rs     — 14 tests: engine smoke/regression
├── rollover_tests.rs   — 15 tests: NonPreeditEngine smoke
└── fast_engine_tests.rs— 14 tests: NonPreeditEngine detail + AdaptiveDelay + benchmarks
```

# NFD-based Mathematical Engine — Full Implementation

## Tổng quan thiết kế

Engine dùng **Non-Preedit approach**: virtual backspace xóa raw keys, commit
chuỗi NFC Vietnamese tại word boundary.

**Pipeline 2 bước:**
1. `normalize_smart()` → input disambiguation (VNI/Telex)
2. NFD-based engine → output rendering

**Core principle:** Parse, don't mutate — mỗi keystroke re-parse toàn bộ raw
buffer, không mutate state.

---

## File 1: `nfd_engine.rs` — Core NFD Math Engine

> **Path:** `crates/vi-engine/src/engine/nfd_engine.rs`

Thay thế 55-entry `VOWEL_CLUSTERS` lookup table bằng Unicode NFD math.

```rust
//! NFD-based Mathematical Vietnamese Engine
//!
//! Thay thế 55-entry VOWEL_CLUSTERS lookup table bằng Unicode NFD math.
//! Nguyên tắc: "Parse, don't mutate" — raw_keys là source of truth.

use unicode_normalization::UnicodeNormalization;

// ─── Unicode combining diacritic codepoints ───────────────────────────
/// Tone marks (thanh điệu) — Combining diacritical marks
const COMBINING_GRAVE:      char = '\u{0300}'; // huyền
const COMBINING_ACUTE:      char = '\u{0301}'; // sắc
const COMBINING_HOOK_ABOVE: char = '\u{0309}'; // hỏi
const COMBINING_TILDE:      char = '\u{0303}'; // ngã
const COMBINING_DOT_BELOW:  char = '\u{0323}'; // nặng

/// Vowel shape marks (dấu hình)
const COMBINING_CIRCUMFLEX: char = '\u{0302}'; // â, ê, ô (Telex: aa, ee, oo)
const COMBINING_BREVE:      char = '\u{0306}'; // ă       (Telex: aw)
const COMBINING_HORN:       char = '\u{031B}'; // ư, ơ    (Telex: uw, ow)
```

### Core Types

```rust
/// Tone theo TCVN 6909:2001
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Flat,       // ngang (không dấu)
    Grave,      // huyền
    Acute,      // sắc
    HookAbove,  // hỏi
    Tilde,      // ngã
    DotBelow,   // nặng
}

/// Vowel shape diacritic
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VowelShape {
    None,
    Circumflex,  // â, ê, ô
    Breve,       // ă
    Horn,        // ư, ơ
}

/// Một âm tiết tiếng Việt đã parse
#[derive(Debug, Clone)]
pub struct ViSyllable {
    pub onset:   String,  // phụ âm đầu (có thể rỗng)
    pub nucleus: String,  // nguyên âm (vowel cluster)
    pub coda:    String,  // phụ âm cuối (có thể rỗng)
    pub tone:    Tone,
}
```

### NFD Engine Entry Point

```rust
pub struct NfdEngine;

impl NfdEngine {
    /// Entry point chính: raw_keys → NFC Vietnamese string
    pub fn process(raw_keys: &[char]) -> String {
        // Bước 1: Detect method + strip glide onsets
        let (onset_glide, rest) = strip_glide_onset(raw_keys);

        // Bước 2: Detect method (Telex vs VNI) và parse
        let syllable = match detect_method(rest) {
            InputMethod::Telex => parse_telex(onset_glide, rest),
            InputMethod::Vni   => parse_vni(onset_glide, rest),
            InputMethod::Raw   => return raw_keys.iter().collect(),
        };

        // Bước 3: Build NFD string với combining marks
        let nfd = build_nfd(&syllable);

        // Bước 4: Normalize NFD → NFC trước khi commit
        nfd.nfc().collect()
    }

    /// Word boundary check — trigger commit
    pub fn is_word_boundary(k: char) -> bool {
        matches!(k, ' ' | '\t' | '\n' | '\r' | '.' | ',' | '!' | '?'
                  | ':' | ';' | '(' | ')' | '[' | ']' | '{' | '}'
                  | '"' | '\'' | '/' | '\\' | '@' | '#')
    }
}
```

### Glide Onset Stripping

```rust
/// Strip 'qu' và 'gi' như distinct initial consonant clusters
/// TRƯỚC khi parse để tránh nhầm 'u'/'i' thành nguyên âm
fn strip_glide_onset(keys: &[char]) -> (&'static str, &[char]) {
    if keys.len() >= 2 {
        match (keys[0], keys[1]) {
            ('q', 'u') => return ("qu", &keys[2..]),
            ('g', 'i') => return ("gi", &keys[2..]),
            _ => {}
        }
    }
    ("", keys)
}
```

### Input Method Detection

```rust
#[derive(Debug, PartialEq)]
enum InputMethod { Telex, Vni, Raw }

/// Telex: dấu là chữ cái (s, f, r, x, j; aa, ee, oo, aw, ow, uw, dd)
/// VNI:   dấu là số (1-9)
fn detect_method(keys: &[char]) -> InputMethod {
    let has_vni_tone  = keys.iter().any(|&c| matches!(c, '1'..='9'));
    let has_telex_tone = keys.iter().any(|&c| {
        matches!(c, 's' | 'f' | 'r' | 'x' | 'j')
    });
    let has_telex_shape = {
        let s: String = keys.iter().collect();
        s.contains("aa") || s.contains("ee") || s.contains("oo")
            || s.contains("aw") || s.contains("ow") || s.contains("uw")
            || s.contains("dd")
    };
    if has_vni_tone && !has_telex_tone && !has_telex_shape {
        InputMethod::Vni
    } else if has_telex_tone || has_telex_shape {
        InputMethod::Telex
    } else {
        InputMethod::Raw  // Không có tone marker → giữ nguyên (plain latin)
    }
}
```

### Telex Parser

```rust
fn parse_telex(onset_glide: &str, keys: &[char]) -> ViSyllable {
    let raw: String = keys.iter().collect();
    let mut buf = raw.as_str();

    // 1. Parse onset (initial consonants)
    let (onset_str, after_onset) = split_onset(buf);
    let onset = format!("{}{}", onset_glide, onset_str);
    buf = after_onset;

    // 2. Giải quyết shape marks trước (vì chúng thay đổi vowel letters)
    let buf_dd = buf.replace("dd", "đ");
    buf = &buf_dd;

    // 3. Parse vowel cluster + shape marks
    let (nucleus_raw, shape, after_nucleus) = parse_telex_nucleus(buf);
    let coda = after_nucleus
        .trim_end_matches(|c: char| matches!(c, 's' | 'f' | 'r' | 'x' | 'j'))
        .to_string();

    // 4. Parse tone (ký tự cuối: s=sắc, f=huyền, r=hỏi, x=ngã, j=nặng)
    let tone = parse_telex_tone(&raw);

    ViSyllable { onset, nucleus: apply_shape_nfd(&nucleus_raw, shape), coda, tone }
}

fn parse_telex_tone(s: &str) -> Tone {
    match s.chars().last() {
        Some('s') => Tone::Acute,
        Some('f') => Tone::Grave,
        Some('r') => Tone::HookAbove,
        Some('x') => Tone::Tilde,
        Some('j') => Tone::DotBelow,
        _         => Tone::Flat,
    }
}

fn parse_telex_nucleus(s: &str) -> (String, VowelShape, &str) {
    for (digraph, shape, base) in [
        ("aa", VowelShape::Circumflex, "a"),
        ("ee", VowelShape::Circumflex, "e"),
        ("oo", VowelShape::Circumflex, "o"),
        ("aw", VowelShape::Breve,      "a"),
        ("ow", VowelShape::Horn,       "o"),
        ("uw", VowelShape::Horn,       "u"),
    ] {
        if s.starts_with(digraph) {
            return (base.to_string(), shape, &s[digraph.len()..]);
        }
    }
    // No shape mark — parse first vowel sequence
    let end = s.find(|c: char| !is_vowel_base(c)).unwrap_or(s.len());
    (s[..end].to_string(), VowelShape::None, &s[end..])
}
```

### VNI Parser

```rust
fn parse_vni(onset_glide: &str, keys: &[char]) -> ViSyllable {
    let raw: String = keys.iter().collect();
    let (onset_str, after_onset) = split_onset(&raw);
    let onset = format!("{}{}", onset_glide, onset_str);

    // VNI shape marks: a6=â, a8=ă, e6=ê, o6=ô, o7=ơ, u7=ư, d9=đ
    let (nucleus_raw, shape) = parse_vni_nucleus(after_onset);

    // VNI tone: 1=sắc, 2=huyền, 3=hỏi, 4=ngã, 5=nặng, 0=xóa dấu
    let tone = parse_vni_tone(&raw);
    let coda = extract_vni_coda(after_onset);

    ViSyllable { onset, nucleus: apply_shape_nfd(&nucleus_raw, shape), coda, tone }
}

fn parse_vni_tone(s: &str) -> Tone {
    for c in s.chars().rev() {
        match c {
            '1' => return Tone::Acute,
            '2' => return Tone::Grave,
            '3' => return Tone::HookAbove,
            '4' => return Tone::Tilde,
            '5' => return Tone::DotBelow,
            '0' => return Tone::Flat,
            _ => {}
        }
    }
    Tone::Flat
}

fn parse_vni_nucleus(s: &str) -> (String, VowelShape) {
    let mut chars = s.chars().peekable();
    let mut base = String::new();
    let mut shape = VowelShape::None;

    while let Some(&c) = chars.peek() {
        match c {
            'a' | 'e' | 'i' | 'o' | 'u' | 'y' => {
                base.push(c);
                chars.next();
                // Check shape marker immediately after vowel
                match chars.peek() {
                    Some(&'6') => { shape = VowelShape::Circumflex; chars.next(); }
                    Some(&'8') => { shape = VowelShape::Breve;      chars.next(); }
                    Some(&'7') => { shape = VowelShape::Horn;       chars.next(); }
                    _ => {}
                }
                break;
            }
            _ => break,
        }
    }
    (base, shape)
}

fn extract_vni_coda(s: &str) -> String {
    s.chars()
     .skip_while(|c| !is_vowel_base(*c))
     .skip_while(|c| is_vowel_base(*c) || c.is_ascii_digit())
     .filter(|c| c.is_alphabetic())
     .collect()
}
```

### NFD Builder — Tone Placement

```rust
/// Build NFD string từ ViSyllable
/// Đặt combining marks đúng thứ tự: shape mark trước, tone mark sau
fn build_nfd(syl: &ViSyllable) -> String {
    let mut result = String::new();
    result.push_str(&syl.onset);

    let tone_pos = find_tone_position(&syl.nucleus, &syl.coda);
    for (i, c) in syl.nucleus.chars().enumerate() {
        result.push(c);
        if i == tone_pos {
            if let Some(cm) = tone_to_combining(syl.tone) {
                result.push(cm);
            }
        }
    }
    result.push_str(&syl.coda);
    result
}

/// Tìm vị trí đặt tone trong nucleus (TCVN 6909 "new rules")
fn find_tone_position(nucleus: &str, coda: &str) -> usize {
    let chars: Vec<char> = nucleus.chars().collect();
    let n = chars.len();
    if n <= 1 { return 0; }

    // Rule 1: Nếu có coda → đặt trên vowel cuối của nucleus
    if !coda.is_empty() { return n - 1; }

    // Rule 2: "uye", "uê", "oe", "oa" → đặt trên vowel cuối
    if n >= 2 {
        let last = chars[n - 1];
        if "eêioyướ".contains(last) || is_vowel_base(last) {
            return n - 1;
        }
    }

    // Rule 3: Với "ia", "ua", "ưa" (vowel + 'a' không coda) → đặt trên vowel đầu
    if n == 2 && chars[1] == 'a' { return 0; }

    n - 1
}

fn tone_to_combining(tone: Tone) -> Option<char> {
    match tone {
        Tone::Flat      => None,
        Tone::Grave     => Some(COMBINING_GRAVE),
        Tone::Acute     => Some(COMBINING_ACUTE),
        Tone::HookAbove => Some(COMBINING_HOOK_ABOVE),
        Tone::Tilde     => Some(COMBINING_TILDE),
        Tone::DotBelow  => Some(COMBINING_DOT_BELOW),
    }
}

/// Áp dụng shape mark vào vowel base bằng NFD combining
fn apply_shape_nfd(base: &str, shape: VowelShape) -> String {
    let cm = match shape {
        VowelShape::None       => return base.to_string(),
        VowelShape::Circumflex => COMBINING_CIRCUMFLEX,
        VowelShape::Breve      => COMBINING_BREVE,
        VowelShape::Horn       => COMBINING_HORN,
    };
    let mut out = String::new();
    let mut inserted = false;
    for c in base.chars() {
        out.push(c);
        if !inserted && is_vowel_base(c) {
            out.push(cm);
            inserted = true;
        }
    }
    out
}
```

### Helpers

```rust
fn is_vowel_base(c: char) -> bool {
    matches!(c.to_ascii_lowercase(),
        'a' | 'e' | 'i' | 'o' | 'u' | 'y'
        | 'ă' | 'â' | 'ê' | 'ô' | 'ơ' | 'ư'
    )
}

/// Tách phụ âm đầu từ chuỗi raw
/// Ví dụ: "ngang" → onset="ng", rest="ang"
fn split_onset(s: &str) -> (&str, &str) {
    const MULTI_ONSETS: &[&str] = &[
        "ngh", "ng", "gh", "kh", "ph", "th", "tr", "ch", "nh",
    ];
    for onset in MULTI_ONSETS {
        if s.starts_with(onset) {
            return (&s[..onset.len()], &s[onset.len()..]);
        }
    }
    if let Some(c) = s.chars().next() {
        if !is_vowel_base(c) {
            let len = c.len_utf8();
            return (&s[..len], &s[len..]);
        }
    }
    ("", s)
}
```

---

## File 2: `mod.rs` — Module Re-exports + ViEngine Trait

> **Path:** `crates/vi-engine/src/engine/mod.rs`

```rust
pub mod nfd_engine;
pub mod smart;

pub use nfd_engine::{NfdEngine, Tone, VowelShape, ViSyllable};
pub use smart::normalize_smart;

/// Trait chung cho mọi engine implementation
pub trait ViEngine: Send + Sync {
    fn process(&self, raw_keys: &[char]) -> String;
    fn is_word_boundary(&self, k: char) -> bool {
        NfdEngine::is_word_boundary(k)
    }
}

/// NFD engine instance (default)
pub struct ModernVietnameseEngine;

impl ViEngine for ModernVietnameseEngine {
    fn process(&self, raw_keys: &[char]) -> String {
        NfdEngine::process(raw_keys)
    }
}
```

---

## File 3: `smart.rs` — Smart Method (Mixed VNI/Telex)

> **Path:** `crates/vi-engine/src/engine/smart.rs`

```rust
//! Smart mode: auto-detect VNI/Telex, Telex preferred on conflict.

use super::nfd_engine::NfdEngine;

/// Two-step Smart normalization pipeline:
/// 1. normalize_smart() → disambiguate VNI/Telex
/// 2. NfdEngine::process() → NFD render → NFC output
pub fn normalize_smart(raw_keys: &[char]) -> String {
    // Step 1: strip glide onsets ('qu', 'gi') TRƯỚC
    let (glide, rest) = strip_glide_onset_smart(raw_keys);

    // Step 2: detect conflict & resolve (Telex wins)
    let resolved = resolve_conflict(rest);

    // Step 3: forward vào NFD engine
    let full: Vec<char> = glide.chars().chain(resolved.iter().copied()).collect();
    NfdEngine::process(&full)
}

fn strip_glide_onset_smart(keys: &[char]) -> (&'static str, &[char]) {
    if keys.len() >= 2 {
        match (keys[0], keys[1]) {
            ('q', 'u') => return ("qu", &keys[2..]),
            ('g', 'i') => return ("gi", &keys[2..]),
            _ => {}
        }
    }
    ("", keys)
}

/// Resolve VNI/Telex conflict — Telex wins
fn resolve_conflict(keys: &[char]) -> Vec<char> {
    let has_telex_marker = keys.iter().any(|&c| {
        matches!(c, 's' | 'f' | 'r' | 'x' | 'j')
    });
    let has_vni_marker = keys.iter().any(|&c| {
        matches!(c, '1' | '2' | '3' | '4' | '5')
    });
    if has_telex_marker && has_vni_marker {
        // Conflict: strip VNI numbers, keep Telex
        keys.iter().copied().filter(|c| !c.is_ascii_digit()).collect()
    } else {
        keys.to_vec()
    }
}
```

---

## File 4: `buffer.rs` — KeyBuffer (Source of Truth)

> **Path:** `crates/vi-engine/src/engine/buffer.rs`

```rust
//! Raw key buffer — source of truth cho re-parsing.
//! Theo nguyên tắc "Parse, don't mutate".

use crate::engine::{ViEngine, NfdEngine};

pub struct KeyBuffer {
    /// Source of truth: raw ASCII keystrokes từ người dùng
    raw_keys: Vec<char>,
    /// Cache của lần parse cuối (invalidated khi raw_keys thay đổi)
    cached_output: Option<String>,
}

impl KeyBuffer {
    pub fn new() -> Self {
        Self { raw_keys: Vec::new(), cached_output: None }
    }

    /// Thêm keystroke, invalidate cache
    pub fn push(&mut self, k: char) {
        self.raw_keys.push(k);
        self.cached_output = None;
    }

    /// Xóa ký tự cuối (backspace từ người dùng)
    pub fn backspace(&mut self) {
        self.raw_keys.pop();
        self.cached_output = None;
    }

    /// Reset hoàn toàn sau commit
    pub fn clear(&mut self) {
        self.raw_keys.clear();
        self.cached_output = None;
    }

    /// Số ký tự cần virtual-backspace trước khi commit
    pub fn raw_len(&self) -> usize { self.raw_keys.len() }

    /// Parse toàn bộ buffer → Vietnamese string (lazy, cached)
    pub fn render(&mut self) -> &str {
        if self.cached_output.is_none() {
            self.cached_output = Some(NfdEngine::process(&self.raw_keys));
        }
        self.cached_output.as_deref().unwrap()
    }

    /// Check word boundary → trigger commit
    pub fn should_commit(&self, incoming: char) -> bool {
        NfdEngine::is_word_boundary(incoming)
    }
}
```

---

## Dependencies

```toml
[dependencies]
unicode-normalization = "0.1"
```

---

## Flow Tổng Thể

```
Wayland key event
        │
        ▼
  KeyBuffer::push(k)
        │
        ├─── is_word_boundary? ──── YES ──→ virtual_backspace(n) + commit_string(NFC)
        │                                          │
        │                                   KeyBuffer::clear()
        │
        NO
        │
        ▼
  KeyBuffer::render()
        │
        ▼
  NfdEngine::process(raw_keys)
        │
  ┌─────┴──────────────────────────────────────┐
  │ 1. strip_glide_onset (qu, gi)               │
  │ 2. detect_method (Telex/VNI/Raw)            │
  │ 3. parse_telex / parse_vni                  │
  │    → ViSyllable { onset, nucleus, coda, tone}│
  │ 4. build_nfd() + tone placement             │
  │ 5. .nfc().collect() → NFC String            │
  └──────────────────────────────────────────────┘
```

---

## Test Cases

```rust
#[cfg(test)]
mod tests {
    use super::engine::NfdEngine;
    use super::engine::smart::normalize_smart;

    // ─── Telex tests ───────────────────────────────────────────────
    #[test]
    fn test_telex_viet() {
        let keys = vec!['v', 'i', 'e', 't', 'j'];
        assert_eq!(NfdEngine::process(&keys), "việt");
    }

    #[test]
    fn test_telex_toan() {
        let keys = vec!['t', 'o', 'a', 'n', 'f'];
        assert_eq!(NfdEngine::process(&keys), "toàn");
    }

    // ─── VNI tests ─────────────────────────────────────────────────
    #[test]
    fn test_vni_viet() {
        let keys = vec!['v', 'i', 'e', 't', '5'];
        assert_eq!(NfdEngine::process(&keys), "việt");
    }

    #[test]
    fn test_vni_toan() {
        let keys = vec!['t', 'o', '6', 'a', 'n', '2'];
        assert_eq!(NfdEngine::process(&keys), "toàn");
    }

    // ─── Glide edge cases ──────────────────────────────────────────
    #[test]
    fn test_qu_glide_quoc() {
        let keys = vec!['q', 'u', 'o', 'o', 'c', 'j'];
        assert_eq!(NfdEngine::process(&keys), "quốc");
    }

    #[test]
    fn test_gi_glide_giao() {
        let keys = vec!['g', 'i', 'a', 'o', 'f'];
        assert_eq!(NfdEngine::process(&keys), "giào");
    }

    // ─── Smart mode tests ──────────────────────────────────────────
    #[test]
    fn test_smart_pure_telex() {
        let keys = vec!['t', 'o', 'a', 'n', 'f'];
        assert_eq!(normalize_smart(&keys), "toàn");
    }

    // ─── Word boundary ─────────────────────────────────────────────
    #[test]
    fn test_word_boundary() {
        assert!(NfdEngine::is_word_boundary(' '));
        assert!(NfdEngine::is_word_boundary('.'));
        assert!(!NfdEngine::is_word_boundary('a'));
    }

    // ─── NFD → NFC normalization ───────────────────────────────────
    #[test]
    fn test_output_is_nfc() {
        use unicode_normalization::UnicodeNormalization;
        let keys = vec!['v', 'i', 'e', 't', 'j'];
        let result = NfdEngine::process(&keys);
        let nfc: String = result.nfc().collect();
        assert_eq!(result, nfc, "Output phải là NFC");
    }
}
```

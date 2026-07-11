# Glyph Algorithm — NFD/NFC Unicode Algebra cho chữ Việt

File [`crates/vi-daemon/src/engine/glyph.rs`](../crates/vi-daemon/src/engine/glyph.rs) là trái tim của engine VI-IME: thuật toán tạo chữ Unicode dựng sẵn (precomposed) bằng NFD/NFC composition, không dùng bảng tra cứu.

## Nguyên lý

Một chữ cái tiếng Việt = `base × quality × tone`. Thay vì hardcode bảng mapping `'a'→'á'` (60+ entry), `glyph.rs` dùng chính **database Unicode** làm bảng tra:

```
base vowel + combining mark → NFC → precomposed char (hoặc None nếu không tồn tại)
```

### Các combining codepoint

| Hằng số | Mã Unicode | Vai trò |
|---|---|---|
| `CIRCUMFLEX` `◌̂` | U+0302 | â ê ô |
| `BREVE` `◌̆` | U+0306 | ă |
| `HORN` `◌̛` | U+031B | ơ ư |
| `tone_mark(Tone)` | U+0301/0300/0309/0303/0323 | sắc/huyền/hỏi/ngã/nặng |

## Pipeline

```
raw keys → normalize (quality marks: â/ê/ô/ă/ơ/ư/đ)
        → syllable (tone marks: sắc/huyền/hỏi/ngã/nặng)
        → NFC compose → precomposed Unicode char
```

Ví dụ: `compose('â', U+0301)` → NFD: `a+◌̂+◌́` → NFC: `ấ` (U+1EA5). ✅

## Các hàm chính

### `compose(base, mark) → Option<char>`
Ghép một ký tự gốc + một combining mark qua `nfc()`. Nếu NFC trả về đúng 1 char → thành công. Mọi tổ hợp đều đi qua **một đường này**.

### `apply_quality(base, mark) → Option<char>`
Áp dấu chất lượng (mũ/trăng/móc) lên nguyên âm gốc. Có guard: nếu NFC trả về đúng base cũ → từ chối (đề phòng `q+◌̂` lọt qua).

### `base_of(ch) → char`
Lấy ký tự ASCII gốc: NFD decompose rồi lấy codepoint đầu tiên. Ngoại lệ duy nhất: `đ` → `d` (stroke của đ không nằm trong NFD canonical decomposition).

### `tone_mark(tone) → Option<char>`
Tone → combining codepoint. Level (ngang) trả về `None`.

## Ngoại lệ duy nhất: đ

Stroke (U+0335) của đ **không canonically composable** với `d` trong Unicode. Nên `apply_quality` xử lý riêng bằng match trực tiếp:

```rust
'd' => Some('đ'),
'D' => Some('Đ'),
```

Đây là **special case duy nhất trong toàn crate**. Tất cả các ký tự khác đều qua NFC.

## Độ phủ Unicode

Unicode có precomposed form cho **toàn bộ 60 tổ hợp vowel×tone** của tiếng Việt:

| Nguyên âm | Sắc | Huyền | Hỏi | Ngã | Nặng |
|---|---|---|---|---|---|
| a | á | à | ả | ã | ạ |
| ă | ắ | ằ | ẳ | ẵ | ặ |
| â | ấ | ầ | ẩ | ẫ | ậ |
| e | é | è | ẻ | ẽ | ẹ |
| ê | ế | ề | ể | ễ | ệ |
| i | í | ì | ỉ | ĩ | ị |
| o | ó | ò | ỏ | õ | ọ |
| ô | ố | ồ | ổ | ỗ | ộ |
| ơ | ớ | ờ | ở | ỡ | ợ |
| u | ú | ù | ủ | ũ | ụ |
| ư | ứ | ừ | ử | ữ | ự |
| y | ý | ỳ | ỷ | ỹ | ỵ |

Cộng với quality vowels (â, ê, ô, ă, ơ, ư) + đ/Đ = **toàn bộ bảng chữ cái tiếng Việt**.

## So sánh Algebra NFD/C vs VOWEL

### NFD/C (glyph.rs hiện tại)

**Ưu điểm:**
- Không thể sai typo Unicode — Unicode database là oracle đã được kiểm chứng
- Tự động xử lý quality + tone trên CÙNG nguyên âm trong một lần NFC
- Đúng ngay với mọi tổ hợp Unicode định nghĩa được, không cần maintain
- Một đường code cho mọi kiểu gõ (Telex, VNI, Smart), sau này có thể add thêm tiếng hmong, thái, lào - miễn có unicode là được (trừ ngôn ngữ ngoài hành tinh)

**Nhược điểm:**
- Đ phá vỡ tính "thuần đại số" — vẫn phải có một branch đặc biệt
- Silent failure — nếu có bug đẩy consonant vào `toned()`, dấu mất không ai biết
- Khó audit bằng mắt — phải hiểu NFD/NFC mới verify được, chạy doctor để debug
- Runtime cost nhỏ nhưng có thật (vài µs, không đáng kể với tần suất gõ phím)

### VOWEL (traditional, kiểu `'a'→'á'`)

**Ưu điểm:**
- Compile-time exhaustiveness — Rust `match` bắt lỗi thiếu case
- Audit được bằng mắt — ai cũng verify được trong 30 giây
- Zero dependency — không cần `unicode_normalization`
- Đ chỉ là một entry bình thường, không có "special case"

**Nhược điểm:**
- Rủi ro typo Unicode — 60 dòng hex, một lỗi nhỏ là sai
- Trùng lặp với Unicode database — DRY violation
- Phải xử lý uppercase riêng (thêm 60 dòng hoặc post-process)

## Đánh giá tổng

| Tiêu chí | Algebra NFD/C | VOWEL |
|---|---|---|
| Tính đúng đắn | Chính xác tuyệt đối | Chính xác nếu không typo |
| Audit được | Khó — phải hiểu NFC | Dễ — nhìn bảng là biết |
| Maintainability | Tự động, không cần update | Phải update nếu Unicode đổi |
| Compile-time safety | Fail runtime âm thầm | Exhaustiveness check |
| Đ special case | Là ngoại lệ duy nhất | Là entry bình thường |
| Dependencies | Cần `unicode_normalization` | Zero dep |
| Code size | ~20 dòng logic | hàng nghàn dòng code |

**Kết luận:** 
- Algebra NFD/C là lựa chọn đúng cho VI-IME vì tuân thủ triết lý NFD của toàn bộ engine và không có rủi ro typo. Cần bổ sung unit test cho toàn bộ 60 tổ hợp vowel×tone và thêm `debug_assert!` để bắt fail sớm.
- NFD/C là thuật toán rất mới so với bảng tra vowel 20 năm tuổi, chưa dc nghiên cứu đầy đủ nên có thể còn sót lỗi, nếu có lỗi hãy giúp chạy doctor, collect log và tạo issue nhé.

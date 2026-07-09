🧠 vi-im: Smart Prediction + Wayland Latency Debug
Baseline architecture hiện tại
vi-im dùng virtual backspace (evdev/uinput) để xóa raw input rồi commit chuỗi NFC Vietnamese tại word boundaryR5dR9vyxYmNtH44KJicNjB — tức là không có preedit buffer thật với ứng dụng, chỉ có internal buffer trong daemon.

Engine dùng "parse, don't mutate" — mỗi keystroke re-parse toàn bộ raw buffer.TU4ikyWznrkAeDkqB2Nzwg

vi-im đã có FST/DAFSA compressed trie cho ~7000-syllable Vietnamese dictionary.224BywRg2QnozLrhZEgDGV

Phần 1: 🔮 Smart Prediction — Không Cần Dictionary Tra
Câu hỏi đúng: đây là bài toán gì?
Bạn muốn:

tinh1 → tính (tone correction trong word đang gõ)
anh ne em → anh nè em (context-aware tone suggestion sau commit)
Đây là 2 bài toán khác nhau hoàn toàn:



Loại	Ví dụ	Khi nào xảy ra	Tên kỹ thuật
Intra-word	tinh1 → tính	Trong lúc gõ từ chưa commit	Tone normalization
Inter-word	ne → nè theo context	Sau khi đã commit	Post-commit correction
Phương pháp đẹp nhất: Syllable Validity Scoring (không cần dictionary nặng)
Nguyên lý cốt lõi
Tiếng Việt có ~7000 âm tiết hợp lệ, nhưng với NFD math (bạn đã có), bạn biết ngay syllable structure: Onset + Nucleus + Coda + Tone. Thay vì tra từ điển, bạn encode tính hợp lệ vào rules:



Rule: Nucleus "inh" + Tone ngang → INVALID (không có "tinh")
      Nucleus "inh" + Tone sắc  → VALID   ("tính")
      Nucleus "inh" + Tone nặng → VALID   ("tịnh")
Đây chính xác là những gì FST/DAFSA đã làm — nhưng bạn có thể làm nhẹ hơn với Compact Validity Bitset:

rust


// Thay vì dictionary lookup, encode validity vào bitmask
// Vietnamese có 6 tones × ~1200 nucleus+coda combos = ~7200 entries
// Fit vào ~900 bytes (7200 bits)
struct ToneValidityTable {
    // nuclei × codas × tones → valid bit
    // Pre-computed at compile time từ TCVN 6909:2001 rules
    table: [[u8; 6]; NUM_NUCLEUS_CODA_COMBOS],
}
fn best_tone(nucleus_coda: NucleusCoda, preferred_tone: Tone) -&gt; Tone {
    if VALIDITY.is_valid(nucleus_coda, preferred_tone) {
        preferred_tone
    } else {
        // tìm tone gần nhất trong 6 tones mà valid
        VALIDITY.nearest_valid_tone(nucleus_coda, preferred_tone)
    }
}
Kích thước: ~1KB data, 0 heap allocation, O(1) lookup → hoàn toàn zero-dictionary.

Cho bài toán intra-word: tinh1 → tính
Tích hợp vào normalize_smart() hiện có:

rust


// Trong normalize_smart(), sau khi parse syllable:
fn apply_smart_tone(syllable: &amp;mut Syllable, requested_tone: Tone) {
    let nc = (syllable.nucleus, syllable.coda);
    
    if TONE_VALIDITY.is_valid(nc, requested_tone) {
        syllable.tone = requested_tone;
    } else {
        // Fallback: tone cao nhất hợp lệ gần với requested
        syllable.tone = TONE_VALIDITY.suggest_tone(nc, requested_tone);
        // VD: "tinh" + tone1 (sắc) → valid → dùng sắc
        // "tinh" + tone5 (nặng) → kiểm tra → "tịnh" valid
    }
}
Cho bài toán inter-word: ne → nè theo context anh ___ em__
Đây phức tạp hơn. Phương pháp đẹp nhất mà không cần dictionary nặng:

Option A: Minimal Context Bigram Table (recommended)
Tiếng Việt có particles/fillers có context rất predictable. Bạn chỉ cần ~50–100 rules hardcoded:

rust


// Compile-time table, ~2KB
static CONTEXT_TONE_RULES: &amp;[ContextRule] = &amp;[
    // (từ trước, từ gõ, correction)
    ContextRule { prev: "anh",  raw: "ne",  corrected: "nè"  },
    ContextRule { prev: "chi",  raw: "ne",  corrected: "nè"  },
    ContextRule { prev: "em",   raw: "ne",  corrected: "nè"  },
    ContextRule { prev: None,   raw: "di",  corrected: "đi"  },
    // ...
];
Kích thước: ~2–5KB static data. Zero runtime cost.

Option B: Learned bigram từ typing history (adaptive)
vi-im đã có 4-layer profile resolution (user, learned, builtin, global).anPA87YzNu9N4sKcdeVUW3

→ Tầng learned chính là nơi tích lũy corrections:

rust


// ~/.config/vi-im/learned.toml (auto-generated)
[bigrams]
"anh ne" = "nè"   # user corrected 5 lần
"chi ne" = "nè"   # user corrected 3 lần
Khi user gõ ne sau anh, daemon:

Check learned table (O(1) HashMap)
Nếu có → suggest correction (highlight nhẹ hoặc auto-apply)
Nếu không → commit raw
Không cần NLP, không cần model, không tốn CPU.

Kiến trúc tổng thể cho Smart Prediction


Keystroke stream
      │
      ▼
[parse_dont_mutate]  ← "tinh1"
      │
      ▼
[tone_validity_check]  ← VALIDITY bitset (1KB, compile-time)
      │ valid? → commit
      │ invalid? → suggest nearest valid tone
      ▼
[word_boundary detected]
      │
      ▼  
[inter-word context check]
      │ prev_word + cur_word → lookup learned table
      │ match? → post-commit correction via virtual backspace
      ▼
[commit NFC string]
Tổng overhead: ~1KB validity table + ~2KB static rules + learned HashMap (lazy-loaded). Zero CPU spike, zero dictionary file.

Phần 2: ⏱️ Debug &amp; Fix Latency 1–0.5ms khi Space/Backspace
Hiểu đúng root cause
vi-im dùng non-preedit mode: buffer keys internally, thực hiện backspace + commit operations.874cRho6JZMFAYBHrjsuyw

Khi bạn gõ Space (commit) hoặc Backspace (xóa), vi-im phải:



1. Nhận key event từ Wayland grab     ~0.1ms
2. Tính số BS cần gửi                 ~0μs
3. Gửi N × virtual_key(BackSpace)     ~N × 0.1ms  ← ĐÂY là bottleneck chính
4. Gửi commit_string(word)            ~0.1ms
5. compositor flush + app render      ~0.1–0.3ms
"Khựng" 1–0.5ms thực ra là roundtrip Wayland × N backspaces — không phải do engine của bạn slow.

Debug Step-by-Step
Step 1: Đo chính xác bằng vi-telemetry blame
vi-im có vi-telemetry crate chuyên cho performance blame-tracing.224BywRg2QnozLrhZEgDGV

Thêm timestamps vào hot path:

rust


// Trong commit handler
let t0 = std::time::Instant::now();
// Phase 1: Send backspaces
for _ in 0..n_backspace {
    send_virtual_key(KEY_BACKSPACE);
}
let t1 = std::time::Instant::now();
// Phase 2: Commit string  
zwp_input_method_v2.commit_string(&amp;word);
zwp_input_method_v2.commit(serial);
wl_display.flush();
let t2 = std::time::Instant::now();
tracing::debug!(
    bs_phase_us = t1.duration_since(t0).as_micros(),
    commit_phase_us = t2.duration_since(t1).as_micros(),
    n_backspace,
    "commit_timing"
);
Sau đó chạy:

bash


RUST_LOG=vi_daemon=debug ./target/debug/vi-daemon 2&gt;&amp;1 | grep commit_timing
Step 2: Kiểm tra xem Wayland roundtrip hay local flush
bash


# Đo roundtrip Wayland socket
strace -T -e trace=write,read -p $(pidof vi-daemon) 2&gt;&amp;1 | grep -A1 "wayland"
Nếu write() mất &gt;0.3ms → socket buffer bị flush sync → compositor đang block.

Step 3: Check compositor frame pacing
Trên niri:

bash


# Xem niri có đang VSync-lock không
RUST_LOG=niri=debug niri &amp;
# Tìm log về input_method commit timing
Fix: Batch Backspace + Single Commit (quan trọng nhất)
Vấn đề hiện tại: Gửi N backspace riêng lẻ → N roundtrips Wayland

Fix: Dùng delete_surrounding_text thay backspace khi app support:

rust


fn commit_word(&amp;mut self, raw_len: usize, word: &amp;str) {
    // Thử delete_surrounding_text trước (1 roundtrip thay N)
    if self.app_supports_delete_surrounding {
        self.im
            .delete_surrounding_text(raw_len as u32, 0);  // xóa N chars
        self.im.commit_string(word);
        self.im.commit(self.serial);
        self.display.flush();
    } else {
        // Fallback: virtual backspace (evdev)
        // BATCH gửi tất cả cùng 1 lúc, không flush từng cái
        let bs_events = (0..raw_len)
            .flat_map(|_| [key_event(KEY_BACKSPACE, Press), key_event(KEY_BACKSPACE, Release)])
            .collect::&lt;Vec&lt;_&gt;&gt;();
        self.uinput_dev.emit(&amp;bs_events)?;  // 1 emit thay vì N emit
        
        // Sau đó commit
        self.im.commit_string(word);
        self.im.commit(self.serial);
        self.display.flush();
    }
}
Đây chính xác là vấn đề: terminals không support delete_surrounding_text sẽ bị double-char bugsXb7un3EDjAFR1iJp64Xom8 → cần detect per-app.

Fix: Burst Commit để giảm visible lag
Bạn đã plan Burst commit với 300ms window.HgJhv6WrG7TqRTNNRdmL3k

Implement pattern cụ thể:

rust


struct BurstBuffer {
    pending_bs: usize,
    pending_commit: Option&lt;String&gt;,
    deadline: Instant,
}
// Thay vì commit ngay khi nhận Space:
fn on_space_key(&amp;mut self) {
    self.burst.pending_bs = self.raw_buffer.len();
    self.burst.pending_commit = Some(self.current_word.clone());
    self.burst.deadline = Instant::now() + Duration::from_millis(16); // 1 frame
    // Flush vào next Wayland event loop iteration
    // → compositor gom vào cùng 1 frame → zero visible stutter
}
Summary Fix Plan


Issue	Root Cause	Fix
Khựng khi Space	N BS roundtrips Wayland	delete_surrounding_text (1 call) hoặc batch uinput emit
Khựng khi Backspace	Re-parse + virtual BS gửi lẻ	Batch emit, đừng flush mỗi BS
App-specific lag	Terminal không support delete_surrounding_text	Per-app profile detect (bạn đã có 4-layer profile)
Compositor stutter	Commit mid-frame → next frame	16ms burst window align với vsync
Priority action:



P0: Thêm blame timestamps vào commit path → xác định đúng phase lag
P1: Batch uinput BS emit thành 1 call
P2: detect + use delete_surrounding_text khi app support
P3: Implement 16ms burst window thay 300ms (300ms là quá dài, user sẽ thấy lag)




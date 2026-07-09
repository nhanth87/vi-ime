# Hướng dẫn xây dựng Vietnamese IME (Rust) cho Hyprland / Niri / COSMIC

> Tài liệu này dành cho một Rust dev cấp junior/mid, chưa từng đụng vào Wayland
> protocol hay input method framework. Đọc từ trên xuống, làm theo từng
> milestone, đừng nhảy cóc — mỗi milestone đều có một "định nghĩa hoàn thành"
> (Definition of Done) rõ ràng để tự kiểm tra.

---

## 0. Mục tiêu & phạm vi

Xây một Vietnamese Input Method Engine độc lập (không phụ thuộc fcitx5/ibus),
viết bằng Rust, hỗ trợ gõ Telex/VNI kiểu Unikey, chạy trên 3 Wayland
compositor: **Hyprland, Niri, COSMIC**. Tạm thời **không** hỗ trợ GNOME, KDE,
X11 thuần.

Kiến trúc: implement trực tiếp 2 protocol Wayland chuẩn — `input-method-v2`
(để IME "nói chuyện" với compositor) và lắng nghe `text-input-v3` (protocol
mà app dùng để báo trạng thái ô nhập liệu). Không viết GTK/Qt module riêng —
dựa vào việc GTK ≥3.24 và Qt ≥6.7 đã hỗ trợ `text-input-v3` sẵn.

**Không phải làm ngay từ đầu:** XIM (X11/XWayland), popup theming đẹp,
dictionary/gõ tắt nâng cao, settings UI. Những thứ này để ở milestone cuối
hoặc phase 2.

---

## 1. Kiến thức nền cần đọc trước (bắt buộc, đừng bỏ qua)

Nếu bạn chưa quen các khái niệm dưới đây, dành 1-2 ngày đọc trước khi code,
sẽ tiết kiệm rất nhiều thời gian debug sau này:

1. **Wayland cơ bản**: khái niệm `wl_display`, `wl_registry`, global object,
   request/event, object ID. Đọc: https://wayland-book.com (đọc chương 1-4 là đủ).
2. **`input-method-unstable-v2` protocol**: đọc trực tiếp file XML protocol tại
   `wayland-protocols-misc` hoặc https://wayland.app/protocols/input-method-unstable-v2 —
   đây là spec chính bạn sẽ implement. Chú ý 3 khái niệm:
   `zwp_input_method_v2` (bản thân IME), `zwp_input_method_keyboard_grab_v2`
   (để nhận raw keyboard event), `zwp_input_popup_surface_v2` (để làm popup
   gạch chân/candidate).
3. **`text-input-unstable-v3` protocol**: https://wayland.app/protocols/text-input-unstable-v3 —
   đây là protocol phía app dùng, bạn chỉ cần đọc để hiểu app sẽ gửi gì
   (content_type, cursor_rectangle, surrounding_text) và bạn (IME) phản hồi
   thế nào (preedit_string, commit_string, delete_surrounding_text).
4. **`xdg_popup` vs `xdg_toplevel`**: hiểu tại sao popup PHẢI dùng `xdg_popup`
   neo theo parent surface, KHÔNG được tạo bằng `xdg_toplevel`. (Lý do thực
   tế: trên Niri, một cửa sổ Fcitx5 preedit từng bị tạo sai kiểu này và bị
   compositor coi là cửa sổ tiling bình thường, chiếm nửa màn hình).
5. **Cấu trúc âm tiết tiếng Việt** (nếu bạn không phải người Việt hoặc chưa
   từng cài đặt bộ gõ): phụ âm đầu — âm đệm — nguyên âm chính — âm cuối —
   thanh điệu. Đọc thuật toán đặt dấu thanh kiểu mới/kiểu cũ (oà vs òa) trước
   khi code phần engine, đây là phần dễ sai edge-case nhất.

---

## 2. Cấu trúc Cargo workspace

```
vi-ime/
├── Cargo.toml                  # workspace root
├── crates/
│   ├── vi-engine/               # M1 — logic Telex/VNI thuần, KHÔNG đụng Wayland
│   ├── vi-wayland-im/           # M2-M4 — client input-method-v2 + text-input-v3
│   ├── vi-compositor-ipc/       # M5 — theo dõi active window (hyprctl/niri msg)
│   └── vi-daemon/               # binary chính, ráp mọi thứ lại, entrypoint
└── xtask/                       # script test/dev tiện ích (tuỳ chọn)
```

Nguyên tắc quan trọng: **`vi-engine` không được có bất kỳ dependency nào liên
quan Wayland**. Đây là phần lõi thuần logic, phải test được bằng
`cargo test` mà không cần compositor nào chạy cả. Tách bạch điều này ngay từ
đầu sẽ giúp bạn viết unit test nhanh và không bị block bởi môi trường đồ hoạ.

```toml
# Cargo.toml (workspace root)
[workspace]
resolver = "2"
members = ["crates/vi-engine", "crates/vi-wayland-im", "crates/vi-compositor-ipc", "crates/vi-daemon"]
```

---

## 3. Lộ trình theo milestone

### M0 — Dựng khung, "Hello Wayland"

**Việc cần làm:**
- Tạo workspace như trên.
- Trong `vi-wayland-im`, dùng crate [`wayland-client`](https://docs.rs/wayland-client)
  và [`wayland-protocols-misc`](https://docs.rs/wayland-protocols-misc) (có sẵn
  binding cho `zwp_input_method_v2`) để kết nối tới compositor, in ra danh
  sách global object nó quảng cáo (`wl_registry`).
- Chạy thử trên Niri trước (mở sẵn 1 session Niri lồng nested bằng
  `niri --session` hoặc chạy trong VM/máy thật).

**Definition of Done:** chương trình kết nối thành công, in ra log thấy có
global `zwp_input_method_manager_v2` (nếu không thấy global này nghĩa là
compositor không hỗ trợ, kiểm tra lại version Niri).

```rust
// crates/vi-wayland-im/src/main.rs (khung tối thiểu)
use wayland_client::{Connection, QueueHandle, Dispatch};
use wayland_client::protocol::wl_registry;

struct AppState;

impl Dispatch<wl_registry::WlRegistry, ()> for AppState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global { name, interface, version } = event {
            println!("global: {name} {interface} v{version}");
        }
    }
}

fn main() {
    let conn = Connection::connect_to_env().expect("Không kết nối được tới Wayland compositor");
    let display = conn.display();
    let mut event_queue = conn.new_event_queue();
    let qh = event_queue.handle();
    let _registry = display.get_registry(&qh, ());

    let mut state = AppState;
    event_queue.roundtrip(&mut state).unwrap();
}
```

---

### M1 — Vietnamese engine thuần (không đụng Wayland)

**Việc cần làm:** viết state machine xử lý gõ Telex/VNI. Input là chuỗi phím
gõ vào (dạng ký tự thô), output là chuỗi tiếng Việt đã bỏ dấu đúng.

Thiết kế gợi ý — dùng kiểu dữ liệu tách rõ từng thành phần âm tiết thay vì
xử lý chuỗi string trực tiếp (tránh bug khi backspace/sửa giữa từ):

```rust
// crates/vi-engine/src/lib.rs

#[derive(Debug, Default, Clone)]
pub struct Syllable {
    pub initial: String,   // phụ âm đầu, vd "ngh", "tr"
    pub vowel: String,     // nguyên âm chính (chưa có dấu), vd "uo", "a"
    pub final_: String,    // âm cuối, vd "ng", "t"
    pub tone: Tone,        // thanh điệu
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub enum Tone {
    #[default]
    Level,      // không dấu
    Acute,      // sắc
    Grave,      // huyền
    Hook,       // hỏi
    Tilde,      // ngã
    Dot,        // nặng
}

pub enum InputMethod { Telex, Vni }

pub struct Engine {
    method: InputMethod,
    buffer: Syllable,
    raw_keys: Vec<char>, // lưu lại phím gốc để có thể "undo" khi gõ sai/backspace
}

impl Engine {
    pub fn new(method: InputMethod) -> Self {
        Self { method, buffer: Syllable::default(), raw_keys: Vec::new() }
    }

    /// Nhận 1 ký tự gõ vào, trả về Action mà lớp Wayland cần thực hiện.
    pub fn push_key(&mut self, ch: char) -> Action {
        // TODO: đây là phần lõi cần bạn tự implement quy tắc Telex/VNI.
        // Gợi ý: tách riêng hàm `apply_tone_mark` và `apply_diacritic`
        // rồi viết unit test cho từng quy tắc riêng biệt trước khi ráp lại.
        todo!()
    }

    pub fn backspace(&mut self) -> Action {
        todo!()
    }

    pub fn reset(&mut self) {
        self.buffer = Syllable::default();
        self.raw_keys.clear();
    }
}

/// Action là điều Engine muốn lớp trên (Wayland client) thực hiện.
/// Engine KHÔNG tự gửi gì tới Wayland — tách trách nhiệm rõ ràng.
pub enum Action {
    /// Cập nhật chuỗi đang gõ dở (chưa commit) — dùng cho preedit
    UpdatePreedit(String),
    /// Chốt chuỗi cuối cùng vào văn bản
    Commit(String),
    /// Không đổi gì (phím không liên quan tới tiếng Việt, ví dụ phím mũi tên)
    PassThrough,
}
```

**Việc cần làm tiếp:**
- Viết bảng quy tắc Telex (`aa→â, aw→ă, ow→ơ, w→ư, dd→đ, s/f/r/x/j = 5 dấu
  thanh`) dưới dạng data (không hardcode if-else lan man) — gợi ý dùng
  `phf` (perfect hash function crate) hoặc đơn giản là `match` có comment rõ.
- Viết **ít nhất 50 test case** cho các từ khó: `nghiêng`, `khuỷu`, `huỳnh`,
  `uống`, `chuyến`, kể cả case gõ sai rồi backspace giữa chừng.
- Chưa cần quan tâm tới UTF-8 byte offset ở bước này — dùng `String`/`char`
  bình thường trong `vi-engine`, việc quy đổi sang UTF-8 byte offset cho
  Wayland protocol (`commit_string` cần offset kiểu UTF-8 byte, không phải
  char index) để dành cho M3.

**Definition of Done:** `cargo test -p vi-engine` chạy xanh với ≥50 test
case, không đụng gì tới Wayland.

---

### M2 — Kết nối `zwp_input_method_v2`, log sự kiện

Ráp `vi-wayland-im` lên trên khung M0: implement `Dispatch` cho
`zwp_input_method_manager_v2` và `zwp_input_method_v2`, in log mỗi khi
compositor gọi `activate`, `deactivate`, `done`, `surrounding_text`.

**Chưa gõ được gì cả** ở bước này — mục tiêu chỉ là quan sát đúng luồng sự
kiện khi bạn click vào 1 ô input rồi click ra chỗ khác, để hiểu trước khi
code phần commit.

**Definition of Done:** mở `foot` (terminal) hoặc `gedit`, click vào ô nhập
liệu, thấy log in ra `activate` → `done`; click ra ngoài thấy `deactivate`.

⚠️ **Lưu ý đã biết trên Hyprland**: có báo cáo là implementation
`text-input-v3` từng gửi sự kiện vào một `zwp_text_input_v3` instance chưa
được `enable()`. Nếu log của bạn thấy sự kiện đến trước khi có `enable`, đó
không phải bug code bạn — hãy filter bỏ event tới khi instance đã enable.

---

### M3 — Preedit + Commit cơ bản (chỉ cần chạy đúng trên 1 app test)

Ráp `vi-engine` (M1) vào `vi-wayland-im` (M2):
- Bắt `zwp_input_method_keyboard_grab_v2` để nhận phím gõ thô.
- Mỗi phím gõ → đưa vào `Engine::push_key()` → nhận `Action`.
- `Action::UpdatePreedit(s)` → gọi `set_preedit_string` + `commit()` (theo
  đúng thứ tự request/commit của protocol — đọc kỹ spec phần "double
  buffering", đây là chỗ dễ nhầm nhất với dev mới).
- `Action::Commit(s)` → gọi `commit_string` + `commit()`.

**Chuyển đổi offset:** protocol yêu cầu offset tính bằng **UTF-8 byte**, còn
`Engine` của bạn làm việc với `char`. Viết 1 hàm helper dùng
`str::char_indices()` để quy đổi, và viết test riêng cho hàm này với các ký
tự tiếng Việt có dấu (chiếm 2-3 byte UTF-8) — đây là nguồn bug âm thầm rất
phổ biến (sai offset → xoá nhầm ký tự, hoặc panic do cắt giữa 1 UTF-8
sequence).

**Definition of Done:** gõ được "vieejt nam" ra "việt nam" trong app test
(khuyến nghị test đầu tiên trên `foot` + `kitty`, vì kitty đã hỗ trợ tốt
`text-input-v3`; đừng test đầu tiên trên `alacritty`, hỗ trợ còn hạn chế).

---

### M4 — Popup hiển thị đúng (né bug tiling)

Dùng `zwp_input_popup_surface_v2`, tạo surface theo `wl_surface` +
`xdg_surface` + **`xdg_popup`** (không phải `xdg_toplevel`), neo vị trí theo
`cursor_rectangle` mà app báo qua `text-input-v3`.

⚠️ **Đây là bước dễ dính bug nhất trên Niri.** Nếu bạn (hoặc dependency nào
đó) lỡ tạo popup dưới dạng toplevel window thông thường, Niri (compositor
scrollable-tiling) sẽ coi nó như 1 cửa sổ ứng dụng bình thường và chèn vào
layout — đúng bug từng xảy ra với chính Fcitx5. Checklist tự kiểm tra:
- Surface có `role` là `xdg_popup`, không phải `xdg_toplevel`. ✅
- Có set `parent` đúng surface đang focus. ✅
- Vị trí lấy từ `cursor_rectangle` (x, y, width, height) app gửi qua
  `text-input-v3`, không hardcode toạ độ. ✅

**Definition of Done:** gõ trong app test, thấy popup gạch chân/candidate
hiện đúng ngay dưới con trỏ, không bị coi là cửa sổ riêng trong layout.

---

### M5 — Theo dõi active window để bật/tắt theo app (per-compositor IPC)

Tạo crate `vi-compositor-ipc` với 1 trait chung, mỗi compositor implement
riêng:

```rust
// crates/vi-compositor-ipc/src/lib.rs
pub trait ActiveWindowWatcher {
    /// Trả về app_id/class của cửa sổ đang focus, None nếu không xác định được
    fn current_app_id(&mut self) -> Option<String>;
}

pub struct HyprlandWatcher { /* mở socket $HYPRLAND_INSTANCE_SIGNATURE */ }
pub struct NiriWatcher { /* mở socket niri IPC, gọi "Windows" request */ }
pub struct CosmicWatcher { /* TODO: khảo sát API cosmic-comp lúc bắt đầu code phần này,
                                thông tin hiện chưa đủ ổn định để viết sẵn khung */ }
```

- **Hyprland**: đơn giản nhất để bắt đầu — có thể prototype bằng cách gọi
  `hyprctl activewindow -j` (JSON qua CLI) trước, sau đó nâng cấp lên dùng
  socket trực tiếp (`$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket2.sock`)
  để nhận event real-time thay vì polling.
- **Niri**: dùng `niri msg --json windows` để prototype, sau đó chuyển sang
  event-stream mode của niri IPC (`niri msg event-stream`) để nhận real-time
  thay vì polling liên tục — polling tốn CPU và có độ trễ.
- **COSMIC**: khi bắt đầu phần này, việc đầu tiên là tìm hiểu API/protocol
  hiện tại của `cosmic-comp` (kiểm tra repo `pop-os/cosmic-comp` và
  `pop-os/cosmic-protocols` để xem đã có protocol tương đương chưa) — đừng
  copy khung code của Hyprland/Niri vì kiến trúc IPC khác hẳn.

**Definition of Done:** đổi cửa sổ active giữa 2 app (vd terminal ↔ browser),
IME tự bật/tắt tiếng Việt theo cấu hình per-app đã lưu.

---

### M6 — Edge case & app cụ thể

Danh sách việc làm (ưu tiên theo tần suất người dùng gặp phải):

1. **Chromium/Electron**: mặc định không tự dùng `text-input-v3`. Cần tài
   liệu hướng dẫn user set flag `--enable-wayland-ime --wayland-text-input-version=3`,
   hoặc tạo wrapper script tự động thêm flag cho các app phổ biến
   (`google-chrome`, `code`, `discord`...).
2. **LibreOffice**: dùng toolkit VCL riêng, không đi qua GTK/Qt bình thường.
   Đừng cố "sửa" từ phía IME — test kỹ và document rõ giới hạn đã biết thay
   vì cố gắng vá.
3. **Terminal + vim/helix**: nhắc lại — IME của bạn không thể tự fix việc
   này, phụ thuộc terminal emulator có forward `text-input-v3` hay không.
   Document rõ trong README: "khuyến nghị dùng kitty, alacritty còn hạn
   chế" thay vì hứa hẹn hỗ trợ toàn bộ terminal.
4. **XWayland/XIM** (nếu quyết định làm): cần thêm 1 server XIM riêng biệt
   chạy song song, phức tạp hơn nhiều so với phần Wayland thuần — cân nhắc
   để phase 2, không bắt buộc cho MVP.

---

## 4. Danh sách crate cần dùng

| Crate | Việc dùng để làm gì |
|---|---|
| `wayland-client` | Kết nối & dispatch event loop Wayland cơ bản |
| `wayland-protocols-misc` | Binding sẵn cho `input-method-unstable-v2` |
| `wayland-protocols` | Binding cho `text-input-unstable-v3`, `xdg-shell` |
| `serde` + `serde_json` | Parse JSON output từ `hyprctl`/`niri msg` |
| `tokio` (hoặc `calloop` nếu muốn tích hợp cùng event loop của wayland-client) | Async runtime cho IPC socket, tránh block event loop chính |
| `tracing` + `tracing-subscriber` | Logging có cấu trúc, quan trọng vì debug Wayland rất cần log chi tiết |

Ghi chú: `wayland-client` dùng model callback đồng bộ (`Dispatch` trait), nếu
bạn thêm `tokio` cho phần IPC socket thì cần cẩn thận không để 2 event loop
(Wayland event queue và tokio runtime) giẫm chân nhau — khuyến nghị: dùng
`calloop` (crate mà chính wayland-client khuyên dùng) để gộp cả Wayland
event queue lẫn IPC socket vào chung 1 event loop, đơn giản hơn chạy 2
runtime song song.

---

## 5. Checklist các bẫy đã biết (tổng hợp, dán lên tường mà nhìn)

- [ ] Popup dùng `xdg_popup`, **không** dùng `xdg_toplevel` (bug tiling trên Niri).
- [ ] Offset trong `commit_string`/`delete_surrounding_text` tính bằng **UTF-8 byte**, không phải char index.
- [ ] Trên Hyprland: lọc bỏ event gửi tới `zwp_text_input_v3` instance chưa `enable()`.
- [ ] Trên Niri: test riêng Chromium — có báo cáo double-input với `text-input-v3`.
- [ ] Đừng tự tin "protocol chuẩn thì chắc chạy" — GTK4 popup từng không hiện ra với Fcitx5 trên Niri dù dùng đúng protocol; luôn test thật trên cả 3 compositor trước khi coi 1 tính năng là "xong".
- [ ] Terminal support phụ thuộc terminal emulator, không phải thứ IME sửa được — document rõ giới hạn thay vì cố fix.
- [ ] `vi-engine` phải test được độc lập, không import gì từ `vi-wayland-im`.

---

## 6. Debug & test trên máy thật

- Chạy nested session để dev an toàn, không phá session chính:
  Niri: `niri --session` trong 1 cửa sổ winit riêng nếu đang chạy sẵn 1 DE khác;
  Hyprland: tương tự có chế độ chạy nested qua Xephyr/winit backend.
- Bật log giao thức thô để đối chiếu khi nghi ngờ compositor gửi sai:
  `WAYLAND_DEBUG=1 ./target/debug/vi-daemon 2>&1 | tee wayland-debug.log`
- Khi 1 app cụ thể không nhận preedit, luôn kiểm tra biến môi trường trước
  khi nghi code sai: `GTK_IM_MODULE`, `QT_IM_MODULE`, `QT_IM_MODULES`,
  `XMODIFIERS` — set sai các biến này (hoặc set thừa, override lẫn nhau) là
  nguyên nhân phổ biến nhất khiến người mới tưởng nhầm là bug trong IME.

---

## 7. Tài liệu tham khảo

- Spec protocol: https://wayland.app/protocols/input-method-unstable-v2 và
  https://wayland.app/protocols/text-input-unstable-v3
- Wayland book (nền tảng chung): https://wayland-book.com
- Đọc source Fcitx5 phần Wayland frontend để đối chiếu cách họ implement
  (không copy license GPL vào code bạn, chỉ đọc để hiểu logic):
  `src/frontend/waylandim/` trong repo `fcitx/fcitx5`.
- Niri IPC docs: xem `niri msg --help` và wiki chính thức của niri-wm.
- Hyprland IPC docs: wiki chính thức phần "IPC".

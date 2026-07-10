// Probe: reproduce vi-ime's virtual-keyboard typing mechanisms against a
// focused GTK window (zenity --entry). Modes:
//   pertap  — keymap swap per sync op, keycodes reused (EvdevTyper today)
//   sleep   — pertap + 30ms sleep after each keymap upload
//   word    — ONE keymap for the whole run, uploaded once (static)
// Types the sync sequence for "việt" (v,i,e, BS+ê, BS+ệ, t) then Return.

use std::io::Write;
use std::os::fd::{AsFd, FromRawFd, OwnedFd};
use std::time::Instant;

use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::wl_registry;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1,
    zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
};

struct St;
macro_rules! stub {
    ($iface:ty, $data:ty) => {
        impl Dispatch<$iface, $data> for St {
            fn event(_: &mut Self, _: &$iface, _: <$iface as Proxy>::Event, _: &$data,
                     _: &Connection, _: &QueueHandle<Self>) {}
        }
    };
}
stub!(wl_registry::WlRegistry, GlobalListContents);
stub!(WlSeat, ());
stub!(ZwpVirtualKeyboardManagerV1, ());
stub!(ZwpVirtualKeyboardV1, ());

fn build_keymap(chars: &[(char, u32)]) -> String {
    let mut codes = String::new();
    let mut syms = String::new();
    for (ch, evdev) in chars {
        codes.push_str(&format!("<K{evdev}> = {};\n", evdev + 8));
        let sym = match *ch {
            '\u{0008}' => "BackSpace".to_string(),
            '\r' => "Return".to_string(),
            c => format!("U{:04X}", c as u32),
        };
        syms.push_str(&format!("key <K{evdev}> {{ [ {sym} ] }};\n"));
    }
    format!(
        "xkb_keymap {{\n\
         xkb_keycodes \"vi\" {{ minimum = 8; maximum = 255;\n{codes}}};\n\
         xkb_types \"vi\" {{ include \"complete\" }};\n\
         xkb_compatibility \"vi\" {{ include \"complete\" }};\n\
         xkb_symbols \"vi\" {{\n{syms}}};\n\
         }};\n"
    )
}

fn memfd_keymap(text: &str) -> (OwnedFd, u32) {
    let name = std::ffi::CString::new("probe-keymap").unwrap();
    let raw = unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC) };
    let mut f = unsafe { std::fs::File::from_raw_fd(raw) };
    f.write_all(text.as_bytes()).unwrap();
    f.write_all(&[0]).unwrap();
    (f.into(), text.len() as u32 + 1)
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "pertap".into());
    let conn = Connection::connect_to_env().expect("wayland connect");
    let (globals, mut queue) = registry_queue_init::<St>(&conn).expect("registry");
    let qh = queue.handle();
    let seat: WlSeat = globals.bind(&qh, 1..=9, ()).expect("seat");
    let mgr: ZwpVirtualKeyboardManagerV1 = globals.bind(&qh, 1..=1, ()).expect("vk mgr");
    let vk = mgr.create_virtual_keyboard(&seat, &qh, ());
    queue.roundtrip(&mut St).unwrap();
    let start = Instant::now();

    // sync ops: (backspaces, suffix) — selectable variants
    let variant = std::env::args().nth(3).unwrap_or_default();
    let ops: &[(usize, &str)] = match variant.as_str() {
        "nobs" => &[(0, "v"), (0, "ê"), (0, "t")],          // toned char, no BS
        "bsascii" => &[(0, "v"), (0, "e"), (1, "x"), (0, "t")], // BS + ASCII
        "twochar" => &[(0, "j"), (0, "xy"), (0, "k")],       // 2 ASCII in one burst
        "bsonly" => &[(0, "a"), (0, "b"), (1, "")],          // bare BS op
        _ => &[(0, "v"), (0, "i"), (0, "e"), (1, "ê"), (1, "ệ"), (0, "t")],
    };

    // monotonic timestamps exactly like the daemon: +2ms per tap
    let mut clock = 0u32;
    let mut t = move || { let now = start.elapsed().as_millis() as u32; if now > clock { clock = now; } clock += 2; clock };
    let tap = |vk: &ZwpVirtualKeyboardV1, t0: u32, code: u32| {
        vk.key(t0, code, 1);
        vk.key(t0 + 1, code, 0);
    };

    let no_return = std::env::args().nth(2).as_deref() == Some("nr");
    let paced = mode == "paced";
    match mode.as_str() {
        "clean" => {
            // BackSpace × 4 (undo one typed "việt"), nothing else.
            let (fd, size) = memfd_keymap(&build_keymap(&[('\u{0008}', 2)]));
            vk.keymap(1, fd.as_fd(), size);
            queue.roundtrip(&mut St).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(50));
            for _ in 0..4 {
                tap(&vk, t(), 2);
                queue.roundtrip(&mut St).unwrap();
                std::thread::sleep(std::time::Duration::from_millis(60));
            }
            eprintln!("probe done mode=clean");
            return;
        }
        "word" => {
            // one static keymap: BS, Return, and every char of every suffix
            let mut assigned: Vec<(char, u32)> = vec![('\u{0008}', 2), ('\r', 3)];
            for (_, s) in ops {
                for ch in s.chars() {
                    if !assigned.iter().any(|(c, _)| *c == ch) {
                        assigned.push((ch, 2 + assigned.len() as u32));
                    }
                }
            }
            let (fd, size) = memfd_keymap(&build_keymap(&assigned));
            vk.keymap(1, fd.as_fd(), size);
            queue.roundtrip(&mut St).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(50));
            for (bs, s) in ops {
                for _ in 0..*bs {
                    tap(&vk, t(), 2);
                    if paced {
                        queue.roundtrip(&mut St).unwrap();
                        std::thread::sleep(std::time::Duration::from_millis(15));
                    }
                }
                for ch in s.chars() {
                    let code = assigned.iter().find(|(c, _)| *c == ch).unwrap().1;
                    tap(&vk, t(), code);
                    if paced {
                        queue.roundtrip(&mut St).unwrap();
                        std::thread::sleep(std::time::Duration::from_millis(15));
                    }
                }
                queue.roundtrip(&mut St).unwrap();
                std::thread::sleep(std::time::Duration::from_millis(120));
            }
            if !no_return {
                tap(&vk, t(), 3); // Return
            }
            queue.roundtrip(&mut St).unwrap();
        }
        m => {
            let sleep_ms: u64 = match m { "sleep" => 30, _ => 0 };
            for (bs, s) in ops {
                // per-sync keymap: BS at K2, suffix chars at K3.. (EvdevTyper)
                let mut assigned: Vec<(char, u32)> = vec![('\u{0008}', 2)];
                for ch in s.chars() {
                    if !assigned.iter().any(|(c, _)| *c == ch) {
                        assigned.push((ch, 2 + assigned.len() as u32));
                    }
                }
                let (fd, size) = memfd_keymap(&build_keymap(&assigned));
                vk.keymap(1, fd.as_fd(), size);
                if sleep_ms > 0 {
                    queue.roundtrip(&mut St).unwrap();
                    std::thread::sleep(std::time::Duration::from_millis(sleep_ms));
                }
                for _ in 0..*bs {
                    tap(&vk, t(), 2);
                    if paced {
                        queue.roundtrip(&mut St).unwrap();
                        std::thread::sleep(std::time::Duration::from_millis(15));
                    }
                }
                for ch in s.chars() {
                    let code = assigned.iter().find(|(c, _)| *c == ch).unwrap().1;
                    tap(&vk, t(), code);
                    if paced {
                        queue.roundtrip(&mut St).unwrap();
                        std::thread::sleep(std::time::Duration::from_millis(15));
                    }
                }
                queue.roundtrip(&mut St).unwrap();
                std::thread::sleep(std::time::Duration::from_millis(120));
            }
            if !no_return {
                // Return via its own keymap swap (same mechanism)
                let (fd, size) = memfd_keymap(&build_keymap(&[('\r', 2)]));
                vk.keymap(1, fd.as_fd(), size);
                if sleep_ms > 0 {
                    queue.roundtrip(&mut St).unwrap();
                    std::thread::sleep(std::time::Duration::from_millis(sleep_ms));
                }
                tap(&vk, t(), 2);
            }
            queue.roundtrip(&mut St).unwrap();
        }
    }
    eprintln!("probe done mode={mode}");
}

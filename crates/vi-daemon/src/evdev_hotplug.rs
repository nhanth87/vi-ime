// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Hot-plug keyboard grabbing for the evdev fallback, split from
//! evdev_mode.rs (R4).

use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};

use evdev::Device;
use tracing::info;

use super::{try_grab, Grabbed};

/// Keyboards that appear WHILE the fallback is engaged (USB/Bluetooth
/// keyboard reconnect, KVM switch) were never grabbed: their keys went
/// straight to the app — no Vietnamese until the next focus change (found
/// 2026-10-24 by a field test whose uinput keyboard appeared after the
/// grab). Watch /dev/input and hand every new keyboard to `spawn`. Blocking
/// poll with the readers' 200ms cadence — zero CPU while idle (R15).
pub(super) fn watch_hotplug(stop: &AtomicBool, dead: &AtomicBool, spawn: &dyn Fn(Grabbed)) {
    use inotify::{Inotify, WatchMask};
    let Ok(mut ino) = Inotify::init() else { return };
    if ino.watches().add("/dev/input", WatchMask::CREATE | WatchMask::ATTRIB).is_err() {
        return;
    }
    let fd = ino.as_raw_fd();
    let mut buf = [0u8; 1024];
    let mut seen: std::collections::HashSet<std::path::PathBuf> =
        evdev::enumerate().map(|(p, _)| p).collect();
    while !stop.load(Ordering::Relaxed) && !dead.load(Ordering::Relaxed) {
        let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        if unsafe { libc::poll(&mut pfd, 1, 200) } <= 0 {
            continue;
        }
        let Ok(events) = ino.read_events(&mut buf) else { continue };
        let names: Vec<String> = events
            .filter_map(|ev| ev.name.map(|n| n.to_string_lossy().into_owned()))
            .filter(|n| n.starts_with("event"))
            .collect();
        for name in names {
            let path = std::path::Path::new("/dev/input").join(&name);
            if seen.contains(&path) {
                continue;
            }
            // udev applies the `input` group a moment after the node appears.
            let dev = (0..10).find_map(|_| {
                Device::open(&path).ok().or_else(|| {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    None
                })
            });
            let Some(dev) = dev else { continue };
            seen.insert(path.clone());
            if let Some(kbd) = try_grab(&path, dev, stop) {
                info!("evdev: bàn phím mới cắm vào khi đang grab — grab luôn {:?}", path);
                spawn(kbd);
            }
        }
    }
}

// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! Unit tests for privfile.rs (R38): private bytes stay private.

use super::*;
use std::os::unix::fs::PermissionsExt;

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("vi-privtest-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn write_sets_0600_and_dir_0700() {
    let base = scratch("write");
    let f = base.join("sub").join("predict.tsv");
    write(&f, b"test\t1\n").expect("write");
    let mode = |p: &std::path::Path| std::fs::metadata(p).expect("stat").permissions().mode() & 0o777;
    assert_eq!(mode(&f), 0o600);
    assert_eq!(mode(f.parent().unwrap()), 0o700);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn harden_dir_fixes_old_0644_files() {
    let dir = scratch("harden");
    std::fs::create_dir_all(&dir).expect("mkdir");
    let f = dir.join("habit.tsv");
    std::fs::write(&f, b"old").expect("write");
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).expect("chmod");
    harden_dir(&dir);
    let mode = std::fs::metadata(&f).expect("stat").permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let _ = std::fs::remove_dir_all(&dir);
}

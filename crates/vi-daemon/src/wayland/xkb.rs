// SPDX-License-Identifier: GPL-3.0-or-later OR Commercial
// Copyright (c) 2024-2026 vi-im contributors
//! XKB (X Keyboard Extension) handling via libxkbcommon FFI.
//!
//! Provides XKB state management for keycode-to-character conversion
//! and modifier tracking.

#![allow(non_camel_case_types, non_upper_case_globals, dead_code)]

use std::os::raw::{c_char, c_int, c_void};
use std::os::unix::io::{AsRawFd, OwnedFd};

use tracing::{error, info};

// ---------------------------------------------------------------------------
// FFI constants
// ---------------------------------------------------------------------------

pub(crate) const XKB_CONTEXT_NO_FLAGS: c_int = 0;
pub(crate) const XKB_KEYMAP_FORMAT_TEXT_V1: c_int = 1;
pub(crate) const XKB_KEYMAP_COMPILE_NO_FLAGS: c_int = 0;
pub(crate) const XKB_KEY_NoSymbol: u32 = 0x000000;
pub(crate) const XKB_KEY_BackSpace: u32 = 0xff08;
pub(crate) const XKB_KEY_Tab: u32 = 0xff09;
pub(crate) const XKB_KEY_Return: u32 = 0xff0d;
pub(crate) const XKB_KEY_Escape: u32 = 0xff1b;
pub(crate) const XKB_KEY_Delete: u32 = 0xffff;

pub(crate) type xkb_context = c_void;
pub(crate) type xkb_keymap = c_void;
pub(crate) type xkb_state = c_void;

#[link(name = "xkbcommon")]
unsafe extern "C" {
    pub(crate) fn xkb_context_new(flags: c_int) -> *mut xkb_context;
    pub(crate) fn xkb_context_unref(ctx: *mut xkb_context);
    pub(crate) fn xkb_keymap_new_from_string(
        ctx: *mut xkb_context,
        string: *const c_char,
        format: c_int,
        flags: c_int,
    ) -> *mut xkb_keymap;
    pub(crate) fn xkb_keymap_unref(keymap: *mut xkb_keymap);
    pub(crate) fn xkb_state_new(keymap: *mut xkb_keymap) -> *mut xkb_state;
    pub(crate) fn xkb_state_unref(state: *mut xkb_state);
    pub(crate) fn xkb_state_key_get_one_sym(state: *mut xkb_state, key: u32) -> u32;
    pub(crate) fn xkb_state_key_get_utf8(
        state: *mut xkb_state,
        key: u32,
        buffer: *mut c_char,
        size: usize,
    ) -> c_int;
    pub(crate) fn xkb_state_update_mask(
        state: *mut xkb_state,
        depressed_mods: u32,
        latched_mods: u32,
        locked_mods: u32,
        depressed_layout: u32,
        latched_layout: u32,
        locked_layout: u32,
    );
    /// Check if a named modifier is active in the current state.
    /// `name` examples: "Control", "Mod1" (Alt), "Mod4" (Super), "Shift", "Lock".
    /// `type_`: 0 = XKB_STATE_MODS_EFFECTIVE (what the app sees).
    pub(crate) fn xkb_state_mod_name_is_active(
        state: *mut xkb_state,
        name: *const c_char,
        type_: c_int,
    ) -> c_int;
    /// Check if modifier at `idx` (0=Shift,1=Lock,2=Control,3=Mod1,4=Mod2,
    /// 5=Mod3,6=Mod4,7=Mod5) is active. Index-based is immune to keymap
    /// renames — always reliable across custom layouts.
    /// `type_`: 0 = XKB_STATE_MODS_EFFECTIVE.
    pub(crate) fn xkb_state_mod_index_is_active(
        state: *mut xkb_state,
        idx: u32,
        type_: c_int,
    ) -> c_int;
}

// ---------------------------------------------------------------------------
// XkbState
// ---------------------------------------------------------------------------

pub(crate) struct XkbState {
    ctx: *mut xkb_context,
    keymap: *mut xkb_keymap,
    state: *mut xkb_state,
}

unsafe impl Send for XkbState {}

impl XkbState {
    pub(crate) fn new() -> Self {
        let ctx = unsafe { xkb_context_new(XKB_CONTEXT_NO_FLAGS) };
        Self {
            ctx,
            keymap: std::ptr::null_mut(),
            state: std::ptr::null_mut(),
        }
    }

    pub(crate) fn set_keymap(&mut self, fd: OwnedFd, size: u32) {
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                size as usize,
                libc::PROT_READ,
                libc::MAP_PRIVATE,
                fd.as_raw_fd(),
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            error!("Failed to mmap keymap fd");
            return;
        }
        let keymap_slice =
            unsafe { std::slice::from_raw_parts(ptr as *const u8, size as usize) };
        let keymap_str = match std::str::from_utf8(keymap_slice) {
            Ok(s) => s,
            Err(_) => {
                error!("Keymap data is not valid UTF-8");
                unsafe {
                    libc::munmap(ptr, size as usize)
                };
                return;
            }
        };
        let new_keymap = unsafe {
            xkb_keymap_new_from_string(
                self.ctx,
                keymap_str.as_ptr() as *const c_char,
                XKB_KEYMAP_FORMAT_TEXT_V1,
                XKB_KEYMAP_COMPILE_NO_FLAGS,
            )
        };
        if new_keymap.is_null() {
            error!("Failed to compile xkb keymap");
            unsafe {
                libc::munmap(ptr, size as usize)
            };
            return;
        }
        if !self.keymap.is_null() {
            unsafe {
                xkb_keymap_unref(self.keymap)
            };
        }
        if !self.state.is_null() {
            unsafe {
                xkb_state_unref(self.state)
            };
        }
        self.keymap = new_keymap;
        self.state = unsafe { xkb_state_new(new_keymap) };
        unsafe {
            libc::munmap(ptr, size as usize)
        };
        info!("xkb keymap loaded ({size} bytes)");
    }

    pub(crate) fn keycode_to_char(&self, keycode: u32) -> Option<char> {
        if self.state.is_null() {
            return None;
        }
        let keycode = keycode + 8;
        let keysym = unsafe { xkb_state_key_get_one_sym(self.state, keycode) };
        if !is_printable_keysym(keysym) {
            return None;
        }
        let mut buf = [0u8; 8];
        let len = unsafe {
            xkb_state_key_get_utf8(
                self.state,
                keycode,
                buf.as_mut_ptr() as *mut c_char,
                buf.len(),
            )
        };
        if len <= 0 || len as usize > buf.len() {
            return None;
        }
        let s = std::str::from_utf8(&buf[..len as usize]).ok()?;
        s.chars().next()
    }

    pub(crate) fn update_modifiers(
        &mut self,
        mods_depressed: u32,
        mods_latched: u32,
        mods_locked: u32,
        group: u32,
    ) {
        if self.state.is_null() {
            return;
        }
        unsafe {
            xkb_state_update_mask(
                self.state,
                mods_depressed,
                mods_latched,
                mods_locked,
                0,
                0,
                group,
            )
        };
    }

    /// Check if a named modifier is active (e.g. "Control", "Mod1", "Mod4").
    pub(crate) fn is_mod_active(&self, name: &str) -> bool {
        if self.state.is_null() {
            return false;
        }
        let c_name = std::ffi::CString::new(name).unwrap();
        unsafe { xkb_state_mod_name_is_active(self.state, c_name.as_ptr(), 0) != 0 }
    }

    /// True if any "system" modifier (Ctrl, Alt, Super) is held.
    /// Uses index-based check (Control=2, Mod1=3, Mod4=6) which is
    /// immune to keymap renames — always works even with custom layouts.
    /// Excludes Shift(0), Lock(1), Mod2/NumLock(4), Mod3(5), Mod5(7).
    pub(crate) fn is_system_modifier_active(&self) -> bool {
        if self.state.is_null() {
            return false;
        }
        // XKB_STATE_MODS_EFFECTIVE = 0
        unsafe {
            xkb_state_mod_index_is_active(self.state, 2, 0) != 0  // Control
                || xkb_state_mod_index_is_active(self.state, 3, 0) != 0  // Mod1 (Alt)
                || xkb_state_mod_index_is_active(self.state, 6, 0) != 0   // Mod4 (Super)
        }
    }

    /// True if Control (mod index 2) is held.
    pub(crate) fn is_control_active(&self) -> bool {
        if self.state.is_null() { return false; }
        unsafe { xkb_state_mod_index_is_active(self.state, 2, 0) != 0 }
    }

    /// True if Shift (mod index 0) is held.
    pub(crate) fn is_shift_active(&self) -> bool {
        if self.state.is_null() { return false; }
        unsafe { xkb_state_mod_index_is_active(self.state, 0, 0) != 0 }
    }
}

impl Drop for XkbState {
    fn drop(&mut self) {
        if !self.state.is_null() {
            unsafe {
                xkb_state_unref(self.state)
            };
        }
        if !self.keymap.is_null() {
            unsafe {
                xkb_keymap_unref(self.keymap)
            };
        }
        if !self.ctx.is_null() {
            unsafe {
                xkb_context_unref(self.ctx)
            };
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn is_printable_keysym(keysym: u32) -> bool {
    match keysym {
        0x0020..=0x00FF
        | XKB_KEY_BackSpace
        | XKB_KEY_Return
        | XKB_KEY_Tab
        | XKB_KEY_Escape
        | XKB_KEY_Delete => true,
        0x0100..=0x10FFFF => !(0xF0000..=0x10FFFD).contains(&keysym),
        _ => false,
    }
}

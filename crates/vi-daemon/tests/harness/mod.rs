// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! P1: Headless integration harness for catching render-race bugs in CI.
//!
//! Two test variants (both paths from P0):
//! - `with_surrounding_text`: client announces surrounding_text → P0 atomic path
//! - `without_surrounding_text`: client does NOT announce → VietTyper fallback

pub mod fake_app;
pub mod word_matrix;


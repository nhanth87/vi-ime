// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 vi-im contributors
/// Vietnamese tone marks (thanh điệu).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// No tone (không dấu)
    #[default]
    Level,
    /// Sắc (acute)
    Acute,
    /// Huyền (grave)
    Grave,
    /// Hỏi (hook)
    Hook,
    /// Ngã (tilde)
    Tilde,
    /// Nặng (dot under)
    Dot,
}

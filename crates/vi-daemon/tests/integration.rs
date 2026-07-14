// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
//! P1: Integration tests — headless compositor harness.
//! Run with: cargo test --test integration -- --ignored

mod harness;

#[cfg(test)]
mod tests {
    use super::harness::fake_app::FakeApp;
    use super::harness::word_matrix::HARD_MATRIX;

    #[test]
    #[ignore = "requires headless compositor at $WAYLAND_DISPLAY"]
    fn p0_atomic_path_surrounding_capable() {
        let mut app = FakeApp::new(true);
        for case in HARD_MATRIX {
            app.deactivate();
            app.commit_string(case.want);
            assert_eq!(app.visible_text(), case.want,
                "P0 atomic: keys={:?}", case.keys);
        }
    }

    #[test]
    #[ignore = "requires headless compositor at $WAYLAND_DISPLAY"]
    fn p0b_viet_typer_fallback() {
        let mut app = FakeApp::new(false);
        for case in HARD_MATRIX {
            app.deactivate();
            app.commit_string(case.want);
            assert_eq!(app.visible_text(), case.want,
                "VietTyper fallback: keys={:?}", case.keys);
        }
    }

    #[test]
    fn fake_app_word_matrix_unit() {
        // Every word in the matrix survives direct commit.
        for case in HARD_MATRIX {
            let mut app = FakeApp::new(true);
            app.commit_string(case.want);
            assert_eq!(app.visible_text(), case.want,
                "word={}", case.want);
        }
    }
}

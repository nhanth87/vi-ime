// SPDX-License-Identifier: GPL-3.0-only
//! R-macro boundary tests — extracted to keep fast_engine.rs under R4.

use super::*;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn engine_with(macros: &[(&str, &str)]) -> NonPreeditEngine {
        let mut kw = HashMap::new();
        for (k, v) in macros {
            kw.insert((*k).to_string(), (*v).to_string());
        }
        let mut e = NonPreeditEngine::new(InputMethod::Vni, ImeMode::NonPreedit);
        e.set_macros(Arc::new(MacroDict::build(&kw, &HashMap::new())));
        e
    }

    /// R-macro: typed keyword + boundary commits the expansion (both IME
    /// paths funnel through `commit_text`).
    #[test]
    fn keyword_expands_at_word_boundary() {
        let mut e = engine_with(&[("vn", "Việt Nam")]);
        for c in "vn".chars() {
            e.push_key(c);
        }
        match e.push_key(' ') {
            NonPreeditAction::CommitWithBackspace { backspace_count, text } => {
                assert_eq!(backspace_count, 2);
                assert_eq!(text, "Việt Nam");
            }
            a => panic!("expected expansion commit, got {a:?}"),
        }
    }

    #[test]
    fn no_match_commits_normally() {
        let mut e = engine_with(&[("vn", "Việt Nam")]);
        for c in "xyz".chars() {
            e.push_key(c);
        }
        match e.push_key(' ') {
            NonPreeditAction::CommitWithBackspace { text, .. } => {
                assert_eq!(text, "xyz");
            }
            a => panic!("expected normal commit, got {a:?}"),
        }
    }

    #[test]
    fn empty_table_is_passthrough() {
        let mut e = NonPreeditEngine::new(InputMethod::Vni, ImeMode::NonPreedit);
        for c in "vn".chars() {
            e.push_key(c);
        }
        match e.push_key(' ') {
            NonPreeditAction::CommitWithBackspace { backspace_count, text } => {
                assert_eq!(backspace_count, 2);
                assert_eq!(text, "vn");
            }
            a => panic!("expected raw commit, got {a:?}"),
        }
    }

    #[test]
    fn set_macros_same_arc_skips_rebuild() {
        let mut e = engine_with(&[("vn", "Việt Nam")]);
        let same = e.macros.clone();
        e.set_macros(same);
        assert!(e.macros.expand_keyword("vn").is_some());
    }
}

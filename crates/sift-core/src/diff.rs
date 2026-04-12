//! Unified diff rendering and stat computation.

use crate::entry::DiffStats;
use similar::{ChangeTag, TextDiff};

pub fn stats(before: &str, after: &str) -> DiffStats {
    let diff = TextDiff::from_lines(before, after);
    let mut added: u32 = 0;
    let mut removed: u32 = 0;
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Insert => added += 1,
            ChangeTag::Delete => removed += 1,
            ChangeTag::Equal => {}
        }
    }
    DiffStats { added, removed }
}

pub fn unified(before: &str, after: &str, context: usize) -> String {
    TextDiff::from_lines(before, after)
        .unified_diff()
        .context_radius(context)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_counts_added_and_removed_lines() {
        let before = "a\nb\nc\n";
        let after = "a\nB\nc\nd\n";
        let s = stats(before, after);
        assert_eq!(s.added, 2); // "B" and "d"
        assert_eq!(s.removed, 1); // "b"
    }

    #[test]
    fn stats_empty_on_identical_input() {
        let s = stats("same\nstuff\n", "same\nstuff\n");
        assert_eq!(s.added, 0);
        assert_eq!(s.removed, 0);
    }

    #[test]
    fn unified_contains_change_markers() {
        let u = unified("a\nb\n", "a\nc\n", 3);
        assert!(u.contains("-b"));
        assert!(u.contains("+c"));
    }

    #[test]
    fn stats_handles_empty_inputs() {
        // Both empty: no change.
        let s = stats("", "");
        assert_eq!(s.added, 0);
        assert_eq!(s.removed, 0);
        // Empty before, content after: everything added.
        let s = stats("", "one\ntwo\n");
        assert_eq!(s.added, 2);
        assert_eq!(s.removed, 0);
        // Content before, empty after: everything removed.
        let s = stats("one\ntwo\n", "");
        assert_eq!(s.added, 0);
        assert_eq!(s.removed, 2);
    }

    #[test]
    fn stats_handles_input_without_trailing_newline() {
        // Should not panic and should count the change sensibly.
        let s = stats("a", "b");
        assert_eq!(s.added, 1);
        assert_eq!(s.removed, 1);
    }

    #[test]
    fn unified_empty_strings_returns_empty_output() {
        assert_eq!(unified("", "", 3), "");
    }
}

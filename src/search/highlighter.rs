//! Search match highlighting for `hlsearch`.
//!
//! When the `hlsearch` setting is enabled, all matches of the current search
//! pattern are highlighted in reverse video. This module provides the data
//! structures and functions for computing highlight ranges from a compiled
//! regex pattern.

use crate::search::regex_utils::ViRegex;

/// A highlight range within a single line (byte offsets, end exclusive).
///
/// Represents a single search match highlight on a line. Multiple
/// `HighlightRange` values per line are common when the pattern matches
/// several times on the same line.
///
/// # Examples
///
/// ```
/// use rvi::search::highlighter::HighlightRange;
///
/// let range = HighlightRange { start: 0, end: 5 };
/// assert_eq!(range.start, 0);
/// assert_eq!(range.end, 5);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightRange {
    /// Start byte offset within the line (inclusive).
    pub start: usize,
    /// End byte offset within the line (exclusive).
    pub end: usize,
}

/// Find all highlight ranges on a single line.
///
/// Returns an empty `Vec` if no matches are found or if the regex engine
/// encounters a runtime error (errors are silently swallowed during
/// rendering to avoid disrupting display).
///
/// Zero-length matches are excluded since there is nothing to highlight.
///
/// # Examples
///
/// ```
/// use rvi::search::highlighter::find_highlights;
/// use rvi::search::regex_utils::ViRegex;
///
/// let regex = ViRegex::compile("foo").unwrap();
/// let highlights = find_highlights("foo bar foo", &regex);
/// assert_eq!(highlights.len(), 2);
/// assert_eq!(highlights[0].start, 0);
/// assert_eq!(highlights[0].end, 3);
/// assert_eq!(highlights[1].start, 8);
/// assert_eq!(highlights[1].end, 11);
/// ```
pub fn find_highlights(line: &str, regex: &ViRegex) -> Vec<HighlightRange> {
    regex
        .find_all_in(line)
        .unwrap_or_default()
        .into_iter()
        .filter(|(start, end)| start != end)
        .map(|(start, end)| HighlightRange { start, end })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_highlight_range_debug() {
        let range = HighlightRange { start: 0, end: 5 };
        let debug = format!("{:?}", range);
        assert!(debug.contains("HighlightRange"));
    }

    #[test]
    fn test_find_highlights_basic() {
        let regex = ViRegex::compile("hello").unwrap();
        let highlights = find_highlights("hello world hello", &regex);
        assert_eq!(highlights.len(), 2);
        assert_eq!(highlights[0], HighlightRange { start: 0, end: 5 });
        assert_eq!(highlights[1], HighlightRange { start: 12, end: 17 });
    }

    #[test]
    fn test_find_highlights_no_match() {
        let regex = ViRegex::compile("xyz").unwrap();
        let highlights = find_highlights("hello world", &regex);
        assert!(highlights.is_empty());
    }

    #[test]
    fn test_find_highlights_empty_line() {
        let regex = ViRegex::compile("hello").unwrap();
        let highlights = find_highlights("", &regex);
        assert!(highlights.is_empty());
    }

    #[test]
    fn test_find_highlights_zero_length_match_excluded() {
        // Pattern that matches zero-length (e.g., ^) should be excluded
        let regex = ViRegex::compile("^").unwrap();
        let highlights = find_highlights("hello", &regex);
        assert!(highlights.is_empty());
    }

    #[test]
    fn test_find_highlights_single_char() {
        let regex = ViRegex::compile("o").unwrap();
        let highlights = find_highlights("foo", &regex);
        assert_eq!(highlights.len(), 2);
        assert_eq!(highlights[0], HighlightRange { start: 1, end: 2 });
        assert_eq!(highlights[1], HighlightRange { start: 2, end: 3 });
    }

    #[test]
    fn test_find_highlights_unicode() {
        // CJK chars are 3 bytes each
        let regex = ViRegex::compile("\u{4F60}").unwrap();
        let highlights = find_highlights("\u{4F60}\u{597D}\u{4F60}", &regex);
        assert_eq!(highlights.len(), 2);
        assert_eq!(highlights[0], HighlightRange { start: 0, end: 3 });
        assert_eq!(highlights[1], HighlightRange { start: 6, end: 9 });
    }

    #[test]
    fn test_find_highlights_case_insensitive() {
        let regex = ViRegex::compile_with_flags("hello", true).unwrap();
        let highlights = find_highlights("Hello HELLO hello", &regex);
        assert_eq!(highlights.len(), 3);
    }

    #[test]
    fn test_find_highlights_adjacent_matches() {
        let regex = ViRegex::compile("ab").unwrap();
        let highlights = find_highlights("ababab", &regex);
        assert_eq!(highlights.len(), 3);
        assert_eq!(highlights[0], HighlightRange { start: 0, end: 2 });
        assert_eq!(highlights[1], HighlightRange { start: 2, end: 4 });
        assert_eq!(highlights[2], HighlightRange { start: 4, end: 6 });
    }

    #[test]
    fn test_find_highlights_tab_content() {
        let regex = ViRegex::compile("a\tb").unwrap();
        let highlights = find_highlights("a\tb", &regex);
        assert_eq!(highlights.len(), 1);
        assert_eq!(highlights[0], HighlightRange { start: 0, end: 3 });
    }
}

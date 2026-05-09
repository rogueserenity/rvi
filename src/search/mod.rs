//! Search module for vi-style forward and backward pattern search.
//!
//! This module provides:
//! - `SearchDirection`: Forward (`/`) or backward (`?`) search
//! - `SearchState`: Persistent state for last search pattern and direction
//! - `SearchMatch`: A match position with row and byte-offset columns
//! - `find_next()`: Core search function with wrap-around support
//! - `regex_utils`: Vi regex syntax conversion and `ViRegex` wrapper
//! - `substitute`: Substitute command parsing and execution
//! - `highlighter`: Search match highlighting for `hlsearch`

pub mod highlighter;
pub mod regex_utils;
pub mod substitute;

use crate::buffer::{unicode::next_char_boundary, Buffer, Cursor};
use crate::error::SearchError;

pub use regex_utils::ViRegex;
pub use substitute::{SubstituteCommand, SubstituteFlags, SubstituteRange, SubstituteResult};

/// Direction of a search operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchDirection {
    /// Search forward through the buffer (`/` command).
    Forward,
    /// Search backward through the buffer (`?` command).
    Backward,
}

impl SearchDirection {
    /// Return the opposite direction.
    pub fn reversed(self) -> Self {
        match self {
            Self::Forward => Self::Backward,
            Self::Backward => Self::Forward,
        }
    }

    /// Return the prompt character for this direction.
    pub fn prompt(self) -> char {
        match self {
            Self::Forward => '/',
            Self::Backward => '?',
        }
    }
}

/// A single search match within the buffer.
///
/// Positions use byte offsets consistent with the `Cursor` type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchMatch {
    /// Line number (0-based row index).
    pub row: usize,
    /// Start column as a byte offset within the line (inclusive).
    pub col_start: usize,
    /// End column as a byte offset within the line (exclusive).
    pub col_end: usize,
}

/// Persistent search state stored in `EditorState`.
///
/// Tracks the last search pattern, compiled regex, and search direction
/// so that `n` and `N` can repeat the previous search.
#[derive(Debug, Clone, Default)]
pub struct SearchState {
    /// The original vi pattern string (for display and repeat).
    last_pattern: Option<String>,
    /// The compiled regex from the last pattern.
    compiled: Option<ViRegex>,
    /// The direction of the last search.
    direction: Option<SearchDirection>,
    /// When true, search highlights are suppressed until the next search.
    ///
    /// Set by `:nohl` / `:nohlsearch`. Does not change `hlsearch` — just
    /// hides the highlights for the current match. Cleared whenever the
    /// user performs a new `/`, `?`, `n`, `N`, `*`, or `#` search.
    pub suppress_highlights: bool,
}

impl SearchState {
    /// Create an empty search state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a new search pattern and direction.
    ///
    /// Compiles the pattern via `ViRegex::compile`. On success the pattern,
    /// compiled regex, and direction are stored. On failure the previous
    /// state is unchanged.
    ///
    /// # Errors
    ///
    /// Returns `SearchError` if the pattern fails to compile.
    pub fn set_pattern(
        &mut self,
        pattern: &str,
        direction: SearchDirection,
    ) -> Result<(), SearchError> {
        self.set_pattern_with_flags(pattern, direction, false)
    }

    /// Set a new search pattern and direction with optional case-insensitive flag.
    ///
    /// Compiles the pattern via `ViRegex::compile_with_flags`. When
    /// `case_insensitive` is true, the compiled regex will match regardless
    /// of case. On success the pattern, compiled regex, and direction are
    /// stored. On failure the previous state is unchanged.
    ///
    /// # Errors
    ///
    /// Returns `SearchError` if the pattern fails to compile.
    pub fn set_pattern_with_flags(
        &mut self,
        pattern: &str,
        direction: SearchDirection,
        case_insensitive: bool,
    ) -> Result<(), SearchError> {
        let compiled = ViRegex::compile_with_flags(pattern, case_insensitive)?;
        self.last_pattern = Some(pattern.to_string());
        self.compiled = Some(compiled);
        self.direction = Some(direction);
        Ok(())
    }

    /// Set a new search pattern with case, magic, and direction settings.
    ///
    /// When `magic` is false, `.`, `*`, `[`, `~` are treated as literal
    /// unless backslash-escaped, matching POSIX vi `nomagic` behavior.
    ///
    /// # Errors
    ///
    /// Returns `SearchError` if the pattern fails to compile.
    pub fn set_pattern_with_magic(
        &mut self,
        pattern: &str,
        direction: SearchDirection,
        case_insensitive: bool,
        magic: bool,
    ) -> Result<(), SearchError> {
        let compiled = ViRegex::compile_with_magic(pattern, case_insensitive, magic)?;
        self.last_pattern = Some(pattern.to_string());
        self.compiled = Some(compiled);
        self.direction = Some(direction);
        Ok(())
    }

    /// Get the original vi pattern string, if any.
    pub fn last_pattern(&self) -> Option<&str> {
        self.last_pattern.as_deref()
    }

    /// Get the last search direction, if any.
    pub fn direction(&self) -> Option<SearchDirection> {
        self.direction
    }

    /// Update the search direction without recompiling.
    ///
    /// This is used when repeating a search with an empty pattern,
    /// which should reuse the existing compiled regex but update the direction.
    pub fn set_direction(&mut self, direction: SearchDirection) {
        self.direction = Some(direction);
    }

    /// Get the compiled regex, if any.
    pub fn compiled(&self) -> Option<&ViRegex> {
        self.compiled.as_ref()
    }

    /// Clear all search state.
    pub fn clear(&mut self) {
        self.last_pattern = None;
        self.compiled = None;
        self.direction = None;
    }
}

/// Find the next match from the cursor position in the given direction.
///
/// Searches line-by-line through the buffer with wrap-around. Returns the
/// match and a boolean indicating whether the search wrapped past the
/// beginning or end of the buffer.
///
/// For forward search, the starting position is one byte after the cursor
/// column on the current line to avoid re-matching the same position.
/// For backward search, the starting position is the cursor column.
///
/// # Errors
///
/// Returns `SearchError::RegexError` if the regex engine encounters a
/// runtime error during matching.
pub fn find_next(
    buffer: &Buffer,
    cursor: &Cursor,
    regex: &ViRegex,
    direction: SearchDirection,
    wrapscan: bool,
) -> Result<Option<(SearchMatch, bool)>, SearchError> {
    match direction {
        SearchDirection::Forward => find_forward(buffer, cursor.row, cursor.col, regex, wrapscan),
        SearchDirection::Backward => find_backward(buffer, cursor.row, cursor.col, regex, wrapscan),
    }
}

/// Search forward from `(start_row, start_col)` with wrap-around.
///
/// 1. Search the current line from `start_col + 1` onward.
/// 2. Search subsequent lines from the beginning.
/// 3. If no match found, wrap to line 0 and search up to and including
///    the starting line.
fn find_forward(
    buffer: &Buffer,
    start_row: usize,
    start_col: usize,
    regex: &ViRegex,
    wrapscan: bool,
) -> Result<Option<(SearchMatch, bool)>, SearchError> {
    let line_count = buffer.len();

    // Search current line from start_col + 1 onward
    if let Some(line) = buffer.line(start_row) {
        let search_from = next_char_boundary(line, start_col);
        if search_from < line.len() {
            let slice = &line[search_from..];
            if let Some((s, e)) = regex.find_in(slice)? {
                return Ok(Some((
                    SearchMatch {
                        row: start_row,
                        col_start: search_from + s,
                        col_end: search_from + e,
                    },
                    false,
                )));
            }
        }
    }

    // Search lines after current line
    for row in (start_row + 1)..line_count {
        if let Some(m) = find_first_in_line(buffer, row, regex)? {
            return Ok(Some((m, false)));
        }
    }

    if !wrapscan {
        return Ok(None);
    }

    // Wrap around: search from line 0 up to and including start_row
    for row in 0..=start_row {
        if let Some(m) = find_first_in_line(buffer, row, regex)? {
            // If we found a match on the original line at or before start_col,
            // it is a wrap. If on the same line but after start_col, it would
            // have been found above, so this is also a wrap.
            return Ok(Some((m, true)));
        }
    }

    Ok(None)
}

/// Search backward from `(start_row, start_col)` with wrap-around.
///
/// For backward search we need the last match on a line whose start is
/// strictly before the cursor position. Since `fancy_regex` only finds
/// left-to-right, we find all matches and take the last qualifying one.
fn find_backward(
    buffer: &Buffer,
    start_row: usize,
    start_col: usize,
    regex: &ViRegex,
    wrapscan: bool,
) -> Result<Option<(SearchMatch, bool)>, SearchError> {
    let line_count = buffer.len();

    // Search current line for last match before start_col
    if let Some(line) = buffer.line(start_row) {
        if let Some(m) = find_last_before(line, start_row, start_col, regex)? {
            return Ok(Some((m, false)));
        }
    }

    // Search lines before current line (in reverse order)
    for row in (0..start_row).rev() {
        if let Some(m) = find_last_in_line(buffer, row, regex)? {
            return Ok(Some((m, false)));
        }
    }

    if !wrapscan {
        return Ok(None);
    }

    // Wrap around: search from last line down to start_row+1 (full lines),
    // then on start_row find the last match at or after start_col (i.e., matches
    // that are "later" in wrap order but were not already found above).
    for row in ((start_row + 1)..line_count).rev() {
        if let Some(m) = find_last_in_line(buffer, row, regex)? {
            return Ok(Some((m, true)));
        }
    }
    // On the start row, pick the last match at or after start_col.
    if let Some(line) = buffer.line(start_row) {
        if let Some(m) = find_last_from(line, start_row, start_col, regex)? {
            return Ok(Some((m, true)));
        }
    }

    Ok(None)
}

/// Find the first match on an entire line.
fn find_first_in_line(
    buffer: &Buffer,
    row: usize,
    regex: &ViRegex,
) -> Result<Option<SearchMatch>, SearchError> {
    if let Some(line) = buffer.line(row) {
        if let Some((s, e)) = regex.find_in(line)? {
            return Ok(Some(SearchMatch {
                row,
                col_start: s,
                col_end: e,
            }));
        }
    }
    Ok(None)
}

/// Find the last match on an entire line.
fn find_last_in_line(
    buffer: &Buffer,
    row: usize,
    regex: &ViRegex,
) -> Result<Option<SearchMatch>, SearchError> {
    if let Some(line) = buffer.line(row) {
        let matches = regex.find_all_in(line)?;
        if let Some(&(s, e)) = matches.last() {
            return Ok(Some(SearchMatch {
                row,
                col_start: s,
                col_end: e,
            }));
        }
    }
    Ok(None)
}

/// Find the last match on `line` whose start is at or after `col`.
/// Used by backward wrapscan to find matches on the start row that come
/// after the cursor (which are "earlier" in reverse search order).
fn find_last_from(
    line: &str,
    row: usize,
    col: usize,
    regex: &ViRegex,
) -> Result<Option<SearchMatch>, SearchError> {
    let matches = regex.find_all_in(line)?;
    for &(s, e) in matches.iter().rev() {
        if s >= col {
            return Ok(Some(SearchMatch {
                row,
                col_start: s,
                col_end: e,
            }));
        }
    }
    Ok(None)
}

/// Find the last match on `line` whose start is strictly before `col`.
fn find_last_before(
    line: &str,
    row: usize,
    col: usize,
    regex: &ViRegex,
) -> Result<Option<SearchMatch>, SearchError> {
    let matches = regex.find_all_in(line)?;
    for &(s, e) in matches.iter().rev() {
        if s < col {
            return Ok(Some(SearchMatch {
                row,
                col_start: s,
                col_end: e,
            }));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // SearchDirection tests
    // =========================================================================

    #[test]
    fn test_direction_reversed() {
        assert_eq!(
            SearchDirection::Forward.reversed(),
            SearchDirection::Backward
        );
        assert_eq!(
            SearchDirection::Backward.reversed(),
            SearchDirection::Forward
        );
    }

    #[test]
    fn test_direction_prompt() {
        assert_eq!(SearchDirection::Forward.prompt(), '/');
        assert_eq!(SearchDirection::Backward.prompt(), '?');
    }

    // =========================================================================
    // SearchState tests
    // =========================================================================

    #[test]
    fn test_search_state_new() {
        let state = SearchState::new();
        assert!(state.last_pattern().is_none());
        assert!(state.direction().is_none());
        assert!(state.compiled().is_none());
    }

    #[test]
    fn test_search_state_set_pattern() {
        let mut state = SearchState::new();
        state
            .set_pattern("hello", SearchDirection::Forward)
            .unwrap();
        assert_eq!(state.last_pattern(), Some("hello"));
        assert_eq!(state.direction(), Some(SearchDirection::Forward));
        assert!(state.compiled().is_some());
    }

    #[test]
    fn test_search_state_set_pattern_overwrites() {
        let mut state = SearchState::new();
        state
            .set_pattern("first", SearchDirection::Forward)
            .unwrap();
        state
            .set_pattern("second", SearchDirection::Backward)
            .unwrap();
        assert_eq!(state.last_pattern(), Some("second"));
        assert_eq!(state.direction(), Some(SearchDirection::Backward));
    }

    #[test]
    fn test_search_state_set_invalid_pattern() {
        let mut state = SearchState::new();
        state
            .set_pattern("hello", SearchDirection::Forward)
            .unwrap();
        // Try to set invalid pattern
        let result = state.set_pattern(r"\(", SearchDirection::Backward);
        assert!(result.is_err());
        // Previous state should be preserved
        assert_eq!(state.last_pattern(), Some("hello"));
        assert_eq!(state.direction(), Some(SearchDirection::Forward));
    }

    #[test]
    fn test_search_state_clear() {
        let mut state = SearchState::new();
        state
            .set_pattern("hello", SearchDirection::Forward)
            .unwrap();
        state.clear();
        assert!(state.last_pattern().is_none());
        assert!(state.direction().is_none());
        assert!(state.compiled().is_none());
    }

    // =========================================================================
    // SearchState::set_pattern_with_flags tests
    // =========================================================================

    #[test]
    fn test_set_pattern_with_flags_case_sensitive() {
        let mut state = SearchState::new();
        state
            .set_pattern_with_flags("Hello", SearchDirection::Forward, false)
            .unwrap();
        assert_eq!(state.last_pattern(), Some("Hello"));
        assert_eq!(state.direction(), Some(SearchDirection::Forward));

        // Verify the compiled regex is case-sensitive
        let regex = state.compiled().unwrap();
        assert_eq!(regex.find_in("hello world").unwrap(), None);
        assert_eq!(regex.find_in("Hello world").unwrap(), Some((0, 5)));
    }

    #[test]
    fn test_set_pattern_with_flags_case_insensitive() {
        let mut state = SearchState::new();
        state
            .set_pattern_with_flags("Hello", SearchDirection::Backward, true)
            .unwrap();
        assert_eq!(state.last_pattern(), Some("Hello"));
        assert_eq!(state.direction(), Some(SearchDirection::Backward));

        // Verify the compiled regex is case-insensitive
        let regex = state.compiled().unwrap();
        assert_eq!(regex.find_in("hello world").unwrap(), Some((0, 5)));
        assert_eq!(regex.find_in("HELLO world").unwrap(), Some((0, 5)));
    }

    #[test]
    fn test_set_pattern_with_flags_preserves_state_on_error() {
        let mut state = SearchState::new();
        state
            .set_pattern_with_flags("valid", SearchDirection::Forward, false)
            .unwrap();

        // Try to set an invalid pattern
        let result = state.set_pattern_with_flags(r"\(", SearchDirection::Backward, true);
        assert!(result.is_err());

        // Previous state should be preserved
        assert_eq!(state.last_pattern(), Some("valid"));
        assert_eq!(state.direction(), Some(SearchDirection::Forward));
    }

    #[test]
    fn test_set_pattern_with_flags_overwrites_previous() {
        let mut state = SearchState::new();
        state
            .set_pattern_with_flags("first", SearchDirection::Forward, false)
            .unwrap();
        state
            .set_pattern_with_flags("second", SearchDirection::Backward, true)
            .unwrap();
        assert_eq!(state.last_pattern(), Some("second"));
        assert_eq!(state.direction(), Some(SearchDirection::Backward));
    }

    // =========================================================================
    // find_next forward tests
    // =========================================================================

    #[test]
    fn test_find_forward_basic() {
        let buffer = Buffer::from_string("hello world\nfoo bar\nbaz".to_string());
        let cursor = Cursor::new(0, 0);
        let regex = ViRegex::compile("bar").unwrap();

        let result = find_next(&buffer, &cursor, &regex, SearchDirection::Forward, true).unwrap();
        let (m, wrapped) = result.unwrap();
        assert_eq!(m.row, 1);
        assert_eq!(m.col_start, 4);
        assert_eq!(m.col_end, 7);
        assert!(!wrapped);
    }

    #[test]
    fn test_find_forward_same_line() {
        let buffer = Buffer::from_string("hello world hello".to_string());
        let cursor = Cursor::new(0, 0);
        let regex = ViRegex::compile("hello").unwrap();

        let result = find_next(&buffer, &cursor, &regex, SearchDirection::Forward, true).unwrap();
        let (m, wrapped) = result.unwrap();
        // Should find the second "hello" since we start searching from col+1
        assert_eq!(m.row, 0);
        assert_eq!(m.col_start, 12);
        assert_eq!(m.col_end, 17);
        assert!(!wrapped);
    }

    #[test]
    fn test_find_forward_wrap_around() {
        let buffer = Buffer::from_string("hello world\nfoo bar".to_string());
        let cursor = Cursor::new(1, 0);
        let regex = ViRegex::compile("hello").unwrap();

        let result = find_next(&buffer, &cursor, &regex, SearchDirection::Forward, true).unwrap();
        let (m, wrapped) = result.unwrap();
        assert_eq!(m.row, 0);
        assert_eq!(m.col_start, 0);
        assert_eq!(m.col_end, 5);
        assert!(wrapped);
    }

    #[test]
    fn test_find_forward_no_match() {
        let buffer = Buffer::from_string("hello world".to_string());
        let cursor = Cursor::new(0, 0);
        let regex = ViRegex::compile("xyz").unwrap();

        let result = find_next(&buffer, &cursor, &regex, SearchDirection::Forward, true).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_find_forward_empty_buffer() {
        let buffer = Buffer::new();
        let cursor = Cursor::new(0, 0);
        let regex = ViRegex::compile("hello").unwrap();

        let result = find_next(&buffer, &cursor, &regex, SearchDirection::Forward, true).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_find_forward_cursor_at_end_of_line() {
        let buffer = Buffer::from_string("hello\nworld".to_string());
        let cursor = Cursor::new(0, 5); // At end of "hello"
        let regex = ViRegex::compile("world").unwrap();

        let result = find_next(&buffer, &cursor, &regex, SearchDirection::Forward, true).unwrap();
        let (m, _) = result.unwrap();
        assert_eq!(m.row, 1);
        assert_eq!(m.col_start, 0);
    }

    #[test]
    fn test_find_forward_only_match_at_cursor() {
        let buffer = Buffer::from_string("unique".to_string());
        let cursor = Cursor::new(0, 0);
        let regex = ViRegex::compile("unique").unwrap();

        // Searching forward from cursor should wrap and find the same match
        let result = find_next(&buffer, &cursor, &regex, SearchDirection::Forward, true).unwrap();
        let (m, wrapped) = result.unwrap();
        assert_eq!(m.row, 0);
        assert_eq!(m.col_start, 0);
        assert!(wrapped);
    }

    // =========================================================================
    // find_next backward tests
    // =========================================================================

    #[test]
    fn test_find_backward_basic() {
        let buffer = Buffer::from_string("hello world\nfoo bar".to_string());
        let cursor = Cursor::new(1, 7); // End of "foo bar"
        let regex = ViRegex::compile("foo").unwrap();

        let result = find_next(&buffer, &cursor, &regex, SearchDirection::Backward, true).unwrap();
        let (m, wrapped) = result.unwrap();
        assert_eq!(m.row, 1);
        assert_eq!(m.col_start, 0);
        assert_eq!(m.col_end, 3);
        assert!(!wrapped);
    }

    #[test]
    fn test_find_backward_previous_line() {
        let buffer = Buffer::from_string("hello world\nfoo bar".to_string());
        let cursor = Cursor::new(1, 0);
        let regex = ViRegex::compile("hello").unwrap();

        let result = find_next(&buffer, &cursor, &regex, SearchDirection::Backward, true).unwrap();
        let (m, wrapped) = result.unwrap();
        assert_eq!(m.row, 0);
        assert_eq!(m.col_start, 0);
        assert_eq!(m.col_end, 5);
        assert!(!wrapped);
    }

    #[test]
    fn test_find_backward_wrap_around() {
        let buffer = Buffer::from_string("hello world\nfoo bar".to_string());
        let cursor = Cursor::new(0, 0);
        let regex = ViRegex::compile("bar").unwrap();

        let result = find_next(&buffer, &cursor, &regex, SearchDirection::Backward, true).unwrap();
        let (m, wrapped) = result.unwrap();
        assert_eq!(m.row, 1);
        assert_eq!(m.col_start, 4);
        assert_eq!(m.col_end, 7);
        assert!(wrapped);
    }

    #[test]
    fn test_find_backward_no_match() {
        let buffer = Buffer::from_string("hello world".to_string());
        let cursor = Cursor::new(0, 5);
        let regex = ViRegex::compile("xyz").unwrap();

        let result = find_next(&buffer, &cursor, &regex, SearchDirection::Backward, true).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_find_backward_same_line_multiple() {
        let buffer = Buffer::from_string("aa bb aa cc".to_string());
        let cursor = Cursor::new(0, 8); // After second "aa"
        let regex = ViRegex::compile("aa").unwrap();

        let result = find_next(&buffer, &cursor, &regex, SearchDirection::Backward, true).unwrap();
        let (m, _) = result.unwrap();
        // Should find the second "aa" at col 6, since it starts before col 8
        assert_eq!(m.row, 0);
        assert_eq!(m.col_start, 6);
    }

    #[test]
    fn test_find_backward_empty_buffer() {
        let buffer = Buffer::new();
        let cursor = Cursor::new(0, 0);
        let regex = ViRegex::compile("hello").unwrap();

        let result = find_next(&buffer, &cursor, &regex, SearchDirection::Backward, true).unwrap();
        assert!(result.is_none());
    }

    // =========================================================================
    // Unicode search tests
    // =========================================================================

    #[test]
    fn test_search_unicode_content() {
        // CJK characters are 3 bytes each
        let buffer = Buffer::from_string("\u{4F60}\u{597D}\u{4E16}\u{754C}".to_string());
        let cursor = Cursor::new(0, 0);
        let regex = ViRegex::compile("\u{4E16}\u{754C}").unwrap();

        let result = find_next(&buffer, &cursor, &regex, SearchDirection::Forward, true).unwrap();
        let (m, _) = result.unwrap();
        assert_eq!(m.row, 0);
        assert_eq!(m.col_start, 6); // After two 3-byte CJK chars
        assert_eq!(m.col_end, 12);
    }

    #[test]
    fn test_search_after_unicode_cursor() {
        let buffer = Buffer::from_string("\u{4F60}hello".to_string());
        let cursor = Cursor::new(0, 0); // At the CJK char (3 bytes)
        let regex = ViRegex::compile("hello").unwrap();

        let result = find_next(&buffer, &cursor, &regex, SearchDirection::Forward, true).unwrap();
        let (m, _) = result.unwrap();
        assert_eq!(m.row, 0);
        assert_eq!(m.col_start, 3);
    }

    // =========================================================================
    // Buffer boundary tests
    // =========================================================================

    #[test]
    fn test_search_single_line_buffer() {
        let buffer = Buffer::from_string("hello".to_string());
        let cursor = Cursor::new(0, 3);
        let regex = ViRegex::compile("he").unwrap();

        // Forward: should wrap to find "he" at start
        let result = find_next(&buffer, &cursor, &regex, SearchDirection::Forward, true).unwrap();
        let (m, wrapped) = result.unwrap();
        assert_eq!(m.col_start, 0);
        assert!(wrapped);
    }

    #[test]
    fn test_search_last_line() {
        let buffer = Buffer::from_string("aaa\nbbb\nccc".to_string());
        let cursor = Cursor::new(2, 0);
        let regex = ViRegex::compile("ccc").unwrap();

        // Forward from last line: wraps around to find "ccc" on same line
        let result = find_next(&buffer, &cursor, &regex, SearchDirection::Forward, true).unwrap();
        let (m, wrapped) = result.unwrap();
        assert_eq!(m.row, 2);
        assert!(wrapped);
    }

    // =========================================================================
    // next_char_boundary tests
    // =========================================================================

    #[test]
    fn test_next_char_boundary_ascii() {
        assert_eq!(next_char_boundary("hello", 0), 1);
        assert_eq!(next_char_boundary("hello", 4), 5);
        assert_eq!(next_char_boundary("hello", 5), 5); // At end
    }

    #[test]
    fn test_next_char_boundary_multibyte() {
        // CJK char is 3 bytes
        let s = "\u{4F60}x";
        assert_eq!(next_char_boundary(s, 0), 3); // Skip over 3-byte char
        assert_eq!(next_char_boundary(s, 3), 4); // Skip over 'x'
    }
}

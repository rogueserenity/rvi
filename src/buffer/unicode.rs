//! Unicode utilities for grapheme and display width operations.
//!
//! Provides functions for working with Unicode text, including:
//! - Grapheme cluster segmentation
//! - Display width calculations
//! - Byte offset validation
//! - Character classification for vi word motions

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthChar;

/// Default tab stop width (number of columns between tab stops).
///
/// Used as the default value in `Settings::default()` and in tests that
/// do not need a specific tabstop value.
pub const DEFAULT_TAB_WIDTH: usize = 8;

/// Get the grapheme cluster at the given byte offset.
///
/// Returns `None` if the offset is invalid or at the end of the string.
pub fn grapheme_at(s: &str, byte_offset: usize) -> Option<&str> {
    if byte_offset > s.len() {
        return None;
    }

    let (_before, after) = s.split_at(byte_offset);
    after.graphemes(true).next()
}

/// Find the next grapheme boundary after the given byte offset.
///
/// Returns the byte offset of the next grapheme boundary, or the end of the string
/// if there are no more graphemes.
pub fn next_grapheme_boundary(s: &str, byte_offset: usize) -> usize {
    if byte_offset >= s.len() {
        return s.len();
    }

    let (_before, after) = s.split_at(byte_offset);
    if let Some(grapheme) = after.graphemes(true).next() {
        byte_offset + grapheme.len()
    } else {
        s.len()
    }
}

/// Find the previous grapheme boundary before the given byte offset.
///
/// Returns the byte offset of the previous grapheme boundary, or 0 if at the start.
pub fn prev_grapheme_boundary(s: &str, byte_offset: usize) -> usize {
    if byte_offset == 0 {
        return 0;
    }

    let offset = byte_offset.min(s.len());
    let (before, _) = s.split_at(offset);

    // Get the last grapheme's starting position in the 'before' slice.
    // This is O(n) on the length of 'before' rather than the whole string,
    // but more importantly it's correct and simple.
    // For cursor movements near the end of a line, this is much faster than
    // iterating from the start of a potentially long line.
    before
        .grapheme_indices(true)
        .next_back()
        .map(|(start, _)| start)
        .unwrap_or(0)
}

/// Calculate the display width of a string.
///
/// Takes into account wide characters (like CJK), combining characters, and tabs.
/// Tabs expand to the next tab stop (multiples of `tabstop`).
///
/// # Arguments
///
/// * `s` - The string to measure
/// * `tabstop` - Number of columns between tab stops
pub fn display_width(s: &str, tabstop: usize) -> usize {
    display_width_with_tabs(s, 0, tabstop)
}

/// Calculate the display width of a string, starting from a given column.
///
/// This is needed for accurate tab expansion since tab width depends on
/// the current column position.
fn display_width_with_tabs(s: &str, start_col: usize, tabstop: usize) -> usize {
    let mut col = start_col;
    for ch in s.chars() {
        if ch == '\t' {
            col = next_tab_stop(col, tabstop);
        } else {
            col += ch.width().unwrap_or(0);
        }
    }
    col - start_col
}

/// Calculate the display width from the start of the string up to the given byte offset.
///
/// Takes tabs into account, expanding them to the next tab stop.
///
/// # Arguments
///
/// * `s` - The string to measure
/// * `byte_offset` - Byte offset up to which to measure
/// * `tabstop` - Number of columns between tab stops
///
/// # Panics
///
/// Panics if `byte_offset` is not a valid UTF-8 boundary in the string.
pub fn display_width_up_to(s: &str, byte_offset: usize, tabstop: usize) -> usize {
    if byte_offset > s.len() {
        return display_width(s, tabstop);
    }

    let (prefix, _) = s.split_at(byte_offset);
    display_width_with_tabs(prefix, 0, tabstop)
}

/// Get the column position of the next tab stop.
fn next_tab_stop(col: usize, tabstop: usize) -> usize {
    ((col / tabstop) + 1) * tabstop
}

/// Get the display width of a tab character at the given column.
///
/// # Arguments
///
/// * `col` - Current display column
/// * `tabstop` - Number of columns between tab stops
pub fn tab_width_at_col(col: usize, tabstop: usize) -> usize {
    next_tab_stop(col, tabstop) - col
}

/// Validate that a byte offset is at a valid UTF-8 character boundary.
///
/// Returns `true` if the offset is valid (at a character boundary), `false` otherwise.
pub fn validate_byte_offset(s: &str, byte_offset: usize) -> bool {
    if byte_offset > s.len() {
        return false;
    }

    // Check if we're at a character boundary by trying to split there
    s.is_char_boundary(byte_offset)
}

/// Get the byte offset for the nth grapheme from the start.
///
/// Returns the byte offset of the start of the nth grapheme (0-indexed).
pub fn grapheme_byte_offset(s: &str, grapheme_index: usize) -> Option<usize> {
    for (count, (offset, _)) in s.grapheme_indices(true).enumerate() {
        if count == grapheme_index {
            return Some(offset);
        }
    }
    None
}

/// Count the number of grapheme clusters in a string.
pub fn grapheme_count(s: &str) -> usize {
    s.graphemes(true).count()
}

/// Find the next UTF-8 character boundary at or after `offset`.
///
/// If `offset` is at or beyond the end of the string, returns `s.len()`.
pub fn next_char_boundary(s: &str, offset: usize) -> usize {
    if offset >= s.len() {
        return s.len();
    }
    let mut pos = offset + 1;
    while pos < s.len() && !s.is_char_boundary(pos) {
        pos += 1;
    }
    pos
}

/// Classification of characters for vi word motions.
///
/// Used by word/WORD motions (w, b, e, W, B, E) and text objects (aw, iw, aW, iW)
/// to determine word boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharClass {
    /// Alphanumeric characters and underscore
    Word,
    /// Non-whitespace, non-word characters (operators, punctuation, etc.)
    Punctuation,
    /// Whitespace characters (space, tab, etc.)
    Whitespace,
}

/// Check if a character is a vi word character (alphanumeric or underscore).
pub fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Classify a character for vi word motion purposes.
///
/// When `big_word` is true (WORD motions), all non-whitespace characters
/// are treated as a single class.
pub fn classify_char(c: char, big_word: bool) -> CharClass {
    if c.is_whitespace() {
        CharClass::Whitespace
    } else if big_word || is_word_char(c) {
        CharClass::Word
    } else {
        CharClass::Punctuation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grapheme_at() {
        let s = "hello";
        assert_eq!(grapheme_at(s, 0), Some("h"));
        assert_eq!(grapheme_at(s, 2), Some("l"));
        assert_eq!(grapheme_at(s, 100), None);
    }

    #[test]
    fn test_next_grapheme_boundary() {
        let s = "hello";
        assert_eq!(next_grapheme_boundary(s, 0), 1);
        assert_eq!(next_grapheme_boundary(s, 2), 3);
        assert_eq!(next_grapheme_boundary(s, s.len()), s.len());
    }

    #[test]
    fn test_prev_grapheme_boundary() {
        let s = "hello";
        assert_eq!(prev_grapheme_boundary(s, 0), 0);
        assert_eq!(prev_grapheme_boundary(s, 3), 2);
        assert_eq!(prev_grapheme_boundary(s, s.len()), 4);

        // Test with combining character (a + combining acute = 1 grapheme, 3 bytes)
        let s2 = "a\u{0301}b"; // á followed by b (4 bytes total)
        assert_eq!(prev_grapheme_boundary(s2, 0), 0);
        assert_eq!(prev_grapheme_boundary(s2, 1), 0); // In middle of first grapheme
        assert_eq!(prev_grapheme_boundary(s2, 3), 0); // At end of first grapheme
        assert_eq!(prev_grapheme_boundary(s2, 4), 3); // After 'b', prev is start of 'b'

        // Test with CJK (each char is 3 bytes)
        let s3 = "你好";
        assert_eq!(prev_grapheme_boundary(s3, 3), 0); // After first char
        assert_eq!(prev_grapheme_boundary(s3, 6), 3); // After second char

        // Test beyond string length
        assert_eq!(prev_grapheme_boundary(s, 100), 4);
    }

    #[test]
    fn test_display_width() {
        assert_eq!(display_width("hello", DEFAULT_TAB_WIDTH), 5);
        assert_eq!(display_width("你好", DEFAULT_TAB_WIDTH), 4); // Each CJK char is 2 columns wide
        assert_eq!(display_width("a\u{0301}", DEFAULT_TAB_WIDTH), 1); // Combining character
    }

    #[test]
    fn test_display_width_with_tabs() {
        // Tab at column 0 expands to 8 columns
        assert_eq!(display_width("\t", DEFAULT_TAB_WIDTH), 8);
        // "a" is 1 col, tab expands from col 1 to col 8 (7 spaces)
        assert_eq!(display_width("a\t", DEFAULT_TAB_WIDTH), 8);
        // "ab" is 2 cols, tab expands from col 2 to col 8 (6 spaces)
        assert_eq!(display_width("ab\t", DEFAULT_TAB_WIDTH), 8);
        // "abcdefg" is 7 cols, tab expands from col 7 to col 8 (1 space)
        assert_eq!(display_width("abcdefg\t", DEFAULT_TAB_WIDTH), 8);
        // "abcdefgh" is 8 cols, tab expands from col 8 to col 16 (8 spaces)
        assert_eq!(display_width("abcdefgh\t", DEFAULT_TAB_WIDTH), 16);
        // Multiple tabs
        assert_eq!(display_width("\t\t", DEFAULT_TAB_WIDTH), 16);
        // Tab then text
        assert_eq!(display_width("\thello", DEFAULT_TAB_WIDTH), 13); // 8 + 5
    }

    #[test]
    fn test_display_width_up_to() {
        let s = "hello";
        assert_eq!(display_width_up_to(s, 2, DEFAULT_TAB_WIDTH), 2);
        assert_eq!(display_width_up_to(s, 5, DEFAULT_TAB_WIDTH), 5);
    }

    #[test]
    fn test_display_width_up_to_with_tabs() {
        let s = "a\tb";
        // Up to 'a' (1 byte)
        assert_eq!(display_width_up_to(s, 1, DEFAULT_TAB_WIDTH), 1);
        // Up to after tab (2 bytes: 'a' + '\t')
        assert_eq!(display_width_up_to(s, 2, DEFAULT_TAB_WIDTH), 8);
        // Up to after 'b' (3 bytes)
        assert_eq!(display_width_up_to(s, 3, DEFAULT_TAB_WIDTH), 9);
    }

    #[test]
    fn test_tab_width_at_col() {
        assert_eq!(tab_width_at_col(0, DEFAULT_TAB_WIDTH), 8); // col 0 -> col 8
        assert_eq!(tab_width_at_col(1, DEFAULT_TAB_WIDTH), 7); // col 1 -> col 8
        assert_eq!(tab_width_at_col(7, DEFAULT_TAB_WIDTH), 1); // col 7 -> col 8
        assert_eq!(tab_width_at_col(8, DEFAULT_TAB_WIDTH), 8); // col 8 -> col 16
    }

    #[test]
    fn test_validate_byte_offset() {
        let s = "hello";
        assert!(validate_byte_offset(s, 0));
        assert!(validate_byte_offset(s, 2));
        assert!(validate_byte_offset(s, s.len()));
        assert!(!validate_byte_offset(s, 100));
        // Test with multi-byte character
        let s2 = "a\u{0301}"; // 'a' with combining acute accent (3 bytes total: 'a' is 1, combining char is 2)
        assert_eq!(s2.len(), 3); // Verify expected length
        assert!(validate_byte_offset(s2, 0));
        assert!(validate_byte_offset(s2, 1)); // After 'a'
        assert!(validate_byte_offset(s2, 3)); // At end (1 + 2 = 3 bytes)
        assert!(!validate_byte_offset(s2, 2)); // Middle of multi-byte char
    }

    #[test]
    fn test_grapheme_count() {
        assert_eq!(grapheme_count("hello"), 5);
        assert_eq!(grapheme_count("a\u{0301}"), 1); // One grapheme cluster
        assert_eq!(grapheme_count("café"), 4);
    }

    // Tests for non-default tabstop values
    #[test]
    fn test_display_width_tabstop_4() {
        assert_eq!(display_width("\t", 4), 4);
        assert_eq!(display_width("a\t", 4), 4);
        assert_eq!(display_width("ab\t", 4), 4);
        assert_eq!(display_width("abc\t", 4), 4);
        assert_eq!(display_width("abcd\t", 4), 8);
        assert_eq!(display_width("\t\t", 4), 8);
        assert_eq!(display_width("\thello", 4), 9); // 4 + 5
    }

    #[test]
    fn test_display_width_tabstop_2() {
        assert_eq!(display_width("\t", 2), 2);
        assert_eq!(display_width("a\t", 2), 2);
        assert_eq!(display_width("ab\t", 2), 4);
        assert_eq!(display_width("\thello", 2), 7); // 2 + 5
    }

    #[test]
    fn test_tab_width_at_col_tabstop_4() {
        assert_eq!(tab_width_at_col(0, 4), 4); // col 0 -> col 4
        assert_eq!(tab_width_at_col(1, 4), 3); // col 1 -> col 4
        assert_eq!(tab_width_at_col(3, 4), 1); // col 3 -> col 4
        assert_eq!(tab_width_at_col(4, 4), 4); // col 4 -> col 8
    }

    #[test]
    fn test_display_width_up_to_tabstop_4() {
        let s = "a\tb";
        assert_eq!(display_width_up_to(s, 1, 4), 1); // up to 'a'
        assert_eq!(display_width_up_to(s, 2, 4), 4); // up to after tab
        assert_eq!(display_width_up_to(s, 3, 4), 5); // up to after 'b'
    }

    #[test]
    fn test_is_word_char() {
        assert!(is_word_char('a'));
        assert!(is_word_char('Z'));
        assert!(is_word_char('0'));
        assert!(is_word_char('_'));
        assert!(!is_word_char(' '));
        assert!(!is_word_char('.'));
        assert!(!is_word_char('('));
    }

    #[test]
    fn test_classify_char_small_word() {
        assert_eq!(classify_char('a', false), CharClass::Word);
        assert_eq!(classify_char('_', false), CharClass::Word);
        assert_eq!(classify_char('.', false), CharClass::Punctuation);
        assert_eq!(classify_char(' ', false), CharClass::Whitespace);
    }

    #[test]
    fn test_classify_char_big_word() {
        assert_eq!(classify_char('a', true), CharClass::Word);
        assert_eq!(classify_char('.', true), CharClass::Word);
        assert_eq!(classify_char(' ', true), CharClass::Whitespace);
    }

    // =========================================================================
    // Coverage gap fills
    // =========================================================================

    #[test]
    fn test_grapheme_at_empty_string() {
        assert_eq!(grapheme_at("", 0), None);
    }

    #[test]
    fn test_grapheme_at_at_end() {
        // Offset exactly at end of string returns None (no grapheme there)
        let s = "hi";
        assert_eq!(grapheme_at(s, 2), None);
    }

    #[test]
    fn test_grapheme_at_multibyte() {
        // CJK chars are 3 bytes each
        let s = "你好";
        assert_eq!(grapheme_at(s, 0), Some("你"));
        assert_eq!(grapheme_at(s, 3), Some("好"));
        assert_eq!(grapheme_at(s, 6), None);
    }

    #[test]
    fn test_grapheme_at_combining() {
        // Combining char attaches to base — single grapheme
        let s = "a\u{0301}b";
        assert_eq!(grapheme_at(s, 0), Some("a\u{0301}"));
        assert_eq!(grapheme_at(s, 3), Some("b"));
    }

    #[test]
    fn test_next_grapheme_boundary_multibyte() {
        // CJK: each grapheme is 3 bytes
        let s = "你好世界";
        assert_eq!(next_grapheme_boundary(s, 0), 3);
        assert_eq!(next_grapheme_boundary(s, 3), 6);
        assert_eq!(next_grapheme_boundary(s, 9), 12);
    }

    #[test]
    fn test_next_grapheme_boundary_combining() {
        // a + combining acute: one grapheme spanning 3 bytes
        let s = "a\u{0301}b";
        assert_eq!(next_grapheme_boundary(s, 0), 3);
        assert_eq!(next_grapheme_boundary(s, 3), 4);
    }

    #[test]
    fn test_next_grapheme_boundary_past_end() {
        // Offset beyond length returns string length
        let s = "hi";
        assert_eq!(next_grapheme_boundary(s, 100), s.len());
    }

    #[test]
    fn test_next_grapheme_boundary_empty_string() {
        assert_eq!(next_grapheme_boundary("", 0), 0);
    }

    #[test]
    fn test_prev_grapheme_boundary_empty_string() {
        assert_eq!(prev_grapheme_boundary("", 0), 0);
        assert_eq!(prev_grapheme_boundary("", 5), 0);
    }

    #[test]
    fn test_display_width_up_to_offset_past_end() {
        // When offset > s.len(), returns full display width
        let s = "hello";
        assert_eq!(display_width_up_to(s, 100, DEFAULT_TAB_WIDTH), 5);
    }

    #[test]
    fn test_display_width_up_to_zero_offset() {
        let s = "hello";
        assert_eq!(display_width_up_to(s, 0, DEFAULT_TAB_WIDTH), 0);
    }

    #[test]
    fn test_display_width_empty_string() {
        assert_eq!(display_width("", DEFAULT_TAB_WIDTH), 0);
        assert_eq!(display_width("", 4), 0);
    }

    #[test]
    fn test_display_width_control_chars_zero_width() {
        // Control characters (other than tab) are typically zero-width via .width().unwrap_or(0)
        // Null char has no width, so it's treated as 0
        assert_eq!(display_width("\0", DEFAULT_TAB_WIDTH), 0);
    }

    #[test]
    fn test_validate_byte_offset_empty_string() {
        assert!(validate_byte_offset("", 0));
        assert!(!validate_byte_offset("", 1));
    }

    #[test]
    fn test_validate_byte_offset_cjk() {
        let s = "你好";
        assert!(validate_byte_offset(s, 0));
        assert!(!validate_byte_offset(s, 1)); // mid-char
        assert!(!validate_byte_offset(s, 2)); // mid-char
        assert!(validate_byte_offset(s, 3)); // boundary
        assert!(!validate_byte_offset(s, 4));
        assert!(validate_byte_offset(s, 6)); // end
    }

    #[test]
    fn test_grapheme_byte_offset_basic() {
        let s = "hello";
        assert_eq!(grapheme_byte_offset(s, 0), Some(0));
        assert_eq!(grapheme_byte_offset(s, 1), Some(1));
        assert_eq!(grapheme_byte_offset(s, 4), Some(4));
        assert_eq!(grapheme_byte_offset(s, 5), None);
    }

    #[test]
    fn test_grapheme_byte_offset_multibyte() {
        // CJK: each grapheme starts at 3-byte multiples
        let s = "你好世界";
        assert_eq!(grapheme_byte_offset(s, 0), Some(0));
        assert_eq!(grapheme_byte_offset(s, 1), Some(3));
        assert_eq!(grapheme_byte_offset(s, 2), Some(6));
        assert_eq!(grapheme_byte_offset(s, 3), Some(9));
        assert_eq!(grapheme_byte_offset(s, 4), None);
    }

    #[test]
    fn test_grapheme_byte_offset_combining() {
        // "a\u{0301}b" => 2 graphemes
        let s = "a\u{0301}b";
        assert_eq!(grapheme_byte_offset(s, 0), Some(0));
        assert_eq!(grapheme_byte_offset(s, 1), Some(3)); // "b" starts at byte 3
        assert_eq!(grapheme_byte_offset(s, 2), None);
    }

    #[test]
    fn test_grapheme_byte_offset_empty_string() {
        assert_eq!(grapheme_byte_offset("", 0), None);
    }

    #[test]
    fn test_grapheme_count_empty_string() {
        assert_eq!(grapheme_count(""), 0);
    }

    #[test]
    fn test_grapheme_count_emoji() {
        // Many emoji are single graphemes despite being multi-byte / multi-codepoint.
        // Use a simple BMP emoji to keep the test robust across unicode tables.
        assert_eq!(grapheme_count("☺"), 1);
    }

    #[test]
    fn test_next_char_boundary_basic() {
        let s = "hello";
        assert_eq!(next_char_boundary(s, 0), 1);
        assert_eq!(next_char_boundary(s, 2), 3);
        assert_eq!(next_char_boundary(s, 4), 5);
    }

    #[test]
    fn test_next_char_boundary_multibyte() {
        // CJK chars are 3 bytes
        let s = "你好";
        // Starting at byte 0, next char boundary is 3
        assert_eq!(next_char_boundary(s, 0), 3);
        // Starting mid-character at byte 1, advances to 3
        assert_eq!(next_char_boundary(s, 1), 3);
        // Starting at byte 3 advances to 6
        assert_eq!(next_char_boundary(s, 3), 6);
    }

    #[test]
    fn test_next_char_boundary_at_end() {
        let s = "hi";
        // At/past the end returns s.len()
        assert_eq!(next_char_boundary(s, 2), 2);
        assert_eq!(next_char_boundary(s, 100), 2);
    }

    #[test]
    fn test_next_char_boundary_empty_string() {
        assert_eq!(next_char_boundary("", 0), 0);
        assert_eq!(next_char_boundary("", 5), 0);
    }

    #[test]
    fn test_next_char_boundary_last_char() {
        // Multi-byte char at end: from byte 0, advances past entire char to len
        let s = "你";
        assert_eq!(next_char_boundary(s, 0), 3);
        assert_eq!(next_char_boundary(s, 1), 3);
        assert_eq!(next_char_boundary(s, 2), 3);
    }

    #[test]
    fn test_classify_char_digits_word() {
        assert_eq!(classify_char('0', false), CharClass::Word);
        assert_eq!(classify_char('9', false), CharClass::Word);
    }

    #[test]
    fn test_classify_char_unicode_letter() {
        // Unicode letter is a word char per is_alphanumeric()
        assert_eq!(classify_char('é', false), CharClass::Word);
        assert_eq!(classify_char('你', false), CharClass::Word);
    }

    #[test]
    fn test_classify_char_tab_is_whitespace() {
        assert_eq!(classify_char('\t', false), CharClass::Whitespace);
        assert_eq!(classify_char('\t', true), CharClass::Whitespace);
        assert_eq!(classify_char('\n', false), CharClass::Whitespace);
    }

    #[test]
    fn test_classify_char_big_word_punctuation() {
        // In big-word mode, punctuation classifies as Word
        assert_eq!(classify_char('(', true), CharClass::Word);
        assert_eq!(classify_char('@', true), CharClass::Word);
        assert_eq!(classify_char('/', true), CharClass::Word);
    }

    #[test]
    fn test_is_word_char_unicode() {
        // Unicode alphanumerics qualify as word chars
        assert!(is_word_char('é'));
        assert!(is_word_char('Ω'));
        assert!(is_word_char('你'));
    }

    #[test]
    fn test_charclass_equality_and_clone() {
        // Trait coverage for derived impls
        let c1 = CharClass::Word;
        let c2 = c1;
        assert_eq!(c1, c2);
        assert_ne!(CharClass::Word, CharClass::Punctuation);
        let _debug = format!("{:?}", CharClass::Whitespace);
    }

    #[test]
    fn test_default_tab_width_constant() {
        // Sanity: ensure the constant is the documented value
        assert_eq!(DEFAULT_TAB_WIDTH, 8);
    }
}

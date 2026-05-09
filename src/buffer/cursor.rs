//! Cursor position tracking.
//!
//! The Cursor represents a position in the buffer using byte offsets.
//! Row and column are both stored as byte offsets (like vi).

use super::Buffer;
use crate::buffer::unicode::{
    next_grapheme_boundary, prev_grapheme_boundary, validate_byte_offset,
};

/// Cursor position in the buffer.
///
/// Both `row` and `col` are byte offsets:
/// - `row`: Index into the buffer's lines vector
/// - `col`: Byte offset within the line string
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cursor {
    /// Row index (0-based, byte offset in lines vector)
    pub row: usize,
    /// Column index (0-based, byte offset within the line)
    pub col: usize,
}

impl Cursor {
    /// Create a new cursor at the given position.
    pub fn new(row: usize, col: usize) -> Self {
        Self { row, col }
    }

    /// Check if this cursor position is valid within the given buffer.
    ///
    /// A cursor is valid if:
    /// - The row exists in the buffer
    /// - The column is a valid byte offset in that line (can be at end of line)
    pub fn validate_in_buffer(&self, buffer: &Buffer) -> bool {
        if self.row >= buffer.len() {
            return false;
        }

        if let Some(line) = buffer.line(self.row) {
            // Column can be at the end of the line (equal to line length)
            validate_byte_offset(line, self.col) || self.col == line.len()
        } else {
            false
        }
    }

    /// Clamp the cursor to a valid position within the buffer.
    ///
    /// If the cursor is out of bounds, it will be moved to the nearest valid position:
    /// - Row clamped to last line if too large
    /// - Column clamped to end of line if too large
    /// - Column set to 0 if row is too large
    pub fn clamp_to_buffer(&self, buffer: &Buffer) -> Self {
        let mut row = self.row;
        let mut col = self.col;

        // Clamp row
        if buffer.is_empty() {
            return Cursor::new(0, 0);
        }

        if row >= buffer.len() {
            row = buffer.len() - 1;
            col = 0;
        }

        // Clamp column
        if let Some(line) = buffer.line(row) {
            let max_col = line.len();
            if col > max_col {
                col = max_col;
            }

            // Ensure col is at a valid byte boundary by finding nearest valid position
            col = find_valid_byte_boundary(line, col);
        } else {
            col = 0;
        }

        Cursor::new(row, col)
    }

    /// Move cursor to start of line (column 0).
    pub fn start_of_line(&self) -> Self {
        Cursor::new(self.row, 0)
    }

    /// Move cursor to end of line.
    ///
    /// Requires a buffer to determine the line length.
    pub fn end_of_line(&self, buffer: &Buffer) -> Self {
        if let Some(line) = buffer.line(self.row) {
            Cursor::new(self.row, line.len())
        } else {
            *self
        }
    }

    /// Move cursor left by one grapheme cluster.
    ///
    /// Takes the current line content to perform grapheme-aware movement.
    /// If already at the start of the line (col == 0), position is unchanged.
    ///
    /// # Arguments
    ///
    /// * `line` - The content of the current line
    ///
    /// # Example
    ///
    /// ```ignore
    /// let mut cursor = Cursor::new(0, 5);
    /// cursor.move_left_grapheme("hello");
    /// assert_eq!(cursor.col, 4);
    /// ```
    pub fn move_left_grapheme(&mut self, line: &str) {
        if self.col > 0 {
            self.col = prev_grapheme_boundary(line, self.col);
        }
    }

    /// Move cursor right by one grapheme cluster.
    ///
    /// Takes the current line content and maximum column position for mode-aware movement.
    /// In normal mode, `max_col` should be the position of the last character.
    /// In insert mode, `max_col` should be the line length (allows positioning after last char).
    ///
    /// # Arguments
    ///
    /// * `line` - The content of the current line
    /// * `max_col` - The maximum column position allowed (mode-dependent)
    ///
    /// # Example
    ///
    /// ```ignore
    /// let mut cursor = Cursor::new(0, 0);
    /// cursor.move_right_grapheme("hello", 4); // Normal mode: max is last char position
    /// assert_eq!(cursor.col, 1);
    /// ```
    pub fn move_right_grapheme(&mut self, line: &str, max_col: usize) {
        if self.col < max_col {
            let new_col = next_grapheme_boundary(line, self.col);
            self.col = new_col.min(max_col);
        }
    }

    /// Move cursor right by one grapheme cluster unconditionally.
    ///
    /// This allows moving past the last character (useful for insert mode operations
    /// like 'a' which appends after the cursor). Only stops at end of line.
    ///
    /// # Arguments
    ///
    /// * `line` - The content of the current line
    pub fn move_right_grapheme_unconstrained(&mut self, line: &str) {
        if self.col < line.len() {
            self.col = next_grapheme_boundary(line, self.col);
        }
    }

    /// Clamp cursor column to a valid grapheme boundary within line.
    ///
    /// If the cursor is beyond `max_col`, it is clamped. If the cursor is at
    /// an invalid byte boundary, it is moved to the previous grapheme boundary.
    ///
    /// # Arguments
    ///
    /// * `line` - The content of the current line
    /// * `max_col` - The maximum column position allowed (mode-dependent)
    pub fn clamp_to_line_grapheme(&mut self, line: &str, max_col: usize) {
        if self.col > max_col {
            self.col = max_col;
        }
        // Ensure we're at a valid grapheme boundary
        if !line.is_empty() {
            self.col = find_valid_byte_boundary(line, self.col);
        }
    }
}

/// Find the nearest valid byte boundary at or before the given offset.
///
/// Scans backward from offset until a valid char boundary is found.
fn find_valid_byte_boundary(s: &str, offset: usize) -> usize {
    let offset = offset.min(s.len());
    // Scan backward to find a valid char boundary
    let mut pos = offset;
    while pos > 0 && !s.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cursor_new() {
        let cursor = Cursor::new(5, 10);
        assert_eq!(cursor.row, 5);
        assert_eq!(cursor.col, 10);
    }

    #[test]
    fn test_cursor_validate() {
        let buffer = Buffer::from_string("hello\nworld".to_string());
        assert!(Cursor::new(0, 2).validate_in_buffer(&buffer));
        assert!(Cursor::new(1, 5).validate_in_buffer(&buffer)); // At end of line
        assert!(!Cursor::new(10, 0).validate_in_buffer(&buffer)); // Row out of bounds
        assert!(!Cursor::new(0, 100).validate_in_buffer(&buffer)); // Col out of bounds
    }

    #[test]
    fn test_cursor_clamp() {
        let buffer = Buffer::from_string("hello\nworld".to_string());

        // Valid cursor should remain unchanged
        let cursor = Cursor::new(0, 2);
        assert_eq!(cursor.clamp_to_buffer(&buffer), cursor);

        // Row too large
        let cursor = Cursor::new(10, 5);
        let clamped = cursor.clamp_to_buffer(&buffer);
        assert_eq!(clamped.row, 1);
        assert_eq!(clamped.col, 0);

        // Col too large
        let cursor = Cursor::new(0, 100);
        let clamped = cursor.clamp_to_buffer(&buffer);
        assert_eq!(clamped.row, 0);
        assert_eq!(clamped.col, 5); // Length of "hello"

        // Empty buffer
        let empty = Buffer::new();
        let cursor = Cursor::new(5, 10);
        let clamped = cursor.clamp_to_buffer(&empty);
        assert_eq!(clamped.row, 0);
        assert_eq!(clamped.col, 0);
    }

    #[test]
    fn test_cursor_start_of_line() {
        let cursor = Cursor::new(5, 10);
        assert_eq!(cursor.start_of_line(), Cursor::new(5, 0));
    }

    #[test]
    fn test_cursor_end_of_line() {
        let buffer = Buffer::from_string("hello\nworld".to_string());
        let cursor = Cursor::new(0, 2);
        let end = cursor.end_of_line(&buffer);
        assert_eq!(end.row, 0);
        assert_eq!(end.col, 5);
    }

    #[test]
    fn test_move_left_grapheme_ascii() {
        let line = "hello";
        let mut cursor = Cursor::new(0, 3);
        cursor.move_left_grapheme(line);
        assert_eq!(cursor.col, 2);

        // At start of line - no change
        cursor.col = 0;
        cursor.move_left_grapheme(line);
        assert_eq!(cursor.col, 0);
    }

    #[test]
    fn test_move_left_grapheme_unicode() {
        // "a" with combining acute accent (1 + 2 = 3 bytes for first grapheme)
        let line = "a\u{0301}b";
        let mut cursor = Cursor::new(0, 4); // After 'b'
        cursor.move_left_grapheme(line);
        assert_eq!(cursor.col, 3); // At 'b'

        cursor.move_left_grapheme(line);
        assert_eq!(cursor.col, 0); // At start of combining grapheme
    }

    #[test]
    fn test_move_right_grapheme_ascii() {
        let line = "hello";
        let mut cursor = Cursor::new(0, 0);
        cursor.move_right_grapheme(line, 4); // Normal mode max
        assert_eq!(cursor.col, 1);

        // At max position - no change
        cursor.col = 4;
        cursor.move_right_grapheme(line, 4);
        assert_eq!(cursor.col, 4);
    }

    #[test]
    fn test_move_right_grapheme_unicode() {
        // CJK characters (3 bytes each)
        let line = "你好";
        let mut cursor = Cursor::new(0, 0);
        cursor.move_right_grapheme(line, 6); // Insert mode max
        assert_eq!(cursor.col, 3); // After first CJK char
    }

    #[test]
    fn test_move_right_grapheme_unconstrained() {
        let line = "hello";
        let mut cursor = Cursor::new(0, 4);
        cursor.move_right_grapheme_unconstrained(line);
        assert_eq!(cursor.col, 5); // At end of line

        // At end of line - no change
        cursor.move_right_grapheme_unconstrained(line);
        assert_eq!(cursor.col, 5);
    }

    #[test]
    fn test_clamp_to_line_grapheme() {
        let line = "hello";
        let mut cursor = Cursor::new(0, 10);
        cursor.clamp_to_line_grapheme(line, 4);
        assert_eq!(cursor.col, 4);

        // Within bounds - no change
        cursor.col = 2;
        cursor.clamp_to_line_grapheme(line, 4);
        assert_eq!(cursor.col, 2);
    }

    #[test]
    fn test_clamp_to_line_grapheme_invalid_boundary() {
        // Multi-byte character: 'a' + combining accent (3 bytes total) + 'b'
        let line = "a\u{0301}b";
        let mut cursor = Cursor::new(0, 2); // Invalid boundary (middle of combining char)
        cursor.clamp_to_line_grapheme(line, 4);
        // Moved to previous valid char boundary (after 'a' which is at byte 1)
        assert_eq!(cursor.col, 1);
    }

    #[test]
    fn test_find_valid_byte_boundary() {
        // ASCII - all positions valid
        let ascii = "hello";
        assert_eq!(find_valid_byte_boundary(ascii, 3), 3);
        assert_eq!(find_valid_byte_boundary(ascii, 0), 0);
        assert_eq!(find_valid_byte_boundary(ascii, 10), 5); // Clamped to len

        // Multi-byte: 'a' (1 byte) + combining accent (2 bytes) = 3 bytes, then 'b' (1 byte)
        let multi = "a\u{0301}b";
        assert_eq!(find_valid_byte_boundary(multi, 0), 0); // Valid
        assert_eq!(find_valid_byte_boundary(multi, 1), 1); // Valid (after 'a')
        assert_eq!(find_valid_byte_boundary(multi, 2), 1); // Invalid, move back to 1
        assert_eq!(find_valid_byte_boundary(multi, 3), 3); // Valid (after combining)
        assert_eq!(find_valid_byte_boundary(multi, 4), 4); // Valid (after 'b')
    }
}

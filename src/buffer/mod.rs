//! Text buffer module for managing editor content.
//!
//! This module provides the core data structures for storing and manipulating text:
//! - Buffer: Stores text as a vector of lines
//! - Cursor: Tracks position using byte offsets
//! - Selection: Represents selected text regions
//! - Unicode utilities: Grapheme and display width operations

pub mod cursor;
pub mod operations;
pub mod selection;
pub mod unicode;

pub use cursor::Cursor;
pub use selection::{Selection, SelectionType};

use crate::error::BufferError;
use unicode::validate_byte_offset;

/// Main text buffer storing lines of text.
#[derive(Debug, Clone)]
pub struct Buffer {
    /// Lines of text. Each line is stored as a String (UTF-8 bytes).
    lines: Vec<String>,
}

impl Buffer {
    /// Create a new empty buffer with one empty line.
    pub fn new() -> Self {
        Self {
            lines: vec![String::new()],
        }
    }

    /// Create a buffer from a vector of lines.
    pub fn from_lines(lines: Vec<String>) -> Self {
        // Ensure at least one line exists
        if lines.is_empty() {
            Self::new()
        } else {
            Self { lines }
        }
    }

    /// Create a buffer from a string, splitting on newlines.
    ///
    /// A trailing newline is consumed by `str::lines` and does not produce a
    /// phantom empty last line.  The writer always appends a final newline, so
    /// round-tripping a standard POSIX text file is lossless.
    pub fn from_string(s: String) -> Self {
        if s.is_empty() {
            Self::new()
        } else {
            let lines: Vec<String> = s.lines().map(|line| line.to_string()).collect();
            Self::from_lines(lines)
        }
    }

    /// Get the number of lines in the buffer.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Check if the buffer is empty (has only one empty line).
    pub fn is_empty(&self) -> bool {
        self.lines.len() == 1 && self.lines[0].is_empty()
    }

    /// Get a line by index (0-based).
    pub fn line(&self, row: usize) -> Option<&str> {
        self.lines.get(row).map(|s| s.as_str())
    }

    /// Get a mutable reference to a line by index.
    pub fn line_mut(&mut self, row: usize) -> Option<&mut String> {
        self.lines.get_mut(row)
    }

    /// Insert a new line at the given row index.
    ///
    /// All lines at and after the given index are shifted down.
    pub fn insert_line(&mut self, row: usize, content: String) -> Result<(), BufferError> {
        if row > self.lines.len() {
            return Err(BufferError::RowOutOfBounds {
                row,
                len: self.lines.len(),
            });
        }

        self.lines.insert(row, content);
        Ok(())
    }

    /// Remove a line from the buffer at the given row.
    ///
    /// Returns the removed line content.
    /// If the buffer only has one line, it clears the line instead of removing it.
    pub fn remove_line(&mut self, row: usize) -> Result<String, BufferError> {
        if row >= self.lines.len() {
            return Err(BufferError::RowOutOfBounds {
                row,
                len: self.lines.len(),
            });
        }

        if self.lines.len() == 1 {
            // Only one line - clear it instead of removing
            let content = std::mem::take(&mut self.lines[0]);
            Ok(content)
        } else {
            Ok(self.lines.remove(row))
        }
    }

    /// Insert a character at the cursor position.
    pub fn insert_char(&mut self, cursor: &Cursor, ch: char) -> Result<(), BufferError> {
        if cursor.row >= self.lines.len() {
            return Err(BufferError::RowOutOfBounds {
                row: cursor.row,
                len: self.lines.len(),
            });
        }

        let line = &mut self.lines[cursor.row];

        // Validate byte offset
        if !validate_byte_offset(line, cursor.col) {
            return Err(BufferError::InvalidByteOffset { col: cursor.col });
        }

        line.insert(cursor.col, ch);
        Ok(())
    }

    /// Insert a string at the cursor position.
    pub fn insert_string(&mut self, cursor: &Cursor, s: &str) -> Result<(), BufferError> {
        if cursor.row >= self.lines.len() {
            return Err(BufferError::RowOutOfBounds {
                row: cursor.row,
                len: self.lines.len(),
            });
        }

        let line = &mut self.lines[cursor.row];

        // Validate byte offset
        if !validate_byte_offset(line, cursor.col) {
            return Err(BufferError::InvalidByteOffset { col: cursor.col });
        }

        line.insert_str(cursor.col, s);
        Ok(())
    }

    /// Delete a character at the cursor position (the character before the cursor).
    pub fn delete_char(&mut self, cursor: &Cursor) -> Result<Option<char>, BufferError> {
        if cursor.row >= self.lines.len() {
            return Err(BufferError::RowOutOfBounds {
                row: cursor.row,
                len: self.lines.len(),
            });
        }

        let line = &mut self.lines[cursor.row];

        if cursor.col == 0 {
            // At start of line - join with previous line
            if cursor.row > 0 {
                let current_line = self.lines.remove(cursor.row);
                let prev_line = &mut self.lines[cursor.row - 1];
                prev_line.push_str(&current_line);
                return Ok(None); // Return None since we removed a line, not a char
            }
            return Ok(None);
        }

        // Validate byte offset
        if !validate_byte_offset(line, cursor.col) {
            return Err(BufferError::InvalidByteOffset { col: cursor.col });
        }

        // Delete character before cursor using proper grapheme boundary
        if cursor.col > 0 {
            let byte_idx = cursor.col;
            if byte_idx <= line.len() {
                // Find the start of the previous grapheme cluster
                let prev_boundary = unicode::prev_grapheme_boundary(line, byte_idx);
                let removed_char = line[prev_boundary..byte_idx].chars().next();
                // Remove all bytes of the grapheme
                line.replace_range(prev_boundary..byte_idx, "");
                return Ok(removed_char);
            }
        }

        Ok(None)
    }

    /// Delete text in the range from start to end cursor.
    pub fn delete_range(&mut self, start: &Cursor, end: &Cursor) -> Result<String, BufferError> {
        let (start, end) = if start <= end {
            (*start, *end)
        } else {
            (*end, *start)
        };

        if start.row == end.row {
            // Same line - delete substring
            if start.row >= self.lines.len() {
                return Err(BufferError::RowOutOfBounds {
                    row: start.row,
                    len: self.lines.len(),
                });
            }

            let line = &mut self.lines[start.row];
            if !validate_byte_offset(line, start.col) {
                return Err(BufferError::InvalidByteOffset { col: start.col });
            }
            if !validate_byte_offset(line, end.col) {
                return Err(BufferError::InvalidByteOffset { col: end.col });
            }

            let deleted = line[start.col..end.col].to_string();
            line.replace_range(start.col..end.col, "");
            Ok(deleted)
        } else {
            // Multi-line deletion
            if end.row >= self.lines.len() {
                return Err(BufferError::RowOutOfBounds {
                    row: end.row,
                    len: self.lines.len(),
                });
            }

            // Collect deleted text
            let mut deleted = String::new();

            // First line: from start.col to end (including the newline)
            if start.row < self.lines.len() {
                let first_line = &mut self.lines[start.row];
                if !validate_byte_offset(first_line, start.col) {
                    return Err(BufferError::InvalidByteOffset { col: start.col });
                }
                deleted.push_str(&first_line[start.col..]);
                deleted.push('\n'); // Include newline between first line and rest
                first_line.truncate(start.col);
            }

            // Middle lines: entire lines
            for i in (start.row + 1)..end.row {
                if i < self.lines.len() {
                    deleted.push_str(&self.lines[i]);
                    deleted.push('\n');
                }
            }

            // Last line: from start to end.col
            if end.row < self.lines.len() {
                let last_line = &self.lines[end.row];
                if !validate_byte_offset(last_line, end.col) {
                    return Err(BufferError::InvalidByteOffset { col: end.col });
                }
                deleted.push_str(&last_line[..end.col]);
            }

            // Join first and last line, remove middle lines
            if end.row < self.lines.len() {
                let last_line = self.lines.remove(end.row);
                let remaining = last_line[end.col..].to_string();

                // Remove middle lines in a single O(n) pass
                let mid_end = (end.row).min(self.lines.len());
                if start.row + 1 < mid_end {
                    self.lines.drain((start.row + 1)..mid_end);
                }

                // Append remaining part of last line to first line
                if start.row < self.lines.len() {
                    self.lines[start.row].push_str(&remaining);
                }
            }

            Ok(deleted)
        }
    }

    /// Split the line at the cursor position (insert a newline).
    pub fn split_line(&mut self, cursor: &Cursor) -> Result<(), BufferError> {
        if cursor.row >= self.lines.len() {
            return Err(BufferError::RowOutOfBounds {
                row: cursor.row,
                len: self.lines.len(),
            });
        }

        let line = &mut self.lines[cursor.row];

        if !validate_byte_offset(line, cursor.col) {
            return Err(BufferError::InvalidByteOffset { col: cursor.col });
        }

        let remainder = line[cursor.col..].to_string();
        line.truncate(cursor.col);
        self.lines.insert(cursor.row + 1, remainder);
        Ok(())
    }

    /// Join the line at row with the next line.
    pub fn join_lines(&mut self, row: usize) -> Result<(), BufferError> {
        if row >= self.lines.len() {
            return Err(BufferError::RowOutOfBounds {
                row,
                len: self.lines.len(),
            });
        }

        if row + 1 >= self.lines.len() {
            // Nothing to join
            return Ok(());
        }

        let next_line = self.lines.remove(row + 1);
        self.lines[row].push_str(&next_line);
        Ok(())
    }

    /// Get all lines as a slice of strings.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }
}

impl Default for Buffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_new() {
        let buffer = Buffer::new();
        assert_eq!(buffer.len(), 1);
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_buffer_from_string() {
        let buffer = Buffer::from_string("hello\nworld".to_string());
        assert_eq!(buffer.len(), 2);
        assert_eq!(buffer.line(0), Some("hello"));
        assert_eq!(buffer.line(1), Some("world"));
    }

    #[test]
    fn test_buffer_insert_char() {
        let mut buffer = Buffer::from_string("hello".to_string());
        let cursor = Cursor::new(0, 2);
        buffer.insert_char(&cursor, 'X').unwrap();
        assert_eq!(buffer.line(0), Some("heXllo"));
    }

    #[test]
    fn test_buffer_insert_line() {
        let mut buffer = Buffer::from_string("hello\nworld".to_string());
        assert_eq!(buffer.len(), 2);

        buffer.insert_line(1, "middle".to_string()).unwrap();
        assert_eq!(buffer.len(), 3);
        assert_eq!(buffer.line(0), Some("hello"));
        assert_eq!(buffer.line(1), Some("middle"));
        assert_eq!(buffer.line(2), Some("world"));
    }

    #[test]
    fn test_buffer_insert_line_at_start() {
        let mut buffer = Buffer::from_string("hello".to_string());
        buffer.insert_line(0, "before".to_string()).unwrap();
        assert_eq!(buffer.len(), 2);
        assert_eq!(buffer.line(0), Some("before"));
        assert_eq!(buffer.line(1), Some("hello"));
    }

    #[test]
    fn test_buffer_insert_line_at_end() {
        let mut buffer = Buffer::from_string("hello".to_string());
        buffer.insert_line(1, "after".to_string()).unwrap();
        assert_eq!(buffer.len(), 2);
        assert_eq!(buffer.line(0), Some("hello"));
        assert_eq!(buffer.line(1), Some("after"));
    }

    #[test]
    fn test_buffer_insert_line_out_of_bounds() {
        let mut buffer = Buffer::from_string("hello".to_string());
        let result = buffer.insert_line(10, "oops".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn test_buffer_remove_line() {
        let mut buffer = Buffer::from_string("hello\nworld\ntest".to_string());
        assert_eq!(buffer.len(), 3);

        let removed = buffer.remove_line(1).unwrap();
        assert_eq!(removed, "world");
        assert_eq!(buffer.len(), 2);
        assert_eq!(buffer.line(0), Some("hello"));
        assert_eq!(buffer.line(1), Some("test"));
    }

    #[test]
    fn test_buffer_remove_last_line() {
        let mut buffer = Buffer::from_string("only line".to_string());
        assert_eq!(buffer.len(), 1);

        let removed = buffer.remove_line(0).unwrap();
        assert_eq!(removed, "only line");
        assert_eq!(buffer.len(), 1);
        assert_eq!(buffer.line(0), Some("")); // Line cleared, not removed
    }

    #[test]
    fn test_buffer_remove_line_out_of_bounds() {
        let mut buffer = Buffer::from_string("hello".to_string());
        let result = buffer.remove_line(10);
        assert!(result.is_err());
    }
}

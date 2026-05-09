//! Higher-level buffer operations.
//!
//! Provides convenience methods for common editing operations that combine
//! multiple low-level buffer operations.

use super::unicode;
use super::{Buffer, Cursor};
use crate::error::BufferError;

/// Insert text at the cursor position and return the new cursor position.
///
/// The new cursor will be positioned after the inserted text.
pub fn insert_text(
    buffer: &mut Buffer,
    cursor: &Cursor,
    text: &str,
) -> Result<Cursor, BufferError> {
    // Handle newlines in the text
    let lines: Vec<&str> = text.split('\n').collect();

    if lines.len() == 1 {
        // No newlines - simple insert
        buffer.insert_string(cursor, text)?;
        Ok(Cursor::new(cursor.row, cursor.col + text.len()))
    } else {
        // Has newlines - need to split the line
        let first_line = lines[0];
        let last_line = lines[lines.len() - 1];

        // Insert first line
        buffer.insert_string(cursor, first_line)?;

        // Split line at insertion point + first line length
        let split_cursor = Cursor::new(cursor.row, cursor.col + first_line.len());
        buffer.split_line(&split_cursor)?;

        // Insert middle lines (if any)
        let mut current_row = split_cursor.row + 1;
        for &line in &lines[1..lines.len() - 1] {
            buffer.insert_string(&Cursor::new(current_row, 0), line)?;
            buffer.split_line(&Cursor::new(current_row, line.len()))?;
            current_row += 1;
        }

        // Insert last line at the final row
        let last_row = cursor.row + lines.len() - 1;
        buffer.insert_string(&Cursor::new(last_row, 0), last_line)?;

        Ok(Cursor::new(last_row, last_line.len()))
    }
}

/// Delete characters forward from the cursor.
///
/// Returns the deleted text.
pub fn delete_forward(
    buffer: &mut Buffer,
    cursor: &Cursor,
    count: usize,
) -> Result<String, BufferError> {
    let mut deleted = String::new();

    // We delete at the same position each time since the buffer shrinks
    // after each deletion. The cursor position stays constant.
    for _ in 0..count {
        if let Some(line) = buffer.line(cursor.row) {
            if cursor.col < line.len() {
                // Delete character at current position using delete_range
                let next_boundary = unicode::next_grapheme_boundary(line, cursor.col);
                let start = Cursor::new(cursor.row, cursor.col);
                let end = Cursor::new(cursor.row, next_boundary);
                let del = buffer.delete_range(&start, &end)?;
                deleted.push_str(&del);
            } else if cursor.row + 1 < buffer.len() {
                // At end of line - delete newline (join lines)
                buffer.join_lines(cursor.row)?;
                deleted.push('\n');
            } else {
                // At end of buffer
                break;
            }
        } else {
            break;
        }
    }

    Ok(deleted)
}

/// Delete characters backward from the cursor.
///
/// Returns the deleted text (in forward order, not reversed).
pub fn delete_backward(
    buffer: &mut Buffer,
    cursor: &Cursor,
    count: usize,
) -> Result<String, BufferError> {
    let mut deleted: Vec<String> = Vec::new();
    let mut current_cursor = *cursor;

    for _ in 0..count {
        if current_cursor.col > 0 {
            // Capture the full grapheme cluster text BEFORE deleting so we
            // store the complete cluster (not just the first codepoint).
            let grapheme_bytes = if let Some(line) = buffer.line(current_cursor.row) {
                let prev = unicode::prev_grapheme_boundary(line, current_cursor.col);
                current_cursor.col - prev
            } else {
                0
            };
            let grapheme_text = buffer
                .line(current_cursor.row)
                .map(|line| {
                    line[(current_cursor.col - grapheme_bytes)..current_cursor.col].to_string()
                })
                .unwrap_or_default();
            // Delete the grapheme before cursor
            buffer.delete_char(&current_cursor)?;
            if !grapheme_text.is_empty() {
                deleted.push(grapheme_text);
                current_cursor.col -= grapheme_bytes;
            }
        } else if current_cursor.row > 0 {
            // At start of line - join with previous line
            let prev_line_len = buffer
                .line(current_cursor.row - 1)
                .map(|s| s.len())
                .unwrap_or(0);
            buffer.join_lines(current_cursor.row - 1)?;
            deleted.push("\n".to_string());
            current_cursor.row -= 1;
            current_cursor.col = prev_line_len;
        } else {
            // At start of buffer
            break;
        }
    }

    deleted.reverse();
    Ok(deleted.into_iter().collect())
}

/// Delete the entire line at the given row.
///
/// Returns the deleted line content (without newline).
pub fn delete_line(buffer: &mut Buffer, row: usize) -> Result<String, BufferError> {
    buffer.remove_line(row)
}

/// Delete from cursor to end of line.
pub fn delete_to_end_of_line(buffer: &mut Buffer, cursor: &Cursor) -> Result<String, BufferError> {
    if cursor.row >= buffer.len() {
        return Err(BufferError::RowOutOfBounds {
            row: cursor.row,
            len: buffer.len(),
        });
    }

    let end_cursor = cursor.end_of_line(buffer);
    buffer.delete_range(cursor, &end_cursor)
}

/// Delete from start of line to cursor.
pub fn delete_to_start_of_line(
    buffer: &mut Buffer,
    cursor: &Cursor,
) -> Result<String, BufferError> {
    let start_cursor = cursor.start_of_line();
    buffer.delete_range(&start_cursor, cursor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_text_simple() {
        let mut buffer = Buffer::from_string("hello".to_string());
        let cursor = Cursor::new(0, 2);
        let new_cursor = insert_text(&mut buffer, &cursor, "X").unwrap();
        assert_eq!(buffer.line(0), Some("heXllo"));
        assert_eq!(new_cursor, Cursor::new(0, 3));
    }

    #[test]
    fn test_insert_text_with_newline() {
        let mut buffer = Buffer::from_string("hello".to_string());
        let cursor = Cursor::new(0, 2);
        let new_cursor = insert_text(&mut buffer, &cursor, "X\nY").unwrap();
        assert_eq!(buffer.line(0), Some("heX"));
        assert_eq!(buffer.line(1), Some("Yllo"));
        assert_eq!(new_cursor, Cursor::new(1, 1));
    }

    #[test]
    fn test_insert_text_multiple_lines() {
        let mut buffer = Buffer::from_string("hello".to_string());
        let cursor = Cursor::new(0, 2);
        let new_cursor = insert_text(&mut buffer, &cursor, "A\nB\nC").unwrap();
        assert_eq!(buffer.line(0), Some("heA"));
        assert_eq!(buffer.line(1), Some("B"));
        assert_eq!(buffer.line(2), Some("Cllo"));
        assert_eq!(buffer.len(), 3);
        assert_eq!(new_cursor, Cursor::new(2, 1));
    }

    #[test]
    fn test_delete_forward() {
        let mut buffer = Buffer::from_string("hello".to_string());
        let cursor = Cursor::new(0, 1);
        let deleted = delete_forward(&mut buffer, &cursor, 2).unwrap();
        assert_eq!(deleted, "el");
        assert_eq!(buffer.line(0), Some("hlo"));
    }

    #[test]
    fn test_delete_forward_join_lines() {
        let mut buffer = Buffer::from_string("ab\ncd".to_string());
        let cursor = Cursor::new(0, 2); // At end of first line
        let deleted = delete_forward(&mut buffer, &cursor, 1).unwrap();
        assert_eq!(deleted, "\n");
        assert_eq!(buffer.len(), 1);
        assert_eq!(buffer.line(0), Some("abcd"));
    }

    #[test]
    fn test_delete_line() {
        let mut buffer = Buffer::from_string("hello\nworld".to_string());
        let deleted = delete_line(&mut buffer, 0).unwrap();
        assert_eq!(deleted, "hello");
        assert_eq!(buffer.len(), 1);
        assert_eq!(buffer.line(0), Some("world"));
    }

    #[test]
    fn test_delete_line_uses_buffer_remove() {
        // Verify that delete_line uses the efficient remove_line method
        let mut buffer = Buffer::from_string("a\nb\nc".to_string());
        assert_eq!(buffer.len(), 3);

        delete_line(&mut buffer, 1).unwrap();
        assert_eq!(buffer.len(), 2);
        assert_eq!(buffer.line(0), Some("a"));
        assert_eq!(buffer.line(1), Some("c"));
    }
}

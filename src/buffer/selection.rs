//! Text selection representation.
//!
//! Provides structures for representing selected text regions in the buffer.

use super::{unicode::next_grapheme_boundary, Buffer, Cursor};

/// Type of text selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionType {
    /// Character-wise selection
    Character,
    /// Line-wise selection
    Line,
    /// Block selection (for future implementation)
    Block,
}

/// Represents a text selection in the buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    /// Start cursor position
    pub start: Cursor,
    /// End cursor position
    pub end: Cursor,
    /// Type of selection
    pub selection_type: SelectionType,
}

impl Selection {
    /// Create a new selection.
    pub fn new(start: Cursor, end: Cursor, selection_type: SelectionType) -> Self {
        Self {
            start,
            end,
            selection_type,
        }
    }

    /// Normalize the selection so that `start <= end`.
    ///
    /// For Character and Line selections, the cursor with the smaller `(row,
    /// col)` position becomes `start`. For Block selections, rows are ordered
    /// so `start.row <= end.row` **and** columns are ordered so
    /// `start.col <= end.col`, giving a well-defined top-left / bottom-right
    /// rectangle.
    pub fn normalize(&self) -> Self {
        match self.selection_type {
            SelectionType::Character | SelectionType::Line => {
                let (start, end) = if self.start <= self.end {
                    (self.start, self.end)
                } else {
                    (self.end, self.start)
                };
                Self {
                    start,
                    end,
                    selection_type: self.selection_type,
                }
            }
            SelectionType::Block => {
                // Order rows and columns independently so the result is a
                // top-left / bottom-right rectangle regardless of which corner
                // the anchor and cursor occupy.
                let start = Cursor::new(
                    self.start.row.min(self.end.row),
                    self.start.col.min(self.end.col),
                );
                let end = Cursor::new(
                    self.start.row.max(self.end.row),
                    self.start.col.max(self.end.col),
                );
                Self {
                    start,
                    end,
                    selection_type: self.selection_type,
                }
            }
        }
    }

    /// Check if the selection contains the given cursor position.
    pub fn contains_cursor(&self, cursor: &Cursor) -> bool {
        let normalized = self.normalize();
        match normalized.selection_type {
            SelectionType::Character => {
                // Check if cursor is within the selection bounds
                (normalized.start.row < cursor.row
                    || (normalized.start.row == cursor.row && normalized.start.col <= cursor.col))
                    && (cursor.row < normalized.end.row
                        || (cursor.row == normalized.end.row && cursor.col <= normalized.end.col))
            }
            SelectionType::Line => {
                normalized.start.row <= cursor.row && cursor.row <= normalized.end.row
            }
            SelectionType::Block => {
                // After normalize(), start.col <= end.col and start.row <= end.row.
                normalized.start.row <= cursor.row
                    && cursor.row <= normalized.end.row
                    && normalized.start.col <= cursor.col
                    && cursor.col <= normalized.end.col
            }
        }
    }

    /// Get the text content of this selection.
    pub fn text(&self, buffer: &Buffer) -> String {
        let normalized = self.normalize();

        match normalized.selection_type {
            SelectionType::Character => {
                if normalized.start.row == normalized.end.row {
                    // Single line selection — end is inclusive (matches operator behaviour).
                    if let Some(line) = buffer.line(normalized.start.row) {
                        let start = normalized.start.col.min(line.len());
                        let end = next_grapheme_boundary(line, normalized.end.col.min(line.len()));
                        line[start..end].to_string()
                    } else {
                        String::new()
                    }
                } else {
                    // Multi-line selection
                    let mut result = String::new();

                    // First line: from start.col to end of line
                    if let Some(line) = buffer.line(normalized.start.row) {
                        result.push_str(&line[normalized.start.col.min(line.len())..]);
                    }

                    // Middle lines: entire lines
                    for i in (normalized.start.row + 1)..normalized.end.row {
                        if let Some(line) = buffer.line(i) {
                            result.push('\n');
                            result.push_str(line);
                        }
                    }

                    // Last line: from start to end.col (inclusive)
                    if normalized.start.row < normalized.end.row {
                        if let Some(line) = buffer.line(normalized.end.row) {
                            result.push('\n');
                            let end = next_grapheme_boundary(
                                line,
                                normalized.end.col.min(line.len()),
                            );
                            result.push_str(&line[..end]);
                        }
                    }

                    result
                }
            }
            SelectionType::Line => {
                let mut result = String::new();
                for i in
                    normalized.start.row..=normalized.end.row.min(buffer.len().saturating_sub(1))
                {
                    if let Some(line) = buffer.line(i) {
                        if !result.is_empty() {
                            result.push('\n');
                        }
                        result.push_str(line);
                    }
                }
                result
            }
            SelectionType::Block => {
                // After normalize(), start.col <= end.col and start.row <= end.row.
                let mut result = String::new();
                for i in
                    normalized.start.row..=normalized.end.row.min(buffer.len().saturating_sub(1))
                {
                    if let Some(line) = buffer.line(i) {
                        if !result.is_empty() {
                            result.push('\n');
                        }
                        let s = normalized.start.col.min(line.len());
                        // Advance past the last grapheme for inclusive end.
                        let e = next_grapheme_boundary(line, normalized.end.col.min(line.len()));
                        result.push_str(&line[s..e]);
                    }
                }
                result
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_selection_new() {
        let start = Cursor::new(0, 0);
        let end = Cursor::new(0, 5);
        let selection = Selection::new(start, end, SelectionType::Character);
        assert_eq!(selection.start, start);
        assert_eq!(selection.end, end);
    }

    #[test]
    fn test_selection_normalize() {
        let start = Cursor::new(0, 5);
        let end = Cursor::new(0, 0);
        let selection = Selection::new(start, end, SelectionType::Character);
        let normalized = selection.normalize();
        assert_eq!(normalized.start, end);
        assert_eq!(normalized.end, start);
    }

    #[test]
    fn test_selection_contains_cursor() {
        let _buffer = Buffer::from_string("hello\nworld".to_string());
        let selection = Selection::new(
            Cursor::new(0, 1),
            Cursor::new(0, 4),
            SelectionType::Character,
        );

        assert!(selection.contains_cursor(&Cursor::new(0, 2)));
        assert!(!selection.contains_cursor(&Cursor::new(0, 0)));
        assert!(!selection.contains_cursor(&Cursor::new(0, 5)));
    }

    #[test]
    fn test_selection_text_character() {
        let buffer = Buffer::from_string("hello\nworld".to_string());
        let selection = Selection::new(
            Cursor::new(0, 1),
            Cursor::new(0, 4),
            SelectionType::Character,
        );
        // end col 4 is 'o' in "hello"; inclusive → "ello"
        assert_eq!(selection.text(&buffer), "ello");
    }

    #[test]
    fn test_selection_text_line() {
        let buffer = Buffer::from_string("hello\nworld\ntest".to_string());
        let selection = Selection::new(Cursor::new(0, 0), Cursor::new(1, 0), SelectionType::Line);
        assert_eq!(selection.text(&buffer), "hello\nworld");
    }

    #[test]
    fn test_selection_text_multiline_character() {
        let buffer = Buffer::from_string("hello\nworld".to_string());
        let selection = Selection::new(
            Cursor::new(0, 3),
            Cursor::new(1, 2),
            SelectionType::Character,
        );
        // end col 2 is 'r' in "world"; inclusive last line → "lo\nwor"
        assert_eq!(selection.text(&buffer), "lo\nwor");
    }

    // =========================================================================
    // Block selection tests (Chunk E)
    // =========================================================================

    #[test]
    fn test_block_contains_cursor() {
        // Block from (0,1) to (2,3): rows 0-2, cols 1-3
        let selection = Selection::new(Cursor::new(0, 1), Cursor::new(2, 3), SelectionType::Block);
        // Inside: row 1, col 2
        assert!(selection.contains_cursor(&Cursor::new(1, 2)));
        // Corner: row 0, col 1
        assert!(selection.contains_cursor(&Cursor::new(0, 1)));
        // Corner: row 2, col 3
        assert!(selection.contains_cursor(&Cursor::new(2, 3)));
        // Outside: col too small
        assert!(!selection.contains_cursor(&Cursor::new(1, 0)));
        // Outside: col too large
        assert!(!selection.contains_cursor(&Cursor::new(1, 4)));
        // Outside: row too large
        assert!(!selection.contains_cursor(&Cursor::new(3, 2)));
    }

    #[test]
    fn test_block_contains_cursor_reversed_anchor() {
        // Anchor at (2,3), cursor at (0,1) — un-normalized.
        // normalize() independently sorts rows (0..=2) and cols (1..=3).
        let selection = Selection::new(Cursor::new(2, 3), Cursor::new(0, 1), SelectionType::Block);
        assert!(selection.contains_cursor(&Cursor::new(1, 2)));
        assert!(!selection.contains_cursor(&Cursor::new(1, 0)));
        assert!(!selection.contains_cursor(&Cursor::new(1, 4)));
    }

    #[test]
    fn test_block_normalize_mixed_corners() {
        // Anchor top-right (0,3), cursor bottom-left (2,1) — rows ordered but cols reversed.
        // normalize() must sort cols independently: start.col=1, end.col=3.
        let selection = Selection::new(Cursor::new(0, 3), Cursor::new(2, 1), SelectionType::Block);
        let norm = selection.normalize();
        assert_eq!(norm.start, Cursor::new(0, 1));
        assert_eq!(norm.end, Cursor::new(2, 3));
        // Cursor inside the block should be found
        assert!(selection.contains_cursor(&Cursor::new(1, 2)));
        // Cursor outside the column range should not be found
        assert!(!selection.contains_cursor(&Cursor::new(1, 0)));
        assert!(!selection.contains_cursor(&Cursor::new(1, 4)));
    }

    #[test]
    fn test_block_text() {
        // "hello\nworld\ntest" — block cols 1..=3 across rows 0..=2
        let buffer = Buffer::from_string("hello\nworld\ntest".to_string());
        let selection = Selection::new(Cursor::new(0, 1), Cursor::new(2, 3), SelectionType::Block);
        // Row 0: "hello"[1..4] = "ell"
        // Row 1: "world"[1..4] = "orl"
        // Row 2: "test"[1..4]  = "est"
        assert_eq!(selection.text(&buffer), "ell\norl\nest");
    }

    #[test]
    fn test_block_text_col_clamped_to_line_length() {
        // Lines shorter than col_end should be clamped gracefully
        let buffer = Buffer::from_string("hi\nworld\nok".to_string());
        let selection = Selection::new(Cursor::new(0, 1), Cursor::new(2, 4), SelectionType::Block);
        // Row 0: "hi",   next_grapheme_boundary("hi", min(4,2)=2) = 2 -> [1..2] = "i"
        // Row 1: "world", next_grapheme_boundary("world", 4) = 5       -> [1..5] = "orld"
        // Row 2: "ok",   next_grapheme_boundary("ok", min(4,2)=2) = 2  -> [1..2] = "k"
        assert_eq!(selection.text(&buffer), "i\norld\nk");
    }
}

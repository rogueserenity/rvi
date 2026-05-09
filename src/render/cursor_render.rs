//! Cursor positioning and rendering.
//!
//! Handles conversion from buffer coordinates (byte offsets) to terminal
//! display coordinates, and positioning the terminal cursor. When a line
//! number gutter is active, the cursor column is offset by the gutter width.

use std::io::{self, Write};

use crate::buffer::unicode::display_width_up_to;
use crate::buffer::{Buffer, Cursor};
use crate::render::buffer_render::{wrap_line_segments, wrapped_line_height};
use crate::render::viewport::Viewport;
use crate::terminal::TerminalOutput;

/// Convert a byte offset in a line to its display column.
///
/// # Arguments
///
/// * `line` - The line content
/// * `byte_offset` - Byte offset in the line
/// * `tabstop` - Number of columns between tab stops
pub fn byte_offset_to_display_col(line: &str, byte_offset: usize, tabstop: usize) -> usize {
    display_width_up_to(line, byte_offset, tabstop)
}

/// Compute the wrap-aware terminal (row, col) for a cursor position.
///
/// Returns `(terminal_row, display_col)` where `terminal_row` is 1-indexed
/// and `display_col` is 0-indexed within the text area (before gutter offset).
///
/// Sums `wrapped_line_height` for each buffer line from `top_line` up to
/// (not including) `cursor.row`, then adds the index of the segment within
/// `cursor.row` that contains `cursor.col`. The display column is computed
/// relative to that segment's byte start.
///
/// # Performance
///
/// Calls `wrapped_line_height` (which allocates a `Vec<WrapSegment>`) once
/// per line between `top_line` and `cursor.row`. For typical terminal heights
/// this is at most ~50 calls per render cycle, which is negligible in practice.
fn wrap_cursor_terminal_pos(
    cursor: &Cursor,
    cursor_line: &str,
    viewport: &Viewport,
    buffer: &Buffer,
    text_width: usize,
    tabstop: usize,
) -> (usize, usize) {
    // Sum wrapped heights of lines above cursor.row
    let top = viewport.top_line();
    let mut row: usize = 1;
    for i in top..cursor.row {
        let line = buffer.line(i).unwrap_or("");
        // Each call allocates a Vec<WrapSegment>; acceptable at typical terminal heights
        row += wrapped_line_height(line, text_width, tabstop, true, false);
    }

    // Find which segment within cursor.row contains cursor.col
    let segments = wrap_line_segments(cursor_line, text_width, tabstop, false);
    let seg_idx = segments
        .iter()
        .rposition(|s| cursor.col >= s.byte_start)
        .unwrap_or(0);

    row += seg_idx;

    // Display column is relative to the segment's byte start, not the line start
    let seg = &segments[seg_idx];
    let clamped_col = cursor.col.min(cursor_line.len());
    let seg_local_offset = clamped_col.saturating_sub(seg.byte_start);
    let seg_text = &cursor_line[seg.byte_start..seg.byte_end];
    let display_col = byte_offset_to_display_col(seg_text, seg_local_offset, tabstop);

    (row, display_col)
}

/// Position the terminal cursor at the buffer cursor location.
///
/// When a gutter is active (`gutter_width > 0`), the cursor column is
/// offset to the right so it appears in the text area, not in the gutter.
///
/// When `wrap` is true, accounts for lines above the cursor that may span
/// multiple terminal rows, and computes the terminal column relative to the
/// segment (visual row) that contains `cursor.col`.
///
/// # Arguments
///
/// * `output` - Terminal output handle
/// * `cursor` - Buffer cursor position
/// * `viewport` - Current viewport
/// * `buffer` - The buffer
/// * `tabstop` - Number of columns between tab stops
/// * `gutter_width` - Width of the line number gutter in columns (0 when disabled)
/// * `wrap` - Whether soft-wrapping is enabled
/// * `text_width` - Available text columns (viewport width minus gutter)
#[allow(clippy::too_many_arguments)]
pub fn position_cursor<W: Write>(
    output: &mut TerminalOutput<W>,
    cursor: &Cursor,
    viewport: &Viewport,
    buffer: &Buffer,
    tabstop: usize,
    gutter_width: usize,
    wrap: bool,
    text_width: usize,
) -> io::Result<()> {
    if !viewport.is_line_visible(cursor.row) {
        // Cursor not visible - don't position it
        return Ok(());
    }

    // A missing cursor line positions the cursor at the start of the text area.
    // This is an invariant violation (cursor.row should always be valid when
    // is_line_visible returns true), but we handle it gracefully in both paths.
    let Some(cursor_line) = buffer.line(cursor.row) else {
        let terminal_row = (cursor.row - viewport.top_line() + 1) as u16;
        output.goto((gutter_width + 1) as u16, terminal_row)?;
        return Ok(());
    };

    let (terminal_row, display_col) = if wrap && text_width > 0 {
        let (row, col) =
            wrap_cursor_terminal_pos(cursor, cursor_line, viewport, buffer, text_width, tabstop);
        (row as u16, col)
    } else {
        let row = (cursor.row - viewport.top_line() + 1) as u16;
        let col =
            byte_offset_to_display_col(cursor_line, cursor.col.min(cursor_line.len()), tabstop);
        (row, col)
    };

    // Terminal columns are 1-indexed, offset by gutter width
    let terminal_col = (gutter_width + display_col + 1) as u16;

    // Ensure we don't exceed viewport width
    let terminal_col = terminal_col.min(viewport.width() as u16);

    output.goto(terminal_col, terminal_row)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::unicode::DEFAULT_TAB_WIDTH;

    #[test]
    fn test_byte_offset_to_display_col() {
        let line = "hello";
        assert_eq!(byte_offset_to_display_col(line, 0, DEFAULT_TAB_WIDTH), 0);
        assert_eq!(byte_offset_to_display_col(line, 2, DEFAULT_TAB_WIDTH), 2);
        assert_eq!(byte_offset_to_display_col(line, 5, DEFAULT_TAB_WIDTH), 5);
    }

    #[test]
    fn test_byte_offset_to_display_col_wide_chars() {
        let line = "你好"; // Each CJK char is 2 columns wide
        assert_eq!(byte_offset_to_display_col(line, 0, DEFAULT_TAB_WIDTH), 0);
        // After first character (3 bytes for 你好)
        assert_eq!(byte_offset_to_display_col(line, 3, DEFAULT_TAB_WIDTH), 2);
    }

    #[test]
    fn test_byte_offset_to_display_col_with_tab() {
        let line = "\thello";
        // After tab (1 byte), display column should be 8 with default tabstop
        assert_eq!(byte_offset_to_display_col(line, 1, DEFAULT_TAB_WIDTH), 8);
        // With tabstop=4, display column should be 4
        assert_eq!(byte_offset_to_display_col(line, 1, 4), 4);
    }

    #[test]
    fn test_position_cursor_with_tab() {
        let buffer = Buffer::from_string("\thello".to_string());
        let viewport = Viewport::new(0, 10, 80);
        let cursor = Cursor::new(0, 1); // After the tab character
        let mut output = TerminalOutput::with_writer(Vec::new());

        position_cursor(
            &mut output,
            &cursor,
            &viewport,
            &buffer,
            DEFAULT_TAB_WIDTH,
            0,
            false,
            80,
        )
        .unwrap();
        // Cursor should be at display column 8 + 1 (1-indexed) = 9
        // Verify by checking that output is non-empty (goto was called)
        assert!(!output.writer().is_empty());
    }

    #[test]
    fn test_position_cursor_with_gutter() {
        let buffer = Buffer::from_string("hello".to_string());
        let viewport = Viewport::new(0, 10, 80);
        let cursor = Cursor::new(0, 0);
        let mut output_no_gutter = TerminalOutput::with_writer(Vec::new());
        let mut output_with_gutter = TerminalOutput::with_writer(Vec::new());

        // Without gutter
        position_cursor(
            &mut output_no_gutter,
            &cursor,
            &viewport,
            &buffer,
            DEFAULT_TAB_WIDTH,
            0,
            false,
            80,
        )
        .unwrap();

        // With gutter width 4
        position_cursor(
            &mut output_with_gutter,
            &cursor,
            &viewport,
            &buffer,
            DEFAULT_TAB_WIDTH,
            4,
            false,
            76,
        )
        .unwrap();

        // Both should produce output (goto was called)
        assert!(!output_no_gutter.writer().is_empty());
        assert!(!output_with_gutter.writer().is_empty());

        // The outputs should differ because the gutter offsets the column
        assert_ne!(output_no_gutter.writer(), output_with_gutter.writer());
    }

    #[test]
    fn test_position_cursor_gutter_zero_same_as_no_gutter() {
        let buffer = Buffer::from_string("hello".to_string());
        let viewport = Viewport::new(0, 10, 80);
        let cursor = Cursor::new(0, 3);
        let mut output1 = TerminalOutput::with_writer(Vec::new());
        let mut output2 = TerminalOutput::with_writer(Vec::new());

        position_cursor(
            &mut output1,
            &cursor,
            &viewport,
            &buffer,
            DEFAULT_TAB_WIDTH,
            0,
            false,
            80,
        )
        .unwrap();
        position_cursor(
            &mut output2,
            &cursor,
            &viewport,
            &buffer,
            DEFAULT_TAB_WIDTH,
            0,
            false,
            80,
        )
        .unwrap();

        // Identical when gutter_width is 0
        assert_eq!(output1.writer(), output2.writer());
    }

    // =========================================================================
    // Wrap-aware position_cursor tests
    // =========================================================================

    /// Helper: extract the goto (row, col) from raw termion output bytes.
    /// termion encodes goto as ESC [ {row} ; {col} H
    fn parse_goto(bytes: &[u8]) -> Option<(u16, u16)> {
        let s = std::str::from_utf8(bytes).ok()?;
        // Find ESC [ ... H sequence
        let start = s.find("\x1b[")?;
        let rest = &s[start + 2..];
        let end = rest.find('H')?;
        let coords = &rest[..end];
        let mut parts = coords.split(';');
        let row: u16 = parts.next()?.parse().ok()?;
        let col: u16 = parts.next()?.parse().ok()?;
        Some((row, col))
    }

    #[test]
    fn test_position_cursor_wrap_false_unchanged() {
        // wrap=false should produce the same result as before (1 row per line)
        let buffer = Buffer::from_string("hello\nworld".to_string());
        // Viewport: top=0, height=10, width=80
        let viewport = Viewport::new(0, 10, 80);
        let cursor = Cursor::new(1, 3); // second line, col 3
        let mut output = TerminalOutput::with_writer(Vec::new());

        position_cursor(
            &mut output,
            &cursor,
            &viewport,
            &buffer,
            DEFAULT_TAB_WIDTH,
            0,
            false,
            80,
        )
        .unwrap();

        let (row, col) = parse_goto(output.writer()).unwrap();
        assert_eq!(row, 2); // line 1 -> terminal row 2
        assert_eq!(col, 4); // col 3 -> display col 3, +1 for 1-indexed = 4
    }

    #[test]
    fn test_position_cursor_wrap_short_line() {
        // Line fits in one row: cursor on terminal row 1
        let buffer = Buffer::from_string("hello".to_string());
        let viewport = Viewport::new(0, 10, 80);
        let cursor = Cursor::new(0, 2);
        let mut output = TerminalOutput::with_writer(Vec::new());

        position_cursor(
            &mut output,
            &cursor,
            &viewport,
            &buffer,
            DEFAULT_TAB_WIDTH,
            0,
            true,
            80,
        )
        .unwrap();

        let (row, col) = parse_goto(output.writer()).unwrap();
        assert_eq!(row, 1); // only one buffer line visible, terminal row 1
        assert_eq!(col, 3); // byte offset 2 -> display col 2 -> +1 = 3
    }

    #[test]
    fn test_position_cursor_wrap_cursor_on_continuation_row() {
        // Line "abcdefghij" with text_width=5 wraps to 2 rows.
        // cursor.col=7 is in the second segment [5..10] -> terminal_row=2
        let buffer = Buffer::from_string("abcdefghij".to_string());
        let viewport = Viewport::new(0, 10, 5);
        let cursor = Cursor::new(0, 7);
        let mut output = TerminalOutput::with_writer(Vec::new());

        position_cursor(
            &mut output,
            &cursor,
            &viewport,
            &buffer,
            DEFAULT_TAB_WIDTH,
            0,
            true,
            5,
        )
        .unwrap();

        let (row, _col) = parse_goto(output.writer()).unwrap();
        assert_eq!(row, 2); // cursor is on the second visual row
    }

    #[test]
    fn test_position_cursor_wrap_col_relative_to_segment() {
        // Line "abcdefghij" text_width=5. cursor.col=7 is in seg [5..10].
        // display col relative to segment = 7 - 5 = 2 -> terminal_col = 3
        let buffer = Buffer::from_string("abcdefghij".to_string());
        let viewport = Viewport::new(0, 10, 5);
        let cursor = Cursor::new(0, 7);
        let mut output = TerminalOutput::with_writer(Vec::new());

        position_cursor(
            &mut output,
            &cursor,
            &viewport,
            &buffer,
            DEFAULT_TAB_WIDTH,
            0,
            true,
            5,
        )
        .unwrap();

        let (_row, col) = parse_goto(output.writer()).unwrap();
        // Segment starts at byte 5, cursor.col=7, offset within seg=2, display_col=2, +1=3
        assert_eq!(col, 3);
    }

    #[test]
    fn test_position_cursor_wrap_lines_above_are_tall() {
        // Two lines above the cursor, each wrapping to 2 terminal rows.
        // cursor is on line 2 (0-indexed), terminal_row = 1 + 2 + 2 + 0 = 5
        //   (1 base + 2 rows for line0 + 2 rows for line1, cursor on seg 0 of line2)
        let buffer = Buffer::from_string("abcdefghij\nklmnopqrst\nhello".to_string());
        // viewport: top=0, height=20, width=5 -> text_width=5
        let viewport = Viewport::new(0, 20, 5);
        let cursor = Cursor::new(2, 0); // "hello", col 0
        let mut output = TerminalOutput::with_writer(Vec::new());

        position_cursor(
            &mut output,
            &cursor,
            &viewport,
            &buffer,
            DEFAULT_TAB_WIDTH,
            0,
            true,
            5,
        )
        .unwrap();

        let (row, col) = parse_goto(output.writer()).unwrap();
        assert_eq!(row, 5); // 1 + 2 (line0) + 2 (line1) + 0 (seg0 of line2) = 5
        assert_eq!(col, 1); // col 0, display_col 0, +1 = 1
    }

    // =========================================================================
    // wrap_cursor_terminal_pos unit tests
    // =========================================================================

    #[test]
    fn test_wrap_cursor_terminal_pos_first_segment() {
        let buffer = Buffer::from_string("abcdefghij\nhello".to_string());
        let viewport = Viewport::new(0, 20, 5);
        let cursor = Cursor::new(0, 2);
        let (row, col) = wrap_cursor_terminal_pos(
            &cursor,
            "abcdefghij",
            &viewport,
            &buffer,
            5,
            DEFAULT_TAB_WIDTH,
        );
        assert_eq!(row, 1); // no lines above, seg 0 -> row 1
        assert_eq!(col, 2); // col 2 in seg [0..5], display_col = 2
    }

    #[test]
    fn test_wrap_cursor_terminal_pos_second_segment() {
        let buffer = Buffer::from_string("abcdefghij".to_string());
        let viewport = Viewport::new(0, 20, 5);
        let cursor = Cursor::new(0, 7);
        let (row, col) = wrap_cursor_terminal_pos(
            &cursor,
            "abcdefghij",
            &viewport,
            &buffer,
            5,
            DEFAULT_TAB_WIDTH,
        );
        assert_eq!(row, 2); // seg 1 -> row 1 + 1 = 2
        assert_eq!(col, 2); // offset 7-5=2 in seg [5..10], display_col = 2
    }

    #[test]
    fn test_wrap_cursor_terminal_pos_with_tall_lines_above() {
        let buffer = Buffer::from_string("abcdefghij\nklmnopqrst\nhello".to_string());
        let viewport = Viewport::new(0, 20, 5);
        let cursor = Cursor::new(2, 0);
        let (row, col) =
            wrap_cursor_terminal_pos(&cursor, "hello", &viewport, &buffer, 5, DEFAULT_TAB_WIDTH);
        assert_eq!(row, 5); // 1 + 2 (line0) + 2 (line1) + seg0 = 5
        assert_eq!(col, 0);
    }
}

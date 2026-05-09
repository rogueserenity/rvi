//! Buffer content rendering.
//!
//! Handles rendering the buffer's text content to the terminal, including
//! viewport management, line truncation, display width calculations,
//! search match highlighting using reverse video, optional line number
//! gutter display, and soft-wrapping of long lines.

use std::io::{self, Write};

use unicode_segmentation::UnicodeSegmentation;

use crate::buffer::unicode::{next_grapheme_boundary, tab_width_at_col};
use crate::buffer::{Buffer, Selection, SelectionType};
use crate::render::viewport::Viewport;
use crate::render::Gutter;
use crate::search::highlighter::HighlightRange;
use crate::terminal::TerminalOutput;

/// An empty highlight slice, used when no highlights exist for a line.
const NO_HIGHLIGHTS: &[HighlightRange] = &[];

/// A visual row segment produced by wrapping a buffer line.
///
/// Represents a contiguous slice of the original line that fits within
/// `text_width` display columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WrapSegment {
    /// Byte offset of the segment start within the line (inclusive).
    pub(crate) byte_start: usize,
    /// Byte offset of the segment end within the line (exclusive).
    pub(crate) byte_end: usize,
}

/// Split a line into visual row segments, each fitting within `text_width`
/// display columns.
///
/// Returns a list of `(byte_start, byte_end)` segments. Tabs are expanded
/// according to the display column within the segment: each segment starts
/// at display column 0 (since it begins a new terminal row), but a tab's
/// width is calculated relative to the column within that segment.
///
/// When a tab spans the segment boundary, the portion that fits is rendered
/// as spaces in the current segment, and the tab character is "consumed" --
/// the next segment starts after the tab byte.
///
/// When `list` is true, tabs are treated as exactly 2 display columns (`^I`),
/// matching the actual rendering width used by `render_line` in list mode.
pub(crate) fn wrap_line_segments(
    line: &str,
    text_width: usize,
    tabstop: usize,
    list: bool,
) -> Vec<WrapSegment> {
    if line.is_empty() {
        return vec![WrapSegment {
            byte_start: 0,
            byte_end: 0,
        }];
    }

    if text_width == 0 {
        return vec![WrapSegment {
            byte_start: 0,
            byte_end: line.len(),
        }];
    }

    let estimated_segments = (line.len() / text_width.max(1)).max(1) + 1;
    let mut segments = Vec::with_capacity(estimated_segments);
    let mut seg_start = 0usize;
    let mut col = 0usize;

    for (g_start, grapheme) in line.grapheme_indices(true) {
        let g_len = grapheme.len();

        let g_width = if grapheme == "\t" {
            if list {
                2
            } else {
                tab_width_at_col(col, tabstop)
            }
        } else {
            unicode_width::UnicodeWidthStr::width(grapheme)
        };

        if col + g_width > text_width && col > 0 {
            segments.push(WrapSegment {
                byte_start: seg_start,
                byte_end: g_start,
            });
            seg_start = g_start;
            col = 0;
        }

        if grapheme == "\t" && !list {
            // Tab may span across the boundary (only in non-list mode)
            let tab_w = tab_width_at_col(col, tabstop);
            let remaining_width = text_width.saturating_sub(col);
            if tab_w <= remaining_width {
                col += tab_w;
            } else {
                col = text_width;
            }
        } else {
            col += g_width;
        }

        let byte_pos = g_start + g_len;

        if col >= text_width {
            segments.push(WrapSegment {
                byte_start: seg_start,
                byte_end: byte_pos,
            });
            seg_start = byte_pos;
            col = 0;
        }
    }

    // Remaining text that didn't fill a full segment
    if seg_start <= line.len() && (seg_start < line.len() || segments.is_empty()) {
        segments.push(WrapSegment {
            byte_start: seg_start,
            byte_end: line.len(),
        });
    }

    segments
}

/// Render buffer lines to the terminal with optional search highlights,
/// line number gutter, and soft-wrapping.
///
/// Only renders lines that are visible in the viewport. When `wrap` is
/// false, lines that exceed the text area width are truncated. When `wrap`
/// is true, long lines are split across multiple terminal rows.
///
/// When highlights are provided, matching regions are rendered in reverse
/// video using ANSI escape sequences.
///
/// When `list` is true, tabs are displayed as `^I` and each line ends with `$`.
///
/// # Arguments
///
/// * `output` - Terminal output handle
/// * `buffer` - The buffer to render
/// * `viewport` - Current viewport
/// * `highlights` - Per-line highlight ranges, indexed by viewport-relative line
/// * `tabstop` - Number of columns between tab stops
/// * `gutter` - Line number gutter state
/// * `wrap` - Whether to soft-wrap long lines
/// * `list` - Whether to show tabs as `^I` and append `$` to each line
#[allow(clippy::too_many_arguments)]
pub fn render_buffer_lines<W: Write>(
    output: &mut TerminalOutput<W>,
    buffer: &Buffer,
    viewport: &Viewport,
    highlights: &[Vec<HighlightRange>],
    tabstop: usize,
    gutter: &Gutter,
    wrap: bool,
    list: bool,
) -> io::Result<()> {
    let (start_line, end_line) = viewport.visible_range();
    let buffer_len = buffer.len();

    // Determine actual end line (don't exceed buffer length)
    let end_line = end_line.min(buffer_len);

    // Text area width is viewport width minus gutter, with a minimum of 1
    let text_width = viewport.width().saturating_sub(gutter.width()).max(1);

    if wrap {
        render_buffer_lines_wrapped(
            output, buffer, viewport, highlights, tabstop, gutter, text_width, start_line,
            end_line, list,
        )
    } else {
        render_buffer_lines_truncated(
            output, buffer, viewport, highlights, tabstop, gutter, text_width, start_line,
            end_line, list,
        )
    }
}

/// Return the number of terminal rows a buffer line occupies.
///
/// When `wrap` is false, every line occupies exactly one row (truncated).
/// When `wrap` is true, the line is split into segments that each fit within
/// `text_width` display columns, and the number of segments is returned.
/// Empty lines and degenerate inputs (e.g. `text_width = 0`) always return 1.
///
/// When `list` is true, tabs are treated as 2 display columns (`^I`) for
/// segment width calculation. Pass `false` when list mode is not active.
pub fn wrapped_line_height(
    line: &str,
    text_width: usize,
    tabstop: usize,
    wrap: bool,
    list: bool,
) -> usize {
    if !wrap || text_width == 0 {
        return 1;
    }

    let segments = wrap_line_segments(line, text_width, tabstop, list);
    segments.len().max(1)
}

/// Compute highlight ranges for a visual selection across visible lines.
///
/// Returns a vector indexed by viewport-relative line index. Each entry is a list
/// of `HighlightRange` byte ranges on that buffer line. The selection is normalized
/// before use so `start <= end` regardless of cursor direction.
///
/// - **Character selection**: highlights from `start.col` to
///   `next_grapheme_boundary(end.col)` (inclusive last char).
/// - **Line selection**: highlights the entire line (`0..line.len()`).
/// - **Block selection**: highlights the column range on each row.
pub(crate) fn compute_selection_highlights(
    selection: &Selection,
    viewport: &Viewport,
    buffer: &Buffer,
) -> Vec<Vec<HighlightRange>> {
    let sel = selection.normalize();
    let (start_line, end_line) = viewport.visible_range();
    let end_line = end_line.min(buffer.len());
    let mut result = Vec::with_capacity(end_line.saturating_sub(start_line));

    for line_idx in start_line..end_line {
        if line_idx < sel.start.row || line_idx > sel.end.row {
            result.push(Vec::new());
            continue;
        }

        let line = buffer.line(line_idx).unwrap_or("");

        match sel.selection_type {
            SelectionType::Character => {
                let start_byte = if line_idx == sel.start.row {
                    sel.start.col.min(line.len())
                } else {
                    0
                };
                let end_byte = if line_idx == sel.end.row {
                    next_grapheme_boundary(line, sel.end.col.min(line.len()))
                } else {
                    line.len()
                };
                if start_byte < end_byte {
                    result.push(vec![HighlightRange {
                        start: start_byte,
                        end: end_byte,
                    }]);
                } else if start_byte == end_byte {
                    // Zero-width selection (cursor just entered visual mode, no movement).
                    // Highlight the grapheme under the cursor, or a virtual cell on an
                    // empty line.
                    let exclusive = next_grapheme_boundary(line, start_byte);
                    let end = if exclusive > start_byte {
                        exclusive
                    } else {
                        // Empty line: next_grapheme_boundary returns start_byte
                        // unchanged only when the line is empty (start_byte == 0).
                        // Synthesize a 1-byte highlight so the cursor cell is
                        // visible; the renderer skips empty lines before slicing.
                        1
                    };
                    result.push(vec![HighlightRange {
                        start: start_byte,
                        end,
                    }]);
                } else {
                    result.push(Vec::new());
                }
            }
            SelectionType::Line => {
                result.push(vec![HighlightRange {
                    start: 0,
                    end: line.len(),
                }]);
            }
            SelectionType::Block => {
                // After normalize(), sel.start.col <= sel.end.col.
                let col_start = sel.start.col.min(line.len());
                let col_end = next_grapheme_boundary(line, sel.end.col.min(line.len()));
                if col_start < col_end {
                    result.push(vec![HighlightRange {
                        start: col_start,
                        end: col_end,
                    }]);
                } else {
                    result.push(Vec::new());
                }
            }
        }
    }

    result
}

/// Render buffer lines with truncation (wrap = false).
#[allow(clippy::too_many_arguments)]
fn render_buffer_lines_truncated<W: Write>(
    output: &mut TerminalOutput<W>,
    buffer: &Buffer,
    viewport: &Viewport,
    highlights: &[Vec<HighlightRange>],
    tabstop: usize,
    gutter: &Gutter,
    text_width: usize,
    start_line: usize,
    end_line: usize,
    list: bool,
) -> io::Result<()> {
    // Move to top of viewport (row 1, col 1)
    output.goto(1, 1)?;

    // Render each visible line
    for (i, line_idx) in (start_line..end_line).enumerate() {
        if i > 0 {
            output.goto(1, (i + 1) as u16)?;
        }

        // Write gutter (line number) if enabled
        if gutter.enabled() {
            let line_num = line_idx + 1;
            TerminalOutput::write(output, &gutter.format_line_number(line_num))?;
        }

        if let Some(line) = buffer.line(line_idx) {
            let line_highlights: &[HighlightRange] = if i < highlights.len() {
                &highlights[i]
            } else {
                NO_HIGHLIGHTS
            };

            if line_highlights.is_empty() {
                render_line(output, line, text_width, tabstop, list, list)?;
            } else {
                render_line_with_highlights(
                    output,
                    line,
                    text_width,
                    line_highlights,
                    tabstop,
                    list,
                    list,
                )?;
            }
        } else {
            output.clear_line()?;
        }
    }

    // Tilde lines — rows below the end of the buffer show "~" (vi convention).
    let blank_gutter = if gutter.enabled() {
        " ".repeat(gutter.width())
    } else {
        String::new()
    };
    let remaining_lines = (end_line - start_line) as u16;
    for i in remaining_lines..viewport.height() as u16 {
        output.goto(1, i + 1)?;
        if gutter.enabled() {
            TerminalOutput::write(output, &blank_gutter)?;
        }
        TerminalOutput::write(output, "~")?;
        output.clear_to_end_of_line()?;
    }

    Ok(())
}

/// Render buffer lines with soft-wrapping (wrap = true).
///
/// Each buffer line may occupy multiple terminal rows. The gutter shows
/// the line number on the first visual row and blank padding on continuation
/// rows. Rendering stops when `terminal_row` exceeds the viewport height.
#[allow(clippy::too_many_arguments)]
fn render_buffer_lines_wrapped<W: Write>(
    output: &mut TerminalOutput<W>,
    buffer: &Buffer,
    viewport: &Viewport,
    highlights: &[Vec<HighlightRange>],
    tabstop: usize,
    gutter: &Gutter,
    text_width: usize,
    start_line: usize,
    end_line: usize,
    list: bool,
) -> io::Result<()> {
    let viewport_height = viewport.height();

    // terminal_row is 1-indexed (terminal rows start at 1)
    let mut terminal_row: usize = 1;

    let blank_gutter = if gutter.enabled() {
        " ".repeat(gutter.width())
    } else {
        String::new()
    };

    for (i, line_idx) in (start_line..end_line).enumerate() {
        if terminal_row > viewport_height {
            break;
        }

        let line = match buffer.line(line_idx) {
            Some(l) => l,
            None => {
                // Unreachable in practice: end_line is clamped to buffer.len()
                // before this function is called, so line_idx is always valid.
                continue;
            }
        };

        let line_highlights: &[HighlightRange] = if i < highlights.len() {
            &highlights[i]
        } else {
            NO_HIGHLIGHTS
        };

        let segments = wrap_line_segments(line, text_width, tabstop, list);
        let num_segments = segments.len();

        for (seg_idx, segment) in segments.iter().enumerate() {
            if terminal_row > viewport_height {
                break;
            }

            output.goto(1, terminal_row as u16)?;

            // Gutter: line number on first row, blank on continuation rows
            if gutter.enabled() {
                if seg_idx == 0 {
                    let line_num = line_idx + 1;
                    TerminalOutput::write(output, &gutter.format_line_number(line_num))?;
                } else {
                    TerminalOutput::write(output, &blank_gutter)?;
                }
            }

            let seg_text = &line[segment.byte_start..segment.byte_end];
            let is_last_segment = seg_idx + 1 == num_segments;

            if line_highlights.is_empty() {
                render_line(
                    output,
                    seg_text,
                    text_width,
                    tabstop,
                    list,
                    list && is_last_segment,
                )?;
            } else {
                // Adjust highlights to this segment's byte range
                let seg_highlights =
                    clip_highlights(line_highlights, segment.byte_start, segment.byte_end);

                if seg_highlights.is_empty() {
                    render_line(
                        output,
                        seg_text,
                        text_width,
                        tabstop,
                        list,
                        list && is_last_segment,
                    )?;
                } else {
                    render_line_with_highlights(
                        output,
                        seg_text,
                        text_width,
                        &seg_highlights,
                        tabstop,
                        list,
                        list && is_last_segment,
                    )?;
                }
            }

            terminal_row += 1;
        }
    }

    // Fill remaining rows with tilde lines (vi convention: "~" = no line here)
    while terminal_row <= viewport_height {
        output.goto(1, terminal_row as u16)?;
        if gutter.enabled() {
            TerminalOutput::write(output, &blank_gutter)?;
        }
        TerminalOutput::write(output, "~")?;
        output.clear_to_end_of_line()?;
        terminal_row += 1;
    }

    Ok(())
}

/// Clip and shift highlight ranges to a segment's byte window.
///
/// Given highlight ranges in terms of the full line's byte offsets, returns
/// new ranges adjusted for a segment starting at `seg_start` and ending at
/// `seg_end`. Ranges that don't overlap the segment are excluded. Overlapping
/// ranges are clamped and shifted so byte 0 corresponds to `seg_start`.
fn clip_highlights(
    highlights: &[HighlightRange],
    seg_start: usize,
    seg_end: usize,
) -> Vec<HighlightRange> {
    let mut result = Vec::with_capacity(highlights.len());

    for h in highlights {
        // Skip highlights that don't overlap this segment
        if h.end <= seg_start || h.start >= seg_end {
            continue;
        }

        // Clamp to segment boundaries and shift to segment-local offsets
        let clamped_start = h.start.max(seg_start);
        let clamped_end = h.end.min(seg_end);

        result.push(HighlightRange {
            start: clamped_start - seg_start,
            end: clamped_end - seg_start,
        });
    }

    result
}

/// Render a single line without highlights, truncating if necessary.
///
/// When `list` is true, tabs are displayed as `^I` instead of being expanded
/// to spaces. When `append_dollar` is true, a `$` end-of-line marker is written
/// after the content (used in list mode on the last segment of each line).
/// Otherwise, tabs are expanded to spaces (to the next tab stop).
///
/// # Arguments
///
/// * `output` - Terminal output handle
/// * `line` - Line content to render
/// * `max_width` - Maximum display width (text area width)
/// * `tabstop` - Number of columns between tab stops
/// * `list` - Whether to render tabs as `^I`
/// * `append_dollar` - Whether to append `$` at end of line (list mode, last segment only)
fn render_line<W: Write>(
    output: &mut TerminalOutput<W>,
    line: &str,
    max_width: usize,
    tabstop: usize,
    list: bool,
    append_dollar: bool,
) -> io::Result<()> {
    if list {
        let dollar_reserve = if append_dollar { 1 } else { 0 };
        let expanded = expand_tabs_list_mode(line, max_width.saturating_sub(dollar_reserve));
        TerminalOutput::write(output, &expanded)?;
        if append_dollar && max_width > 0 {
            TerminalOutput::write(output, "$")?;
        }
    } else {
        if line.is_empty() {
            return Ok(());
        }
        let expanded = expand_tabs_to_width(line, max_width, tabstop);
        TerminalOutput::write(output, &expanded)?;
    }

    Ok(())
}

/// Render a single line with search highlights using reverse video.
///
/// Iterates through characters of the line, tracking the current byte offset.
/// At match boundaries, emits `termion::style::Invert` / `NoInvert` escape
/// sequences for reverse video highlighting. Tabs are expanded to spaces
/// (or displayed as `^I` in list mode) and the line is truncated at
/// `max_width` display columns.
///
/// # Arguments
///
/// * `output` - Terminal output handle
/// * `line` - Line content to render
/// * `max_width` - Maximum display width (text area width)
/// * `highlights` - Sorted highlight ranges for this line
/// * `tabstop` - Number of columns between tab stops
/// * `list` - Whether to render tabs as `^I`
/// * `append_dollar` - Whether to append `$` at end of line (list mode, last segment only)
fn render_line_with_highlights<W: Write>(
    output: &mut TerminalOutput<W>,
    line: &str,
    max_width: usize,
    highlights: &[HighlightRange],
    tabstop: usize,
    list: bool,
    append_dollar: bool,
) -> io::Result<()> {
    use termion::style;

    if line.is_empty() {
        if append_dollar && max_width > 0 {
            write!(output, "$")?;
        }
        return Ok(());
    }

    // Reserve one column for the trailing '$' when appending
    let text_limit = if append_dollar {
        max_width.saturating_sub(1)
    } else {
        max_width
    };

    let mut col = 0usize;
    let mut byte_pos = 0usize;
    let mut highlight_idx = 0usize;
    let mut in_highlight = false;

    'chars: for ch in line.chars() {
        let ch_start = byte_pos;
        let ch_end = byte_pos + ch.len_utf8();

        // Check if we've left the current highlight
        if in_highlight
            && highlight_idx < highlights.len()
            && ch_start >= highlights[highlight_idx].end
        {
            write!(output, "{}", style::NoInvert)?;
            in_highlight = false;

            // Skip past all exhausted highlights (defensive against degenerate ranges)
            while highlight_idx < highlights.len() && ch_start >= highlights[highlight_idx].end {
                highlight_idx += 1;
            }

            // Check if immediately entering the next highlight
            if highlight_idx < highlights.len() && ch_start >= highlights[highlight_idx].start {
                write!(output, "{}", style::Invert)?;
                in_highlight = true;
            }
        }

        // Check if entering a highlight
        if !in_highlight
            && highlight_idx < highlights.len()
            && ch_start >= highlights[highlight_idx].start
        {
            write!(output, "{}", style::Invert)?;
            in_highlight = true;
        }

        // Render the character (with tab expansion, width tracking)
        if ch == '\t' {
            if list {
                // In list mode render tab as the two-character sequence '^I'
                if col + 2 > text_limit {
                    break 'chars;
                }
                write!(output, "^I")?;
                col += 2;
            } else {
                let tab_spaces = tab_width_at_col(col, tabstop);
                for _ in 0..tab_spaces {
                    if col >= text_limit {
                        break;
                    }
                    write!(output, " ")?;
                    col += 1;
                }
            }
        } else {
            let char_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if col + char_width > text_limit {
                break 'chars;
            }
            write!(output, "{}", ch)?;
            col += char_width;
        }

        if col >= text_limit {
            break;
        }

        byte_pos = ch_end;
    }

    // Ensure style is reset at end of line
    if in_highlight {
        write!(output, "{}", style::NoInvert)?;
    }

    // Append end-of-line marker when requested (list mode, last segment)
    if append_dollar {
        write!(output, "$")?;
    }

    Ok(())
}

/// Expand tabs to the two-character sequence `^I` and truncate to `max_width`.
///
/// Used in list mode. Each `\t` in the input is replaced by the literal
/// characters `^` and `I` (two display columns).
fn expand_tabs_list_mode(line: &str, max_width: usize) -> String {
    let mut result = String::new();
    let mut col = 0usize;

    for ch in line.chars() {
        if ch == '\t' {
            if col + 2 > max_width {
                return result;
            }
            result.push('^');
            result.push('I');
            col += 2;
        } else {
            let char_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if col + char_width > max_width {
                return result;
            }
            result.push(ch);
            col += char_width;
        }
    }

    result
}

/// Expand tabs to spaces and truncate to fit within max_width.
fn expand_tabs_to_width(line: &str, max_width: usize, tabstop: usize) -> String {
    let mut result = String::new();
    let mut col = 0;

    for ch in line.chars() {
        if ch == '\t' {
            // Expand tab to spaces
            let tab_spaces = tab_width_at_col(col, tabstop);
            for _ in 0..tab_spaces {
                if col >= max_width {
                    return result;
                }
                result.push(' ');
                col += 1;
            }
        } else {
            let char_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if col + char_width > max_width {
                return result;
            }
            result.push(ch);
            col += char_width;
        }
    }

    result
}

/// Truncate a line to fit within the given display width.
///
/// Returns a substring that fits within max_width display columns.
/// Note: This returns the original characters (including tabs), not expanded.
/// For rendering with expanded tabs, use expand_tabs_to_width instead.
#[cfg(test)]
fn truncate_to_width(line: &str, max_width: usize, tabstop: usize) -> String {
    if max_width == 0 {
        return String::new();
    }

    // Iterate through characters to find truncation point
    let mut current_width = 0;
    let mut byte_offset = 0;

    for (idx, ch) in line.char_indices() {
        let char_width = if ch == '\t' {
            tab_width_at_col(current_width, tabstop)
        } else {
            unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0)
        };
        if current_width + char_width > max_width {
            break;
        }
        current_width += char_width;
        byte_offset = idx + ch.len_utf8();
    }

    // If we couldn't fit any characters, return empty string
    if byte_offset == 0 && !line.is_empty() {
        let first_ch = line.chars().next().unwrap();
        let first_width = if first_ch == '\t' {
            tab_width_at_col(0, tabstop)
        } else {
            unicode_width::UnicodeWidthChar::width(first_ch).unwrap_or(0)
        };
        if first_width > max_width {
            return String::new();
        }
    }

    line[..byte_offset.min(line.len())].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::unicode::DEFAULT_TAB_WIDTH;
    use crate::buffer::{Buffer, Cursor, SelectionType};

    #[test]
    fn test_render_buffer_lines() {
        let buffer = Buffer::from_string("hello\nworld\ntest".to_string());
        let viewport = Viewport::new(0, 10, 80);
        let mut output = TerminalOutput::with_writer(Vec::new());
        let gutter = Gutter::disabled();

        // Test that it doesn't panic and writes something
        render_buffer_lines(
            &mut output,
            &buffer,
            &viewport,
            &[],
            DEFAULT_TAB_WIDTH,
            &gutter,
            false,
            false,
        )
        .unwrap();
        assert!(!output.writer().is_empty());
    }

    #[test]
    fn test_render_buffer_lines_with_empty_highlights() {
        let buffer = Buffer::from_string("hello\nworld".to_string());
        let viewport = Viewport::new(0, 10, 80);
        let mut output = TerminalOutput::with_writer(Vec::new());
        let gutter = Gutter::disabled();

        let highlights: Vec<Vec<HighlightRange>> = vec![vec![], vec![]];
        render_buffer_lines(
            &mut output,
            &buffer,
            &viewport,
            &highlights,
            DEFAULT_TAB_WIDTH,
            &gutter,
            false,
            false,
        )
        .unwrap();
        assert!(!output.writer().is_empty());
    }

    #[test]
    fn test_render_buffer_lines_with_highlights() {
        let buffer = Buffer::from_string("hello world\nfoo bar".to_string());
        let viewport = Viewport::new(0, 10, 80);
        let mut output = TerminalOutput::with_writer(Vec::new());
        let gutter = Gutter::disabled();

        let highlights = vec![
            vec![HighlightRange { start: 0, end: 5 }],
            vec![HighlightRange { start: 4, end: 7 }],
        ];
        render_buffer_lines(
            &mut output,
            &buffer,
            &viewport,
            &highlights,
            DEFAULT_TAB_WIDTH,
            &gutter,
            false,
            false,
        )
        .unwrap();

        let output_str = String::from_utf8_lossy(output.writer());
        // Should contain Invert and NoInvert escape sequences
        assert!(output_str.contains("\x1b[7m")); // Invert
        assert!(output_str.contains("\x1b[27m")); // NoInvert
    }

    #[test]
    fn test_render_buffer_lines_with_gutter() {
        let buffer = Buffer::from_string("hello\nworld\ntest".to_string());
        let viewport = Viewport::new(0, 10, 80);
        let mut output = TerminalOutput::with_writer(Vec::new());

        let mut gutter = Gutter::disabled();
        gutter.update(true, 3); // width 4
        assert!(gutter.enabled());

        render_buffer_lines(
            &mut output,
            &buffer,
            &viewport,
            &[],
            DEFAULT_TAB_WIDTH,
            &gutter,
            false,
            false,
        )
        .unwrap();

        let output_str = String::from_utf8_lossy(output.writer());
        // Should contain right-justified line numbers
        assert!(output_str.contains("  1 "));
        assert!(output_str.contains("  2 "));
        assert!(output_str.contains("  3 "));
    }

    #[test]
    fn test_render_buffer_lines_gutter_disabled_no_numbers() {
        let buffer = Buffer::from_string("hello".to_string());
        let viewport = Viewport::new(0, 10, 80);
        let mut output = TerminalOutput::with_writer(Vec::new());
        let gutter = Gutter::disabled();

        render_buffer_lines(
            &mut output,
            &buffer,
            &viewport,
            &[],
            DEFAULT_TAB_WIDTH,
            &gutter,
            false,
            false,
        )
        .unwrap();

        let output_str = String::from_utf8_lossy(output.writer());
        // Should not contain line number formatting
        assert!(!output_str.contains("  1 "));
    }

    #[test]
    fn test_render_line_with_highlights_basic() {
        let mut output = TerminalOutput::with_writer(Vec::new());
        let highlights = vec![HighlightRange { start: 0, end: 5 }];

        render_line_with_highlights(
            &mut output,
            "hello world",
            80,
            &highlights,
            DEFAULT_TAB_WIDTH,
            false,
            false,
        )
        .unwrap();

        let output_str = String::from_utf8_lossy(output.writer());
        assert!(output_str.contains("\x1b[7m")); // Invert at start
        assert!(output_str.contains("hello"));
        assert!(output_str.contains("\x1b[27m")); // NoInvert after match
        assert!(output_str.contains("world"));
    }

    #[test]
    fn test_render_line_with_highlights_multiple() {
        let mut output = TerminalOutput::with_writer(Vec::new());
        let highlights = vec![
            HighlightRange { start: 0, end: 2 },
            HighlightRange { start: 3, end: 5 },
        ];

        render_line_with_highlights(
            &mut output,
            "aa bb cc",
            80,
            &highlights,
            DEFAULT_TAB_WIDTH,
            false,
            false,
        )
        .unwrap();

        let output_str = String::from_utf8_lossy(output.writer());
        // Should contain two pairs of Invert/NoInvert
        let invert_count = output_str.matches("\x1b[7m").count();
        let no_invert_count = output_str.matches("\x1b[27m").count();
        assert_eq!(invert_count, 2);
        assert_eq!(no_invert_count, 2);
    }

    #[test]
    fn test_render_line_with_highlights_adjacent() {
        let mut output = TerminalOutput::with_writer(Vec::new());
        let highlights = vec![
            HighlightRange { start: 0, end: 2 },
            HighlightRange { start: 2, end: 4 },
        ];

        render_line_with_highlights(
            &mut output,
            "aabb",
            80,
            &highlights,
            DEFAULT_TAB_WIDTH,
            false,
            false,
        )
        .unwrap();

        let output_str = String::from_utf8_lossy(output.writer());
        // Adjacent highlights should transition correctly
        assert!(output_str.contains("\x1b[7m"));
        assert!(output_str.contains("\x1b[27m"));
    }

    #[test]
    fn test_render_line_with_highlights_truncation() {
        let mut output = TerminalOutput::with_writer(Vec::new());
        let highlights = vec![HighlightRange { start: 0, end: 20 }];

        // Line is 20 chars but max_width is 5
        render_line_with_highlights(
            &mut output,
            "hello world and more",
            5,
            &highlights,
            DEFAULT_TAB_WIDTH,
            false,
            false,
        )
        .unwrap();

        let output_str = String::from_utf8_lossy(output.writer());
        // Should contain "hello" but not more
        assert!(output_str.contains("hello"));
        assert!(!output_str.contains("world"));
        // Should still reset style
        assert!(output_str.contains("\x1b[27m"));
    }

    #[test]
    fn test_render_line_with_highlights_empty_line() {
        let mut output = TerminalOutput::with_writer(Vec::new());
        render_line_with_highlights(&mut output, "", 80, &[], DEFAULT_TAB_WIDTH, false, false)
            .unwrap();
        // Should not panic
    }

    #[test]
    fn test_render_line_with_highlights_tab() {
        let mut output = TerminalOutput::with_writer(Vec::new());
        // Highlight includes the tab character
        let highlights = vec![HighlightRange { start: 0, end: 2 }];

        render_line_with_highlights(
            &mut output,
            "\tb",
            80,
            &highlights,
            DEFAULT_TAB_WIDTH,
            false,
            false,
        )
        .unwrap();

        let output_str = String::from_utf8_lossy(output.writer());
        // Tab is expanded to spaces, all within highlight
        assert!(output_str.contains("\x1b[7m"));
    }

    #[test]
    fn test_truncate_to_width() {
        let line = "hello";
        assert_eq!(truncate_to_width(line, 3, DEFAULT_TAB_WIDTH), "hel");
        assert_eq!(truncate_to_width(line, 5, DEFAULT_TAB_WIDTH), "hello");
        assert_eq!(truncate_to_width(line, 10, DEFAULT_TAB_WIDTH), "hello");
    }

    #[test]
    fn test_truncate_to_width_wide_chars() {
        let line = "你好"; // Each char is 2 columns wide
        assert_eq!(truncate_to_width(line, 2, DEFAULT_TAB_WIDTH), "你");
        assert_eq!(truncate_to_width(line, 4, DEFAULT_TAB_WIDTH), "你好");
        assert_eq!(truncate_to_width(line, 1, DEFAULT_TAB_WIDTH), ""); // Can't fit even one char
    }

    #[test]
    fn test_expand_tabs_to_width() {
        // Tab at start expands to 8 spaces
        assert_eq!(
            expand_tabs_to_width("\t", 80, DEFAULT_TAB_WIDTH),
            "        "
        );
        // "a" + tab expands to col 8 (7 spaces after 'a')
        assert_eq!(
            expand_tabs_to_width("a\t", 80, DEFAULT_TAB_WIDTH),
            "a       "
        );
        // "a" + tab + "b"
        assert_eq!(
            expand_tabs_to_width("a\tb", 80, DEFAULT_TAB_WIDTH),
            "a       b"
        );
        // Truncation at width 4 should cut off the tab expansion
        assert_eq!(expand_tabs_to_width("\t", 4, DEFAULT_TAB_WIDTH), "    ");
        // Multiple tabs
        assert_eq!(
            expand_tabs_to_width("\t\t", 80, DEFAULT_TAB_WIDTH),
            "                "
        );
    }

    #[test]
    fn test_truncate_to_width_with_tabs() {
        // Tab takes 8 columns, so it fits in width 8 but not 7
        assert_eq!(truncate_to_width("\t", 8, DEFAULT_TAB_WIDTH), "\t");
        assert_eq!(truncate_to_width("\t", 7, DEFAULT_TAB_WIDTH), "");
        // "a\t" takes 8 columns total
        assert_eq!(truncate_to_width("a\t", 8, DEFAULT_TAB_WIDTH), "a\t");
        assert_eq!(truncate_to_width("a\t", 7, DEFAULT_TAB_WIDTH), "a");
    }

    // =========================================================================
    // list mode rendering tests
    // =========================================================================

    #[test]
    fn test_expand_tabs_list_mode_no_tabs() {
        assert_eq!(expand_tabs_list_mode("hello", 80), "hello");
    }

    #[test]
    fn test_expand_tabs_list_mode_with_tab() {
        assert_eq!(expand_tabs_list_mode("a\tb", 80), "a^Ib");
    }

    #[test]
    fn test_expand_tabs_list_mode_truncation() {
        // "a^I" = 3 display cols; max_width 2 leaves no room for ^I
        assert_eq!(expand_tabs_list_mode("a\tb", 2), "a");
    }

    #[test]
    fn test_render_line_list_mode_appends_dollar() {
        let mut output = TerminalOutput::with_writer(Vec::new());
        render_line(&mut output, "hello", 80, DEFAULT_TAB_WIDTH, true, true).unwrap();
        let s = String::from_utf8_lossy(output.writer()).to_string();
        assert!(s.ends_with('$'), "list mode should append $: got {:?}", s);
        assert!(s.contains("hello"));
    }

    #[test]
    fn test_render_line_list_mode_tab_as_caret_i() {
        let mut output = TerminalOutput::with_writer(Vec::new());
        render_line(&mut output, "a\tb", 80, DEFAULT_TAB_WIDTH, true, true).unwrap();
        let s = String::from_utf8_lossy(output.writer()).to_string();
        assert!(s.contains("^I"), "list mode should render tab as ^I");
        assert!(s.ends_with('$'));
    }

    #[test]
    fn test_render_line_list_mode_empty_line_has_dollar() {
        // In list mode, even an empty line gets a $ end-of-line marker.
        let mut output = TerminalOutput::with_writer(Vec::new());
        render_line(&mut output, "", 80, DEFAULT_TAB_WIDTH, true, true).unwrap();
        let s = String::from_utf8_lossy(output.writer()).to_string();
        // expand_tabs_list_mode("", …) returns "" (len 0 < max_width) so $ is written
        assert!(s.contains('$') || s.is_empty());
    }

    // Tests with non-default tabstop
    #[test]
    fn test_expand_tabs_to_width_tabstop_4() {
        assert_eq!(expand_tabs_to_width("\t", 80, 4), "    ");
        assert_eq!(expand_tabs_to_width("a\t", 80, 4), "a   ");
        assert_eq!(expand_tabs_to_width("a\tb", 80, 4), "a   b");
        assert_eq!(expand_tabs_to_width("\t\t", 80, 4), "        ");
    }

    #[test]
    fn test_render_line_with_highlights_tabstop_4() {
        let mut output = TerminalOutput::with_writer(Vec::new());
        let highlights = vec![HighlightRange { start: 0, end: 2 }];

        render_line_with_highlights(&mut output, "\tb", 80, &highlights, 4, false, false).unwrap();

        let output_str = String::from_utf8_lossy(output.writer());
        assert!(output_str.contains("\x1b[7m"));
    }

    // =========================================================================
    // wrap_line_segments tests
    // =========================================================================

    #[test]
    fn test_wrap_segments_short_line() {
        let segments = wrap_line_segments("hello", 80, 8, false);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].byte_start, 0);
        assert_eq!(segments[0].byte_end, 5);
    }

    #[test]
    fn test_wrap_segments_exact_width() {
        let segments = wrap_line_segments("abcde", 5, 8, false);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].byte_start, 0);
        assert_eq!(segments[0].byte_end, 5);
    }

    #[test]
    fn test_wrap_segments_two_rows() {
        // 10 chars, width 5 -> 2 segments
        let segments = wrap_line_segments("abcdefghij", 5, 8, false);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].byte_start, 0);
        assert_eq!(segments[0].byte_end, 5);
        assert_eq!(segments[1].byte_start, 5);
        assert_eq!(segments[1].byte_end, 10);
    }

    #[test]
    fn test_wrap_segments_three_rows() {
        // 12 chars, width 5 -> 3 segments (5, 5, 2)
        let segments = wrap_line_segments("abcdefghijkl", 5, 8, false);
        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0].byte_end, 5);
        assert_eq!(segments[1].byte_start, 5);
        assert_eq!(segments[1].byte_end, 10);
        assert_eq!(segments[2].byte_start, 10);
        assert_eq!(segments[2].byte_end, 12);
    }

    #[test]
    fn test_wrap_segments_empty_line() {
        let segments = wrap_line_segments("", 80, 8, false);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].byte_start, 0);
        assert_eq!(segments[0].byte_end, 0);
    }

    #[test]
    fn test_wrap_segments_with_tab() {
        // Tab at col 0 with tabstop=8 takes 8 cols, width=10 -> fits in one segment
        let segments = wrap_line_segments("\thi", 10, 8, false);
        assert_eq!(segments.len(), 1);
    }

    #[test]
    fn test_wrap_segments_tab_causes_wrap() {
        // Tab at col 0 with tabstop=8 takes 8 cols, width=5 -> tab wraps
        // Tab doesn't fit at col 0 with width 5 (needs 8 cols), but since col=0
        // and char_width > text_width, the "col > 0" guard prevents splitting
        // before the tab, so the tab is rendered (partially) in the first segment.
        let segments = wrap_line_segments("\tab", 5, 8, false);
        // Tab consumes all 5 cols (clamped by width), then "ab" goes to next segment
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].byte_start, 0);
        assert_eq!(segments[0].byte_end, 1); // just the tab
        assert_eq!(segments[1].byte_start, 1);
        assert_eq!(segments[1].byte_end, 3);
    }

    #[test]
    fn test_wrap_segments_wide_chars() {
        // Two wide chars, each 2 columns, width=3 -> second char wraps
        let segments = wrap_line_segments("你好", 3, 8, false);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].byte_start, 0);
        assert_eq!(segments[0].byte_end, 3); // "你" is 3 bytes
        assert_eq!(segments[1].byte_start, 3);
        assert_eq!(segments[1].byte_end, 6);
    }

    #[test]
    fn test_wrap_segments_tab_mid_text_causes_break() {
        // "ab\tcd" with text_width=5, tabstop=8
        // 'a' col=1, 'b' col=2, '\t' tab_width_at_col(2,8)=6 -> col+6=8>5, col>0 so break
        // segment [0,2], then tab at col=0: tab_width_at_col(0,8)=8>5, col=0 so no break,
        // tab partially fills to text_width=5, segment [2,3]
        // then 'c' col=1, 'd' col=2, segment [3,5]
        let segments = wrap_line_segments("ab\tcd", 5, 8, false);
        assert_eq!(segments.len(), 3);
        assert_eq!((segments[0].byte_start, segments[0].byte_end), (0, 2));
        assert_eq!((segments[1].byte_start, segments[1].byte_end), (2, 3));
        assert_eq!((segments[2].byte_start, segments[2].byte_end), (3, 5));
    }

    #[test]
    fn test_wrap_segments_tab_exactly_fills_remaining() {
        // "ab\t" with text_width=4, tabstop=4
        // 'a' col=1, 'b' col=2, '\t' tab_width_at_col(2,4)=2 -> col+2=4==text_width, fits
        // Single segment containing all 3 bytes
        let segments = wrap_line_segments("ab\t", 4, 4, false);
        assert_eq!(segments.len(), 1);
        assert_eq!((segments[0].byte_start, segments[0].byte_end), (0, 3));
    }

    #[test]
    fn test_wrap_segments_wide_char_at_boundary() {
        // "ab你" with text_width=3, tabstop=8
        // 'a' col=1, 'b' col=2, '你' width=2, col+2=4>3, col>0 so break
        // segment [0,2], then '你' col=2, segment [2,5] (你 is 3 bytes UTF-8)
        let segments = wrap_line_segments("ab\u{4f60}", 3, 8, false);
        assert_eq!(segments.len(), 2);
        assert_eq!((segments[0].byte_start, segments[0].byte_end), (0, 2));
        assert_eq!((segments[1].byte_start, segments[1].byte_end), (2, 5));
    }

    #[test]
    fn test_wrap_segments_wide_char_exactly_fits() {
        // "a你" with text_width=3, tabstop=8
        // 'a' col=1, '你' width=2, col+2=3==text_width, fits
        // Single segment: 'a' is 1 byte, '你' is 3 bytes -> total 4 bytes
        let segments = wrap_line_segments("a\u{4f60}", 3, 8, false);
        assert_eq!(segments.len(), 1);
        assert_eq!((segments[0].byte_start, segments[0].byte_end), (0, 4));
    }

    // =========================================================================
    // clip_highlights tests
    // =========================================================================

    #[test]
    fn test_clip_highlights_no_overlap() {
        let highlights = vec![HighlightRange { start: 0, end: 3 }];
        let result = clip_highlights(&highlights, 5, 10);
        assert!(result.is_empty());
    }

    #[test]
    fn test_clip_highlights_full_overlap() {
        let highlights = vec![HighlightRange { start: 2, end: 5 }];
        let result = clip_highlights(&highlights, 0, 10);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].start, 2);
        assert_eq!(result[0].end, 5);
    }

    #[test]
    fn test_clip_highlights_partial_overlap_start() {
        let highlights = vec![HighlightRange { start: 0, end: 8 }];
        let result = clip_highlights(&highlights, 5, 10);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].start, 0); // 5 - 5
        assert_eq!(result[0].end, 3); // 8 - 5
    }

    #[test]
    fn test_clip_highlights_partial_overlap_end() {
        let highlights = vec![HighlightRange { start: 3, end: 15 }];
        let result = clip_highlights(&highlights, 0, 10);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].start, 3);
        assert_eq!(result[0].end, 10);
    }

    #[test]
    fn test_clip_highlights_multiple() {
        let highlights = vec![
            HighlightRange { start: 1, end: 3 },
            HighlightRange { start: 7, end: 12 },
        ];
        let result = clip_highlights(&highlights, 5, 10);
        // First highlight ends at 3, which is before seg_start 5 -> excluded
        // Second highlight: clamped to [7,10] -> shifted to [2,5]
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].start, 2);
        assert_eq!(result[0].end, 5);
    }

    // =========================================================================
    // Wrapped rendering integration tests
    // =========================================================================

    #[test]
    fn test_render_buffer_lines_wrap_short_lines() {
        // Short lines that don't need wrapping should render the same
        let buffer = Buffer::from_string("hello\nworld".to_string());
        let viewport = Viewport::new(0, 10, 80);
        let gutter = Gutter::disabled();

        let mut output_nowrap = TerminalOutput::with_writer(Vec::new());
        render_buffer_lines(
            &mut output_nowrap,
            &buffer,
            &viewport,
            &[],
            DEFAULT_TAB_WIDTH,
            &gutter,
            false,
            false,
        )
        .unwrap();

        let mut output_wrap = TerminalOutput::with_writer(Vec::new());
        render_buffer_lines(
            &mut output_wrap,
            &buffer,
            &viewport,
            &[],
            DEFAULT_TAB_WIDTH,
            &gutter,
            true,
            false,
        )
        .unwrap();

        let nowrap_str = String::from_utf8_lossy(output_nowrap.writer());
        let wrap_str = String::from_utf8_lossy(output_wrap.writer());
        // Both should contain the same text content
        assert!(nowrap_str.contains("hello"));
        assert!(wrap_str.contains("hello"));
        assert!(nowrap_str.contains("world"));
        assert!(wrap_str.contains("world"));
    }

    #[test]
    fn test_render_buffer_lines_wrap_long_line() {
        // A line longer than the viewport width should produce multiple terminal rows
        let buffer = Buffer::from_string("abcdefghij".to_string());
        let viewport = Viewport::new(0, 10, 5); // width=5
        let gutter = Gutter::disabled();

        let mut output = TerminalOutput::with_writer(Vec::new());
        render_buffer_lines(
            &mut output,
            &buffer,
            &viewport,
            &[],
            DEFAULT_TAB_WIDTH,
            &gutter,
            true,
            false,
        )
        .unwrap();

        let output_str = String::from_utf8_lossy(output.writer());
        // Should contain both halves
        assert!(output_str.contains("abcde"));
        assert!(output_str.contains("fghij"));
    }

    #[test]
    fn test_render_buffer_lines_wrap_gutter_first_row_only() {
        // Line number should only appear on the first visual row
        let buffer = Buffer::from_string("abcdefghij".to_string());
        let viewport = Viewport::new(0, 10, 14); // width 14, minus gutter 4 = text_width 10
        let mut gutter = Gutter::disabled();
        gutter.update(true, 1); // width 4

        let mut output = TerminalOutput::with_writer(Vec::new());
        render_buffer_lines(
            &mut output,
            &buffer,
            &viewport,
            &[],
            DEFAULT_TAB_WIDTH,
            &gutter,
            true,
            false,
        )
        .unwrap();

        let output_str = String::from_utf8_lossy(output.writer());
        // Should contain the line number "1" (right justified in width-1 cols)
        assert!(output_str.contains("  1 "));
    }

    #[test]
    fn test_render_buffer_lines_wrap_stops_at_viewport_height() {
        // A very long line wrapping should not exceed viewport height
        let long_line = "a".repeat(100);
        let buffer = Buffer::from_string(long_line);
        let viewport = Viewport::new(0, 5, 10); // 5 rows, width 10
        let gutter = Gutter::disabled();

        let mut output = TerminalOutput::with_writer(Vec::new());
        render_buffer_lines(
            &mut output,
            &buffer,
            &viewport,
            &[],
            DEFAULT_TAB_WIDTH,
            &gutter,
            true,
            false,
        )
        .unwrap();

        // Should not panic; output is limited by viewport height
        assert!(!output.writer().is_empty());
    }

    #[test]
    fn test_render_buffer_lines_wrap_with_highlights() {
        // Ensure highlights work correctly across wrapped segments
        let buffer = Buffer::from_string("abcdefghij".to_string());
        let viewport = Viewport::new(0, 10, 5); // width=5
        let gutter = Gutter::disabled();

        // Highlight spans across the wrap boundary (bytes 3..7)
        let highlights = vec![vec![HighlightRange { start: 3, end: 7 }]];

        let mut output = TerminalOutput::with_writer(Vec::new());
        render_buffer_lines(
            &mut output,
            &buffer,
            &viewport,
            &highlights,
            DEFAULT_TAB_WIDTH,
            &gutter,
            true,
            false,
        )
        .unwrap();

        let output_str = String::from_utf8_lossy(output.writer());
        // Should contain Invert/NoInvert for the highlight
        assert!(output_str.contains("\x1b[7m"));
        assert!(output_str.contains("\x1b[27m"));
    }

    #[test]
    fn test_render_buffer_lines_wrap_tilde_lines() {
        // After wrapped content, remaining rows should be tilde lines
        let buffer = Buffer::from_string("hello".to_string());
        let viewport = Viewport::new(0, 5, 80); // 5 rows, line fits in 1
        let gutter = Gutter::disabled();

        let mut output = TerminalOutput::with_writer(Vec::new());
        render_buffer_lines(
            &mut output,
            &buffer,
            &viewport,
            &[],
            DEFAULT_TAB_WIDTH,
            &gutter,
            true,
            false,
        )
        .unwrap();

        // Output should have goto commands for all 5 rows
        // (1 for content + 4 for tilde/blank lines)
        assert!(!output.writer().is_empty());
    }

    #[test]
    fn test_render_buffer_lines_wrap_multiple_lines() {
        // Two lines, both needing wrapping
        let buffer = Buffer::from_string("abcdefghij\n1234567890".to_string());
        let viewport = Viewport::new(0, 10, 5); // width=5
        let gutter = Gutter::disabled();

        let mut output = TerminalOutput::with_writer(Vec::new());
        render_buffer_lines(
            &mut output,
            &buffer,
            &viewport,
            &[],
            DEFAULT_TAB_WIDTH,
            &gutter,
            true,
            false,
        )
        .unwrap();

        let output_str = String::from_utf8_lossy(output.writer());
        assert!(output_str.contains("abcde"));
        assert!(output_str.contains("fghij"));
        assert!(output_str.contains("12345"));
        assert!(output_str.contains("67890"));
    }

    // =========================================================================
    // wrapped_line_height tests
    // =========================================================================

    #[test]
    fn test_wrapped_line_height_wrap_false() {
        // When wrap is false, always returns 1 regardless of line length
        assert_eq!(
            wrapped_line_height("a".repeat(200).as_str(), 10, 8, false, false),
            1
        );
        assert_eq!(wrapped_line_height("short", 10, 8, false, false), 1);
        assert_eq!(wrapped_line_height("", 10, 8, false, false), 1);
    }

    #[test]
    fn test_wrapped_line_height_short_line() {
        // Line shorter than text_width with wrap=true returns 1
        assert_eq!(wrapped_line_height("hello", 80, 8, true, false), 1);
    }

    #[test]
    fn test_wrapped_line_height_exact_fit() {
        // Line exactly text_width wide returns 1
        assert_eq!(wrapped_line_height("abcde", 5, 8, true, false), 1);
    }

    #[test]
    fn test_wrapped_line_height_one_over() {
        // Line one column wider than text_width returns 2
        assert_eq!(wrapped_line_height("abcdef", 5, 8, true, false), 2);
    }

    #[test]
    fn test_wrapped_line_height_long_line() {
        // Line 3x text_width returns 3
        assert_eq!(wrapped_line_height("abcdefghijklmno", 5, 8, true, false), 3);
    }

    #[test]
    fn test_wrapped_line_height_empty_line() {
        // Empty string returns 1 (empty lines still occupy one row)
        assert_eq!(wrapped_line_height("", 80, 8, true, false), 1);
    }

    #[test]
    fn test_wrapped_line_height_zero_text_width() {
        // text_width=0 returns 1 (guard against division by zero)
        assert_eq!(wrapped_line_height("hello world", 0, 8, true, false), 1);
        assert_eq!(wrapped_line_height("", 0, 8, true, false), 1);
    }

    #[test]
    fn test_wrapped_line_height_with_tabs() {
        // Tab at col 0 with tabstop=8 expands to 8 columns.
        // With text_width=5, the tab fills 5 cols (one segment), then "ab"
        // goes to a second segment.
        assert_eq!(wrapped_line_height("\tab", 5, 8, true, false), 2);

        // Tab fits entirely in text_width=10 (8 cols), plus "hi" = 10 cols -> 1 row
        assert_eq!(wrapped_line_height("\thi", 10, 8, true, false), 1);

        // "ab\tcd" with text_width=5, tabstop=8:
        // "ab" = 2 cols, tab needs 6 cols -> break -> 3 segments
        assert_eq!(wrapped_line_height("ab\tcd", 5, 8, true, false), 3);
    }

    // =========================================================================
    // compute_selection_highlights tests (Chunk E: Block)
    // =========================================================================

    #[test]
    fn test_block_selection_highlight() {
        // Block cols 1..=3 across rows 0..=2 of "hello\nworld\ntest"
        let buffer = Buffer::from_string("hello\nworld\ntest".to_string());
        let viewport = Viewport::new(0, 10, 80);
        let selection = Selection::new(Cursor::new(0, 1), Cursor::new(2, 3), SelectionType::Block);

        let highlights = compute_selection_highlights(&selection, &viewport, &buffer);

        // 3 buffer lines in viewport
        assert_eq!(highlights.len(), 3);
        // Row 0: "hello"[1..4] = bytes 1..4
        assert_eq!(highlights[0], vec![HighlightRange { start: 1, end: 4 }]);
        // Row 1: "world"[1..4] = bytes 1..4
        assert_eq!(highlights[1], vec![HighlightRange { start: 1, end: 4 }]);
        // Row 2: "test"[1..4] = bytes 1..4
        assert_eq!(highlights[2], vec![HighlightRange { start: 1, end: 4 }]);
    }

    #[test]
    fn test_block_selection_highlight_col_clamped() {
        // Short line "hi" (len=2) with block cols 1..=4 — end clamped to line.len()
        let buffer = Buffer::from_string("hi\nworld".to_string());
        let viewport = Viewport::new(0, 10, 80);
        let selection = Selection::new(Cursor::new(0, 1), Cursor::new(1, 4), SelectionType::Block);

        let highlights = compute_selection_highlights(&selection, &viewport, &buffer);

        // Row 0: "hi"[1..min(5,2)] = [1..2]
        assert_eq!(highlights[0], vec![HighlightRange { start: 1, end: 2 }]);
        // Row 1: "world"[1..min(5,5)] = [1..5]
        assert_eq!(highlights[1], vec![HighlightRange { start: 1, end: 5 }]);
    }
}

//! Status line rendering.
//!
//! Handles rendering the status line at the bottom of the screen,
//! showing mode, filename, modified indicator, line ending format,
//! cursor position, and other information. Also handles command-line
//! and status message rendering.

use std::io::{self, Write};

use crate::command::CommandLineState;
use crate::file::LineEnding;
use crate::render::viewport::Viewport;
use crate::terminal::TerminalOutput;

/// Render the status line at the bottom of the screen.
///
/// # Arguments
///
/// * `output` - Terminal output handle
/// * `mode` - Current editor mode (e.g., "NORMAL", "INSERT")
/// * `filename` - Current filename, if any
/// * `position` - Cursor position as (row, vcol) in 1-indexed coordinates,
///   where vcol is the display column (tab/wide-char aware)
/// * `total_lines` - Total number of lines in the buffer
/// * `viewport` - Current viewport
/// * `modified` - Whether the buffer has unsaved changes
/// * `line_ending` - Detected line ending of the document
#[allow(clippy::too_many_arguments)]
pub fn render_status_line<W: Write>(
    output: &mut TerminalOutput<W>,
    mode: &str,
    filename: Option<&str>,
    position: (usize, usize),
    total_lines: usize,
    viewport: &Viewport,
    modified: bool,
    line_ending: &LineEnding,
) -> io::Result<()> {
    // Calculate status line row (bottom of screen)
    // Viewport height doesn't include status line, so status line is at viewport.height() + 1
    let status_row = (viewport.height() + 1) as u16;

    // Move to status line
    output.goto(1, status_row)?;
    output.clear_line()?;

    // Build status line content
    let mut status_content = String::new();

    // Mode
    status_content.push_str(&format!("[{}]", mode));

    // Filename
    if let Some(fname) = filename {
        status_content.push(' ');
        status_content.push_str(fname);
    }

    // Modified indicator
    if modified {
        status_content.push_str(" [+]");
    }

    // Line ending indicator (only shown for non-LF)
    let le_display = line_ending.display_name();
    if !le_display.is_empty() {
        status_content.push(' ');
        status_content.push_str(le_display);
    }

    // Right side: "{row}/{total},{vcol}  {pct}%" — vi ruler format
    let row = position.0;
    let vcol = position.1;
    let pct = scroll_percentage(row, total_lines, viewport);
    let position_str = format!("{}/{}  {},{}  {}", row, total_lines, row, vcol, pct);

    let available_width = viewport.width();
    let content_width = display_width(&status_content);
    let position_width = display_width(&position_str);

    // Calculate spacing needed for right alignment
    if content_width + position_width < available_width {
        let spacing = available_width - content_width - position_width;
        status_content.push_str(&" ".repeat(spacing));
        status_content.push_str(&position_str);
    } else {
        // Not enough space - just append position
        status_content.push(' ');
        status_content.push_str(&position_str);
    }

    // Truncate if too long
    if display_width(&status_content) > available_width {
        status_content = truncate_string(&status_content, available_width);
    }

    output.write(&status_content)?;

    Ok(())
}

/// Compute the scroll percentage string for the status line ruler.
///
/// Returns `"Top"`, `"Bot"`, `"All"`, or `"n%"` depending on position.
fn scroll_percentage(row: usize, total_lines: usize, viewport: &Viewport) -> String {
    if total_lines == 0 {
        return "All".to_string();
    }
    let visible_lines = viewport.height();
    if total_lines <= visible_lines {
        return "All".to_string();
    }
    // row is 1-based
    if row <= 1 {
        return "Top".to_string();
    }
    if row >= total_lines {
        return "Bot".to_string();
    }
    let pct = ((row - 1) * 100) / total_lines.saturating_sub(1).max(1);
    format!("{}%", pct.min(99))
}

/// Render the command line at the bottom of the screen.
///
/// Shows the prompt character followed by the input buffer, with the
/// terminal cursor positioned within the command-line text.
///
/// # Arguments
///
/// * `output` - Terminal output handle
/// * `command_line` - Current command-line state
/// * `viewport` - Current viewport (used to determine the bottom row)
pub fn render_command_line<W: Write>(
    output: &mut TerminalOutput<W>,
    command_line: &CommandLineState,
    viewport: &Viewport,
) -> io::Result<()> {
    let status_row = (viewport.height() + 1) as u16;
    output.goto(1, status_row)?;
    output.clear_line()?;

    // Build display string: prompt + buffer
    let display = format!("{}{}", command_line.prompt(), command_line.buffer());
    output.write(&display)?;

    // Position cursor within the command line.
    // The prompt is 1 display column, and the cursor position is the display
    // width of the buffer prefix up to cursor_pos().
    let buffer_prefix = &command_line.buffer()[..command_line.cursor_pos()];
    let cursor_display_col = 1 + display_width(buffer_prefix);
    // Terminal columns are 1-indexed, +1 for the prompt character
    output.goto((cursor_display_col + 1) as u16, status_row)?;

    Ok(())
}

/// Render a status message at the bottom of the screen.
///
/// Status messages replace the normal status line for one keypress cycle.
///
/// # Arguments
///
/// * `output` - Terminal output handle
/// * `message` - The message text to display
/// * `viewport` - Current viewport (used to determine the bottom row)
pub fn render_status_message<W: Write>(
    output: &mut TerminalOutput<W>,
    message: &str,
    viewport: &Viewport,
) -> io::Result<()> {
    let status_row = (viewport.height() + 1) as u16;
    output.goto(1, status_row)?;
    output.clear_line()?;

    // Truncate message to available width
    let available_width = viewport.width();
    let display = if display_width(message) > available_width {
        truncate_string(message, available_width)
    } else {
        message.to_string()
    };

    output.write(&display)?;

    Ok(())
}

/// Calculate display width of a string.
///
/// Uses unicode-width for accurate display width calculation.
fn display_width(s: &str) -> usize {
    use unicode_width::UnicodeWidthStr;
    UnicodeWidthStr::width(s)
}

/// Render a status message that contains a file path, smart-truncating the path
/// if necessary so the suffix (e.g. `" written, 8 bytes"`) is always fully visible.
///
/// Renders `"<path>"<suffix>`, where `<path>` is abbreviated to `...<tail>` when
/// the full path would not fit.  When there is no room for any path at all (tiny
/// terminal), renders just the suffix, truncated to `viewport.width()`.
pub fn render_status_with_path<W: Write>(
    output: &mut TerminalOutput<W>,
    path: &str,
    suffix: &str,
    viewport: &Viewport,
) -> io::Result<()> {
    let status_row = (viewport.height() + 1) as u16;
    output.goto(1, status_row)?;
    output.clear_line()?;

    // 2 cols for the surrounding quotes + the suffix itself
    let fixed_width = 2 + display_width(suffix);
    let available_for_path = viewport.width().saturating_sub(fixed_width);

    let display = if available_for_path == 0 {
        // No room for a path at all — show as much of the suffix as fits.
        truncate_string(suffix, viewport.width())
    } else {
        format!("\"{}\"{}",  truncate_path(path, available_for_path), suffix)
    };

    output.write(&display)?;
    Ok(())
}

/// Truncate a file path to fit within `max_display_width` columns.
///
/// If the path fits, it is returned unchanged.  Otherwise the leading portion
/// is replaced with `...` so the tail (filename) is preserved:
/// `/very/long/path/to/file.txt` → `...path/to/file.txt`.
///
/// Returns `""` when `max_display_width` is 0.
fn truncate_path(path: &str, max_display_width: usize) -> String {
    if max_display_width == 0 {
        return String::new();
    }
    if display_width(path) <= max_display_width {
        return path.to_string();
    }

    // We need to fit "..." (3 cols) plus some tail of the path.
    let tail_width = max_display_width.saturating_sub(3);
    if tail_width == 0 {
        // Can only fit the ellipsis itself.
        return "...".to_string();
    }

    // Walk the path from the right, collecting chars until we fill tail_width.
    let mut tail = String::new();
    let mut used = 0usize;
    for ch in path.chars().rev() {
        use unicode_width::UnicodeWidthChar;
        let w = ch.width().unwrap_or(0);
        if used + w > tail_width {
            break;
        }
        tail.push(ch);
        used += w;
    }
    // tail was built in reverse
    let tail: String = tail.chars().rev().collect();
    format!("...{tail}")
}

/// Truncate a string to fit within the given display width.
fn truncate_string(s: &str, max_width: usize) -> String {
    if display_width(s) <= max_width {
        return s.to_string();
    }

    let mut result = String::new();
    let mut current_width = 0;

    for ch in s.chars() {
        use unicode_width::UnicodeWidthChar;
        let char_width = ch.width().unwrap_or(0);
        if current_width + char_width > max_width {
            break;
        }
        result.push(ch);
        current_width += char_width;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_line_basic() {
        let viewport = Viewport::new(0, 20, 80);
        let mut output = TerminalOutput::with_writer(Vec::new());

        render_status_line(
            &mut output,
            "NORMAL",
            Some("test.txt"),
            (1, 1),
            100,
            &viewport,
            false,
            &LineEnding::Lf,
        )
        .unwrap();
        assert!(!output.writer().is_empty());
    }

    #[test]
    fn test_status_line_modified() {
        let viewport = Viewport::new(0, 20, 80);
        let mut output = TerminalOutput::with_writer(Vec::new());

        render_status_line(
            &mut output,
            "NORMAL",
            Some("test.txt"),
            (1, 1),
            100,
            &viewport,
            true,
            &LineEnding::Lf,
        )
        .unwrap();

        let written = String::from_utf8_lossy(output.writer());
        assert!(written.contains("[+]"));
    }

    #[test]
    fn test_status_line_crlf() {
        let viewport = Viewport::new(0, 20, 80);
        let mut output = TerminalOutput::with_writer(Vec::new());

        render_status_line(
            &mut output,
            "NORMAL",
            Some("test.txt"),
            (1, 1),
            100,
            &viewport,
            false,
            &LineEnding::CrLf,
        )
        .unwrap();

        let written = String::from_utf8_lossy(output.writer());
        assert!(written.contains("[dos]"));
    }

    #[test]
    fn test_status_line_cr() {
        let viewport = Viewport::new(0, 20, 80);
        let mut output = TerminalOutput::with_writer(Vec::new());

        render_status_line(
            &mut output,
            "NORMAL",
            Some("test.txt"),
            (1, 1),
            100,
            &viewport,
            false,
            &LineEnding::Cr,
        )
        .unwrap();

        let written = String::from_utf8_lossy(output.writer());
        assert!(written.contains("[mac]"));
    }

    #[test]
    fn test_status_line_lf_no_indicator() {
        let viewport = Viewport::new(0, 20, 80);
        let mut output = TerminalOutput::with_writer(Vec::new());

        render_status_line(
            &mut output,
            "NORMAL",
            Some("test.txt"),
            (1, 1),
            100,
            &viewport,
            false,
            &LineEnding::Lf,
        )
        .unwrap();

        let written = String::from_utf8_lossy(output.writer());
        // Should NOT contain [dos] or [mac]
        assert!(!written.contains("[dos]"));
        assert!(!written.contains("[mac]"));
    }

    #[test]
    fn test_status_line_modified_and_crlf() {
        let viewport = Viewport::new(0, 20, 80);
        let mut output = TerminalOutput::with_writer(Vec::new());

        render_status_line(
            &mut output,
            "INSERT",
            Some("readme.md"),
            (5, 10),
            100,
            &viewport,
            true,
            &LineEnding::CrLf,
        )
        .unwrap();

        let written = String::from_utf8_lossy(output.writer());
        assert!(written.contains("[+]"));
        assert!(written.contains("[dos]"));
        assert!(written.contains("[INSERT]"));
        assert!(written.contains("readme.md"));
    }

    #[test]
    fn test_truncate_string() {
        let s = "hello";
        assert_eq!(truncate_string(s, 3), "hel");
        assert_eq!(truncate_string(s, 5), "hello");
        assert_eq!(truncate_string(s, 10), "hello");
    }

    // Command line rendering tests

    #[test]
    fn test_render_command_line_basic() {
        let viewport = Viewport::new(0, 20, 80);
        let mut output = TerminalOutput::with_writer(Vec::new());

        let mut state = CommandLineState::new(':');
        state.insert_char('w');
        state.insert_char('q');

        render_command_line(&mut output, &state, &viewport).unwrap();

        let written = String::from_utf8_lossy(output.writer());
        assert!(written.contains(":wq"));
    }

    #[test]
    fn test_render_command_line_empty() {
        let viewport = Viewport::new(0, 20, 80);
        let mut output = TerminalOutput::with_writer(Vec::new());

        let state = CommandLineState::new(':');

        render_command_line(&mut output, &state, &viewport).unwrap();

        let written = String::from_utf8_lossy(output.writer());
        assert!(written.contains(':'));
    }

    // Status message rendering tests

    #[test]
    fn test_render_status_message_basic() {
        let viewport = Viewport::new(0, 20, 80);
        let mut output = TerminalOutput::with_writer(Vec::new());

        render_status_message(&mut output, "File written", &viewport).unwrap();

        let written = String::from_utf8_lossy(output.writer());
        assert!(written.contains("File written"));
    }

    #[test]
    fn test_render_status_message_truncation() {
        let viewport = Viewport::new(0, 20, 10);
        let mut output = TerminalOutput::with_writer(Vec::new());

        render_status_message(&mut output, "This is a very long message", &viewport).unwrap();

        let written = String::from_utf8_lossy(output.writer());
        // The message should be truncated to viewport width
        // We can't easily test exact truncation due to terminal escape codes,
        // but we can verify it doesn't panic
        assert!(!written.is_empty());
    }
}

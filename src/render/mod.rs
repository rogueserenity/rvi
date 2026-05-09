//! Rendering module for displaying editor content.
//!
//! This module handles all terminal output for the editor, including:
//! - Viewport management (which portion of buffer is visible)
//! - Buffer content rendering
//! - Cursor positioning
//! - Status line display
//! - Command-line rendering
//! - Status message display
//! - Search match highlighting (when `hlsearch` is enabled)
//! - Line number gutter (when `number` is enabled)
//! - Soft-wrapping of long lines (when `wrap` is enabled)

pub mod buffer_render;
pub mod cursor_render;
pub mod status_line;
pub mod viewport;

pub use viewport::Viewport;

use std::io::{self, Write};

use crate::buffer::{Buffer, Cursor, Selection};
use crate::command::CommandLineState;
use crate::error::RenderError;
use crate::file::LineEnding;
use crate::search::highlighter::{find_highlights, HighlightRange};
use crate::search::regex_utils::ViRegex;
use crate::terminal::{Size, TerminalOutput};

/// Document content passed to `render_full`.
pub struct RenderContext<'a> {
    /// The text buffer to render.
    pub buffer: &'a Buffer,
    /// Current cursor position.
    pub cursor: &'a Cursor,
    /// Display string for the current mode (e.g. `"NORMAL"`, `"INSERT"`).
    pub mode: &'a str,
    /// File name shown in the status line, if any.
    pub filename: Option<&'a str>,
    /// Whether the buffer has unsaved changes.
    pub modified: bool,
    /// Line ending style used by the file.
    pub line_ending: &'a LineEnding,
}

/// How a status message should be rendered.
pub enum StatusDisplay<'a> {
    /// Plain text rendered with simple truncation.
    Text(&'a str),
    /// A file path + suffix rendered as `"<path>" <suffix>` with smart path truncation.
    WithPath { path: &'a str, suffix: &'a str },
}

/// Options passed to `render_full` that don't describe the buffer content itself.
pub struct RenderOptions<'a> {
    /// The command line state, if currently in command-line mode.
    pub command_line: Option<&'a CommandLineState>,
    /// A status message to display in place of the normal status line.
    pub status_message: Option<StatusDisplay<'a>>,
    /// Search highlighting configuration.
    pub search: SearchDisplay<'a>,
    /// Tab stop width for display.
    pub tabstop: usize,
    /// Whether to show line numbers.
    pub number: bool,
    /// Whether to soft-wrap long lines.
    pub wrap: bool,
    /// Whether list mode is active (show tabs/EOL markers).
    pub list: bool,
    /// The active visual selection, if any.
    pub selection: Option<&'a Selection>,
}

/// Search-related display parameters for rendering.
///
/// Groups search highlighting state to reduce parameter passing.
#[derive(Debug, Clone, Copy)]
pub struct SearchDisplay<'a> {
    /// The compiled search regex, if any.
    pub regex: Option<&'a ViRegex>,
    /// Whether to highlight all search matches.
    pub hlsearch: bool,
}

impl<'a> SearchDisplay<'a> {
    /// Create a new search display configuration.
    pub fn new(regex: Option<&'a ViRegex>, hlsearch: bool) -> Self {
        Self { regex, hlsearch }
    }

    /// Create a disabled search display (no highlighting).
    pub fn disabled() -> Self {
        Self {
            regex: None,
            hlsearch: false,
        }
    }
}

/// Line number gutter state.
///
/// Tracks the current gutter width and enabled state to implement
/// hysteresis: the gutter expands eagerly but shrinks conservatively,
/// preventing visual jitter when editing near threshold boundaries.
#[derive(Debug)]
pub struct Gutter {
    /// Current gutter width in columns (0 when disabled).
    width: usize,
    /// Whether line numbers are enabled.
    enabled: bool,
}

impl Gutter {
    /// Create a disabled gutter (width 0).
    pub fn disabled() -> Self {
        Self {
            width: 0,
            enabled: false,
        }
    }

    /// Width of the gutter in columns. Returns 0 when disabled.
    pub fn width(&self) -> usize {
        self.width
    }

    /// Whether the gutter is currently enabled.
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Update gutter state based on the `number` setting and current line count.
    ///
    /// Called once per render cycle. When `number_enabled` is false, width is 0.
    /// When true, applies hysteresis rules:
    ///
    /// **Initial width (when enabling or first file open):**
    /// - < 750 lines: width 4
    /// - < 9,750 lines: width 5
    /// - < 99,750 lines: width 6
    /// - >= 99,750 lines: width 7
    ///
    /// **Expand thresholds (current width -> new width):**
    /// - crosses 1,000: 4 -> 5
    /// - crosses 10,000: 5 -> 6
    /// - crosses 100,000: 6 -> 7
    ///
    /// **Shrink thresholds (conservative):**
    /// - below 750: 5 -> 4
    /// - below 9,750: 6 -> 5
    /// - below 99,750: 7 -> 6
    pub fn update(&mut self, number_enabled: bool, line_count: usize) {
        if !number_enabled {
            self.width = 0;
            self.enabled = false;
            return;
        }

        if !self.enabled {
            // Transitioning from disabled to enabled: use initial thresholds
            self.width = Self::initial_width(line_count);
            self.enabled = true;
            return;
        }

        // Already enabled: apply hysteresis
        let new_width = match self.width {
            4 => {
                if line_count >= 1_000 {
                    5
                } else {
                    4
                }
            }
            5 => {
                if line_count >= 10_000 {
                    6
                } else if line_count < 750 {
                    4
                } else {
                    5
                }
            }
            6 => {
                if line_count >= 100_000 {
                    7
                } else if line_count < 9_750 {
                    5
                } else {
                    6
                }
            }
            7 => {
                if line_count < 99_750 {
                    6
                } else {
                    7
                }
            }
            _ => Self::initial_width(line_count),
        };
        self.width = new_width;
    }

    /// Compute the initial width for a given line count (no hysteresis).
    fn initial_width(line_count: usize) -> usize {
        if line_count < 750 {
            4
        } else if line_count < 9_750 {
            5
        } else if line_count < 99_750 {
            6
        } else {
            7
        }
    }

    /// Format a 1-indexed line number for display in the gutter.
    ///
    /// Returns a right-justified string with a trailing space separator.
    /// Example with width 4: `"  1 "`, `" 42 "`, `"999 "`
    ///
    /// The number is right-justified in `(width - 1)` columns, followed
    /// by one space separator.
    pub fn format_line_number(&self, line_num: usize) -> String {
        // width is always >= 4 when enabled; this guards against future misuse
        debug_assert!(self.width > 0, "format_line_number called with width=0");
        format!("{:>width$} ", line_num, width = self.width - 1)
    }
}

/// Main renderer that coordinates all rendering operations.
pub struct Renderer<W: Write = io::Stdout> {
    /// Terminal output handle
    output: TerminalOutput<W>,
    /// Current viewport state
    viewport: Viewport,
    /// Terminal dimensions
    terminal_size: Size,
    /// Line number gutter state
    gutter: Gutter,
}

impl Renderer<io::Stdout> {
    /// Create a new renderer with stdout and the given terminal size.
    pub fn new(output: TerminalOutput, terminal_size: Size) -> Self {
        Self::with_writer(output, terminal_size)
    }
}

impl<W: Write> Renderer<W> {
    /// Create a new renderer with a custom writer and the given terminal size.
    pub fn with_writer(output: TerminalOutput<W>, terminal_size: Size) -> Self {
        // Reserve one line for status line
        let viewport = Viewport::new(
            0,
            (terminal_size.rows as usize).saturating_sub(1), // Reserve bottom line for status
            terminal_size.cols as usize,
        );

        Self {
            output,
            viewport,
            terminal_size,
            gutter: Gutter::disabled(),
        }
    }

    /// Render the editor interface with optional command-line, status message,
    /// search highlighting, line numbers, and soft-wrapping.
    ///
    /// The bottom line displays one of three things, in priority order:
    /// 1. The command line (when `command_line` is `Some`)
    /// 2. A status message (when `status_message` is `Some`)
    /// 3. The normal status line (otherwise)
    ///
    /// When `search.hlsearch` is true and `search.regex` is `Some`, all matches
    /// of the search pattern in the visible viewport are highlighted using
    /// reverse video (ANSI Invert).
    ///
    /// When `number` is true, a line number gutter is displayed to the left of
    /// each buffer line. The gutter width adapts dynamically with hysteresis.
    ///
    /// When `wrap` is true, long lines are soft-wrapped across multiple terminal
    /// rows instead of being truncated.
    ///
    /// When `list` is true, tabs are displayed as `^I` and each line ends with `$`.
    pub fn render_full(
        &mut self,
        ctx: RenderContext<'_>,
        opts: RenderOptions<'_>,
    ) -> Result<(), RenderError> {
        let RenderContext {
            buffer,
            cursor,
            mode,
            filename,
            modified,
            line_ending,
        } = ctx;
        let RenderOptions {
            command_line,
            status_message,
            search,
            tabstop,
            number,
            wrap,
            list,
            selection,
        } = opts;

        // Update gutter state for this render cycle
        self.gutter.update(number, buffer.len());

        // Compute text_width before update_viewport so wrap-aware scrolling
        // uses the same value as rendering
        let text_width = self
            .viewport
            .width()
            .saturating_sub(self.gutter.width())
            .max(1);

        // Update viewport to keep cursor visible (wrap-aware)
        self.update_viewport(cursor, buffer, text_width, tabstop, wrap, list);

        // Clear screen
        self.clear_screen()?;

        // Compute search highlights for visible lines
        let search_highlights = if search.hlsearch {
            if let Some(regex) = search.regex {
                self.compute_highlights(buffer, regex)
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        // Compute selection highlights and merge with search highlights
        // Selection highlights take priority (overwrite search highlights where they overlap)
        let highlights = if let Some(sel) = selection {
            let sel_highlights =
                buffer_render::compute_selection_highlights(sel, &self.viewport, buffer);
            merge_highlights(search_highlights, sel_highlights)
        } else {
            search_highlights
        };

        // Render buffer content with highlights, optional gutter, and wrapping
        buffer_render::render_buffer_lines(
            &mut self.output,
            buffer,
            &self.viewport,
            &highlights,
            tabstop,
            &self.gutter,
            wrap,
            list,
        )
        .map_err(RenderError::RenderFailed)?;

        // Render the bottom line: command line, status message, or status line
        if let Some(cl) = command_line {
            // Command-line mode: show the command line with cursor
            status_line::render_command_line(&mut self.output, cl, &self.viewport)
                .map_err(RenderError::RenderFailed)?;
        } else if let Some(msg) = status_message {
            // Status message: dispatch based on content type
            match msg {
                StatusDisplay::Text(text) => {
                    status_line::render_status_message(&mut self.output, text, &self.viewport)
                        .map_err(RenderError::RenderFailed)?;
                }
                StatusDisplay::WithPath { path, suffix } => {
                    status_line::render_status_with_path(
                        &mut self.output,
                        path,
                        suffix,
                        &self.viewport,
                    )
                    .map_err(RenderError::RenderFailed)?;
                }
            }

            // Still position the buffer cursor for normal editing
            cursor_render::position_cursor(
                &mut self.output,
                cursor,
                &self.viewport,
                buffer,
                tabstop,
                self.gutter.width(),
                wrap,
                text_width,
            )
            .map_err(RenderError::CursorPosition)?;
        } else {
            // Normal status line
            let row = cursor.row + 1; // 1-indexed
                                      // Compute display column (tab/wide-char aware, 1-indexed)
            let vcol = if let Some(line) = buffer.line(cursor.row) {
                cursor_render::byte_offset_to_display_col(line, cursor.col, tabstop) + 1
            } else {
                1
            };
            let position = (row, vcol);
            let total_lines = buffer.len();
            status_line::render_status_line(
                &mut self.output,
                mode,
                filename,
                position,
                total_lines,
                &self.viewport,
                modified,
                line_ending,
            )
            .map_err(RenderError::RenderFailed)?;

            // Position cursor in the buffer area
            cursor_render::position_cursor(
                &mut self.output,
                cursor,
                &self.viewport,
                buffer,
                tabstop,
                self.gutter.width(),
                wrap,
                text_width,
            )
            .map_err(RenderError::CursorPosition)?;
        }

        self.output.flush().map_err(RenderError::RenderFailed)?;

        Ok(())
    }

    /// Compute highlight ranges for all visible lines.
    ///
    /// Scans each visible line with the compiled regex and collects byte-offset
    /// ranges. The returned vector is indexed by viewport-relative line number.
    fn compute_highlights(&self, buffer: &Buffer, regex: &ViRegex) -> Vec<Vec<HighlightRange>> {
        let (start, end) = self.viewport.visible_range();
        let end = end.min(buffer.len());
        let mut result = Vec::with_capacity(end.saturating_sub(start));

        for line_idx in start..end {
            if let Some(line) = buffer.line(line_idx) {
                result.push(find_highlights(line, regex));
            } else {
                result.push(Vec::new());
            }
        }

        result
    }

    /// Update the viewport to ensure the cursor is visible.
    ///
    /// When `wrap` is false or `text_width` is 0, falls back to the simple
    /// 1:1 row comparison via `viewport.ensure_visible`. When `wrap` is true,
    /// accounts for wrapped line heights to determine whether the cursor's
    /// buffer line fits within the viewport's terminal rows.
    ///
    /// When scrolling up in wrap mode, the cursor is centered vertically in
    /// the viewport (matching the non-wrap centering behavior).
    ///
    /// # Arguments
    ///
    /// * `cursor` - Current cursor position
    /// * `buffer` - The text buffer
    /// * `text_width` - Available text columns (viewport width minus gutter)
    /// * `tabstop` - Tab stop width for tab expansion
    /// * `wrap` - Whether soft-wrapping is enabled
    /// * `list` - Whether list mode is active (tabs render as 2-column `^I`)
    pub fn update_viewport(
        &mut self,
        cursor: &Cursor,
        buffer: &Buffer,
        text_width: usize,
        tabstop: usize,
        wrap: bool,
        list: bool,
    ) {
        // Fall back to simple 1:1 logic when wrapping is off or text_width is 0
        if !wrap || text_width == 0 {
            self.viewport.ensure_visible(cursor.row, buffer.len());
            return;
        }

        // Clamp cursor row to valid range
        let cursor_row = cursor.row.min(buffer.len().saturating_sub(1));
        let top_line = self.viewport.top_line();
        let height = self.viewport.height();

        if height == 0 {
            return;
        }

        // Cursor above viewport: center the cursor vertically
        if cursor_row < top_line {
            let centered_top = cursor_row.saturating_sub(height / 2);
            self.viewport.set_top_line(centered_top);
            return;
        }

        // Count terminal rows from top_line to cursor_row (inclusive),
        // with short-circuit when accumulated rows exceed viewport height
        let mut rows_used: usize = 0;
        for i in top_line..=cursor_row {
            let line = buffer.line(i).unwrap_or("");
            rows_used += buffer_render::wrapped_line_height(line, text_width, tabstop, true, list);
            if rows_used > height {
                break;
            }
        }

        // If cursor fits within the viewport, nothing to do
        if rows_used <= height {
            return;
        }

        // Cursor is off screen below. Walk backward from cursor_row to find the
        // largest new_top such that the sum of heights from new_top..=cursor_row
        // fits within the viewport height.
        let mut budget = height;
        let mut new_top = cursor_row;

        // Start with the cursor's own line
        let cursor_height = buffer_render::wrapped_line_height(
            buffer.line(cursor_row).unwrap_or(""),
            text_width,
            tabstop,
            true,
            list,
        );

        if cursor_height >= budget {
            // Cursor's own line fills or exceeds the viewport
            self.viewport.set_top_line(cursor_row);
            return;
        }

        budget -= cursor_height;

        // Walk backward from cursor_row - 1
        if cursor_row > 0 {
            for row in (0..cursor_row).rev() {
                let line_height = buffer_render::wrapped_line_height(
                    buffer.line(row).unwrap_or(""),
                    text_width,
                    tabstop,
                    true,
                    list,
                );

                if line_height > budget {
                    // This line doesn't fit; new_top stays at row + 1
                    break;
                }

                budget -= line_height;
                new_top = row;

                if budget == 0 {
                    break;
                }
            }
        }

        self.viewport.set_top_line(new_top);
    }

    /// Handle terminal resize event.
    pub fn handle_resize(&mut self, new_size: Size) {
        self.terminal_size = new_size;
        // Reserve one line for status line
        self.viewport
            .set_height((new_size.rows as usize).saturating_sub(1));
        self.viewport.set_width(new_size.cols as usize);
    }

    /// Clear the entire screen.
    pub fn clear_screen(&mut self) -> Result<(), RenderError> {
        self.output.clear().map_err(RenderError::RenderFailed)?;
        Ok(())
    }

    /// Emit a BEL character (`\x07`) to the terminal.
    ///
    /// Called when `errorbells` is on and a status message has just been set.
    pub fn ring_bell(&mut self) -> Result<(), RenderError> {
        self.output
            .write_byte(0x07)
            .map_err(RenderError::RenderFailed)?;
        self.output.flush().map_err(RenderError::RenderFailed)?;
        Ok(())
    }

    /// Get the current viewport.
    pub fn viewport(&self) -> &Viewport {
        &self.viewport
    }

    /// Get a mutable reference to the viewport.
    pub fn viewport_mut(&mut self) -> &mut Viewport {
        &mut self.viewport
    }
}

/// Merge two sets of per-line highlights, with `overlay` taking priority over `base`.
///
/// Both slices are indexed by viewport-relative line. The overlay highlights replace
/// (not append to) the base highlights for any line where the overlay is non-empty.
fn merge_highlights(
    base: Vec<Vec<HighlightRange>>,
    overlay: Vec<Vec<HighlightRange>>,
) -> Vec<Vec<HighlightRange>> {
    let len = base.len().max(overlay.len());
    let mut result = Vec::with_capacity(len);
    for i in 0..len {
        let b = base.get(i).cloned().unwrap_or_default();
        let o = overlay.get(i).cloned().unwrap_or_default();
        if o.is_empty() {
            result.push(b);
        } else {
            result.push(o);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Gutter unit tests ---

    #[test]
    fn test_gutter_disabled() {
        let gutter = Gutter::disabled();
        assert_eq!(gutter.width(), 0);
        assert!(!gutter.enabled());
    }

    #[test]
    fn test_gutter_initial_width_small_file() {
        let mut gutter = Gutter::disabled();
        gutter.update(true, 100);
        assert!(gutter.enabled());
        assert_eq!(gutter.width(), 4);
    }

    #[test]
    fn test_gutter_initial_width_medium_file() {
        let mut gutter = Gutter::disabled();
        gutter.update(true, 5_000);
        assert_eq!(gutter.width(), 5);
    }

    #[test]
    fn test_gutter_initial_width_large_file() {
        let mut gutter = Gutter::disabled();
        gutter.update(true, 50_000);
        assert_eq!(gutter.width(), 6);
    }

    #[test]
    fn test_gutter_initial_width_very_large_file() {
        let mut gutter = Gutter::disabled();
        gutter.update(true, 100_000);
        assert_eq!(gutter.width(), 7);
    }

    #[test]
    fn test_gutter_initial_width_boundary_749() {
        let mut gutter = Gutter::disabled();
        gutter.update(true, 749);
        assert_eq!(gutter.width(), 4);
    }

    #[test]
    fn test_gutter_initial_width_boundary_750() {
        let mut gutter = Gutter::disabled();
        gutter.update(true, 750);
        assert_eq!(gutter.width(), 5);
    }

    #[test]
    fn test_gutter_initial_width_boundary_9749() {
        let mut gutter = Gutter::disabled();
        gutter.update(true, 9_749);
        assert_eq!(gutter.width(), 5);
    }

    #[test]
    fn test_gutter_initial_width_boundary_9750() {
        let mut gutter = Gutter::disabled();
        gutter.update(true, 9_750);
        assert_eq!(gutter.width(), 6);
    }

    #[test]
    fn test_gutter_initial_width_boundary_99749() {
        let mut gutter = Gutter::disabled();
        gutter.update(true, 99_749);
        assert_eq!(gutter.width(), 6);
    }

    #[test]
    fn test_gutter_initial_width_boundary_99750() {
        let mut gutter = Gutter::disabled();
        gutter.update(true, 99_750);
        assert_eq!(gutter.width(), 7);
    }

    #[test]
    fn test_gutter_expand_4_to_5() {
        let mut gutter = Gutter::disabled();
        gutter.update(true, 500); // initial width 4
        assert_eq!(gutter.width(), 4);

        gutter.update(true, 1_000); // crosses 1,000 -> expand to 5
        assert_eq!(gutter.width(), 5);
    }

    #[test]
    fn test_gutter_expand_5_to_6() {
        let mut gutter = Gutter::disabled();
        gutter.update(true, 5_000); // initial width 5
        assert_eq!(gutter.width(), 5);

        gutter.update(true, 10_000); // crosses 10,000 -> expand to 6
        assert_eq!(gutter.width(), 6);
    }

    #[test]
    fn test_gutter_expand_6_to_7() {
        let mut gutter = Gutter::disabled();
        gutter.update(true, 50_000); // initial width 6
        assert_eq!(gutter.width(), 6);

        gutter.update(true, 100_000); // crosses 100,000 -> expand to 7
        assert_eq!(gutter.width(), 7);
    }

    #[test]
    fn test_gutter_no_expand_below_threshold() {
        let mut gutter = Gutter::disabled();
        gutter.update(true, 500); // initial width 4
        assert_eq!(gutter.width(), 4);

        gutter.update(true, 999); // still below 1,000 -> stay at 4
        assert_eq!(gutter.width(), 4);
    }

    #[test]
    fn test_gutter_shrink_5_to_4() {
        let mut gutter = Gutter::disabled();
        gutter.update(true, 5_000); // initial width 5
        assert_eq!(gutter.width(), 5);

        gutter.update(true, 749); // below 750 -> shrink to 4
        assert_eq!(gutter.width(), 4);
    }

    #[test]
    fn test_gutter_no_shrink_at_750() {
        let mut gutter = Gutter::disabled();
        gutter.update(true, 5_000); // initial width 5
        assert_eq!(gutter.width(), 5);

        gutter.update(true, 750); // at 750 -> stay at 5 (hysteresis)
        assert_eq!(gutter.width(), 5);
    }

    #[test]
    fn test_gutter_shrink_6_to_5() {
        let mut gutter = Gutter::disabled();
        gutter.update(true, 50_000); // initial width 6
        assert_eq!(gutter.width(), 6);

        gutter.update(true, 9_749); // below 9,750 -> shrink to 5
        assert_eq!(gutter.width(), 5);
    }

    #[test]
    fn test_gutter_no_shrink_at_9750() {
        let mut gutter = Gutter::disabled();
        gutter.update(true, 50_000); // initial width 6
        assert_eq!(gutter.width(), 6);

        gutter.update(true, 9_750); // at 9,750 -> stay at 6
        assert_eq!(gutter.width(), 6);
    }

    #[test]
    fn test_gutter_shrink_7_to_6() {
        let mut gutter = Gutter::disabled();
        gutter.update(true, 100_000); // initial width 7
        assert_eq!(gutter.width(), 7);

        gutter.update(true, 99_749); // below 99,750 -> shrink to 6
        assert_eq!(gutter.width(), 6);
    }

    #[test]
    fn test_gutter_no_shrink_at_99750() {
        let mut gutter = Gutter::disabled();
        gutter.update(true, 100_000); // initial width 7
        assert_eq!(gutter.width(), 7);

        gutter.update(true, 99_750); // at 99,750 -> stay at 7
        assert_eq!(gutter.width(), 7);
    }

    #[test]
    fn test_gutter_disable_resets() {
        let mut gutter = Gutter::disabled();
        gutter.update(true, 500);
        assert!(gutter.enabled());
        assert_eq!(gutter.width(), 4);

        gutter.update(false, 500);
        assert!(!gutter.enabled());
        assert_eq!(gutter.width(), 0);
    }

    #[test]
    fn test_gutter_reenable_uses_initial_width() {
        let mut gutter = Gutter::disabled();
        gutter.update(true, 500); // width 4
        gutter.update(false, 500); // disabled

        // Re-enable with a larger file -> uses initial_width, not hysteresis
        gutter.update(true, 5_000);
        assert_eq!(gutter.width(), 5);
    }

    #[test]
    fn test_gutter_format_line_number_width_4() {
        let mut gutter = Gutter::disabled();
        gutter.update(true, 100);
        assert_eq!(gutter.width(), 4);

        assert_eq!(gutter.format_line_number(1), "  1 ");
        assert_eq!(gutter.format_line_number(42), " 42 ");
        assert_eq!(gutter.format_line_number(999), "999 ");
    }

    #[test]
    fn test_gutter_format_line_number_width_5() {
        let mut gutter = Gutter::disabled();
        gutter.update(true, 5_000);
        assert_eq!(gutter.width(), 5);

        assert_eq!(gutter.format_line_number(1), "   1 ");
        assert_eq!(gutter.format_line_number(9999), "9999 ");
    }

    #[test]
    fn test_gutter_format_line_number_width_6() {
        let mut gutter = Gutter::disabled();
        gutter.update(true, 50_000);
        assert_eq!(gutter.width(), 6);

        assert_eq!(gutter.format_line_number(1), "    1 ");
        assert_eq!(gutter.format_line_number(99999), "99999 ");
    }

    #[test]
    fn test_gutter_format_line_number_width_7() {
        let mut gutter = Gutter::disabled();
        gutter.update(true, 100_000);
        assert_eq!(gutter.width(), 7);

        assert_eq!(gutter.format_line_number(1), "     1 ");
        assert_eq!(gutter.format_line_number(999999), "999999 ");
    }

    // --- Renderer tests ---

    #[test]
    fn test_renderer_new() {
        let output = TerminalOutput::new();
        let size = Size::new(80, 24);
        let renderer = Renderer::new(output, size);
        assert_eq!(renderer.viewport.height(), 23); // 24 - 1 for status line
    }

    #[test]
    fn test_compute_highlights_no_regex() {
        let output = TerminalOutput::with_writer(Vec::new());
        let size = Size::new(80, 24);
        let renderer = Renderer::with_writer(output, size);

        let buffer = Buffer::from_string("hello world".to_string());
        let regex = ViRegex::compile("world").unwrap();
        let highlights = renderer.compute_highlights(&buffer, &regex);

        assert_eq!(highlights.len(), 1);
        assert_eq!(highlights[0].len(), 1);
        assert_eq!(highlights[0][0].start, 6);
        assert_eq!(highlights[0][0].end, 11);
    }

    #[test]
    fn test_compute_highlights_multiple_lines() {
        let output = TerminalOutput::with_writer(Vec::new());
        let size = Size::new(80, 24);
        let renderer = Renderer::with_writer(output, size);

        let buffer = Buffer::from_string("foo bar\nbaz foo\nno match".to_string());
        let regex = ViRegex::compile("foo").unwrap();
        let highlights = renderer.compute_highlights(&buffer, &regex);

        assert_eq!(highlights.len(), 3);
        assert_eq!(highlights[0].len(), 1); // "foo" on line 1
        assert_eq!(highlights[1].len(), 1); // "foo" on line 2
        assert_eq!(highlights[2].len(), 0); // no match on line 3
    }

    #[test]
    fn test_compute_highlights_empty_buffer() {
        let output = TerminalOutput::with_writer(Vec::new());
        let size = Size::new(80, 24);
        let renderer = Renderer::with_writer(output, size);

        let buffer = Buffer::new();
        let regex = ViRegex::compile("hello").unwrap();
        let highlights = renderer.compute_highlights(&buffer, &regex);

        assert_eq!(highlights.len(), 1); // Buffer always has at least 1 line
    }

    // --- Wrap-aware update_viewport tests ---

    #[test]
    fn test_update_viewport_no_wrap_cursor_visible() {
        // Cursor is within the viewport, no scroll needed
        let output = TerminalOutput::with_writer(Vec::new());
        let size = Size::new(80, 11); // 10 rows viewport (11 - 1 for status)
        let mut renderer = Renderer::with_writer(output, size);

        let buffer = Buffer::from_string("line1\nline2\nline3\nline4\nline5".to_string());
        let cursor = Cursor { row: 2, col: 0 };

        renderer.update_viewport(&cursor, &buffer, 80, 8, false, false);
        assert_eq!(renderer.viewport.top_line(), 0);
    }

    #[test]
    fn test_update_viewport_no_wrap_cursor_below() {
        // Cursor is below the viewport, should scroll down
        let output = TerminalOutput::with_writer(Vec::new());
        let size = Size::new(80, 6); // 5 rows viewport
        let mut renderer = Renderer::with_writer(output, size);

        // 20 lines
        let lines: Vec<&str> = (0..20).map(|_| "line").collect();
        let buffer = Buffer::from_string(lines.join("\n"));
        let cursor = Cursor { row: 15, col: 0 };

        renderer.update_viewport(&cursor, &buffer, 80, 8, false, false);
        // Cursor should now be visible
        assert!(renderer.viewport.is_line_visible(15));
    }

    #[test]
    fn test_update_viewport_no_wrap_cursor_above() {
        // Cursor is above the viewport, should scroll up
        let output = TerminalOutput::with_writer(Vec::new());
        let size = Size::new(80, 11); // 10 rows viewport
        let mut renderer = Renderer::with_writer(output, size);

        let lines: Vec<&str> = (0..20).map(|_| "line").collect();
        let buffer = Buffer::from_string(lines.join("\n"));

        // Set viewport to show lines 10-19
        renderer.viewport_mut().set_top_line(10);

        let cursor = Cursor { row: 3, col: 0 };
        renderer.update_viewport(&cursor, &buffer, 80, 8, false, false);

        // Cursor should now be visible
        assert!(renderer.viewport.is_line_visible(3));
    }

    #[test]
    fn test_update_viewport_wrap_long_lines_push_cursor_off() {
        // Lines above cursor each wrap to 2 terminal rows, pushing cursor off screen
        let output = TerminalOutput::with_writer(Vec::new());
        // viewport height = 5 rows (6 - 1 for status)
        let size = Size::new(10, 6);
        let mut renderer = Renderer::with_writer(output, size);

        // 5 lines, each 15 chars wide -> at text_width=10, each wraps to 2 rows
        let buffer = Buffer::from_string(
            "aaaaabbbbb12345\n\
             cccccdddddeeeee\n\
             fffffggggghhhhh\n\
             iiiiijjjjjkkkkk\n\
             lllllmmmmmoo123"
                .to_string(),
        );

        // Cursor on line 4 (0-indexed). Lines 0-3 each take 2 rows = 8 rows,
        // plus line 4 takes 2 rows = 10 total. Viewport is only 5 rows.
        let cursor = Cursor { row: 4, col: 0 };
        renderer.update_viewport(&cursor, &buffer, 10, 8, true, false);

        let top = renderer.viewport.top_line();
        // Verify cursor line is reachable from top within viewport height
        let rows_from_top: usize = (top..=4)
            .map(|i| {
                let line = buffer.line(i).unwrap_or("");
                buffer_render::wrapped_line_height(line, 10, 8, true, false)
            })
            .sum();
        assert!(
            rows_from_top <= 5,
            "cursor should be visible: rows_from_top={rows_from_top}, viewport_height=5"
        );
    }

    #[test]
    fn test_update_viewport_wrap_cursor_line_itself_tall() {
        // Cursor's own line fills the entire viewport
        let output = TerminalOutput::with_writer(Vec::new());
        // viewport height = 3 rows (4 - 1 for status)
        let size = Size::new(5, 4);
        let mut renderer = Renderer::with_writer(output, size);

        // Line 0 is short, line 1 is very long (20 chars at width 5 = 4 rows)
        let buffer = Buffer::from_string("hi\naaaabbbbccccddddeeee".to_string());

        let cursor = Cursor { row: 1, col: 0 };
        renderer.update_viewport(&cursor, &buffer, 5, 8, true, false);

        // top_line should be set to cursor_row since the line itself exceeds
        // the viewport height
        assert_eq!(
            renderer.viewport.top_line(),
            1,
            "top_line should be cursor_row when cursor line fills viewport"
        );
    }

    #[test]
    fn test_update_viewport_wrap_scroll_up_centers_cursor() {
        // When cursor is above viewport in wrap mode, it should be centered
        let output = TerminalOutput::with_writer(Vec::new());
        // viewport height = 10 rows (11 - 1 for status)
        let size = Size::new(80, 11);
        let mut renderer = Renderer::with_writer(output, size);

        let lines: Vec<&str> = (0..20).map(|_| "line").collect();
        let buffer = Buffer::from_string(lines.join("\n"));

        // Set viewport to show lines 15+
        renderer.viewport_mut().set_top_line(15);

        // Cursor at line 8, which is above viewport
        let cursor = Cursor { row: 8, col: 0 };
        renderer.update_viewport(&cursor, &buffer, 80, 8, true, false);

        // With height=10, centering around row 8 means top_line = 8 - 10/2 = 3
        assert_eq!(
            renderer.viewport.top_line(),
            3,
            "wrap-mode scroll-up should center cursor in viewport"
        );
    }

    #[test]
    fn test_update_viewport_wrap_scroll_up_clamps_to_zero() {
        // When cursor is near top and centering would go negative, clamp to 0
        let output = TerminalOutput::with_writer(Vec::new());
        // viewport height = 10 rows (11 - 1 for status)
        let size = Size::new(80, 11);
        let mut renderer = Renderer::with_writer(output, size);

        let lines: Vec<&str> = (0..20).map(|_| "line").collect();
        let buffer = Buffer::from_string(lines.join("\n"));

        // Set viewport to show lines 10+
        renderer.viewport_mut().set_top_line(10);

        // Cursor at line 2, centering would try 2 - 5 = negative -> clamp to 0
        let cursor = Cursor { row: 2, col: 0 };
        renderer.update_viewport(&cursor, &buffer, 80, 8, true, false);

        assert_eq!(
            renderer.viewport.top_line(),
            0,
            "wrap-mode scroll-up should clamp to 0 when cursor is near top"
        );
    }
}

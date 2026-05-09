//! Viewport management for controlling which portion of the buffer is visible.
//!
//! The viewport determines which lines of the buffer are currently displayed
//! on the terminal screen.

/// Represents the visible portion of the buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Viewport {
    /// First visible line (0-based buffer row index)
    top_line: usize,
    /// Number of visible lines (height)
    height: usize,
    /// Number of visible columns (width)
    width: usize,
}

impl Viewport {
    /// Create a new viewport.
    ///
    /// # Arguments
    ///
    /// * `top_line` - First visible line (0-based)
    /// * `height` - Number of visible lines
    /// * `width` - Number of visible columns
    pub fn new(top_line: usize, height: usize, width: usize) -> Self {
        Self {
            top_line,
            height,
            width,
        }
    }

    /// Get the first visible line.
    pub fn top_line(&self) -> usize {
        self.top_line
    }

    /// Get the viewport height (number of visible lines).
    pub fn height(&self) -> usize {
        self.height
    }

    /// Get the viewport width (number of visible columns).
    pub fn width(&self) -> usize {
        self.width
    }

    /// Set the viewport height.
    pub fn set_height(&mut self, height: usize) {
        self.height = height;
    }

    /// Set the viewport width.
    pub fn set_width(&mut self, width: usize) {
        self.width = width;
    }

    /// Set the first visible line directly.
    pub fn set_top_line(&mut self, top_line: usize) {
        self.top_line = top_line;
    }

    /// Scroll the viewport to show a specific line.
    ///
    /// The line will be positioned at the top of the viewport.
    ///
    /// # Arguments
    ///
    /// * `line` - Line index to scroll to (0-based)
    /// * `buffer_len` - Total number of lines in the buffer
    pub fn scroll_to_line(&mut self, line: usize, buffer_len: usize) {
        if buffer_len == 0 {
            self.top_line = 0;
            return;
        }

        // Clamp line to valid range
        let line = line.min(buffer_len.saturating_sub(1));

        // Set top_line, ensuring we don't scroll past the end
        let max_top = buffer_len.saturating_sub(self.height);
        self.top_line = line.min(max_top);
    }

    /// Ensure that the given line is visible in the viewport.
    ///
    /// If the line is outside the viewport, the viewport will be scrolled
    /// to bring it into view. The line will be positioned in the middle
    /// of the viewport if possible, or at the edges if near buffer boundaries.
    ///
    /// # Arguments
    ///
    /// * `cursor_row` - Row index that should be visible (0-based)
    /// * `buffer_len` - Total number of lines in the buffer
    pub fn ensure_visible(&mut self, cursor_row: usize, buffer_len: usize) {
        if buffer_len == 0 {
            self.top_line = 0;
            return;
        }

        let cursor_row = cursor_row.min(buffer_len.saturating_sub(1));
        let bottom_line = self.top_line + self.height.saturating_sub(1);

        if cursor_row < self.top_line {
            // Cursor is above viewport - scroll up
            self.scroll_to_line(cursor_row, buffer_len);
        } else if cursor_row > bottom_line && bottom_line < buffer_len {
            // Cursor is below viewport - scroll down
            // Try to center the cursor in the viewport
            let center_offset = self.height / 2;
            if cursor_row >= center_offset {
                self.top_line = cursor_row - center_offset;
            } else {
                self.top_line = 0;
            }

            // Ensure we don't scroll past the end
            let max_top = buffer_len.saturating_sub(self.height);
            self.top_line = self.top_line.min(max_top);
        }
    }

    /// Get the range of visible lines as (start, end) indices.
    ///
    /// The end index is exclusive (like Rust ranges).
    pub fn visible_range(&self) -> (usize, usize) {
        (self.top_line, self.top_line + self.height)
    }

    /// Check if a line is currently visible in the viewport.
    pub fn is_line_visible(&self, line: usize) -> bool {
        line >= self.top_line && line < self.top_line + self.height
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_viewport_new() {
        let viewport = Viewport::new(0, 24, 80);
        assert_eq!(viewport.top_line(), 0);
        assert_eq!(viewport.height(), 24);
        assert_eq!(viewport.width(), 80);
    }

    #[test]
    fn test_viewport_scroll_to_line() {
        let mut viewport = Viewport::new(0, 10, 80);
        viewport.scroll_to_line(5, 20);
        assert_eq!(viewport.top_line(), 5);
    }

    #[test]
    fn test_viewport_scroll_to_line_bounds() {
        let mut viewport = Viewport::new(0, 10, 80);
        viewport.scroll_to_line(15, 20);
        // Should not scroll past end
        assert_eq!(viewport.top_line(), 10); // 20 - 10 = 10

        viewport.scroll_to_line(100, 20);
        assert_eq!(viewport.top_line(), 10);
    }

    #[test]
    fn test_viewport_ensure_visible() {
        let mut viewport = Viewport::new(0, 10, 80);

        // Cursor below viewport
        viewport.ensure_visible(15, 20);
        assert!(viewport.is_line_visible(15));

        // Cursor above viewport
        viewport.top_line = 10;
        viewport.ensure_visible(5, 20);
        assert!(viewport.is_line_visible(5));
    }

    #[test]
    fn test_viewport_visible_range() {
        let viewport = Viewport::new(5, 10, 80);
        assert_eq!(viewport.visible_range(), (5, 15));
    }

    #[test]
    fn test_viewport_set_top_line() {
        let mut viewport = Viewport::new(0, 10, 80);
        viewport.set_top_line(5);
        assert_eq!(viewport.top_line(), 5);
    }
}

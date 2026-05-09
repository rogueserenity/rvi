//! Cursor movement methods for the editor.
//!
//! This module contains all movement-related methods for the Editor.

use super::Editor;
use crate::buffer::unicode::prev_grapheme_boundary;

impl Editor {
    /// Move cursor left.
    pub(super) fn move_left(&mut self) {
        if self.document.cursor.col > 0 {
            if let Some(line) = self.document.buffer.line(self.document.cursor.row) {
                self.document.cursor.move_left_grapheme(line);
            }
        }
    }

    /// Move cursor right.
    ///
    /// In normal mode, cursor cannot move past the last character.
    /// In insert mode, cursor can be at end of line (after last character).
    pub(super) fn move_right(&mut self) {
        if let Some(line) = self.document.buffer.line(self.document.cursor.row) {
            let max_col = self.max_cursor_col(line);
            self.document.cursor.move_right_grapheme(line, max_col);
        }
    }

    /// Move cursor right unconditionally (for insert mode operations like 'a').
    ///
    /// This allows moving past the last character, which is needed when
    /// appending after the cursor.
    pub(super) fn move_right_unconstrained(&mut self) {
        if let Some(line) = self.document.buffer.line(self.document.cursor.row) {
            self.document.cursor.move_right_grapheme_unconstrained(line);
        }
    }

    /// Move cursor up.
    pub(super) fn move_up(&mut self) {
        if self.document.cursor.row > 0 {
            self.document.cursor.row -= 1;
            self.clamp_cursor_to_line();
        }
    }

    /// Move cursor down.
    pub(super) fn move_down(&mut self) {
        if self.document.cursor.row < self.document.buffer.len() - 1 {
            self.document.cursor.row += 1;
            self.clamp_cursor_to_line();
        }
    }

    /// Move cursor to first non-blank character on the line.
    pub(super) fn move_to_first_non_blank(&mut self) {
        if let Some(line) = self.document.buffer.line(self.document.cursor.row) {
            // Find first non-whitespace character, or stay at 0 if all whitespace
            let col = line
                .char_indices()
                .find(|(_, ch)| !ch.is_whitespace())
                .map(|(idx, _)| idx)
                .unwrap_or(0);
            // Clamp to valid position
            self.document.cursor.col = col.min(self.max_cursor_col(line));
        }
    }

    /// Move cursor to end of line unconditionally (for insert mode operations).
    pub(super) fn move_to_line_end_unconstrained(&mut self) {
        if let Some(line) = self.document.buffer.line(self.document.cursor.row) {
            self.document.cursor.col = line.len();
        }
    }

    /// Get the maximum cursor column for the current mode.
    ///
    /// In normal mode, cursor cannot be past the last character.
    /// In insert mode, cursor can be at end of line.
    pub(super) fn max_cursor_col(&self, line: &str) -> usize {
        if line.is_empty() {
            return 0;
        }
        if self.state.mode.allows_cursor_past_end() {
            // In insert mode and similar, cursor can be at end of line
            line.len()
        } else {
            // In normal mode and similar, cursor stays on last character
            prev_grapheme_boundary(line, line.len())
        }
    }

    /// Clamp cursor column to current line length respecting mode.
    pub(super) fn clamp_cursor_to_line(&mut self) {
        if let Some(line) = self.document.buffer.line(self.document.cursor.row) {
            let max_col = self.max_cursor_col(line);
            self.document.cursor.clamp_to_line_grapheme(line, max_col);
        }
    }

    /// Scroll forward one full screen (Ctrl-f).
    ///
    /// Standard vi scrolls by `height - 2` lines to keep 2 lines of
    /// context visible from the previous screen.
    pub(super) fn scroll_full_screen_forward(&mut self, count: usize) {
        let buf_len = self.document.buffer.len();
        let height = self.state.viewport.height();
        // Keep 2 lines of context (minimum scroll of 1)
        let page_size = height.saturating_sub(2).max(1);
        let scroll_amount = page_size.saturating_mul(count);
        let new_top =
            (self.state.viewport.top_line() + scroll_amount).min(buf_len.saturating_sub(1));
        self.state.viewport.set_top_line(new_top);
        // Move cursor to top of new viewport, clamped to buffer
        self.document.cursor.row = new_top.min(buf_len.saturating_sub(1));
        self.clamp_cursor_to_line();
    }

    /// Scroll backward one full screen (Ctrl-b).
    ///
    /// Standard vi scrolls by `height - 2` lines to keep 2 lines of
    /// context visible from the previous screen.
    pub(super) fn scroll_full_screen_backward(&mut self, count: usize) {
        let height = self.state.viewport.height();
        // Keep 2 lines of context (minimum scroll of 1)
        let page_size = height.saturating_sub(2).max(1);
        let scroll_amount = page_size.saturating_mul(count);
        let new_top = self.state.viewport.top_line().saturating_sub(scroll_amount);
        self.state.viewport.set_top_line(new_top);
        // Move cursor to top of new viewport
        let buf_len = self.document.buffer.len();
        self.document.cursor.row = new_top.min(buf_len.saturating_sub(1));
        self.clamp_cursor_to_line();
    }

    /// Scroll down half screen (Ctrl-d).
    ///
    /// If `count` is Some, updates the persistent half-screen size.
    pub(super) fn scroll_half_screen_down(&mut self, count: Option<usize>) {
        let height = self.state.viewport.height();
        let half =
            count.unwrap_or_else(|| self.state.scroll_half_size.unwrap_or(height / 2).max(1));
        if let Some(c) = count {
            self.state.scroll_half_size = Some(c.max(1));
        }
        let buf_len = self.document.buffer.len();
        let new_top = (self.state.viewport.top_line() + half).min(buf_len.saturating_sub(1));
        self.state.viewport.set_top_line(new_top);
        // Move cursor down by the same amount, clamped to buffer
        self.document.cursor.row = (self.document.cursor.row + half).min(buf_len.saturating_sub(1));
        self.clamp_cursor_to_line();
    }

    /// Scroll up half screen (Ctrl-u).
    ///
    /// If `count` is Some, updates the persistent half-screen size.
    pub(super) fn scroll_half_screen_up(&mut self, count: Option<usize>) {
        let height = self.state.viewport.height();
        let half =
            count.unwrap_or_else(|| self.state.scroll_half_size.unwrap_or(height / 2).max(1));
        if let Some(c) = count {
            self.state.scroll_half_size = Some(c.max(1));
        }
        let new_top = self.state.viewport.top_line().saturating_sub(half);
        self.state.viewport.set_top_line(new_top);
        // Move cursor up by same amount
        self.document.cursor.row = self.document.cursor.row.saturating_sub(half);
        self.clamp_cursor_to_line();
    }

    /// Scroll down one line, keeping cursor on screen (Ctrl-e).
    pub(super) fn scroll_line_down(&mut self, count: usize) {
        let buf_len = self.document.buffer.len();
        let new_top = (self.state.viewport.top_line() + count).min(buf_len.saturating_sub(1));
        self.state.viewport.set_top_line(new_top);
        // Clamp cursor to remain within the viewport
        if self.document.cursor.row < new_top {
            self.document.cursor.row = new_top.min(buf_len.saturating_sub(1));
            self.clamp_cursor_to_line();
        }
    }

    /// Scroll up one line, keeping cursor on screen (Ctrl-y).
    pub(super) fn scroll_line_up(&mut self, count: usize) {
        let new_top = self.state.viewport.top_line().saturating_sub(count);
        self.state.viewport.set_top_line(new_top);
        // Clamp cursor to remain within the viewport
        let bottom = new_top + self.state.viewport.height().saturating_sub(1);
        let buf_len = self.document.buffer.len();
        let bottom = bottom.min(buf_len.saturating_sub(1));
        if self.document.cursor.row > bottom {
            self.document.cursor.row = bottom;
            self.clamp_cursor_to_line();
        }
    }

    /// Move cursor to screen-relative position H/M/L.
    ///
    /// `H` goes to the top of the screen (+ offset lines), `M` goes to the
    /// middle, `L` goes to the bottom (- offset lines).
    pub(super) fn move_to_screen_top(&mut self, offset: usize) {
        let buf_len = self.document.buffer.len();
        let target = (self.state.viewport.top_line() + offset).min(buf_len.saturating_sub(1));
        self.document.cursor.row = target;
        self.move_to_first_non_blank();
    }

    pub(super) fn move_to_screen_middle(&mut self) {
        let buf_len = self.document.buffer.len();
        let top = self.state.viewport.top_line();
        let visible_lines = self
            .state
            .viewport
            .height()
            .min(buf_len.saturating_sub(top));
        let target = (top + visible_lines / 2).min(buf_len.saturating_sub(1));
        self.document.cursor.row = target;
        self.move_to_first_non_blank();
    }

    pub(super) fn move_to_screen_bottom(&mut self, offset: usize) {
        let buf_len = self.document.buffer.len();
        let top = self.state.viewport.top_line();
        let bottom =
            (top + self.state.viewport.height().saturating_sub(1)).min(buf_len.saturating_sub(1));
        let target = bottom.saturating_sub(offset);
        // Ensure target >= top
        let target = target.max(top).min(buf_len.saturating_sub(1));
        self.document.cursor.row = target;
        self.move_to_first_non_blank();
    }
}

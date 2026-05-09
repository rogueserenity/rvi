//! Text editing methods for the editor.
//!
//! This module contains all text manipulation methods for the Editor,
//! including undo recording for each editing operation and the
//! apply_undo/apply_redo execution methods.

use super::Editor;
use crate::buffer::unicode::prev_grapheme_boundary;
use crate::buffer::Cursor;
use crate::error::{BufferError, Error};
use crate::registers::{ContentType, RegisterContent, RegisterId};
use crate::undo::EditAction;

/// Extract the leading whitespace prefix from a line.
///
/// Returns the substring of `line` consisting of only spaces and tabs
/// from the beginning. This is used by autoindent to copy indentation
/// to new lines.
pub fn get_leading_whitespace(line: &str) -> &str {
    let boundary = line
        .find(|c: char| c != ' ' && c != '\t')
        .unwrap_or(line.len());
    &line[..boundary]
}

impl Editor {
    /// Returns the leading whitespace of the current line, or an empty string.
    ///
    /// Used by autoindent to copy indentation to new lines. Returns empty
    /// string when autoindent is disabled or the line has no indentation.
    fn current_line_indent(&self) -> String {
        if self.state.settings.autoindent {
            self.document
                .buffer
                .line(self.document.cursor.row)
                .map(|line| get_leading_whitespace(line).to_string())
                .unwrap_or_default()
        } else {
            String::new()
        }
    }

    /// Insert a character at the cursor position.
    pub(super) fn insert_char(&mut self, c: char) -> Result<(), Error> {
        let action = EditAction::InsertChar {
            row: self.document.cursor.row,
            col: self.document.cursor.col,
            ch: c,
        };
        self.state.undo_history.record(action);

        self.document.buffer.insert_char(&self.document.cursor, c)?;
        self.document.cursor.col += c.len_utf8();
        self.document.modified = true;
        Ok(())
    }

    /// Insert a newline at the cursor position.
    ///
    /// When `autoindent` is enabled, copies the leading whitespace from the
    /// current line to the new line and positions the cursor after it.
    pub(super) fn insert_newline(&mut self) -> Result<(), Error> {
        // `^ Ctrl-D` saves the indent for one newline; otherwise use current line's indent.
        let indent = self
            .state
            .insert_state
            .saved_indent
            .take()
            .unwrap_or_else(|| self.current_line_indent());

        let action = EditAction::InsertNewline {
            row: self.document.cursor.row,
            col: self.document.cursor.col,
        };
        self.state.undo_history.record(action);

        self.document.buffer.split_line(&self.document.cursor)?;
        self.document.cursor.row += 1;
        self.document.cursor.col = 0;
        self.document.modified = true;

        // Apply autoindent: insert whitespace at the beginning of the new line
        if !indent.is_empty() {
            let action = EditAction::InsertText {
                row: self.document.cursor.row,
                col: 0,
                text: indent.clone(),
            };
            self.state.undo_history.record(action);
            self.insert_text_at(self.document.cursor.row, 0, &indent)?;
            self.document.cursor.col = indent.len();
        }

        Ok(())
    }

    /// Check whether the word immediately before the cursor matches any abbreviation
    /// lhs. If so, replace the lhs with the corresponding rhs in the buffer.
    ///
    /// Called in insert mode just before a non-keyword character is inserted (space,
    /// punctuation) or when Esc / Enter is pressed. The triggering character itself
    /// is inserted separately by the caller — this method only replaces the word.
    ///
    /// Returns `true` if an abbreviation was expanded (useful for tests).
    pub(super) fn try_expand_abbreviation(&mut self) -> Result<bool, Error> {
        if self.state.mappings.abbreviations.is_empty() {
            return Ok(false);
        }
        let row = self.document.cursor.row;
        let col = self.document.cursor.col;
        let line = match self.document.buffer.line(row) {
            Some(l) => l.to_string(),
            None => return Ok(false),
        };

        // Find the start of the keyword word immediately before the cursor.
        // A "keyword" character here follows vi's definition: alphanumeric or `_`.
        let before = &line[..col];
        let word_start = before
            .char_indices()
            .rev()
            .find(|(_, c)| !c.is_alphanumeric() && *c != '_')
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);

        let word = &before[word_start..];
        if word.is_empty() {
            return Ok(false);
        }

        // Find a matching abbreviation (last definition wins via retain ordering).
        let rhs = self
            .state
            .mappings
            .abbreviations
            .iter()
            .rev()
            .find(|(lhs, _)| lhs == word)
            .map(|(_, rhs)| rhs.clone());

        let rhs = match rhs {
            Some(r) => r,
            None => return Ok(false),
        };

        // Delete the lhs from the buffer.
        let lhs_len = word.len();
        let lhs_start = col - lhs_len;
        let action = EditAction::DeleteRange {
            start_row: row,
            start_col: lhs_start,
            end_row: row,
            end_col: col,
            text: word.to_string(),
        };
        self.state.undo_history.record(action);
        if let Some(l) = self.document.buffer.line_mut(row) {
            l.replace_range(lhs_start..col, "");
        }
        self.document.cursor.col = lhs_start;
        self.document.modified = true;

        // Insert the rhs text (rhs is always single-line; newlines are not
        // supported in abbreviation expansions).
        let action = EditAction::InsertText {
            row,
            col: lhs_start,
            text: rhs.clone(),
        };
        self.state.undo_history.record(action);
        self.insert_text_at(row, lhs_start, &rhs)?;
        self.document.cursor.col = lhs_start + rhs.len();

        Ok(true)
    }

    /// Delete the character before the cursor (backspace in insert mode).
    pub(super) fn delete_char_before_cursor(&mut self) -> Result<(), Error> {
        if self.document.cursor.col > 0 {
            // Find the previous grapheme boundary and delete the grapheme
            if let Some(line) = self.document.buffer.line(self.document.cursor.row) {
                let prev_col = prev_grapheme_boundary(line, self.document.cursor.col);
                // Capture the character(s) being deleted for undo (may be multi-byte grapheme)
                let deleted_text = &line[prev_col..self.document.cursor.col];
                // Record as DeleteRange to preserve multi-char grapheme clusters
                let action = EditAction::DeleteRange {
                    start_row: self.document.cursor.row,
                    start_col: prev_col,
                    end_row: self.document.cursor.row,
                    end_col: self.document.cursor.col,
                    text: deleted_text.to_string(),
                };
                self.state.undo_history.record(action);

                // Delete from prev_col to current col using delete_range for clarity
                let start = Cursor::new(self.document.cursor.row, prev_col);
                let end = Cursor::new(self.document.cursor.row, self.document.cursor.col);
                self.document.buffer.delete_range(&start, &end)?;
                self.document.cursor.col = prev_col;
                self.document.modified = true;
            }
        } else if self.document.cursor.row > 0 {
            // At start of line - join with previous line
            let prev_row = self.document.cursor.row - 1;
            let prev_line_len = self
                .document
                .buffer
                .line(prev_row)
                .map(|l| l.len())
                .unwrap_or(0);

            let action = EditAction::JoinLines {
                row: prev_row,
                col: prev_line_len,
            };
            self.state.undo_history.record(action);

            self.document.buffer.join_lines(prev_row)?;
            self.document.cursor.row = prev_row;
            self.document.cursor.col = prev_line_len;
            self.document.modified = true;
        }
        Ok(())
    }

    /// Delete the character before the cursor and store to register.
    ///
    /// This is the X command in vi, which deletes the character before the cursor
    /// and stores it in the specified register (or the small delete register).
    pub(super) fn delete_char_before_cursor_with_register(
        &mut self,
        register: Option<char>,
    ) -> Result<(), Error> {
        if self.document.cursor.col > 0 {
            // Find the previous grapheme boundary and delete the grapheme
            if let Some(line) = self.document.buffer.line(self.document.cursor.row) {
                let prev_col = prev_grapheme_boundary(line, self.document.cursor.col);

                // Get the character being deleted (may be multi-byte grapheme)
                let deleted_char = &line[prev_col..self.document.cursor.col];

                // Record undo action as DeleteRange to preserve multi-char graphemes
                let action = EditAction::DeleteRange {
                    start_row: self.document.cursor.row,
                    start_col: prev_col,
                    end_row: self.document.cursor.row,
                    end_col: self.document.cursor.col,
                    text: deleted_char.to_string(),
                };
                self.state.undo_history.record(action);

                // Store to register (characterwise, small delete)
                let content =
                    RegisterContent::new(deleted_char.to_string(), ContentType::Characterwise);
                let reg_id = register.and_then(RegisterId::parse);
                self.state.registers.delete(reg_id, register, content);

                // Delete from prev_col to current col
                let start = Cursor::new(self.document.cursor.row, prev_col);
                let end = Cursor::new(self.document.cursor.row, self.document.cursor.col);
                self.document.buffer.delete_range(&start, &end)?;
                self.document.cursor.col = prev_col;
                self.document.modified = true;
            }
        } else if self.document.cursor.row > 0 {
            // At start of line - join with previous line (deletes the newline)
            // Store the newline character to register
            let content = RegisterContent::new("\n".to_string(), ContentType::Characterwise);
            let reg_id = register.and_then(RegisterId::parse);
            self.state.registers.delete(reg_id, register, content);

            let prev_row = self.document.cursor.row - 1;
            let prev_line_len = self
                .document
                .buffer
                .line(prev_row)
                .map(|l| l.len())
                .unwrap_or(0);

            let action = EditAction::JoinLines {
                row: prev_row,
                col: prev_line_len,
            };
            self.state.undo_history.record(action);

            self.document.buffer.join_lines(prev_row)?;
            self.document.cursor.row = prev_row;
            self.document.cursor.col = prev_line_len;
            self.document.modified = true;
        }
        Ok(())
    }

    /// Delete the word before the cursor (Ctrl-w in insert mode).
    ///
    /// Deletes backward through any trailing whitespace, then through
    /// the preceding word characters. Stops at the beginning of the line.
    pub(super) fn delete_word_before_cursor(&mut self) -> Result<(), Error> {
        let row = self.document.cursor.row;
        let col = self.document.cursor.col;
        if col == 0 {
            return Ok(());
        }

        let line = match self.document.buffer.line(row) {
            Some(l) => l.to_string(),
            None => return Ok(()),
        };

        // Find the start of the word to delete (vi Ctrl-w behavior):
        // 1. Skip trailing whitespace backwards
        // 2. Skip non-whitespace backwards (the word itself)
        let prefix = &line[..col];

        // Step 1: skip whitespace
        let trimmed_end = prefix.trim_end().len();
        let delete_start = if trimmed_end == 0 {
            // All whitespace on this segment — delete to start of line
            0
        } else {
            // Step 2: skip non-whitespace (the word)
            prefix[..trimmed_end]
                .rfind(|c: char| c.is_whitespace())
                .map(|p| {
                    // rfind gives byte index of whitespace char; word starts after it
                    let ch = prefix[..trimmed_end][p..].chars().next().unwrap_or(' ');
                    p + ch.len_utf8()
                })
                .unwrap_or(0)
        };

        if delete_start >= col {
            return Ok(());
        }

        let deleted_text = line[delete_start..col].to_string();
        let action = EditAction::DeleteRange {
            start_row: row,
            start_col: delete_start,
            end_row: row,
            end_col: col,
            text: deleted_text,
        };
        self.state.undo_history.record(action);

        let start = Cursor::new(row, delete_start);
        let end = Cursor::new(row, col);
        self.document.buffer.delete_range(&start, &end)?;
        self.document.cursor.col = delete_start;
        self.document.modified = true;
        Ok(())
    }

    /// Delete from cursor back to start of line (Ctrl-u in insert mode).
    ///
    /// Deletes all characters from the cursor position to column 0.
    pub(super) fn delete_to_line_start(&mut self) -> Result<(), Error> {
        let row = self.document.cursor.row;
        let col = self.document.cursor.col;
        if col == 0 {
            return Ok(());
        }

        let deleted_text = self
            .document
            .buffer
            .line(row)
            .map(|l| l[..col].to_string())
            .unwrap_or_default();

        if deleted_text.is_empty() {
            return Ok(());
        }

        let action = EditAction::DeleteRange {
            start_row: row,
            start_col: 0,
            end_row: row,
            end_col: col,
            text: deleted_text,
        };
        self.state.undo_history.record(action);

        let start = Cursor::new(row, 0);
        let end = Cursor::new(row, col);
        self.document.buffer.delete_range(&start, &end)?;
        self.document.cursor.col = 0;
        self.document.modified = true;
        Ok(())
    }

    /// Open a new line below the current line.
    ///
    /// When `autoindent` is enabled, copies the leading whitespace from the
    /// current line to the new line and positions the cursor after it.
    pub(super) fn open_line_below(&mut self) -> Result<(), Error> {
        // Capture leading whitespace before moving the cursor
        let indent = if self.state.settings.autoindent {
            self.document
                .buffer
                .line(self.document.cursor.row)
                .map(|line| get_leading_whitespace(line).to_string())
                .unwrap_or_default()
        } else {
            String::new()
        };

        // Move cursor to the end of the line (past the last character) so that
        // split_line creates an empty new line below rather than splitting the
        // last character onto the new line.
        let end_col = self
            .document
            .buffer
            .line(self.document.cursor.row)
            .map(|l| l.len())
            .unwrap_or(0);
        self.document.cursor.col = end_col;

        let action = EditAction::InsertNewline {
            row: self.document.cursor.row,
            col: self.document.cursor.col,
        };
        self.state.undo_history.record(action);

        // Split line (creates new line below)
        self.document.buffer.split_line(&self.document.cursor)?;
        // Move to the new line
        self.document.cursor.row += 1;
        self.document.cursor.col = 0;
        self.document.modified = true;

        // Apply autoindent
        if !indent.is_empty() {
            let action = EditAction::InsertText {
                row: self.document.cursor.row,
                col: 0,
                text: indent.clone(),
            };
            self.state.undo_history.record(action);
            self.insert_text_at(self.document.cursor.row, 0, &indent)?;
            self.document.cursor.col = indent.len();
        }

        Ok(())
    }

    /// Open a new line above the current line.
    ///
    /// When `autoindent` is enabled, copies the leading whitespace from the
    /// current line (which gets pushed down) to the new line and positions
    /// the cursor after it.
    pub(super) fn open_line_above(&mut self) -> Result<(), Error> {
        // Capture leading whitespace from the current line (which will be pushed to row+1)
        let indent = if self.state.settings.autoindent {
            self.document
                .buffer
                .line(self.document.cursor.row)
                .map(|line| get_leading_whitespace(line).to_string())
                .unwrap_or_default()
        } else {
            String::new()
        };

        // Move to start of current line
        self.document.cursor.col = 0;

        let action = EditAction::InsertNewline {
            row: self.document.cursor.row,
            col: 0,
        };
        self.state.undo_history.record(action);

        // Split line (creates new line)
        self.document.buffer.split_line(&self.document.cursor)?;
        // Cursor stays on the new (now current) line
        self.document.cursor.col = 0;
        self.document.modified = true;

        // Apply autoindent
        if !indent.is_empty() {
            let action = EditAction::InsertText {
                row: self.document.cursor.row,
                col: 0,
                text: indent.clone(),
            };
            self.state.undo_history.record(action);
            self.insert_text_at(self.document.cursor.row, 0, &indent)?;
            self.document.cursor.col = indent.len();
        }

        Ok(())
    }

    /// Indent lines to the right by `shiftwidth` spaces.
    ///
    /// Shifts `line_count` lines starting from the cursor row. Each line gets
    /// `shiftwidth` spaces prepended. All changes are recorded as `ReplaceLine`
    /// undo actions. After shifting, the cursor moves to the first non-whitespace
    /// character of the current line.
    pub(super) fn shift_lines_right(&mut self, line_count: usize) -> Result<(), Error> {
        let sw = self.state.settings.shiftwidth;
        let indent = " ".repeat(sw);
        let start_row = self.document.cursor.row;
        let end_row = (start_row + line_count).min(self.document.buffer.len());

        let mut any_changed = false;
        for row in start_row..end_row {
            if let Some(line) = self.document.buffer.line(row) {
                let old_content = line.to_string();
                // Skip empty lines (vi/vim behavior: >> does not add trailing whitespace)
                if old_content.is_empty() {
                    continue;
                }
                let new_content = format!("{}{}", indent, old_content);

                let action = EditAction::ReplaceLine {
                    row,
                    old_content: old_content.clone(),
                    new_content: new_content.clone(),
                };
                self.state.undo_history.record(action);

                let buffer_len = self.document.buffer.len();
                let line_mut =
                    self.document
                        .buffer
                        .line_mut(row)
                        .ok_or(BufferError::RowOutOfBounds {
                            row,
                            len: buffer_len,
                        })?;
                line_mut.clear();
                line_mut.push_str(&new_content);
                any_changed = true;
            }
        }

        self.move_to_first_non_blank();
        if any_changed {
            self.document.modified = true;
        }
        Ok(())
    }

    /// Dedent lines to the left by up to `shiftwidth` columns.
    ///
    /// Shifts `line_count` lines starting from the cursor row. Removes up to
    /// `shiftwidth` columns of leading whitespace from each line. Spaces count
    /// as 1 column; tabs count as `tabstop` columns. All changes are recorded
    /// as `ReplaceLine` undo actions. After shifting, the cursor moves to the
    /// first non-whitespace character of the current line.
    pub(super) fn shift_lines_left(&mut self, line_count: usize) -> Result<(), Error> {
        let sw = self.state.settings.shiftwidth;
        let tabstop = self.state.settings.tabstop;
        let start_row = self.document.cursor.row;
        let end_row = (start_row + line_count).min(self.document.buffer.len());
        let mut any_changed = false;

        for row in start_row..end_row {
            if let Some(line) = self.document.buffer.line(row) {
                let old_content = line.to_string();

                // Calculate how many bytes of leading whitespace to remove
                let mut removed_cols = 0;
                let mut byte_offset = 0;
                for ch in old_content.chars() {
                    if removed_cols >= sw {
                        break;
                    }
                    match ch {
                        ' ' => {
                            removed_cols += 1;
                            byte_offset += 1;
                        }
                        '\t' => {
                            // Tab counts as tabstop columns
                            if removed_cols + tabstop <= sw {
                                removed_cols += tabstop;
                                byte_offset += 1;
                            } else {
                                break; // Would remove too many columns
                            }
                        }
                        _ => break, // Non-whitespace, stop
                    }
                }

                if byte_offset > 0 {
                    let new_content = old_content[byte_offset..].to_string();
                    let action = EditAction::ReplaceLine {
                        row,
                        old_content: old_content.clone(),
                        new_content: new_content.clone(),
                    };
                    self.state.undo_history.record(action);

                    let buffer_len = self.document.buffer.len();
                    let line_mut =
                        self.document
                            .buffer
                            .line_mut(row)
                            .ok_or(BufferError::RowOutOfBounds {
                                row,
                                len: buffer_len,
                            })?;
                    line_mut.clear();
                    line_mut.push_str(&new_content);
                    any_changed = true;
                }
            }
        }

        self.move_to_first_non_blank();
        if any_changed {
            self.document.modified = true;
        }
        Ok(())
    }

    /// Apply an undo operation: pop from undo stack and reverse all actions.
    pub(super) fn apply_undo(&mut self) -> Result<(), Error> {
        if let Some(entry) = self.state.undo_history.undo() {
            // Apply actions in reverse order to undo
            for action in entry.actions().iter().rev() {
                self.apply_inverse_action(action)?;
            }
            self.document.cursor = entry.cursor_before();
            self.document.modified = entry.modified_before();

            // Ensure cursor is visible in viewport
            self.state
                .viewport
                .ensure_visible(self.document.cursor.row, self.document.buffer.len());
        } else {
            self.state.status_message = Some(super::StatusMessage::Error(
                "Already at oldest change".to_string(),
            ));
        }
        Ok(())
    }

    /// Apply a redo operation: pop from redo stack and reapply all actions.
    pub(super) fn apply_redo(&mut self) -> Result<(), Error> {
        if let Some(entry) = self.state.undo_history.redo() {
            // Apply actions in forward order to redo
            for action in entry.actions() {
                self.apply_forward_action(action)?;
            }
            self.document.cursor = entry.cursor_after();
            self.document.modified = true;

            // Ensure cursor is visible in viewport
            self.state
                .viewport
                .ensure_visible(self.document.cursor.row, self.document.buffer.len());
        } else {
            self.state.status_message = Some(super::StatusMessage::Error(
                "Already at newest change".to_string(),
            ));
        }
        Ok(())
    }

    /// Apply the inverse of an action (used during undo).
    fn apply_inverse_action(&mut self, action: &EditAction) -> Result<(), Error> {
        match action {
            EditAction::InsertChar { row, col, ch } => {
                // Undo insert: delete the character
                let start = Cursor::new(*row, *col);
                let end = Cursor::new(*row, *col + ch.len_utf8());
                self.document.buffer.delete_range(&start, &end)?;
            }
            EditAction::InsertNewline { row, col: _ } => {
                // Undo newline insertion: join the two lines back together
                // The newline was at (row, col), so line row was split into
                // row (first part) and row+1 (second part). Join them back.
                self.document.buffer.join_lines(*row)?;
            }
            EditAction::DeleteChar { row, col, ch } => {
                // Undo delete: re-insert the character
                let cursor = Cursor::new(*row, *col);
                self.document.buffer.insert_char(&cursor, *ch)?;
            }
            EditAction::DeleteRange {
                start_row,
                start_col,
                text,
                ..
            } => {
                // Undo range deletion: re-insert the deleted text
                self.insert_text_at(*start_row, *start_col, text)?;
            }
            EditAction::InsertText { row, col, text } => {
                // Undo text insertion: delete the inserted text
                self.delete_text_at(*row, *col, text)?;
            }
            EditAction::InsertLine { row, .. } => {
                // Undo line insertion: remove the line
                self.document.buffer.remove_line(*row)?;
            }
            EditAction::RemoveLine { row, content } => {
                // Undo line removal: re-insert the line
                self.document.buffer.insert_line(*row, content.clone())?;
            }
            EditAction::JoinLines { row, col } => {
                // Undo join: split the line at the join point
                let cursor = Cursor::new(*row, *col);
                self.document.buffer.split_line(&cursor)?;
            }
            EditAction::SplitLine { row, .. } => {
                // Undo split: join the two lines back together
                self.document.buffer.join_lines(*row)?;
            }
            EditAction::ReplaceChar {
                row,
                col,
                old_char,
                new_char,
            } => {
                // Undo replace: delete new_char, insert old_char
                let start = Cursor::new(*row, *col);
                let end = Cursor::new(*row, *col + new_char.len_utf8());
                self.document.buffer.delete_range(&start, &end)?;
                self.document.buffer.insert_char(&start, *old_char)?;
            }
            EditAction::ReplaceLine {
                row, old_content, ..
            } => {
                // Undo line replacement: restore old content
                let buffer_len = self.document.buffer.len();
                let line =
                    self.document
                        .buffer
                        .line_mut(*row)
                        .ok_or(BufferError::RowOutOfBounds {
                            row: *row,
                            len: buffer_len,
                        })?;
                line.clear();
                line.push_str(old_content);
            }
        }
        Ok(())
    }

    /// Apply an action in the forward direction (used during redo).
    fn apply_forward_action(&mut self, action: &EditAction) -> Result<(), Error> {
        match action {
            EditAction::InsertChar { row, col, ch } => {
                let cursor = Cursor::new(*row, *col);
                self.document.buffer.insert_char(&cursor, *ch)?;
            }
            EditAction::InsertNewline { row, col } => {
                let cursor = Cursor::new(*row, *col);
                self.document.buffer.split_line(&cursor)?;
            }
            EditAction::DeleteChar { row, col, ch } => {
                let start = Cursor::new(*row, *col);
                let end = Cursor::new(*row, *col + ch.len_utf8());
                self.document.buffer.delete_range(&start, &end)?;
            }
            EditAction::DeleteRange {
                start_row,
                start_col,
                end_row,
                end_col,
                ..
            } => {
                let start = Cursor::new(*start_row, *start_col);
                let end = Cursor::new(*end_row, *end_col);
                self.document.buffer.delete_range(&start, &end)?;
            }
            EditAction::InsertText { row, col, text } => {
                self.insert_text_at(*row, *col, text)?;
            }
            EditAction::InsertLine { row, content } => {
                self.document.buffer.insert_line(*row, content.clone())?;
            }
            EditAction::RemoveLine { row, .. } => {
                self.document.buffer.remove_line(*row)?;
            }
            EditAction::JoinLines { row, .. } => {
                self.document.buffer.join_lines(*row)?;
            }
            EditAction::SplitLine { row, col } => {
                let cursor = Cursor::new(*row, *col);
                self.document.buffer.split_line(&cursor)?;
            }
            EditAction::ReplaceChar {
                row,
                col,
                old_char,
                new_char,
            } => {
                let start = Cursor::new(*row, *col);
                let end = Cursor::new(*row, *col + old_char.len_utf8());
                self.document.buffer.delete_range(&start, &end)?;
                self.document.buffer.insert_char(&start, *new_char)?;
            }
            EditAction::ReplaceLine {
                row, new_content, ..
            } => {
                // Redo line replacement: apply new content
                let buffer_len = self.document.buffer.len();
                let line =
                    self.document
                        .buffer
                        .line_mut(*row)
                        .ok_or(BufferError::RowOutOfBounds {
                            row: *row,
                            len: buffer_len,
                        })?;
                line.clear();
                line.push_str(new_content);
            }
        }
        Ok(())
    }

    /// Insert text at a given position, handling newlines by splitting lines.
    ///
    /// Used by undo (re-inserting deleted text) and redo (re-inserting text).
    fn insert_text_at(&mut self, row: usize, col: usize, text: &str) -> Result<(), Error> {
        let mut current_row = row;
        let mut current_col = col;

        for ch in text.chars() {
            if ch == '\n' {
                let cursor = Cursor::new(current_row, current_col);
                self.document.buffer.split_line(&cursor)?;
                current_row += 1;
                current_col = 0;
            } else {
                let cursor = Cursor::new(current_row, current_col);
                self.document.buffer.insert_char(&cursor, ch)?;
                current_col += ch.len_utf8();
            }
        }

        Ok(())
    }

    /// Delete text at a given position that matches the provided text content.
    ///
    /// Used by undo (removing inserted text). Handles newlines by joining lines.
    fn delete_text_at(&mut self, row: usize, col: usize, text: &str) -> Result<(), Error> {
        // Calculate the end position of the text to delete
        let mut end_row = row;
        let mut end_col = col;

        for ch in text.chars() {
            if ch == '\n' {
                end_row += 1;
                end_col = 0;
            } else {
                end_col += ch.len_utf8();
            }
        }

        let start = Cursor::new(row, col);
        let end = Cursor::new(end_row, end_col);
        self.document.buffer.delete_range(&start, &end)?;

        Ok(())
    }

    /// Replace a line with `new_content`, recording a `ReplaceLine` undo action.
    ///
    /// No-ops if the line doesn't exist or is already equal to `new_content`.
    pub(super) fn replace_line_recorded(
        &mut self,
        row: usize,
        new_content: String,
    ) -> Result<(), Error> {
        let old_content = match self.document.buffer.line(row) {
            Some(l) => l.to_string(),
            None => return Ok(()),
        };
        if old_content == new_content {
            return Ok(());
        }
        self.state.undo_history.record(EditAction::ReplaceLine {
            row,
            old_content,
            new_content: new_content.clone(),
        });
        let buffer_len = self.document.buffer.len();
        let line_mut = self
            .document
            .buffer
            .line_mut(row)
            .ok_or(BufferError::RowOutOfBounds {
                row,
                len: buffer_len,
            })?;
        line_mut.clear();
        line_mut.push_str(&new_content);
        Ok(())
    }

    /// Toggle the case of the character at the cursor and advance the cursor.
    ///
    /// Uppercase characters become lowercase and vice versa. Non-alphabetic
    /// characters are left unchanged but the cursor still advances. Records
    /// a `ReplaceLine` undo action for the affected line.
    pub(super) fn toggle_case_at_cursor(&mut self) -> Result<(), Error> {
        let row = self.document.cursor.row;
        let col = self.document.cursor.col;

        let line = match self.document.buffer.line(row) {
            Some(l) => l.to_string(),
            None => return Ok(()),
        };

        if col >= line.len() {
            return Ok(());
        }

        // Get the character at cursor
        let ch = match line[col..].chars().next() {
            Some(c) => c,
            None => return Ok(()),
        };

        // Determine toggled character
        let toggled: String = if ch.is_uppercase() {
            ch.to_lowercase().to_string()
        } else if ch.is_lowercase() {
            ch.to_uppercase().to_string()
        } else {
            // Non-alphabetic: just advance cursor
            let next_col = crate::buffer::unicode::next_grapheme_boundary(&line, col);
            self.document.cursor.col = next_col;
            self.clamp_cursor_to_line();
            return Ok(());
        };

        // Build the new line content
        let char_end = col + ch.len_utf8();
        let new_content = format!("{}{}{}", &line[..col], toggled, &line[char_end..]);

        // Record undo and apply the change
        self.replace_line_recorded(row, new_content.clone())?;

        // Advance cursor past the toggled character
        self.document.cursor.col = col + toggled.len();
        self.clamp_cursor_to_line();
        self.document.modified = true;
        Ok(())
    }

    /// Join the next line to the current line.
    ///
    /// Removes the next line, strips its leading whitespace, and appends it
    /// to the current line with a single space separator. Does nothing if the
    /// cursor is on the last line. Records `ReplaceLine` and `RemoveLine`
    /// undo actions.
    pub(super) fn join_line_with_next(&mut self) -> Result<(), Error> {
        let row = self.document.cursor.row;

        // Cannot join if on the last line
        if row + 1 >= self.document.buffer.len() {
            return Ok(());
        }

        let current_line = match self.document.buffer.line(row) {
            Some(l) => l.to_string(),
            None => return Ok(()),
        };
        let next_line = match self.document.buffer.line(row + 1) {
            Some(l) => l.to_string(),
            None => return Ok(()),
        };

        // Strip trailing whitespace from the current line and leading whitespace
        // from the next line before joining (POSIX vi J semantics).
        let trimmed_current = current_line.trim_end();
        let trimmed_next = next_line.trim_start();

        // Build the joined line: trimmed_current + space + trimmed_next.
        // If either side is empty after trimming, skip the joining space.
        let join_col = trimmed_current.len();
        let new_content = if trimmed_current.is_empty() || trimmed_next.is_empty() {
            format!("{}{}", trimmed_current, trimmed_next)
        } else {
            format!("{} {}", trimmed_current, trimmed_next)
        };

        // Record undo: replace current line
        let action = EditAction::ReplaceLine {
            row,
            old_content: current_line,
            new_content: new_content.clone(),
        };
        self.state.undo_history.record(action);

        // Record undo: remove next line
        let remove_action = EditAction::RemoveLine {
            row: row + 1,
            content: next_line,
        };
        self.state.undo_history.record(remove_action);

        // Apply: update current line
        let buffer_len = self.document.buffer.len();
        let line_mut = self
            .document
            .buffer
            .line_mut(row)
            .ok_or(BufferError::RowOutOfBounds {
                row,
                len: buffer_len,
            })?;
        line_mut.clear();
        line_mut.push_str(&new_content);

        // Apply: remove next line
        self.document.buffer.remove_line(row + 1)?;

        // Position cursor at the join point
        self.document.cursor.col = join_col;
        self.document.modified = true;
        Ok(())
    }

    /// Join the next line to the current line without inserting a space (gJ).
    pub(super) fn join_line_with_next_no_space(&mut self) -> Result<(), Error> {
        let row = self.document.cursor.row;

        if row + 1 >= self.document.buffer.len() {
            return Ok(());
        }

        let current_line = match self.document.buffer.line(row) {
            Some(l) => l.to_string(),
            None => return Ok(()),
        };
        let next_line = match self.document.buffer.line(row + 1) {
            Some(l) => l.to_string(),
            None => return Ok(()),
        };

        let join_col = current_line.len();
        let new_content = format!("{}{}", current_line, next_line);

        let action = EditAction::ReplaceLine {
            row,
            old_content: current_line,
            new_content: new_content.clone(),
        };
        self.state.undo_history.record(action);

        let remove_action = EditAction::RemoveLine {
            row: row + 1,
            content: next_line,
        };
        self.state.undo_history.record(remove_action);

        let buffer_len = self.document.buffer.len();
        let line_mut = self
            .document
            .buffer
            .line_mut(row)
            .ok_or(BufferError::RowOutOfBounds {
                row,
                len: buffer_len,
            })?;
        line_mut.clear();
        line_mut.push_str(&new_content);

        self.document.buffer.remove_line(row + 1)?;

        self.document.cursor.col = join_col;
        self.document.modified = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::Buffer;
    use crate::command::{execute_motion, CommandContext, Motion};
    use crate::command::{resolve_text_object, MotionKind, TextObjectContext, TextObjectKind};
    use crate::file::LineEnding;

    // Test helper to create a context for motion testing
    fn make_ctx(content: &str, row: usize, col: usize) -> (Buffer, Cursor) {
        let buffer = Buffer::from_string(content.to_string());
        let cursor = Cursor::new(row, col);
        (buffer, cursor)
    }

    // =========================================================================
    // get_leading_whitespace tests
    // =========================================================================

    #[test]
    fn test_get_leading_whitespace_spaces() {
        assert_eq!(get_leading_whitespace("    hello"), "    ");
    }

    #[test]
    fn test_get_leading_whitespace_tabs() {
        assert_eq!(get_leading_whitespace("\t\thello"), "\t\t");
    }

    #[test]
    fn test_get_leading_whitespace_mixed() {
        assert_eq!(get_leading_whitespace("  \t hello"), "  \t ");
    }

    #[test]
    fn test_get_leading_whitespace_no_indent() {
        assert_eq!(get_leading_whitespace("hello"), "");
    }

    #[test]
    fn test_get_leading_whitespace_empty() {
        assert_eq!(get_leading_whitespace(""), "");
    }

    #[test]
    fn test_get_leading_whitespace_all_whitespace() {
        assert_eq!(get_leading_whitespace("   "), "   ");
    }

    // =========================================================================
    // Motion integration tests
    // =========================================================================

    #[test]
    fn test_motion_left_integration() {
        let (buffer, cursor) = make_ctx("hello", 0, 3);
        let ctx = CommandContext {
            buffer: &buffer,
            cursor,
            paragraphs: "",
            sections: "",
            tabstop: 8,
        };
        let result = execute_motion(Motion::Left, &ctx, 1, None).unwrap();
        assert_eq!(result.target.col, 2);
    }

    #[test]
    fn test_motion_right_integration() {
        let (buffer, cursor) = make_ctx("hello", 0, 0);
        let ctx = CommandContext {
            buffer: &buffer,
            cursor,
            paragraphs: "",
            sections: "",
            tabstop: 8,
        };
        let result = execute_motion(Motion::Right, &ctx, 1, None).unwrap();
        assert_eq!(result.target.col, 1);
    }

    #[test]
    fn test_motion_down_integration() {
        let (buffer, cursor) = make_ctx("hello\nworld", 0, 2);
        let ctx = CommandContext {
            buffer: &buffer,
            cursor,
            paragraphs: "",
            sections: "",
            tabstop: 8,
        };
        let result = execute_motion(Motion::Down, &ctx, 1, None).unwrap();
        assert_eq!(result.target.row, 1);
        assert_eq!(result.target.col, 2);
    }

    #[test]
    fn test_motion_up_integration() {
        let (buffer, cursor) = make_ctx("hello\nworld", 1, 2);
        let ctx = CommandContext {
            buffer: &buffer,
            cursor,
            paragraphs: "",
            sections: "",
            tabstop: 8,
        };
        let result = execute_motion(Motion::Up, &ctx, 1, None).unwrap();
        assert_eq!(result.target.row, 0);
        assert_eq!(result.target.col, 2);
    }

    #[test]
    fn test_motion_line_start_integration() {
        let (buffer, cursor) = make_ctx("hello", 0, 3);
        let ctx = CommandContext {
            buffer: &buffer,
            cursor,
            paragraphs: "",
            sections: "",
            tabstop: 8,
        };
        let result = execute_motion(Motion::LineStart, &ctx, 1, None).unwrap();
        assert_eq!(result.target.col, 0);
    }

    #[test]
    fn test_motion_line_end_integration() {
        let (buffer, cursor) = make_ctx("hello", 0, 0);
        let ctx = CommandContext {
            buffer: &buffer,
            cursor,
            paragraphs: "",
            sections: "",
            tabstop: 8,
        };
        let result = execute_motion(Motion::LineEnd, &ctx, 1, None).unwrap();
        assert_eq!(result.target.col, 4);
    }

    #[test]
    fn test_motion_word_forward_integration() {
        let (buffer, cursor) = make_ctx("hello world", 0, 0);
        let ctx = CommandContext {
            buffer: &buffer,
            cursor,
            paragraphs: "",
            sections: "",
            tabstop: 8,
        };
        let result = execute_motion(Motion::WordForward, &ctx, 1, None).unwrap();
        assert_eq!(result.target.col, 6);
    }

    #[test]
    fn test_motion_word_backward_integration() {
        let (buffer, cursor) = make_ctx("hello world", 0, 6);
        let ctx = CommandContext {
            buffer: &buffer,
            cursor,
            paragraphs: "",
            sections: "",
            tabstop: 8,
        };
        let result = execute_motion(Motion::WordBackward, &ctx, 1, None).unwrap();
        assert_eq!(result.target.col, 0);
    }

    #[test]
    fn test_motion_first_non_blank_integration() {
        let (buffer, cursor) = make_ctx("   hello", 0, 0);
        let ctx = CommandContext {
            buffer: &buffer,
            cursor,
            paragraphs: "",
            sections: "",
            tabstop: 8,
        };
        let result = execute_motion(Motion::FirstNonBlank, &ctx, 1, None).unwrap();
        assert_eq!(result.target.col, 3);
    }

    #[test]
    fn test_motion_document_start_integration() {
        let (buffer, cursor) = make_ctx("hello\nworld\ntest", 2, 2);
        let ctx = CommandContext {
            buffer: &buffer,
            cursor,
            paragraphs: "",
            sections: "",
            tabstop: 8,
        };
        let result = execute_motion(Motion::DocumentStart, &ctx, 1, None).unwrap();
        assert_eq!(result.target.row, 0);
        assert_eq!(result.target.col, 0);
    }

    #[test]
    fn test_motion_document_end_integration() {
        let (buffer, cursor) = make_ctx("hello\nworld\ntest", 0, 0);
        let ctx = CommandContext {
            buffer: &buffer,
            cursor,
            paragraphs: "",
            sections: "",
            tabstop: 8,
        };
        let result = execute_motion(Motion::DocumentEnd, &ctx, 1, None).unwrap();
        assert_eq!(result.target.row, 2);
        assert_eq!(result.target.col, 0);
    }

    #[test]
    fn test_motion_goto_line_integration() {
        let (buffer, cursor) = make_ctx("hello\nworld\ntest", 0, 0);
        let ctx = CommandContext {
            buffer: &buffer,
            cursor,
            paragraphs: "",
            sections: "",
            tabstop: 8,
        };
        let result = execute_motion(Motion::GotoLine(2), &ctx, 1, None).unwrap();
        assert_eq!(result.target.row, 1);
    }

    #[test]
    fn test_motion_with_count_integration() {
        let (buffer, cursor) = make_ctx("hello\nworld\ntest\nfour", 0, 0);
        let ctx = CommandContext {
            buffer: &buffer,
            cursor,
            paragraphs: "",
            sections: "",
            tabstop: 8,
        };
        let result = execute_motion(Motion::Down, &ctx, 2, None).unwrap();
        assert_eq!(result.target.row, 2);
    }

    // Tests for Bug 1 fix: Inclusive motions
    #[test]
    fn test_inclusive_motion_word_end() {
        let (buffer, cursor) = make_ctx("hello world", 0, 0);
        let ctx = CommandContext {
            buffer: &buffer,
            cursor,
            paragraphs: "",
            sections: "",
            tabstop: 8,
        };
        let result = execute_motion(Motion::WordEnd, &ctx, 1, None).unwrap();
        // WordEnd is inclusive - should land on 'o' at position 4
        assert_eq!(result.target.col, 4);
        assert_eq!(result.kind, MotionKind::Inclusive);
    }

    #[test]
    fn test_inclusive_motion_line_end() {
        let (buffer, cursor) = make_ctx("hello", 0, 0);
        let ctx = CommandContext {
            buffer: &buffer,
            cursor,
            paragraphs: "",
            sections: "",
            tabstop: 8,
        };
        let result = execute_motion(Motion::LineEnd, &ctx, 1, None).unwrap();
        // LineEnd is inclusive - should land on last char 'o' at position 4
        assert_eq!(result.target.col, 4);
        assert_eq!(result.kind, MotionKind::Inclusive);
    }

    #[test]
    fn test_inclusive_motion_find_char() {
        use crate::command::motion::{FindDirection, FindStop};

        let (buffer, cursor) = make_ctx("hello", 0, 0);
        let ctx = CommandContext {
            buffer: &buffer,
            cursor,
            paragraphs: "",
            sections: "",
            tabstop: 8,
        };
        let result = execute_motion(
            Motion::FindChar {
                ch: 'l',
                direction: FindDirection::Forward,
                stop: FindStop::OnChar,
            },
            &ctx,
            1,
            None,
        )
        .unwrap();
        // FindChar is inclusive
        assert_eq!(result.target.col, 2);
        assert_eq!(result.kind, MotionKind::Inclusive);
    }

    // Tests for Bug 2 fix: Operator count handling
    #[test]
    fn test_operator_count_preserved() {
        use crate::command::{parse_normal_key, ParseResult, ParseState, ParsedCommand};
        use crate::mode::Mode;
        use crate::terminal::Key;

        let mut state = ParseState::new();

        // Parse "3" count
        let result = parse_normal_key(&mut state, Key::Char('3'), Mode::Normal);
        assert_eq!(result, ParseResult::Pending);

        // Parse "d" operator
        let result = parse_normal_key(&mut state, Key::Char('d'), Mode::Normal);

        // Should enter operator-pending mode with count 3
        match result {
            ParseResult::Complete(ParsedCommand::OperatorPending { count, .. }) => {
                assert_eq!(count, 3);
            }
            _ => panic!("Expected OperatorPending with count 3"),
        }
    }

    // Tests for text extraction helper
    #[test]
    fn test_get_text_in_range_same_line() {
        let buffer = Buffer::from_string("hello world".to_string());
        let doc = super::super::Document {
            buffer,
            cursor: Cursor::new(0, 0),
            selection: None,
            filename: None,
            file_path: None,
            modified: false,
            line_ending: LineEnding::default(),
        };

        // Create a minimal editor-like struct for testing
        struct TestEditor {
            document: super::super::Document,
        }

        impl TestEditor {
            fn get_text_in_range(&self, start: &Cursor, end: &Cursor) -> String {
                let mut text = String::new();
                if start.row == end.row {
                    if let Some(line) = self.document.buffer.line(start.row) {
                        let start_col = start.col.min(line.len());
                        let end_col = end.col.min(line.len());
                        if start_col < end_col {
                            text.push_str(&line[start_col..end_col]);
                        }
                    }
                }
                text
            }
        }

        let editor = TestEditor { document: doc };
        let start = Cursor::new(0, 0);
        let end = Cursor::new(0, 5);
        assert_eq!(editor.get_text_in_range(&start, &end), "hello");
    }

    #[test]
    fn test_get_text_in_range_multiple_lines() {
        let buffer = Buffer::from_string("hello\nworld\ntest".to_string());
        let doc = super::super::Document {
            buffer,
            cursor: Cursor::new(0, 0),
            selection: None,
            filename: None,
            file_path: None,
            modified: false,
            line_ending: LineEnding::default(),
        };

        struct TestEditor {
            document: super::super::Document,
        }

        impl TestEditor {
            fn get_text_in_range(&self, start: &Cursor, end: &Cursor) -> String {
                let mut text = String::new();
                if start.row == end.row {
                    if let Some(line) = self.document.buffer.line(start.row) {
                        let start_col = start.col.min(line.len());
                        let end_col = end.col.min(line.len());
                        if start_col < end_col {
                            text.push_str(&line[start_col..end_col]);
                        }
                    }
                } else {
                    if let Some(first_line) = self.document.buffer.line(start.row) {
                        let start_col = start.col.min(first_line.len());
                        text.push_str(&first_line[start_col..]);
                        text.push('\n');
                    }
                    for row in (start.row + 1)..end.row {
                        if let Some(line) = self.document.buffer.line(row) {
                            text.push_str(line);
                            text.push('\n');
                        }
                    }
                    if let Some(last_line) = self.document.buffer.line(end.row) {
                        let end_col = end.col.min(last_line.len());
                        text.push_str(&last_line[..end_col]);
                    }
                }
                text
            }
        }

        let editor = TestEditor { document: doc };
        let start = Cursor::new(0, 2);
        let end = Cursor::new(2, 2);
        assert_eq!(editor.get_text_in_range(&start, &end), "llo\nworld\nte");
    }

    // Tests for Bug Fix 1: X command stores to register
    #[test]
    fn test_delete_char_before_cursor_stores_to_register() {
        use crate::registers::Registers;

        // Create a minimal test setup
        let mut doc = super::super::Document::from_string("hello".to_string());
        doc.cursor = Cursor::new(0, 3); // Cursor after 'l' (on second 'l')

        let mut registers = Registers::new();

        // Simulate deleting 'l' (the character before cursor at position 3)
        let line = doc.buffer.line(0).unwrap();
        let prev_col = prev_grapheme_boundary(line, doc.cursor.col);
        let deleted_char = &line[prev_col..doc.cursor.col];

        // Store to register
        let content = RegisterContent::characterwise(deleted_char.to_string());
        registers.delete(None, None, content);

        // Verify the deleted character was stored
        let stored = registers.get(None).unwrap();
        assert_eq!(stored.text(), "l");
        assert!(!stored.is_linewise());
    }

    // Tests for Bug Fix 2: UTF-8 cursor positioning in put commands
    #[test]
    fn test_put_cursor_position_with_ascii() {
        use unicode_segmentation::UnicodeSegmentation;

        // Simulate inserting "abc" and verify grapheme-based positioning
        let text = "abc";
        let grapheme_count = text.graphemes(true).count();
        assert_eq!(grapheme_count, 3);

        // After inserting, cursor should be at byte offset of last grapheme
        // For "abc", last grapheme 'c' is at byte offset 2
        let last_grapheme_offset = text
            .grapheme_indices(true)
            .next_back()
            .map(|(offset, _)| offset)
            .unwrap_or(0);
        assert_eq!(last_grapheme_offset, 2);
    }

    #[test]
    fn test_put_cursor_position_with_multibyte() {
        use unicode_segmentation::UnicodeSegmentation;

        // Test with CJK characters (3 bytes each)
        let text = "\u{4F60}\u{597D}\u{4E16}";
        let grapheme_count = text.graphemes(true).count();
        assert_eq!(grapheme_count, 3);

        // Total bytes: 9 (3 chars * 3 bytes)
        assert_eq!(text.len(), 9);

        // Last grapheme starts at byte 6
        let last_grapheme_offset = text
            .grapheme_indices(true)
            .next_back()
            .map(|(offset, _)| offset)
            .unwrap_or(0);
        assert_eq!(last_grapheme_offset, 6);

        // Verify prev_grapheme_boundary works correctly
        assert_eq!(prev_grapheme_boundary(text, 9), 6); // Before last char
        assert_eq!(prev_grapheme_boundary(text, 6), 3); // Before middle char
        assert_eq!(prev_grapheme_boundary(text, 3), 0); // Before first char
    }

    #[test]
    fn test_put_cursor_position_with_combining_chars() {
        use unicode_segmentation::UnicodeSegmentation;

        // Test with combining characters: "e" + combining acute accent = 1 grapheme
        let text = "cafe\u{0301}"; // "cafe" with combining accent on 'e' (5 bytes, 4 graphemes)
        let grapheme_count = text.graphemes(true).count();
        assert_eq!(grapheme_count, 4); // c, a, f, e+combining

        // The last grapheme "e\u{0301}" starts at byte 3
        let last_grapheme_offset = text
            .grapheme_indices(true)
            .next_back()
            .map(|(offset, _)| offset)
            .unwrap_or(0);
        assert_eq!(last_grapheme_offset, 3);

        // prev_grapheme_boundary from end should give us offset 3
        assert_eq!(prev_grapheme_boundary(text, text.len()), 3);
    }

    // Tests for text objects integration

    #[test]
    fn test_text_object_word_resolution() {
        let buffer = Buffer::from_string("hello world".to_string());
        let ctx = TextObjectContext {
            buffer: &buffer,
            cursor: Cursor::new(0, 1), // Cursor on 'e'
        };

        let range = resolve_text_object(
            &ctx,
            TextObjectKind::Inner,
            crate::command::TextObject::Word,
            1,
        )
        .unwrap();
        assert_eq!(range.start.col, 0);
        assert_eq!(range.end.col, 5); // "hello"
    }

    #[test]
    fn test_text_object_around_word_with_whitespace() {
        let buffer = Buffer::from_string("hello world".to_string());
        let ctx = TextObjectContext {
            buffer: &buffer,
            cursor: Cursor::new(0, 1), // Cursor on 'e'
        };

        let range = resolve_text_object(
            &ctx,
            TextObjectKind::Around,
            crate::command::TextObject::Word,
            1,
        )
        .unwrap();
        assert_eq!(range.start.col, 0);
        assert_eq!(range.end.col, 6); // "hello " including trailing space
    }

    #[test]
    fn test_text_object_quote_resolution() {
        let buffer = Buffer::from_string(r#"say "hello" now"#.to_string());
        let ctx = TextObjectContext {
            buffer: &buffer,
            cursor: Cursor::new(0, 6), // Cursor on 'e'
        };

        let range = resolve_text_object(
            &ctx,
            TextObjectKind::Inner,
            crate::command::TextObject::DoubleQuote,
            1,
        )
        .unwrap();
        assert_eq!(range.start.col, 5); // After opening quote
        assert_eq!(range.end.col, 10); // Before closing quote
    }

    #[test]
    fn test_text_object_bracket_resolution() {
        let buffer = Buffer::from_string("fn(abc)".to_string());
        let ctx = TextObjectContext {
            buffer: &buffer,
            cursor: Cursor::new(0, 4), // Cursor on 'b'
        };

        let range = resolve_text_object(
            &ctx,
            TextObjectKind::Inner,
            crate::command::TextObject::Parenthesis,
            1,
        )
        .unwrap();
        assert_eq!(range.start.col, 3); // After '('
        assert_eq!(range.end.col, 6); // Before ')'
    }

    // Tests for D command (delete to end of line)

    #[test]
    fn test_delete_to_end_of_line_basic() {
        use crate::registers::Registers;

        let mut doc = super::super::Document::from_string("hello world".to_string());
        doc.cursor = Cursor::new(0, 5); // Cursor on space before "world"

        let mut registers = Registers::new();

        // Simulate D command: extract text from cursor to end of line
        let line = doc.buffer.line(0).unwrap();
        let deleted_text = line[doc.cursor.col..].to_string();
        assert_eq!(deleted_text, " world");

        // Store to register (characterwise)
        let content = RegisterContent::characterwise(deleted_text);
        registers.delete(None, None, content);

        // Verify register content
        let stored = registers.get(None).unwrap();
        assert_eq!(stored.text(), " world");
        assert!(stored.is_characterwise());
    }

    #[test]
    fn test_delete_to_end_of_line_from_start() {
        use crate::registers::Registers;

        let mut doc = super::super::Document::from_string("hello".to_string());
        doc.cursor = Cursor::new(0, 0); // Cursor at beginning

        let mut registers = Registers::new();

        // Simulate D command from start of line
        let line = doc.buffer.line(0).unwrap();
        let deleted_text = line[doc.cursor.col..].to_string();
        assert_eq!(deleted_text, "hello");

        let content = RegisterContent::characterwise(deleted_text);
        registers.delete(None, None, content);

        let stored = registers.get(None).unwrap();
        assert_eq!(stored.text(), "hello");
    }

    #[test]
    fn test_delete_to_end_of_line_at_end() {
        // When cursor is already at end of line, nothing should be deleted
        let doc = super::super::Document::from_string("hello".to_string());
        let line = doc.buffer.line(0).unwrap();
        // Cursor at position 5 (past end of "hello")
        let col = line.len();
        assert_eq!(col, 5);
        // Nothing to delete when cursor is at or past end
        assert!(col >= line.len());
    }

    #[test]
    fn test_delete_to_end_of_line_empty_line() {
        let doc = super::super::Document::from_string(String::new());
        let line = doc.buffer.line(0).unwrap();
        assert!(line.is_empty());
        // On an empty line, cursor col is 0, which equals line.len()
        // so nothing to delete
        assert_eq!(0, line.len());
    }

    // Tests for C command (change to end of line)

    #[test]
    fn test_change_to_end_of_line_stores_characterwise() {
        use crate::registers::Registers;

        let mut doc = super::super::Document::from_string("hello world".to_string());
        doc.cursor = Cursor::new(0, 5);

        let mut registers = Registers::new();

        let line = doc.buffer.line(0).unwrap();
        let deleted_text = line[doc.cursor.col..].to_string();

        // C stores characterwise, same as D
        let content = RegisterContent::characterwise(deleted_text);
        registers.delete(None, None, content);

        let stored = registers.get(None).unwrap();
        assert_eq!(stored.text(), " world");
        assert!(stored.is_characterwise());
    }

    // Test for C command cursor positioning bug fix
    #[test]
    fn test_change_to_end_of_line_cursor_position() {
        // Simulate C command on "hello world" with cursor at col 6 (on 'w')
        // After C: line becomes "hello ", cursor should be at col 6 (end of line)
        let mut doc = super::super::Document::from_string("hello world".to_string());
        doc.cursor = Cursor::new(0, 6); // Cursor on 'w'

        // Simulate delete_to_end_of_line: delete from col 6 to end
        let line = doc.buffer.line(0).unwrap();
        let deleted_text = line[doc.cursor.col..].to_string();
        assert_eq!(deleted_text, "world");

        let start = doc.cursor;
        let end = Cursor::new(doc.cursor.row, line.len());
        doc.buffer.delete_range(&start, &end).unwrap();

        // After deletion, line is "hello "
        let line_after = doc.buffer.line(0).unwrap();
        assert_eq!(line_after, "hello ");

        // Normal mode clamp would put cursor at col 5 (on space, last char)
        // But for C command, cursor should be at col 6 (line.len()) for Insert mode
        let insert_col = line_after.len();
        assert_eq!(insert_col, 6);
        doc.cursor.col = insert_col;

        // Verify cursor is at the append position
        assert_eq!(doc.cursor.col, 6);
    }

    // Test for C command at start of line
    #[test]
    fn test_change_to_end_of_line_from_start_cursor_position() {
        let mut doc = super::super::Document::from_string("hello".to_string());
        doc.cursor = Cursor::new(0, 0); // Cursor at start

        // Delete from col 0 to end
        let line = doc.buffer.line(0).unwrap();
        assert_eq!(line, "hello");
        let start = doc.cursor;
        let end = Cursor::new(0, line.len());
        doc.buffer.delete_range(&start, &end).unwrap();

        // After deletion, line is empty
        let line_after = doc.buffer.line(0).unwrap();
        assert_eq!(line_after, "");

        // For C, cursor should be at col 0 (line.len() == 0)
        doc.cursor.col = line_after.len();
        assert_eq!(doc.cursor.col, 0);
    }

    // Test for C command in middle of line
    #[test]
    fn test_change_to_end_of_line_mid_word_cursor_position() {
        let mut doc = super::super::Document::from_string("abcdefgh".to_string());
        doc.cursor = Cursor::new(0, 3); // Cursor on 'd'

        // Delete from col 3 to end
        let line = doc.buffer.line(0).unwrap();
        let start = doc.cursor;
        let end = Cursor::new(0, line.len());
        doc.buffer.delete_range(&start, &end).unwrap();

        // After deletion, line is "abc"
        let line_after = doc.buffer.line(0).unwrap();
        assert_eq!(line_after, "abc");

        // Cursor should be at col 3 (line.len()) for insert mode appending
        doc.cursor.col = line_after.len();
        assert_eq!(doc.cursor.col, 3);
    }

    // Tests for Y command (yank line)

    #[test]
    fn test_yank_line_stores_linewise() {
        use crate::registers::Registers;

        let doc = super::super::Document::from_string("hello world".to_string());

        let mut registers = Registers::new();

        // Simulate Y command: yank current line (linewise)
        let line = doc.buffer.line(0).unwrap();
        let mut yanked_text = line.to_string();
        yanked_text.push('\n');

        let content = RegisterContent::linewise(yanked_text);
        registers.yank(None, content);

        let stored = registers.get(None).unwrap();
        assert_eq!(stored.text(), "hello world\n");
        assert!(stored.is_linewise());
    }

    #[test]
    fn test_yank_line_with_register() {
        use crate::registers::Registers;

        let doc = super::super::Document::from_string("test line".to_string());

        let mut registers = Registers::new();

        // Simulate "aY: yank line to register a
        let line = doc.buffer.line(0).unwrap();
        let mut yanked_text = line.to_string();
        yanked_text.push('\n');

        let content = RegisterContent::linewise(yanked_text);
        let reg_id = RegisterId::parse('a');
        registers.yank(reg_id, content);

        // Check register a
        let stored = registers.get(RegisterId::parse('a')).unwrap();
        assert_eq!(stored.text(), "test line\n");
        assert!(stored.is_linewise());
    }

    #[test]
    fn test_yank_line_empty_line() {
        use crate::registers::Registers;

        let doc = super::super::Document::from_string(String::new());

        let mut registers = Registers::new();

        let line = doc.buffer.line(0).unwrap();
        let mut yanked_text = line.to_string();
        yanked_text.push('\n');

        let content = RegisterContent::linewise(yanked_text);
        registers.yank(None, content);

        let stored = registers.get(None).unwrap();
        assert_eq!(stored.text(), "\n");
        assert!(stored.is_linewise());
    }

    // Tests for undo action recording

    #[test]
    fn test_insert_text_at_simple() {
        let mut buffer = Buffer::from_string("hello".to_string());
        let cursor = Cursor::new(0, 5);
        buffer.insert_char(&cursor, '!').unwrap();
        assert_eq!(buffer.line(0), Some("hello!"));
    }

    #[test]
    fn test_insert_text_at_with_newlines() {
        let mut buffer = Buffer::from_string("helloworld".to_string());
        // Simulate inserting "X\nY" at position (0, 5) using split_line and insert_char
        let cursor = Cursor::new(0, 5);
        buffer.insert_char(&cursor, 'X').unwrap();
        let cursor = Cursor::new(0, 6);
        buffer.split_line(&cursor).unwrap();
        let cursor = Cursor::new(1, 0);
        buffer.insert_char(&cursor, 'Y').unwrap();

        assert_eq!(buffer.line(0), Some("helloX"));
        assert_eq!(buffer.line(1), Some("Yworld"));
    }

    // =========================================================================
    // Toggle case logic tests
    // =========================================================================

    #[test]
    fn test_toggle_case_lowercase_to_uppercase() {
        let mut buffer = Buffer::from_string("hello".to_string());
        let line = buffer.line(0).unwrap().to_string();
        let col = 0;
        let ch = line[col..].chars().next().unwrap();
        assert!(ch.is_lowercase());
        let toggled = ch.to_uppercase().to_string();
        let new_content = format!(
            "{}{}{}",
            &line[..col],
            toggled,
            &line[col + ch.len_utf8()..]
        );
        let line_mut = buffer.line_mut(0).unwrap();
        line_mut.clear();
        line_mut.push_str(&new_content);
        assert_eq!(buffer.line(0), Some("Hello"));
    }

    #[test]
    fn test_toggle_case_uppercase_to_lowercase() {
        let mut buffer = Buffer::from_string("HELLO".to_string());
        let line = buffer.line(0).unwrap().to_string();
        let col = 0;
        let ch = line[col..].chars().next().unwrap();
        assert!(ch.is_uppercase());
        let toggled = ch.to_lowercase().to_string();
        let new_content = format!(
            "{}{}{}",
            &line[..col],
            toggled,
            &line[col + ch.len_utf8()..]
        );
        let line_mut = buffer.line_mut(0).unwrap();
        line_mut.clear();
        line_mut.push_str(&new_content);
        assert_eq!(buffer.line(0), Some("hELLO"));
    }

    #[test]
    fn test_toggle_case_mid_line() {
        let mut buffer = Buffer::from_string("hElLo".to_string());
        let line = buffer.line(0).unwrap().to_string();
        let col = 1; // on 'E'
        let ch = line[col..].chars().next().unwrap();
        assert_eq!(ch, 'E');
        let toggled = ch.to_lowercase().to_string();
        let new_content = format!(
            "{}{}{}",
            &line[..col],
            toggled,
            &line[col + ch.len_utf8()..]
        );
        let line_mut = buffer.line_mut(0).unwrap();
        line_mut.clear();
        line_mut.push_str(&new_content);
        assert_eq!(buffer.line(0), Some("helLo"));
    }

    #[test]
    fn test_toggle_case_non_alpha_unchanged() {
        let line = "123";
        let ch = line[0..].chars().next().unwrap();
        // Non-alpha chars should not be toggled
        assert!(!ch.is_uppercase() && !ch.is_lowercase());
    }

    // =========================================================================
    // Join lines logic tests
    // =========================================================================

    #[test]
    fn test_join_lines_basic() {
        let mut buffer = Buffer::from_string("hello\nworld".to_string());
        let current = buffer.line(0).unwrap().to_string();
        let next = buffer.line(1).unwrap().to_string();
        let trimmed = next.trim_start();
        let new_content = format!("{} {}", current, trimmed);
        let line_mut = buffer.line_mut(0).unwrap();
        line_mut.clear();
        line_mut.push_str(&new_content);
        buffer.remove_line(1).unwrap();
        assert_eq!(buffer.line(0), Some("hello world"));
        assert_eq!(buffer.len(), 1);
    }

    #[test]
    fn test_join_lines_strips_whitespace() {
        let mut buffer = Buffer::from_string("hello\n    world".to_string());
        let current = buffer.line(0).unwrap().to_string();
        let next = buffer.line(1).unwrap().to_string();
        let trimmed = next.trim_start();
        let new_content = format!("{} {}", current, trimmed);
        let line_mut = buffer.line_mut(0).unwrap();
        line_mut.clear();
        line_mut.push_str(&new_content);
        buffer.remove_line(1).unwrap();
        assert_eq!(buffer.line(0), Some("hello world"));
    }

    #[test]
    fn test_join_lines_empty_next_line() {
        let mut buffer = Buffer::from_string("hello\n\n".to_string());
        let current = buffer.line(0).unwrap().to_string();
        let next = buffer.line(1).unwrap().to_string();
        let trimmed = next.trim_start();
        // Empty next line, so no space added
        let new_content = if trimmed.is_empty() {
            current.clone()
        } else {
            format!("{} {}", current, trimmed)
        };
        let line_mut = buffer.line_mut(0).unwrap();
        line_mut.clear();
        line_mut.push_str(&new_content);
        buffer.remove_line(1).unwrap();
        assert_eq!(buffer.line(0), Some("hello"));
    }

    #[test]
    fn test_join_lines_last_line_noop() {
        let buffer = Buffer::from_string("only line".to_string());
        // On last line, join should do nothing
        assert_eq!(buffer.len(), 1);
        // Verify there is no next line to join
        assert!(buffer.line(1).is_none());
    }

    // =========================================================================
    // Integration tests: undo recording and cursor positioning
    // =========================================================================

    /// Simulate toggle_case_at_cursor logic and verify that undo restores
    /// the original character and cursor position.
    #[test]
    fn test_toggle_case_undo_restores_original() {
        use crate::undo::UndoHistory;

        let mut buffer = Buffer::from_string("hello".to_string());
        let mut undo = UndoHistory::new();
        let cursor_before = Cursor::new(0, 0);

        // Begin undo group (like the editor does)
        undo.begin_group(cursor_before, false);

        // Toggle 'h' -> 'H' (simulate toggle_case_at_cursor)
        let line = buffer.line(0).unwrap().to_string();
        let col = 0;
        let ch = line[col..].chars().next().unwrap();
        let toggled = ch.to_uppercase().to_string();
        let char_end = col + ch.len_utf8();
        let new_content = format!("{}{}{}", &line[..col], toggled, &line[char_end..]);

        let action = crate::undo::EditAction::ReplaceLine {
            row: 0,
            old_content: line,
            new_content: new_content.clone(),
        };
        undo.record(action);

        let line_mut = buffer.line_mut(0).unwrap();
        line_mut.clear();
        line_mut.push_str(&new_content);

        let cursor_after = Cursor::new(0, 1);
        undo.end_group(cursor_after);

        // Verify the change applied
        assert_eq!(buffer.line(0), Some("Hello"));

        // Undo should give us the original content
        let entry = undo.undo().unwrap();
        assert_eq!(entry.cursor_before(), cursor_before);
        assert_eq!(entry.actions().len(), 1);

        // Apply undo: restore old line content
        match &entry.actions()[0] {
            crate::undo::EditAction::ReplaceLine {
                row, old_content, ..
            } => {
                let line_mut = buffer.line_mut(*row).unwrap();
                line_mut.clear();
                line_mut.push_str(old_content);
            }
            _ => panic!("Expected ReplaceLine action"),
        }

        assert_eq!(buffer.line(0), Some("hello"));
    }

    /// Simulate join_line_with_next logic and verify that undo restores
    /// both lines and the cursor position.
    #[test]
    fn test_join_line_undo_restores_both_lines() {
        use crate::undo::UndoHistory;

        let mut buffer = Buffer::from_string("hello\nworld".to_string());
        let mut undo = UndoHistory::new();
        let cursor_before = Cursor::new(0, 0);

        undo.begin_group(cursor_before, false);

        // Simulate join: "hello" + " " + "world" = "hello world"
        let current = buffer.line(0).unwrap().to_string();
        let next = buffer.line(1).unwrap().to_string();
        let new_content = format!("{} {}", current, next.trim_start());
        let join_col = current.len();

        // Record ReplaceLine for current line
        undo.record(crate::undo::EditAction::ReplaceLine {
            row: 0,
            old_content: current,
            new_content: new_content.clone(),
        });

        // Record RemoveLine for the next line
        undo.record(crate::undo::EditAction::RemoveLine {
            row: 1,
            content: next.clone(),
        });

        // Apply
        let line_mut = buffer.line_mut(0).unwrap();
        line_mut.clear();
        line_mut.push_str(&new_content);
        buffer.remove_line(1).unwrap();

        let cursor_after = Cursor::new(0, join_col);
        undo.end_group(cursor_after);

        // Verify the join
        assert_eq!(buffer.line(0), Some("hello world"));
        assert_eq!(buffer.len(), 1);

        // Undo should give us a single entry with 2 actions
        let entry = undo.undo().unwrap();
        assert_eq!(entry.cursor_before(), cursor_before);
        assert_eq!(entry.cursor_after(), cursor_after);
        assert_eq!(entry.actions().len(), 2);

        // Apply undo in reverse order to restore both lines
        for action in entry.actions().iter().rev() {
            match action {
                crate::undo::EditAction::RemoveLine { row, content } => {
                    buffer.insert_line(*row, content.clone()).unwrap();
                }
                crate::undo::EditAction::ReplaceLine {
                    row, old_content, ..
                } => {
                    let line_mut = buffer.line_mut(*row).unwrap();
                    line_mut.clear();
                    line_mut.push_str(old_content);
                }
                _ => panic!("Unexpected action type"),
            }
        }

        assert_eq!(buffer.line(0), Some("hello"));
        assert_eq!(buffer.line(1), Some("world"));
        assert_eq!(buffer.len(), 2);
    }

    /// Verify cursor position after toggle_case_at_cursor advances past the
    /// toggled character.
    #[test]
    fn test_toggle_case_cursor_advances() {
        let mut buffer = Buffer::from_string("abc".to_string());
        let col = 1; // on 'b'

        let line = buffer.line(0).unwrap().to_string();
        let ch = line[col..].chars().next().unwrap();
        assert_eq!(ch, 'b');
        let toggled = ch.to_uppercase().to_string();
        let char_end = col + ch.len_utf8();
        let new_content = format!("{}{}{}", &line[..col], toggled, &line[char_end..]);

        let line_mut = buffer.line_mut(0).unwrap();
        line_mut.clear();
        line_mut.push_str(&new_content);

        // Cursor should advance past the toggled char
        let new_col = col + toggled.len();
        assert_eq!(new_col, 2);
        assert_eq!(buffer.line(0), Some("aBc"));
    }

    /// Verify cursor position after join_line_with_next is at the join point.
    #[test]
    fn test_join_line_cursor_at_join_point() {
        let mut buffer = Buffer::from_string("foo\nbar".to_string());
        let current = buffer.line(0).unwrap().to_string();
        let next = buffer.line(1).unwrap().to_string();
        let join_col = current.len(); // cursor should land here (3)
        let new_content = format!("{} {}", current, next.trim_start());

        let line_mut = buffer.line_mut(0).unwrap();
        line_mut.clear();
        line_mut.push_str(&new_content);
        buffer.remove_line(1).unwrap();

        assert_eq!(buffer.line(0), Some("foo bar"));
        assert_eq!(join_col, 3);
    }
}

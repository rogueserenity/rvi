//! Shared editing operations: delete, yank, put, case, jump helpers.

use super::super::Editor;
use super::{apply_case_to_line, keys_to_string, string_to_keys};
use crate::buffer::unicode::next_grapheme_boundary;
use crate::buffer::unicode::prev_grapheme_boundary;
use crate::buffer::Cursor;
use crate::error::Error;
use crate::mode::Operator;
use crate::registers::{ContentType, RegisterContent, RegisterId};
use crate::undo::EditAction;

impl Editor {
    /// Push the current cursor position onto the jump list.
    ///
    /// Called before any "large" motion (G, gg, nG, /, ?, n, N, *, #, %, marks,
    /// :n). Resets the jump list index to 0 (current position) so Ctrl-O can
    /// navigate backward from the new entry.
    pub(in crate::editor) fn push_jump(&mut self) {
        const MAX_JUMP_LIST: usize = 100;
        let pos = self.document.cursor;
        // Don't add a duplicate of the most recent entry
        if self.state.navigation.jump_list.back() == Some(&pos) {
            return;
        }
        self.state.navigation.jump_list.push_back(pos);
        if self.state.navigation.jump_list.len() > MAX_JUMP_LIST {
            self.state.navigation.jump_list.pop_front();
        }
        // Reset index: we're now at the "newest" position; clear any saved origin.
        self.state.navigation.jump_list_idx = 0;
        self.state.navigation.jump_origin = None;
    }

    /// Jump to an older position (Ctrl-O).
    pub(super) fn execute_jump_back(&mut self) {
        let len = self.state.navigation.jump_list.len();
        if len == 0 {
            return;
        }
        // Save current cursor as origin on first Ctrl-O so Ctrl-I can restore it.
        if self.state.navigation.jump_list_idx == 0 {
            self.state.navigation.jump_origin = Some(self.document.cursor);
        }
        // idx 0 means "just before the newest entry",
        // idx len means "at the oldest entry"
        let new_idx = self.state.navigation.jump_list_idx + 1;
        if new_idx > len {
            self.state.status_message = Some(super::super::StatusMessage::Error(
                "Already at oldest position".to_string(),
            ));
            return;
        }
        self.state.navigation.jump_list_idx = new_idx;
        // Index from the back: idx=1 → last element, idx=2 → second-to-last, etc.
        if let Some(&dest) = self
            .state
            .navigation
            .jump_list
            .iter()
            .rev()
            .nth(new_idx - 1)
        {
            let row = dest.row.min(self.document.buffer.len().saturating_sub(1));
            let col = if let Some(line) = self.document.buffer.line(row) {
                dest.col.min(line.len().saturating_sub(1).max(0))
            } else {
                0
            };
            self.document.cursor = Cursor::new(row, col);
            self.state
                .viewport
                .ensure_visible(self.document.cursor.row, self.document.buffer.len());
        }
    }

    /// Jump to a newer position (Ctrl-I / Tab).
    pub(super) fn execute_jump_forward(&mut self) {
        if self.state.navigation.jump_list_idx == 0 {
            self.state.status_message = Some(super::super::StatusMessage::Error(
                "Already at newest position".to_string(),
            ));
            return;
        }
        self.state.navigation.jump_list_idx -= 1;
        let idx = self.state.navigation.jump_list_idx;
        if idx == 0 {
            // Back to where we were before the first Ctrl-O: restore saved origin.
            if let Some(origin) = self.state.navigation.jump_origin.take() {
                let row = origin.row.min(self.document.buffer.len().saturating_sub(1));
                let col = if let Some(line) = self.document.buffer.line(row) {
                    origin.col.min(line.len().saturating_sub(1).max(0))
                } else {
                    0
                };
                self.document.cursor = Cursor::new(row, col);
                self.state
                    .viewport
                    .ensure_visible(self.document.cursor.row, self.document.buffer.len());
            }
            return;
        }
        if let Some(&dest) = self.state.navigation.jump_list.iter().rev().nth(idx - 1) {
            let row = dest.row.min(self.document.buffer.len().saturating_sub(1));
            let col = if let Some(line) = self.document.buffer.line(row) {
                dest.col.min(line.len().saturating_sub(1).max(0))
            } else {
                0
            };
            self.document.cursor = Cursor::new(row, col);
            self.state
                .viewport
                .ensure_visible(self.document.cursor.row, self.document.buffer.len());
        }
    }

    /// Show file information in the status line (Ctrl-g).
    ///
    /// Displays: filename, modified status, total lines, percentage, current line.
    pub(super) fn show_file_info(&mut self) {
        let filename = self
            .document
            .filename
            .as_deref()
            .unwrap_or("[No Name]")
            .to_string();
        let modified = if self.document.modified {
            " [Modified]"
        } else {
            ""
        };
        let total = self.document.buffer.len();
        let row = self.document.cursor.row + 1; // 1-indexed
        let pct = if total == 0 {
            "All".to_string()
        } else if row <= 1 {
            "Top".to_string()
        } else if row >= total {
            "Bot".to_string()
        } else {
            format!("{}%", row * 100 / total)
        };
        let msg = format!(
            "\"{}\"{}  {} lines  --{}--  line {}",
            filename, modified, total, pct, row
        );
        self.state.status_message = Some(super::super::StatusMessage::Info(msg));
    }

    /// Undo all changes to the current line (U command).
    ///
    /// Restores the current line to its state when the cursor last entered it.
    /// The restoration itself is recorded as an undoable action.
    pub(super) fn execute_undo_line(&mut self) -> Result<(), Error> {
        let row = self.document.cursor.row;

        // Get the snapshot (line content when cursor arrived at this row)
        let snapshot = match &self.state.navigation.line_snapshot {
            Some((snap_row, snap_content)) if *snap_row == row => snap_content.clone(),
            _ => {
                // No snapshot for this line — nothing to do
                return Ok(());
            }
        };

        // Get current line content
        let current = self.document.buffer.line(row).unwrap_or("").to_string();

        if current == snapshot {
            // Already at the snapshot state — nothing to do
            return Ok(());
        }

        // Record the line replacement as a single undoable action.
        self.state
            .undo_history
            .begin_group(self.document.cursor, self.document.modified);

        self.state.undo_history.record(EditAction::ReplaceLine {
            row,
            old_content: current,
            new_content: snapshot.clone(),
        });

        // Apply the replacement to the buffer.
        if let Some(line) = self.document.buffer.line_mut(row) {
            line.clear();
            line.push_str(&snapshot);
        }

        self.document.cursor.col = 0;
        self.clamp_cursor_to_line();
        self.document.modified = true;

        self.state.undo_history.end_group(self.document.cursor);
        Ok(())
    }

    /// Update the line snapshot when the cursor moves to a different row.
    ///
    /// Called after any motion that might change the cursor's row. Takes a
    /// snapshot of the new line so `U` can restore it later.
    pub(super) fn update_line_snapshot(&mut self) {
        let row = self.document.cursor.row;
        let needs_update = self
            .state
            .navigation
            .line_snapshot
            .as_ref()
            .is_none_or(|(snap_row, _)| *snap_row != row);

        if needs_update {
            let content = self.document.buffer.line(row).unwrap_or("").to_string();
            self.state.navigation.line_snapshot = Some((row, content));
        }
    }

    /// Stop macro recording and store the buffer in the named register.
    pub(super) fn stop_macro_recording(&mut self) {
        if let Some(ch) = self.state.macro_state.recording.take() {
            let keys = std::mem::take(&mut self.state.macro_state.buffer);
            let text = keys_to_string(&keys);
            let content = RegisterContent::new(text, ContentType::Characterwise);
            if let Some(id) = RegisterId::parse(ch) {
                self.state.registers.yank(Some(id), content);
            }
            // Clear the "recording @x" status message
            self.state.status_message = None;
        }
    }

    /// Play back a macro from the named register.
    ///
    /// Reads the register content and feeds each stored key through the
    /// normal key handler. Prevents re-entrant playback: won't run during
    /// recording, and caps nesting depth at 100 to prevent stack overflow
    /// from self-calling macros (e.g. `@a` inside register `a`).
    pub(super) fn play_macro(&mut self, ch: char) -> Result<(), Error> {
        const MAX_MACRO_DEPTH: usize = 100;

        // Don't play while recording
        if self.state.macro_state.recording.is_some() {
            return Ok(());
        }

        // Recursion guard
        if self.state.macro_state.playback_depth >= MAX_MACRO_DEPTH {
            self.state.status_message = Some(super::super::StatusMessage::Error(
                "E169: Macro recursion depth exceeded".to_string(),
            ));
            return Ok(());
        }

        let text = {
            let id = RegisterId::parse(ch);
            match self.state.registers.get(id) {
                Some(c) => c.text().to_string(),
                None => return Ok(()),
            }
        };

        self.state.macro_state.playback_depth += 1;
        let keys = string_to_keys(&text);
        for key in keys {
            if self.state.should_quit {
                break;
            }
            self.handle_key(key)?;
        }
        self.state.macro_state.playback_depth -= 1;
        Ok(())
    }

    /// Jump to a named mark or the previous position.
    ///
    /// For `''` / ` `` ` (mark == `` '`' ``), swaps the current position with
    /// the stored previous-position mark `'`'` so subsequent jumps toggle
    /// back and forth.
    pub(super) fn execute_jump_to_mark(&mut self, mark: char, line_start: bool) {
        let target = if mark == '`' {
            // Jump to previous position ('' or ``)
            self.state.navigation.marks.get(&'`').copied()
        } else {
            self.state.navigation.marks.get(&mark).copied()
        };

        match target {
            Some(dest) => {
                // Save current position as previous-position mark before jumping
                let prev = self.document.cursor;
                self.state.navigation.marks.insert('`', prev);

                let row = dest.row.min(self.document.buffer.len().saturating_sub(1));
                if line_start {
                    // Jump to first non-blank of the mark's line
                    self.document.cursor.row = row;
                    self.document.cursor.col = 0;
                    self.move_to_first_non_blank();
                } else {
                    // Jump to exact position, clamping col to line length
                    let col = if let Some(line) = self.document.buffer.line(row) {
                        dest.col.min(line.len().saturating_sub(1).max(0))
                    } else {
                        0
                    };
                    self.document.cursor = Cursor::new(row, col);
                }
                self.state
                    .viewport
                    .ensure_visible(self.document.cursor.row, self.document.buffer.len());
            }
            None => {
                self.state.status_message = Some(super::super::StatusMessage::Error(
                    "E20: Mark not set".to_string(),
                ));
            }
        }
    }

    /// Compute the exclusive end row for a line operation starting at `start_row`
    /// spanning `count` lines, clamped to the buffer length.
    pub(super) fn clamped_end_row(&self, start_row: usize, count: usize) -> usize {
        (start_row + count).min(self.document.buffer.len())
    }

    /// Delete lines from the current line and store to register.
    pub(super) fn delete_lines_with_register(
        &mut self,
        count: usize,
        register: Option<char>,
    ) -> Result<(), Error> {
        let start_row = self.document.cursor.row;
        let end_row = self.clamped_end_row(start_row, count);

        // Collect the lines to be deleted and record undo actions
        let mut deleted_text = String::new();
        for row in start_row..end_row {
            if let Some(line) = self.document.buffer.line(row) {
                deleted_text.push_str(line);
                deleted_text.push('\n');
            }
        }

        // Store to register (linewise delete)
        let content = RegisterContent::linewise(deleted_text);
        let reg_id = register.and_then(RegisterId::parse);
        self.state.registers.delete(reg_id, register, content);

        // Remove lines from end to start, recording undo actions after each
        // successful removal so the undo history stays consistent with buffer state.
        for row in (start_row..end_row).rev() {
            let content = self.document.buffer.line(row).map(|l| l.to_string());
            self.document.buffer.remove_line(row)?;
            if let Some(content) = content {
                self.state
                    .undo_history
                    .record(EditAction::RemoveLine { row, content });
            }
        }

        // Adjust cursor row if needed (buffer always has at least one line after remove_line)
        if self.document.cursor.row >= self.document.buffer.len() {
            self.document.cursor.row = self.document.buffer.len().saturating_sub(1);
        }

        // Move cursor to first non-blank of the new current line
        self.move_to_first_non_blank();

        self.document.modified = true;
        self.report_lines(end_row - start_row, "lines deleted");
        Ok(())
    }

    /// Implement `cc`: yank count lines to register, clear their content (leaving
    /// empty lines), apply autoindent on the first line, and position the cursor.
    pub(super) fn change_lines(
        &mut self,
        count: usize,
        register: Option<char>,
    ) -> Result<(), Error> {
        let start_row = self.document.cursor.row;
        let end_row = self.clamped_end_row(start_row, count);

        // Collect lines to yank
        let mut yanked_text = String::new();
        for row in start_row..end_row {
            if let Some(line) = self.document.buffer.line(row) {
                yanked_text.push_str(line);
                yanked_text.push('\n');
            }
        }

        // Store to register as linewise
        let content = RegisterContent::linewise(yanked_text);
        let reg_id = register.and_then(RegisterId::parse);
        self.state.registers.delete(reg_id, register, content);

        // Compute autoindent prefix for the first line
        let indent = if self.state.settings.autoindent {
            self.document
                .buffer
                .line(start_row)
                .map(|l| super::super::editing::get_leading_whitespace(l).to_string())
                .unwrap_or_default()
        } else {
            String::new()
        };

        // Determine the new content for the first (retained) line
        let first_new = indent.clone();

        // Replace each line: first line gets autoindent, rest become empty then are removed
        for row in (start_row..end_row).rev() {
            if let Some(line) = self.document.buffer.line(row) {
                let old = line.to_string();
                let new_content = if row == start_row {
                    first_new.clone()
                } else {
                    String::new()
                };
                if old != new_content {
                    let action = EditAction::ReplaceLine {
                        row,
                        old_content: old,
                        new_content: new_content.clone(),
                    };
                    self.state.undo_history.record(action);
                    if let Some(l) = self.document.buffer.line_mut(row) {
                        *l = new_content;
                    }
                }
            }
        }

        // Delete extra lines (rows start_row+1 .. end_row) that are now empty
        for row in (start_row + 1..end_row).rev() {
            let action = EditAction::RemoveLine {
                row,
                content: String::new(),
            };
            self.state.undo_history.record(action);
            self.document.buffer.remove_line(row)?;
        }

        // Position cursor at end of indent (or col 0)
        self.document.cursor.row = start_row;
        self.document.cursor.col = indent.len();
        self.clamp_cursor_to_line();
        self.document.modified = true;
        Ok(())
    }

    /// Yank lines without deleting and store to register.
    pub(super) fn yank_lines(&mut self, count: usize, register: Option<char>) -> Result<(), Error> {
        let start_row = self.document.cursor.row;
        let end_row = self.clamped_end_row(start_row, count);

        // Collect the lines to be yanked
        let mut yanked_text = String::new();
        for row in start_row..end_row {
            if let Some(line) = self.document.buffer.line(row) {
                yanked_text.push_str(line);
                yanked_text.push('\n');
            }
        }

        // Store to register (linewise yank)
        let content = RegisterContent::linewise(yanked_text);
        let reg_id = register.and_then(RegisterId::parse);
        self.state.registers.yank(reg_id, content);
        self.report_lines(end_row - start_row, "lines yanked");

        Ok(())
    }

    /// Delete from cursor to end of the current line and store to register.
    ///
    /// This implements the D command (equivalent to d$). The deleted text is
    /// stored as characterwise content since it does not span full lines.
    pub(super) fn delete_to_end_of_line(&mut self, register: Option<char>) -> Result<(), Error> {
        if let Some(line) = self.document.buffer.line(self.document.cursor.row) {
            if self.document.cursor.col >= line.len() {
                // Cursor at or past end of line - nothing to delete
                return Ok(());
            }

            // Extract the text that will be deleted (from cursor to end of line)
            let deleted_text = line[self.document.cursor.col..].to_string();

            // Record undo action
            let action = EditAction::DeleteRange {
                start_row: self.document.cursor.row,
                start_col: self.document.cursor.col,
                end_row: self.document.cursor.row,
                end_col: line.len(),
                text: deleted_text.clone(),
            };
            self.state.undo_history.record(action);

            // Store to register (characterwise, like d$)
            let content = RegisterContent::characterwise(deleted_text);
            let reg_id = register.and_then(RegisterId::parse);
            self.state.registers.delete(reg_id, register, content);

            // Delete from cursor position to end of line
            let start = self.document.cursor;
            let end = Cursor::new(self.document.cursor.row, line.len());
            self.document.buffer.delete_range(&start, &end)?;

            // Clamp cursor to valid position on the (now shorter) line
            self.clamp_cursor_to_line();

            self.document.modified = true;
        }

        Ok(())
    }

    /// Delete the range from start to end cursor positions and store to register.
    pub(super) fn delete_motion_range_with_register(
        &mut self,
        start: &Cursor,
        end: &Cursor,
        register: Option<char>,
        content_type: ContentType,
    ) -> Result<(), Error> {
        // Extract text before deleting
        let deleted_text = self.get_text_in_range(start, end);

        // Record undo action
        let action = EditAction::DeleteRange {
            start_row: start.row,
            start_col: start.col,
            end_row: end.row,
            end_col: end.col,
            text: deleted_text.clone(),
        };
        self.state.undo_history.record(action);

        // Store to register
        let content = RegisterContent::new(deleted_text, content_type);
        let reg_id = register.and_then(RegisterId::parse);
        self.state.registers.delete(reg_id, register, content);

        // Perform the delete
        self.document.buffer.delete_range(start, end)?;
        self.document.cursor = *start;
        self.clamp_cursor_to_line();
        self.document.modified = true;
        Ok(())
    }

    /// Yank the range from start to end cursor positions without deleting.
    pub(super) fn yank_range(
        &mut self,
        start: &Cursor,
        end: &Cursor,
        register: Option<char>,
        content_type: ContentType,
    ) -> Result<(), Error> {
        // Extract text
        let yanked_text = self.get_text_in_range(start, end);

        // Store to register
        let content = RegisterContent::new(yanked_text, content_type);
        let reg_id = register.and_then(RegisterId::parse);
        self.state.registers.yank(reg_id, content);

        Ok(())
    }

    /// Get text in a range between two cursor positions.
    pub(super) fn get_text_in_range(&self, start: &Cursor, end: &Cursor) -> String {
        let mut text = String::new();

        if start.row == end.row {
            // Same line - extract substring
            if let Some(line) = self.document.buffer.line(start.row) {
                let start_col = start.col.min(line.len());
                let end_col = end.col.min(line.len());
                if start_col < end_col {
                    text.push_str(&line[start_col..end_col]);
                }
            }
        } else {
            // Multiple lines
            // First line (from start.col to end of line)
            if let Some(first_line) = self.document.buffer.line(start.row) {
                let start_col = start.col.min(first_line.len());
                text.push_str(&first_line[start_col..]);
                text.push('\n');
            }

            // Middle lines (full lines)
            for row in (start.row + 1)..end.row {
                if let Some(line) = self.document.buffer.line(row) {
                    text.push_str(line);
                    text.push('\n');
                }
            }

            // Last line (from start to end.col)
            if let Some(last_line) = self.document.buffer.line(end.row) {
                let end_col = end.col.min(last_line.len());
                text.push_str(&last_line[..end_col]);
            }
        }

        text
    }

    /// Delete the character at the cursor position and store to register.
    pub(super) fn delete_char_at_cursor_with_register(
        &mut self,
        register: Option<char>,
    ) -> Result<(), Error> {
        if let Some(line) = self.document.buffer.line(self.document.cursor.row) {
            if self.document.cursor.col < line.len() {
                // Get the character being deleted
                let next_col = next_grapheme_boundary(line, self.document.cursor.col);
                let deleted_char = &line[self.document.cursor.col..next_col];

                // Record undo action for each character in the grapheme
                for ch in deleted_char.chars() {
                    let action = EditAction::DeleteChar {
                        row: self.document.cursor.row,
                        col: self.document.cursor.col,
                        ch,
                    };
                    self.state.undo_history.record(action);
                }

                // Store to register (characterwise, small delete)
                let content = RegisterContent::characterwise(deleted_char.to_string());
                let reg_id = register.and_then(RegisterId::parse);
                self.state.registers.delete(reg_id, register, content);

                // Delete the character
                let start = self.document.cursor;
                let end = Cursor::new(self.document.cursor.row, next_col);
                self.document.buffer.delete_range(&start, &end)?;
                self.clamp_cursor_to_line();
                self.document.modified = true;
            }
        }
        Ok(())
    }

    /// Put content after the cursor.
    pub(super) fn put_after(&mut self, register: Option<char>) -> Result<(), Error> {
        let reg_id = register.and_then(RegisterId::parse);
        let content = self.state.registers.get_owned(reg_id);

        if let Some(content) = content {
            if content.is_block() {
                // Block-wise: insert each segment at the block column on successive lines.
                let start_col = self.document.cursor.col;
                let start_row = self.document.cursor.row;
                let segments: Vec<&str> = content.text().split('\n').collect();
                let seg_count = if segments.last().map(|s| s.is_empty()).unwrap_or(false) {
                    segments.len().saturating_sub(1)
                } else {
                    segments.len()
                };
                for (i, seg) in segments.iter().take(seg_count).enumerate() {
                    let row = start_row + i;
                    // Extend the buffer with blank lines if needed.
                    while self.document.buffer.len() <= row {
                        let new_row = self.document.buffer.len();
                        self.document.buffer.insert_line(new_row, String::new())?;
                    }
                    // Insert after cursor column (p inserts after).
                    let insert_col = if let Some(line) = self.document.buffer.line(row) {
                        next_grapheme_boundary(line, start_col.min(line.len()))
                    } else {
                        start_col
                    };
                    let cursor = Cursor::new(row, insert_col);
                    for ch in seg.chars() {
                        self.document.buffer.insert_char(&cursor, ch)?;
                    }
                }
                // Leave cursor on start of pasted block.
                self.document.cursor.row = start_row;
                self.document.cursor.col = start_col;
                self.document.modified = true;
                return Ok(());
            } else if content.is_linewise() {
                // Linewise: insert on new line below
                let insert_row = self.document.cursor.row + 1;
                let text = content.text();

                // Split the text into lines and insert each
                for (i, line) in text.lines().enumerate() {
                    let action = EditAction::InsertLine {
                        row: insert_row + i,
                        content: line.to_string(),
                    };
                    self.state.undo_history.record(action);
                    self.document
                        .buffer
                        .insert_line(insert_row + i, line.to_string())?;
                }

                // Move cursor to first non-blank of first inserted line
                self.document.cursor.row = insert_row;
                self.document.cursor.col = 0;
                self.move_to_first_non_blank();
            } else {
                // Characterwise: insert after cursor
                // Move cursor one position to the right first (if possible)
                if let Some(line) = self.document.buffer.line(self.document.cursor.row) {
                    if self.document.cursor.col < line.len() {
                        self.document.cursor.col =
                            next_grapheme_boundary(line, self.document.cursor.col);
                    }
                }

                // Record the insertion as a single InsertText action
                let insert_row = self.document.cursor.row;
                let insert_col = self.document.cursor.col;
                let text = content.text();

                let action = EditAction::InsertText {
                    row: insert_row,
                    col: insert_col,
                    text: text.to_string(),
                };
                self.state.undo_history.record(action);

                // Track the starting position for cursor placement
                let start_row = self.document.cursor.row;

                // Insert the text
                for ch in text.chars() {
                    if ch == '\n' {
                        self.document.buffer.split_line(&self.document.cursor)?;
                        self.document.cursor.row += 1;
                        self.document.cursor.col = 0;
                    } else {
                        self.document
                            .buffer
                            .insert_char(&self.document.cursor, ch)?;
                        self.document.cursor.col += ch.len_utf8();
                    }
                }

                // Move cursor back one grapheme (vi behavior: cursor lands on last pasted char)
                if self.document.cursor.col > 0 {
                    if let Some(line) = self.document.buffer.line(self.document.cursor.row) {
                        self.document.cursor.col =
                            prev_grapheme_boundary(line, self.document.cursor.col);
                    }
                } else if self.document.cursor.row > start_row {
                    self.document.cursor.row -= 1;
                    if let Some(line) = self.document.buffer.line(self.document.cursor.row) {
                        self.document.cursor.col = prev_grapheme_boundary(line, line.len());
                    }
                }
            }
            self.document.modified = true;
        }

        Ok(())
    }

    /// Put content before the cursor.
    pub(super) fn put_before(&mut self, register: Option<char>) -> Result<(), Error> {
        let reg_id = register.and_then(RegisterId::parse);
        let content = self.state.registers.get_owned(reg_id);

        if let Some(content) = content {
            if content.is_block() {
                // Block-wise: insert each segment at the block column on successive lines.
                let start_col = self.document.cursor.col;
                let start_row = self.document.cursor.row;
                let segments: Vec<&str> = content.text().split('\n').collect();
                let seg_count = if segments.last().map(|s| s.is_empty()).unwrap_or(false) {
                    segments.len().saturating_sub(1)
                } else {
                    segments.len()
                };
                for (i, seg) in segments.iter().take(seg_count).enumerate() {
                    let row = start_row + i;
                    while self.document.buffer.len() <= row {
                        let new_row = self.document.buffer.len();
                        self.document.buffer.insert_line(new_row, String::new())?;
                    }
                    let insert_col =
                        start_col.min(self.document.buffer.line(row).map(|l| l.len()).unwrap_or(0));
                    let cursor = Cursor::new(row, insert_col);
                    for ch in seg.chars() {
                        self.document.buffer.insert_char(&cursor, ch)?;
                    }
                }
                self.document.cursor.row = start_row;
                self.document.cursor.col = start_col;
                self.document.modified = true;
                return Ok(());
            } else if content.is_linewise() {
                // Linewise: insert on new line above
                let insert_row = self.document.cursor.row;
                let text = content.text();

                // Split the text into lines and insert each
                for (i, line) in text.lines().enumerate() {
                    let action = EditAction::InsertLine {
                        row: insert_row + i,
                        content: line.to_string(),
                    };
                    self.state.undo_history.record(action);
                    self.document
                        .buffer
                        .insert_line(insert_row + i, line.to_string())?;
                }

                // Move cursor to first non-blank of first inserted line
                self.document.cursor.row = insert_row;
                self.document.cursor.col = 0;
                self.move_to_first_non_blank();
            } else {
                // Characterwise: insert before cursor
                let insert_row = self.document.cursor.row;
                let insert_col = self.document.cursor.col;
                let text = content.text();

                // Record the insertion as a single InsertText action
                let action = EditAction::InsertText {
                    row: insert_row,
                    col: insert_col,
                    text: text.to_string(),
                };
                self.state.undo_history.record(action);

                // Track the starting position for cursor placement
                let start_row = self.document.cursor.row;

                for ch in text.chars() {
                    if ch == '\n' {
                        self.document.buffer.split_line(&self.document.cursor)?;
                        self.document.cursor.row += 1;
                        self.document.cursor.col = 0;
                    } else {
                        self.document
                            .buffer
                            .insert_char(&self.document.cursor, ch)?;
                        self.document.cursor.col += ch.len_utf8();
                    }
                }

                // Move cursor back one grapheme (vi behavior: cursor lands on last pasted char)
                if self.document.cursor.col > 0 {
                    if let Some(line) = self.document.buffer.line(self.document.cursor.row) {
                        self.document.cursor.col =
                            prev_grapheme_boundary(line, self.document.cursor.col);
                    }
                } else if self.document.cursor.row > start_row {
                    self.document.cursor.row -= 1;
                    if let Some(line) = self.document.buffer.line(self.document.cursor.row) {
                        self.document.cursor.col = prev_grapheme_boundary(line, line.len());
                    }
                }
            }
            self.document.modified = true;
        }

        Ok(())
    }

    /// Yank a specific range of lines (by row index, exclusive end).
    pub(super) fn yank_lines_range(
        &mut self,
        start_row: usize,
        end_row: usize,
        register: Option<char>,
    ) -> Result<(), Error> {
        let mut yanked = String::new();
        let buf_len = self.document.buffer.len();
        let end = end_row.min(buf_len);
        for row in start_row..end {
            if let Some(line) = self.document.buffer.line(row) {
                yanked.push_str(line);
                yanked.push('\n');
            }
        }
        let content = RegisterContent::linewise(yanked);
        let reg_id = register.and_then(RegisterId::parse);
        self.state.registers.yank(reg_id, content);
        Ok(())
    }

    /// Apply a case operator (Uppercase/Lowercase/ToggleCase) to full lines.
    pub(super) fn apply_case_operator_lines(
        &mut self,
        row_start: usize,
        row_end: usize,
        operator: Operator,
    ) -> Result<(), Error> {
        let row_end = row_end.min(self.document.buffer.len().saturating_sub(1));
        for row in row_start..=row_end {
            if let Some(line) = self.document.buffer.line(row) {
                let old_content = line.to_string();
                let new_content = apply_case_to_line(&old_content, 0, old_content.len(), operator);
                self.replace_line_recorded(row, new_content)?;
            }
        }
        Ok(())
    }

    /// Apply a case operator (Uppercase/Lowercase/ToggleCase) to a byte range.
    ///
    /// The range may span multiple lines. `start` and `end` are exclusive-end
    /// positions (consistent with how motion ranges are stored).
    pub(super) fn apply_case_operator_range(
        &mut self,
        start: &Cursor,
        end: &Cursor,
        operator: Operator,
    ) -> Result<(), Error> {
        // Determine which cursor is the top (earlier) and which is the bottom (later).
        let (top, bot) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        let row_start = top.row;
        let row_end = bot.row;
        for row in row_start..=row_end.min(self.document.buffer.len().saturating_sub(1)) {
            if let Some(line) = self.document.buffer.line(row) {
                let old_content = line.to_string();
                let col_start = if row == row_start { top.col } else { 0 };
                let col_end = if row == row_end {
                    bot.col.min(old_content.len())
                } else {
                    old_content.len()
                };
                let new_content = apply_case_to_line(&old_content, col_start, col_end, operator);
                self.replace_line_recorded(row, new_content)?;
            }
        }
        Ok(())
    }

    /// Extract the identifier word (chars matching `[A-Za-z0-9_]`) under the cursor.
    ///
    /// Walks backward to the start of the word, then forward to its end.
    /// Returns an empty string if the cursor is on whitespace or a non-identifier char.
    pub(super) fn word_under_cursor(&self) -> String {
        let row = self.document.cursor.row;
        let col = self.document.cursor.col;
        let line = match self.document.buffer.line(row) {
            Some(l) => l,
            None => return String::new(),
        };

        let is_ident = |c: char| c.is_ascii_alphanumeric() || c == '_';

        // Cursor must be on an identifier character
        let ch = match line[col..].chars().next() {
            Some(c) => c,
            None => return String::new(),
        };
        if !is_ident(ch) {
            return String::new();
        }

        // Walk backward to start of word
        let start = line[..col]
            .char_indices()
            .rev()
            .take_while(|(_, c)| is_ident(*c))
            .last()
            .map(|(i, _)| i)
            .unwrap_or(col);

        // Walk forward to end of word
        let end = col
            + line[col..]
                .char_indices()
                .take_while(|(_, c): &(usize, char)| is_ident(*c))
                .last()
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(0);

        line[start..end].to_string()
    }

    /// Reflow (reformat) lines in the given row range to fit within `width` columns.
    ///
    /// Collects the leading whitespace from the first line as the indent prefix,
    /// joins all words across the range, then re-wraps them so each line is at
    /// most `width` columns wide. If `width` is 0 the operation is a no-op.
    pub(super) fn reflow_lines(
        &mut self,
        row_start: usize,
        row_end: usize,
        width: usize,
    ) -> Result<(), Error> {
        if width == 0 {
            return Ok(());
        }
        let row_end = row_end.min(self.document.buffer.len().saturating_sub(1));
        if row_start > row_end {
            return Ok(());
        }

        // Detect leading indent from the first line.
        let indent: String = self
            .document
            .buffer
            .line(row_start)
            .unwrap_or("")
            .chars()
            .take_while(|c| c.is_ascii_whitespace())
            .collect();

        // Collect all words from the range.
        let mut words: Vec<String> = Vec::new();
        for row in row_start..=row_end {
            if let Some(line) = self.document.buffer.line(row) {
                for word in line.split_ascii_whitespace() {
                    words.push(word.to_string());
                }
            }
        }

        // Wrap words into lines of at most `width` columns.
        let mut new_lines: Vec<String> = Vec::new();
        let mut current = indent.clone();
        for word in &words {
            let current_is_empty = current == indent;
            if current_is_empty {
                // First word on a new line.
                current.push_str(word);
            } else if current.len() + 1 + word.len() <= width {
                current.push(' ');
                current.push_str(word);
            } else {
                new_lines.push(current);
                current = format!("{}{}", indent, word);
            }
        }
        new_lines.push(current);

        let old_count = row_end - row_start + 1;
        let new_count = new_lines.len();

        // Replace lines that exist in both old and new ranges.
        let replace_count = old_count.min(new_count);
        for (i, line) in new_lines.iter().enumerate().take(replace_count) {
            self.replace_line_recorded(row_start + i, line.clone())?;
        }

        // Remove extra old lines (reflow shrank the paragraph).
        if old_count > new_count {
            for _ in 0..(old_count - new_count) {
                let remove_row = row_start + new_count;
                let content = self
                    .document
                    .buffer
                    .line(remove_row)
                    .unwrap_or("")
                    .to_string();
                self.state.undo_history.record(EditAction::RemoveLine {
                    row: remove_row,
                    content,
                });
                self.document.buffer.remove_line(remove_row)?;
            }
        }

        // Insert extra new lines (reflow expanded the paragraph).
        if new_count > old_count {
            for (i, line) in new_lines.iter().enumerate().take(new_count).skip(old_count) {
                let insert_row = row_start + i;
                let content = line.clone();
                self.state.undo_history.record(EditAction::InsertLine {
                    row: insert_row,
                    content: content.clone(),
                });
                self.document.buffer.insert_line(insert_row, content)?;
            }
        }

        Ok(())
    }
}

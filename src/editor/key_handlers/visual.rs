//! Visual mode handler and visual helpers.

use super::super::Editor;
use super::{apply_case_to_line, toggle_case_range, visual_kind_to_selection_type};
use crate::buffer::unicode::next_grapheme_boundary;
use crate::buffer::{Cursor, Selection};
use crate::command::{
    execute_motion, parse_normal_key, parse_pending, CommandContext, CommandLineState, ParseResult,
    ParsedCommand,
};
use crate::error::Error;
use crate::mode::{Mode, Operator, VisualKind};
use crate::registers::{ContentType, RegisterContent, RegisterId};
use crate::terminal::Key;
use crate::undo::EditAction;

impl Editor {
    /// Enter visual mode with the given kind, setting the anchor at the cursor.
    pub(super) fn enter_visual_mode(&mut self, kind: VisualKind) -> Result<(), Error> {
        let anchor = self.document.cursor;
        self.state.mode = Mode::Visual(kind);
        self.state.visual_anchor = Some(anchor);
        let sel_type = visual_kind_to_selection_type(kind);
        self.document.selection = Some(Selection::new(anchor, anchor, sel_type));
        Ok(())
    }

    /// Handle a key press in visual mode.
    pub(super) fn handle_visual_key(&mut self, key: Key, kind: VisualKind) -> Result<(), Error> {
        // Esc: leave visual mode
        if matches!(key, Key::Esc) {
            self.exit_visual_mode();
            return Ok(());
        }

        // Same-kind visual key: toggle off visual mode
        let same_kind_key = match kind {
            VisualKind::Character => matches!(key, Key::Char('v')),
            VisualKind::Line => matches!(key, Key::Char('V')),
            VisualKind::Block => matches!(key, Key::Ctrl('v')),
        };
        if same_kind_key {
            self.exit_visual_mode();
            return Ok(());
        }

        // Different-kind visual key: switch kind, keep anchor
        if matches!(key, Key::Char('v')) {
            return self.switch_visual_kind(VisualKind::Character);
        }
        if matches!(key, Key::Char('V')) {
            return self.switch_visual_kind(VisualKind::Line);
        }
        if matches!(key, Key::Ctrl('v')) {
            return self.switch_visual_kind(VisualKind::Block);
        }

        // Check if we're in a pending parse (register prefix, find char, gg, etc.)
        // For pending states, we need to process via parse_pending first.
        // Operator keys (d, x, y, Y, c, ~, >, <, J, o, :) are checked AFTER
        // any pending multi-key sequence resolves.
        if self.state.parse_state.is_pending() {
            let parse_result = parse_pending(&mut self.state.parse_state, key, self.state.mode);
            match parse_result {
                ParseResult::Complete(cmd) => {
                    match cmd {
                        ParsedCommand::Motion { motion, count } => {
                            let last_find = self.state.parse_state.last_find();
                            let ctx = CommandContext {
                                buffer: &self.document.buffer,
                                cursor: self.document.cursor,
                                paragraphs: &self.state.settings.paragraphs,
                                sections: &self.state.settings.sections,
                                tabstop: self.state.settings.tabstop,
                            };
                            if let Some(result) = execute_motion(motion, &ctx, count, last_find) {
                                self.document.cursor = result.target;
                                self.state.viewport.ensure_visible(
                                    self.document.cursor.row,
                                    self.document.buffer.len(),
                                );
                                self.update_visual_selection(kind);
                            }
                        }
                        ParsedCommand::OperatorPending { operator, .. }
                            if matches!(operator, Operator::Uppercase | Operator::Lowercase) =>
                        {
                            self.execute_visual_case_operator(kind, operator)?;
                            self.exit_visual_mode();
                        }
                        _ => {} // Non-motion complete commands: ignore in visual
                    }
                }
                ParseResult::Pending => {}
                ParseResult::Invalid => {
                    self.state.parse_state.reset();
                }
            }
            return Ok(());
        }

        // Read any pending register (set by previous '"X' sequence)
        let register = self.state.parse_state.register();

        // Operator keys: d, x, y, Y, c, ~, >, <, J, o, :
        match key {
            Key::Char('d') | Key::Char('x') => {
                self.state.parse_state.reset();
                self.execute_visual_delete(kind, register)?;
                self.exit_visual_mode();
                return Ok(());
            }
            Key::Char('y') => {
                self.state.parse_state.reset();
                self.execute_visual_yank(kind, register)?;
                // Cursor jumps to selection start
                if let Some(anchor) = self.state.visual_anchor {
                    let sel_start = if anchor <= self.document.cursor {
                        anchor
                    } else {
                        self.document.cursor
                    };
                    self.document.cursor = sel_start;
                }
                self.exit_visual_mode();
                return Ok(());
            }
            Key::Char('Y') => {
                self.state.parse_state.reset();
                self.execute_visual_yank_linewise(register)?;
                self.exit_visual_mode();
                return Ok(());
            }
            Key::Char('c') => {
                self.state.parse_state.reset();
                // Visual `c` must share a single undo group across the delete
                // and the subsequent insert session. Begin the group here and
                // call the inner delete directly (without its own group).
                let sel = match &self.document.selection {
                    Some(s) => s.normalize(),
                    None => {
                        self.exit_visual_mode_to_insert();
                        return Ok(());
                    }
                };
                self.state
                    .undo_history
                    .begin_group(self.document.cursor, self.document.modified);
                self.execute_visual_delete_inner(kind, register, &sel)?;
                // Transition to insert mode WITHOUT starting a new group
                self.state.mode = Mode::Insert;
                self.state.visual_anchor = None;
                self.document.selection = None;
                self.state.parse_state.reset_all();
                self.state.insert_state.insert_keys.clear();
                self.state.insert_state.insert_entry_kind = None;
                // Group will be ended when leaving insert mode
                return Ok(());
            }
            Key::Char('~') => {
                self.state.parse_state.reset();
                self.execute_visual_toggle_case(kind)?;
                self.exit_visual_mode();
                return Ok(());
            }
            Key::Char('>') => {
                self.state.parse_state.reset();
                self.execute_visual_indent(kind, true)?;
                self.exit_visual_mode();
                return Ok(());
            }
            Key::Char('<') => {
                self.state.parse_state.reset();
                self.execute_visual_indent(kind, false)?;
                self.exit_visual_mode();
                return Ok(());
            }
            Key::Char('J') => {
                self.state.parse_state.reset();
                self.execute_visual_join(kind)?;
                self.exit_visual_mode();
                return Ok(());
            }
            Key::Char('o') => {
                // Swap cursor and anchor
                if let Some(anchor) = self.state.visual_anchor {
                    let old_cursor = self.document.cursor;
                    self.document.cursor = anchor;
                    self.state.visual_anchor = Some(old_cursor);
                    let sel_type = visual_kind_to_selection_type(kind);
                    self.document.selection = Some(Selection::new(old_cursor, anchor, sel_type));
                    self.state
                        .viewport
                        .ensure_visible(self.document.cursor.row, self.document.buffer.len());
                }
                return Ok(());
            }
            Key::Char(':') => {
                // Set '< and '> marks from the current selection so the
                // pre-filled range resolves correctly in the ex command.
                if let Some(anchor) = self.state.visual_anchor {
                    let cursor = self.document.cursor;
                    let (start, end) = if anchor <= cursor {
                        (anchor, cursor)
                    } else {
                        (cursor, anchor)
                    };
                    self.state.navigation.marks.insert('<', start);
                    self.state.navigation.marks.insert('>', end);
                }
                self.state.mode = Mode::CommandLine;
                self.state.command_line = CommandLineState::new(':');
                // Pre-fill with visual range prefix
                for ch in "'<,'>".chars() {
                    self.state.command_line.insert_char(ch);
                }
                return Ok(());
            }
            Key::Char('p') => {
                self.state.parse_state.reset();
                self.execute_visual_put(kind, register)?;
                self.exit_visual_mode();
                return Ok(());
            }
            Key::Char('I') if kind == VisualKind::Block => {
                self.state.parse_state.reset();
                let sel = match &self.document.selection {
                    Some(s) => s.normalize(),
                    None => {
                        self.exit_visual_mode();
                        return Ok(());
                    }
                };
                let (start_row, end_row, col) = (sel.start.row, sel.end.row, sel.start.col);
                self.exit_visual_mode();
                let clamped_col = self
                    .document
                    .buffer
                    .line(start_row)
                    .map(|l| col.min(l.len()))
                    .unwrap_or(0);
                self.document.cursor = Cursor::new(start_row, clamped_col);
                self.state
                    .undo_history
                    .begin_group(self.document.cursor, self.document.modified);
                self.state.mode = Mode::Insert;
                self.state.insert_state.insert_keys.clear();
                self.state.insert_state.insert_entry_kind = None;
                self.state.insert_state.insert_count = 1;
                self.state.insert_state.block_insert = Some((start_row, end_row, col));
                return Ok(());
            }
            _ => {}
        }

        // Try as a motion key via the normal-mode parser
        let last_find = self.state.parse_state.last_find();
        let parse_result = parse_normal_key(&mut self.state.parse_state, key, self.state.mode);
        match parse_result {
            ParseResult::Complete(cmd) => {
                match cmd {
                    ParsedCommand::Motion { motion, count } => {
                        let ctx = CommandContext {
                            buffer: &self.document.buffer,
                            cursor: self.document.cursor,
                            paragraphs: &self.state.settings.paragraphs,
                            sections: &self.state.settings.sections,
                            tabstop: self.state.settings.tabstop,
                        };
                        if let Some(result) = execute_motion(motion, &ctx, count, last_find) {
                            self.document.cursor = result.target;
                            self.state.viewport.ensure_visible(
                                self.document.cursor.row,
                                self.document.buffer.len(),
                            );
                            self.update_visual_selection(kind);
                        }
                    }
                    ParsedCommand::OperatorPending { operator, .. }
                        if matches!(operator, Operator::Uppercase | Operator::Lowercase) =>
                    {
                        self.execute_visual_case_operator(kind, operator)?;
                        self.exit_visual_mode();
                    }
                    _ => {} // other complete commands ignored in visual mode
                }
            }
            ParseResult::Pending => {
                // Multi-key sequence in progress (e.g., '"' for register, 'f' for find, 'g' for gg)
            }
            ParseResult::Invalid => {
                self.state.parse_state.reset();
                // Unknown key in visual mode: no-op
            }
        }

        Ok(())
    }

    /// Update document.selection based on anchor and current cursor.
    pub(super) fn update_visual_selection(&mut self, kind: VisualKind) {
        if let Some(anchor) = self.state.visual_anchor {
            let sel_type = visual_kind_to_selection_type(kind);
            self.document.selection = Some(Selection::new(anchor, self.document.cursor, sel_type));
        }
    }

    /// Exit visual mode, clearing anchor and selection, returning to Normal.
    ///
    /// Sets the `'<` and `'>` marks to the start and end of the last visual
    /// selection so they can be used as ex addresses after leaving visual mode.
    pub(super) fn exit_visual_mode(&mut self) {
        if let Some(anchor) = self.state.visual_anchor {
            let cursor = self.document.cursor;
            let (start, end) = if anchor <= cursor {
                (anchor, cursor)
            } else {
                (cursor, anchor)
            };
            self.state.navigation.marks.insert('<', start);
            self.state.navigation.marks.insert('>', end);
        }
        self.state.mode = Mode::Normal;
        self.state.visual_anchor = None;
        self.document.selection = None;
        self.state.parse_state.reset_all();
    }

    /// Exit visual mode to Insert mode (for 'c').
    pub(super) fn exit_visual_mode_to_insert(&mut self) {
        if let Some(anchor) = self.state.visual_anchor {
            let cursor = self.document.cursor;
            let (start, end) = if anchor <= cursor {
                (anchor, cursor)
            } else {
                (cursor, anchor)
            };
            self.state.navigation.marks.insert('<', start);
            self.state.navigation.marks.insert('>', end);
        }
        self.state.mode = Mode::Insert;
        self.state.visual_anchor = None;
        self.document.selection = None;
        self.state.parse_state.reset_all();
        // Start undo group for the insert session
        self.state
            .undo_history
            .begin_group(self.document.cursor, self.document.modified);
        self.state.insert_state.insert_keys.clear();
        self.state.insert_state.insert_entry_kind = None;
    }

    /// Switch visual kind while staying in visual mode (keep anchor).
    pub(super) fn switch_visual_kind(&mut self, new_kind: VisualKind) -> Result<(), Error> {
        self.state.mode = Mode::Visual(new_kind);
        self.update_visual_selection(new_kind);
        Ok(())
    }

    /// Delete the visual selection.
    pub(super) fn execute_visual_delete(
        &mut self,
        kind: VisualKind,
        register: Option<char>,
    ) -> Result<(), Error> {
        let sel = match &self.document.selection {
            Some(s) => s.normalize(),
            None => return Ok(()),
        };

        self.state
            .undo_history
            .begin_group(self.document.cursor, self.document.modified);

        let result = self.execute_visual_delete_inner(kind, register, &sel);

        self.state.undo_history.end_group(self.document.cursor);
        result
    }

    pub(super) fn execute_visual_delete_inner(
        &mut self,
        kind: VisualKind,
        register: Option<char>,
        sel: &crate::buffer::Selection,
    ) -> Result<(), Error> {
        match kind {
            VisualKind::Character => {
                let end_exclusive = if let Some(line) = self.document.buffer.line(sel.end.row) {
                    next_grapheme_boundary(line, sel.end.col)
                } else {
                    sel.end.col
                };
                let end_cursor = Cursor::new(sel.end.row, end_exclusive);
                self.delete_motion_range_with_register(
                    &sel.start,
                    &end_cursor,
                    register,
                    ContentType::Characterwise,
                )?;
            }
            VisualKind::Line => {
                let count = sel.end.row - sel.start.row + 1;
                self.document.cursor = sel.start;
                self.delete_lines_with_register(count, register)?;
            }
            VisualKind::Block => {
                let block_text = sel.text(&self.document.buffer);
                let content = RegisterContent::new(block_text, ContentType::Block);
                let reg_id = register.and_then(RegisterId::parse);
                self.state.registers.delete(reg_id, register, content);

                // Delete column range from each row (iterate in reverse to preserve offsets)
                for row in (sel.start.row..=sel.end.row).rev() {
                    if let Some(line) = self.document.buffer.line(row) {
                        let s = sel.start.col.min(line.len());
                        let e = next_grapheme_boundary(line, sel.end.col.min(line.len()));
                        if s < e {
                            let start_c = Cursor::new(row, s);
                            let end_c = Cursor::new(row, e);
                            let deleted = line[s..e].to_string();
                            let action = EditAction::DeleteRange {
                                start_row: row,
                                start_col: s,
                                end_row: row,
                                end_col: e,
                                text: deleted,
                            };
                            self.state.undo_history.record(action);
                            self.document.buffer.delete_range(&start_c, &end_c)?;
                        }
                    }
                }
                self.document.cursor = Cursor::new(sel.start.row, sel.start.col);
                self.clamp_cursor_to_line();
                self.document.modified = true;
            }
        }
        Ok(())
    }

    /// Yank the visual selection (characterwise or linewise).
    pub(super) fn execute_visual_yank(
        &mut self,
        kind: VisualKind,
        register: Option<char>,
    ) -> Result<(), Error> {
        let sel = match &self.document.selection {
            Some(s) => s.normalize(),
            None => return Ok(()),
        };

        match kind {
            VisualKind::Character => {
                let end_exclusive = if let Some(line) = self.document.buffer.line(sel.end.row) {
                    next_grapheme_boundary(line, sel.end.col)
                } else {
                    sel.end.col
                };
                let end_cursor = Cursor::new(sel.end.row, end_exclusive);
                self.yank_range(
                    &sel.start,
                    &end_cursor,
                    register,
                    ContentType::Characterwise,
                )?;
            }
            VisualKind::Line => {
                self.execute_visual_yank_linewise(register)?;
                return Ok(());
            }
            VisualKind::Block => {
                let block_text = sel.text(&self.document.buffer);
                let content = RegisterContent::new(block_text, ContentType::Block);
                let reg_id = register.and_then(RegisterId::parse);
                self.state.registers.yank(reg_id, content);
            }
        }

        Ok(())
    }

    /// Yank full lines of the visual selection (Y or V-mode y).
    pub(super) fn execute_visual_yank_linewise(
        &mut self,
        register: Option<char>,
    ) -> Result<(), Error> {
        let sel = match &self.document.selection {
            Some(s) => s.normalize(),
            None => return Ok(()),
        };
        self.yank_lines_range(sel.start.row, sel.end.row + 1, register)
    }

    /// Toggle case of all characters in the visual selection.
    pub(super) fn execute_visual_toggle_case(&mut self, kind: VisualKind) -> Result<(), Error> {
        let sel = match &self.document.selection {
            Some(s) => s.normalize(),
            None => return Ok(()),
        };

        self.state
            .undo_history
            .begin_group(self.document.cursor, self.document.modified);

        match kind {
            VisualKind::Character => {
                // Toggle each character in the selection range
                for row in sel.start.row..=sel.end.row {
                    let (col_start, col_end) = if let Some(line) = self.document.buffer.line(row) {
                        let end = if sel.start.row == sel.end.row || row == sel.end.row {
                            next_grapheme_boundary(line, sel.end.col.min(line.len()))
                        } else {
                            line.len()
                        };
                        let start = if row == sel.start.row {
                            sel.start.col.min(line.len())
                        } else {
                            0
                        };
                        (start, end)
                    } else {
                        continue;
                    };

                    if let Some(line) = self.document.buffer.line(row) {
                        let new_line = toggle_case_range(line, col_start, col_end);
                        self.replace_line_recorded(row, new_line)?;
                    }
                }
                self.document.modified = true;
            }
            VisualKind::Line | VisualKind::Block => {
                for row in sel.start.row..=sel.end.row {
                    if let Some(line) = self.document.buffer.line(row) {
                        let new_line = toggle_case_range(line, 0, line.len());
                        self.replace_line_recorded(row, new_line)?;
                    }
                }
                self.document.modified = true;
            }
        }

        self.document.cursor = sel.start;
        self.clamp_cursor_to_line();
        self.state.undo_history.end_group(self.document.cursor);
        Ok(())
    }

    /// Apply an uppercase or lowercase operator to the visual selection.
    pub(super) fn execute_visual_case_operator(
        &mut self,
        kind: VisualKind,
        operator: Operator,
    ) -> Result<(), Error> {
        let sel = match &self.document.selection {
            Some(s) => s.normalize(),
            None => return Ok(()),
        };

        self.state
            .undo_history
            .begin_group(self.document.cursor, self.document.modified);

        match kind {
            VisualKind::Character => {
                for row in sel.start.row..=sel.end.row {
                    let (col_start, col_end) = if let Some(line) = self.document.buffer.line(row) {
                        let end = if sel.start.row == sel.end.row || row == sel.end.row {
                            next_grapheme_boundary(line, sel.end.col.min(line.len()))
                        } else {
                            line.len()
                        };
                        let start = if row == sel.start.row {
                            sel.start.col.min(line.len())
                        } else {
                            0
                        };
                        (start, end)
                    } else {
                        continue;
                    };
                    if let Some(line) = self.document.buffer.line(row) {
                        let new_line = apply_case_to_line(line, col_start, col_end, operator);
                        self.replace_line_recorded(row, new_line)?;
                    }
                }
                self.document.modified = true;
            }
            VisualKind::Line | VisualKind::Block => {
                for row in sel.start.row..=sel.end.row {
                    if let Some(line) = self.document.buffer.line(row) {
                        let new_line = apply_case_to_line(line, 0, line.len(), operator);
                        self.replace_line_recorded(row, new_line)?;
                    }
                }
                self.document.modified = true;
            }
        }

        self.document.cursor = sel.start;
        self.clamp_cursor_to_line();
        self.state.undo_history.end_group(self.document.cursor);
        Ok(())
    }

    /// Delete the visual selection and paste the (pre-delete) unnamed register in its place.
    pub(super) fn execute_visual_put(
        &mut self,
        kind: VisualKind,
        register: Option<char>,
    ) -> Result<(), Error> {
        let sel = match &self.document.selection {
            Some(s) => s.normalize(),
            None => return Ok(()),
        };

        // Save the paste content before the delete overwrites the unnamed register.
        let reg_id = register.and_then(RegisterId::parse);
        let paste_content = self.state.registers.get_owned(reg_id);

        self.state
            .undo_history
            .begin_group(self.document.cursor, self.document.modified);

        // Delete the selection (puts selection into unnamed register).
        self.execute_visual_delete_inner(kind, None, &sel)?;

        // Save the deleted selection — it must remain in the unnamed register
        // after the paste so the `vp` → `p` cycling idiom works.
        let deleted_content = self.state.registers.get_owned(None);

        // Paste the original register content at cursor (now at selection start).
        if let Some(content) = paste_content {
            self.state.registers.yank(None, content);
            self.put_before(None)?;
        }

        // Restore the deleted selection to the unnamed register.
        if let Some(deleted) = deleted_content {
            self.state.registers.yank(None, deleted);
        }

        self.state.undo_history.end_group(self.document.cursor);
        Ok(())
    }

    /// Indent or dedent lines in the visual selection.
    pub(super) fn execute_visual_indent(
        &mut self,
        _kind: VisualKind,
        indent: bool,
    ) -> Result<(), Error> {
        let sel = match &self.document.selection {
            Some(s) => s.normalize(),
            None => return Ok(()),
        };
        let line_count = sel.end.row - sel.start.row + 1;

        self.state
            .undo_history
            .begin_group(self.document.cursor, self.document.modified);

        // Move cursor to start row, call shift, then restore
        self.document.cursor = Cursor::new(sel.start.row, 0);
        if indent {
            self.shift_lines_right(line_count)?;
        } else {
            self.shift_lines_left(line_count)?;
        }

        self.state.undo_history.end_group(self.document.cursor);
        Ok(())
    }

    /// Join lines in the visual selection.
    pub(super) fn execute_visual_join(&mut self, _kind: VisualKind) -> Result<(), Error> {
        let sel = match &self.document.selection {
            Some(s) => s.normalize(),
            None => return Ok(()),
        };
        // When only one line is selected, join it with the next line (like normal J).
        let joins = if sel.end.row > sel.start.row {
            sel.end.row - sel.start.row
        } else {
            1
        };

        self.state
            .undo_history
            .begin_group(self.document.cursor, self.document.modified);

        self.document.cursor = Cursor::new(sel.start.row, 0);
        for _ in 0..joins {
            self.join_line_with_next()?;
        }

        self.state.undo_history.end_group(self.document.cursor);
        Ok(())
    }
}

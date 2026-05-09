//! Normal mode and operator-pending mode key handlers.

use super::super::{Editor, IncSearchSaved, StatusMessage};
use crate::buffer::Cursor;
use crate::command::CommandLineState;
use crate::command::{
    execute_motion, parse_normal_key, parse_pending, parse_text_object_key, CommandContext,
    ModeChangeKind, Motion, MotionKind, ParseResult, ParsedCommand, RepeatableChange, SimpleAction,
};
use crate::error::Error;
use crate::mode::{Mode, Operator, VisualKind};
use crate::registers::ContentType;
use crate::terminal::Key;

impl Editor {
    /// Handle key press in normal mode using the command parser.
    pub(super) fn handle_normal_key(&mut self, key: Key) -> Result<(), Error> {
        // When recording a macro, a bare 'q' (not followed by a register) stops recording.
        // We check this before the command parser because the parser requires q{a-z}.
        if self.state.macro_state.recording.is_some()
            && matches!(key, Key::Char('q'))
            && !self.state.parse_state.is_pending()
        {
            // Remove the 'q' we just added to macro_buffer (it shouldn't be stored)
            self.state.macro_state.buffer.pop();
            self.stop_macro_recording();
            return Ok(());
        }

        // ':' enters command-line mode (must be checked before the command parser)
        if matches!(key, Key::Char(':')) {
            self.state.mode = Mode::CommandLine;
            self.state.command_line = CommandLineState::new(':');
            return Ok(());
        }

        // '/' enters forward search mode
        if matches!(key, Key::Char('/')) {
            if self.state.settings.incsearch {
                self.state.incsearch_saved = Some(IncSearchSaved {
                    search: self.state.search.clone(),
                    cursor: self.document.cursor,
                    viewport: self.state.viewport.clone(),
                });
            }
            self.state.mode = Mode::CommandLine;
            self.state.command_line = CommandLineState::new('/');
            return Ok(());
        }

        // '?' enters backward search mode
        if matches!(key, Key::Char('?')) {
            if self.state.settings.incsearch {
                self.state.incsearch_saved = Some(IncSearchSaved {
                    search: self.state.search.clone(),
                    cursor: self.document.cursor,
                    viewport: self.state.viewport.clone(),
                });
            }
            self.state.mode = Mode::CommandLine;
            self.state.command_line = CommandLineState::new('?');
            return Ok(());
        }

        // Ctrl-C: POSIX interrupt — cancel pending state, do not quit
        if matches!(key, Key::Ctrl('c')) {
            self.state.mode = Mode::Normal;
            self.state.parse_state.reset_all();
            self.state.status_message = Some(StatusMessage::Error("Interrupt".to_string()));
            return Ok(());
        }

        // Scroll commands — handled before the command parser.
        // These accept an optional count prefix from parse_state.
        match key {
            Key::Ctrl('f') => {
                let count = self.state.parse_state.count();
                self.state.parse_state.reset();
                self.scroll_full_screen_forward(count);
                return Ok(());
            }
            Key::Ctrl('b') => {
                let count = self.state.parse_state.count();
                self.state.parse_state.reset();
                self.scroll_full_screen_backward(count);
                return Ok(());
            }
            Key::Ctrl('d') => {
                let count_opt = self.state.parse_state.count_opt();
                self.state.parse_state.reset();
                self.scroll_half_screen_down(count_opt);
                return Ok(());
            }
            Key::Ctrl('u') => {
                let count_opt = self.state.parse_state.count_opt();
                self.state.parse_state.reset();
                self.scroll_half_screen_up(count_opt);
                return Ok(());
            }
            Key::Ctrl('e') => {
                let count = self.state.parse_state.count();
                self.state.parse_state.reset();
                self.scroll_line_down(count);
                return Ok(());
            }
            Key::Ctrl('y') => {
                let count = self.state.parse_state.count();
                self.state.parse_state.reset();
                self.scroll_line_up(count);
                return Ok(());
            }
            Key::Ctrl('g') => {
                self.state.parse_state.reset();
                self.show_file_info();
                return Ok(());
            }
            Key::Ctrl('l') => {
                self.state.parse_state.reset();
                // Full redraw is handled by the render loop automatically;
                // just return so the next render cycle redraws everything.
                return Ok(());
            }
            Key::Ctrl(']') => {
                self.state.parse_state.reset();
                // Extract the word (identifier chars) under the cursor
                let word = self.word_under_cursor();
                if word.is_empty() {
                    self.state.status_message = Some(super::super::StatusMessage::Error(
                        "No tag name".to_string(),
                    ));
                } else if let Err(e) = self.execute_tag_jump(&word) {
                    self.state.status_message = Some(super::super::StatusMessage::Error(e));
                }
                return Ok(());
            }
            Key::Ctrl('t') => {
                self.state.parse_state.reset();
                if let Err(e) = self.execute_tag_pop() {
                    self.state.status_message = Some(super::super::StatusMessage::Error(e));
                }
                return Ok(());
            }
            _ => {}
        }

        // Enter visual mode
        if let Key::Char('v') = key {
            return self.enter_visual_mode(VisualKind::Character);
        }
        if let Key::Char('V') = key {
            return self.enter_visual_mode(VisualKind::Line);
        }
        if let Key::Ctrl('v') = key {
            return self.enter_visual_mode(VisualKind::Block);
        }

        // Use the command parser
        let result = if self.state.parse_state.is_pending() {
            parse_pending(&mut self.state.parse_state, key, self.state.mode)
        } else {
            parse_normal_key(&mut self.state.parse_state, key, self.state.mode)
        };

        match result {
            ParseResult::Complete(cmd) => self.execute_command(cmd),
            ParseResult::Pending => {
                // Command in progress, update status if needed
                Ok(())
            }
            ParseResult::Invalid => {
                // Invalid key sequence, reset and ignore
                self.state.parse_state.reset();
                Ok(())
            }
        }
    }

    /// Handle key press in operator-pending mode.
    pub(super) fn handle_operator_pending_key(
        &mut self,
        key: Key,
        operator: Operator,
    ) -> Result<(), Error> {
        // Escape cancels operator-pending mode
        if matches!(key, Key::Esc) {
            self.state.mode = Mode::Normal;
            self.state.parse_state.reset_all();
            return Ok(());
        }

        // Check if we have a pending text object
        if self.state.parse_state.pending_text_object().is_some() {
            // Parse the text object specifier
            let result = parse_text_object_key(&mut self.state.parse_state, key, operator);
            match result {
                ParseResult::Complete(cmd) => {
                    self.state.mode = Mode::Normal;
                    return self.execute_command(cmd);
                }
                ParseResult::Pending => {
                    return Ok(());
                }
                ParseResult::Invalid => {
                    self.state.mode = Mode::Normal;
                    self.state.parse_state.reset_all();
                    return Ok(());
                }
            }
        }

        // Parse the key as a motion or the operator repeated (dd, yy, cc)
        let result = if self.state.parse_state.is_pending() {
            parse_pending(&mut self.state.parse_state, key, self.state.mode)
        } else {
            parse_normal_key(&mut self.state.parse_state, key, self.state.mode)
        };

        match result {
            ParseResult::Complete(cmd) => {
                // Return to normal mode first
                self.state.mode = Mode::Normal;
                self.execute_operator_command(operator, cmd)
            }
            ParseResult::Pending => {
                // Still waiting for more keys (could be text object)
                Ok(())
            }
            ParseResult::Invalid => {
                // Invalid key, return to normal mode
                self.state.mode = Mode::Normal;
                self.state.parse_state.reset_all();
                Ok(())
            }
        }
    }

    /// Execute a fully parsed command.
    pub(super) fn execute_command(&mut self, cmd: ParsedCommand) -> Result<(), Error> {
        match cmd {
            ParsedCommand::Motion { motion, count } => {
                self.execute_motion_command(motion, count)?;
            }
            ParsedCommand::ModeChange(kind, count) => {
                self.execute_mode_change_with_count(kind, count)?;
            }
            ParsedCommand::SimpleAction { action, count } => {
                self.execute_simple_action(action, None, count)?;
            }
            ParsedCommand::SimpleActionWithRegister {
                action,
                register,
                count,
            } => {
                self.execute_simple_action(action, register, count)?;
            }
            ParsedCommand::OperatorPending {
                operator,
                count,
                register,
            } => {
                // Enter operator-pending mode and store the operator count and register
                self.state.mode = Mode::OperatorPending(operator);
                // Store the count so it can be multiplied with the motion count
                self.state.parse_state.set_operator_count(count);
                // Store the register for the operator
                self.state.parse_state.set_operator_register(register);
            }
            ParsedCommand::OperatorMotion {
                operator,
                motion,
                count,
                register,
            } => {
                // This is dd, yy, cc, >>, <<, gUU, etc. — Motion::Down is a sentinel
                self.execute_operator_with_motion(operator, motion, count, register, true)?;
            }
            ParsedCommand::OperatorTextObject {
                operator,
                kind,
                object,
                count,
                register,
            } => {
                self.execute_operator_with_text_object(operator, kind, object, count, register)?;
            }
            ParsedCommand::Repeat { count } => {
                self.execute_repeat(count)?;
            }
            ParsedCommand::Incomplete | ParsedCommand::Invalid => {
                // Should not reach here from Complete, but handle gracefully
            }
        }
        Ok(())
    }

    /// Execute an operator command after receiving a motion.
    fn execute_operator_command(
        &mut self,
        operator: Operator,
        cmd: ParsedCommand,
    ) -> Result<(), Error> {
        // Get the operator count and register that were stored when entering operator-pending mode
        let operator_count = self.state.parse_state.operator_count();
        let operator_register = self.state.parse_state.operator_register();

        match cmd {
            ParsedCommand::Motion { motion, count } => {
                // Multiply operator count by motion count (e.g., 3d2w = delete 6 words)
                let total_count = operator_count * count;
                self.execute_operator_with_motion(
                    operator,
                    motion,
                    total_count,
                    operator_register,
                    false,
                )?;
            }
            ParsedCommand::OperatorMotion {
                operator: op2,
                motion,
                count,
                register,
            } if op2 == operator => {
                // This handles the case of dd, yy, cc where the second d/y/c
                // is parsed while already in operator-pending mode
                // Multiply operator count by motion count
                let total_count = operator_count * count;
                // Use the most recently specified register (from second operator if set)
                let reg = register.or(operator_register);
                self.execute_operator_with_motion(operator, motion, total_count, reg, true)?;
            }
            ParsedCommand::OperatorMotion { .. } => {
                // Invalid operator combination
            }
            ParsedCommand::OperatorTextObject {
                operator: _,
                kind,
                object,
                count,
                register,
            } => {
                // Text object command - use the stored operator count and register
                let total_count = operator_count * count;
                let reg = register.or(operator_register);
                self.execute_operator_with_text_object(operator, kind, object, total_count, reg)?;
            }
            _ => {
                // Invalid command in operator-pending context
            }
        }

        // Clear the operator count and register after execution
        self.state.parse_state.clear_operator_count();
        self.state.parse_state.clear_operator_register();
        Ok(())
    }

    /// Execute a motion command, moving the cursor.
    pub(super) fn execute_motion_command(
        &mut self,
        motion: Motion,
        count: usize,
    ) -> Result<(), Error> {
        // Handle screen position motions that need viewport data.
        match motion {
            Motion::ScreenTop(offset) => {
                self.move_to_screen_top(offset);
                return Ok(());
            }
            Motion::ScreenMiddle => {
                self.move_to_screen_middle();
                return Ok(());
            }
            Motion::ScreenBottom(offset) => {
                self.move_to_screen_bottom(offset);
                return Ok(());
            }
            _ => {}
        }

        // Push to jump list before large motions
        let is_jump_motion = matches!(
            motion,
            Motion::DocumentStart
                | Motion::DocumentEnd
                | Motion::GotoLine(_)
                | Motion::MatchingBracket
        );
        if is_jump_motion {
            self.push_jump();
        }

        let ctx = CommandContext {
            buffer: &self.document.buffer,
            cursor: self.document.cursor,
            paragraphs: &self.state.settings.paragraphs,
            sections: &self.state.settings.sections,
            tabstop: self.state.settings.tabstop,
        };

        let last_find = self.state.parse_state.last_find();

        if let Some(result) = execute_motion(motion, &ctx, count, last_find) {
            self.document.cursor = result.target;
            // Clamp so motions that legitimately return line.len() (e.g. `w`
            // at end of last line, where operators need the exclusive sentinel)
            // don't leave the cursor past the last character in normal mode.
            self.clamp_cursor_to_line();

            // Ensure cursor is visible in viewport
            self.state
                .viewport
                .ensure_visible(self.document.cursor.row, self.document.buffer.len());
        }

        // Take a snapshot of the new line for the U command.
        self.update_line_snapshot();

        Ok(())
    }

    /// Compute the effective reflow width for the `gq` operator.
    ///
    /// Returns `textwidth` if set (> 0); otherwise falls back to
    /// `viewport_width - wrapmargin` if `wrapmargin > 0`; otherwise returns 0
    /// (meaning gq is a no-op).
    fn effective_textwidth(&self) -> usize {
        let tw = self.state.settings.textwidth;
        if tw > 0 {
            return tw;
        }
        let wm = self.state.settings.wrapmargin;
        if wm > 0 {
            return self.state.viewport.width().saturating_sub(wm);
        }
        0
    }

    /// Compute the target row for a screen position motion (H/M/L) for use in operators.
    pub(super) fn screen_motion_target_row(&self, motion: Motion) -> Option<usize> {
        let buf_len = self.document.buffer.len();
        if buf_len == 0 {
            return None;
        }
        match motion {
            Motion::ScreenTop(offset) => {
                let row = (self.state.viewport.top_line() + offset).min(buf_len.saturating_sub(1));
                Some(row)
            }
            Motion::ScreenMiddle => {
                let top = self.state.viewport.top_line();
                let visible = self
                    .state
                    .viewport
                    .height()
                    .min(buf_len.saturating_sub(top));
                Some((top + visible / 2).min(buf_len.saturating_sub(1)))
            }
            Motion::ScreenBottom(offset) => {
                let top = self.state.viewport.top_line();
                let bottom = (top + self.state.viewport.height().saturating_sub(1))
                    .min(buf_len.saturating_sub(1));
                Some(
                    bottom
                        .saturating_sub(offset)
                        .max(top)
                        .min(buf_len.saturating_sub(1)),
                )
            }
            _ => None,
        }
    }

    /// Execute a mode change command with a count (e.g. 3i — repeat insert 3 times on Esc).
    pub(super) fn execute_mode_change_with_count(
        &mut self,
        kind: ModeChangeKind,
        count: usize,
    ) -> Result<(), Error> {
        self.state.insert_state.insert_count = count;
        self.execute_mode_change(kind)
    }

    /// Execute a mode change command.
    ///
    /// When entering insert mode, an undo group is started so all edits
    /// during the insert session form a single undoable unit. The insert
    /// key buffer is cleared for dot recording.
    pub(super) fn execute_mode_change(&mut self, kind: ModeChangeKind) -> Result<(), Error> {
        // Clear insert key buffer for dot recording
        self.state.insert_state.insert_keys.clear();
        // Record how we entered insert mode
        self.state.insert_state.insert_entry_kind = Some(kind);

        match kind {
            ModeChangeKind::InsertBeforeCursor => {
                // i - insert before cursor
                self.state
                    .undo_history
                    .begin_group(self.document.cursor, self.document.modified);
                self.state.mode = Mode::Insert;
            }
            ModeChangeKind::InsertAfterCursor => {
                // a - append after cursor
                self.state
                    .undo_history
                    .begin_group(self.document.cursor, self.document.modified);
                self.move_right_unconstrained();
                self.state.mode = Mode::Insert;
            }
            ModeChangeKind::InsertAtLineEnd => {
                // A - append at end of line
                self.state
                    .undo_history
                    .begin_group(self.document.cursor, self.document.modified);
                self.move_to_line_end_unconstrained();
                self.state.mode = Mode::Insert;
            }
            ModeChangeKind::InsertAtFirstNonBlank => {
                // I - insert at first non-blank
                self.state
                    .undo_history
                    .begin_group(self.document.cursor, self.document.modified);
                self.move_to_first_non_blank();
                self.state.mode = Mode::Insert;
            }
            ModeChangeKind::OpenLineBelow => {
                // o - open line below
                self.state
                    .undo_history
                    .begin_group(self.document.cursor, self.document.modified);
                self.open_line_below()?;
                self.state.mode = Mode::Insert;
            }
            ModeChangeKind::OpenLineAbove => {
                // O - open line above
                self.state
                    .undo_history
                    .begin_group(self.document.cursor, self.document.modified);
                self.open_line_above()?;
                self.state.mode = Mode::Insert;
            }
            ModeChangeKind::ReplaceMode => {
                self.state.insert_state.replace_originals.clear();
                self.state
                    .undo_history
                    .begin_group(self.document.cursor, self.document.modified);
                self.state.insert_state.insert_entry_kind = Some(ModeChangeKind::ReplaceMode);
                self.state.mode = Mode::Replace;
            }
        }
        Ok(())
    }

    /// Execute a simple action.
    ///
    /// Normal mode editing commands (x, X, r, D, C, p, P) are each wrapped
    /// in their own undo group so they form individual undoable units.
    /// Undo and redo are not wrapped since they operate on the undo stack directly.
    ///
    /// Buffer-modifying commands record themselves as `last_change` for dot
    /// repeat, unless the `replaying_dot` flag is set.
    ///
    /// For most actions, count means "repeat N times". For JoinLines,
    /// count means "number of lines" and the action is called once with the
    /// full count passed through.
    pub(super) fn execute_simple_action(
        &mut self,
        action: SimpleAction,
        register: Option<char>,
        count: usize,
    ) -> Result<(), Error> {
        // Non-buffer-modifying commands: skip dot recording
        let is_search = matches!(
            action,
            SimpleAction::SearchNext
                | SimpleAction::SearchPrev
                | SimpleAction::SearchWordForward
                | SimpleAction::SearchWordBackward
                | SimpleAction::SetMark(_)
                | SimpleAction::JumpToMark { .. }
                | SimpleAction::ToggleMacroRecord(_)
                | SimpleAction::PlayMacro(_)
                | SimpleAction::RepeatMacro
                | SimpleAction::JumpBack
                | SimpleAction::JumpForward
                | SimpleAction::ScrollCursorTop
                | SimpleAction::ScrollCursorMiddle
                | SimpleAction::ScrollCursorBottom
                | SimpleAction::ShowFileInfo
                | SimpleAction::WriteQuit
                | SimpleAction::ForceQuit
        );

        // Record the change BEFORE executing (so we capture the full count)
        if !self.state.insert_state.replaying_dot && !is_search {
            self.state.last_change = Some(RepeatableChange::SimpleAction {
                action,
                register,
                count,
            });
        }

        // JoinLines and JoinLinesNoSpace use count as "number of lines to join"
        if matches!(action, SimpleAction::JoinLines) {
            return self.execute_join_lines_action(count);
        }
        if matches!(action, SimpleAction::JoinLinesNoSpace) {
            return self.execute_join_lines_no_space_action(count);
        }

        // ToggleCase wraps all N toggles in a single undo group so that
        // a counted command like 3~ can be undone with a single `u`.
        if matches!(action, SimpleAction::ToggleCase) {
            self.state
                .undo_history
                .begin_group(self.document.cursor, self.document.modified);
            for _ in 0..count {
                self.toggle_case_at_cursor()?;
            }
            self.state.undo_history.end_group(self.document.cursor);
            return Ok(());
        }

        // ReplaceChar (r): replace N consecutive characters then leave cursor on the last.
        if let SimpleAction::ReplaceChar(ch) = action {
            self.state
                .undo_history
                .begin_group(self.document.cursor, self.document.modified);
            for i in 0..count {
                // Check we haven't walked off the end of the line.
                let at_end = self
                    .document
                    .buffer
                    .line(self.document.cursor.row)
                    .map(|l| self.document.cursor.col >= l.len())
                    .unwrap_or(true);
                if at_end {
                    break;
                }
                self.replace_char_at_cursor(ch)?;
                // Advance cursor between replacements, but not after the last one.
                if i + 1 < count {
                    self.move_right();
                }
            }
            self.state.undo_history.end_group(self.document.cursor);
            return Ok(());
        }

        // Execute the action count times
        for _ in 0..count {
            self.execute_simple_action_once(action, register)?;
        }
        Ok(())
    }

    /// Execute a join lines (J) action with the given line count.
    ///
    /// Wraps all line joins in a single undo group so a single `u` undoes
    /// the entire operation. With count N, joins N lines (current + N-1 following).
    fn execute_join_lines_action(&mut self, line_count: usize) -> Result<(), Error> {
        self.state
            .undo_history
            .begin_group(self.document.cursor, self.document.modified);

        // Join line_count - 1 times (e.g., 3J joins 3 lines = 2 join operations)
        let joins = if line_count <= 1 { 1 } else { line_count - 1 };
        for _ in 0..joins {
            self.join_line_with_next()?;
        }

        self.state.undo_history.end_group(self.document.cursor);
        Ok(())
    }

    fn execute_join_lines_no_space_action(&mut self, line_count: usize) -> Result<(), Error> {
        self.state
            .undo_history
            .begin_group(self.document.cursor, self.document.modified);
        let joins = if line_count <= 1 { 1 } else { line_count - 1 };
        for _ in 0..joins {
            self.join_line_with_next_no_space()?;
        }
        self.state.undo_history.end_group(self.document.cursor);
        Ok(())
    }

    pub(super) fn execute_simple_action_once(
        &mut self,
        action: SimpleAction,
        register: Option<char>,
    ) -> Result<(), Error> {
        match action {
            SimpleAction::DeleteCharAtCursor => {
                self.state
                    .undo_history
                    .begin_group(self.document.cursor, self.document.modified);
                self.delete_char_at_cursor_with_register(register)?;
                self.state.undo_history.end_group(self.document.cursor);
            }
            SimpleAction::DeleteCharBeforeCursor => {
                self.state
                    .undo_history
                    .begin_group(self.document.cursor, self.document.modified);
                self.delete_char_before_cursor_with_register(register)?;
                self.state.undo_history.end_group(self.document.cursor);
            }
            SimpleAction::ReplaceChar(ch) => {
                self.state
                    .undo_history
                    .begin_group(self.document.cursor, self.document.modified);
                self.replace_char_at_cursor(ch)?;
                self.state.undo_history.end_group(self.document.cursor);
            }
            SimpleAction::Undo => {
                // Not repeatable
                self.apply_undo()?;
            }
            SimpleAction::Redo => {
                // Not repeatable
                self.apply_redo()?;
            }
            SimpleAction::PutAfter => {
                self.state
                    .undo_history
                    .begin_group(self.document.cursor, self.document.modified);
                self.put_after(register)?;
                self.state.undo_history.end_group(self.document.cursor);
            }
            SimpleAction::PutBefore => {
                self.state
                    .undo_history
                    .begin_group(self.document.cursor, self.document.modified);
                self.put_before(register)?;
                self.state.undo_history.end_group(self.document.cursor);
            }
            SimpleAction::DeleteToEndOfLine => {
                self.state
                    .undo_history
                    .begin_group(self.document.cursor, self.document.modified);
                self.delete_to_end_of_line(register)?;
                self.state.undo_history.end_group(self.document.cursor);
            }
            SimpleAction::ChangeToEndOfLine => {
                // C enters insert mode, so we begin a group that spans the
                // delete and all subsequent insert mode edits.
                // Model as OperatorMotion with Change + LineEnd for dot repeat.
                self.state
                    .undo_history
                    .begin_group(self.document.cursor, self.document.modified);
                self.delete_to_end_of_line(register)?;
                // Position cursor at end of line for Insert mode.
                if let Some(line) = self.document.buffer.line(self.document.cursor.row) {
                    self.document.cursor.col = line.len();
                }
                self.state.mode = Mode::Insert;
                // Group will be ended when leaving insert mode

                // Prepare dot recording: store as OperatorMotion with Change + LineEnd.
                // insert_keys will be filled when insert mode exits.
                self.state.insert_state.insert_keys.clear();
                self.state.insert_state.insert_entry_kind = None; // Not a pure insert session

                if !self.state.insert_state.replaying_dot {
                    self.state.last_change = Some(RepeatableChange::OperatorMotion {
                        operator: Operator::Change,
                        motion: Motion::LineEnd,
                        count: 1,
                        register,
                        insert_keys: None,
                        linewise_self: false,
                    });
                }
            }
            SimpleAction::YankLine => {
                // Yank does not modify the buffer, so no undo group needed.
                // Not repeatable.
                self.yank_lines(1, register)?;
            }
            SimpleAction::SearchNext => {
                self.execute_search_repeat(false)?;
            }
            SimpleAction::SearchPrev => {
                self.execute_search_repeat(true)?;
            }
            // JoinLines is handled in execute_simple_action before reaching this method.
            SimpleAction::JoinLines => {
                // Handled by execute_join_lines_action; should not reach here
            }
            SimpleAction::ToggleCase => {
                self.state
                    .undo_history
                    .begin_group(self.document.cursor, self.document.modified);
                self.toggle_case_at_cursor()?;
                self.state.undo_history.end_group(self.document.cursor);
            }
            SimpleAction::SearchWordForward => {
                self.search_word_under_cursor(crate::search::SearchDirection::Forward)?;
            }
            SimpleAction::SearchWordBackward => {
                self.search_word_under_cursor(crate::search::SearchDirection::Backward)?;
            }
            SimpleAction::SetMark(ch) => {
                self.state.navigation.marks.insert(ch, self.document.cursor);
            }
            SimpleAction::JumpToMark { mark, line_start } => {
                self.push_jump();
                self.execute_jump_to_mark(mark, line_start);
            }
            SimpleAction::JumpBack => {
                self.execute_jump_back();
            }
            SimpleAction::JumpForward => {
                self.execute_jump_forward();
            }
            SimpleAction::ToggleMacroRecord(ch) => {
                if self.state.macro_state.recording.is_some() {
                    // Already recording: stop (any register name closes it)
                    self.stop_macro_recording();
                } else {
                    // Start recording into register ch
                    self.state.macro_state.recording = Some(ch);
                    self.state.macro_state.buffer.clear();
                    self.state.status_message = Some(super::super::StatusMessage::Info(format!(
                        "recording @{}",
                        ch
                    )));
                }
            }
            SimpleAction::PlayMacro(ch) => {
                self.state.macro_state.last_macro = Some(ch);
                self.play_macro(ch)?;
            }
            SimpleAction::RepeatMacro => {
                if let Some(ch) = self.state.macro_state.last_macro {
                    self.play_macro(ch)?;
                }
            }
            SimpleAction::ScrollCursorTop => {
                let row = self.document.cursor.row;
                let buf_len = self.document.buffer.len();
                self.state.viewport.scroll_to_line(row, buf_len);
            }
            SimpleAction::ScrollCursorMiddle => {
                let row = self.document.cursor.row;
                let buf_len = self.document.buffer.len();
                let height = self.state.viewport.height();
                let new_top = row.saturating_sub(height / 2);
                let max_top = buf_len.saturating_sub(height);
                self.state.viewport.set_top_line(new_top.min(max_top));
            }
            SimpleAction::ScrollCursorBottom => {
                let row = self.document.cursor.row;
                let height = self.state.viewport.height();
                let buf_len = self.document.buffer.len();
                let new_top = row.saturating_sub(height.saturating_sub(1));
                let max_top = buf_len.saturating_sub(height);
                self.state.viewport.set_top_line(new_top.min(max_top));
            }
            SimpleAction::ShowFileInfo => {
                self.show_file_info();
            }
            SimpleAction::UndoLine => {
                self.execute_undo_line()?;
            }
            SimpleAction::WriteQuit => {
                // ZZ is equivalent to :x — write only if modified, then quit
                if self.document.modified {
                    if let Err(e) = self.save_file() {
                        self.state.status_message =
                            Some(super::super::StatusMessage::Error(e.to_string()));
                        return Ok(());
                    }
                }
                self.state.should_quit = true;
            }
            SimpleAction::ForceQuit => {
                self.state.should_quit = true;
            }
            // JoinLinesNoSpace is handled in execute_join_lines_no_space_action.
            SimpleAction::JoinLinesNoSpace => {}
            SimpleAction::EnterExMode => {
                if let Err(e) = self.run_ex_mode() {
                    self.state.status_message =
                        Some(super::super::StatusMessage::Error(e.to_string()));
                }
            }
            SimpleAction::RepeatSubstitute => {
                if let Some(cmd) = self.state.substitute_repeat.last_substitute.clone() {
                    // Re-run with current line range
                    let mut repeat_cmd = cmd;
                    repeat_cmd.range = crate::search::substitute::SubstituteRange::CurrentLine;
                    self.execute_substitute(repeat_cmd);
                } else {
                    self.state.status_message = Some(super::super::StatusMessage::Error(
                        "No previous substitute command".to_string(),
                    ));
                }
            }
            SimpleAction::EditAlternateFile => {
                if let Some(alt) = self.state.file_navigation.alternate_file.clone() {
                    if self.document.modified && self.state.settings.autowrite {
                        if let Err(e) = self.save_file() {
                            self.state.status_message =
                                Some(super::super::StatusMessage::Error(e.to_string()));
                            return Ok(());
                        }
                    }
                    if self.document.modified {
                        self.state.status_message = Some(super::super::StatusMessage::Error(
                            "No write since last change (add ! to override)".to_string(),
                        ));
                    } else {
                        let prev = self.document.filename.clone();
                        if let Err(e) = self.open_file(&alt) {
                            self.state.status_message =
                                Some(super::super::StatusMessage::Error(e.to_string()));
                        } else {
                            self.state.file_navigation.alternate_file = prev;
                        }
                    }
                } else {
                    self.state.status_message = Some(super::super::StatusMessage::Error(
                        "No alternate file".to_string(),
                    ));
                }
            }
        }
        Ok(())
    }

    /// Execute an operator with a motion.
    ///
    /// Wraps the operation in an undo group. For Change operator, the group
    /// spans both the deletion and the subsequent insert mode session.
    /// Records the command as `last_change` for dot repeat.
    pub(super) fn execute_operator_with_motion(
        &mut self,
        operator: Operator,
        motion: Motion,
        count: usize,
        register: Option<char>,
        linewise_self: bool,
    ) -> Result<(), Error> {
        // `linewise_self` is true for doubled operators (dd, yy, cc, >>, <<, gUU, etc.)
        // where Motion::Down is a sentinel meaning "operate on N lines from cursor".
        // It is false for genuine motion commands (dj, >j, gUw, etc.).
        let is_linewise_self = linewise_self && matches!(motion, Motion::Down);

        if is_linewise_self {
            // dd, yy, cc operate on count lines starting from current line
            match operator {
                Operator::Delete => {
                    self.state
                        .undo_history
                        .begin_group(self.document.cursor, self.document.modified);
                    self.delete_lines_with_register(count, register)?;
                    self.state.undo_history.end_group(self.document.cursor);

                    if !self.state.insert_state.replaying_dot {
                        self.state.last_change = Some(RepeatableChange::OperatorMotion {
                            operator,
                            motion,
                            count,
                            register,
                            insert_keys: None,
                            linewise_self: true,
                        });
                    }
                }
                Operator::Yank => {
                    // Yank does not modify buffer, not repeatable
                    self.yank_lines(count, register)?;
                }
                Operator::Change => {
                    // cc: yank line(s), replace content with empty (or autoindent),
                    // then enter Insert mode. The line itself is NOT removed.
                    self.state
                        .undo_history
                        .begin_group(self.document.cursor, self.document.modified);
                    self.change_lines(count, register)?;
                    self.state.mode = Mode::Insert;
                    // Group will be ended when leaving insert mode

                    // Prepare dot recording
                    self.state.insert_state.insert_keys.clear();
                    self.state.insert_state.insert_entry_kind = None; // Not a pure insert session

                    if !self.state.insert_state.replaying_dot {
                        self.state.last_change = Some(RepeatableChange::OperatorMotion {
                            operator,
                            motion,
                            count,
                            register,
                            insert_keys: None,
                            linewise_self: true,
                        });
                    }
                }
                Operator::IndentRight => {
                    self.state
                        .undo_history
                        .begin_group(self.document.cursor, self.document.modified);
                    self.shift_lines_right(count)?;
                    self.state.undo_history.end_group(self.document.cursor);

                    if !self.state.insert_state.replaying_dot {
                        self.state.last_change = Some(RepeatableChange::OperatorMotion {
                            operator,
                            motion,
                            count,
                            register,
                            insert_keys: None,
                            linewise_self: true,
                        });
                    }
                }
                Operator::IndentLeft => {
                    self.state
                        .undo_history
                        .begin_group(self.document.cursor, self.document.modified);
                    self.shift_lines_left(count)?;
                    self.state.undo_history.end_group(self.document.cursor);
                    if !self.state.insert_state.replaying_dot {
                        self.state.last_change = Some(RepeatableChange::OperatorMotion {
                            operator,
                            motion,
                            count,
                            register,
                            insert_keys: None,
                            linewise_self: true,
                        });
                    }
                }
                Operator::Uppercase | Operator::Lowercase | Operator::ToggleCase => {
                    debug_assert!(count >= 1, "count must be >= 1 for line-wise case operator");
                    self.state
                        .undo_history
                        .begin_group(self.document.cursor, self.document.modified);
                    self.apply_case_operator_lines(
                        self.document.cursor.row,
                        self.document.cursor.row + count.saturating_sub(1),
                        operator,
                    )?;
                    self.document.modified = true;
                    self.state.undo_history.end_group(self.document.cursor);
                    if !self.state.insert_state.replaying_dot {
                        self.state.last_change = Some(RepeatableChange::OperatorMotion {
                            operator,
                            motion,
                            count,
                            register,
                            insert_keys: None,
                            linewise_self: true,
                        });
                    }
                }
                Operator::Format => {
                    let width = self.effective_textwidth();
                    if width > 0 {
                        let row_start = self.document.cursor.row;
                        let row_end = (row_start + count.saturating_sub(1))
                            .min(self.document.buffer.len().saturating_sub(1));
                        self.state
                            .undo_history
                            .begin_group(self.document.cursor, self.document.modified);
                        self.reflow_lines(row_start, row_end, width)?;
                        self.document.modified = true;
                        self.state.undo_history.end_group(self.document.cursor);
                        if !self.state.insert_state.replaying_dot {
                            self.state.last_change = Some(RepeatableChange::OperatorMotion {
                                operator,
                                motion,
                                count,
                                register,
                                insert_keys: None,
                                linewise_self: true,
                            });
                        }
                    }
                }
                Operator::Filter => {
                    // !! — filter count lines through external command
                    let start = self.document.cursor.row;
                    let end = (start + count - 1).min(self.document.buffer.len().saturating_sub(1));
                    self.state.pending_filter_range = Some((start, end));
                    // Enter command-line mode with ! prefix for shell command
                    self.state.mode = Mode::CommandLine;
                    self.state.command_line = crate::command::CommandLineState::new('!');
                }
            }
        } else {
            // Screen position motions (H/M/L) need viewport data — handle separately.
            if let Some(target_row) = self.screen_motion_target_row(motion) {
                let start = self.document.cursor.row.min(target_row);
                let end = self.document.cursor.row.max(target_row);
                let start_cursor = Cursor::new(start, 0);
                let end_cursor = Cursor::new(end, 0);
                match operator {
                    Operator::Delete => {
                        self.state
                            .undo_history
                            .begin_group(self.document.cursor, self.document.modified);
                        self.delete_motion_range_with_register(
                            &start_cursor,
                            &end_cursor,
                            register,
                            ContentType::Linewise,
                        )?;
                        self.state.undo_history.end_group(self.document.cursor);
                        if !self.state.insert_state.replaying_dot {
                            self.state.last_change = Some(RepeatableChange::OperatorMotion {
                                operator,
                                motion,
                                count,
                                register,
                                insert_keys: None,
                                linewise_self: false,
                            });
                        }
                    }
                    Operator::Yank => {
                        self.yank_range(
                            &start_cursor,
                            &end_cursor,
                            register,
                            ContentType::Linewise,
                        )?;
                    }
                    _ => {}
                }
                return Ok(());
            }

            // `cw` and `cW` act like `ce`/`cE`: delete to end of word rather
            // than to the start of the next word (vi compatibility).
            let motion = if operator == Operator::Change {
                match motion {
                    Motion::WordForward => Motion::WordEnd,
                    Motion::WORDForward => Motion::WORDEnd,
                    other => other,
                }
            } else {
                motion
            };

            // Execute the motion to get the range
            let ctx = CommandContext {
                buffer: &self.document.buffer,
                cursor: self.document.cursor,
                paragraphs: &self.state.settings.paragraphs,
                sections: &self.state.settings.sections,
                tabstop: self.state.settings.tabstop,
            };

            let last_find = self.state.parse_state.last_find();

            if let Some(result) = execute_motion(motion, &ctx, count, last_find) {
                // Determine content type based on motion kind
                let content_type = if result.kind == MotionKind::Linewise {
                    ContentType::Linewise
                } else {
                    ContentType::Characterwise
                };

                // For inclusive motions, extend the end position by one grapheme
                // since delete_range uses exclusive semantics
                let (start, end) = if result.kind == MotionKind::Inclusive {
                    // Extend end by one grapheme to include the character at the end position
                    if let Some(line) = self.document.buffer.line(result.range.end.row) {
                        let extended_col = crate::buffer::unicode::next_grapheme_boundary(
                            line,
                            result.range.end.col,
                        );
                        let extended_end = Cursor::new(result.range.end.row, extended_col);
                        (result.range.start, extended_end)
                    } else {
                        (result.range.start, result.range.end)
                    }
                } else {
                    (result.range.start, result.range.end)
                };

                match operator {
                    Operator::Delete => {
                        self.state
                            .undo_history
                            .begin_group(self.document.cursor, self.document.modified);
                        self.delete_motion_range_with_register(
                            &start,
                            &end,
                            register,
                            content_type,
                        )?;
                        self.state.undo_history.end_group(self.document.cursor);

                        if !self.state.insert_state.replaying_dot {
                            self.state.last_change = Some(RepeatableChange::OperatorMotion {
                                operator,
                                motion,
                                count,
                                register,
                                insert_keys: None,
                                linewise_self: false,
                            });
                        }
                    }
                    Operator::Yank => {
                        // Yank does not modify buffer, not repeatable
                        self.yank_range(&start, &end, register, content_type)?;
                    }
                    Operator::Change => {
                        // Change starts group that spans delete + insert mode
                        self.state
                            .undo_history
                            .begin_group(self.document.cursor, self.document.modified);
                        self.delete_motion_range_with_register(
                            &start,
                            &end,
                            register,
                            content_type,
                        )?;
                        self.state.mode = Mode::Insert;
                        // Group will be ended when leaving insert mode

                        // Prepare dot recording
                        self.state.insert_state.insert_keys.clear();
                        self.state.insert_state.insert_entry_kind = None;

                        if !self.state.insert_state.replaying_dot {
                            self.state.last_change = Some(RepeatableChange::OperatorMotion {
                                operator,
                                motion,
                                count,
                                register,
                                insert_keys: None,
                                linewise_self: false,
                            });
                        }
                    }
                    Operator::IndentRight => {
                        let row_start = start.row.min(end.row);
                        let row_end = start.row.max(end.row);
                        let line_count = row_end - row_start + 1;
                        self.state
                            .undo_history
                            .begin_group(self.document.cursor, self.document.modified);
                        self.document.cursor.row = row_start;
                        self.shift_lines_right(line_count)?;
                        self.state.undo_history.end_group(self.document.cursor);
                        if !self.state.insert_state.replaying_dot {
                            self.state.last_change = Some(RepeatableChange::OperatorMotion {
                                operator,
                                motion,
                                count,
                                register,
                                insert_keys: None,
                                linewise_self: false,
                            });
                        }
                    }
                    Operator::IndentLeft => {
                        let row_start = start.row.min(end.row);
                        let row_end = start.row.max(end.row);
                        let line_count = row_end - row_start + 1;
                        self.state
                            .undo_history
                            .begin_group(self.document.cursor, self.document.modified);
                        self.document.cursor.row = row_start;
                        self.shift_lines_left(line_count)?;
                        self.state.undo_history.end_group(self.document.cursor);
                        if !self.state.insert_state.replaying_dot {
                            self.state.last_change = Some(RepeatableChange::OperatorMotion {
                                operator,
                                motion,
                                count,
                                register,
                                insert_keys: None,
                                linewise_self: false,
                            });
                        }
                    }
                    Operator::Uppercase | Operator::Lowercase | Operator::ToggleCase => {
                        self.state
                            .undo_history
                            .begin_group(self.document.cursor, self.document.modified);
                        if result.kind == MotionKind::Linewise {
                            let row_start = start.row.min(end.row);
                            let row_end = start.row.max(end.row);
                            self.apply_case_operator_lines(row_start, row_end, operator)?;
                        } else {
                            self.apply_case_operator_range(&start, &end, operator)?;
                        }
                        self.document.cursor = start;
                        self.clamp_cursor_to_line();
                        self.state.undo_history.end_group(self.document.cursor);
                        self.document.modified = true;
                        if !self.state.insert_state.replaying_dot {
                            self.state.last_change = Some(RepeatableChange::OperatorMotion {
                                operator,
                                motion,
                                count,
                                register,
                                insert_keys: None,
                                linewise_self: false,
                            });
                        }
                    }
                    Operator::Format => {
                        let width = self.effective_textwidth();
                        if width > 0 {
                            let row_start = start.row.min(end.row);
                            let row_end = start.row.max(end.row);
                            self.state
                                .undo_history
                                .begin_group(self.document.cursor, self.document.modified);
                            self.document.cursor.row = row_start;
                            self.reflow_lines(row_start, row_end, width)?;
                            self.document.modified = true;
                            self.state.undo_history.end_group(self.document.cursor);
                            if !self.state.insert_state.replaying_dot {
                                self.state.last_change = Some(RepeatableChange::OperatorMotion {
                                    operator,
                                    motion,
                                    count,
                                    register,
                                    insert_keys: None,
                                    linewise_self: false,
                                });
                            }
                        }
                    }
                    Operator::Filter => {
                        // !{motion} — filter lines covered by motion
                        let row_start = start.row.min(end.row);
                        let row_end = start.row.max(end.row);
                        self.state.pending_filter_range = Some((row_start, row_end));
                        self.state.mode = Mode::CommandLine;
                        self.state.command_line = crate::command::CommandLineState::new('!');
                    }
                }
            }
        }

        Ok(())
    }

    /// Execute an operator with a text object.
    ///
    /// Records the command as `last_change` for dot repeat.
    pub(super) fn execute_operator_with_text_object(
        &mut self,
        operator: Operator,
        kind: crate::command::TextObjectKind,
        object: crate::command::TextObject,
        count: usize,
        register: Option<char>,
    ) -> Result<(), Error> {
        use crate::command::{resolve_text_object, TextObjectContext};

        let ctx = TextObjectContext {
            buffer: &self.document.buffer,
            cursor: self.document.cursor,
        };

        // Resolve the text object to get the affected range
        if let Some(range) = resolve_text_object(&ctx, kind, object, count) {
            // Text objects are always characterwise regardless of range span
            let content_type = ContentType::Characterwise;

            match operator {
                Operator::Delete => {
                    self.state
                        .undo_history
                        .begin_group(self.document.cursor, self.document.modified);
                    self.delete_motion_range_with_register(
                        &range.start,
                        &range.end,
                        register,
                        content_type,
                    )?;
                    self.state.undo_history.end_group(self.document.cursor);

                    if !self.state.insert_state.replaying_dot {
                        self.state.last_change = Some(RepeatableChange::OperatorTextObject {
                            operator,
                            kind,
                            object,
                            count,
                            register,
                            insert_keys: None,
                        });
                    }
                }
                Operator::Yank => {
                    // Yank does not modify buffer, not repeatable
                    self.yank_range(&range.start, &range.end, register, content_type)?;
                }
                Operator::Change => {
                    self.state
                        .undo_history
                        .begin_group(self.document.cursor, self.document.modified);
                    self.delete_motion_range_with_register(
                        &range.start,
                        &range.end,
                        register,
                        content_type,
                    )?;
                    self.state.mode = Mode::Insert;
                    // Group will be ended when leaving insert mode

                    // Prepare dot recording
                    self.state.insert_state.insert_keys.clear();
                    self.state.insert_state.insert_entry_kind = None;

                    if !self.state.insert_state.replaying_dot {
                        self.state.last_change = Some(RepeatableChange::OperatorTextObject {
                            operator,
                            kind,
                            object,
                            count,
                            register,
                            insert_keys: None,
                        });
                    }
                }
                Operator::IndentRight => {
                    let row_start = range.start.row.min(range.end.row);
                    let row_end = range.start.row.max(range.end.row);
                    let line_count = row_end - row_start + 1;
                    self.state
                        .undo_history
                        .begin_group(self.document.cursor, self.document.modified);
                    self.document.cursor.row = row_start;
                    self.shift_lines_right(line_count)?;
                    self.state.undo_history.end_group(self.document.cursor);
                    if !self.state.insert_state.replaying_dot {
                        self.state.last_change = Some(RepeatableChange::OperatorTextObject {
                            operator,
                            kind,
                            object,
                            count,
                            register,
                            insert_keys: None,
                        });
                    }
                }
                Operator::IndentLeft => {
                    let row_start = range.start.row.min(range.end.row);
                    let row_end = range.start.row.max(range.end.row);
                    let line_count = row_end - row_start + 1;
                    self.state
                        .undo_history
                        .begin_group(self.document.cursor, self.document.modified);
                    self.document.cursor.row = row_start;
                    self.shift_lines_left(line_count)?;
                    self.state.undo_history.end_group(self.document.cursor);
                    if !self.state.insert_state.replaying_dot {
                        self.state.last_change = Some(RepeatableChange::OperatorTextObject {
                            operator,
                            kind,
                            object,
                            count,
                            register,
                            insert_keys: None,
                        });
                    }
                }
                Operator::Uppercase | Operator::Lowercase | Operator::ToggleCase => {
                    self.state
                        .undo_history
                        .begin_group(self.document.cursor, self.document.modified);
                    if content_type == ContentType::Linewise {
                        let row_start = range.start.row.min(range.end.row);
                        let row_end = range.start.row.max(range.end.row);
                        self.apply_case_operator_lines(row_start, row_end, operator)?;
                    } else {
                        self.apply_case_operator_range(&range.start, &range.end, operator)?;
                    }
                    self.document.cursor = range.start;
                    self.clamp_cursor_to_line();
                    self.state.undo_history.end_group(self.document.cursor);
                    self.document.modified = true;
                    if !self.state.insert_state.replaying_dot {
                        self.state.last_change = Some(RepeatableChange::OperatorTextObject {
                            operator,
                            kind,
                            object,
                            count,
                            register,
                            insert_keys: None,
                        });
                    }
                }
                Operator::Format => {
                    let width = self.effective_textwidth();
                    if width > 0 {
                        let row_start = range.start.row.min(range.end.row);
                        let row_end = range.start.row.max(range.end.row);
                        self.state
                            .undo_history
                            .begin_group(self.document.cursor, self.document.modified);
                        self.reflow_lines(row_start, row_end, width)?;
                        self.document.modified = true;
                        self.state.undo_history.end_group(self.document.cursor);
                        if !self.state.insert_state.replaying_dot {
                            self.state.last_change = Some(RepeatableChange::OperatorTextObject {
                                operator,
                                kind,
                                object,
                                count,
                                register,
                                insert_keys: None,
                            });
                        }
                    }
                }
                _ => {
                    // Other operators not yet implemented
                }
            }
        }

        Ok(())
    }

    /// Replay the last buffer-modifying command (dot command).
    ///
    /// If `count` is `Some(n)`, the new count replaces the original count
    /// stored in the command. If `None`, the original count is used.
    /// Each replay creates its own undo group. The `replaying_dot` flag
    /// prevents recording during replay to avoid overwriting the stored command.
    pub(super) fn execute_repeat(&mut self, count: Option<usize>) -> Result<(), Error> {
        let change = match &self.state.last_change {
            Some(c) => c.clone(),
            None => return Ok(()), // Nothing to repeat
        };

        self.state.insert_state.replaying_dot = true;

        let result = match change {
            RepeatableChange::SimpleAction {
                action,
                register,
                count: orig_count,
            } => {
                let effective_count = count.unwrap_or(orig_count);
                self.execute_simple_action(action, register, effective_count)
            }
            RepeatableChange::OperatorMotion {
                operator,
                motion,
                count: orig_count,
                register,
                insert_keys,
                linewise_self,
            } => {
                let effective_count = count.unwrap_or(orig_count);
                let mut res = self.execute_operator_with_motion(
                    operator,
                    motion,
                    effective_count,
                    register,
                    linewise_self,
                );
                if res.is_ok() {
                    if let Some(keys) = &insert_keys {
                        for key in keys {
                            res = self.handle_insert_key(key.clone());
                            if res.is_err() {
                                break;
                            }
                        }
                        // Exit insert mode
                        if res.is_ok() {
                            res = self.handle_insert_key(Key::Esc);
                        }
                    }
                }
                res
            }
            RepeatableChange::OperatorTextObject {
                operator,
                kind,
                object,
                count: orig_count,
                register,
                insert_keys,
            } => {
                let effective_count = count.unwrap_or(orig_count);
                let mut res = self.execute_operator_with_text_object(
                    operator,
                    kind,
                    object,
                    effective_count,
                    register,
                );
                if res.is_ok() {
                    if let Some(keys) = &insert_keys {
                        for key in keys {
                            res = self.handle_insert_key(key.clone());
                            if res.is_err() {
                                break;
                            }
                        }
                        if res.is_ok() {
                            res = self.handle_insert_key(Key::Esc);
                        }
                    }
                }
                res
            }
            RepeatableChange::InsertSession {
                entry_kind,
                typed_keys,
                count: orig_count,
            } => {
                // A dot-command count overrides the original count; otherwise use the
                // count that was active when insert mode was entered (e.g. 3 from 3ifoo<Esc>).
                let repeat_count = count.unwrap_or(orig_count);
                let is_replace = entry_kind == ModeChangeKind::ReplaceMode;
                // Enter insert mode once, replay the typed keys `repeat_count` times, then Esc.
                let mut res = self.execute_mode_change(entry_kind);
                if res.is_ok() {
                    for _ in 0..repeat_count {
                        for key in &typed_keys {
                            res = if is_replace {
                                self.handle_replace_key(key.clone())
                            } else {
                                self.handle_insert_key(key.clone())
                            };
                            if res.is_err() {
                                break;
                            }
                        }
                        if res.is_err() {
                            break;
                        }
                    }
                }
                if res.is_ok() {
                    res = if is_replace {
                        self.handle_replace_key(Key::Esc)
                    } else {
                        self.handle_insert_key(Key::Esc)
                    };
                }
                res
            }
        };

        self.state.insert_state.replaying_dot = false;
        result
    }
}

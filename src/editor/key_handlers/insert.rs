//! Insert mode and replace mode key handlers.

use super::super::Editor;
use crate::buffer::unicode::next_grapheme_boundary;
use crate::command::RepeatableChange;
use crate::error::Error;
use crate::mode::Mode;
use crate::registers::RegisterId;
use crate::terminal::Key;
use crate::undo::EditAction;

impl Editor {
    /// Replace the character at cursor with the given character.
    pub(super) fn replace_char_at_cursor(&mut self, ch: char) -> Result<(), Error> {
        // Capture old character for undo before deletion
        if let Some(line) = self.document.buffer.line(self.document.cursor.row) {
            if self.document.cursor.col < line.len() {
                let next_col = next_grapheme_boundary(line, self.document.cursor.col);
                // Get the old character for the ReplaceChar action
                if let Some(old_char) = line[self.document.cursor.col..next_col].chars().next() {
                    let action = EditAction::ReplaceChar {
                        row: self.document.cursor.row,
                        col: self.document.cursor.col,
                        old_char,
                        new_char: ch,
                    };
                    self.state.undo_history.record(action);
                }

                let start = self.document.cursor;
                let end = crate::buffer::Cursor::new(self.document.cursor.row, next_col);
                self.document.buffer.delete_range(&start, &end)?;
            }
        }

        // Re-fetch cursor position after delete
        let cursor = self.document.cursor;

        // Insert the replacement character
        self.document.buffer.insert_char(&cursor, ch)?;

        // Don't move cursor forward (r replaces in place)
        self.document.modified = true;
        Ok(())
    }

    /// Handle key press in insert mode.
    ///
    /// When exiting insert mode (Esc or Ctrl-C), the undo group that was
    /// started when entering insert mode is finalized. Insert mode keystrokes
    /// are recorded into `insert_keys` for dot repeat.
    pub(super) fn handle_insert_key(&mut self, key: Key) -> Result<(), Error> {
        // Ctrl-r register-next: insert contents of named register.
        if self.state.insert_state.insert_register_next {
            self.state.insert_state.insert_register_next = false;
            if let Key::Char(c) = key {
                let id = RegisterId::parse(c);
                if let Some(content) = self.state.registers.get(id) {
                    let text = content.text().to_string();
                    for ch in text.chars() {
                        self.insert_char(ch)?;
                    }
                }
            }
            return Ok(());
        }

        // Ctrl-v literal-next: insert the following character verbatim.
        if self.state.insert_state.insert_literal_next {
            self.state.insert_state.insert_literal_next = false;
            let ch = match key {
                Key::Char(c) => Some(c),
                Key::Ctrl(c) => {
                    // Map Ctrl-X to the actual control character (0x00–0x1F)
                    let cp = c as u32;
                    // Ctrl-@ = 0x00, Ctrl-A = 0x01 .. Ctrl-Z = 0x1A
                    if cp <= 0x7F {
                        char::from_u32(cp & 0x1F)
                    } else {
                        None
                    }
                }
                // Esc inserts ESC (0x1B) literally
                Key::Esc => char::from_u32(0x1B),
                _ => None,
            };
            if let Some(c) = ch {
                self.state.insert_state.maybe_record_key(Key::Char(c));
                self.insert_char(c)?;
            }
            return Ok(());
        }

        match key {
            // Ctrl-@ (NUL): re-insert text from last insert session and exit insert mode.
            //
            // Replays the typed_keys from the most recent InsertSession stored in
            // last_change, then exits insert mode. If no prior insert exists, just
            // exits (matching vi behavior of treating Ctrl-@ as a no-op insert + Esc).
            Key::Ctrl('@') => {
                // Collect the keys to replay without holding a borrow on self.
                let keys_to_replay: Vec<Key> = match &self.state.last_change {
                    Some(RepeatableChange::InsertSession { typed_keys, .. }) => typed_keys.clone(),
                    _ => Vec::new(),
                };
                // Replay into the current insert session (do not update last_change).
                let old_replaying = self.state.insert_state.replaying_dot;
                self.state.insert_state.replaying_dot = true;
                for k in keys_to_replay {
                    self.handle_insert_key(k)?;
                }
                self.state.insert_state.replaying_dot = old_replaying;
                // Expand any pending abbreviation before leaving insert mode.
                self.try_expand_abbreviation()?;
                // Exit insert mode (mirrors the Esc handler, but does not update last_change).
                self.state.undo_history.end_group(self.document.cursor);
                self.state.mode = Mode::Normal;
                if self.document.cursor.col > 0 {
                    self.move_left();
                }
                self.state.insert_state.saved_indent = None;
                self.state.insert_state.insert_entry_kind = None;
            }

            // Exit insert mode
            Key::Esc => {
                // Expand any pending abbreviation before leaving insert mode.
                self.try_expand_abbreviation()?;

                // If the session was entered with a count > 1 (e.g. 3ifoo<Esc>),
                // replay the typed keys count-1 more times before exiting.
                let repeat = self.state.insert_state.insert_count.max(1);
                if repeat > 1 && !self.state.insert_state.replaying_dot {
                    let keys = self.state.insert_state.insert_keys.clone();
                    for _ in 1..repeat {
                        for key in &keys {
                            self.handle_insert_key(key.clone())?;
                        }
                    }
                }

                // Visual block `I`: replay typed keys on remaining rows in the block.
                if let Some((start_row, end_row, col)) = self.state.insert_state.block_insert.take()
                {
                    let keys = self.state.insert_state.insert_keys.clone();
                    if !keys.is_empty() {
                        let old_replaying = self.state.insert_state.replaying_dot;
                        self.state.insert_state.replaying_dot = true;
                        for row in (start_row + 1)..=end_row {
                            let clamped = self
                                .document
                                .buffer
                                .line(row)
                                .map(|l| col.min(l.len()))
                                .unwrap_or(0);
                            self.document.cursor = crate::buffer::Cursor::new(row, clamped);
                            for key in &keys {
                                self.handle_insert_key(key.clone())?;
                            }
                        }
                        self.state.insert_state.replaying_dot = old_replaying;
                    }
                }

                // End the undo group that was started when entering insert mode
                self.state.undo_history.end_group(self.document.cursor);
                self.state.mode = Mode::Normal;
                // Move cursor back one position (vi behavior)
                if self.document.cursor.col > 0 {
                    self.move_left();
                }

                // Finalize dot recording for insert mode exit
                if !self.state.insert_state.replaying_dot {
                    self.finalize_insert_for_dot();
                }
                self.state.insert_state.insert_count = 1;
                self.state.insert_state.saved_indent = None;
                self.state.insert_state.insert_entry_kind = None;
            }
            Key::Ctrl('c') => {
                // Expand any pending abbreviation before leaving insert mode.
                self.try_expand_abbreviation()?;
                self.state.undo_history.end_group(self.document.cursor);
                self.state.mode = Mode::Normal;
                if self.document.cursor.col > 0 {
                    self.move_left();
                }

                // Finalize dot recording
                if !self.state.insert_state.replaying_dot {
                    self.finalize_insert_for_dot();
                }
                self.state.insert_state.saved_indent = None;
                self.state.insert_state.insert_entry_kind = None;
            }

            // Character input
            Key::Char(c) => {
                // Non-keyword characters (space, punctuation, etc.) trigger
                // abbreviation expansion on the word just before the cursor.
                if !c.is_alphanumeric() && c != '_' {
                    self.try_expand_abbreviation()?;
                }
                // Record for dot repeat (up to MAX_INSERT_KEYS limit)
                self.state.insert_state.maybe_record_key(Key::Char(c));
                self.insert_char(c)?;
                self.apply_wrapmargin()?;
            }

            // Tab key - insert tab or spaces based on expandtab setting
            Key::Tab => {
                // Record for dot repeat
                self.state.insert_state.maybe_record_key(Key::Tab);
                if self.state.settings.expandtab {
                    // Insert spaces to reach the next tab stop (vim behavior)
                    let line = self
                        .document
                        .buffer
                        .line(self.document.cursor.row)
                        .unwrap_or("");
                    let prefix = &line[..self.document.cursor.col.min(line.len())];
                    let current_col =
                        crate::buffer::unicode::display_width(prefix, self.state.settings.tabstop);
                    let spaces_to_next_tab = crate::buffer::unicode::tab_width_at_col(
                        current_col,
                        self.state.settings.tabstop,
                    );
                    for _ in 0..spaces_to_next_tab {
                        self.insert_char(' ')?;
                    }
                } else {
                    // Insert literal tab character
                    self.insert_char('\t')?;
                }
            }

            // Enter - split line
            Key::Enter => {
                // Expand any pending abbreviation before the newline.
                self.try_expand_abbreviation()?;
                self.state.insert_state.maybe_record_key(Key::Enter);
                self.insert_newline()?;
            }

            // Backspace / Ctrl-H - delete character before cursor
            Key::Backspace | Key::Ctrl('h') => {
                self.state.insert_state.maybe_record_key(Key::Backspace);
                self.delete_char_before_cursor()?;
            }

            // Delete key - delete character at cursor
            Key::Delete => {
                self.state.insert_state.maybe_record_key(Key::Delete);
                self.delete_char_at_cursor_with_register(None)?;
            }

            // Arrow keys work in insert mode too
            Key::Left => {
                self.state.insert_state.maybe_record_key(Key::Left);
                self.move_left();
            }
            Key::Right => {
                self.state.insert_state.maybe_record_key(Key::Right);
                self.move_right();
            }
            Key::Up => {
                self.state.insert_state.maybe_record_key(Key::Up);
                self.move_up();
            }
            Key::Down => {
                self.state.insert_state.maybe_record_key(Key::Down);
                self.move_down();
            }

            // Ctrl-w: delete word before cursor (unix-style)
            Key::Ctrl('w') => {
                self.state.insert_state.maybe_record_key(Key::Ctrl('w'));
                self.delete_word_before_cursor()?;
            }

            // Ctrl-u: delete to beginning of line
            Key::Ctrl('u') => {
                self.state.insert_state.maybe_record_key(Key::Ctrl('u'));
                self.delete_to_line_start()?;
            }

            // Ctrl-v: insert next character literally
            Key::Ctrl('v') => {
                self.state.insert_state.insert_literal_next = true;
            }

            // Ctrl-r: insert register contents (next key is register name)
            Key::Ctrl('r') => {
                self.state.insert_state.insert_register_next = true;
            }

            // Ctrl-t: indent current line by shiftwidth
            Key::Ctrl('t') => {
                self.state.insert_state.maybe_record_key(Key::Ctrl('t'));
                let row = self.document.cursor.row;
                let col_before = self.document.cursor.col;
                let len_before = self.document.buffer.line(row).map_or(0, |l| l.len());
                self.shift_lines_right(1)?;
                let len_after = self.document.buffer.line(row).map_or(0, |l| l.len());
                let added = len_after.saturating_sub(len_before);
                self.document.cursor.col = (col_before + added).min(len_after);
            }

            // Ctrl-d: dedent current line by shiftwidth.
            //
            // Special prefixes (checked by inspecting the character just before
            // the cursor on the current line):
            //   `0 Ctrl-D` — delete the `0`, then delete ALL leading whitespace.
            //                 Resets the autoindent level (next Enter inherits no indent).
            //   `^ Ctrl-D` — delete the `^`, then delete ALL leading whitespace.
            //                 Preserves the indent level for the next line (saves the
            //                 current indent in `state.insert_state.saved_indent`).
            //   plain Ctrl-D — dedent by one shiftwidth (existing behaviour).
            Key::Ctrl('d') => {
                let row = self.document.cursor.row;
                let col = self.document.cursor.col;

                // Peek at the character immediately before the cursor.
                let prefix_char = if col > 0 {
                    self.document
                        .buffer
                        .line(row)
                        .and_then(|l| l[..col].chars().next_back())
                } else {
                    None
                };

                match prefix_char {
                    Some('0') | Some('^') => {
                        let is_caret = prefix_char == Some('^');

                        // Record Ctrl-D for dot repeat. The prefix char ('0' or '^')
                        // was already recorded by the Key::Char arm when it was typed.
                        self.state.insert_state.maybe_record_key(Key::Ctrl('d'));

                        // Delete the prefix character (`0` or `^`).
                        self.delete_char_before_cursor()?;

                        // Delete all leading whitespace from the line.
                        let row = self.document.cursor.row;
                        let old_content = self.document.buffer.line(row).unwrap_or("").to_string();

                        // For `^ Ctrl-D`: save the indent we're about to delete so
                        // that the next Enter inherits it (regardless of autoindent).
                        if is_caret {
                            let saved = super::super::editing::get_leading_whitespace(&old_content)
                                .to_string();
                            self.state.insert_state.saved_indent = Some(saved);
                        }

                        let new_content: String = old_content
                            .chars()
                            .skip_while(|c| c.is_ascii_whitespace())
                            .collect();
                        if new_content != old_content {
                            let action = crate::undo::EditAction::ReplaceLine {
                                row,
                                old_content: old_content.clone(),
                                new_content: new_content.clone(),
                            };
                            self.state.undo_history.record(action);
                            if let Some(line) = self.document.buffer.line_mut(row) {
                                *line = new_content;
                            }
                            self.document.cursor.col = 0;
                            self.document.modified = true;
                        }
                    }
                    _ => {
                        // Plain Ctrl-D: dedent by one shiftwidth.
                        self.state.insert_state.maybe_record_key(Key::Ctrl('d'));
                        let col_before = self.document.cursor.col;
                        let len_before = self.document.buffer.line(row).map_or(0, |l| l.len());
                        self.shift_lines_left(1)?;
                        let len_after = self.document.buffer.line(row).map_or(0, |l| l.len());
                        let removed = len_before.saturating_sub(len_after);
                        self.document.cursor.col = col_before.saturating_sub(removed);
                    }
                }
            }

            _ => {}
        }
        Ok(())
    }

    /// Finalize insert mode recording for dot repeat.
    ///
    /// Called when leaving insert mode (Esc/Ctrl-C). Determines whether
    /// the insert session should be stored as a standalone `InsertSession`
    /// or if the typed keys should be attached to a pending
    /// `OperatorMotion`/`OperatorTextObject` Change command.
    pub(super) fn finalize_insert_for_dot(&mut self) {
        let typed_keys = std::mem::take(&mut self.state.insert_state.insert_keys);

        if let Some(entry_kind) = self.state.insert_state.insert_entry_kind {
            // Pure insert session (entered via i, a, o, O, A, I)
            let count = self.state.insert_state.insert_count.max(1);
            self.state.last_change = Some(RepeatableChange::InsertSession {
                entry_kind,
                typed_keys,
                count,
            });
        } else {
            // Insert mode was entered via a Change operator (cw, cc, C, etc.)
            // Attach the typed keys to the existing last_change.
            if let Some(ref mut change) = self.state.last_change {
                match change {
                    RepeatableChange::OperatorMotion {
                        ref mut insert_keys,
                        ..
                    } => {
                        *insert_keys = Some(typed_keys);
                    }
                    RepeatableChange::OperatorTextObject {
                        ref mut insert_keys,
                        ..
                    } => {
                        *insert_keys = Some(typed_keys);
                    }
                    _ => {
                        // Unexpected variant; do not overwrite
                    }
                }
            }
        }
    }

    // =========================================================================
    // Replace mode methods
    // =========================================================================

    /// Handle key press in replace mode.
    ///
    /// Typed characters overwrite existing text one-for-one. At end-of-line,
    /// characters are appended. Backspace restores the previously overwritten
    /// character. Esc/Ctrl-C exit to Normal mode. Enter splits the line.
    pub(super) fn handle_replace_key(&mut self, key: Key) -> Result<(), Error> {
        match key {
            Key::Esc => {
                self.state.undo_history.end_group(self.document.cursor);
                self.state.mode = Mode::Normal;
                if self.document.cursor.col > 0 {
                    self.move_left();
                }
                if !self.state.insert_state.replaying_dot {
                    self.finalize_insert_for_dot();
                }
                self.state.insert_state.insert_entry_kind = None;
                self.state.insert_state.replace_originals.clear();
            }
            Key::Ctrl('c') => {
                self.state.undo_history.end_group(self.document.cursor);
                self.state.mode = Mode::Normal;
                if self.document.cursor.col > 0 {
                    self.move_left();
                }
                if !self.state.insert_state.replaying_dot {
                    self.finalize_insert_for_dot();
                }
                self.state.insert_state.insert_entry_kind = None;
                self.state.insert_state.replace_originals.clear();
            }

            Key::Char(c) => {
                self.state.insert_state.maybe_record_key(Key::Char(c));

                let line_len = self
                    .document
                    .buffer
                    .line(self.document.cursor.row)
                    .map(|l| l.len())
                    .unwrap_or(0);

                if self.document.cursor.col < line_len {
                    // Overwrite: capture original grapheme before replacing
                    let orig =
                        if let Some(line) = self.document.buffer.line(self.document.cursor.row) {
                            let next_col = next_grapheme_boundary(line, self.document.cursor.col);
                            Some(line[self.document.cursor.col..next_col].to_string())
                        } else {
                            None
                        };
                    self.state.insert_state.replace_originals.push(orig);
                    self.replace_char_at_cursor(c)?;
                    // Advance cursor right by the width of the replacement char
                    let new_col = self.document.cursor.col + c.len_utf8();
                    self.document.cursor.col = new_col;
                } else {
                    // At or past end-of-line: append
                    self.state.insert_state.replace_originals.push(None);
                    self.insert_char(c)?;
                }
            }

            Key::Backspace => {
                self.state.insert_state.maybe_record_key(Key::Backspace);

                if let Some(entry) = self.state.insert_state.replace_originals.pop() {
                    match entry {
                        Some(orig) => {
                            // Move cursor left to the replaced character's position
                            self.move_left();
                            let col = self.document.cursor.col;
                            let row = self.document.cursor.row;
                            // Restore by replacing the replacement char with the original
                            // grapheme. Record as ReplaceChar (mirroring replace_char_at_cursor)
                            // so the undo log stays symmetric and `u` reverses correctly.
                            if let Some(line) = self.document.buffer.line(row) {
                                if col < line.len() {
                                    let next_col = next_grapheme_boundary(line, col);
                                    // Capture the replacement char that's currently at cursor
                                    let replacement_first =
                                        line[col..next_col].chars().next().unwrap_or('\0');
                                    let orig_first = orig.chars().next().unwrap_or('\0');
                                    // Record as ReplaceChar so undo reverses it cleanly
                                    let action = EditAction::ReplaceChar {
                                        row,
                                        col,
                                        old_char: replacement_first,
                                        new_char: orig_first,
                                    };
                                    self.state.undo_history.record(action);
                                    // Delete the replacement grapheme
                                    let start = self.document.cursor;
                                    let end = crate::buffer::Cursor::new(row, next_col);
                                    self.document.buffer.delete_range(&start, &end)?;
                                    // Insert the original grapheme
                                    let cursor = self.document.cursor;
                                    for ch in orig.chars() {
                                        self.document.buffer.insert_char(&cursor, ch)?;
                                        self.document.cursor.col += ch.len_utf8();
                                    }
                                    self.document.cursor.col = col;
                                    self.document.modified = true;
                                }
                            }
                        }
                        None => {
                            // Was an appended char; just delete it
                            self.delete_char_before_cursor()?;
                        }
                    }
                }
                // If replace_originals is empty, we're at the entry point — do nothing
            }

            Key::Enter => {
                self.state.insert_state.maybe_record_key(Key::Enter);
                // Push None so Backspace doesn't pop the previous char's entry
                self.state.insert_state.replace_originals.push(None);
                self.insert_newline()?;
            }

            Key::Left => {
                self.state.insert_state.maybe_record_key(Key::Left);
                self.move_left();
            }
            Key::Right => {
                self.state.insert_state.maybe_record_key(Key::Right);
                self.move_right();
            }
            Key::Up => {
                self.state.insert_state.maybe_record_key(Key::Up);
                self.move_up();
            }
            Key::Down => {
                self.state.insert_state.maybe_record_key(Key::Down);
                self.move_down();
            }

            _ => {}
        }
        Ok(())
    }

    /// Apply `wrapmargin` after a character was inserted in insert mode.
    ///
    /// When `wrapmargin` is non-zero, the right-margin threshold is
    /// `terminal_width - wrapmargin`. If the cursor's display column has
    /// reached or passed that threshold, scan backward through the current
    /// line to find the last space before the threshold and break the line
    /// there (equivalent to pressing Enter at that space). If no space is
    /// found before the threshold, do nothing (vi behavior: don't break a
    /// word in the middle).
    fn apply_wrapmargin(&mut self) -> Result<(), Error> {
        let wm = self.state.settings.wrapmargin;
        if wm == 0 {
            return Ok(());
        }

        let term_width = self.state.viewport.width();
        if term_width == 0 || wm >= term_width {
            return Ok(());
        }
        let threshold = term_width - wm; // display column at which we must wrap

        let row = self.document.cursor.row;
        let line = match self.document.buffer.line(row) {
            Some(l) => l.to_string(),
            None => return Ok(()),
        };

        // Compute display column of cursor
        let col = self.document.cursor.col.min(line.len());
        let cursor_display =
            crate::buffer::unicode::display_width(&line[..col], self.state.settings.tabstop);

        if cursor_display < threshold {
            return Ok(());
        }

        // Find the last space before the threshold in the line
        // Walk through the line tracking display column, record the byte
        // offset of each space that is before the threshold.
        let mut last_space_byte: Option<usize> = None;
        let mut display_col = 0usize;
        let mut byte_pos = 0usize;
        for ch in line.chars() {
            if display_col >= threshold {
                break;
            }
            if ch == ' ' {
                last_space_byte = Some(byte_pos);
            }
            let w = if ch == '\t' {
                crate::buffer::unicode::tab_width_at_col(display_col, self.state.settings.tabstop)
            } else {
                crate::buffer::unicode::display_width(&ch.to_string(), self.state.settings.tabstop)
            };
            display_col += w;
            byte_pos += ch.len_utf8();
        }

        let break_at = match last_space_byte {
            Some(b) => b,
            None => return Ok(()), // no space found; don't break mid-word
        };

        // Remember where the cursor is in the portion after the break
        let cursor_byte_after_break = self.document.cursor.col.saturating_sub(break_at + 1);

        // Move cursor to the space position and replace space with newline
        self.document.cursor.col = break_at;
        // Delete the space
        let old_line = self.document.buffer.line(row).unwrap_or("").to_string();
        let new_line = format!("{}{}", &old_line[..break_at], &old_line[break_at + 1..]);
        let action = crate::undo::EditAction::ReplaceLine {
            row,
            old_content: old_line,
            new_content: new_line.clone(),
        };
        self.state.undo_history.record(action);
        if let Some(l) = self.document.buffer.line_mut(row) {
            l.clear();
            l.push_str(&new_line);
        }

        // Now insert a newline at break_at (cursor is already there)
        self.insert_newline()?;

        // Restore cursor to where it was in the text after the break
        self.document.cursor.col = cursor_byte_after_break;

        Ok(())
    }
}

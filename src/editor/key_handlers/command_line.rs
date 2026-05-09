//! Command-line mode handler and search-related methods.

use super::super::Editor;
use crate::buffer::unicode::is_word_char;
use crate::buffer::Cursor;
use crate::command::{parse_ex_command, CommandLineState};
use crate::error::Error;
use crate::mode::Mode;
use crate::search::{find_next, SearchDirection};
use crate::terminal::Key;

impl Editor {
    /// Handle key press in command-line mode.
    ///
    /// Processes input for the ':', '/', and '?' command lines. Characters
    /// are inserted into the command-line buffer, Enter executes the command
    /// or search, Escape cancels, and Backspace on an empty buffer cancels.
    pub(super) fn handle_command_line_key(&mut self, key: Key) -> Result<(), Error> {
        match key {
            Key::Esc | Key::Ctrl('c') => {
                // Cancel command line, return to normal mode.
                // Restore search state, cursor, and viewport to where they were
                // before the user started typing the search pattern.
                if let Some(saved) = self.state.incsearch_saved.take() {
                    self.state.search = saved.search;
                    self.document.cursor = saved.cursor;
                    self.state.viewport = saved.viewport;
                }
                self.state.pending_filter_range = None;
                self.state.command_line = CommandLineState::default();
                self.state.mode = Mode::Normal;
            }
            Key::Enter => {
                let prompt = self.state.command_line.prompt();
                let input = self.state.command_line.take_buffer();
                // Commit: discard saved state (the live state is now the committed state)
                self.state.incsearch_saved = None;
                self.state.mode = Mode::Normal;

                match prompt {
                    ':' => {
                        // Ex command — build full context (marks, buffer lines, last pattern)
                        let ctx = self.make_ex_context();
                        match parse_ex_command(&input, &ctx) {
                            Ok(cmd) => {
                                self.execute_ex_command(cmd);
                            }
                            Err(msg) => {
                                if !msg.is_empty() {
                                    self.state.status_message =
                                        Some(super::super::StatusMessage::Error(msg));
                                }
                            }
                        }
                    }
                    '/' => {
                        // Forward search
                        self.execute_search(&input, SearchDirection::Forward);
                    }
                    '?' => {
                        // Backward search
                        self.execute_search(&input, SearchDirection::Backward);
                    }
                    '!' => {
                        // Filter command: use pending_filter_range
                        if let Some((start, end)) = self.state.pending_filter_range.take() {
                            if !input.is_empty() {
                                self.execute_filter(start, end, &input);
                            }
                        }
                    }
                    _ => {
                        // Unknown prompt, ignore
                    }
                }
            }
            Key::Backspace | Key::Ctrl('h') => {
                if !self.state.command_line.delete_char_before_cursor() {
                    // Buffer was empty and backspace hit - cancel command line.
                    // Restore search state, cursor, and viewport to pre-search position.
                    if let Some(saved) = self.state.incsearch_saved.take() {
                        self.state.search = saved.search;
                        self.document.cursor = saved.cursor;
                        self.state.viewport = saved.viewport;
                    }
                    self.state.command_line = CommandLineState::default();
                    self.state.mode = Mode::Normal;
                } else {
                    self.update_incsearch();
                }
            }
            Key::Char(c) => {
                self.state.command_line.insert_char(c);
                self.update_incsearch();
            }
            Key::Left => {
                self.state.command_line.move_left();
            }
            Key::Right => {
                self.state.command_line.move_right();
            }
            Key::Home => {
                self.state.command_line.move_to_start();
            }
            Key::End => {
                self.state.command_line.move_to_end();
            }
            _ => {
                // Ignore other keys in command line mode
            }
        }
        Ok(())
    }

    /// Update the live search state for incremental search.
    ///
    /// Called on every character insertion or deletion in a `/` or `?`
    /// command line when `incsearch` is enabled. Compiles the current
    /// command-line buffer as a search pattern, updates `self.state.search`
    /// so that `hlsearch` highlights refresh on the next render, and jumps
    /// the cursor to the first match (searching from the saved pre-search
    /// cursor position). If the buffer is empty or the pattern fails to
    /// compile, restores the saved state (search, cursor, viewport) so no
    /// stale highlights or cursor drift remain.
    pub(super) fn update_incsearch(&mut self) {
        let prompt = self.state.command_line.prompt();
        if !self.state.settings.incsearch || (prompt != '/' && prompt != '?') {
            return;
        }

        let pattern = self.state.command_line.buffer().to_string();
        let direction = if prompt == '/' {
            SearchDirection::Forward
        } else {
            SearchDirection::Backward
        };

        // Empty pattern or compile failure: restore pre-search position
        if pattern.is_empty() {
            self.restore_incsearch_position();
            return;
        }

        let case_insensitive = self.state.settings.ignorecase;
        let magic = self.state.settings.magic;
        if self
            .state
            .search
            .set_pattern_with_magic(&pattern, direction, case_insensitive, magic)
            .is_err()
        {
            self.restore_incsearch_position();
            return;
        }

        // Jump cursor to the first match from the saved pre-search position
        if let Some(saved) = &self.state.incsearch_saved {
            let origin = saved.cursor;
            if let Some(regex) = self.state.search.compiled() {
                let result = find_next(
                    &self.document.buffer,
                    &origin,
                    regex,
                    direction,
                    self.state.settings.wrapscan,
                );
                if let Ok(Some((m, _))) = result {
                    self.document.cursor = Cursor::new(m.row, m.col_start);
                } else {
                    // No match: return cursor to pre-search position
                    self.document.cursor = origin;
                    self.state.viewport = saved.viewport.clone();
                }
            }
        }
    }

    /// Restore cursor, viewport, and search state to the snapshot taken
    /// when the user entered the `/` or `?` command line.
    pub(super) fn restore_incsearch_position(&mut self) {
        if let Some(saved) = &self.state.incsearch_saved {
            self.state.search = saved.search.clone();
            self.document.cursor = saved.cursor;
            self.state.viewport = saved.viewport.clone();
        }
    }

    /// Compile a search pattern, store it in SearchState, and jump to the first match.
    ///
    /// If the pattern is empty, repeats the last search in the given direction.
    /// When the `ignorecase` setting is enabled, the pattern is compiled with
    /// case-insensitive matching.
    /// Displays status messages for errors and wrap-around notifications.
    pub(super) fn execute_search(&mut self, pattern: &str, direction: SearchDirection) {
        // Clear :nohl suppression — any new search re-enables highlights
        self.state.search.suppress_highlights = false;
        // Record position before search jump
        self.push_jump();
        if pattern.is_empty() {
            // Empty pattern: repeat last search in the given direction
            if self.state.search.compiled().is_some() {
                // Update direction only without recompiling
                self.state.search.set_direction(direction);
                self.jump_to_next_match(direction);
            } else {
                self.state.status_message = Some(super::super::StatusMessage::Error(
                    "E486: Pattern not found: ".to_string(),
                ));
            }
            return;
        }

        // Compile the new pattern, respecting ignorecase and magic settings
        let case_insensitive = self.state.settings.ignorecase;
        let magic = self.state.settings.magic;
        match self
            .state
            .search
            .set_pattern_with_magic(pattern, direction, case_insensitive, magic)
        {
            Ok(()) => {
                self.jump_to_next_match(direction);
            }
            Err(e) => {
                self.state.status_message =
                    Some(super::super::StatusMessage::Error(format!("E486: {}", e)));
            }
        }
    }

    /// Repeat the last search. If `reverse` is true, search in the opposite direction.
    ///
    /// Note: Uses the compiled regex from the original search, so changes to the
    /// `ignorecase` setting after the initial search will not take effect until
    /// the next explicit `/` or `?` search. This matches traditional vi behavior.
    pub(super) fn execute_search_repeat(&mut self, reverse: bool) -> Result<(), Error> {
        // Clear :nohl suppression
        self.state.search.suppress_highlights = false;
        // Record position before search jump
        self.push_jump();
        let direction = match self.state.search.direction() {
            Some(d) => {
                if reverse {
                    d.reversed()
                } else {
                    d
                }
            }
            None => {
                self.state.status_message = Some(super::super::StatusMessage::Error(
                    "E486: Pattern not found: ".to_string(),
                ));
                return Ok(());
            }
        };

        self.jump_to_next_match(direction);
        Ok(())
    }

    /// Jump to the next match using the compiled regex in SearchState.
    ///
    /// Moves the cursor to the match position and displays wrap-around
    /// or "not found" messages in the status line.
    pub(super) fn jump_to_next_match(&mut self, direction: SearchDirection) {
        // Perform search by temporarily accessing both state and document
        // We can't hold a borrow to state while borrowing document mutably,
        // so clone the regex pattern and re-borrow within a limited scope.
        let result = {
            let Some(regex) = self.state.search.compiled() else {
                return;
            };
            find_next(
                &self.document.buffer,
                &self.document.cursor,
                regex,
                direction,
                self.state.settings.wrapscan,
            )
        };

        match result {
            Ok(Some((m, wrapped))) => {
                self.document.cursor = Cursor::new(m.row, m.col_start);
                self.state
                    .viewport
                    .ensure_visible(self.document.cursor.row, self.document.buffer.len());

                if wrapped {
                    let msg = match direction {
                        SearchDirection::Forward => "search hit BOTTOM, continuing at TOP",
                        SearchDirection::Backward => "search hit TOP, continuing at BOTTOM",
                    };
                    self.state.status_message =
                        Some(super::super::StatusMessage::Info(msg.to_string()));
                }
            }
            Ok(None) => {
                let pattern = self.state.search.last_pattern().unwrap_or("").to_string();
                let msg = if !self.state.settings.wrapscan {
                    match direction {
                        SearchDirection::Forward => {
                            format!("E384: search hit BOTTOM without match for: {}", pattern)
                        }
                        SearchDirection::Backward => {
                            format!("E385: search hit TOP without match for: {}", pattern)
                        }
                    }
                } else {
                    format!("E486: Pattern not found: {}", pattern)
                };
                self.state.status_message = Some(super::super::StatusMessage::Error(msg));
            }
            Err(e) => {
                self.state.status_message =
                    Some(super::super::StatusMessage::Error(format!("E486: {}", e)));
            }
        }
    }

    /// Search for the word under the cursor.
    ///
    /// Extracts the word at the cursor position, builds a whole-word bounded
    /// pattern (\\<word\\>), sets it in SearchState, and jumps to the next
    /// match. Shows an error if the cursor is not on a word character.
    pub(super) fn search_word_under_cursor(
        &mut self,
        direction: SearchDirection,
    ) -> Result<(), Error> {
        // Clear :nohl suppression
        self.state.search.suppress_highlights = false;

        let (word, _word_start) = match self.extract_word_at_cursor() {
            Some(w) => w,
            None => {
                self.state.status_message = Some(super::super::StatusMessage::Error(
                    "E348: No string under cursor".to_string(),
                ));
                return Ok(());
            }
        };

        // Build whole-word bounded pattern: \<word\>
        // Word chars (alphanumeric + underscore via is_word_char) are all
        // regex-safe, so no escaping of the word content is needed.
        let pattern = format!("\\<{}\\>", word);

        // Compile and set the pattern
        let case_insensitive = self.state.settings.ignorecase;
        let magic = self.state.settings.magic;
        match self
            .state
            .search
            .set_pattern_with_magic(&pattern, direction, case_insensitive, magic)
        {
            Ok(()) => {
                self.jump_to_next_match(direction);
            }
            Err(e) => {
                self.state.status_message =
                    Some(super::super::StatusMessage::Error(format!("E486: {}", e)));
            }
        }
        Ok(())
    }

    /// Extract the word at the current cursor position.
    ///
    /// Returns the word string and its starting byte offset within the line.
    /// A word consists of alphanumeric characters and underscores (vi word chars).
    /// Returns `None` if the cursor is not on a word character.
    pub(super) fn extract_word_at_cursor(&self) -> Option<(String, usize)> {
        let line = self.document.buffer.line(self.document.cursor.row)?;
        let col = self.document.cursor.col;

        if col >= line.len() {
            return None;
        }

        // Check if cursor is on a word character
        let ch = line[col..].chars().next()?;
        if !is_word_char(ch) {
            return None;
        }

        // Find word start (scan backward)
        let start = line[..col]
            .char_indices()
            .rev()
            .take_while(|(_, c)| is_word_char(*c))
            .last()
            .map(|(i, _)| i)
            .unwrap_or(col);

        // Find word end (scan forward)
        let end = line[col..]
            .char_indices()
            .take_while(|(_, c)| is_word_char(*c))
            .last()
            .map(|(i, c)| col + i + c.len_utf8())
            .unwrap_or(col);

        if start < end {
            Some((line[start..end].to_string(), start))
        } else {
            None
        }
    }
}

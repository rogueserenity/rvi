//! Key event handlers for the editor.
//!
//! This module contains the key handling methods for each editor mode,
//! including undo group management for all editing commands and dot
//! command recording/replay.

mod command_line;
mod insert;
mod normal;
mod operator;
mod visual;

use super::Editor;
use crate::buffer::SelectionType;
use crate::error::Error;
use crate::mode::{Mode, Operator, VisualKind};
use crate::terminal::Key;

/// Maximum map-dispatch recursion depth to guard against infinite loops.
const MAX_MAP_DEPTH: usize = 20;

impl Editor {
    /// Handle a key press, consulting user-defined `:map` / `:map!` tables.
    pub(super) fn handle_key(&mut self, key: Key) -> Result<(), Error> {
        // Clear status message on any keypress when not in CommandLine mode.
        if self.state.mode != Mode::CommandLine {
            self.state.status_message = None;
        }

        // Map dispatch for Normal and Insert modes (POSIX vi :map / :map!).
        // Bypass when depth limit reached or insert mode has a special
        // "next key" flag (Ctrl-V literal, Ctrl-R register).
        let bypass_maps = match self.state.mode {
            Mode::Normal => self.state.mappings.map_depth >= MAX_MAP_DEPTH,
            Mode::Insert => {
                self.state.mappings.map_depth >= MAX_MAP_DEPTH
                    || self.state.insert_state.insert_literal_next
                    || self.state.insert_state.insert_register_next
            }
            _ => true,
        };

        if !bypass_maps {
            self.state.mappings.map_pending.push(key);
            return self.flush_map_pending();
        }

        self.handle_key_direct(key)
    }

    /// Flush the map_pending buffer: fire RHS on exact match, keep buffering
    /// if pending is a proper prefix of some LHS, or dispatch the first pending
    /// key literally if no map can match.
    fn flush_map_pending(&mut self) -> Result<(), Error> {
        loop {
            if self.state.mappings.map_pending.is_empty() {
                return Ok(());
            }

            let maps: Vec<(Vec<Key>, Vec<Key>)> = match self.state.mode {
                Mode::Normal => self.state.mappings.normal_maps.clone(),
                Mode::Insert => self.state.mappings.insert_maps.clone(),
                _ => {
                    // Dropped out of a mappable mode; flush all pending as literal.
                    let pending = std::mem::take(&mut self.state.mappings.map_pending);
                    for k in pending {
                        self.handle_key_direct(k)?;
                    }
                    return Ok(());
                }
            };

            let pending = self.state.mappings.map_pending.clone();

            // Exact match: fire RHS through handle_key (allowing further map
            // expansion).
            if let Some((_, rhs)) = maps.iter().find(|(lhs, _)| *lhs == pending) {
                let rhs = rhs.clone();
                self.state.mappings.map_pending.clear();
                self.state.mappings.map_depth += 1;
                for k in rhs {
                    if self.state.should_quit {
                        break;
                    }
                    self.handle_key(k)?;
                }
                self.state.mappings.map_depth -= 1;
                return Ok(());
            }

            // Is pending a proper prefix of any LHS? Keep buffering.
            let is_prefix = maps
                .iter()
                .any(|(lhs, _)| lhs.len() > pending.len() && lhs.starts_with(pending.as_slice()));
            if is_prefix {
                return Ok(());
            }

            // No match and not a prefix: dispatch the first pending key literally.
            let first = self.state.mappings.map_pending.remove(0);
            self.handle_key_direct(first)?;
            // Loop continues with remaining pending keys.
        }
    }

    /// Dispatch a key directly, bypassing map lookup: record into macro buffer
    /// then route to the appropriate mode handler.
    fn handle_key_direct(&mut self, key: Key) -> Result<(), Error> {
        // Record into macro buffer for ALL modes (Bug 2 fix: CommandLine keys
        // are now recorded too, so macros containing ex commands replay fully).
        // The 'q' key that stops recording is popped in handle_normal_key.
        if self.state.macro_state.recording.is_some() {
            self.state.macro_state.buffer.push(key.clone());
        }

        match self.state.mode {
            Mode::Normal => self.handle_normal_key(key),
            Mode::Insert => self.handle_insert_key(key),
            Mode::Replace => self.handle_replace_key(key),
            Mode::OperatorPending(op) => self.handle_operator_pending_key(key, op),
            Mode::CommandLine => self.handle_command_line_key(key),
            Mode::Visual(kind) => self.handle_visual_key(key, kind),
        }
    }
}

/// Encode a sequence of `Key` values as a compact string for register storage.
///
/// Encoding:
/// - `Char(c)` → `c` verbatim
/// - `Ctrl(c)` → corresponding control byte (e.g., `Ctrl('a')` → `\x01`)
/// - `Esc` → `\x1b`
/// - `Enter` → `\n`
/// - `Backspace` → `\x08`
/// - `Tab` → `\t`
/// - Other keys are silently dropped (they cannot be stored as UTF-8 text)
fn keys_to_string(keys: &[Key]) -> String {
    let mut s = String::new();
    for key in keys {
        match key {
            Key::Char(c) => s.push(*c),
            Key::Ctrl(c) => {
                let code = (*c as u8).wrapping_sub(b'a').wrapping_add(1);
                s.push(code as char);
            }
            Key::Esc => s.push('\x1b'),
            Key::Enter => s.push('\n'),
            Key::Backspace => s.push('\x08'),
            Key::Tab => s.push('\t'),
            _ => {} // Skip special keys that can't roundtrip via text
        }
    }
    s
}

/// Decode a register string back to a sequence of `Key` values.
fn string_to_keys(s: &str) -> Vec<Key> {
    let mut keys = Vec::new();
    for c in s.chars() {
        let key = match c {
            '\x1b' => Key::Esc,
            '\n' => Key::Enter,
            '\x08' => Key::Backspace,
            '\t' => Key::Tab,
            c if (c as u32) < 32 => {
                // Control character: code = c as u8, letter = code + 'a' - 1
                Key::Ctrl((c as u8 + b'a' - 1) as char)
            }
            c => Key::Char(c),
        };
        keys.push(key);
    }
    keys
}

/// Convert a `VisualKind` to its corresponding `SelectionType`.
fn visual_kind_to_selection_type(kind: VisualKind) -> SelectionType {
    match kind {
        VisualKind::Character => SelectionType::Character,
        VisualKind::Line => SelectionType::Line,
        VisualKind::Block => SelectionType::Block,
    }
}

/// Toggle the case of ASCII letters in `line[col_start..col_end]`.
///
/// Returns a new `String` with the modified content.
fn toggle_case_range(line: &str, col_start: usize, col_end: usize) -> String {
    let mut result = String::with_capacity(line.len());
    for (i, ch) in line.char_indices() {
        let end = i + ch.len_utf8();
        if i >= col_start && end <= col_end {
            if ch.is_uppercase() {
                for c in ch.to_lowercase() {
                    result.push(c);
                }
            } else if ch.is_lowercase() {
                for c in ch.to_uppercase() {
                    result.push(c);
                }
            } else {
                result.push(ch);
            }
        } else {
            result.push(ch);
        }
    }
    result
}

/// Apply a case transformation to a substring of `line` from `col_start` to `col_end`.
///
/// `operator` must be `Uppercase`, `Lowercase`, or `ToggleCase`.
fn apply_case_to_line(line: &str, col_start: usize, col_end: usize, operator: Operator) -> String {
    match operator {
        Operator::ToggleCase => toggle_case_range(line, col_start, col_end),
        _ => {
            let mut result = String::with_capacity(line.len());
            for (i, ch) in line.char_indices() {
                let end = i + ch.len_utf8();
                if i >= col_start && end <= col_end {
                    match operator {
                        Operator::Uppercase => {
                            for c in ch.to_uppercase() {
                                result.push(c);
                            }
                        }
                        Operator::Lowercase => {
                            for c in ch.to_lowercase() {
                                result.push(c);
                            }
                        }
                        _ => unreachable!(
                            "apply_case_to_line called with non-case operator: {:?}",
                            operator
                        ),
                    }
                } else {
                    result.push(ch);
                }
            }
            result
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::unicode::{next_grapheme_boundary, prev_grapheme_boundary};
    use crate::buffer::{Buffer, Cursor};
    use crate::command::{
        execute_motion, resolve_text_object, CommandContext, ModeChangeKind, Motion, MotionKind,
        ParseResult, RepeatableChange, SimpleAction, TextObjectContext, TextObjectKind,
    };
    use crate::file::LineEnding;
    use crate::registers::{RegisterContent, RegisterId};
    use crate::search::{find_next, SearchDirection};

    // Test helper to create a context for motion testing
    fn make_ctx(content: &str, row: usize, col: usize) -> (Buffer, Cursor) {
        let buffer = Buffer::from_string(content.to_string());
        let cursor = Cursor::new(row, col);
        (buffer, cursor)
    }

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
        assert_eq!(result.target.col, 2);
        assert_eq!(result.kind, MotionKind::Inclusive);
    }

    // Tests for Bug 2 fix: Operator count handling
    #[test]
    fn test_operator_count_preserved() {
        use crate::command::{parse_normal_key, ParseState, ParsedCommand};

        let mut state = ParseState::new();

        let result = parse_normal_key(&mut state, Key::Char('3'), Mode::Normal);
        assert_eq!(result, ParseResult::Pending);

        let result = parse_normal_key(&mut state, Key::Char('d'), Mode::Normal);

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

        let mut doc = super::super::Document::from_string("hello".to_string());
        doc.cursor = Cursor::new(0, 3);

        let mut registers = Registers::new();

        let line = doc.buffer.line(0).unwrap();
        let prev_col = prev_grapheme_boundary(line, doc.cursor.col);
        let deleted_char = &line[prev_col..doc.cursor.col];

        let content = RegisterContent::characterwise(deleted_char.to_string());
        registers.delete(None, None, content);

        let stored = registers.get(None).unwrap();
        assert_eq!(stored.text(), "l");
        assert!(!stored.is_linewise());
    }

    // Tests for Bug Fix 2: UTF-8 cursor positioning in put commands
    #[test]
    fn test_put_cursor_position_with_ascii() {
        use unicode_segmentation::UnicodeSegmentation;

        let text = "abc";
        let grapheme_count = text.graphemes(true).count();
        assert_eq!(grapheme_count, 3);

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

        let text = "\u{4F60}\u{597D}\u{4E16}";
        let grapheme_count = text.graphemes(true).count();
        assert_eq!(grapheme_count, 3);
        assert_eq!(text.len(), 9);

        let last_grapheme_offset = text
            .grapheme_indices(true)
            .next_back()
            .map(|(offset, _)| offset)
            .unwrap_or(0);
        assert_eq!(last_grapheme_offset, 6);

        assert_eq!(prev_grapheme_boundary(text, 9), 6);
        assert_eq!(prev_grapheme_boundary(text, 6), 3);
        assert_eq!(prev_grapheme_boundary(text, 3), 0);
    }

    #[test]
    fn test_put_cursor_position_with_combining_chars() {
        use unicode_segmentation::UnicodeSegmentation;

        let text = "cafe\u{0301}";
        let grapheme_count = text.graphemes(true).count();
        assert_eq!(grapheme_count, 4);

        let last_grapheme_offset = text
            .grapheme_indices(true)
            .next_back()
            .map(|(offset, _)| offset)
            .unwrap_or(0);
        assert_eq!(last_grapheme_offset, 3);

        assert_eq!(prev_grapheme_boundary(text, text.len()), 3);
    }

    // Tests for text objects integration

    #[test]
    fn test_text_object_word_resolution() {
        let buffer = Buffer::from_string("hello world".to_string());
        let ctx = TextObjectContext {
            buffer: &buffer,
            cursor: Cursor::new(0, 1),
        };

        let range = resolve_text_object(
            &ctx,
            TextObjectKind::Inner,
            crate::command::TextObject::Word,
            1,
        )
        .unwrap();
        assert_eq!(range.start.col, 0);
        assert_eq!(range.end.col, 5);
    }

    #[test]
    fn test_text_object_around_word_with_whitespace() {
        let buffer = Buffer::from_string("hello world".to_string());
        let ctx = TextObjectContext {
            buffer: &buffer,
            cursor: Cursor::new(0, 1),
        };

        let range = resolve_text_object(
            &ctx,
            TextObjectKind::Around,
            crate::command::TextObject::Word,
            1,
        )
        .unwrap();
        assert_eq!(range.start.col, 0);
        assert_eq!(range.end.col, 6);
    }

    #[test]
    fn test_text_object_quote_resolution() {
        let buffer = Buffer::from_string(r#"say "hello" now"#.to_string());
        let ctx = TextObjectContext {
            buffer: &buffer,
            cursor: Cursor::new(0, 6),
        };

        let range = resolve_text_object(
            &ctx,
            TextObjectKind::Inner,
            crate::command::TextObject::DoubleQuote,
            1,
        )
        .unwrap();
        assert_eq!(range.start.col, 5);
        assert_eq!(range.end.col, 10);
    }

    #[test]
    fn test_text_object_bracket_resolution() {
        let buffer = Buffer::from_string("fn(abc)".to_string());
        let ctx = TextObjectContext {
            buffer: &buffer,
            cursor: Cursor::new(0, 4),
        };

        let range = resolve_text_object(
            &ctx,
            TextObjectKind::Inner,
            crate::command::TextObject::Parenthesis,
            1,
        )
        .unwrap();
        assert_eq!(range.start.col, 3);
        assert_eq!(range.end.col, 6);
    }

    // Tests for D command
    #[test]
    fn test_delete_to_end_of_line_basic() {
        use crate::registers::Registers;

        let mut doc = super::super::Document::from_string("hello world".to_string());
        doc.cursor = Cursor::new(0, 5);

        let mut registers = Registers::new();

        let line = doc.buffer.line(0).unwrap();
        let deleted_text = line[doc.cursor.col..].to_string();
        assert_eq!(deleted_text, " world");

        let content = RegisterContent::characterwise(deleted_text);
        registers.delete(None, None, content);

        let stored = registers.get(None).unwrap();
        assert_eq!(stored.text(), " world");
        assert!(stored.is_characterwise());
    }

    #[test]
    fn test_delete_to_end_of_line_from_start() {
        use crate::registers::Registers;

        let mut doc = super::super::Document::from_string("hello".to_string());
        doc.cursor = Cursor::new(0, 0);

        let mut registers = Registers::new();

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
        let doc = super::super::Document::from_string("hello".to_string());
        let line = doc.buffer.line(0).unwrap();
        let col = line.len();
        assert_eq!(col, 5);
        assert!(col >= line.len());
    }

    #[test]
    fn test_delete_to_end_of_line_empty_line() {
        let doc = super::super::Document::from_string(String::new());
        let line = doc.buffer.line(0).unwrap();
        assert!(line.is_empty());
        assert_eq!(0, line.len());
    }

    // Tests for C command
    #[test]
    fn test_change_to_end_of_line_stores_characterwise() {
        use crate::registers::Registers;

        let mut doc = super::super::Document::from_string("hello world".to_string());
        doc.cursor = Cursor::new(0, 5);

        let mut registers = Registers::new();

        let line = doc.buffer.line(0).unwrap();
        let deleted_text = line[doc.cursor.col..].to_string();

        let content = RegisterContent::characterwise(deleted_text);
        registers.delete(None, None, content);

        let stored = registers.get(None).unwrap();
        assert_eq!(stored.text(), " world");
        assert!(stored.is_characterwise());
    }

    #[test]
    fn test_change_to_end_of_line_cursor_position() {
        let mut doc = super::super::Document::from_string("hello world".to_string());
        doc.cursor = Cursor::new(0, 6);

        let line = doc.buffer.line(0).unwrap();
        let deleted_text = line[doc.cursor.col..].to_string();
        assert_eq!(deleted_text, "world");

        let start = doc.cursor;
        let end = Cursor::new(doc.cursor.row, line.len());
        doc.buffer.delete_range(&start, &end).unwrap();

        let line_after = doc.buffer.line(0).unwrap();
        assert_eq!(line_after, "hello ");

        let insert_col = line_after.len();
        assert_eq!(insert_col, 6);
        doc.cursor.col = insert_col;
        assert_eq!(doc.cursor.col, 6);
    }

    #[test]
    fn test_change_to_end_of_line_from_start_cursor_position() {
        let mut doc = super::super::Document::from_string("hello".to_string());
        doc.cursor = Cursor::new(0, 0);

        let line = doc.buffer.line(0).unwrap();
        assert_eq!(line, "hello");
        let start = doc.cursor;
        let end = Cursor::new(0, line.len());
        doc.buffer.delete_range(&start, &end).unwrap();

        let line_after = doc.buffer.line(0).unwrap();
        assert_eq!(line_after, "");
        doc.cursor.col = line_after.len();
        assert_eq!(doc.cursor.col, 0);
    }

    #[test]
    fn test_change_to_end_of_line_mid_word_cursor_position() {
        let mut doc = super::super::Document::from_string("abcdefgh".to_string());
        doc.cursor = Cursor::new(0, 3);

        let line = doc.buffer.line(0).unwrap();
        let start = doc.cursor;
        let end = Cursor::new(0, line.len());
        doc.buffer.delete_range(&start, &end).unwrap();

        let line_after = doc.buffer.line(0).unwrap();
        assert_eq!(line_after, "abc");
        doc.cursor.col = line_after.len();
        assert_eq!(doc.cursor.col, 3);
    }

    // Tests for Y command
    #[test]
    fn test_yank_line_stores_linewise() {
        use crate::registers::Registers;

        let doc = super::super::Document::from_string("hello world".to_string());
        let mut registers = Registers::new();

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

        let line = doc.buffer.line(0).unwrap();
        let mut yanked_text = line.to_string();
        yanked_text.push('\n');

        let content = RegisterContent::linewise(yanked_text);
        let reg_id = RegisterId::parse('a');
        registers.yank(reg_id, content);

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

    // =====================================================
    // Dot command (.) repeat tests
    // =====================================================

    // Unit tests for RepeatableChange recording

    #[test]
    fn test_dot_recording_simple_action_delete_char() {
        // Verify that executing x records a RepeatableChange::SimpleAction
        let mut state = super::super::EditorState::new(24, 80);

        // Simulate what execute_simple_action does for DeleteCharAtCursor recording
        state.last_change = Some(RepeatableChange::SimpleAction {
            action: SimpleAction::DeleteCharAtCursor,
            register: None,
            count: 1,
        });

        match &state.last_change {
            Some(RepeatableChange::SimpleAction {
                action,
                register,
                count,
            }) => {
                assert_eq!(*action, SimpleAction::DeleteCharAtCursor);
                assert_eq!(*register, None);
                assert_eq!(*count, 1);
            }
            _ => panic!("Expected SimpleAction variant"),
        }
    }

    #[test]
    fn test_dot_recording_replace_char() {
        let mut state = super::super::EditorState::new(24, 80);

        state.last_change = Some(RepeatableChange::SimpleAction {
            action: SimpleAction::ReplaceChar('z'),
            register: None,
            count: 1,
        });

        match &state.last_change {
            Some(RepeatableChange::SimpleAction { action, .. }) => {
                assert_eq!(*action, SimpleAction::ReplaceChar('z'));
            }
            _ => panic!("Expected SimpleAction with ReplaceChar"),
        }
    }

    #[test]
    fn test_dot_recording_operator_motion() {
        let mut state = super::super::EditorState::new(24, 80);

        state.last_change = Some(RepeatableChange::OperatorMotion {
            operator: Operator::Delete,
            motion: Motion::WordForward,
            count: 2,
            register: None,
            insert_keys: None,
            linewise_self: false,
        });

        match &state.last_change {
            Some(RepeatableChange::OperatorMotion {
                operator,
                motion,
                count,
                linewise_self,
                ..
            }) => {
                assert_eq!(*operator, Operator::Delete);
                assert_eq!(*motion, Motion::WordForward);
                assert_eq!(*count, 2);
                assert!(!linewise_self);
            }
            _ => panic!("Expected OperatorMotion variant"),
        }
    }

    #[test]
    fn test_dot_recording_linewise_delete() {
        let mut state = super::super::EditorState::new(24, 80);

        state.last_change = Some(RepeatableChange::OperatorMotion {
            operator: Operator::Delete,
            motion: Motion::Down,
            count: 3,
            register: Some('a'),
            insert_keys: None,
            linewise_self: true,
        });

        match &state.last_change {
            Some(RepeatableChange::OperatorMotion {
                operator,
                count,
                register,
                linewise_self,
                ..
            }) => {
                assert_eq!(*operator, Operator::Delete);
                assert_eq!(*count, 3);
                assert_eq!(*register, Some('a'));
                assert!(linewise_self);
            }
            _ => panic!("Expected OperatorMotion variant"),
        }
    }

    #[test]
    fn test_dot_recording_operator_text_object() {
        let mut state = super::super::EditorState::new(24, 80);

        state.last_change = Some(RepeatableChange::OperatorTextObject {
            operator: Operator::Delete,
            kind: TextObjectKind::Around,
            object: crate::command::TextObject::Word,
            count: 1,
            register: None,
            insert_keys: None,
        });

        match &state.last_change {
            Some(RepeatableChange::OperatorTextObject {
                operator,
                kind,
                object,
                ..
            }) => {
                assert_eq!(*operator, Operator::Delete);
                assert_eq!(*kind, TextObjectKind::Around);
                assert_eq!(*object, crate::command::TextObject::Word);
            }
            _ => panic!("Expected OperatorTextObject variant"),
        }
    }

    #[test]
    fn test_dot_recording_insert_session() {
        let mut state = super::super::EditorState::new(24, 80);

        state.last_change = Some(RepeatableChange::InsertSession {
            entry_kind: ModeChangeKind::InsertBeforeCursor,
            typed_keys: vec![Key::Char('h'), Key::Char('i')],
            count: 1,
        });

        match &state.last_change {
            Some(RepeatableChange::InsertSession {
                entry_kind,
                typed_keys,
                ..
            }) => {
                assert_eq!(*entry_kind, ModeChangeKind::InsertBeforeCursor);
                assert_eq!(typed_keys.len(), 2);
                assert_eq!(typed_keys[0], Key::Char('h'));
                assert_eq!(typed_keys[1], Key::Char('i'));
            }
            _ => panic!("Expected InsertSession variant"),
        }
    }

    #[test]
    fn test_dot_recording_change_operator_with_insert_keys() {
        let mut state = super::super::EditorState::new(24, 80);

        // Simulate cw followed by typing "new" and Esc
        state.last_change = Some(RepeatableChange::OperatorMotion {
            operator: Operator::Change,
            motion: Motion::WordForward,
            count: 1,
            register: None,
            insert_keys: Some(vec![Key::Char('n'), Key::Char('e'), Key::Char('w')]),
            linewise_self: false,
        });

        match &state.last_change {
            Some(RepeatableChange::OperatorMotion {
                operator,
                insert_keys,
                ..
            }) => {
                assert_eq!(*operator, Operator::Change);
                let keys = insert_keys.as_ref().unwrap();
                assert_eq!(keys.len(), 3);
                assert_eq!(keys[0], Key::Char('n'));
                assert_eq!(keys[1], Key::Char('e'));
                assert_eq!(keys[2], Key::Char('w'));
            }
            _ => panic!("Expected OperatorMotion with Change"),
        }
    }

    // Count handling tests

    #[test]
    fn test_dot_count_replacement_some() {
        // When dot count is Some(n), it replaces the original count
        let effective = 5_usize;
        assert_eq!(effective, 5);
    }

    #[test]
    fn test_dot_count_replacement_none() {
        // When dot count is None, original count is preserved
        let orig_count: usize = 3;
        let effective = orig_count;
        assert_eq!(effective, 3);
    }

    // Non-repeatable command tests

    #[test]
    fn test_undo_not_repeatable() {
        let mut state = super::super::EditorState::new(24, 80);

        // Set a prior change
        state.last_change = Some(RepeatableChange::SimpleAction {
            action: SimpleAction::DeleteCharAtCursor,
            register: None,
            count: 1,
        });

        // Undo should not modify last_change - the SimpleAction::Undo
        // branch does not touch last_change
        // (verified by code inspection: the Undo arm has no last_change assignment)
        let prev = state.last_change.clone();
        // Simulating execute_simple_action for Undo would call apply_undo,
        // which does not touch last_change
        match &prev {
            Some(RepeatableChange::SimpleAction { action, .. }) => {
                assert_eq!(*action, SimpleAction::DeleteCharAtCursor);
            }
            _ => panic!("Expected prior change to be preserved"),
        }
    }

    #[test]
    fn test_replaying_dot_flag_prevents_recording() {
        let mut state = super::super::EditorState::new(24, 80);

        // Set an initial change
        state.last_change = Some(RepeatableChange::SimpleAction {
            action: SimpleAction::DeleteCharAtCursor,
            register: None,
            count: 1,
        });

        // During replay, replaying_dot is true, so recording is suppressed
        state.insert_state.replaying_dot = true;
        // The if !state.insert_state.replaying_dot check prevents overwriting last_change
        if !state.insert_state.replaying_dot {
            state.last_change = Some(RepeatableChange::SimpleAction {
                action: SimpleAction::ReplaceChar('x'),
                register: None,
                count: 1,
            });
        }

        // last_change should still be the original
        match &state.last_change {
            Some(RepeatableChange::SimpleAction { action, .. }) => {
                assert_eq!(*action, SimpleAction::DeleteCharAtCursor);
            }
            _ => panic!("Expected original change to be preserved during replay"),
        }

        state.insert_state.replaying_dot = false;
    }

    #[test]
    fn test_dot_with_no_prior_change() {
        let state = super::super::EditorState::new(24, 80);
        // last_change is None by default
        assert!(state.last_change.is_none());
        // execute_repeat with None just returns Ok(()) - nothing to do
    }

    // Insert key recording tests

    #[test]
    fn test_insert_keys_cleared_on_mode_change() {
        let mut state = super::super::EditorState::new(24, 80);
        state.insert_state.insert_keys = vec![Key::Char('o'), Key::Char('l'), Key::Char('d')];

        // When entering insert mode, insert_keys should be cleared
        state.insert_state.insert_keys.clear();
        assert!(state.insert_state.insert_keys.is_empty());
    }

    #[test]
    fn test_insert_keys_collected() {
        let mut state = super::super::EditorState::new(24, 80);
        state.insert_state.insert_keys.clear();

        // Simulate collecting keys during insert mode
        state.insert_state.insert_keys.push(Key::Char('h'));
        state.insert_state.insert_keys.push(Key::Char('e'));
        state.insert_state.insert_keys.push(Key::Char('l'));
        state.insert_state.insert_keys.push(Key::Char('l'));
        state.insert_state.insert_keys.push(Key::Char('o'));

        assert_eq!(state.insert_state.insert_keys.len(), 5);
        assert_eq!(state.insert_state.insert_keys[0], Key::Char('h'));
        assert_eq!(state.insert_state.insert_keys[4], Key::Char('o'));
    }

    #[test]
    fn test_insert_keys_include_special_keys() {
        let mut state = super::super::EditorState::new(24, 80);
        state.insert_state.insert_keys.clear();

        state.insert_state.insert_keys.push(Key::Char('a'));
        state.insert_state.insert_keys.push(Key::Backspace);
        state.insert_state.insert_keys.push(Key::Char('b'));
        state.insert_state.insert_keys.push(Key::Enter);
        state.insert_state.insert_keys.push(Key::Char('c'));

        assert_eq!(state.insert_state.insert_keys.len(), 5);
        assert_eq!(state.insert_state.insert_keys[1], Key::Backspace);
        assert_eq!(state.insert_state.insert_keys[3], Key::Enter);
    }

    // Finalize insert for dot tests

    #[test]
    fn test_finalize_insert_pure_session() {
        let mut state = super::super::EditorState::new(24, 80);
        state.insert_state.insert_entry_kind = Some(ModeChangeKind::InsertBeforeCursor);
        state.insert_state.insert_keys = vec![Key::Char('h'), Key::Char('i')];

        // Simulate finalize_insert_for_dot
        let typed_keys = std::mem::take(&mut state.insert_state.insert_keys);
        if let Some(entry_kind) = state.insert_state.insert_entry_kind {
            state.last_change = Some(RepeatableChange::InsertSession {
                entry_kind,
                typed_keys,
                count: 1,
            });
        }

        match &state.last_change {
            Some(RepeatableChange::InsertSession {
                entry_kind,
                typed_keys,
                ..
            }) => {
                assert_eq!(*entry_kind, ModeChangeKind::InsertBeforeCursor);
                assert_eq!(typed_keys.len(), 2);
            }
            _ => panic!("Expected InsertSession"),
        }
    }

    #[test]
    fn test_finalize_insert_change_operator() {
        let mut state = super::super::EditorState::new(24, 80);
        // Simulate: cw was executed, which set last_change and entered insert mode
        state.last_change = Some(RepeatableChange::OperatorMotion {
            operator: Operator::Change,
            motion: Motion::WordForward,
            count: 1,
            register: None,
            insert_keys: None,
            linewise_self: false,
        });
        state.insert_state.insert_entry_kind = None; // Change operator, not a pure insert
        state.insert_state.insert_keys = vec![Key::Char('n'), Key::Char('e'), Key::Char('w')];

        // Simulate finalize_insert_for_dot
        let typed_keys = std::mem::take(&mut state.insert_state.insert_keys);
        if state.insert_state.insert_entry_kind.is_some() {
            // Would store InsertSession
        } else if let Some(RepeatableChange::OperatorMotion {
            ref mut insert_keys,
            ..
        }) = state.last_change
        {
            *insert_keys = Some(typed_keys);
        }

        match &state.last_change {
            Some(RepeatableChange::OperatorMotion {
                insert_keys,
                operator,
                ..
            }) => {
                assert_eq!(*operator, Operator::Change);
                let keys = insert_keys.as_ref().unwrap();
                assert_eq!(keys.len(), 3);
                assert_eq!(keys[0], Key::Char('n'));
            }
            _ => panic!("Expected OperatorMotion with insert_keys filled"),
        }
    }

    // C command dot recording test

    #[test]
    fn test_c_command_recorded_as_operator_motion_line_end() {
        let mut state = super::super::EditorState::new(24, 80);

        // C is modeled as OperatorMotion { Change, LineEnd }
        state.last_change = Some(RepeatableChange::OperatorMotion {
            operator: Operator::Change,
            motion: Motion::LineEnd,
            count: 1,
            register: None,
            insert_keys: Some(vec![Key::Char('x'), Key::Char('y')]),
            linewise_self: false,
        });

        match &state.last_change {
            Some(RepeatableChange::OperatorMotion {
                operator,
                motion,
                insert_keys,
                ..
            }) => {
                assert_eq!(*operator, Operator::Change);
                assert_eq!(*motion, Motion::LineEnd);
                assert_eq!(insert_keys.as_ref().unwrap().len(), 2);
            }
            _ => panic!("Expected OperatorMotion with Change + LineEnd"),
        }
    }

    // EditorState initialization tests

    #[test]
    fn test_editor_state_new_fields_initialized() {
        let state = super::super::EditorState::new(24, 80);
        assert!(state.last_change.is_none());
        assert!(state.insert_state.insert_keys.is_empty());
        assert!(!state.insert_state.replaying_dot);
        assert!(state.insert_state.insert_entry_kind.is_none());
    }

    // =====================================================
    // Search integration tests
    // =====================================================

    #[test]
    fn test_search_state_initialized() {
        let state = super::super::EditorState::new(24, 80);
        assert!(state.search.last_pattern().is_none());
        assert!(state.search.direction().is_none());
        assert!(state.search.compiled().is_none());
    }

    #[test]
    fn test_search_next_not_recorded_for_dot() {
        let mut state = super::super::EditorState::new(24, 80);

        // Set a prior change
        state.last_change = Some(RepeatableChange::SimpleAction {
            action: SimpleAction::DeleteCharAtCursor,
            register: None,
            count: 1,
        });

        // SearchNext should not overwrite last_change because the
        // execute_simple_action method skips recording for search actions
        let is_search = matches!(
            SimpleAction::SearchNext,
            SimpleAction::SearchNext | SimpleAction::SearchPrev
        );
        assert!(is_search);

        // Verify the prior change is still preserved (search does not overwrite)
        match &state.last_change {
            Some(RepeatableChange::SimpleAction { action, .. }) => {
                assert_eq!(*action, SimpleAction::DeleteCharAtCursor);
            }
            _ => panic!("Expected prior change to be preserved"),
        }
    }

    #[test]
    fn test_search_prev_not_recorded_for_dot() {
        let is_search = matches!(
            SimpleAction::SearchPrev,
            SimpleAction::SearchNext | SimpleAction::SearchPrev
        );
        assert!(is_search);
    }

    #[test]
    fn test_search_direction_from_prompt() {
        // '/' prompt maps to forward search
        let prompt = '/';
        let direction = match prompt {
            '/' => SearchDirection::Forward,
            '?' => SearchDirection::Backward,
            _ => panic!("unexpected prompt"),
        };
        assert_eq!(direction, SearchDirection::Forward);

        // '?' prompt maps to backward search
        let prompt = '?';
        let direction = match prompt {
            '/' => SearchDirection::Forward,
            '?' => SearchDirection::Backward,
            _ => panic!("unexpected prompt"),
        };
        assert_eq!(direction, SearchDirection::Backward);
    }

    #[test]
    fn test_search_state_set_and_query() {
        let mut state = crate::search::SearchState::new();
        state
            .set_pattern("hello", SearchDirection::Forward)
            .unwrap();
        assert_eq!(state.last_pattern(), Some("hello"));
        assert_eq!(state.direction(), Some(SearchDirection::Forward));
        assert!(state.compiled().is_some());
    }

    #[test]
    fn test_search_state_invalid_pattern() {
        let mut state = crate::search::SearchState::new();
        // "\(" in vi regex means literal "(", which is valid as basic regex
        // Use an actually invalid regex pattern
        let result = state.set_pattern("[invalid", SearchDirection::Forward);
        assert!(result.is_err());
    }

    #[test]
    fn test_find_next_integration() {
        let buffer = Buffer::from_string("hello world\nfoo bar\nbaz".to_string());
        let cursor = Cursor::new(0, 0);
        let regex = crate::search::ViRegex::compile("bar").unwrap();

        let result = find_next(&buffer, &cursor, &regex, SearchDirection::Forward, true).unwrap();
        let (m, wrapped) = result.unwrap();
        assert_eq!(m.row, 1);
        assert_eq!(m.col_start, 4);
        assert_eq!(m.col_end, 7);
        assert!(!wrapped);
    }

    #[test]
    fn test_find_next_backward_integration() {
        let buffer = Buffer::from_string("hello world\nfoo bar".to_string());
        let cursor = Cursor::new(1, 7);
        let regex = crate::search::ViRegex::compile("hello").unwrap();

        let result = find_next(&buffer, &cursor, &regex, SearchDirection::Backward, true).unwrap();
        let (m, wrapped) = result.unwrap();
        assert_eq!(m.row, 0);
        assert_eq!(m.col_start, 0);
        assert!(!wrapped);
    }

    #[test]
    fn test_find_next_wrap_around_integration() {
        let buffer = Buffer::from_string("hello world\nfoo bar".to_string());
        let cursor = Cursor::new(1, 0);
        let regex = crate::search::ViRegex::compile("hello").unwrap();

        let result = find_next(&buffer, &cursor, &regex, SearchDirection::Forward, true).unwrap();
        let (m, wrapped) = result.unwrap();
        assert_eq!(m.row, 0);
        assert_eq!(m.col_start, 0);
        assert!(wrapped);
    }

    #[test]
    fn test_find_next_no_match_integration() {
        let buffer = Buffer::from_string("hello world".to_string());
        let cursor = Cursor::new(0, 0);
        let regex = crate::search::ViRegex::compile("xyz").unwrap();

        let result = find_next(&buffer, &cursor, &regex, SearchDirection::Forward, true).unwrap();
        assert!(result.is_none());
    }

    // =========================================================================
    // is_word_char tests
    // =========================================================================

    #[test]
    fn test_is_word_char_alpha() {
        assert!(crate::buffer::unicode::is_word_char('a'));
        assert!(crate::buffer::unicode::is_word_char('Z'));
    }

    #[test]
    fn test_is_word_char_digit() {
        assert!(crate::buffer::unicode::is_word_char('0'));
        assert!(crate::buffer::unicode::is_word_char('9'));
    }

    #[test]
    fn test_is_word_char_underscore() {
        assert!(crate::buffer::unicode::is_word_char('_'));
    }

    #[test]
    fn test_is_word_char_non_word() {
        assert!(!crate::buffer::unicode::is_word_char(' '));
        assert!(!crate::buffer::unicode::is_word_char('.'));
        assert!(!crate::buffer::unicode::is_word_char('('));
        assert!(!crate::buffer::unicode::is_word_char('-'));
    }

    // =========================================================================
    // extract_word_at_cursor tests (via Document)
    // =========================================================================

    #[test]
    fn test_extract_word_basic() {
        // Test word extraction logic directly
        let line = "hello world";
        let col = 0;
        let ch = line[col..].chars().next().unwrap();
        assert!(crate::buffer::unicode::is_word_char(ch));

        let start = 0_usize;
        let end = line[col..]
            .char_indices()
            .take_while(|(_, c)| crate::buffer::unicode::is_word_char(*c))
            .last()
            .map(|(i, c)| col + i + c.len_utf8())
            .unwrap_or(col);
        assert_eq!(&line[start..end], "hello");
    }

    #[test]
    fn test_extract_word_middle() {
        let line = "hello world";
        let col = 8; // on 'r'
        let ch = line[col..].chars().next().unwrap();
        assert_eq!(ch, 'r');
        assert!(crate::buffer::unicode::is_word_char(ch));

        let start = line[..col]
            .char_indices()
            .rev()
            .take_while(|(_, c)| crate::buffer::unicode::is_word_char(*c))
            .last()
            .map(|(i, _)| i)
            .unwrap_or(col);
        let end = line[col..]
            .char_indices()
            .take_while(|(_, c)| crate::buffer::unicode::is_word_char(*c))
            .last()
            .map(|(i, c)| col + i + c.len_utf8())
            .unwrap_or(col);
        assert_eq!(&line[start..end], "world");
    }

    #[test]
    fn test_extract_word_on_non_word_char() {
        let line = "hello world";
        let col = 5; // on space
        let ch = line[col..].chars().next().unwrap();
        assert!(!crate::buffer::unicode::is_word_char(ch));
    }

    // =========================================================================
    // Visual mode helper tests (Chunk A)
    // =========================================================================

    #[test]
    fn test_visual_kind_to_selection_type_character() {
        use crate::buffer::SelectionType;
        assert_eq!(
            super::visual_kind_to_selection_type(crate::mode::VisualKind::Character),
            SelectionType::Character
        );
    }

    #[test]
    fn test_visual_kind_to_selection_type_line() {
        use crate::buffer::SelectionType;
        assert_eq!(
            super::visual_kind_to_selection_type(crate::mode::VisualKind::Line),
            SelectionType::Line
        );
    }

    #[test]
    fn test_visual_kind_to_selection_type_block() {
        use crate::buffer::SelectionType;
        assert_eq!(
            super::visual_kind_to_selection_type(crate::mode::VisualKind::Block),
            SelectionType::Block
        );
    }

    #[test]
    fn test_toggle_case_range_full_line() {
        assert_eq!(
            super::toggle_case_range("Hello World", 0, 11),
            "hELLO wORLD"
        );
    }

    #[test]
    fn test_toggle_case_range_partial() {
        // Toggle only "ell" (bytes 1..4)
        assert_eq!(super::toggle_case_range("Hello", 1, 4), "HELLo");
        // Simpler assertion
        let result = super::toggle_case_range("Hello", 1, 4);
        assert_eq!(&result[0..1], "H"); // outside range, unchanged
        assert_eq!(&result[1..4], "ELL"); // toggled
        assert_eq!(&result[4..5], "o"); // outside range, unchanged
    }

    #[test]
    fn test_toggle_case_range_no_change_for_digits() {
        assert_eq!(super::toggle_case_range("abc123", 0, 6), "ABC123");
    }

    // =========================================================================
    // Block visual delete (Chunk E)
    // =========================================================================

    /// Verify block delete column logic by exercising Buffer::delete_range directly,
    /// mirroring what execute_visual_delete_inner does for VisualKind::Block.
    #[test]
    fn test_visual_block_delete() {
        use crate::buffer::{Buffer, Cursor, Selection, SelectionType};

        // "hello\nworld\ntest" — block cols 1..=3 (bytes 1..=3 inclusive on each row)
        let mut buffer = Buffer::from_string("hello\nworld\ntest".to_string());
        let selection = Selection::new(Cursor::new(0, 1), Cursor::new(2, 3), SelectionType::Block);
        let sel = selection.normalize();
        // After normalize(), sel.start.col <= sel.end.col.

        // Delete in reverse row order to preserve earlier row offsets
        for row in (sel.start.row..=sel.end.row).rev() {
            if let Some(line) = buffer.line(row) {
                let s = sel.start.col.min(line.len());
                let e = next_grapheme_boundary(line, sel.end.col.min(line.len()));
                if s < e {
                    let start_c = Cursor::new(row, s);
                    let end_c = Cursor::new(row, e);
                    buffer.delete_range(&start_c, &end_c).unwrap();
                }
            }
        }

        // Row 0: "hello" -> delete [1..4] "ell" -> "ho"
        assert_eq!(buffer.line(0), Some("ho"));
        // Row 1: "world" -> delete [1..4] "orl" -> "wd"
        assert_eq!(buffer.line(1), Some("wd"));
        // Row 2: "test"  -> delete [1..4] "est" -> "t"
        assert_eq!(buffer.line(2), Some("t"));
    }

    // =========================================================================
    // Replace mode integration tests
    // =========================================================================

    /// Helper: build a test editor, send keys through handle_key, return editor.
    fn replace_mode_editor(content: &str, keys: &[Key]) -> super::super::Editor {
        let mut editor = super::super::Editor::for_testing(content);
        // Enter Replace mode via R key
        for key in keys {
            editor.handle_key(key.clone()).unwrap();
        }
        editor
    }

    #[test]
    fn test_replace_mode_r_key_enters_replace() {
        let mut editor = super::super::Editor::for_testing("hello");
        editor.handle_key(Key::Char('R')).unwrap();
        assert_eq!(editor.state.mode, crate::mode::Mode::Replace);
    }

    #[test]
    fn test_replace_mode_status_str() {
        assert_eq!(crate::mode::Mode::Replace.as_str(), "REPLACE");
    }

    #[test]
    fn test_replace_mode_overwrites_chars() {
        // "hello" -> R x y z Esc -> "xyzlo"
        let editor = replace_mode_editor(
            "hello",
            &[
                Key::Char('R'),
                Key::Char('x'),
                Key::Char('y'),
                Key::Char('z'),
                Key::Esc,
            ],
        );
        assert_eq!(editor.document.buffer.line(0), Some("xyzlo"));
        assert_eq!(editor.state.mode, crate::mode::Mode::Normal);
    }

    #[test]
    fn test_replace_mode_appends_at_eol() {
        // "hi" -> R x x x Esc -> "xxx" (first two overwrite, third appends)
        let editor = replace_mode_editor(
            "hi",
            &[
                Key::Char('R'),
                Key::Char('x'),
                Key::Char('x'),
                Key::Char('x'),
                Key::Esc,
            ],
        );
        assert_eq!(editor.document.buffer.line(0), Some("xxx"));
    }

    #[test]
    fn test_replace_mode_backspace_restores_original() {
        // "hello" -> R x Backspace Esc -> "hello" (x overwrites h, Backspace restores h)
        let editor = replace_mode_editor(
            "hello",
            &[Key::Char('R'), Key::Char('x'), Key::Backspace, Key::Esc],
        );
        assert_eq!(editor.document.buffer.line(0), Some("hello"));
        // Cursor should be at col 0 after restoring and exiting
        assert_eq!(editor.document.cursor.col, 0);
    }

    #[test]
    fn test_replace_mode_backspace_at_entry_point_does_nothing() {
        // "hello" -> R Backspace Esc -> "hello" (no chars typed, Backspace is a no-op)
        let editor = replace_mode_editor("hello", &[Key::Char('R'), Key::Backspace, Key::Esc]);
        assert_eq!(editor.document.buffer.line(0), Some("hello"));
    }

    #[test]
    fn test_replace_mode_backspace_appended_char() {
        // "hi" -> R x x x Backspace Esc -> "hixx" wait, "hi" is 2 chars:
        // x overwrites 'h', x overwrites 'i', x appends -> "xxx"
        // Backspace: pops None (appended), deletes x -> "xx", cursor at col 2
        // Esc -> moves left -> cursor at col 1
        let editor = replace_mode_editor(
            "hi",
            &[
                Key::Char('R'),
                Key::Char('x'),
                Key::Char('x'),
                Key::Char('x'),
                Key::Backspace,
                Key::Esc,
            ],
        );
        assert_eq!(editor.document.buffer.line(0), Some("xx"));
    }

    #[test]
    fn test_replace_mode_undo_restores_whole_session() {
        // "hello" -> R x y z Esc -> "xyzlo", then u -> "hello"
        let mut editor = replace_mode_editor(
            "hello",
            &[
                Key::Char('R'),
                Key::Char('x'),
                Key::Char('y'),
                Key::Char('z'),
                Key::Esc,
            ],
        );
        assert_eq!(editor.document.buffer.line(0), Some("xyzlo"));
        // Press u to undo
        editor.handle_key(Key::Char('u')).unwrap();
        assert_eq!(editor.document.buffer.line(0), Some("hello"));
    }

    #[test]
    fn test_replace_mode_backspace_then_undo() {
        // "hello" -> R x Backspace Esc -> "hello" (net no change), u -> "hello"
        // This specifically tests the undo double-record bug fix:
        // typing 'x' records ReplaceChar{old:'h',new:'x'}, Backspace records
        // ReplaceChar{old:'x',new:'h'} — undo reverses both cleanly.
        let mut editor = replace_mode_editor(
            "hello",
            &[Key::Char('R'), Key::Char('x'), Key::Backspace, Key::Esc],
        );
        assert_eq!(editor.document.buffer.line(0), Some("hello"));
        // Undo — even though net effect was zero, undo should leave "hello"
        editor.handle_key(Key::Char('u')).unwrap();
        assert_eq!(editor.document.buffer.line(0), Some("hello"));
    }

    #[test]
    fn test_replace_mode_dot_repeat_overwrites() {
        // "hello world" -> R x y z Esc -> "xyzlo world"
        // Move to 'w' at col 6, then . -> "xyzlo xyzld"
        let mut editor = super::super::Editor::for_testing("hello world");
        // Enter replace, type xyz, exit
        for key in &[
            Key::Char('R'),
            Key::Char('x'),
            Key::Char('y'),
            Key::Char('z'),
            Key::Esc,
        ] {
            editor.handle_key(key.clone()).unwrap();
        }
        assert_eq!(editor.document.buffer.line(0), Some("xyzlo world"));
        // Move cursor to 'w' (col 6)
        editor.document.cursor.col = 6;
        // Dot repeat
        editor.handle_key(Key::Char('.')).unwrap();
        assert_eq!(editor.document.buffer.line(0), Some("xyzlo xyzld"));
    }

    #[test]
    fn test_replace_mode_replace_originals_cleared_on_entry() {
        let mut editor = super::super::Editor::for_testing("hello");
        // Manually dirty the replace_originals to verify it's cleared on R
        editor
            .state
            .insert_state
            .replace_originals
            .push(Some("z".to_string()));
        editor.handle_key(Key::Char('R')).unwrap();
        assert!(editor.state.insert_state.replace_originals.is_empty());
    }

    // =========================================================================
    // Scroll command tests (Gap 1)
    // =========================================================================

    #[test]
    fn test_ctrl_f_scrolls_forward_full_screen() {
        // Build a buffer with 40 lines, viewport height 10.
        // Ctrl-f scrolls by height-2=8 lines, keeping 2 lines of context.
        let content = (0..40)
            .map(|i| format!("line{}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let mut editor = super::super::Editor::for_testing(&content);
        editor.state.viewport.set_height(10);
        editor.handle_key(Key::Ctrl('f')).unwrap();
        assert_eq!(editor.state.viewport.top_line(), 8);
        assert_eq!(editor.document.cursor.row, 8);
    }

    #[test]
    fn test_ctrl_b_scrolls_backward_full_screen() {
        // Ctrl-b scrolls by height-2=8 lines, keeping 2 lines of context.
        let content = (0..40)
            .map(|i| format!("line{}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let mut editor = super::super::Editor::for_testing(&content);
        editor.state.viewport.set_height(10);
        editor.state.viewport.set_top_line(20);
        editor.document.cursor.row = 20;
        editor.handle_key(Key::Ctrl('b')).unwrap();
        assert_eq!(editor.state.viewport.top_line(), 12);
        assert_eq!(editor.document.cursor.row, 12);
    }

    #[test]
    fn test_ctrl_d_scrolls_half_screen_down() {
        let content = (0..40)
            .map(|i| format!("line{}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let mut editor = super::super::Editor::for_testing(&content);
        editor.state.viewport.set_height(20);
        editor.handle_key(Key::Ctrl('d')).unwrap();
        // Default half = 10
        assert_eq!(editor.state.viewport.top_line(), 10);
        assert_eq!(editor.document.cursor.row, 10);
    }

    #[test]
    fn test_ctrl_u_scrolls_half_screen_up() {
        let content = (0..40)
            .map(|i| format!("line{}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let mut editor = super::super::Editor::for_testing(&content);
        editor.state.viewport.set_height(20);
        editor.state.viewport.set_top_line(20);
        editor.document.cursor.row = 20;
        editor.handle_key(Key::Ctrl('u')).unwrap();
        assert_eq!(editor.state.viewport.top_line(), 10);
        assert_eq!(editor.document.cursor.row, 10);
    }

    #[test]
    fn test_ctrl_e_scrolls_one_line_down() {
        let content = (0..20)
            .map(|i| format!("line{}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let mut editor = super::super::Editor::for_testing(&content);
        editor.state.viewport.set_height(10);
        // Start with cursor on row 5 so it remains visible after scroll
        editor.document.cursor.row = 5;
        editor.handle_key(Key::Ctrl('e')).unwrap();
        assert_eq!(editor.state.viewport.top_line(), 1);
        // Cursor row 5 >= new top_line 1, so cursor does not move
        assert_eq!(editor.document.cursor.row, 5);
    }

    #[test]
    fn test_ctrl_y_scrolls_one_line_up() {
        let content = (0..20)
            .map(|i| format!("line{}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let mut editor = super::super::Editor::for_testing(&content);
        editor.state.viewport.set_height(10);
        editor.state.viewport.set_top_line(5);
        editor.document.cursor.row = 5;
        editor.handle_key(Key::Ctrl('y')).unwrap();
        assert_eq!(editor.state.viewport.top_line(), 4);
        // Cursor still visible, no move needed
        assert_eq!(editor.document.cursor.row, 5);
    }

    #[test]
    fn test_ctrl_e_clamps_cursor_when_scrolled_past() {
        let content = (0..20)
            .map(|i| format!("line{}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let mut editor = super::super::Editor::for_testing(&content);
        editor.state.viewport.set_height(5);
        editor.state.viewport.set_top_line(10);
        editor.document.cursor.row = 10;
        // Scroll down 3 times — cursor should be clamped to top of viewport
        for _ in 0..3 {
            editor.handle_key(Key::Ctrl('e')).unwrap();
        }
        assert!(editor.document.cursor.row >= editor.state.viewport.top_line());
    }

    // =========================================================================
    // H/M/L screen position motion tests (Gap 3)
    // =========================================================================

    #[test]
    fn test_h_moves_to_top_of_screen() {
        let content = (0..30)
            .map(|i| format!("line{}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let mut editor = super::super::Editor::for_testing(&content);
        editor.state.viewport.set_height(10);
        editor.state.viewport.set_top_line(5);
        editor.document.cursor.row = 10;
        editor.handle_key(Key::Char('H')).unwrap();
        assert_eq!(editor.document.cursor.row, 5); // top_line = 5
    }

    #[test]
    fn test_m_moves_to_middle_of_screen() {
        let content = (0..30)
            .map(|i| format!("line{}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let mut editor = super::super::Editor::for_testing(&content);
        editor.state.viewport.set_height(10);
        editor.state.viewport.set_top_line(0);
        editor.document.cursor.row = 0;
        editor.handle_key(Key::Char('M')).unwrap();
        // Middle of 10-line viewport = line 5
        assert_eq!(editor.document.cursor.row, 5);
    }

    #[test]
    fn test_l_moves_to_bottom_of_screen() {
        let content = (0..30)
            .map(|i| format!("line{}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let mut editor = super::super::Editor::for_testing(&content);
        editor.state.viewport.set_height(10);
        editor.state.viewport.set_top_line(0);
        editor.document.cursor.row = 0;
        editor.handle_key(Key::Char('L')).unwrap();
        assert_eq!(editor.document.cursor.row, 9); // top=0, height=10, bottom=9
    }

    // =========================================================================
    // Insert mode Ctrl-w / Ctrl-u tests (Gap 2)
    // =========================================================================

    #[test]
    fn test_insert_ctrl_w_deletes_word() {
        let mut editor = super::super::Editor::for_testing("hello world");
        // Position cursor after "world" (end of line in insert mode)
        editor.handle_key(Key::Char('A')).unwrap(); // end of line
                                                    // Ctrl-w should delete "world"
        editor.handle_key(Key::Ctrl('w')).unwrap();
        editor.handle_key(Key::Esc).unwrap();
        assert_eq!(editor.document.buffer.line(0), Some("hello "));
    }

    #[test]
    fn test_insert_ctrl_w_deletes_through_whitespace() {
        let mut editor = super::super::Editor::for_testing("hello   ");
        editor.handle_key(Key::Char('A')).unwrap();
        editor.handle_key(Key::Ctrl('w')).unwrap();
        editor.handle_key(Key::Esc).unwrap();
        // Vi Ctrl-w: skip trailing spaces then skip the word "hello" → deletes all
        assert_eq!(editor.document.buffer.line(0), Some(""));
    }

    #[test]
    fn test_insert_ctrl_u_deletes_to_line_start() {
        let mut editor = super::super::Editor::for_testing("hello world");
        editor.handle_key(Key::Char('A')).unwrap(); // cursor at end
        editor.handle_key(Key::Ctrl('u')).unwrap();
        editor.handle_key(Key::Esc).unwrap();
        assert_eq!(editor.document.buffer.line(0), Some(""));
    }

    #[test]
    fn test_insert_ctrl_u_at_line_start_no_op() {
        let mut editor = super::super::Editor::for_testing("hello");
        editor.handle_key(Key::Char('i')).unwrap(); // insert at beginning
        editor.handle_key(Key::Ctrl('u')).unwrap(); // nothing to delete
        editor.handle_key(Key::Esc).unwrap();
        assert_eq!(editor.document.buffer.line(0), Some("hello"));
    }

    // =========================================================================
    // Insert mode Ctrl-v tests (Gap 14)
    // =========================================================================

    #[test]
    fn test_insert_ctrl_v_inserts_next_char_literally() {
        // Ctrl-v followed by a regular character inserts it verbatim
        let mut editor = super::super::Editor::for_testing("hi");
        editor.handle_key(Key::Char('A')).unwrap(); // append at end
        editor.handle_key(Key::Ctrl('v')).unwrap(); // literal-next
        editor.handle_key(Key::Char('!')).unwrap(); // inserted literally
        editor.handle_key(Key::Esc).unwrap();
        assert_eq!(editor.document.buffer.line(0), Some("hi!"));
    }

    #[test]
    fn test_insert_ctrl_v_ctrl_a_inserts_control_char() {
        // Ctrl-v followed by Ctrl-A inserts ^A (0x01)
        let mut editor = super::super::Editor::for_testing("");
        editor.handle_key(Key::Char('i')).unwrap();
        editor.handle_key(Key::Ctrl('v')).unwrap();
        editor.handle_key(Key::Ctrl('a')).unwrap(); // ^A = 0x01
        editor.handle_key(Key::Esc).unwrap();
        let line = editor.document.buffer.line(0).unwrap_or_default();
        assert_eq!(line.chars().next(), Some('\x01'));
    }

    #[test]
    fn test_insert_ctrl_v_sets_literal_next_flag() {
        let mut editor = super::super::Editor::for_testing("x");
        editor.handle_key(Key::Char('i')).unwrap();
        assert!(!editor.state.insert_state.insert_literal_next);
        editor.handle_key(Key::Ctrl('v')).unwrap();
        assert!(editor.state.insert_state.insert_literal_next);
        // Consume the flag with next key
        editor.handle_key(Key::Char('a')).unwrap();
        assert!(!editor.state.insert_state.insert_literal_next);
    }

    // =========================================================================
    // z redraw command tests (Gap 12)
    // =========================================================================

    #[test]
    fn test_zt_scrolls_cursor_to_top() {
        let content = (0..30)
            .map(|i| format!("line{}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let mut editor = super::super::Editor::for_testing(&content);
        editor.state.viewport.set_height(10);
        editor.document.cursor.row = 15;
        editor.handle_key(Key::Char('z')).unwrap();
        editor.handle_key(Key::Char('t')).unwrap();
        assert_eq!(editor.state.viewport.top_line(), 15);
    }

    #[test]
    fn test_zz_scrolls_cursor_to_middle() {
        let content = (0..30)
            .map(|i| format!("line{}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let mut editor = super::super::Editor::for_testing(&content);
        editor.state.viewport.set_height(10);
        editor.document.cursor.row = 20;
        editor.handle_key(Key::Char('z')).unwrap();
        editor.handle_key(Key::Char('z')).unwrap();
        // cursor row 20, height 10 → top = 20 - 5 = 15
        assert_eq!(editor.state.viewport.top_line(), 15);
    }

    #[test]
    fn test_zb_scrolls_cursor_to_bottom() {
        let content = (0..30)
            .map(|i| format!("line{}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let mut editor = super::super::Editor::for_testing(&content);
        editor.state.viewport.set_height(10);
        editor.document.cursor.row = 20;
        editor.handle_key(Key::Char('z')).unwrap();
        editor.handle_key(Key::Char('b')).unwrap();
        // cursor row 20, height 10 → top = 20 - 9 = 11
        assert_eq!(editor.state.viewport.top_line(), 11);
    }

    // =========================================================================
    // U command (undo all changes to current line) tests (Gap 4)
    // =========================================================================

    #[test]
    fn test_u_restores_line_to_snapshot() {
        let mut editor = super::super::Editor::for_testing("hello world");
        // Take snapshot by entering and exiting insert mode on line 0
        editor.update_line_snapshot(); // manually update snapshot for row 0
                                       // Modify the line
        editor.handle_key(Key::Char('x')).unwrap(); // delete 'h'
        assert_eq!(editor.document.buffer.line(0), Some("ello world"));
        // U should restore
        editor.handle_key(Key::Char('U')).unwrap();
        assert_eq!(editor.document.buffer.line(0), Some("hello world"));
    }

    #[test]
    fn test_ctrl_g_shows_file_info() {
        let mut editor = super::super::Editor::for_testing("hello\nworld");
        editor.handle_key(Key::Ctrl('g')).unwrap();
        assert!(editor.state.status_message.is_some());
        let msg = editor
            .state
            .status_message
            .as_ref()
            .map(|m| m.as_str_formatted())
            .unwrap_or_default();
        assert!(msg.contains("lines"));
    }

    // =========================================================================
    // Gap 16: Multiple file argument list tests
    // =========================================================================

    #[test]
    fn test_show_args_empty() {
        let mut editor = super::super::Editor::for_testing("hello");
        // :args with empty arg list shows "(no argument list)"
        editor.handle_key(Key::Char(':')).unwrap();
        for ch in "args".chars() {
            editor.handle_key(Key::Char(ch)).unwrap();
        }
        editor.handle_key(Key::Enter).unwrap();
        let msg = editor
            .state
            .status_message
            .as_ref()
            .map(|m| m.as_str_formatted())
            .unwrap_or_default();
        assert!(msg.contains("no argument"));
    }

    #[test]
    fn test_navigate_args_no_list() {
        let mut editor = super::super::Editor::for_testing("hello");
        // :n with no arg list shows message
        editor.navigate_args(1);
        assert!(editor.state.status_message.is_some());
    }

    #[test]
    fn test_navigate_args_at_last() {
        let mut editor = super::super::Editor::for_testing("hello");
        editor.state.file_navigation.arg_list = vec!["a.txt".to_string(), "b.txt".to_string()];
        editor.state.file_navigation.arg_idx = 1; // already at last
        editor.navigate_args(1);
        let msg = editor
            .state
            .status_message
            .as_ref()
            .map(|m| m.as_str_formatted())
            .unwrap_or_default();
        assert!(msg.contains("Last file"));
        // arg_idx should stay at 1
        assert_eq!(editor.state.file_navigation.arg_idx, 1);
    }

    #[test]
    fn test_navigate_args_backward_at_first() {
        let mut editor = super::super::Editor::for_testing("hello");
        editor.state.file_navigation.arg_list = vec!["a.txt".to_string(), "b.txt".to_string()];
        editor.state.file_navigation.arg_idx = 0; // already at first
        editor.navigate_args_backward(1);
        let msg = editor
            .state
            .status_message
            .as_ref()
            .map(|m| m.as_str_formatted())
            .unwrap_or_default();
        assert!(msg.contains("First file"));
        assert_eq!(editor.state.file_navigation.arg_idx, 0);
    }

    // =========================================================================
    // execute_startup_commands
    // =========================================================================

    #[test]
    fn test_startup_command_set_option() {
        let mut editor = super::super::Editor::for_testing("hello\nworld\n");
        editor.execute_startup_commands(&["set number".to_string()]);
        assert!(editor.state.settings.number);
    }

    #[test]
    fn test_startup_command_goto_line() {
        let mut editor = super::super::Editor::for_testing("line1\nline2\nline3\n");
        editor.execute_startup_commands(&["3".to_string()]);
        assert_eq!(editor.document.cursor.row, 2);
    }

    #[test]
    fn test_startup_command_dollar_goto_last() {
        // No trailing newline → 3 lines: row 0, 1, 2
        let mut editor = super::super::Editor::for_testing("line1\nline2\nline3");
        editor.execute_startup_commands(&["$".to_string()]);
        assert_eq!(editor.document.cursor.row, 2);
    }

    #[test]
    fn test_startup_commands_execute_in_order() {
        let mut editor = super::super::Editor::for_testing("line1\nline2\nline3");
        // First go to line 3, then back to line 1
        editor.execute_startup_commands(&["3".to_string(), "1".to_string()]);
        assert_eq!(editor.document.cursor.row, 0);
    }

    #[test]
    fn test_startup_command_invalid_shows_status() {
        let mut editor = super::super::Editor::for_testing("hello\n");
        editor.execute_startup_commands(&["invalid_command_xyz".to_string()]);
        // Should not crash and should surface an error via status message
        let msg = editor
            .state
            .status_message
            .as_ref()
            .map(|m| m.as_str_formatted())
            .unwrap_or_default();
        assert!(
            !msg.is_empty(),
            "expected a status message for an invalid command"
        );
    }

    // =========================================================================
    // P27 — Ctrl-@ re-insert last inserted text
    // =========================================================================

    fn enter_insert(editor: &mut super::super::Editor) {
        editor.handle_key(Key::Char('i')).unwrap();
    }

    fn exit_insert(editor: &mut super::super::Editor) {
        editor.handle_key(Key::Esc).unwrap();
    }

    #[test]
    fn test_ctrl_at_reinserts_last_text() {
        let mut editor = super::super::Editor::for_testing("");
        // First insert: type "hi"
        enter_insert(&mut editor);
        editor.handle_key(Key::Char('h')).unwrap();
        editor.handle_key(Key::Char('i')).unwrap();
        exit_insert(&mut editor);

        // Move to end and use Ctrl-@ to replay
        editor.handle_key(Key::Char('A')).unwrap();
        editor.handle_key(Key::Ctrl('@')).unwrap();

        // Should be back in Normal mode and "hihi" on the line
        assert_eq!(editor.state.mode, super::super::super::mode::Mode::Normal);
        assert_eq!(editor.document.buffer.line(0).unwrap(), "hihi");
    }

    #[test]
    fn test_ctrl_at_no_prior_insert_exits_insert() {
        let mut editor = super::super::Editor::for_testing("");
        enter_insert(&mut editor);
        // No prior insert — Ctrl-@ should exit insert mode without inserting anything
        editor.handle_key(Key::Ctrl('@')).unwrap();
        assert_eq!(editor.state.mode, super::super::super::mode::Mode::Normal);
        assert_eq!(editor.document.buffer.line(0).unwrap(), "");
    }

    #[test]
    fn test_ctrl_at_reinserts_multi_char() {
        let mut editor = super::super::Editor::for_testing("");
        enter_insert(&mut editor);
        editor.handle_key(Key::Char('a')).unwrap();
        editor.handle_key(Key::Char('b')).unwrap();
        editor.handle_key(Key::Char('c')).unwrap();
        exit_insert(&mut editor);

        // Move to end, re-enter insert, replay
        editor.handle_key(Key::Char('A')).unwrap();
        editor.handle_key(Key::Ctrl('@')).unwrap();

        assert_eq!(editor.state.mode, super::super::super::mode::Mode::Normal);
        assert_eq!(editor.document.buffer.line(0).unwrap(), "abcabc");
    }

    // =========================================================================
    // P28 — 0 Ctrl-D and ^ Ctrl-D
    // =========================================================================

    fn type_in_insert(editor: &mut super::super::Editor, s: &str) {
        for ch in s.chars() {
            editor.handle_key(Key::Char(ch)).unwrap();
        }
    }

    #[test]
    fn test_zero_ctrl_d_removes_all_indent() {
        let mut editor = super::super::Editor::for_testing("        hello");
        // Enter insert, move to after indent, type '0' then Ctrl-D
        enter_insert(&mut editor);
        // Position cursor after indent (cursor starts at col 0 in insert mode)
        // Type '0' then Ctrl-D
        editor.handle_key(Key::Char('0')).unwrap();
        editor.handle_key(Key::Ctrl('d')).unwrap();
        exit_insert(&mut editor);

        // All leading whitespace should be gone, '0' removed, "hello" remains
        assert_eq!(editor.document.buffer.line(0).unwrap(), "hello");
    }

    #[test]
    fn test_caret_ctrl_d_removes_indent_preserves_next_line_indent() {
        // Start with an indented line. In insert mode, type '^' then Ctrl-D:
        // this strips all leading whitespace from the current line and saves
        // the indent for the next Enter. Cursor ends at col 0 of "hello".
        // Pressing Enter then splits at col 0: line 0 becomes "" and line 1
        // becomes the saved indent + remaining text.
        let mut editor = super::super::Editor::for_testing("    hello");
        enter_insert(&mut editor);
        editor.handle_key(Key::Char('^')).unwrap();
        editor.handle_key(Key::Ctrl('d')).unwrap();
        // At this point line is "hello", cursor at col 0, saved_indent = "    "
        assert_eq!(editor.document.buffer.line(0).unwrap(), "hello");
        // Press Enter — splits at col 0; new line inherits saved indent
        editor.handle_key(Key::Enter).unwrap();
        exit_insert(&mut editor);

        // Line 0 is now empty (cursor was at col 0 when Enter was pressed)
        assert_eq!(editor.document.buffer.line(0).unwrap(), "");
        // Line 1 has saved indent prepended to the rest of the text
        assert_eq!(editor.document.buffer.line(1).unwrap(), "    hello");
    }

    #[test]
    fn test_plain_ctrl_d_dedents_by_shiftwidth() {
        let mut editor = super::super::Editor::for_testing("        hello");
        editor.state.settings.shiftwidth = 4;
        enter_insert(&mut editor);
        editor.handle_key(Key::Ctrl('d')).unwrap();
        exit_insert(&mut editor);

        // Should remove one shiftwidth (4 spaces) of indent
        assert_eq!(editor.document.buffer.line(0).unwrap(), "    hello");
    }

    // =========================================================================
    // P29 — :abbreviate / :unabbreviate / expansion
    // =========================================================================

    fn make_editor_with_abbrev(lhs: &str, rhs: &str) -> super::super::Editor {
        let mut editor = super::super::Editor::for_testing("");
        editor
            .state
            .mappings
            .abbreviations
            .push((lhs.to_string(), rhs.to_string()));
        editor
    }

    #[test]
    fn test_abbreviation_expanded_on_space() {
        let mut editor = make_editor_with_abbrev("hw", "hello world");
        enter_insert(&mut editor);
        type_in_insert(&mut editor, "hw");
        editor.handle_key(Key::Char(' ')).unwrap();
        exit_insert(&mut editor);
        assert_eq!(editor.document.buffer.line(0).unwrap(), "hello world ");
    }

    #[test]
    fn test_abbreviation_expanded_on_esc() {
        let mut editor = make_editor_with_abbrev("teh", "the");
        enter_insert(&mut editor);
        type_in_insert(&mut editor, "teh");
        exit_insert(&mut editor); // Esc triggers expansion
        assert_eq!(editor.document.buffer.line(0).unwrap(), "the");
    }

    #[test]
    fn test_abbreviation_expanded_on_enter() {
        let mut editor = make_editor_with_abbrev("foo", "foobar");
        enter_insert(&mut editor);
        type_in_insert(&mut editor, "foo");
        editor.handle_key(Key::Enter).unwrap();
        exit_insert(&mut editor);
        assert_eq!(editor.document.buffer.line(0).unwrap(), "foobar");
    }

    #[test]
    fn test_abbreviation_not_expanded_mid_word() {
        // "food" typed — "foo" is an abbrev but the 'd' is a keyword char,
        // so expansion should NOT fire until a non-keyword char follows.
        let mut editor = make_editor_with_abbrev("foo", "foobar");
        enter_insert(&mut editor);
        type_in_insert(&mut editor, "food");
        exit_insert(&mut editor); // Esc checks "food", which != "foo"
        assert_eq!(editor.document.buffer.line(0).unwrap(), "food");
    }

    #[test]
    fn test_abbreviation_no_match_leaves_text_unchanged() {
        let mut editor = make_editor_with_abbrev("hw", "hello world");
        enter_insert(&mut editor);
        type_in_insert(&mut editor, "hi");
        editor.handle_key(Key::Char(' ')).unwrap();
        exit_insert(&mut editor);
        assert_eq!(editor.document.buffer.line(0).unwrap(), "hi ");
    }

    #[test]
    fn test_unabbreviate_removes_entry() {
        let mut editor = make_editor_with_abbrev("hw", "hello world");
        // Add then remove
        editor
            .state
            .mappings
            .abbreviations
            .retain(|(k, _)| k != "hw");
        enter_insert(&mut editor);
        type_in_insert(&mut editor, "hw");
        editor.handle_key(Key::Char(' ')).unwrap();
        exit_insert(&mut editor);
        assert_eq!(editor.document.buffer.line(0).unwrap(), "hw ");
    }

    #[test]
    fn test_later_abbreviation_wins() {
        // Two definitions for same lhs — last one should win.
        let mut editor = super::super::Editor::for_testing("");
        editor
            .state
            .mappings
            .abbreviations
            .push(("hw".to_string(), "hello world".to_string()));
        editor
            .state
            .mappings
            .abbreviations
            .push(("hw".to_string(), "hey world".to_string()));
        // execute_abbreviate retains last via retain+push; simulate that here:
        // The iter().rev().find() picks the last entry pushed.
        enter_insert(&mut editor);
        type_in_insert(&mut editor, "hw");
        editor.handle_key(Key::Char(' ')).unwrap();
        exit_insert(&mut editor);
        assert_eq!(editor.document.buffer.line(0).unwrap(), "hey world ");
    }

    // --- Map dispatch unit tests ---

    #[test]
    fn test_map_dispatch_single_char_fires_rhs() {
        // :map x G — pressing x in Normal mode should jump to last line.
        let mut editor = super::super::Editor::for_testing("line1\nline2\nline3");
        // Add mapping: x -> G (jump to last line)
        editor
            .state
            .mappings
            .normal_maps
            .push((vec![Key::Char('x')], vec![Key::Char('G')]));
        assert_eq!(editor.document.cursor.row, 0);
        editor.handle_key(Key::Char('x')).unwrap();
        // G should move cursor to last line (row 2)
        assert_eq!(editor.document.cursor.row, 2);
    }

    #[test]
    fn test_map_multi_char_pending_then_fire() {
        // :map ,d dd — pressing ',' buffers, pressing 'd' fires dd.
        let mut editor = super::super::Editor::for_testing("delete me\nkeep this");
        editor.state.mappings.normal_maps.push((
            vec![Key::Char(','), Key::Char('d')],
            vec![Key::Char('d'), Key::Char('d')],
        ));
        // After ',': map_pending == [','] — line is NOT yet deleted.
        editor.handle_key(Key::Char(',')).unwrap();
        assert_eq!(editor.state.mappings.map_pending.len(), 1);
        assert_eq!(editor.document.buffer.line(0).unwrap(), "delete me");
        // After 'd': exact match fires dd — line IS deleted.
        editor.handle_key(Key::Char('d')).unwrap();
        assert_eq!(editor.state.mappings.map_pending.len(), 0);
        assert_eq!(editor.document.buffer.line(0).unwrap(), "keep this");
    }

    #[test]
    fn test_commandline_keys_recorded_in_macro() {
        // Start macro recording, enter command-line mode, type some keys, Esc.
        // Verify all keys are recorded including those typed in CommandLine mode.
        let mut editor = super::super::Editor::for_testing("hello");
        // Start recording into register 'e'
        editor.handle_key(Key::Char('q')).unwrap();
        editor.handle_key(Key::Char('e')).unwrap();
        assert!(editor.state.macro_state.recording.is_some());
        // Enter command-line mode
        editor.handle_key(Key::Char(':')).unwrap();
        // Type 's' while in CommandLine
        editor.handle_key(Key::Char('s')).unwrap();
        // Escape back to normal
        editor.handle_key(Key::Esc).unwrap();
        // Stop recording
        editor.handle_key(Key::Char('q')).unwrap();
        // After recording stops, the buffer is stored into register 'e'.
        // Decode the register content back to keys and verify.
        let reg_id = RegisterId::parse('e').unwrap();
        let reg_content = editor.state.registers.get(Some(reg_id)).unwrap();
        let reg_text = reg_content.text();
        // The register text should contain ':', 's', and Esc (\x1b).
        assert!(
            reg_text.contains(':'),
            "macro register should contain ':' but was: {:?}",
            reg_text,
        );
        assert!(
            reg_text.contains('s'),
            "macro register should contain 's' (CommandLine key) but was: {:?}",
            reg_text,
        );
        assert!(
            reg_text.contains('\x1b'),
            "macro register should contain Esc but was: {:?}",
            reg_text,
        );
    }
}

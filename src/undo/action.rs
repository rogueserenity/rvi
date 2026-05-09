//! Edit action types and undo entry grouping.
//!
//! This module defines the atomic editing actions that can be recorded and
//! reversed, and the `UndoEntry` struct that groups actions into a single
//! undoable unit.

use crate::buffer::Cursor;

/// A single reversible editing action.
///
/// Each variant stores enough information to both undo (apply the inverse)
/// and redo (reapply the original edit) the action. Actions operate on
/// byte-offset positions consistent with the buffer's coordinate system.
#[derive(Debug, Clone)]
pub enum EditAction {
    /// Character inserted at position.
    InsertChar {
        /// Row where the character was inserted.
        row: usize,
        /// Column (byte offset) where the character was inserted.
        col: usize,
        /// The character that was inserted.
        ch: char,
    },

    /// Newline inserted, splitting a line at position.
    InsertNewline {
        /// Row of the line that was split.
        row: usize,
        /// Column (byte offset) where the split occurred.
        col: usize,
    },

    /// Character deleted at position (stores char for undo restoration).
    DeleteChar {
        /// Row where the character was deleted.
        row: usize,
        /// Column (byte offset) where the character was deleted.
        col: usize,
        /// The character that was deleted.
        ch: char,
    },

    /// Range of text deleted (stores text for undo restoration).
    DeleteRange {
        /// Start row of the deleted range.
        start_row: usize,
        /// Start column (byte offset) of the deleted range.
        start_col: usize,
        /// End row of the deleted range.
        end_row: usize,
        /// End column (byte offset) of the deleted range.
        end_col: usize,
        /// The text that was deleted.
        text: String,
    },

    /// Text inserted at position (for put operations, multi-char inserts).
    InsertText {
        /// Row where the text was inserted.
        row: usize,
        /// Column (byte offset) where the text was inserted.
        col: usize,
        /// The text that was inserted.
        text: String,
    },

    /// Full line inserted at row.
    InsertLine {
        /// Row where the line was inserted.
        row: usize,
        /// Content of the inserted line.
        content: String,
    },

    /// Full line removed at row (stores content for undo restoration).
    RemoveLine {
        /// Row where the line was removed.
        row: usize,
        /// Content of the removed line.
        content: String,
    },

    /// Two lines joined at row (col = byte offset of join point in first line).
    JoinLines {
        /// Row of the first line in the join.
        row: usize,
        /// Column (byte offset) where the join occurred (end of first line).
        col: usize,
    },

    /// Line split at position (inverse of JoinLines).
    SplitLine {
        /// Row of the line that was split.
        row: usize,
        /// Column (byte offset) where the split occurred.
        col: usize,
    },

    /// Character replaced at position.
    ReplaceChar {
        /// Row where the replacement occurred.
        row: usize,
        /// Column (byte offset) where the replacement occurred.
        col: usize,
        /// The original character before replacement.
        old_char: char,
        /// The new character after replacement.
        new_char: char,
    },

    /// Entire line content replaced (used by substitute command).
    ///
    /// More efficient than recording individual insert/delete actions for
    /// each substitution within a line. Captures the complete before/after
    /// state of the line.
    ReplaceLine {
        /// Row where the replacement occurred.
        row: usize,
        /// The line content before replacement.
        old_content: String,
        /// The line content after replacement.
        new_content: String,
    },
}

/// A group of actions forming one undoable unit.
///
/// In vi, the undo granularity is per-command: a single normal mode command
/// or an entire insert mode session maps to one `UndoEntry`. Pressing `u`
/// undoes all actions in the entry as a unit, restoring the cursor to its
/// position before the command was executed.
#[derive(Debug, Clone)]
pub struct UndoEntry {
    /// The edit actions in this group, in chronological order.
    actions: Vec<EditAction>,
    /// Cursor position before the group was started.
    cursor_before: Cursor,
    /// Cursor position after the group was completed.
    cursor_after: Cursor,
    /// Whether the document was marked as modified before this group.
    modified_before: bool,
}

impl UndoEntry {
    /// Create a new undo entry with the given pre-edit state.
    pub fn new(cursor_before: Cursor, modified_before: bool) -> Self {
        Self {
            actions: Vec::new(),
            cursor_before,
            cursor_after: cursor_before,
            modified_before,
        }
    }

    /// Append an action to this entry.
    pub fn push(&mut self, action: EditAction) {
        self.actions.push(action);
    }

    /// Set the cursor position after this entry's edits complete.
    pub fn set_cursor_after(&mut self, cursor: Cursor) {
        self.cursor_after = cursor;
    }

    /// Check whether this entry contains any actions.
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }

    /// Get the actions in this entry.
    pub fn actions(&self) -> &[EditAction] {
        &self.actions
    }

    /// Get the cursor position from before this entry's edits.
    pub fn cursor_before(&self) -> Cursor {
        self.cursor_before
    }

    /// Get the cursor position from after this entry's edits.
    pub fn cursor_after(&self) -> Cursor {
        self.cursor_after
    }

    /// Get the document modified flag from before this entry's edits.
    pub fn modified_before(&self) -> bool {
        self.modified_before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_undo_entry_new() {
        let cursor = Cursor::new(5, 3);
        let entry = UndoEntry::new(cursor, false);
        assert!(entry.is_empty());
        assert_eq!(entry.cursor_before(), cursor);
        assert_eq!(entry.cursor_after(), cursor);
        assert!(!entry.modified_before());
    }

    #[test]
    fn test_undo_entry_push() {
        let mut entry = UndoEntry::new(Cursor::new(0, 0), false);
        assert!(entry.is_empty());

        entry.push(EditAction::InsertChar {
            row: 0,
            col: 0,
            ch: 'a',
        });
        assert!(!entry.is_empty());
        assert_eq!(entry.actions().len(), 1);

        entry.push(EditAction::InsertChar {
            row: 0,
            col: 1,
            ch: 'b',
        });
        assert_eq!(entry.actions().len(), 2);
    }

    #[test]
    fn test_undo_entry_set_cursor_after() {
        let mut entry = UndoEntry::new(Cursor::new(0, 0), false);
        let after = Cursor::new(3, 7);
        entry.set_cursor_after(after);
        assert_eq!(entry.cursor_after(), after);
        // cursor_before unchanged
        assert_eq!(entry.cursor_before(), Cursor::new(0, 0));
    }

    #[test]
    fn test_undo_entry_modified_before() {
        let entry_clean = UndoEntry::new(Cursor::new(0, 0), false);
        assert!(!entry_clean.modified_before());

        let entry_dirty = UndoEntry::new(Cursor::new(0, 0), true);
        assert!(entry_dirty.modified_before());
    }

    #[test]
    fn test_edit_action_clone() {
        let action = EditAction::InsertChar {
            row: 1,
            col: 2,
            ch: 'x',
        };
        let cloned = action.clone();
        match cloned {
            EditAction::InsertChar { row, col, ch } => {
                assert_eq!(row, 1);
                assert_eq!(col, 2);
                assert_eq!(ch, 'x');
            }
            _ => panic!("Expected InsertChar"),
        }
    }

    #[test]
    fn test_edit_action_debug() {
        let action = EditAction::DeleteRange {
            start_row: 0,
            start_col: 0,
            end_row: 2,
            end_col: 5,
            text: "hello\nworld\ntest!".to_string(),
        };
        let debug_str = format!("{:?}", action);
        assert!(debug_str.contains("DeleteRange"));
    }

    #[test]
    fn test_edit_action_replace_char() {
        let action = EditAction::ReplaceChar {
            row: 0,
            col: 0,
            old_char: 'a',
            new_char: 'b',
        };
        match action {
            EditAction::ReplaceChar {
                old_char, new_char, ..
            } => {
                assert_eq!(old_char, 'a');
                assert_eq!(new_char, 'b');
            }
            _ => panic!("Expected ReplaceChar"),
        }
    }

    #[test]
    fn test_edit_action_replace_line() {
        let action = EditAction::ReplaceLine {
            row: 3,
            old_content: "hello world".to_string(),
            new_content: "hello rust".to_string(),
        };
        match action {
            EditAction::ReplaceLine {
                row,
                old_content,
                new_content,
            } => {
                assert_eq!(row, 3);
                assert_eq!(old_content, "hello world");
                assert_eq!(new_content, "hello rust");
            }
            _ => panic!("Expected ReplaceLine"),
        }
    }

    #[test]
    fn test_edit_action_all_variants_constructable() {
        // Verify all variants can be constructed without panic
        let _actions = vec![
            EditAction::InsertChar {
                row: 0,
                col: 0,
                ch: 'a',
            },
            EditAction::InsertNewline { row: 0, col: 5 },
            EditAction::DeleteChar {
                row: 0,
                col: 0,
                ch: 'x',
            },
            EditAction::DeleteRange {
                start_row: 0,
                start_col: 0,
                end_row: 1,
                end_col: 3,
                text: "abc\ndef".to_string(),
            },
            EditAction::InsertText {
                row: 0,
                col: 0,
                text: "hello".to_string(),
            },
            EditAction::InsertLine {
                row: 0,
                content: "line".to_string(),
            },
            EditAction::RemoveLine {
                row: 0,
                content: "removed".to_string(),
            },
            EditAction::JoinLines { row: 0, col: 5 },
            EditAction::SplitLine { row: 0, col: 3 },
            EditAction::ReplaceChar {
                row: 0,
                col: 0,
                old_char: 'a',
                new_char: 'b',
            },
            EditAction::ReplaceLine {
                row: 0,
                old_content: "old".to_string(),
                new_content: "new".to_string(),
            },
        ];
    }
}

//! Linear undo/redo history stack.
//!
//! This module provides `UndoHistory`, which manages the undo and redo stacks
//! using a linear (non-branching) model. New edits after an undo clear the
//! redo stack, matching standard vi behavior.

use std::collections::VecDeque;

use super::action::{EditAction, UndoEntry};
use crate::buffer::Cursor;

/// Default maximum number of entries in the undo stack.
const DEFAULT_MAX_ENTRIES: usize = 1000;

/// Linear undo/redo history.
///
/// Manages two stacks (undo and redo) plus an optional in-progress group
/// for collecting actions during multi-action commands (e.g., an entire
/// insert mode session).
///
/// # Grouping Lifecycle
///
/// 1. Call [`begin_group`](Self::begin_group) before editing.
/// 2. Call [`record`](Self::record) for each atomic edit action.
/// 3. Call [`end_group`](Self::end_group) after editing completes.
///
/// Empty groups (no recorded actions) are silently discarded.
#[derive(Debug)]
pub struct UndoHistory {
    /// Completed entries that can be undone.
    undo_stack: VecDeque<UndoEntry>,
    /// Undone entries that can be redone.
    redo_stack: VecDeque<UndoEntry>,
    /// In-progress group (active during insert mode or multi-step commands).
    current_group: Option<UndoEntry>,
    /// Memory limit on undo stack size.
    max_entries: usize,
}

impl UndoHistory {
    /// Create a new empty undo history.
    pub fn new() -> Self {
        Self {
            undo_stack: VecDeque::new(),
            redo_stack: VecDeque::new(),
            current_group: None,
            max_entries: DEFAULT_MAX_ENTRIES,
        }
    }

    /// Start a new undo group.
    ///
    /// Must be called before recording actions. If a group is already active
    /// (e.g., from a nested call), the existing group is finalized first as
    /// a safety net.
    pub fn begin_group(&mut self, cursor: Cursor, modified: bool) {
        // Finalize any existing group to prevent data loss
        if let Some(group) = self.current_group.take() {
            if !group.is_empty() {
                self.push_undo(group);
            }
        }
        self.current_group = Some(UndoEntry::new(cursor, modified));
    }

    /// Record an action into the current group.
    ///
    /// If no group is active, the action is silently dropped. This avoids
    /// panics in edge cases while ensuring correct usage records actions.
    pub fn record(&mut self, action: EditAction) {
        if let Some(group) = &mut self.current_group {
            group.push(action);
        }
    }

    /// End the current group and push it to the undo stack.
    ///
    /// Sets the group's cursor_after to the provided position. Empty groups
    /// are discarded. New edits after a previous undo clear the redo stack
    /// (linear model -- no branching).
    pub fn end_group(&mut self, cursor_after: Cursor) {
        if let Some(mut group) = self.current_group.take() {
            group.set_cursor_after(cursor_after);
            if !group.is_empty() {
                self.redo_stack.clear();
                self.push_undo(group);
            }
        }
    }

    /// Pop the most recent undo entry.
    ///
    /// Returns `None` if the undo stack is empty. The popped entry is
    /// also pushed onto the redo stack.
    pub fn undo(&mut self) -> Option<UndoEntry> {
        let entry = self.undo_stack.pop_back()?;
        self.redo_stack.push_back(entry.clone());
        Some(entry)
    }

    /// Pop the most recent redo entry.
    ///
    /// Returns `None` if the redo stack is empty. The popped entry is
    /// also pushed onto the undo stack.
    pub fn redo(&mut self) -> Option<UndoEntry> {
        let entry = self.redo_stack.pop_back()?;
        self.undo_stack.push_back(entry.clone());
        Some(entry)
    }

    /// Check if there are entries available to undo.
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Check if there are entries available to redo.
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Check if a group is currently being recorded.
    pub fn is_recording(&self) -> bool {
        self.current_group.is_some()
    }

    /// Push an entry onto the undo stack, enforcing the max_entries limit.
    ///
    /// When the stack exceeds the limit, the oldest entry is removed.
    fn push_undo(&mut self, entry: UndoEntry) {
        if self.undo_stack.len() >= self.max_entries {
            self.undo_stack.pop_front();
        }
        self.undo_stack.push_back(entry);
    }
}

impl Default for UndoHistory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::undo::action::EditAction;

    #[test]
    fn test_new_history_is_empty() {
        let history = UndoHistory::new();
        assert!(!history.can_undo());
        assert!(!history.can_redo());
        assert!(!history.is_recording());
    }

    #[test]
    fn test_default_is_new() {
        let history = UndoHistory::default();
        assert!(!history.can_undo());
        assert!(!history.can_redo());
    }

    #[test]
    fn test_begin_group_starts_recording() {
        let mut history = UndoHistory::new();
        history.begin_group(Cursor::new(0, 0), false);
        assert!(history.is_recording());
    }

    #[test]
    fn test_end_group_stops_recording() {
        let mut history = UndoHistory::new();
        history.begin_group(Cursor::new(0, 0), false);
        history.record(EditAction::InsertChar {
            row: 0,
            col: 0,
            ch: 'a',
        });
        history.end_group(Cursor::new(0, 1));
        assert!(!history.is_recording());
    }

    #[test]
    fn test_empty_group_discarded() {
        let mut history = UndoHistory::new();
        history.begin_group(Cursor::new(0, 0), false);
        // No actions recorded
        history.end_group(Cursor::new(0, 0));
        assert!(!history.can_undo());
    }

    #[test]
    fn test_single_action_undo() {
        let mut history = UndoHistory::new();

        history.begin_group(Cursor::new(0, 0), false);
        history.record(EditAction::InsertChar {
            row: 0,
            col: 0,
            ch: 'a',
        });
        history.end_group(Cursor::new(0, 1));

        assert!(history.can_undo());
        let entry = history.undo().unwrap();
        assert_eq!(entry.actions().len(), 1);
        assert_eq!(entry.cursor_before(), Cursor::new(0, 0));
        assert_eq!(entry.cursor_after(), Cursor::new(0, 1));
        assert!(!entry.modified_before());
    }

    #[test]
    fn test_multi_action_group() {
        let mut history = UndoHistory::new();

        history.begin_group(Cursor::new(0, 0), false);
        history.record(EditAction::InsertChar {
            row: 0,
            col: 0,
            ch: 'h',
        });
        history.record(EditAction::InsertChar {
            row: 0,
            col: 1,
            ch: 'i',
        });
        history.end_group(Cursor::new(0, 2));

        let entry = history.undo().unwrap();
        assert_eq!(entry.actions().len(), 2);
    }

    #[test]
    fn test_undo_then_redo() {
        let mut history = UndoHistory::new();

        history.begin_group(Cursor::new(0, 0), false);
        history.record(EditAction::InsertChar {
            row: 0,
            col: 0,
            ch: 'x',
        });
        history.end_group(Cursor::new(0, 1));

        // Undo
        assert!(history.can_undo());
        let entry = history.undo().unwrap();
        assert_eq!(entry.cursor_before(), Cursor::new(0, 0));
        assert!(!history.can_undo());
        assert!(history.can_redo());

        // Redo
        let entry = history.redo().unwrap();
        assert_eq!(entry.cursor_after(), Cursor::new(0, 1));
        assert!(history.can_undo());
        assert!(!history.can_redo());
    }

    #[test]
    fn test_new_edit_clears_redo_stack() {
        let mut history = UndoHistory::new();

        // First edit
        history.begin_group(Cursor::new(0, 0), false);
        history.record(EditAction::InsertChar {
            row: 0,
            col: 0,
            ch: 'a',
        });
        history.end_group(Cursor::new(0, 1));

        // Undo it
        history.undo();
        assert!(history.can_redo());

        // New edit should clear redo
        history.begin_group(Cursor::new(0, 0), false);
        history.record(EditAction::InsertChar {
            row: 0,
            col: 0,
            ch: 'b',
        });
        history.end_group(Cursor::new(0, 1));

        assert!(!history.can_redo());
    }

    #[test]
    fn test_multiple_undos_and_redos() {
        let mut history = UndoHistory::new();

        // Three edits
        for ch in ['a', 'b', 'c'] {
            history.begin_group(Cursor::new(0, 0), false);
            history.record(EditAction::InsertChar { row: 0, col: 0, ch });
            history.end_group(Cursor::new(0, 1));
        }

        // Undo all three
        assert!(history.undo().is_some()); // undo 'c'
        assert!(history.undo().is_some()); // undo 'b'
        assert!(history.undo().is_some()); // undo 'a'
        assert!(history.undo().is_none()); // nothing left

        // Redo all three
        assert!(history.redo().is_some());
        assert!(history.redo().is_some());
        assert!(history.redo().is_some());
        assert!(history.redo().is_none());
    }

    #[test]
    fn test_undo_empty_returns_none() {
        let mut history = UndoHistory::new();
        assert!(history.undo().is_none());
    }

    #[test]
    fn test_redo_empty_returns_none() {
        let mut history = UndoHistory::new();
        assert!(history.redo().is_none());
    }

    #[test]
    fn test_record_without_group_is_silently_dropped() {
        let mut history = UndoHistory::new();
        // No begin_group called
        history.record(EditAction::InsertChar {
            row: 0,
            col: 0,
            ch: 'a',
        });
        // Should not panic or add to stack
        assert!(!history.can_undo());
    }

    #[test]
    fn test_begin_group_finalizes_existing() {
        let mut history = UndoHistory::new();

        // Start first group with actions
        history.begin_group(Cursor::new(0, 0), false);
        history.record(EditAction::InsertChar {
            row: 0,
            col: 0,
            ch: 'a',
        });

        // Start second group without ending first -- first should be finalized
        history.begin_group(Cursor::new(0, 1), false);
        history.record(EditAction::InsertChar {
            row: 0,
            col: 1,
            ch: 'b',
        });
        history.end_group(Cursor::new(0, 2));

        // Both groups should be on the undo stack
        assert!(history.can_undo());
        let entry2 = history.undo().unwrap();
        assert_eq!(entry2.actions().len(), 1); // second group

        assert!(history.can_undo());
        let entry1 = history.undo().unwrap();
        assert_eq!(entry1.actions().len(), 1); // first group (auto-finalized)
    }

    #[test]
    fn test_max_entries_enforced() {
        let mut history = UndoHistory::new();
        // Override max_entries for testing
        history.max_entries = 3;

        for i in 0..5 {
            history.begin_group(Cursor::new(i, 0), false);
            history.record(EditAction::InsertChar {
                row: i,
                col: 0,
                ch: 'a',
            });
            history.end_group(Cursor::new(i, 1));
        }

        // Only last 3 entries should remain
        assert_eq!(history.undo_stack.len(), 3);

        // The oldest entries (row 0 and 1) should have been dropped
        let entry = history.undo().unwrap();
        match &entry.actions()[0] {
            EditAction::InsertChar { row, .. } => assert_eq!(*row, 4),
            _ => panic!("Expected InsertChar"),
        }
    }

    #[test]
    fn test_modified_before_preserved() {
        let mut history = UndoHistory::new();

        history.begin_group(Cursor::new(0, 0), true);
        history.record(EditAction::InsertChar {
            row: 0,
            col: 0,
            ch: 'a',
        });
        history.end_group(Cursor::new(0, 1));

        let entry = history.undo().unwrap();
        assert!(entry.modified_before());
    }

    #[test]
    fn test_cursor_positions_preserved() {
        let mut history = UndoHistory::new();
        let before = Cursor::new(5, 10);
        let after = Cursor::new(7, 3);

        history.begin_group(before, false);
        history.record(EditAction::InsertChar {
            row: 5,
            col: 10,
            ch: 'z',
        });
        history.end_group(after);

        let entry = history.undo().unwrap();
        assert_eq!(entry.cursor_before(), before);
        assert_eq!(entry.cursor_after(), after);
    }

    #[test]
    fn test_undo_redo_cycle_preserves_entry() {
        let mut history = UndoHistory::new();

        history.begin_group(Cursor::new(0, 0), false);
        history.record(EditAction::InsertChar {
            row: 0,
            col: 0,
            ch: 'q',
        });
        history.end_group(Cursor::new(0, 1));

        // Undo and redo multiple times
        for _ in 0..3 {
            let entry = history.undo().unwrap();
            assert_eq!(entry.actions().len(), 1);
            let entry = history.redo().unwrap();
            assert_eq!(entry.actions().len(), 1);
        }
    }
}

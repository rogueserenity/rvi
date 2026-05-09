//! Dot command repeat system.
//!
//! This module defines the [`RepeatableChange`] enum that captures the last
//! buffer-modifying command at a semantic level. The dot command (`.`) replays
//! the captured command through the existing execution machinery, producing
//! context-appropriate results at the new cursor position.
//!
//! # Design Principles
//!
//! - **Store commands, not actions**: The dot command records what the user
//!   requested (e.g., "delete a word"), not what happened to the buffer.
//! - **Count replacement**: A new count from `.` replaces the original count.
//! - **Register preservation**: The register from the original command is reused.
//! - **Independent undo groups**: Each dot replay creates its own undo entry.

use crate::command::parser::{ModeChangeKind, SimpleAction};
use crate::command::text_objects::{TextObject, TextObjectKind};
use crate::command::Motion;
use crate::mode::Operator;
use crate::terminal::Key;

/// A recorded change that can be replayed by the dot command.
///
/// Each variant captures enough information to faithfully re-execute a
/// buffer-modifying command at a new cursor position.
#[derive(Debug, Clone)]
pub enum RepeatableChange {
    /// A simple normal-mode action (x, X, D, r{ch}).
    ///
    /// Does NOT include C (which is modeled as `OperatorMotion` with
    /// Change operator and LineEnd motion).
    SimpleAction {
        /// The action to repeat.
        action: SimpleAction,
        /// Explicit register, if any.
        register: Option<char>,
        /// Original count.
        count: usize,
    },

    /// An operator applied to a motion (dw, d$, dd, cw, cc, etc.),
    /// with optional insert keys for the Change operator.
    OperatorMotion {
        /// The operator (Delete, Change, Yank).
        operator: Operator,
        /// The motion defining the range.
        motion: Motion,
        /// Original count (can be replaced by dot count).
        count: usize,
        /// Explicit register, if any.
        register: Option<char>,
        /// For Change operator: the keys typed in insert mode after the delete.
        insert_keys: Option<Vec<Key>>,
        /// Whether this was a linewise operator (dd, cc) vs motion-based (dw, cw).
        linewise_self: bool,
    },

    /// An operator applied to a text object (daw, ci", diw, etc.),
    /// with optional insert keys for the Change operator.
    OperatorTextObject {
        /// The operator (Delete, Change, Yank).
        operator: Operator,
        /// Around or Inner.
        kind: TextObjectKind,
        /// The text object type.
        object: TextObject,
        /// Original count.
        count: usize,
        /// Explicit register, if any.
        register: Option<char>,
        /// For Change operator: the keys typed in insert mode after the delete.
        insert_keys: Option<Vec<Key>>,
    },

    /// A pure insert mode session (entered via i, a, o, O, A, I).
    InsertSession {
        /// How insert mode was entered.
        entry_kind: ModeChangeKind,
        /// The keys typed during the session (before Esc).
        typed_keys: Vec<Key>,
        /// The count from when insert mode was entered (e.g. 3 for 3ifoo<Esc>).
        /// The inserted text is replayed this many times on dot repeat.
        count: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repeatable_change_simple_action_debug() {
        let change = RepeatableChange::SimpleAction {
            action: SimpleAction::DeleteCharAtCursor,
            register: None,
            count: 1,
        };
        let debug_str = format!("{:?}", change);
        assert!(debug_str.contains("DeleteCharAtCursor"));
    }

    #[test]
    fn test_repeatable_change_clone() {
        let change = RepeatableChange::SimpleAction {
            action: SimpleAction::ReplaceChar('x'),
            register: Some('a'),
            count: 3,
        };
        let cloned = change.clone();
        match cloned {
            RepeatableChange::SimpleAction {
                action,
                register,
                count,
            } => {
                assert_eq!(action, SimpleAction::ReplaceChar('x'));
                assert_eq!(register, Some('a'));
                assert_eq!(count, 3);
            }
            _ => panic!("Expected SimpleAction variant"),
        }
    }

    #[test]
    fn test_repeatable_change_operator_motion() {
        let change = RepeatableChange::OperatorMotion {
            operator: Operator::Delete,
            motion: Motion::WordForward,
            count: 2,
            register: None,
            insert_keys: None,
            linewise_self: false,
        };
        match change {
            RepeatableChange::OperatorMotion {
                operator,
                motion,
                count,
                ..
            } => {
                assert_eq!(operator, Operator::Delete);
                assert_eq!(motion, Motion::WordForward);
                assert_eq!(count, 2);
            }
            _ => panic!("Expected OperatorMotion variant"),
        }
    }

    #[test]
    fn test_repeatable_change_operator_text_object() {
        let change = RepeatableChange::OperatorTextObject {
            operator: Operator::Change,
            kind: TextObjectKind::Inner,
            object: TextObject::Word,
            count: 1,
            register: None,
            insert_keys: Some(vec![Key::Char('n'), Key::Char('e'), Key::Char('w')]),
        };
        match change {
            RepeatableChange::OperatorTextObject {
                operator,
                kind,
                object,
                insert_keys,
                ..
            } => {
                assert_eq!(operator, Operator::Change);
                assert_eq!(kind, TextObjectKind::Inner);
                assert_eq!(object, TextObject::Word);
                assert_eq!(insert_keys.unwrap().len(), 3);
            }
            _ => panic!("Expected OperatorTextObject variant"),
        }
    }

    #[test]
    fn test_repeatable_change_insert_session() {
        let change = RepeatableChange::InsertSession {
            entry_kind: ModeChangeKind::InsertBeforeCursor,
            typed_keys: vec![Key::Char('h'), Key::Char('i')],
            count: 1,
        };
        match change {
            RepeatableChange::InsertSession {
                entry_kind,
                typed_keys,
                count,
            } => {
                assert_eq!(entry_kind, ModeChangeKind::InsertBeforeCursor);
                assert_eq!(typed_keys.len(), 2);
                assert_eq!(count, 1);
            }
            _ => panic!("Expected InsertSession variant"),
        }
    }

    #[test]
    fn test_repeatable_change_operator_motion_with_insert_keys() {
        let change = RepeatableChange::OperatorMotion {
            operator: Operator::Change,
            motion: Motion::WordForward,
            count: 1,
            register: None,
            insert_keys: Some(vec![Key::Char('a'), Key::Char('b')]),
            linewise_self: false,
        };
        match change {
            RepeatableChange::OperatorMotion {
                operator,
                insert_keys,
                ..
            } => {
                assert_eq!(operator, Operator::Change);
                let keys = insert_keys.unwrap();
                assert_eq!(keys.len(), 2);
                assert_eq!(keys[0], Key::Char('a'));
            }
            _ => panic!("Expected OperatorMotion variant"),
        }
    }

    #[test]
    fn test_repeatable_change_linewise_self() {
        let change = RepeatableChange::OperatorMotion {
            operator: Operator::Delete,
            motion: Motion::Down,
            count: 3,
            register: Some('a'),
            insert_keys: None,
            linewise_self: true,
        };
        match change {
            RepeatableChange::OperatorMotion {
                linewise_self,
                count,
                register,
                ..
            } => {
                assert!(linewise_self);
                assert_eq!(count, 3);
                assert_eq!(register, Some('a'));
            }
            _ => panic!("Expected OperatorMotion variant"),
        }
    }
}

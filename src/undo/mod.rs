//! Undo/redo system for the editor.
//!
//! This module implements a linear (non-branching) undo/redo system using
//! an action-based approach. Each editing operation records an [`EditAction`]
//! that captures enough information to reverse it. Actions are grouped into
//! [`UndoEntry`] units that match vi's undo granularity:
//!
//! - **Normal mode**: each command is one undo entry.
//! - **Insert mode**: all edits between entering and exiting insert mode
//!   form one undo entry.
//!
//! The [`UndoHistory`] struct manages the undo and redo stacks and enforces
//! a configurable entry limit for bounded memory usage.

pub mod action;
pub mod history;

pub use action::{EditAction, UndoEntry};
pub use history::UndoHistory;

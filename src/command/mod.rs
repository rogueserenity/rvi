//! Command parsing and motion execution module.
//!
//! This module provides:
//! - `Motion` enum for all vi motion types
//! - `ParseState` for tracking multi-key command state
//! - `ParsedCommand` for fully parsed commands ready for execution
//! - Motion execution functions that calculate target positions and ranges
//! - Text objects for semantic text selection (words, quotes, brackets)
//! - Ex command parsing for command-line mode (:w, :q, :wq, :set, etc.)
//! - `RepeatableChange` enum for dot command (`.`) replay

pub mod ex_command;
mod ex_range;
pub mod motion;
pub mod parser;
pub mod repeat;
pub mod text_objects;

pub use ex_command::{parse_ex_command, CommandLineState, ExCommand, ExCommandContext};
pub use motion::{
    execute_motion, CommandContext, FindDirection, FindStop, Motion, MotionKind, MotionRange,
    MotionResult,
};
pub use parser::{
    parse_normal_key, parse_pending, parse_text_object_key, ModeChangeKind, ParseResult,
    ParseState, ParsedCommand, PendingTextObject, SimpleAction,
};
pub use repeat::RepeatableChange;
pub use text_objects::{
    parse_text_object_specifier, resolve_text_object, TextObject, TextObjectContext, TextObjectKind,
};

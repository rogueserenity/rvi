//! Editor mode module.
//!
//! This module defines the editing modes and related types for the vi editor.
//! Modes determine how key presses are interpreted.

/// Editor mode.
///
/// The current mode determines how key presses are interpreted:
/// - Normal: Navigation and command entry
/// - Insert: Text entry
/// - Replace: Overwrite existing text
/// - OperatorPending: Waiting for motion after operator (Phase 2+)
/// - Visual: Visual selection mode (Phase 2+)
/// - CommandLine: Command-line input mode (Phase 2+)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Normal mode for navigation and commands.
    #[default]
    Normal,

    /// Insert mode for text entry.
    Insert,

    /// Replace mode for overwriting existing text.
    Replace,

    /// Operator-pending mode - waiting for motion after an operator like 'd', 'c', 'y' (Phase 2+).
    OperatorPending(Operator),

    /// Visual selection mode (Phase 2+).
    Visual(VisualKind),

    /// Command-line mode for :commands (Phase 2+).
    CommandLine,
}

impl Mode {
    /// Get a display string for the mode.
    pub fn as_str(&self) -> &'static str {
        match self {
            Mode::Normal => "NORMAL",
            Mode::Insert => "INSERT",
            Mode::Replace => "REPLACE",
            Mode::OperatorPending(_) => "OP-PENDING",
            Mode::Visual(kind) => match kind {
                VisualKind::Character => "VISUAL",
                VisualKind::Line => "VISUAL LINE",
                VisualKind::Block => "VISUAL BLOCK",
            },
            Mode::CommandLine => "COMMAND",
        }
    }

    /// Check if the mode is a visual mode variant.
    pub fn is_visual(&self) -> bool {
        matches!(self, Mode::Visual(_))
    }

    /// Check if the mode allows cursor past end of line.
    pub fn allows_cursor_past_end(&self) -> bool {
        matches!(self, Mode::Insert | Mode::Replace | Mode::CommandLine)
    }
}

/// Operator type for operator-pending mode (Phase 2+).
///
/// These are the operators that wait for a motion or text object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    /// Delete operator (d)
    Delete,

    /// Yank (copy) operator (y)
    Yank,

    /// Change operator (c)
    Change,

    /// Indent left operator (<)
    IndentLeft,

    /// Indent right operator (>)
    IndentRight,

    /// Format operator (gq)
    Format,

    /// Uppercase operator (gU)
    Uppercase,

    /// Lowercase operator (gu)
    Lowercase,

    /// Toggle case operator (g~)
    ToggleCase,

    /// Filter through external program (!)
    Filter,
}

impl Operator {
    /// Get the character representation of the operator.
    pub fn as_char(&self) -> char {
        match self {
            Operator::Delete => 'd',
            Operator::Yank => 'y',
            Operator::Change => 'c',
            Operator::IndentLeft => '<',
            Operator::IndentRight => '>',
            Operator::Format => 'q',     // Part of gq
            Operator::Uppercase => 'U',  // Part of gU
            Operator::Lowercase => 'u',  // Part of gu
            Operator::ToggleCase => '~', // Part of g~
            Operator::Filter => '!',
        }
    }

    /// Check if the operator causes mode change after execution.
    pub fn enters_insert_mode(&self) -> bool {
        matches!(self, Operator::Change)
    }
}

/// Visual mode variant (Phase 2+).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualKind {
    /// Character-wise visual mode (v)
    Character,

    /// Line-wise visual mode (V)
    Line,

    /// Block-wise visual mode (Ctrl-V)
    Block,
}

impl VisualKind {
    /// Get the character that enters this visual mode.
    pub fn trigger_char(&self) -> char {
        match self {
            VisualKind::Character => 'v',
            VisualKind::Line => 'V',
            VisualKind::Block => '\x16', // Ctrl-V
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mode_as_str() {
        assert_eq!(Mode::Normal.as_str(), "NORMAL");
        assert_eq!(Mode::Insert.as_str(), "INSERT");
        assert_eq!(Mode::Replace.as_str(), "REPLACE");
        assert_eq!(Mode::CommandLine.as_str(), "COMMAND");
        assert_eq!(
            Mode::OperatorPending(Operator::Delete).as_str(),
            "OP-PENDING"
        );
        assert_eq!(Mode::Visual(VisualKind::Character).as_str(), "VISUAL");
        assert_eq!(Mode::Visual(VisualKind::Line).as_str(), "VISUAL LINE");
        assert_eq!(Mode::Visual(VisualKind::Block).as_str(), "VISUAL BLOCK");
    }

    #[test]
    fn test_mode_is_visual() {
        assert!(!Mode::Normal.is_visual());
        assert!(!Mode::Insert.is_visual());
        assert!(Mode::Visual(VisualKind::Character).is_visual());
        assert!(Mode::Visual(VisualKind::Line).is_visual());
        assert!(Mode::Visual(VisualKind::Block).is_visual());
    }

    #[test]
    fn test_mode_allows_cursor_past_end() {
        assert!(!Mode::Normal.allows_cursor_past_end());
        assert!(Mode::Insert.allows_cursor_past_end());
        assert!(Mode::Replace.allows_cursor_past_end());
        assert!(Mode::CommandLine.allows_cursor_past_end());
        assert!(!Mode::Visual(VisualKind::Character).allows_cursor_past_end());
    }

    #[test]
    fn test_operator_as_char() {
        assert_eq!(Operator::Delete.as_char(), 'd');
        assert_eq!(Operator::Yank.as_char(), 'y');
        assert_eq!(Operator::Change.as_char(), 'c');
    }

    #[test]
    fn test_operator_enters_insert_mode() {
        assert!(!Operator::Delete.enters_insert_mode());
        assert!(!Operator::Yank.enters_insert_mode());
        assert!(Operator::Change.enters_insert_mode());
    }

    #[test]
    fn test_visual_kind_trigger() {
        assert_eq!(VisualKind::Character.trigger_char(), 'v');
        assert_eq!(VisualKind::Line.trigger_char(), 'V');
    }

    #[test]
    fn test_mode_default() {
        assert_eq!(Mode::default(), Mode::Normal);
    }
}

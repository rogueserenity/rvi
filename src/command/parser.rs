//! Command parsing for vi normal mode.
//!
//! This module parses key sequences into executable commands. It handles:
//! - Count prefixes (e.g., 3j)
//! - Register prefixes (e.g., "ay)
//! - Multi-key sequences (e.g., gg, fx)
//! - Operators and motions
//! - Text objects (e.g., daw, ci")

use crate::mode::{Mode, Operator};
use crate::terminal::Key;

use super::motion::{FindDirection, FindStop, Motion};
use super::text_objects::{parse_text_object_specifier, TextObject, TextObjectKind};

/// State for parsing multi-key commands.
#[derive(Debug, Clone, Default)]
pub struct ParseState {
    /// Accumulated count prefix (e.g., 3 in 3dd)
    count: Option<usize>,
    /// Register prefix (e.g., 'a' in "ayy)
    register: Option<char>,
    /// Pending characters for multi-key sequences
    pending: Vec<char>,
    /// Last find motion for ; and , repeat
    last_find: Option<(char, FindDirection, FindStop)>,
    /// Operator count stored when entering operator-pending mode (e.g., 3 in 3d2w)
    operator_count: Option<usize>,
    /// Register stored when entering operator-pending mode
    operator_register: Option<char>,
    /// Pending text object kind (set after 'a' or 'i' in operator-pending mode)
    pending_text_object: Option<PendingTextObject>,
}

/// State for text object parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingTextObject {
    /// Waiting for text object specifier after 'a'
    Around,
    /// Waiting for text object specifier after 'i'
    Inner,
}

impl ParseState {
    /// Create a new parse state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset the parse state (called after command execution).
    ///
    /// Note: `last_find` persists across commands for ; and , repeat.
    /// Note: `operator_count` and `operator_register` also persist until the operator is executed.
    pub fn reset(&mut self) {
        self.count = None;
        self.register = None;
        self.pending.clear();
        self.pending_text_object = None;
    }

    /// Full reset including operator count (called after operator execution).
    pub fn reset_all(&mut self) {
        self.reset();
        self.operator_count = None;
        self.operator_register = None;
    }

    /// Get the effective count (defaults to 1).
    pub fn count(&self) -> usize {
        self.count.unwrap_or(1)
    }

    /// Get the raw count if specified.
    pub fn count_opt(&self) -> Option<usize> {
        self.count
    }

    /// Get the register if specified.
    pub fn register(&self) -> Option<char> {
        self.register
    }

    /// Check if we're in the middle of a multi-key command.
    pub fn is_pending(&self) -> bool {
        !self.pending.is_empty() || self.pending_text_object.is_some()
    }

    /// Get the last find motion parameters.
    pub fn last_find(&self) -> Option<(char, FindDirection, FindStop)> {
        self.last_find
    }

    /// Set the last find motion parameters.
    pub fn set_last_find(&mut self, ch: char, direction: FindDirection, stop: FindStop) {
        self.last_find = Some((ch, direction, stop));
    }

    /// Store the operator count when entering operator-pending mode.
    pub fn set_operator_count(&mut self, count: usize) {
        self.operator_count = Some(count);
    }

    /// Get the operator count (defaults to 1 if not set).
    pub fn operator_count(&self) -> usize {
        self.operator_count.unwrap_or(1)
    }

    /// Clear the operator count after execution.
    pub fn clear_operator_count(&mut self) {
        self.operator_count = None;
    }

    /// Store the operator register when entering operator-pending mode.
    pub fn set_operator_register(&mut self, register: Option<char>) {
        self.operator_register = register;
    }

    /// Get the operator register.
    pub fn operator_register(&self) -> Option<char> {
        self.operator_register
    }

    /// Clear the operator register after execution.
    pub fn clear_operator_register(&mut self) {
        self.operator_register = None;
    }

    /// Get the pending text object kind.
    pub fn pending_text_object(&self) -> Option<PendingTextObject> {
        self.pending_text_object
    }

    /// Set the pending text object kind.
    pub fn set_pending_text_object(&mut self, kind: PendingTextObject) {
        self.pending_text_object = Some(kind);
    }
}

/// Kind of mode change command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeChangeKind {
    /// Insert before cursor (i)
    InsertBeforeCursor,
    /// Insert after cursor (a)
    InsertAfterCursor,
    /// Insert at end of line (A)
    InsertAtLineEnd,
    /// Insert at first non-blank (I)
    InsertAtFirstNonBlank,
    /// Open line below (o)
    OpenLineBelow,
    /// Open line above (O)
    OpenLineAbove,
    /// Replace mode (R) - Phase 3+
    ReplaceMode,
}

/// Simple action that doesn't involve motions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimpleAction {
    /// Delete character at cursor (x)
    DeleteCharAtCursor,
    /// Delete character before cursor (X)
    DeleteCharBeforeCursor,
    /// Replace character under cursor (r{char})
    ReplaceChar(char),
    /// Undo (u)
    Undo,
    /// Redo (Ctrl-r)
    Redo,
    /// Put after cursor (p)
    PutAfter,
    /// Put before cursor (P)
    PutBefore,
    /// Delete from cursor to end of line (D), equivalent to d$
    DeleteToEndOfLine,
    /// Change from cursor to end of line (C), equivalent to c$
    ChangeToEndOfLine,
    /// Yank entire current line (Y), equivalent to yy
    YankLine,
    /// Repeat last search in same direction (n)
    SearchNext,
    /// Repeat last search in opposite direction (N)
    SearchPrev,
    /// Toggle case of character under cursor (~), advance cursor
    ToggleCase,
    /// Join next line to current line (J)
    JoinLines,
    /// Search word under cursor forward (*)
    SearchWordForward,
    /// Search word under cursor backward (#)
    SearchWordBackward,
    /// Set mark at current cursor position (m{a-z})
    SetMark(char),
    /// Jump to exact mark position (`{a-z}) or previous position (``)
    JumpToMark { mark: char, line_start: bool },
    /// Start or stop recording a macro into the named register (q{a-z} / q)
    ToggleMacroRecord(char),
    /// Play macro stored in named register (@{a-z})
    PlayMacro(char),
    /// Replay the most recently used macro (@@)
    RepeatMacro,
    /// Navigate to an older jump list position (Ctrl-O)
    JumpBack,
    /// Navigate to a newer jump list position (Ctrl-I / Tab)
    JumpForward,
    /// Scroll so cursor line is at top of screen (zt / z↵)
    ScrollCursorTop,
    /// Scroll so cursor line is at middle of screen (zz / z.)
    ScrollCursorMiddle,
    /// Scroll so cursor line is at bottom of screen (zb / z-)
    ScrollCursorBottom,
    /// Show file info in status line (Ctrl-g)
    ShowFileInfo,
    /// Undo all changes to current line (U)
    UndoLine,
    /// Write file and quit (ZZ), equivalent to :x
    WriteQuit,
    /// Quit without saving (ZQ), equivalent to :q!
    ForceQuit,
    /// Join next line to current line without inserting space (gJ)
    JoinLinesNoSpace,
    /// Enter ex mode (Q) -- stub
    EnterExMode,
    /// Repeat last substitute on current line (&)
    RepeatSubstitute,
    /// Edit alternate file (Ctrl-^)
    EditAlternateFile,
}

/// A fully parsed command ready for execution.
#[derive(Debug, Clone, PartialEq)]
pub enum ParsedCommand {
    /// A motion command (with optional count)
    Motion { motion: Motion, count: usize },

    /// An operator waiting for a motion
    OperatorPending {
        operator: Operator,
        count: usize,
        register: Option<char>,
    },

    /// A complete operator-motion or operator-line command
    OperatorMotion {
        operator: Operator,
        motion: Motion,
        count: usize,
        register: Option<char>,
    },

    /// A complete operator-text-object command (e.g., daw, ci")
    OperatorTextObject {
        operator: Operator,
        kind: TextObjectKind,
        object: TextObject,
        count: usize,
        register: Option<char>,
    },

    /// Mode change command (i, a, A, I, o, O, etc.)
    /// The count is the number of times to repeat the inserted text on Esc (e.g. 3ifoo<Esc>).
    ModeChange(ModeChangeKind, usize),

    /// Simple action (x, r, p, P, etc.)
    SimpleAction { action: SimpleAction, count: usize },

    /// Simple action with register (p, P with explicit register)
    SimpleActionWithRegister {
        action: SimpleAction,
        register: Option<char>,
        count: usize,
    },

    /// Repeat last change (.) with optional count override.
    ///
    /// The count is `None` when `.` is pressed without a prefix count,
    /// meaning the original count from the stored change should be used.
    /// When `Some(n)`, the new count replaces the stored count.
    Repeat { count: Option<usize> },

    /// Command incomplete - need more keys
    Incomplete,

    /// Invalid/cancelled command
    Invalid,
}

/// Result of parsing a key in normal mode.
#[derive(Debug, Clone, PartialEq)]
pub enum ParseResult {
    /// Command is complete and ready to execute
    Complete(ParsedCommand),
    /// Need more input (multi-key command in progress)
    Pending,
    /// Invalid key sequence - reset state
    Invalid,
}

/// Parse a key press in normal mode.
///
/// Returns the parse result indicating whether the command is complete,
/// needs more input, or is invalid.
pub fn parse_normal_key(state: &mut ParseState, key: Key, current_mode: Mode) -> ParseResult {
    match key {
        Key::Esc => {
            state.reset();
            ParseResult::Invalid
        }

        // Digit handling: could be count prefix or motion
        Key::Char(c @ '1'..='9') => {
            if state.is_pending() {
                // In the middle of a command - might be count for g commands
                handle_pending_digit(state, c)
            } else {
                // Start of count prefix
                accumulate_count(state, c);
                ParseResult::Pending
            }
        }
        Key::Char('0') => {
            if state.count.is_some() {
                // Part of a count (e.g., 10j)
                accumulate_count(state, '0');
                ParseResult::Pending
            } else if state.is_pending() {
                handle_pending_digit(state, '0')
            } else {
                // Line start motion
                complete_motion(state, Motion::LineStart)
            }
        }

        // Register prefix
        Key::Char('"') => {
            state.pending.push('"');
            ParseResult::Pending
        }

        // Motion keys
        Key::Char('h') | Key::Left | Key::Backspace | Key::Ctrl('h') => {
            complete_motion(state, Motion::Left)
        }
        Key::Char('j') | Key::Down => complete_motion(state, Motion::Down),
        Key::Char('k') | Key::Up => complete_motion(state, Motion::Up),
        Key::Char('l') | Key::Right | Key::Char(' ') => complete_motion(state, Motion::Right),
        Key::Enter | Key::Char('+') => complete_motion(state, Motion::NextLineFirstNonBlank),
        Key::Char('-') => complete_motion(state, Motion::PrevLineFirstNonBlank),
        Key::Char('_') => complete_motion(state, Motion::CurrentLineFirstNonBlank),
        Key::Char('w') => complete_motion(state, Motion::WordForward),
        Key::Char('b') => complete_motion(state, Motion::WordBackward),
        Key::Char('e') => complete_motion(state, Motion::WordEnd),
        Key::Char('W') => complete_motion(state, Motion::WORDForward),
        Key::Char('B') => complete_motion(state, Motion::WORDBackward),
        Key::Char('E') => complete_motion(state, Motion::WORDEnd),
        Key::Char('^') => complete_motion(state, Motion::FirstNonBlank),
        Key::Char('$') => complete_motion(state, Motion::LineEnd),

        // Matching bracket motion
        Key::Char('%') => complete_motion(state, Motion::MatchingBracket),

        // Paragraph motions ({ and })
        Key::Char('{') => complete_motion(state, Motion::ParagraphBackward),
        Key::Char('}') => complete_motion(state, Motion::ParagraphForward),

        // Sentence motions (( and ))
        Key::Char('(') => complete_motion(state, Motion::SentenceBackward),
        Key::Char(')') => complete_motion(state, Motion::SentenceForward),

        // Multi-key motions
        Key::Char('g') => {
            state.pending.push('g');
            ParseResult::Pending
        }
        Key::Char('G') => {
            // G with count goes to line, without goes to end
            // When count is used for GotoLine, it's consumed (count becomes 1)
            if let Some(n) = state.count {
                state.reset();
                ParseResult::Complete(ParsedCommand::Motion {
                    motion: Motion::GotoLine(n),
                    count: 1,
                })
            } else {
                complete_motion(state, Motion::DocumentEnd)
            }
        }

        // Screen position motions (H, M, L)
        Key::Char('H') => {
            let count = state.count.unwrap_or(0);
            state.reset();
            ParseResult::Complete(ParsedCommand::Motion {
                motion: Motion::ScreenTop(count.saturating_sub(1)),
                count: 1,
            })
        }
        Key::Char('M') => complete_motion(state, Motion::ScreenMiddle),
        Key::Char('L') => {
            let count = state.count.unwrap_or(0);
            state.reset();
            ParseResult::Complete(ParsedCommand::Motion {
                motion: Motion::ScreenBottom(count.saturating_sub(1)),
                count: 1,
            })
        }

        // Find motions (need character argument)
        Key::Char('f') => {
            state.pending.push('f');
            ParseResult::Pending
        }
        Key::Char('F') => {
            state.pending.push('F');
            ParseResult::Pending
        }
        Key::Char('t') => {
            state.pending.push('t');
            ParseResult::Pending
        }
        Key::Char('T') => {
            state.pending.push('T');
            ParseResult::Pending
        }
        Key::Char(';') => complete_motion(state, Motion::RepeatFind),
        Key::Char(',') => complete_motion(state, Motion::RepeatFindReverse),

        // Operators
        Key::Char('d') => handle_operator(state, Operator::Delete, current_mode),
        Key::Char('y') => handle_operator(state, Operator::Yank, current_mode),
        Key::Char('c') => handle_operator(state, Operator::Change, current_mode),

        // Shortcut commands: D, C, Y
        Key::Char('D') => complete_simple_with_register(state, SimpleAction::DeleteToEndOfLine),
        Key::Char('C') => complete_simple_with_register(state, SimpleAction::ChangeToEndOfLine),
        Key::Char('Y') => complete_simple_with_register(state, SimpleAction::YankLine),

        // s = cl (substitute char under cursor, enter insert mode)
        // S = cc (substitute whole line, enter insert mode)
        // Both are invalid in operator-pending mode (e.g. `cs` is not a valid command)
        Key::Char('s') => {
            if matches!(current_mode, Mode::OperatorPending(_)) {
                state.reset();
                return ParseResult::Invalid;
            }
            let count = state.count();
            let register = state.register;
            state.reset();
            ParseResult::Complete(ParsedCommand::OperatorMotion {
                operator: Operator::Change,
                motion: Motion::Right,
                count,
                register,
            })
        }
        Key::Char('S') => {
            if matches!(current_mode, Mode::OperatorPending(_)) {
                state.reset();
                return ParseResult::Invalid;
            }
            let count = state.count();
            let register = state.register;
            state.reset();
            ParseResult::Complete(ParsedCommand::OperatorMotion {
                operator: Operator::Change,
                motion: Motion::Down,
                count,
                register,
            })
        }

        // Mode changes - only in Normal mode (not operator-pending)
        Key::Char('i') => {
            if matches!(current_mode, Mode::OperatorPending(_)) {
                // In operator-pending mode, 'i' means "inner" text object
                state.set_pending_text_object(PendingTextObject::Inner);
                ParseResult::Pending
            } else {
                complete_mode_change(state, ModeChangeKind::InsertBeforeCursor)
            }
        }
        Key::Char('a') => {
            if matches!(current_mode, Mode::OperatorPending(_)) {
                // In operator-pending mode, 'a' means "around" text object
                state.set_pending_text_object(PendingTextObject::Around);
                ParseResult::Pending
            } else {
                complete_mode_change(state, ModeChangeKind::InsertAfterCursor)
            }
        }
        Key::Char('A') => complete_mode_change(state, ModeChangeKind::InsertAtLineEnd),
        Key::Char('I') => complete_mode_change(state, ModeChangeKind::InsertAtFirstNonBlank),
        Key::Char('o') => complete_mode_change(state, ModeChangeKind::OpenLineBelow),
        Key::Char('O') => complete_mode_change(state, ModeChangeKind::OpenLineAbove),
        Key::Char('R') => complete_mode_change(state, ModeChangeKind::ReplaceMode),

        // Simple actions
        Key::Char('x') => complete_simple_with_register(state, SimpleAction::DeleteCharAtCursor),
        Key::Char('X') => {
            complete_simple_with_register(state, SimpleAction::DeleteCharBeforeCursor)
        }
        // `u` is undo in Normal mode, but self-applies `gu` (lowercase) in OperatorPending(Lowercase)
        Key::Char('u') => {
            if matches!(current_mode, Mode::OperatorPending(Operator::Lowercase)) {
                handle_operator(state, Operator::Lowercase, current_mode)
            } else {
                complete_simple(state, SimpleAction::Undo)
            }
        }
        Key::Ctrl('r') => complete_simple(state, SimpleAction::Redo),
        Key::Char('.') => complete_repeat(state),
        Key::Char('r') => {
            state.pending.push('r');
            ParseResult::Pending
        }

        // `~` is toggle-case-char in Normal mode, but self-applies `g~` in OperatorPending(ToggleCase)
        Key::Char('~') => {
            if matches!(current_mode, Mode::OperatorPending(Operator::ToggleCase)) {
                handle_operator(state, Operator::ToggleCase, current_mode)
            } else {
                complete_simple(state, SimpleAction::ToggleCase)
            }
        }

        // `U` self-applies `gU` (uppercase) in OperatorPending(Uppercase);
        // in Normal mode it undoes all changes to the current line.
        Key::Char('U') => {
            if matches!(current_mode, Mode::OperatorPending(Operator::Uppercase)) {
                handle_operator(state, Operator::Uppercase, current_mode)
            } else {
                complete_simple(state, SimpleAction::UndoLine)
            }
        }

        // Join lines (J)
        Key::Char('J') => complete_simple(state, SimpleAction::JoinLines),

        // Search word under cursor (* and #)
        Key::Char('*') => complete_simple(state, SimpleAction::SearchWordForward),
        Key::Char('#') => complete_simple(state, SimpleAction::SearchWordBackward),

        // Indent operators (>> / << for linewise-self; >{motion} / <{motion} for range)
        Key::Char('>') => handle_operator(state, Operator::IndentRight, current_mode),
        Key::Char('<') => handle_operator(state, Operator::IndentLeft, current_mode),

        // Q -- enter ex mode (stub)
        Key::Char('Q') => complete_simple(state, SimpleAction::EnterExMode),

        // & -- repeat last substitute
        Key::Char('&') => complete_simple(state, SimpleAction::RepeatSubstitute),

        // | -- move to column N
        Key::Char('|') => complete_motion(state, Motion::Column),

        // ! -- filter operator
        Key::Char('!') => handle_operator(state, Operator::Filter, current_mode),

        // Ctrl-^ -- edit alternate file
        Key::Ctrl('^') => complete_simple(state, SimpleAction::EditAlternateFile),

        // Put commands
        Key::Char('p') => complete_simple_with_register(state, SimpleAction::PutAfter),
        Key::Char('P') => complete_simple_with_register(state, SimpleAction::PutBefore),

        // Search repeat commands
        Key::Char('n') => complete_simple(state, SimpleAction::SearchNext),
        Key::Char('N') => complete_simple(state, SimpleAction::SearchPrev),

        // Jump list navigation
        Key::Ctrl('o') => complete_simple(state, SimpleAction::JumpBack),
        Key::Tab => complete_simple(state, SimpleAction::JumpForward),

        // `q` starts macro recording in Normal mode, but self-applies `gq` (format) in OperatorPending(Format)
        Key::Char('q') => {
            if matches!(current_mode, Mode::OperatorPending(Operator::Format)) {
                handle_operator(state, Operator::Format, current_mode)
            } else {
                state.pending.push('q');
                ParseResult::Pending
            }
        }
        Key::Char('@') => {
            state.pending.push('@');
            ParseResult::Pending
        }

        // Section motions — need a second character
        Key::Char('[') => {
            state.pending.push('[');
            ParseResult::Pending
        }
        Key::Char(']') => {
            state.pending.push(']');
            ParseResult::Pending
        }

        // Z{key} — ZZ (write-quit) and ZQ (force-quit)
        Key::Char('Z') => {
            state.pending.push('Z');
            ParseResult::Pending
        }

        // z redraw commands — need a second character
        Key::Char('z') => {
            state.pending.push('z');
            ParseResult::Pending
        }

        // Mark commands — need a second character
        Key::Char('m') => {
            state.pending.push('m');
            ParseResult::Pending
        }
        Key::Char('`') => {
            state.pending.push('`');
            ParseResult::Pending
        }
        Key::Char('\'') => {
            state.pending.push('\'');
            ParseResult::Pending
        }

        _ => {
            state.reset();
            ParseResult::Invalid
        }
    }
}

/// Continue parsing when we have pending state.
pub fn parse_pending(state: &mut ParseState, key: Key, current_mode: Mode) -> ParseResult {
    // Handle text object specifier if waiting for one
    if let Some(pending_kind) = state.pending_text_object {
        if let Key::Char(ch) = key {
            if let Some(object) = parse_text_object_specifier(ch) {
                let kind = match pending_kind {
                    PendingTextObject::Around => TextObjectKind::Around,
                    PendingTextObject::Inner => TextObjectKind::Inner,
                };
                let count = state.count();
                let register = state.register;
                state.reset();
                return ParseResult::Complete(ParsedCommand::OperatorTextObject {
                    operator: Operator::Delete, // Placeholder - actual operator comes from mode
                    kind,
                    object,
                    count,
                    register,
                });
            }
        }
        // Invalid text object specifier
        state.reset();
        return ParseResult::Invalid;
    }

    if state.pending.is_empty() {
        return parse_normal_key(state, key, current_mode);
    }

    match state.pending.as_slice() {
        // Register prefix: "a
        ['"'] => {
            if let Key::Char(
                c @ ('a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '"' | '+' | '*' | '_'),
            ) = key
            {
                state.register = Some(c);
                state.pending.clear();
                ParseResult::Pending
            } else {
                state.reset();
                ParseResult::Invalid
            }
        }

        // Z{key} — ZZ (write and quit) / ZQ (force quit)
        ['Z'] => match key {
            Key::Char('Z') => complete_simple(state, SimpleAction::WriteQuit),
            Key::Char('Q') => complete_simple(state, SimpleAction::ForceQuit),
            _ => {
                state.reset();
                ParseResult::Invalid
            }
        },

        // z{key} — viewport scroll commands
        ['z'] => match key {
            Key::Char('t') | Key::Enter => complete_simple(state, SimpleAction::ScrollCursorTop),
            Key::Char('z') | Key::Char('.') => {
                complete_simple(state, SimpleAction::ScrollCursorMiddle)
            }
            Key::Char('b') | Key::Char('-') => {
                complete_simple(state, SimpleAction::ScrollCursorBottom)
            }
            _ => {
                state.reset();
                ParseResult::Invalid
            }
        },

        // [{key} — [[ section backward
        ['['] => match key {
            Key::Char('[') => complete_motion(state, Motion::SectionBackward),
            _ => {
                state.reset();
                ParseResult::Invalid
            }
        },

        // ]{key} — ]] section forward
        [']'] => match key {
            Key::Char(']') => complete_motion(state, Motion::SectionForward),
            _ => {
                state.reset();
                ParseResult::Invalid
            }
        },

        // g{key} — gg motion, gU/gu/g~ operators, gJ join without space
        ['g'] => match key {
            Key::Char('g') => {
                // gg with count goes to line, without goes to start
                // When count is used for GotoLine, it's consumed (count becomes 1)
                if let Some(n) = state.count {
                    state.reset();
                    ParseResult::Complete(ParsedCommand::Motion {
                        motion: Motion::GotoLine(n),
                        count: 1,
                    })
                } else {
                    complete_motion(state, Motion::DocumentStart)
                }
            }
            Key::Char('U') => handle_operator(state, Operator::Uppercase, current_mode),
            Key::Char('u') => handle_operator(state, Operator::Lowercase, current_mode),
            Key::Char('~') => handle_operator(state, Operator::ToggleCase, current_mode),
            Key::Char('q') => handle_operator(state, Operator::Format, current_mode),
            Key::Char('J') => complete_simple(state, SimpleAction::JoinLinesNoSpace),
            _ => {
                state.reset();
                ParseResult::Invalid
            }
        },

        // Find motions: f{char}, F{char}, t{char}, T{char}
        ['f'] => {
            if let Key::Char(ch) = key {
                state.last_find = Some((ch, FindDirection::Forward, FindStop::OnChar));
                let motion = Motion::FindChar {
                    ch,
                    direction: FindDirection::Forward,
                    stop: FindStop::OnChar,
                };
                complete_motion(state, motion)
            } else {
                state.reset();
                ParseResult::Invalid
            }
        }
        ['F'] => {
            if let Key::Char(ch) = key {
                state.last_find = Some((ch, FindDirection::Backward, FindStop::OnChar));
                let motion = Motion::FindChar {
                    ch,
                    direction: FindDirection::Backward,
                    stop: FindStop::OnChar,
                };
                complete_motion(state, motion)
            } else {
                state.reset();
                ParseResult::Invalid
            }
        }
        ['t'] => {
            if let Key::Char(ch) = key {
                state.last_find = Some((ch, FindDirection::Forward, FindStop::BeforeChar));
                let motion = Motion::FindChar {
                    ch,
                    direction: FindDirection::Forward,
                    stop: FindStop::BeforeChar,
                };
                complete_motion(state, motion)
            } else {
                state.reset();
                ParseResult::Invalid
            }
        }
        ['T'] => {
            if let Key::Char(ch) = key {
                state.last_find = Some((ch, FindDirection::Backward, FindStop::BeforeChar));
                let motion = Motion::FindChar {
                    ch,
                    direction: FindDirection::Backward,
                    stop: FindStop::BeforeChar,
                };
                complete_motion(state, motion)
            } else {
                state.reset();
                ParseResult::Invalid
            }
        }

        // Replace char: r{char}
        ['r'] => {
            if let Key::Char(ch) = key {
                complete_simple(state, SimpleAction::ReplaceChar(ch))
            } else {
                state.reset();
                ParseResult::Invalid
            }
        }

        // q{a-z} — start recording macro into register; bare q while recording stops
        ['q'] => match key {
            Key::Char(ch @ 'a'..='z') => {
                complete_simple(state, SimpleAction::ToggleMacroRecord(ch))
            }
            _ => {
                state.reset();
                ParseResult::Invalid
            }
        },

        // @{a-z} — play macro; @@ — repeat last macro
        ['@'] => match key {
            Key::Char(ch @ 'a'..='z') => complete_simple(state, SimpleAction::PlayMacro(ch)),
            Key::Char('@') => complete_simple(state, SimpleAction::RepeatMacro),
            _ => {
                state.reset();
                ParseResult::Invalid
            }
        },

        // Set mark: m{a-z}
        ['m'] => {
            if let Key::Char(ch @ 'a'..='z') = key {
                complete_simple(state, SimpleAction::SetMark(ch))
            } else {
                state.reset();
                ParseResult::Invalid
            }
        }

        // Jump to exact mark position: `{a-z} or `` (previous position)
        // Also handles `< and `> (visual selection start/end marks).
        ['`'] => match key {
            Key::Char(ch @ 'a'..='z') => complete_simple(
                state,
                SimpleAction::JumpToMark {
                    mark: ch,
                    line_start: false,
                },
            ),
            Key::Char('`') => complete_simple(
                state,
                SimpleAction::JumpToMark {
                    mark: '`',
                    line_start: false,
                },
            ),
            Key::Char('<') => complete_simple(
                state,
                SimpleAction::JumpToMark {
                    mark: '<',
                    line_start: false,
                },
            ),
            Key::Char('>') => complete_simple(
                state,
                SimpleAction::JumpToMark {
                    mark: '>',
                    line_start: false,
                },
            ),
            _ => {
                state.reset();
                ParseResult::Invalid
            }
        },

        // Jump to first non-blank of mark's line: '{a-z} or '' (previous position)
        // Also handles '< and '> (visual selection start/end marks).
        ['\''] => match key {
            Key::Char(ch @ 'a'..='z') => complete_simple(
                state,
                SimpleAction::JumpToMark {
                    mark: ch,
                    line_start: true,
                },
            ),
            Key::Char('\'') => complete_simple(
                state,
                SimpleAction::JumpToMark {
                    mark: '`',
                    line_start: true,
                },
            ),
            Key::Char('<') => complete_simple(
                state,
                SimpleAction::JumpToMark {
                    mark: '<',
                    line_start: true,
                },
            ),
            Key::Char('>') => complete_simple(
                state,
                SimpleAction::JumpToMark {
                    mark: '>',
                    line_start: true,
                },
            ),
            _ => {
                state.reset();
                ParseResult::Invalid
            }
        },

        _ => {
            state.reset();
            ParseResult::Invalid
        }
    }
}

/// Parse a key in operator-pending mode, specifically for text objects.
///
/// This is called from key_handlers when in operator-pending mode after
/// receiving 'a' or 'i'.
pub fn parse_text_object_key(state: &mut ParseState, key: Key, operator: Operator) -> ParseResult {
    if let Some(pending_kind) = state.pending_text_object {
        if let Key::Char(ch) = key {
            if let Some(object) = parse_text_object_specifier(ch) {
                let kind = match pending_kind {
                    PendingTextObject::Around => TextObjectKind::Around,
                    PendingTextObject::Inner => TextObjectKind::Inner,
                };
                let count = state.operator_count();
                let register = state.operator_register();
                state.reset_all();
                return ParseResult::Complete(ParsedCommand::OperatorTextObject {
                    operator,
                    kind,
                    object,
                    count,
                    register,
                });
            }
        }
        // Invalid text object specifier
        state.reset_all();
        return ParseResult::Invalid;
    }

    // Not waiting for text object specifier
    ParseResult::Invalid
}

// Helper functions

fn accumulate_count(state: &mut ParseState, digit: char) {
    let digit_val = digit.to_digit(10).unwrap_or(0) as usize;
    state.count = Some(state.count.unwrap_or(0) * 10 + digit_val);
}

fn handle_pending_digit(state: &mut ParseState, digit: char) -> ParseResult {
    // For now, digits in pending state are invalid
    // Future: could extend for g commands that take counts
    state.reset();
    let _ = digit;
    ParseResult::Invalid
}

fn complete_motion(state: &mut ParseState, motion: Motion) -> ParseResult {
    let count = state.count();
    state.reset();
    ParseResult::Complete(ParsedCommand::Motion { motion, count })
}

fn complete_mode_change(state: &mut ParseState, kind: ModeChangeKind) -> ParseResult {
    let count = state.count();
    state.reset();
    ParseResult::Complete(ParsedCommand::ModeChange(kind, count))
}

fn complete_simple(state: &mut ParseState, action: SimpleAction) -> ParseResult {
    let count = state.count();
    state.reset();
    ParseResult::Complete(ParsedCommand::SimpleAction { action, count })
}

/// Complete a simple action, preserving register information.
fn complete_simple_with_register(state: &mut ParseState, action: SimpleAction) -> ParseResult {
    let register = state.register;
    let count = state.count();
    state.reset();
    ParseResult::Complete(ParsedCommand::SimpleActionWithRegister {
        action,
        register,
        count,
    })
}

/// Complete the dot repeat command, preserving the count.
///
/// The count is captured as `Some(n)` when the user typed a count prefix
/// (e.g., `3.`), or `None` when `.` was pressed without a count. This
/// distinction matters because `None` means "use the original count"
/// while `Some(n)` means "replace the original count with n".
fn complete_repeat(state: &mut ParseState) -> ParseResult {
    let count = state.count_opt();
    state.reset();
    ParseResult::Complete(ParsedCommand::Repeat { count })
}

fn handle_operator(state: &mut ParseState, operator: Operator, current_mode: Mode) -> ParseResult {
    // Check if we're already in operator-pending mode for the same operator (dd, yy, cc)
    if let Mode::OperatorPending(pending_op) = current_mode {
        if pending_op == operator {
            // dd, yy, cc - operate on current line
            let count = state.count();
            let register = state.register;
            state.reset();
            return ParseResult::Complete(ParsedCommand::OperatorMotion {
                operator,
                motion: Motion::Down, // Line-wise operation
                count,
                register,
            });
        }
    }

    // Enter operator-pending mode
    let count = state.count();
    let register = state.register;
    state.reset();
    ParseResult::Complete(ParsedCommand::OperatorPending {
        operator,
        count,
        register,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Count parsing tests

    #[test]
    fn test_parse_single_digit_count() {
        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('3'), Mode::Normal),
            ParseResult::Pending
        );
        let result = parse_normal_key(&mut state, Key::Char('j'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::Motion {
                motion: Motion::Down,
                count: 3
            })
        );
    }

    #[test]
    fn test_parse_multi_digit_count() {
        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('1'), Mode::Normal),
            ParseResult::Pending
        );
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('2'), Mode::Normal),
            ParseResult::Pending
        );
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('3'), Mode::Normal),
            ParseResult::Pending
        );
        let result = parse_normal_key(&mut state, Key::Char('j'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::Motion {
                motion: Motion::Down,
                count: 123
            })
        );
    }

    #[test]
    fn test_parse_count_with_zero() {
        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('1'), Mode::Normal),
            ParseResult::Pending
        );
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('0'), Mode::Normal),
            ParseResult::Pending
        );
        let result = parse_normal_key(&mut state, Key::Char('j'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::Motion {
                motion: Motion::Down,
                count: 10
            })
        );
    }

    #[test]
    fn test_parse_zero_alone() {
        let mut state = ParseState::new();
        let result = parse_normal_key(&mut state, Key::Char('0'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::Motion {
                motion: Motion::LineStart,
                count: 1
            })
        );
    }

    // Register parsing tests

    #[test]
    fn test_parse_register_lowercase() {
        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('"'), Mode::Normal),
            ParseResult::Pending
        );
        assert_eq!(
            parse_pending(&mut state, Key::Char('a'), Mode::Normal),
            ParseResult::Pending
        );
        // Register is now set
        assert_eq!(state.register(), Some('a'));
    }

    #[test]
    fn test_parse_register_uppercase() {
        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('"'), Mode::Normal),
            ParseResult::Pending
        );
        assert_eq!(
            parse_pending(&mut state, Key::Char('A'), Mode::Normal),
            ParseResult::Pending
        );
        assert_eq!(state.register(), Some('A'));
    }

    #[test]
    fn test_parse_register_numbered() {
        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('"'), Mode::Normal),
            ParseResult::Pending
        );
        assert_eq!(
            parse_pending(&mut state, Key::Char('1'), Mode::Normal),
            ParseResult::Pending
        );
        assert_eq!(state.register(), Some('1'));
    }

    #[test]
    fn test_parse_invalid_register() {
        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('"'), Mode::Normal),
            ParseResult::Pending
        );
        assert_eq!(
            parse_pending(&mut state, Key::Char('!'), Mode::Normal),
            ParseResult::Invalid
        );
    }

    // Multi-key sequence tests

    #[test]
    fn test_parse_gg() {
        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('g'), Mode::Normal),
            ParseResult::Pending
        );
        let result = parse_pending(&mut state, Key::Char('g'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::Motion {
                motion: Motion::DocumentStart,
                count: 1
            })
        );
    }

    #[test]
    fn test_parse_gg_with_count() {
        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('5'), Mode::Normal),
            ParseResult::Pending
        );
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('g'), Mode::Normal),
            ParseResult::Pending
        );
        let result = parse_pending(&mut state, Key::Char('g'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::Motion {
                motion: Motion::GotoLine(5),
                count: 1 // Count consumed by GotoLine
            })
        );
    }

    #[test]
    fn test_parse_g_then_invalid() {
        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('g'), Mode::Normal),
            ParseResult::Pending
        );
        assert_eq!(
            parse_pending(&mut state, Key::Char('x'), Mode::Normal),
            ParseResult::Invalid
        );
    }

    #[test]
    fn test_parse_find_forward() {
        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('f'), Mode::Normal),
            ParseResult::Pending
        );
        let result = parse_pending(&mut state, Key::Char('a'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::Motion {
                motion: Motion::FindChar {
                    ch: 'a',
                    direction: FindDirection::Forward,
                    stop: FindStop::OnChar
                },
                count: 1
            })
        );
        // Verify last_find was set
        assert_eq!(
            state.last_find(),
            Some(('a', FindDirection::Forward, FindStop::OnChar))
        );
    }

    #[test]
    fn test_parse_find_backward() {
        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('F'), Mode::Normal),
            ParseResult::Pending
        );
        let result = parse_pending(&mut state, Key::Char('a'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::Motion {
                motion: Motion::FindChar {
                    ch: 'a',
                    direction: FindDirection::Backward,
                    stop: FindStop::OnChar
                },
                count: 1
            })
        );
    }

    #[test]
    fn test_parse_till_forward() {
        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('t'), Mode::Normal),
            ParseResult::Pending
        );
        let result = parse_pending(&mut state, Key::Char('a'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::Motion {
                motion: Motion::FindChar {
                    ch: 'a',
                    direction: FindDirection::Forward,
                    stop: FindStop::BeforeChar
                },
                count: 1
            })
        );
    }

    #[test]
    fn test_parse_till_backward() {
        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('T'), Mode::Normal),
            ParseResult::Pending
        );
        let result = parse_pending(&mut state, Key::Char('a'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::Motion {
                motion: Motion::FindChar {
                    ch: 'a',
                    direction: FindDirection::Backward,
                    stop: FindStop::BeforeChar
                },
                count: 1
            })
        );
    }

    // Operator parsing tests

    #[test]
    fn test_parse_d_enters_operator_pending() {
        let mut state = ParseState::new();
        let result = parse_normal_key(&mut state, Key::Char('d'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::OperatorPending {
                operator: Operator::Delete,
                count: 1,
                register: None
            })
        );
    }

    #[test]
    fn test_parse_dd() {
        let mut state = ParseState::new();
        let result = parse_normal_key(
            &mut state,
            Key::Char('d'),
            Mode::OperatorPending(Operator::Delete),
        );
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::OperatorMotion {
                operator: Operator::Delete,
                motion: Motion::Down,
                count: 1,
                register: None
            })
        );
    }

    #[test]
    fn test_parse_3dd() {
        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('3'), Mode::Normal),
            ParseResult::Pending
        );
        let result = parse_normal_key(
            &mut state,
            Key::Char('d'),
            Mode::OperatorPending(Operator::Delete),
        );
        // With count 3, dd becomes delete 3 lines
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::OperatorMotion {
                operator: Operator::Delete,
                motion: Motion::Down,
                count: 3,
                register: None
            })
        );
    }

    // Edge case tests

    #[test]
    fn test_parse_esc_resets_state() {
        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('3'), Mode::Normal),
            ParseResult::Pending
        );
        assert_eq!(
            parse_normal_key(&mut state, Key::Esc, Mode::Normal),
            ParseResult::Invalid
        );
        // State should be reset
        assert_eq!(state.count(), 1);
        assert!(!state.is_pending());
    }

    #[test]
    fn test_parse_incomplete_find() {
        let mut state = ParseState::new();
        let result = parse_normal_key(&mut state, Key::Char('f'), Mode::Normal);
        assert_eq!(result, ParseResult::Pending);
        assert!(state.is_pending());
    }

    #[test]
    fn test_parse_state_persists_last_find() {
        let mut state = ParseState::new();
        // First do fa
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('f'), Mode::Normal),
            ParseResult::Pending
        );
        parse_pending(&mut state, Key::Char('a'), Mode::Normal);

        // Reset should NOT clear last_find
        state.reset();
        assert_eq!(
            state.last_find(),
            Some(('a', FindDirection::Forward, FindStop::OnChar))
        );
    }

    #[test]
    fn test_parse_repeat_find() {
        let mut state = ParseState::new();
        // Set up last_find
        state.set_last_find('x', FindDirection::Forward, FindStop::OnChar);

        let result = parse_normal_key(&mut state, Key::Char(';'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::Motion {
                motion: Motion::RepeatFind,
                count: 1
            })
        );
    }

    #[test]
    fn test_parse_repeat_find_reverse() {
        let mut state = ParseState::new();
        state.set_last_find('x', FindDirection::Forward, FindStop::OnChar);

        let result = parse_normal_key(&mut state, Key::Char(','), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::Motion {
                motion: Motion::RepeatFindReverse,
                count: 1
            })
        );
    }

    // Mode change tests

    #[test]
    fn test_parse_mode_change_i() {
        let mut state = ParseState::new();
        let result = parse_normal_key(&mut state, Key::Char('i'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::ModeChange(
                ModeChangeKind::InsertBeforeCursor,
                1
            ))
        );
    }

    #[test]
    fn test_parse_mode_change_a() {
        let mut state = ParseState::new();
        let result = parse_normal_key(&mut state, Key::Char('a'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::ModeChange(
                ModeChangeKind::InsertAfterCursor,
                1
            ))
        );
    }

    #[test]
    fn test_parse_mode_change_o() {
        let mut state = ParseState::new();
        let result = parse_normal_key(&mut state, Key::Char('o'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::ModeChange(ModeChangeKind::OpenLineBelow, 1))
        );
    }

    // Simple action tests

    #[test]
    fn test_parse_simple_x() {
        let mut state = ParseState::new();
        let result = parse_normal_key(&mut state, Key::Char('x'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::SimpleActionWithRegister {
                action: SimpleAction::DeleteCharAtCursor,
                register: None,
                count: 1,
            })
        );
    }

    #[test]
    fn test_parse_replace_char() {
        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('r'), Mode::Normal),
            ParseResult::Pending
        );
        let result = parse_pending(&mut state, Key::Char('x'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::SimpleAction {
                action: SimpleAction::ReplaceChar('x'),
                count: 1,
            })
        );
    }

    // Motion with arrow keys

    #[test]
    fn test_parse_arrow_keys() {
        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Left, Mode::Normal),
            ParseResult::Complete(ParsedCommand::Motion {
                motion: Motion::Left,
                count: 1
            })
        );

        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Right, Mode::Normal),
            ParseResult::Complete(ParsedCommand::Motion {
                motion: Motion::Right,
                count: 1
            })
        );

        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Up, Mode::Normal),
            ParseResult::Complete(ParsedCommand::Motion {
                motion: Motion::Up,
                count: 1
            })
        );

        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Down, Mode::Normal),
            ParseResult::Complete(ParsedCommand::Motion {
                motion: Motion::Down,
                count: 1
            })
        );
    }

    // G motion tests

    #[test]
    fn test_parse_g_without_count() {
        let mut state = ParseState::new();
        let result = parse_normal_key(&mut state, Key::Char('G'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::Motion {
                motion: Motion::DocumentEnd,
                count: 1
            })
        );
    }

    #[test]
    fn test_parse_g_with_count() {
        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('5'), Mode::Normal),
            ParseResult::Pending
        );
        let result = parse_normal_key(&mut state, Key::Char('G'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::Motion {
                motion: Motion::GotoLine(5),
                count: 1 // Count consumed by GotoLine
            })
        );
    }

    // Word motion tests

    #[test]
    fn test_parse_word_motions() {
        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('w'), Mode::Normal),
            ParseResult::Complete(ParsedCommand::Motion {
                motion: Motion::WordForward,
                count: 1
            })
        );

        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('b'), Mode::Normal),
            ParseResult::Complete(ParsedCommand::Motion {
                motion: Motion::WordBackward,
                count: 1
            })
        );

        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('e'), Mode::Normal),
            ParseResult::Complete(ParsedCommand::Motion {
                motion: Motion::WordEnd,
                count: 1
            })
        );
    }

    #[test]
    fn test_parse_big_word_motions() {
        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('W'), Mode::Normal),
            ParseResult::Complete(ParsedCommand::Motion {
                motion: Motion::WORDForward,
                count: 1
            })
        );

        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('B'), Mode::Normal),
            ParseResult::Complete(ParsedCommand::Motion {
                motion: Motion::WORDBackward,
                count: 1
            })
        );

        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('E'), Mode::Normal),
            ParseResult::Complete(ParsedCommand::Motion {
                motion: Motion::WORDEnd,
                count: 1
            })
        );
    }

    // Line motion tests

    #[test]
    fn test_parse_line_motions() {
        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('^'), Mode::Normal),
            ParseResult::Complete(ParsedCommand::Motion {
                motion: Motion::FirstNonBlank,
                count: 1
            })
        );

        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('$'), Mode::Normal),
            ParseResult::Complete(ParsedCommand::Motion {
                motion: Motion::LineEnd,
                count: 1
            })
        );
    }

    // Operator count tests

    #[test]
    fn test_operator_count_storage() {
        let mut state = ParseState::new();
        state.set_operator_count(3);
        assert_eq!(state.operator_count(), 3);
        state.clear_operator_count();
        assert_eq!(state.operator_count(), 1); // defaults to 1
    }

    #[test]
    fn test_reset_all_clears_operator_count() {
        let mut state = ParseState::new();
        state.set_operator_count(5);
        state.reset_all();
        assert_eq!(state.operator_count(), 1);
    }

    #[test]
    fn test_reset_preserves_operator_count() {
        let mut state = ParseState::new();
        state.set_operator_count(5);
        state.reset();
        // Regular reset should preserve operator_count
        assert_eq!(state.operator_count(), 5);
    }

    // Put command tests

    #[test]
    fn test_parse_put_after() {
        let mut state = ParseState::new();
        let result = parse_normal_key(&mut state, Key::Char('p'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::SimpleActionWithRegister {
                action: SimpleAction::PutAfter,
                register: None,
                count: 1,
            })
        );
    }

    #[test]
    fn test_parse_put_before() {
        let mut state = ParseState::new();
        let result = parse_normal_key(&mut state, Key::Char('P'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::SimpleActionWithRegister {
                action: SimpleAction::PutBefore,
                register: None,
                count: 1,
            })
        );
    }

    #[test]
    fn test_parse_put_with_register() {
        let mut state = ParseState::new();
        // "ap - put from register a after cursor
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('"'), Mode::Normal),
            ParseResult::Pending
        );
        assert_eq!(
            parse_pending(&mut state, Key::Char('a'), Mode::Normal),
            ParseResult::Pending
        );
        let result = parse_normal_key(&mut state, Key::Char('p'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::SimpleActionWithRegister {
                action: SimpleAction::PutAfter,
                register: Some('a'),
                count: 1,
            })
        );
    }

    #[test]
    fn test_parse_delete_with_register() {
        let mut state = ParseState::new();
        // "add - delete line to register a
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('"'), Mode::Normal),
            ParseResult::Pending
        );
        assert_eq!(
            parse_pending(&mut state, Key::Char('a'), Mode::Normal),
            ParseResult::Pending
        );
        let result = parse_normal_key(&mut state, Key::Char('d'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::OperatorPending {
                operator: Operator::Delete,
                count: 1,
                register: Some('a')
            })
        );
    }

    #[test]
    fn test_parse_yank_operator() {
        let mut state = ParseState::new();
        let result = parse_normal_key(&mut state, Key::Char('y'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::OperatorPending {
                operator: Operator::Yank,
                count: 1,
                register: None
            })
        );
    }

    #[test]
    fn test_parse_yy() {
        let mut state = ParseState::new();
        let result = parse_normal_key(
            &mut state,
            Key::Char('y'),
            Mode::OperatorPending(Operator::Yank),
        );
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::OperatorMotion {
                operator: Operator::Yank,
                motion: Motion::Down,
                count: 1,
                register: None
            })
        );
    }

    #[test]
    fn test_parse_register_small_delete() {
        let mut state = ParseState::new();
        // "- - small delete register
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('"'), Mode::Normal),
            ParseResult::Pending
        );
        assert_eq!(
            parse_pending(&mut state, Key::Char('-'), Mode::Normal),
            ParseResult::Pending
        );
        assert_eq!(state.register(), Some('-'));
    }

    #[test]
    fn test_parse_register_unnamed() {
        let mut state = ParseState::new();
        // "" - unnamed register (explicit)
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('"'), Mode::Normal),
            ParseResult::Pending
        );
        assert_eq!(
            parse_pending(&mut state, Key::Char('"'), Mode::Normal),
            ParseResult::Pending
        );
        assert_eq!(state.register(), Some('"'));
    }

    #[test]
    fn test_operator_register_storage() {
        let mut state = ParseState::new();
        state.set_operator_register(Some('a'));
        assert_eq!(state.operator_register(), Some('a'));
        state.clear_operator_register();
        assert_eq!(state.operator_register(), None);
    }

    #[test]
    fn test_reset_all_clears_operator_register() {
        let mut state = ParseState::new();
        state.set_operator_register(Some('a'));
        state.reset_all();
        assert_eq!(state.operator_register(), None);
    }

    // Text object parsing tests

    #[test]
    fn test_parse_text_object_a_in_operator_pending() {
        let mut state = ParseState::new();
        // In operator-pending mode, 'a' should trigger text object parsing
        let result = parse_normal_key(
            &mut state,
            Key::Char('a'),
            Mode::OperatorPending(Operator::Delete),
        );
        assert_eq!(result, ParseResult::Pending);
        assert_eq!(state.pending_text_object(), Some(PendingTextObject::Around));
    }

    #[test]
    fn test_parse_text_object_i_in_operator_pending() {
        let mut state = ParseState::new();
        // In operator-pending mode, 'i' should trigger text object parsing
        let result = parse_normal_key(
            &mut state,
            Key::Char('i'),
            Mode::OperatorPending(Operator::Delete),
        );
        assert_eq!(result, ParseResult::Pending);
        assert_eq!(state.pending_text_object(), Some(PendingTextObject::Inner));
    }

    #[test]
    fn test_parse_text_object_daw() {
        let mut state = ParseState::new();
        // Set up pending state for 'a' (around)
        state.set_pending_text_object(PendingTextObject::Around);
        state.set_operator_count(1);

        let result = parse_text_object_key(&mut state, Key::Char('w'), Operator::Delete);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::OperatorTextObject {
                operator: Operator::Delete,
                kind: TextObjectKind::Around,
                object: TextObject::Word,
                count: 1,
                register: None
            })
        );
    }

    #[test]
    fn test_parse_text_object_diw() {
        let mut state = ParseState::new();
        // Set up pending state for 'i' (inner)
        state.set_pending_text_object(PendingTextObject::Inner);
        state.set_operator_count(1);

        let result = parse_text_object_key(&mut state, Key::Char('w'), Operator::Delete);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::OperatorTextObject {
                operator: Operator::Delete,
                kind: TextObjectKind::Inner,
                object: TextObject::Word,
                count: 1,
                register: None
            })
        );
    }

    #[test]
    fn test_parse_text_object_ci_quote() {
        let mut state = ParseState::new();
        state.set_pending_text_object(PendingTextObject::Inner);
        state.set_operator_count(1);

        let result = parse_text_object_key(&mut state, Key::Char('"'), Operator::Change);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::OperatorTextObject {
                operator: Operator::Change,
                kind: TextObjectKind::Inner,
                object: TextObject::DoubleQuote,
                count: 1,
                register: None
            })
        );
    }

    #[test]
    fn test_parse_text_object_da_paren() {
        let mut state = ParseState::new();
        state.set_pending_text_object(PendingTextObject::Around);
        state.set_operator_count(1);

        let result = parse_text_object_key(&mut state, Key::Char('('), Operator::Delete);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::OperatorTextObject {
                operator: Operator::Delete,
                kind: TextObjectKind::Around,
                object: TextObject::Parenthesis,
                count: 1,
                register: None
            })
        );
    }

    #[test]
    fn test_parse_text_object_with_count() {
        let mut state = ParseState::new();
        state.set_pending_text_object(PendingTextObject::Around);
        state.set_operator_count(2);

        let result = parse_text_object_key(&mut state, Key::Char('w'), Operator::Delete);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::OperatorTextObject {
                operator: Operator::Delete,
                kind: TextObjectKind::Around,
                object: TextObject::Word,
                count: 2,
                register: None
            })
        );
    }

    #[test]
    fn test_parse_text_object_with_register() {
        let mut state = ParseState::new();
        state.set_pending_text_object(PendingTextObject::Inner);
        state.set_operator_count(1);
        state.set_operator_register(Some('a'));

        let result = parse_text_object_key(&mut state, Key::Char('w'), Operator::Yank);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::OperatorTextObject {
                operator: Operator::Yank,
                kind: TextObjectKind::Inner,
                object: TextObject::Word,
                count: 1,
                register: Some('a')
            })
        );
    }

    #[test]
    fn test_parse_text_object_invalid_specifier() {
        let mut state = ParseState::new();
        state.set_pending_text_object(PendingTextObject::Around);
        state.set_operator_count(1);

        let result = parse_text_object_key(&mut state, Key::Char('x'), Operator::Delete);
        assert_eq!(result, ParseResult::Invalid);
    }

    #[test]
    fn test_parse_text_object_alternative_brackets() {
        let mut state = ParseState::new();
        state.set_pending_text_object(PendingTextObject::Around);
        state.set_operator_count(1);

        // 'b' is an alias for '('
        let result = parse_text_object_key(&mut state, Key::Char('b'), Operator::Delete);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::OperatorTextObject {
                operator: Operator::Delete,
                kind: TextObjectKind::Around,
                object: TextObject::Parenthesis,
                count: 1,
                register: None
            })
        );
    }

    #[test]
    fn test_parse_text_object_brace_b() {
        let mut state = ParseState::new();
        state.set_pending_text_object(PendingTextObject::Around);
        state.set_operator_count(1);

        // 'B' is an alias for '{'
        let result = parse_text_object_key(&mut state, Key::Char('B'), Operator::Delete);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::OperatorTextObject {
                operator: Operator::Delete,
                kind: TextObjectKind::Around,
                object: TextObject::Brace,
                count: 1,
                register: None
            })
        );
    }

    // D, C, Y command parsing tests

    #[test]
    fn test_parse_d_uppercase() {
        let mut state = ParseState::new();
        let result = parse_normal_key(&mut state, Key::Char('D'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::SimpleActionWithRegister {
                action: SimpleAction::DeleteToEndOfLine,
                register: None,
                count: 1,
            })
        );
    }

    #[test]
    fn test_parse_c_uppercase() {
        let mut state = ParseState::new();
        let result = parse_normal_key(&mut state, Key::Char('C'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::SimpleActionWithRegister {
                action: SimpleAction::ChangeToEndOfLine,
                register: None,
                count: 1,
            })
        );
    }

    #[test]
    fn test_parse_y_uppercase() {
        let mut state = ParseState::new();
        let result = parse_normal_key(&mut state, Key::Char('Y'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::SimpleActionWithRegister {
                action: SimpleAction::YankLine,
                register: None,
                count: 1,
            })
        );
    }

    #[test]
    fn test_parse_d_uppercase_with_register() {
        let mut state = ParseState::new();
        // "aD - delete to end of line into register a
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('"'), Mode::Normal),
            ParseResult::Pending
        );
        assert_eq!(
            parse_pending(&mut state, Key::Char('a'), Mode::Normal),
            ParseResult::Pending
        );
        let result = parse_normal_key(&mut state, Key::Char('D'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::SimpleActionWithRegister {
                action: SimpleAction::DeleteToEndOfLine,
                register: Some('a'),
                count: 1,
            })
        );
    }

    #[test]
    fn test_parse_c_uppercase_with_register() {
        let mut state = ParseState::new();
        // "aC - change to end of line into register a
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('"'), Mode::Normal),
            ParseResult::Pending
        );
        assert_eq!(
            parse_pending(&mut state, Key::Char('a'), Mode::Normal),
            ParseResult::Pending
        );
        let result = parse_normal_key(&mut state, Key::Char('C'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::SimpleActionWithRegister {
                action: SimpleAction::ChangeToEndOfLine,
                register: Some('a'),
                count: 1,
            })
        );
    }

    #[test]
    fn test_parse_y_uppercase_with_register() {
        let mut state = ParseState::new();
        // "aY - yank line into register a
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('"'), Mode::Normal),
            ParseResult::Pending
        );
        assert_eq!(
            parse_pending(&mut state, Key::Char('a'), Mode::Normal),
            ParseResult::Pending
        );
        let result = parse_normal_key(&mut state, Key::Char('Y'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::SimpleActionWithRegister {
                action: SimpleAction::YankLine,
                register: Some('a'),
                count: 1,
            })
        );
    }

    // Dot repeat parsing tests

    #[test]
    fn test_parse_dot_without_count() {
        let mut state = ParseState::new();
        let result = parse_normal_key(&mut state, Key::Char('.'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::Repeat { count: None })
        );
    }

    #[test]
    fn test_parse_dot_with_count() {
        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('3'), Mode::Normal),
            ParseResult::Pending
        );
        let result = parse_normal_key(&mut state, Key::Char('.'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::Repeat { count: Some(3) })
        );
    }

    #[test]
    fn test_parse_dot_with_multi_digit_count() {
        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('1'), Mode::Normal),
            ParseResult::Pending
        );
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('2'), Mode::Normal),
            ParseResult::Pending
        );
        let result = parse_normal_key(&mut state, Key::Char('.'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::Repeat { count: Some(12) })
        );
    }

    // Search repeat parsing tests

    #[test]
    fn test_parse_search_next() {
        let mut state = ParseState::new();
        let result = parse_normal_key(&mut state, Key::Char('n'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::SimpleAction {
                action: SimpleAction::SearchNext,
                count: 1,
            })
        );
    }

    #[test]
    fn test_parse_search_prev() {
        let mut state = ParseState::new();
        let result = parse_normal_key(&mut state, Key::Char('N'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::SimpleAction {
                action: SimpleAction::SearchPrev,
                count: 1,
            })
        );
    }

    #[test]
    fn test_parse_search_next_with_count() {
        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('3'), Mode::Normal),
            ParseResult::Pending
        );
        let result = parse_normal_key(&mut state, Key::Char('n'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::SimpleAction {
                action: SimpleAction::SearchNext,
                count: 3,
            })
        );
    }

    // Shift command (>> and <<) parsing tests

    #[test]
    fn test_parse_shift_right() {
        // >> produces OperatorMotion with Motion::Down (linewise self-application)
        let mut state = ParseState::new();
        let first = parse_normal_key(&mut state, Key::Char('>'), Mode::Normal);
        assert_eq!(
            first,
            ParseResult::Complete(ParsedCommand::OperatorPending {
                operator: Operator::IndentRight,
                count: 1,
                register: None,
            })
        );
        // Second > in OperatorPending(IndentRight) mode triggers linewise self
        let mut state2 = ParseState::new();
        let result = parse_normal_key(
            &mut state2,
            Key::Char('>'),
            Mode::OperatorPending(Operator::IndentRight),
        );
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::OperatorMotion {
                operator: Operator::IndentRight,
                motion: Motion::Down,
                count: 1,
                register: None,
            })
        );
    }

    #[test]
    fn test_parse_shift_left() {
        // << produces OperatorMotion with Motion::Down (linewise self-application)
        let mut state = ParseState::new();
        let first = parse_normal_key(&mut state, Key::Char('<'), Mode::Normal);
        assert_eq!(
            first,
            ParseResult::Complete(ParsedCommand::OperatorPending {
                operator: Operator::IndentLeft,
                count: 1,
                register: None,
            })
        );
        let mut state2 = ParseState::new();
        let result = parse_normal_key(
            &mut state2,
            Key::Char('<'),
            Mode::OperatorPending(Operator::IndentLeft),
        );
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::OperatorMotion {
                operator: Operator::IndentLeft,
                motion: Motion::Down,
                count: 1,
                register: None,
            })
        );
    }

    #[test]
    fn test_parse_shift_right_with_count() {
        // 3>> — count applies to the operator
        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('3'), Mode::Normal),
            ParseResult::Pending
        );
        let result = parse_normal_key(&mut state, Key::Char('>'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::OperatorPending {
                operator: Operator::IndentRight,
                count: 3,
                register: None,
            })
        );
    }

    #[test]
    fn test_parse_shift_left_with_count() {
        // 5<< — count applies to the operator
        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('5'), Mode::Normal),
            ParseResult::Pending
        );
        let result = parse_normal_key(&mut state, Key::Char('<'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::OperatorPending {
                operator: Operator::IndentLeft,
                count: 5,
                register: None,
            })
        );
    }

    #[test]
    fn test_parse_shift_right_with_motion() {
        // >j — > enters OperatorPending; j resolves to Motion::Down (wrapped by executor)
        let mut state = ParseState::new();
        let op_result = parse_normal_key(&mut state, Key::Char('>'), Mode::Normal);
        assert_eq!(
            op_result,
            ParseResult::Complete(ParsedCommand::OperatorPending {
                operator: Operator::IndentRight,
                count: 1,
                register: None,
            })
        );
        let mut state2 = ParseState::new();
        let result = parse_normal_key(
            &mut state2,
            Key::Char('j'),
            Mode::OperatorPending(Operator::IndentRight),
        );
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::Motion {
                motion: Motion::Down,
                count: 1
            })
        );
    }

    #[test]
    fn test_parse_shift_left_self_application() {
        // << while already in OperatorPending(IndentLeft) → linewise self-application
        let mut state = ParseState::new();
        let result = parse_normal_key(
            &mut state,
            Key::Char('<'),
            Mode::OperatorPending(Operator::IndentLeft),
        );
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::OperatorMotion {
                operator: Operator::IndentLeft,
                motion: Motion::Down,
                count: 1,
                register: None,
            })
        );
    }

    // Toggle case (~) parsing tests

    #[test]
    fn test_parse_toggle_case() {
        let mut state = ParseState::new();
        let result = parse_normal_key(&mut state, Key::Char('~'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::SimpleAction {
                action: SimpleAction::ToggleCase,
                count: 1,
            })
        );
    }

    #[test]
    fn test_parse_toggle_case_with_count() {
        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('3'), Mode::Normal),
            ParseResult::Pending
        );
        let result = parse_normal_key(&mut state, Key::Char('~'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::SimpleAction {
                action: SimpleAction::ToggleCase,
                count: 3,
            })
        );
    }

    // Join lines (J) parsing tests

    #[test]
    fn test_parse_join_lines() {
        let mut state = ParseState::new();
        let result = parse_normal_key(&mut state, Key::Char('J'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::SimpleAction {
                action: SimpleAction::JoinLines,
                count: 1,
            })
        );
    }

    #[test]
    fn test_parse_join_lines_with_count() {
        let mut state = ParseState::new();
        assert_eq!(
            parse_normal_key(&mut state, Key::Char('3'), Mode::Normal),
            ParseResult::Pending
        );
        let result = parse_normal_key(&mut state, Key::Char('J'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::SimpleAction {
                action: SimpleAction::JoinLines,
                count: 3,
            })
        );
    }

    // Matching bracket (%) parsing tests

    #[test]
    fn test_parse_matching_bracket() {
        let mut state = ParseState::new();
        let result = parse_normal_key(&mut state, Key::Char('%'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::Motion {
                motion: Motion::MatchingBracket,
                count: 1
            })
        );
    }

    // Search word under cursor (* and #) parsing tests

    #[test]
    fn test_parse_search_word_forward() {
        let mut state = ParseState::new();
        let result = parse_normal_key(&mut state, Key::Char('*'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::SimpleAction {
                action: SimpleAction::SearchWordForward,
                count: 1,
            })
        );
    }

    #[test]
    fn test_parse_search_word_backward() {
        let mut state = ParseState::new();
        let result = parse_normal_key(&mut state, Key::Char('#'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::SimpleAction {
                action: SimpleAction::SearchWordBackward,
                count: 1,
            })
        );
    }

    // gUU / guu / g~~ self-application tests

    #[test]
    fn test_parse_guu_self_application() {
        // Second `u` while in OperatorPending(Lowercase) → linewise self-apply
        let mut state = ParseState::new();
        let result = parse_normal_key(
            &mut state,
            Key::Char('u'),
            Mode::OperatorPending(Operator::Lowercase),
        );
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::OperatorMotion {
                operator: Operator::Lowercase,
                motion: Motion::Down,
                count: 1,
                register: None,
            })
        );
    }

    #[test]
    fn test_parse_guu_u_in_normal_is_undo() {
        // `u` in Normal mode must still be Undo, not Lowercase
        let mut state = ParseState::new();
        let result = parse_normal_key(&mut state, Key::Char('u'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::SimpleAction {
                action: SimpleAction::Undo,
                count: 1,
            })
        );
    }

    #[test]
    fn test_parse_guu_uppercase_self_application() {
        // Second `U` while in OperatorPending(Uppercase) → linewise self-apply
        let mut state = ParseState::new();
        let result = parse_normal_key(
            &mut state,
            Key::Char('U'),
            Mode::OperatorPending(Operator::Uppercase),
        );
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::OperatorMotion {
                operator: Operator::Uppercase,
                motion: Motion::Down,
                count: 1,
                register: None,
            })
        );
    }

    #[test]
    fn test_parse_guu_uppercase_u_in_normal_is_undo_line() {
        // `U` in Normal mode undoes all changes to the current line
        let mut state = ParseState::new();
        let result = parse_normal_key(&mut state, Key::Char('U'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::SimpleAction {
                action: SimpleAction::UndoLine,
                count: 1,
            })
        );
    }

    #[test]
    fn test_parse_g_tilde_tilde_self_application() {
        // Second `~` while in OperatorPending(ToggleCase) → linewise self-apply
        let mut state = ParseState::new();
        let result = parse_normal_key(
            &mut state,
            Key::Char('~'),
            Mode::OperatorPending(Operator::ToggleCase),
        );
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::OperatorMotion {
                operator: Operator::ToggleCase,
                motion: Motion::Down,
                count: 1,
                register: None,
            })
        );
    }

    #[test]
    fn test_parse_tilde_in_normal_is_toggle_case() {
        // `~` in Normal mode must still be SimpleAction::ToggleCase
        let mut state = ParseState::new();
        let result = parse_normal_key(&mut state, Key::Char('~'), Mode::Normal);
        assert_eq!(
            result,
            ParseResult::Complete(ParsedCommand::SimpleAction {
                action: SimpleAction::ToggleCase,
                count: 1,
            })
        );
    }
}

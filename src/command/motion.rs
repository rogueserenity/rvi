//! Motion types and execution.
//!
//! This module defines all vi motion types and provides functions to execute them.
//! Motions calculate target cursor positions and affected text ranges.

use crate::buffer::unicode::{classify_char, next_grapheme_boundary, prev_grapheme_boundary};
use crate::buffer::{Buffer, Cursor};

/// Direction for find motions (f/F/t/T).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindDirection {
    /// Search forward from cursor (f, t)
    Forward,
    /// Search backward from cursor (F, T)
    Backward,
}

/// Stop position for find motions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindStop {
    /// Stop on the target character (f, F)
    OnChar,
    /// Stop before/after the target character (t, T)
    BeforeChar,
}

/// All vi motion types.
///
/// Using an enum ensures compile-time exhaustive matching when handling motions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    // Character motions
    /// Move left one character (h)
    Left,
    /// Move right one character (l)
    Right,
    /// Move up one line (k)
    Up,
    /// Move down one line (j)
    Down,

    // Word motions
    /// Move to start of next word (w)
    WordForward,
    /// Move to start of previous word (b)
    WordBackward,
    /// Move to end of current/next word (e)
    WordEnd,
    /// Move to start of next WORD - whitespace delimited (W)
    WORDForward,
    /// Move to start of previous WORD (B)
    WORDBackward,
    /// Move to end of current/next WORD (E)
    WORDEnd,

    // Line motions
    /// Move to start of line (0)
    LineStart,
    /// Move to first non-blank character (^)
    FirstNonBlank,
    /// Move to end of line ($)
    LineEnd,

    // Document motions
    /// Move to start of document (gg)
    DocumentStart,
    /// Move to end of document (G without count)
    DocumentEnd,
    /// Move to specific line number (nG or ngg, 1-indexed)
    GotoLine(usize),

    // Find motions
    /// Find character on line (f/F/t/T)
    FindChar {
        ch: char,
        direction: FindDirection,
        stop: FindStop,
    },
    /// Repeat last find motion (;)
    RepeatFind,
    /// Repeat last find motion in reverse direction (,)
    RepeatFindReverse,

    // Bracket matching
    /// Jump to matching bracket (%)
    MatchingBracket,

    // Paragraph/sentence motions (Phase 3+)
    /// Move to next paragraph (})
    ParagraphForward,
    /// Move to previous paragraph ({)
    ParagraphBackward,
    /// Move to next sentence ())
    SentenceForward,
    /// Move to previous sentence (()
    SentenceBackward,

    // Screen position motions
    /// Move to top of screen, optional offset lines from top (H)
    ScreenTop(usize),
    /// Move to middle of screen (M)
    ScreenMiddle,
    /// Move to bottom of screen, optional offset lines from bottom (L)
    ScreenBottom(usize),

    // Section motions
    /// Move to next section start ([[)
    SectionBackward,
    /// Move to previous section start ([[)
    SectionForward,

    // First-non-blank line motions (Enter, +, -, _)
    /// Move to first non-blank of next line (Enter, +)
    NextLineFirstNonBlank,
    /// Move to first non-blank of previous line (-)
    PrevLineFirstNonBlank,
    /// Move to first non-blank of current line + (n-1) lines below (_)
    CurrentLineFirstNonBlank,

    /// Move to screen column N ({count}|)
    Column,
}

/// How a motion affects text when used with an operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionKind {
    /// End position is NOT included (most horizontal motions: h, l, w, b)
    Exclusive,
    /// End position IS included (e, f, t, $)
    Inclusive,
    /// Operates on whole lines (j, k, G, gg)
    Linewise,
}

impl Motion {
    /// Get the kind of this motion for operator behavior.
    pub fn kind(&self) -> MotionKind {
        match self {
            // Exclusive motions
            Motion::Left
            | Motion::Right
            | Motion::WordForward
            | Motion::WordBackward
            | Motion::WORDForward
            | Motion::WORDBackward
            | Motion::LineStart
            | Motion::FirstNonBlank => MotionKind::Exclusive,

            // Inclusive motions
            Motion::WordEnd
            | Motion::WORDEnd
            | Motion::LineEnd
            | Motion::FindChar { .. }
            | Motion::RepeatFind
            | Motion::RepeatFindReverse
            | Motion::MatchingBracket => MotionKind::Inclusive,

            // Linewise motions
            Motion::Up
            | Motion::Down
            | Motion::DocumentStart
            | Motion::DocumentEnd
            | Motion::GotoLine(_)
            | Motion::ParagraphForward
            | Motion::ParagraphBackward
            | Motion::ScreenTop(_)
            | Motion::ScreenMiddle
            | Motion::ScreenBottom(_)
            | Motion::SectionForward
            | Motion::SectionBackward => MotionKind::Linewise,

            Motion::SentenceForward | Motion::SentenceBackward => MotionKind::Exclusive,

            Motion::Column => MotionKind::Exclusive,

            Motion::NextLineFirstNonBlank
            | Motion::PrevLineFirstNonBlank
            | Motion::CurrentLineFirstNonBlank => MotionKind::Linewise,
        }
    }
}

/// Range of text affected by a motion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MotionRange {
    /// Start of the range (always <= end)
    pub start: Cursor,
    /// End of the range
    pub end: Cursor,
}

impl MotionRange {
    /// Create a new motion range, ensuring start <= end.
    pub fn new(a: Cursor, b: Cursor) -> Self {
        if a <= b {
            Self { start: a, end: b }
        } else {
            Self { start: b, end: a }
        }
    }
}

/// Result of executing a motion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MotionResult {
    /// Target cursor position after the motion
    pub target: Cursor,
    /// The range affected by this motion (for operators)
    pub range: MotionRange,
    /// Kind of motion (determines operator behavior)
    pub kind: MotionKind,
}

/// Immutable context for executing motions.
///
/// Borrows from the document to avoid mutation during motion calculation.
pub struct CommandContext<'a> {
    /// Reference to the text buffer
    pub buffer: &'a Buffer,
    /// Current cursor position
    pub cursor: Cursor,
    /// Paragraph macro names from the `paragraphs` setting (e.g. `"IPLPPPQPP LIpplpipbp"`).
    /// Used by `{`/`}` to recognize nroff macro lines as paragraph boundaries.
    /// Empty string disables macro matching (only empty lines are boundaries).
    pub paragraphs: &'a str,
    /// Section macro names from the `sections` setting (e.g. `"NHSHH HUnhsh"`).
    /// Used by `[[`/`]]` to recognize nroff macro lines as section boundaries.
    /// Empty string disables macro matching (only `{` / form-feed lines are boundaries).
    pub sections: &'a str,
    /// Tab stop width for the `|` column motion. Matches the `tabstop` setting.
    pub tabstop: usize,
}

/// Execute a motion and return the result.
///
/// # Arguments
///
/// * `motion` - The motion to execute
/// * `ctx` - Command context with buffer and cursor
/// * `count` - Repeat count (default 1)
/// * `last_find` - Last f/F/t/T for ; and , commands
///
/// # Returns
///
/// `Some(MotionResult)` if the motion can be executed, `None` if the motion
/// cannot be performed (e.g., find char not found).
pub fn execute_motion(
    motion: Motion,
    ctx: &CommandContext,
    count: usize,
    last_find: Option<(char, FindDirection, FindStop)>,
) -> Option<MotionResult> {
    let count = count.max(1);

    match motion {
        Motion::Left => execute_left(ctx, count),
        Motion::Right => execute_right(ctx, count),
        Motion::Up => execute_up(ctx, count),
        Motion::Down => execute_down(ctx, count),

        Motion::WordForward => execute_word_forward(ctx, count, false),
        Motion::WordBackward => execute_word_backward(ctx, count, false),
        Motion::WordEnd => execute_word_end(ctx, count, false),
        Motion::WORDForward => execute_word_forward(ctx, count, true),
        Motion::WORDBackward => execute_word_backward(ctx, count, true),
        Motion::WORDEnd => execute_word_end(ctx, count, true),

        Motion::LineStart => execute_line_start(ctx),
        Motion::FirstNonBlank => execute_first_non_blank(ctx),
        Motion::LineEnd => execute_line_end(ctx, count),

        Motion::DocumentStart => execute_document_start(ctx),
        Motion::DocumentEnd => execute_document_end(ctx),
        Motion::GotoLine(line) => execute_goto_line(ctx, line),

        Motion::FindChar {
            ch,
            direction,
            stop,
        } => execute_find_char(ctx, ch, direction, stop, count),
        Motion::RepeatFind => {
            last_find.and_then(|(ch, dir, stop)| execute_find_char(ctx, ch, dir, stop, count))
        }
        Motion::RepeatFindReverse => last_find.and_then(|(ch, dir, stop)| {
            let reverse_dir = match dir {
                FindDirection::Forward => FindDirection::Backward,
                FindDirection::Backward => FindDirection::Forward,
            };
            execute_find_char(ctx, ch, reverse_dir, stop, count)
        }),

        Motion::MatchingBracket => execute_matching_bracket(ctx),

        Motion::ParagraphForward => execute_paragraph_forward(ctx, count),
        Motion::ParagraphBackward => execute_paragraph_backward(ctx, count),
        Motion::SentenceForward => execute_sentence_forward(ctx, count),
        Motion::SentenceBackward => execute_sentence_backward(ctx, count),

        // Screen position motions are resolved in the editor (need viewport data).
        // They should not reach execute_motion, but return None gracefully if they do.
        Motion::ScreenTop(_) | Motion::ScreenMiddle | Motion::ScreenBottom(_) => None,

        Motion::SectionForward => execute_section_forward(ctx, count),
        Motion::SectionBackward => execute_section_backward(ctx, count),

        Motion::NextLineFirstNonBlank => execute_next_line_first_non_blank(ctx, count),
        Motion::PrevLineFirstNonBlank => execute_prev_line_first_non_blank(ctx, count),
        Motion::CurrentLineFirstNonBlank => execute_current_line_first_non_blank(ctx, count),

        Motion::Column => execute_column(ctx, count),
    }
}

/// Execute the column motion ({count}|).
///
/// Moves to the screen column given by `count` (1-based). Walks graphemes
/// accumulating display width until the target column is reached. Without
/// a count (count=1), moves to column 1 (byte offset 0).
fn execute_column(ctx: &CommandContext, count: usize) -> Option<MotionResult> {
    use unicode_segmentation::UnicodeSegmentation;

    let line = ctx.buffer.line(ctx.cursor.row)?;
    let target_col = count; // 1-based screen column

    if target_col <= 1 || line.is_empty() {
        let target = Cursor::new(ctx.cursor.row, 0);
        return Some(MotionResult {
            target,
            range: MotionRange::new(ctx.cursor, target),
            kind: Motion::Column.kind(),
        });
    }

    // Walk graphemes accumulating display width
    let mut byte_offset = 0;
    let mut display_col: usize = 0;
    let mut last_valid_offset = 0;

    for grapheme in line.graphemes(true) {
        if display_col >= target_col - 1 {
            break;
        }
        last_valid_offset = byte_offset;
        byte_offset += grapheme.len();

        // Calculate display width of this grapheme
        let gw: usize = grapheme
            .chars()
            .map(|c| {
                if c == '\t' {
                    // Tab: next tab stop from current display_col
                    let next_stop = ((display_col / ctx.tabstop) + 1) * ctx.tabstop;
                    next_stop - display_col
                } else {
                    unicode_width::UnicodeWidthChar::width(c).unwrap_or(0)
                }
            })
            .sum();
        display_col += gw;
    }

    // If we went past the target, use the last valid offset
    let final_offset = if display_col > target_col - 1 && byte_offset > 0 {
        last_valid_offset
    } else {
        // Clamp to last char on line (normal mode constraint)
        let max_col = if line.is_empty() {
            0
        } else {
            prev_grapheme_boundary(line, line.len())
        };
        byte_offset.min(max_col)
    };

    let target = Cursor::new(ctx.cursor.row, final_offset);
    Some(MotionResult {
        target,
        range: MotionRange::new(ctx.cursor, target),
        kind: Motion::Column.kind(),
    })
}

// Character motion implementations

fn execute_left(ctx: &CommandContext, count: usize) -> Option<MotionResult> {
    let line = ctx.buffer.line(ctx.cursor.row)?;
    let mut col = ctx.cursor.col;

    for _ in 0..count {
        if col == 0 {
            break;
        }
        col = prev_grapheme_boundary(line, col);
    }

    let target = Cursor::new(ctx.cursor.row, col);
    Some(MotionResult {
        target,
        range: MotionRange::new(ctx.cursor, target),
        kind: Motion::Left.kind(),
    })
}

fn execute_right(ctx: &CommandContext, count: usize) -> Option<MotionResult> {
    let line = ctx.buffer.line(ctx.cursor.row)?;
    let mut col = ctx.cursor.col;

    // In normal mode, cursor cannot go past last character
    // The last valid position is the start of the last grapheme
    let max_col = if line.is_empty() {
        0
    } else {
        prev_grapheme_boundary(line, line.len())
    };

    for _ in 0..count {
        if col >= max_col {
            break;
        }
        let new_col = next_grapheme_boundary(line, col);
        if new_col > max_col {
            break;
        }
        col = new_col;
    }

    let target = Cursor::new(ctx.cursor.row, col);
    Some(MotionResult {
        target,
        range: MotionRange::new(ctx.cursor, target),
        kind: Motion::Right.kind(),
    })
}

fn execute_up(ctx: &CommandContext, count: usize) -> Option<MotionResult> {
    let new_row = ctx.cursor.row.saturating_sub(count);
    let new_col = clamp_col_to_line(ctx.buffer, new_row, ctx.cursor.col);

    let target = Cursor::new(new_row, new_col);
    Some(MotionResult {
        target,
        range: MotionRange::new(ctx.cursor, target),
        kind: Motion::Up.kind(),
    })
}

fn execute_down(ctx: &CommandContext, count: usize) -> Option<MotionResult> {
    let last_row = ctx.buffer.len().saturating_sub(1);
    let new_row = (ctx.cursor.row + count).min(last_row);
    let new_col = clamp_col_to_line(ctx.buffer, new_row, ctx.cursor.col);

    let target = Cursor::new(new_row, new_col);
    Some(MotionResult {
        target,
        range: MotionRange::new(ctx.cursor, target),
        kind: Motion::Down.kind(),
    })
}

fn execute_word_forward(
    ctx: &CommandContext,
    count: usize,
    big_word: bool,
) -> Option<MotionResult> {
    let mut row = ctx.cursor.row;
    let mut col = ctx.cursor.col;

    for _ in 0..count {
        let line = ctx.buffer.line(row)?;

        // col is a byte offset; use char_indices on line[col..] to avoid
        // allocating a Vec<char> and re-counting to convert back.
        let rest = &line[col..];
        let current_char = match rest.chars().next() {
            Some(c) => c,
            None => {
                // At end of line — move to next line
                if row + 1 < ctx.buffer.len() {
                    row += 1;
                    col = 0;
                    if let Some(new_line) = ctx.buffer.line(row) {
                        col = skip_whitespace_forward(new_line, 0);
                    }
                }
                continue;
            }
        };
        let current_class = classify_char(current_char, big_word);

        // Skip current word (same class), tracking byte offset within `rest`.
        let mut word_end = rest.len(); // default: end of line
        for (off, ch) in rest.char_indices() {
            if classify_char(ch, big_word) != current_class {
                word_end = off;
                break;
            }
        }

        // Skip whitespace after the word.
        let after_word = &rest[word_end..];
        let mut ws_end = after_word.len(); // default: end of line
        for (off, ch) in after_word.char_indices() {
            if !ch.is_whitespace() {
                ws_end = off;
                break;
            }
        }
        let new_col = col + word_end + ws_end;

        if new_col >= line.len() {
            // End of line, move to next line
            if row + 1 < ctx.buffer.len() {
                row += 1;
                col = 0;
                if let Some(new_line) = ctx.buffer.line(row) {
                    col = skip_whitespace_forward(new_line, 0);
                }
            } else {
                // No next line: set col to line.len() so that operator ranges
                // (which treat end as exclusive) include the final character.
                // Standalone motion callers are responsible for clamping the
                // cursor to the last valid position before displaying it.
                col = line.len();
            }
        } else {
            col = new_col;
        }
    }

    let target = Cursor::new(row, col);
    Some(MotionResult {
        target,
        range: MotionRange::new(ctx.cursor, target),
        kind: if big_word {
            Motion::WORDForward.kind()
        } else {
            Motion::WordForward.kind()
        },
    })
}

fn execute_word_backward(
    ctx: &CommandContext,
    count: usize,
    big_word: bool,
) -> Option<MotionResult> {
    let mut row = ctx.cursor.row;
    let mut col = ctx.cursor.col;

    for _ in 0..count {
        let line = ctx.buffer.line(row)?;

        if col == 0 {
            // At start of line, move to previous line
            if row > 0 {
                row -= 1;
                if let Some(prev_line) = ctx.buffer.line(row) {
                    col = if prev_line.is_empty() {
                        0
                    } else {
                        prev_grapheme_boundary(prev_line, prev_line.len())
                    };
                }
            }
            continue;
        }

        // Walk backwards through line[..col] without allocating.
        let mut rev = line[..col].char_indices().rev().peekable();

        if rev.peek().is_none() {
            // At start of line
            if row > 0 {
                row -= 1;
                if let Some(prev_line) = ctx.buffer.line(row) {
                    col = if prev_line.is_empty() {
                        0
                    } else {
                        prev_grapheme_boundary(prev_line, prev_line.len())
                    };
                }
            }
            continue;
        }

        // Skip whitespace backwards; find first non-whitespace char.
        let (mut word_off, word_ch) = loop {
            match rev.next() {
                None => {
                    col = 0;
                    break (0, ' '); // sentinel — col = 0 means "go to next count iter"
                }
                Some((off, ch)) if !ch.is_whitespace() => break (off, ch),
                _ => {}
            }
        };

        if col == 0 {
            // All chars before cursor were whitespace
            continue;
        }

        // Find the class of the character we landed on.
        let target_class = classify_char(word_ch, big_word);

        // Walk back while same class to find start of the word.
        for (off, ch) in rev {
            if classify_char(ch, big_word) != target_class {
                break;
            }
            word_off = off;
        }

        col = word_off;
    }

    let target = Cursor::new(row, col);
    Some(MotionResult {
        target,
        range: MotionRange::new(ctx.cursor, target),
        kind: if big_word {
            Motion::WORDBackward.kind()
        } else {
            Motion::WordBackward.kind()
        },
    })
}

fn execute_word_end(ctx: &CommandContext, count: usize, big_word: bool) -> Option<MotionResult> {
    let mut row = ctx.cursor.row;
    let mut col = ctx.cursor.col;

    for _ in 0..count {
        let line = ctx.buffer.line(row)?;

        if line.is_empty() {
            // Empty line, try next line
            if row + 1 < ctx.buffer.len() {
                row += 1;
                col = 0;
            }
            continue;
        }

        // Advance one char past col, then scan forward — all in byte space.
        let after_cur = next_grapheme_boundary(line, col);

        // Skip whitespace starting from after_cur
        let ws_end = line[after_cur..]
            .char_indices()
            .find(|(_, c)| !c.is_whitespace())
            .map(|(off, _)| after_cur + off)
            .unwrap_or(line.len());

        if ws_end >= line.len() {
            // End of line, try next line
            if row + 1 < ctx.buffer.len() {
                row += 1;
                col = 0;
                if let Some(new_line) = ctx.buffer.line(row) {
                    // Skip leading whitespace on next line
                    let start = new_line
                        .char_indices()
                        .find(|(_, c)| !c.is_whitespace())
                        .map(|(off, _)| off)
                        .unwrap_or(new_line.len());
                    if start < new_line.len() {
                        let word_class = classify_char(
                            new_line[start..].chars().next().unwrap(),
                            big_word,
                        );
                        // Advance to end of this word
                        let end = new_line[start..]
                            .char_indices()
                            .take_while(|(_, c)| classify_char(*c, big_word) == word_class)
                            .last()
                            .map(|(off, _)| start + off)
                            .unwrap_or(start);
                        col = end;
                    }
                }
            } else {
                // Stay at end of last line
                col = prev_grapheme_boundary(line, line.len());
            }
            continue;
        }

        // Find the class of the character at ws_end
        let word_class = classify_char(line[ws_end..].chars().next().unwrap(), big_word);

        // Move to end of this word (last char with same class)
        let end = line[ws_end..]
            .char_indices()
            .take_while(|(_, c)| classify_char(*c, big_word) == word_class)
            .last()
            .map(|(off, _)| ws_end + off)
            .unwrap_or(ws_end);
        col = end;
    }

    let target = Cursor::new(row, col);
    Some(MotionResult {
        target,
        range: MotionRange::new(ctx.cursor, target),
        kind: if big_word {
            Motion::WORDEnd.kind()
        } else {
            Motion::WordEnd.kind()
        },
    })
}

fn skip_whitespace_forward(line: &str, start_byte: usize) -> usize {
    line[start_byte..]
        .char_indices()
        .find(|(_, c)| !c.is_whitespace())
        .map(|(off, _)| start_byte + off)
        .unwrap_or(line.len())
}

// Line motion implementations

fn execute_line_start(ctx: &CommandContext) -> Option<MotionResult> {
    let target = Cursor::new(ctx.cursor.row, 0);
    Some(MotionResult {
        target,
        range: MotionRange::new(ctx.cursor, target),
        kind: Motion::LineStart.kind(),
    })
}

fn execute_next_line_first_non_blank(ctx: &CommandContext, count: usize) -> Option<MotionResult> {
    let last_row = ctx.buffer.len().saturating_sub(1);
    let new_row = (ctx.cursor.row + count).min(last_row);
    let new_col = first_non_blank_col(ctx.buffer, new_row);
    let target = Cursor::new(new_row, new_col);
    Some(MotionResult {
        target,
        range: MotionRange::new(ctx.cursor, target),
        kind: MotionKind::Linewise,
    })
}

fn execute_prev_line_first_non_blank(ctx: &CommandContext, count: usize) -> Option<MotionResult> {
    let new_row = ctx.cursor.row.saturating_sub(count);
    let new_col = first_non_blank_col(ctx.buffer, new_row);
    let target = Cursor::new(new_row, new_col);
    Some(MotionResult {
        target,
        range: MotionRange::new(ctx.cursor, target),
        kind: MotionKind::Linewise,
    })
}

fn execute_current_line_first_non_blank(
    ctx: &CommandContext,
    count: usize,
) -> Option<MotionResult> {
    // `_` moves to first non-blank of the line (count - 1) lines below current
    let last_row = ctx.buffer.len().saturating_sub(1);
    let new_row = (ctx.cursor.row + count.saturating_sub(1)).min(last_row);
    let new_col = first_non_blank_col(ctx.buffer, new_row);
    let target = Cursor::new(new_row, new_col);
    Some(MotionResult {
        target,
        range: MotionRange::new(ctx.cursor, target),
        kind: MotionKind::Linewise,
    })
}

fn execute_first_non_blank(ctx: &CommandContext) -> Option<MotionResult> {
    let line = ctx.buffer.line(ctx.cursor.row)?;

    // Find first non-whitespace character
    let col = line
        .char_indices()
        .find(|(_, ch)| !ch.is_whitespace())
        .map(|(idx, _)| idx)
        .unwrap_or(0);

    let target = Cursor::new(ctx.cursor.row, col);
    Some(MotionResult {
        target,
        range: MotionRange::new(ctx.cursor, target),
        kind: Motion::FirstNonBlank.kind(),
    })
}

fn execute_line_end(ctx: &CommandContext, count: usize) -> Option<MotionResult> {
    // {count}$ moves to the end of the line count-1 lines below (POSIX).
    let target_row = (ctx.cursor.row + count - 1).min(ctx.buffer.len().saturating_sub(1));
    let line = ctx.buffer.line(target_row)?;

    // $ moves to the last character of the line (not past it)
    let col = if line.is_empty() {
        0
    } else {
        prev_grapheme_boundary(line, line.len())
    };

    let target = Cursor::new(target_row, col);
    Some(MotionResult {
        target,
        range: MotionRange::new(ctx.cursor, target),
        kind: Motion::LineEnd.kind(),
    })
}

// Document motion implementations

fn execute_document_start(ctx: &CommandContext) -> Option<MotionResult> {
    let line = ctx.buffer.line(0)?;

    // gg goes to first non-blank of first line
    let col = line
        .char_indices()
        .find(|(_, ch)| !ch.is_whitespace())
        .map(|(idx, _)| idx)
        .unwrap_or(0);

    let target = Cursor::new(0, col);
    Some(MotionResult {
        target,
        range: MotionRange::new(ctx.cursor, target),
        kind: Motion::DocumentStart.kind(),
    })
}

fn execute_document_end(ctx: &CommandContext) -> Option<MotionResult> {
    let last_row = ctx.buffer.len().saturating_sub(1);
    let line = ctx.buffer.line(last_row)?;

    // G goes to first non-blank of last line
    let col = line
        .char_indices()
        .find(|(_, ch)| !ch.is_whitespace())
        .map(|(idx, _)| idx)
        .unwrap_or(0);

    let target = Cursor::new(last_row, col);
    Some(MotionResult {
        target,
        range: MotionRange::new(ctx.cursor, target),
        kind: Motion::DocumentEnd.kind(),
    })
}

fn execute_goto_line(ctx: &CommandContext, line_num: usize) -> Option<MotionResult> {
    // Line numbers are 1-indexed in vi, convert to 0-indexed
    let target_row = line_num
        .saturating_sub(1)
        .min(ctx.buffer.len().saturating_sub(1));
    let line = ctx.buffer.line(target_row)?;

    // Go to first non-blank of target line
    let col = line
        .char_indices()
        .find(|(_, ch)| !ch.is_whitespace())
        .map(|(idx, _)| idx)
        .unwrap_or(0);

    let target = Cursor::new(target_row, col);
    Some(MotionResult {
        target,
        range: MotionRange::new(ctx.cursor, target),
        kind: Motion::GotoLine(line_num).kind(),
    })
}

// Find motion implementations

fn execute_find_char(
    ctx: &CommandContext,
    ch: char,
    direction: FindDirection,
    stop: FindStop,
    count: usize,
) -> Option<MotionResult> {
    let line = ctx.buffer.line(ctx.cursor.row)?;
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let current_char_idx = line[..ctx.cursor.col].chars().count();

    let mut found_byte_offset = None;
    let mut found_count = 0;

    match direction {
        FindDirection::Forward => {
            // Search forward from current position
            for (byte_idx, c) in chars.iter().skip(current_char_idx + 1) {
                if *c == ch {
                    found_count += 1;
                    if found_count == count {
                        found_byte_offset = Some(*byte_idx);
                        break;
                    }
                }
            }
        }
        FindDirection::Backward => {
            // Search backward from current position
            for (byte_idx, c) in chars.iter().take(current_char_idx).rev() {
                if *c == ch {
                    found_count += 1;
                    if found_count == count {
                        found_byte_offset = Some(*byte_idx);
                        break;
                    }
                }
            }
        }
    }

    let target_col = match (found_byte_offset, stop, direction) {
        (Some(offset), FindStop::OnChar, _) => offset,
        (Some(offset), FindStop::BeforeChar, FindDirection::Forward) => {
            // t: stop before the character
            prev_grapheme_boundary(line, offset)
        }
        (Some(offset), FindStop::BeforeChar, FindDirection::Backward) => {
            // T: stop after the character (moving backward)
            next_grapheme_boundary(line, offset)
        }
        (None, _, _) => return None,
    };

    let target = Cursor::new(ctx.cursor.row, target_col);
    Some(MotionResult {
        target,
        range: MotionRange::new(ctx.cursor, target),
        kind: Motion::FindChar {
            ch,
            direction,
            stop,
        }
        .kind(),
    })
}

// Matching bracket implementation

/// The bracket pairs recognized by the `%` command.
const BRACKET_PAIRS: &[(char, char)] = &[('(', ')'), ('[', ']'), ('{', '}')];

/// Get the matching bracket character and whether `ch` is an opening bracket.
fn bracket_match_info(ch: char) -> Option<(char, bool)> {
    for &(open, close) in BRACKET_PAIRS {
        if ch == open {
            return Some((close, true));
        }
        if ch == close {
            return Some((open, false));
        }
    }
    None
}

/// Check if a character is any bracket character.
fn is_bracket(ch: char) -> bool {
    bracket_match_info(ch).is_some()
}

/// Execute the matching bracket motion (%).
///
/// If the cursor is on a bracket, jump to its match. Otherwise, scan forward
/// on the current line for the first bracket and jump to its match.
fn execute_matching_bracket(ctx: &CommandContext) -> Option<MotionResult> {
    let line = ctx.buffer.line(ctx.cursor.row)?;
    if line.is_empty() {
        return None;
    }

    // Determine which bracket to match: either the char under cursor or
    // the first bracket found scanning forward on the current line.
    let (bracket_row, bracket_col, bracket_char) = {
        let chars: Vec<(usize, char)> = line.char_indices().collect();
        let cursor_char_idx = line[..ctx.cursor.col].chars().count();

        // Check if cursor is on a bracket
        if cursor_char_idx < chars.len() && is_bracket(chars[cursor_char_idx].1) {
            (
                ctx.cursor.row,
                chars[cursor_char_idx].0,
                chars[cursor_char_idx].1,
            )
        } else {
            // Scan forward from cursor for first bracket on this line
            let mut found = None;
            for &(byte_off, ch) in chars.iter().skip(cursor_char_idx) {
                if is_bracket(ch) {
                    found = Some((ctx.cursor.row, byte_off, ch));
                    break;
                }
            }
            found?
        }
    };

    let (match_char, is_opening) = bracket_match_info(bracket_char)?;

    // Search for the matching bracket through the buffer
    let target = if is_opening {
        find_matching_bracket_forward(
            ctx.buffer,
            bracket_row,
            bracket_col,
            bracket_char,
            match_char,
        )
    } else {
        find_matching_bracket_backward(
            ctx.buffer,
            bracket_row,
            bracket_col,
            bracket_char,
            match_char,
        )
    }?;

    Some(MotionResult {
        target,
        range: MotionRange::new(ctx.cursor, target),
        kind: Motion::MatchingBracket.kind(),
    })
}

/// Scan forward through the buffer to find the matching closing bracket.
///
/// Tracks nesting depth to find the correct match.
fn find_matching_bracket_forward(
    buffer: &Buffer,
    start_row: usize,
    start_col: usize,
    open: char,
    close: char,
) -> Option<Cursor> {
    let mut depth: i32 = 0;
    let line_count = buffer.len();

    for row in start_row..line_count {
        let line = buffer.line(row)?;
        let start = if row == start_row { start_col } else { 0 };

        for (byte_off, ch) in line[start..].char_indices() {
            let abs_col = start + byte_off;
            if ch == open {
                depth += 1;
            } else if ch == close {
                depth -= 1;
                if depth == 0 {
                    return Some(Cursor::new(row, abs_col));
                }
            }
        }
    }

    None
}

/// Scan backward through the buffer to find the matching opening bracket.
///
/// Tracks nesting depth to find the correct match.
fn find_matching_bracket_backward(
    buffer: &Buffer,
    start_row: usize,
    start_col: usize,
    close: char,
    open: char,
) -> Option<Cursor> {
    let mut depth: i32 = 0;

    for row in (0..=start_row).rev() {
        let line = buffer.line(row)?;
        // Collect chars with byte offsets, then iterate in reverse
        let chars: Vec<(usize, char)> = line.char_indices().collect();

        let end = if row == start_row {
            // Include the character at start_col
            chars
                .iter()
                .position(|(off, _)| *off > start_col)
                .unwrap_or(chars.len())
        } else {
            chars.len()
        };

        for &(byte_off, ch) in chars[..end].iter().rev() {
            if ch == close {
                depth += 1;
            } else if ch == open {
                depth -= 1;
                if depth == 0 {
                    return Some(Cursor::new(row, byte_off));
                }
            }
        }
    }

    None
}

// Paragraph and sentence motion implementations

/// Check whether `line` is an nroff macro boundary matching one of the 2-char
/// pairs encoded in `macros`.
///
/// `macros` is a space-separated sequence of 2-character macro names, e.g.
/// `"IPLP PP"`. A line matches if it starts with `.` followed by one of those
/// 2-char names (optionally followed by more text). Spaces in `macros` are
/// ignored as separators.
fn is_macro_boundary(line: &str, macros: &str) -> bool {
    if macros.is_empty() {
        return false;
    }
    let Some(rest) = line.strip_prefix('.') else {
        return false;
    };
    // Iterate 2-char pairs from the macros string (skip spaces)
    let mut it = macros.chars().filter(|c| *c != ' ');
    while let (Some(a), Some(b)) = (it.next(), it.next()) {
        let pat_str: String = [a, b].iter().collect();
        if rest == pat_str
            || rest.starts_with(&pat_str)
                && rest[pat_str.len()..].starts_with(|c: char| !c.is_alphanumeric())
        {
            return true;
        }
    }
    false
}

/// A paragraph boundary is an empty line or a nroff macro line from `paragraphs`.
/// `}` moves to the next boundary (or EOF).
fn execute_paragraph_forward(ctx: &CommandContext, count: usize) -> Option<MotionResult> {
    let total = ctx.buffer.len();
    let mut row = ctx.cursor.row;

    let is_para_boundary = |r: usize| -> bool {
        match ctx.buffer.line(r) {
            Some(l) => l.is_empty() || is_macro_boundary(l, ctx.paragraphs),
            None => false,
        }
    };

    for _ in 0..count {
        // Skip over any consecutive boundary lines we're already on
        while row < total && is_para_boundary(row) {
            row += 1;
        }
        // Skip the non-boundary paragraph body
        while row < total && !is_para_boundary(row) {
            row += 1;
        }
        // `row` is now on a boundary line or past EOF
        if row >= total {
            row = total.saturating_sub(1);
        }
    }

    let col = first_non_blank_col(ctx.buffer, row);
    let target = Cursor::new(row, col);
    Some(MotionResult {
        target,
        range: MotionRange::new(ctx.cursor, target),
        kind: Motion::ParagraphForward.kind(),
    })
}

/// `{` moves to the previous paragraph boundary.
fn execute_paragraph_backward(ctx: &CommandContext, count: usize) -> Option<MotionResult> {
    let mut row = ctx.cursor.row;

    let is_para_boundary = |r: usize| -> bool {
        match ctx.buffer.line(r) {
            Some(l) => l.is_empty() || is_macro_boundary(l, ctx.paragraphs),
            None => false,
        }
    };

    for _ in 0..count {
        // Step back at least one line so we don't stay put
        row = row.saturating_sub(1);
        // Skip consecutive boundary lines upward
        while row > 0 && is_para_boundary(row) {
            row -= 1;
        }
        // Skip the non-boundary paragraph body upward
        while row > 0 && !is_para_boundary(row) {
            row -= 1;
        }
        // `row` is now on the boundary, or at row 0
    }

    let col = first_non_blank_col(ctx.buffer, row);
    let target = Cursor::new(row, col);
    Some(MotionResult {
        target,
        range: MotionRange::new(ctx.cursor, target),
        kind: Motion::ParagraphBackward.kind(),
    })
}

/// A sentence ends at `.`, `!`, or `?` followed by end-of-line or at least two spaces,
/// or at a paragraph boundary (empty line). `)` moves to the start of the next sentence.
fn execute_sentence_forward(ctx: &CommandContext, count: usize) -> Option<MotionResult> {
    let total = ctx.buffer.len();
    let mut row = ctx.cursor.row;
    let mut col = ctx.cursor.col;

    for _ in 0..count {
        'outer: while row < total {
            let line = match ctx.buffer.line(row) {
                Some(l) => l,
                None => break,
            };

            // Empty line is a paragraph/sentence boundary — move past it
            if line.is_empty() {
                row += 1;
                // Skip further consecutive empty lines
                while row < total && ctx.buffer.line(row).is_some_and(|l| l.is_empty()) {
                    row += 1;
                }
                col = if row < total {
                    first_non_blank_col(ctx.buffer, row)
                } else {
                    row = total.saturating_sub(1);
                    0
                };
                break 'outer;
            }

            let chars: Vec<(usize, char)> = line.char_indices().collect();
            let start_idx = if col == 0 {
                0
            } else {
                // Find char index at current byte col
                chars
                    .iter()
                    .position(|(b, _)| *b >= col)
                    .unwrap_or(chars.len())
            };

            for i in start_idx..chars.len() {
                let ch = chars[i].1;
                if matches!(ch, '.' | '!' | '?') {
                    // Check what follows: end-of-line or two spaces
                    let after = &chars[i + 1..];
                    let sentence_end = after.is_empty()
                        || (after.len() >= 2 && after[0].1 == ' ' && after[1].1 == ' ');
                    if sentence_end {
                        // Skip past the punctuation and trailing spaces
                        let mut j = i + 1;
                        while j < chars.len() && chars[j].1 == ' ' {
                            j += 1;
                        }
                        if j < chars.len() {
                            col = chars[j].0;
                        } else {
                            // End of line — move to next line
                            row += 1;
                            col = if row < total {
                                first_non_blank_col(ctx.buffer, row)
                            } else {
                                row = total.saturating_sub(1);
                                0
                            };
                        }
                        break 'outer;
                    }
                }
            }

            // No sentence boundary on this line — move to next
            row += 1;
            col = 0;
        }

        if row >= total {
            row = total.saturating_sub(1);
        }
    }

    let target = Cursor::new(row, col);
    Some(MotionResult {
        target,
        range: MotionRange::new(ctx.cursor, target),
        kind: Motion::SentenceForward.kind(),
    })
}

/// `(` moves to the start of the current or previous sentence.
fn execute_sentence_backward(ctx: &CommandContext, count: usize) -> Option<MotionResult> {
    let mut row = ctx.cursor.row;
    let mut col = ctx.cursor.col;

    for _ in 0..count {
        // Step back past any leading whitespace, then one more character, so we
        // don't immediately re-detect the sentence boundary the cursor is already on.
        loop {
            if col > 0 {
                if let Some(line) = ctx.buffer.line(row) {
                    let prev = prev_grapheme_boundary(line, col);
                    let ch = line[prev..col].chars().next().unwrap_or(' ');
                    col = prev;
                    if ch != ' ' {
                        break;
                    }
                } else {
                    break;
                }
            } else if row > 0 {
                row -= 1;
                col = ctx.buffer.line(row).map_or(0, |l| l.len());
                break;
            } else {
                // Already at buffer start
                break;
            }
        }

        'outer: loop {
            let line = match ctx.buffer.line(row) {
                Some(l) => l,
                None => {
                    row = 0;
                    col = 0;
                    break;
                }
            };

            // Empty line = paragraph boundary → sentence starts on the next line
            if line.is_empty() {
                row = (row + 1).min(ctx.buffer.len().saturating_sub(1));
                col = first_non_blank_col(ctx.buffer, row);
                break;
            }

            let chars: Vec<(usize, char)> = line.char_indices().collect();
            // How many chars are at or before the current byte col?
            let end_idx = chars
                .iter()
                .position(|(b, _)| *b >= col)
                .unwrap_or(chars.len());

            // Scan backward for sentence-ending punctuation before end_idx
            for i in (0..end_idx.saturating_sub(1)).rev() {
                let ch = chars[i].1;
                if matches!(ch, '.' | '!' | '?') {
                    let after = &chars[i + 1..];
                    let sentence_end = after.is_empty()
                        || (after.len() >= 2 && after[0].1 == ' ' && after[1].1 == ' ');
                    if sentence_end {
                        // Start of next sentence is right after trailing spaces
                        let mut j = i + 1;
                        while j < chars.len() && chars[j].1 == ' ' {
                            j += 1;
                        }
                        col = if j < chars.len() { chars[j].0 } else { 0 };
                        break 'outer;
                    }
                }
            }

            // No sentence end found on this line — go to previous line
            if row == 0 {
                col = 0;
                break;
            }
            row -= 1;
            col = ctx.buffer.line(row).map_or(0, |l| l.len());
        }
    }

    let target = Cursor::new(row, col);
    Some(MotionResult {
        target,
        range: MotionRange::new(ctx.cursor, target),
        kind: Motion::SentenceBackward.kind(),
    })
}

/// Section motions: `[[` moves to the previous section start (line beginning with `{` or form-feed),
/// `]]` moves to the next section start.
///
/// Move forward to next section beginning (`]]`).
fn execute_section_forward(ctx: &CommandContext, count: usize) -> Option<MotionResult> {
    let buf_len = ctx.buffer.len();
    if buf_len == 0 {
        return None;
    }

    let mut row = ctx.cursor.row;
    let mut found = 0usize;

    // Start scanning from the line AFTER the cursor
    let start = row + 1;
    for r in start..buf_len {
        if is_section_boundary_with_macros(ctx.buffer, r, ctx.sections) {
            found += 1;
            if found >= count {
                row = r;
                let col = first_non_blank_col(ctx.buffer, row);
                let target = Cursor::new(row, col);
                return Some(MotionResult {
                    target,
                    range: MotionRange::new(ctx.cursor, target),
                    kind: Motion::SectionForward.kind(),
                });
            }
        }
    }

    // No section found — go to last line
    row = buf_len.saturating_sub(1);
    let col = first_non_blank_col(ctx.buffer, row);
    let target = Cursor::new(row, col);
    Some(MotionResult {
        target,
        range: MotionRange::new(ctx.cursor, target),
        kind: Motion::SectionForward.kind(),
    })
}

/// Move backward to previous section beginning (`[[`).
fn execute_section_backward(ctx: &CommandContext, count: usize) -> Option<MotionResult> {
    if ctx.buffer.is_empty() || ctx.cursor.row == 0 {
        let target = Cursor::new(0, 0);
        return Some(MotionResult {
            target,
            range: MotionRange::new(ctx.cursor, target),
            kind: Motion::SectionBackward.kind(),
        });
    }

    let mut found = 0usize;

    // Start scanning from line BEFORE cursor
    let mut r = ctx.cursor.row.saturating_sub(1);
    loop {
        if is_section_boundary_with_macros(ctx.buffer, r, ctx.sections) {
            found += 1;
            if found >= count {
                let col = first_non_blank_col(ctx.buffer, r);
                let target = Cursor::new(r, col);
                return Some(MotionResult {
                    target,
                    range: MotionRange::new(ctx.cursor, target),
                    kind: Motion::SectionBackward.kind(),
                });
            }
        }
        if r == 0 {
            break;
        }
        r -= 1;
    }

    // No section found — go to first line
    let row = 0;
    let col = first_non_blank_col(ctx.buffer, row);
    let target = Cursor::new(row, col);
    Some(MotionResult {
        target,
        range: MotionRange::new(ctx.cursor, target),
        kind: Motion::SectionBackward.kind(),
    })
}

/// Check if `row` is a section boundary.
///
/// A section boundary is a line that starts with `{`, a form-feed (`\x0C`),
/// or a nroff macro whose 2-char name appears in `sections`.
fn is_section_boundary_with_macros(buffer: &Buffer, row: usize, sections: &str) -> bool {
    buffer
        .line(row)
        .map(|l| l.starts_with('{') || l.starts_with('\x0C') || is_macro_boundary(l, sections))
        .unwrap_or(false)
}

/// Return the byte offset of the first non-blank character on `row`, or 0.
fn first_non_blank_col(buffer: &Buffer, row: usize) -> usize {
    buffer
        .line(row)
        .and_then(|l| {
            l.char_indices()
                .find(|(_, c)| !c.is_whitespace())
                .map(|(b, _)| b)
        })
        .unwrap_or(0)
}

// Helper functions

/// Clamp a column to a valid position on the given line.
fn clamp_col_to_line(buffer: &Buffer, row: usize, col: usize) -> usize {
    if let Some(line) = buffer.line(row) {
        if line.is_empty() {
            0
        } else {
            let max_col = prev_grapheme_boundary(line, line.len());
            col.min(max_col)
        }
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ctx(content: &str, row: usize, col: usize) -> (Buffer, CommandContext<'static>) {
        let buffer = Buffer::from_string(content.to_string());
        // SAFETY: We're leaking the buffer for test purposes only.
        // This is fine in tests but would be a memory leak in production.
        let buffer_ref: &'static Buffer = Box::leak(Box::new(buffer.clone()));
        let ctx = CommandContext {
            buffer: buffer_ref,
            cursor: Cursor::new(row, col),
            paragraphs: "",
            sections: "",
            tabstop: 8,
        };
        (buffer, ctx)
    }

    // Character motion tests

    #[test]
    fn test_motion_left_basic() {
        let (_, ctx) = make_ctx("hello", 0, 3);
        let result = execute_left(&ctx, 1).unwrap();
        assert_eq!(result.target.col, 2);
    }

    #[test]
    fn test_motion_left_at_start() {
        let (_, ctx) = make_ctx("hello", 0, 0);
        let result = execute_left(&ctx, 1).unwrap();
        assert_eq!(result.target.col, 0);
    }

    #[test]
    fn test_motion_left_with_count() {
        let (_, ctx) = make_ctx("hello", 0, 4);
        let result = execute_left(&ctx, 3).unwrap();
        assert_eq!(result.target.col, 1);
    }

    #[test]
    fn test_motion_left_unicode() {
        // CJK characters are 3 bytes each
        let (_, ctx) = make_ctx("你好", 0, 3);
        let result = execute_left(&ctx, 1).unwrap();
        assert_eq!(result.target.col, 0);
    }

    #[test]
    fn test_motion_right_basic() {
        let (_, ctx) = make_ctx("hello", 0, 0);
        let result = execute_right(&ctx, 1).unwrap();
        assert_eq!(result.target.col, 1);
    }

    #[test]
    fn test_motion_right_at_end() {
        let (_, ctx) = make_ctx("hello", 0, 4);
        let result = execute_right(&ctx, 1).unwrap();
        // Cannot move past last character in normal mode
        assert_eq!(result.target.col, 4);
    }

    #[test]
    fn test_motion_up_basic() {
        let (_, ctx) = make_ctx("hello\nworld", 1, 2);
        let result = execute_up(&ctx, 1).unwrap();
        assert_eq!(result.target.row, 0);
        assert_eq!(result.target.col, 2);
    }

    #[test]
    fn test_motion_up_at_top() {
        let (_, ctx) = make_ctx("hello\nworld", 0, 2);
        let result = execute_up(&ctx, 1).unwrap();
        assert_eq!(result.target.row, 0);
    }

    #[test]
    fn test_motion_down_basic() {
        let (_, ctx) = make_ctx("hello\nworld", 0, 2);
        let result = execute_down(&ctx, 1).unwrap();
        assert_eq!(result.target.row, 1);
        assert_eq!(result.target.col, 2);
    }

    #[test]
    fn test_motion_down_at_bottom() {
        let (_, ctx) = make_ctx("hello\nworld", 1, 2);
        let result = execute_down(&ctx, 1).unwrap();
        assert_eq!(result.target.row, 1);
    }

    #[test]
    fn test_motion_down_clamps_column() {
        let (_, ctx) = make_ctx("hello\nhi", 0, 4);
        let result = execute_down(&ctx, 1).unwrap();
        assert_eq!(result.target.row, 1);
        // "hi" only has 2 characters, so col 4 clamps to 1
        assert_eq!(result.target.col, 1);
    }

    // Line motion tests

    #[test]
    fn test_line_start() {
        let (_, ctx) = make_ctx("hello", 0, 3);
        let result = execute_line_start(&ctx).unwrap();
        assert_eq!(result.target.col, 0);
    }

    #[test]
    fn test_first_non_blank() {
        let (_, ctx) = make_ctx("   hello", 0, 0);
        let result = execute_first_non_blank(&ctx).unwrap();
        assert_eq!(result.target.col, 3);
    }

    #[test]
    fn test_first_non_blank_no_leading_space() {
        let (_, ctx) = make_ctx("hello", 0, 3);
        let result = execute_first_non_blank(&ctx).unwrap();
        assert_eq!(result.target.col, 0);
    }

    #[test]
    fn test_first_non_blank_all_whitespace() {
        let (_, ctx) = make_ctx("     ", 0, 2);
        let result = execute_first_non_blank(&ctx).unwrap();
        assert_eq!(result.target.col, 0);
    }

    #[test]
    fn test_line_end() {
        let (_, ctx) = make_ctx("hello", 0, 0);
        let result = execute_line_end(&ctx, 1).unwrap();
        assert_eq!(result.target.col, 4);
    }

    #[test]
    fn test_line_end_empty_line() {
        let (_, ctx) = make_ctx("", 0, 0);
        let result = execute_line_end(&ctx, 1).unwrap();
        assert_eq!(result.target.col, 0);
    }

    // Word motion tests

    #[test]
    fn test_word_forward_basic() {
        let (_, ctx) = make_ctx("hello world", 0, 0);
        let result = execute_word_forward(&ctx, 1, false).unwrap();
        assert_eq!(result.target.col, 6); // Start of "world"
    }

    #[test]
    fn test_word_forward_punctuation() {
        let (_, ctx) = make_ctx("hello,world", 0, 0);
        let result = execute_word_forward(&ctx, 1, false).unwrap();
        assert_eq!(result.target.col, 5); // At ","
    }

    #[test]
    fn test_word_forward_with_count() {
        let (_, ctx) = make_ctx("one two three", 0, 0);
        let result = execute_word_forward(&ctx, 2, false).unwrap();
        assert_eq!(result.target.col, 8); // Start of "three"
    }

    #[test]
    fn test_word_backward_basic() {
        let (_, ctx) = make_ctx("hello world", 0, 6);
        let result = execute_word_backward(&ctx, 1, false).unwrap();
        assert_eq!(result.target.col, 0); // Start of "hello"
    }

    #[test]
    fn test_word_end_basic() {
        let (_, ctx) = make_ctx("hello world", 0, 0);
        let result = execute_word_end(&ctx, 1, false).unwrap();
        assert_eq!(result.target.col, 4); // End of "hello"
    }

    #[test]
    fn test_big_word_forward() {
        let (_, ctx) = make_ctx("hello,world test", 0, 0);
        let result = execute_word_forward(&ctx, 1, true).unwrap();
        // WORD motion treats hello,world as one word
        assert_eq!(result.target.col, 12); // Start of "test"
    }

    // Document motion tests

    #[test]
    fn test_document_start() {
        let (_, ctx) = make_ctx("hello\nworld\ntest", 2, 2);
        let result = execute_document_start(&ctx).unwrap();
        assert_eq!(result.target.row, 0);
        assert_eq!(result.target.col, 0);
    }

    #[test]
    fn test_document_end() {
        let (_, ctx) = make_ctx("hello\nworld\ntest", 0, 0);
        let result = execute_document_end(&ctx).unwrap();
        assert_eq!(result.target.row, 2);
        assert_eq!(result.target.col, 0);
    }

    #[test]
    fn test_goto_line() {
        let (_, ctx) = make_ctx("hello\nworld\ntest", 0, 0);
        let result = execute_goto_line(&ctx, 2).unwrap();
        assert_eq!(result.target.row, 1); // Line 2 (1-indexed) = row 1
    }

    #[test]
    fn test_goto_line_past_end() {
        let (_, ctx) = make_ctx("hello\nworld", 0, 0);
        let result = execute_goto_line(&ctx, 100).unwrap();
        assert_eq!(result.target.row, 1); // Clamped to last line
    }

    // Find motion tests

    #[test]
    fn test_find_forward_basic() {
        let (_, ctx) = make_ctx("hello", 0, 0);
        let result = execute_find_char(&ctx, 'l', FindDirection::Forward, FindStop::OnChar, 1);
        let result = result.unwrap();
        assert_eq!(result.target.col, 2); // First 'l'
    }

    #[test]
    fn test_find_forward_not_found() {
        let (_, ctx) = make_ctx("hello", 0, 0);
        let result = execute_find_char(&ctx, 'x', FindDirection::Forward, FindStop::OnChar, 1);
        assert!(result.is_none());
    }

    #[test]
    fn test_find_forward_with_count() {
        let (_, ctx) = make_ctx("hello", 0, 0);
        let result = execute_find_char(&ctx, 'l', FindDirection::Forward, FindStop::OnChar, 2);
        let result = result.unwrap();
        assert_eq!(result.target.col, 3); // Second 'l'
    }

    #[test]
    fn test_find_backward_basic() {
        let (_, ctx) = make_ctx("hello", 0, 4);
        let result = execute_find_char(&ctx, 'l', FindDirection::Backward, FindStop::OnChar, 1);
        let result = result.unwrap();
        assert_eq!(result.target.col, 3); // Last 'l' before cursor
    }

    #[test]
    fn test_till_forward_basic() {
        let (_, ctx) = make_ctx("hello", 0, 0);
        let result = execute_find_char(&ctx, 'l', FindDirection::Forward, FindStop::BeforeChar, 1);
        let result = result.unwrap();
        assert_eq!(result.target.col, 1); // One before first 'l'
    }

    #[test]
    fn test_till_backward_basic() {
        let (_, ctx) = make_ctx("hello", 0, 4);
        let result = execute_find_char(&ctx, 'l', FindDirection::Backward, FindStop::BeforeChar, 1);
        let result = result.unwrap();
        assert_eq!(result.target.col, 4); // One after last 'l' going backward (stays at 4)
    }

    // Motion kind tests

    #[test]
    fn test_motion_kind_exclusive() {
        assert_eq!(Motion::Left.kind(), MotionKind::Exclusive);
        assert_eq!(Motion::Right.kind(), MotionKind::Exclusive);
        assert_eq!(Motion::WordForward.kind(), MotionKind::Exclusive);
        assert_eq!(Motion::WordBackward.kind(), MotionKind::Exclusive);
    }

    #[test]
    fn test_motion_kind_inclusive() {
        assert_eq!(Motion::WordEnd.kind(), MotionKind::Inclusive);
        assert_eq!(Motion::LineEnd.kind(), MotionKind::Inclusive);
        assert_eq!(
            Motion::FindChar {
                ch: 'x',
                direction: FindDirection::Forward,
                stop: FindStop::OnChar
            }
            .kind(),
            MotionKind::Inclusive
        );
        assert_eq!(Motion::MatchingBracket.kind(), MotionKind::Inclusive);
    }

    #[test]
    fn test_motion_kind_linewise() {
        assert_eq!(Motion::Up.kind(), MotionKind::Linewise);
        assert_eq!(Motion::Down.kind(), MotionKind::Linewise);
        assert_eq!(Motion::DocumentStart.kind(), MotionKind::Linewise);
        assert_eq!(Motion::DocumentEnd.kind(), MotionKind::Linewise);
    }

    // MotionRange tests

    #[test]
    fn test_motion_range_new_orders_correctly() {
        let a = Cursor::new(1, 5);
        let b = Cursor::new(0, 3);
        let range = MotionRange::new(a, b);
        assert_eq!(range.start, b);
        assert_eq!(range.end, a);
    }

    // Matching bracket tests

    #[test]
    fn test_matching_bracket_forward_paren() {
        let (_, ctx) = make_ctx("(hello)", 0, 0);
        let result = execute_matching_bracket(&ctx).unwrap();
        assert_eq!(result.target.row, 0);
        assert_eq!(result.target.col, 6);
    }

    #[test]
    fn test_matching_bracket_backward_paren() {
        let (_, ctx) = make_ctx("(hello)", 0, 6);
        let result = execute_matching_bracket(&ctx).unwrap();
        assert_eq!(result.target.row, 0);
        assert_eq!(result.target.col, 0);
    }

    #[test]
    fn test_matching_bracket_forward_brace() {
        let (_, ctx) = make_ctx("{hello}", 0, 0);
        let result = execute_matching_bracket(&ctx).unwrap();
        assert_eq!(result.target.row, 0);
        assert_eq!(result.target.col, 6);
    }

    #[test]
    fn test_matching_bracket_forward_square() {
        let (_, ctx) = make_ctx("[hello]", 0, 0);
        let result = execute_matching_bracket(&ctx).unwrap();
        assert_eq!(result.target.row, 0);
        assert_eq!(result.target.col, 6);
    }

    #[test]
    fn test_matching_bracket_nested() {
        let (_, ctx) = make_ctx("((inner))", 0, 0);
        let result = execute_matching_bracket(&ctx).unwrap();
        // Outer ( matches outer )
        assert_eq!(result.target.row, 0);
        assert_eq!(result.target.col, 8);
    }

    #[test]
    fn test_matching_bracket_multiline() {
        let (_, ctx) = make_ctx("(\nhello\n)", 0, 0);
        let result = execute_matching_bracket(&ctx).unwrap();
        assert_eq!(result.target.row, 2);
        assert_eq!(result.target.col, 0);
    }

    #[test]
    fn test_matching_bracket_scan_forward() {
        // Cursor not on bracket, should scan forward to find '('
        let (_, ctx) = make_ctx("abc(def)", 0, 0);
        let result = execute_matching_bracket(&ctx).unwrap();
        assert_eq!(result.target.row, 0);
        assert_eq!(result.target.col, 7); // Matching ')'
    }

    #[test]
    fn test_matching_bracket_no_bracket() {
        let (_, ctx) = make_ctx("hello", 0, 0);
        let result = execute_matching_bracket(&ctx);
        assert!(result.is_none());
    }

    #[test]
    fn test_matching_bracket_unmatched() {
        let (_, ctx) = make_ctx("(hello", 0, 0);
        let result = execute_matching_bracket(&ctx);
        assert!(result.is_none());
    }

    // Paragraph motion tests

    #[test]
    fn test_paragraph_forward_basic() {
        // Two paragraphs separated by an empty line
        let (_, ctx) = make_ctx("para1\npara1\n\npara2\npara2", 0, 0);
        let result = execute_paragraph_forward(&ctx, 1).unwrap();
        // Should land on the empty line (row 2)
        assert_eq!(result.target.row, 2);
    }

    #[test]
    fn test_paragraph_forward_from_empty_line() {
        // Starting on the empty separator line should skip to next paragraph boundary
        let (_, ctx) = make_ctx("para1\n\npara2\n\npara3", 1, 0);
        let result = execute_paragraph_forward(&ctx, 1).unwrap();
        assert_eq!(result.target.row, 3);
    }

    #[test]
    fn test_paragraph_forward_at_eof() {
        let (_, ctx) = make_ctx("only\none\nparagraph", 0, 0);
        let result = execute_paragraph_forward(&ctx, 1).unwrap();
        // No empty line — lands at last row
        assert_eq!(result.target.row, 2);
    }

    #[test]
    fn test_paragraph_forward_count() {
        let (_, ctx) = make_ctx("p1\n\np2\n\np3", 0, 0);
        let result = execute_paragraph_forward(&ctx, 2).unwrap();
        assert_eq!(result.target.row, 3); // Second empty line
    }

    #[test]
    fn test_paragraph_backward_basic() {
        let (_, ctx) = make_ctx("para1\n\npara2\npara2", 3, 0);
        let result = execute_paragraph_backward(&ctx, 1).unwrap();
        // Should land on the empty line (row 1)
        assert_eq!(result.target.row, 1);
    }

    #[test]
    fn test_paragraph_backward_at_top() {
        let (_, ctx) = make_ctx("hello\nworld", 1, 0);
        let result = execute_paragraph_backward(&ctx, 1).unwrap();
        assert_eq!(result.target.row, 0);
    }

    // Sentence motion tests

    #[test]
    fn test_sentence_forward_basic() {
        let (_, ctx) = make_ctx("Hello.  World.", 0, 0);
        let result = execute_sentence_forward(&ctx, 1).unwrap();
        // Should advance to "World" (col 8)
        assert_eq!(result.target.col, 8);
    }

    #[test]
    fn test_sentence_forward_end_of_line() {
        // Sentence ends at end-of-line, next sentence starts on next line
        let (_, ctx) = make_ctx("Hello.\nWorld.", 0, 0);
        let result = execute_sentence_forward(&ctx, 1).unwrap();
        assert_eq!(result.target.row, 1);
        assert_eq!(result.target.col, 0);
    }

    #[test]
    fn test_sentence_backward_basic() {
        let (_, ctx) = make_ctx("Hello.  World.", 0, 8);
        let result = execute_sentence_backward(&ctx, 1).unwrap();
        assert_eq!(result.target.col, 0);
    }

    #[test]
    fn test_sentence_backward_at_start() {
        let (_, ctx) = make_ctx("Hello world.", 0, 0);
        let result = execute_sentence_backward(&ctx, 1).unwrap();
        assert_eq!(result.target.row, 0);
        assert_eq!(result.target.col, 0);
    }

    #[test]
    fn test_sentence_forward_single_space_no_break() {
        // Single space after punctuation must NOT be treated as a sentence boundary
        let (_, ctx) = make_ctx("e.g. something else", 0, 0);
        let result = execute_sentence_forward(&ctx, 1).unwrap();
        // No sentence boundary found on this line, so cursor should not advance
        // past a false boundary — it reaches end of buffer
        assert_eq!(result.target.row, 0);
        // Should stay at or near col 0 (no valid sentence end found)
        assert_eq!(result.target.col, 0);
    }

    #[test]
    fn test_sentence_forward_exclamation() {
        // "!" is also a sentence terminator
        let (_, ctx) = make_ctx("Wow!  Next sentence.", 0, 0);
        let result = execute_sentence_forward(&ctx, 1).unwrap();
        // "Next" starts at col 6
        assert_eq!(result.target.col, 6);
    }

    #[test]
    fn test_sentence_forward_question_mark() {
        // "?" is also a sentence terminator
        let (_, ctx) = make_ctx("Really?  Yes.", 0, 0);
        let result = execute_sentence_forward(&ctx, 1).unwrap();
        // "Yes" starts at col 9
        assert_eq!(result.target.col, 9);
    }

    #[test]
    fn test_sentence_forward_count() {
        // count=2 should skip two sentence boundaries
        let (_, ctx) = make_ctx("One.  Two.  Three.", 0, 0);
        let result = execute_sentence_forward(&ctx, 2).unwrap();
        // "Two" at col 6, "Three" at col 12
        assert_eq!(result.target.col, 12);
    }

    #[test]
    fn test_sentence_backward_across_lines() {
        // Cursor on line 1; previous sentence ended on line 0
        let (_, ctx) = make_ctx("Hello.\nWorld.", 1, 0);
        let result = execute_sentence_backward(&ctx, 1).unwrap();
        assert_eq!(result.target.row, 0);
        assert_eq!(result.target.col, 0);
    }

    #[test]
    fn test_sentence_backward_exclamation() {
        // "!" as sentence terminator going backward
        let (_, ctx) = make_ctx("Wow!  Next.", 0, 6);
        let result = execute_sentence_backward(&ctx, 1).unwrap();
        assert_eq!(result.target.col, 0);
    }

    // NextLineFirstNonBlank / PrevLineFirstNonBlank / CurrentLineFirstNonBlank tests

    #[test]
    fn test_next_line_first_non_blank_basic() {
        // Enter/+ from row 0 should land on first non-blank of row 1
        let (_, ctx) = make_ctx("hello\n  world", 0, 0);
        let result = execute_next_line_first_non_blank(&ctx, 1).unwrap();
        assert_eq!(result.target.row, 1);
        assert_eq!(result.target.col, 2); // 'w' at byte 2
        assert_eq!(result.kind, MotionKind::Linewise);
    }

    #[test]
    fn test_next_line_first_non_blank_at_last_line() {
        // On last line, motion stays on last line
        let (_, ctx) = make_ctx("hello\nworld", 1, 0);
        let result = execute_next_line_first_non_blank(&ctx, 1).unwrap();
        assert_eq!(result.target.row, 1);
    }

    #[test]
    fn test_next_line_first_non_blank_with_count() {
        // 2+ from row 0 should land on row 2
        let (_, ctx) = make_ctx("a\n  b\nc", 0, 0);
        let result = execute_next_line_first_non_blank(&ctx, 2).unwrap();
        assert_eq!(result.target.row, 2);
        assert_eq!(result.target.col, 0);
    }

    #[test]
    fn test_prev_line_first_non_blank_basic() {
        // - from row 2 should land on first non-blank of row 1
        let (_, ctx) = make_ctx("a\n  b\nc", 2, 0);
        let result = execute_prev_line_first_non_blank(&ctx, 1).unwrap();
        assert_eq!(result.target.row, 1);
        assert_eq!(result.target.col, 2);
        assert_eq!(result.kind, MotionKind::Linewise);
    }

    #[test]
    fn test_prev_line_first_non_blank_at_first_line() {
        // On first line, stays on first line
        let (_, ctx) = make_ctx("  hello", 0, 2);
        let result = execute_prev_line_first_non_blank(&ctx, 1).unwrap();
        assert_eq!(result.target.row, 0);
        assert_eq!(result.target.col, 2);
    }

    #[test]
    fn test_current_line_first_non_blank_no_count() {
        // _ with count=1 stays on current line first non-blank
        let (_, ctx) = make_ctx("  hello", 0, 4);
        let result = execute_current_line_first_non_blank(&ctx, 1).unwrap();
        assert_eq!(result.target.row, 0);
        assert_eq!(result.target.col, 2);
        assert_eq!(result.kind, MotionKind::Linewise);
    }

    #[test]
    fn test_current_line_first_non_blank_with_count() {
        // 2_ should go to first non-blank of 1 line below (count - 1 = 1)
        let (_, ctx) = make_ctx("a\n  b\nc", 0, 0);
        let result = execute_current_line_first_non_blank(&ctx, 2).unwrap();
        assert_eq!(result.target.row, 1);
        assert_eq!(result.target.col, 2);
    }

    // =========================================================================
    // Column motion tests
    // =========================================================================

    #[test]
    fn test_column_motion_default_count() {
        // count=1 should move to column 1 (byte offset 0)
        let (_, ctx) = make_ctx("hello world", 0, 6);
        let result = execute_column(&ctx, 1).unwrap();
        assert_eq!(result.target.col, 0);
    }

    #[test]
    fn test_column_motion_to_col_5() {
        // count=5 should move to screen column 5 (byte offset 4)
        let (_, ctx) = make_ctx("hello world", 0, 0);
        let result = execute_column(&ctx, 5).unwrap();
        assert_eq!(result.target.col, 4);
    }

    #[test]
    fn test_column_motion_beyond_line_end() {
        // count beyond line length should clamp to last char
        let (_, ctx) = make_ctx("hi", 0, 0);
        let result = execute_column(&ctx, 100).unwrap();
        assert_eq!(result.target.col, 1); // 'i' at byte 1
    }

    #[test]
    fn test_column_motion_empty_line() {
        let (_, ctx) = make_ctx("", 0, 0);
        let result = execute_column(&ctx, 5).unwrap();
        assert_eq!(result.target.col, 0);
    }

    #[test]
    fn test_column_motion_kind_is_exclusive() {
        assert_eq!(Motion::Column.kind(), MotionKind::Exclusive);
    }
}

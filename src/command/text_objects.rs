//! Text object types and resolution.
//!
//! Text objects provide a way to select regions of text semantically (words, quotes,
//! brackets) rather than by cursor motions. They work exclusively with operators
//! (d, c, y) via patterns like 'daw' (delete a word) or 'ci"' (change inside quotes).

use crate::buffer::unicode::{classify_char, next_grapheme_boundary, CharClass};
use crate::buffer::{Buffer, Cursor};

use super::motion::MotionRange;

/// Distinguishes 'a' (around) vs 'i' (inner) text objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextObjectKind {
    /// 'a' - includes delimiters/whitespace
    Around,
    /// 'i' - excludes delimiters
    Inner,
}

/// All text object types for vi compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextObject {
    // Word objects
    /// aw, iw - word (alphanumeric + underscore)
    Word,
    /// aW, iW - WORD (whitespace-delimited)
    WORD,

    // Quote objects
    /// a", i"
    DoubleQuote,
    /// a', i'
    SingleQuote,
    /// a`, i`
    Backtick,

    // Bracket/block objects
    /// a(, ab, i(, ib
    Parenthesis,
    /// a[, i[
    Bracket,
    /// a{, aB, i{, iB
    Brace,
    /// a<, i<
    AngleBracket,
}

/// Immutable context for resolving text objects.
pub struct TextObjectContext<'a> {
    /// Reference to the text buffer
    pub buffer: &'a Buffer,
    /// Current cursor position
    pub cursor: Cursor,
}

/// Resolve a text object to get the affected range.
///
/// # Arguments
///
/// * `ctx` - Text object context with buffer and cursor
/// * `kind` - Around or Inner
/// * `object` - The type of text object
/// * `count` - Repeat count (for words, selects count words)
///
/// # Returns
///
/// `Some(MotionRange)` if the text object was found, `None` if not found or not applicable.
pub fn resolve_text_object(
    ctx: &TextObjectContext,
    kind: TextObjectKind,
    object: TextObject,
    count: usize,
) -> Option<MotionRange> {
    let count = count.max(1);

    match object {
        TextObject::Word => resolve_word_object(ctx, kind, false, count),
        TextObject::WORD => resolve_word_object(ctx, kind, true, count),
        TextObject::DoubleQuote => resolve_quote_object(ctx, kind, '"'),
        TextObject::SingleQuote => resolve_quote_object(ctx, kind, '\''),
        TextObject::Backtick => resolve_quote_object(ctx, kind, '`'),
        TextObject::Parenthesis => resolve_bracket_object(ctx, kind, '(', ')'),
        TextObject::Bracket => resolve_bracket_object(ctx, kind, '[', ']'),
        TextObject::Brace => resolve_bracket_object(ctx, kind, '{', '}'),
        TextObject::AngleBracket => resolve_bracket_object(ctx, kind, '<', '>'),
    }
}

/// Resolve a word object (aw, iw, aW, iW).
fn resolve_word_object(
    ctx: &TextObjectContext,
    kind: TextObjectKind,
    big_word: bool,
    count: usize,
) -> Option<MotionRange> {
    let line = ctx.buffer.line(ctx.cursor.row)?;
    let chars: Vec<char> = line.chars().collect();

    if chars.is_empty() {
        return None;
    }

    // Convert byte offset to char index
    let char_idx = line[..ctx.cursor.col.min(line.len())].chars().count();
    if char_idx >= chars.len() {
        return None;
    }

    let current_class = classify_char(chars[char_idx], big_word);

    // Find word start (scan backwards)
    let mut start_idx = char_idx;
    while start_idx > 0 && classify_char(chars[start_idx - 1], big_word) == current_class {
        start_idx -= 1;
    }

    // Find word end (scan forwards) - including count words
    let mut end_idx = char_idx;
    let mut words_found = 1;

    // First, finish the current word
    while end_idx + 1 < chars.len() && classify_char(chars[end_idx + 1], big_word) == current_class
    {
        end_idx += 1;
    }

    // Then find additional words if count > 1
    while words_found < count && end_idx + 1 < chars.len() {
        // Skip whitespace
        while end_idx + 1 < chars.len() && chars[end_idx + 1].is_whitespace() {
            end_idx += 1;
        }

        if end_idx + 1 >= chars.len() {
            break;
        }

        // Find the next word
        let next_class = classify_char(chars[end_idx + 1], big_word);
        end_idx += 1;
        while end_idx + 1 < chars.len() && classify_char(chars[end_idx + 1], big_word) == next_class
        {
            end_idx += 1;
        }

        words_found += 1;
    }

    // For 'around', include trailing whitespace (or leading if no trailing)
    if kind == TextObjectKind::Around && current_class != CharClass::Whitespace {
        // Try trailing whitespace first
        let mut trailing_end = end_idx;
        while trailing_end + 1 < chars.len() && chars[trailing_end + 1].is_whitespace() {
            trailing_end += 1;
        }

        if trailing_end > end_idx {
            end_idx = trailing_end;
        } else {
            // No trailing, try leading
            while start_idx > 0 && chars[start_idx - 1].is_whitespace() {
                start_idx -= 1;
            }
        }
    } else if kind == TextObjectKind::Around && current_class == CharClass::Whitespace {
        // Cursor is on whitespace: extend end_idx forward through the following word
        if end_idx + 1 < chars.len() {
            let next_class = classify_char(chars[end_idx + 1], big_word);
            if next_class != CharClass::Whitespace {
                end_idx += 1;
                while end_idx + 1 < chars.len()
                    && classify_char(chars[end_idx + 1], big_word) == next_class
                {
                    end_idx += 1;
                }
            }
        }
    }

    // Convert char indices back to byte offsets
    let start_byte: usize = chars[..start_idx].iter().map(|c| c.len_utf8()).sum();
    let end_byte: usize = chars[..=end_idx].iter().map(|c| c.len_utf8()).sum();

    Some(MotionRange::new(
        Cursor::new(ctx.cursor.row, start_byte),
        Cursor::new(ctx.cursor.row, end_byte),
    ))
}

/// Find unescaped quotes in a line.
///
/// Tracks consecutive backslashes to correctly handle double-escaped backslashes.
/// An odd number of backslashes before a quote means the quote is escaped;
/// an even number (including zero) means it is not.
fn find_unescaped_quotes(line: &str, quote_char: char) -> Vec<usize> {
    let mut positions = Vec::new();
    let mut backslash_count: usize = 0;

    for (idx, ch) in line.char_indices() {
        if ch == quote_char && backslash_count.is_multiple_of(2) {
            positions.push(idx);
        }
        if ch == '\\' {
            backslash_count += 1;
        } else {
            backslash_count = 0;
        }
    }

    positions
}

/// Resolve a quote object (a", i", a', i', a`, i`).
fn resolve_quote_object(
    ctx: &TextObjectContext,
    kind: TextObjectKind,
    quote_char: char,
) -> Option<MotionRange> {
    let line = ctx.buffer.line(ctx.cursor.row)?;
    let col = ctx.cursor.col;

    // Find quote positions on this line
    let quote_positions = find_unescaped_quotes(line, quote_char);

    if quote_positions.len() < 2 {
        return None; // Need at least opening and closing
    }

    // Find the pair that contains or follows the cursor
    let mut pair: Option<(usize, usize)> = None;

    // Group positions into pairs (0-1, 2-3, 4-5, ...)
    let mut i = 0;
    while i + 1 < quote_positions.len() {
        let open = quote_positions[i];
        let close = quote_positions[i + 1];

        // Check if cursor is inside or on this pair
        if col <= close {
            if col >= open {
                // Cursor is inside or on this pair
                pair = Some((open, close));
                break;
            } else if pair.is_none() {
                // Cursor is before this pair - use it (search forward behavior)
                pair = Some((open, close));
                break;
            }
        }

        i += 2;
    }

    let (open, close) = pair?;

    let (start, end) = match kind {
        TextObjectKind::Around => (open, close + quote_char.len_utf8()),
        TextObjectKind::Inner => {
            let inner_start = open + quote_char.len_utf8();
            let inner_end = close;
            if inner_start >= inner_end {
                // Empty inner (adjacent quotes) - return empty range at opening quote position
                return Some(MotionRange::new(
                    Cursor::new(ctx.cursor.row, inner_start),
                    Cursor::new(ctx.cursor.row, inner_start),
                ));
            }
            (inner_start, inner_end)
        }
    };

    Some(MotionRange::new(
        Cursor::new(ctx.cursor.row, start),
        Cursor::new(ctx.cursor.row, end),
    ))
}

/// Find the opening bracket searching backwards.
fn find_opening_bracket(
    buffer: &Buffer,
    start_row: usize,
    start_col: usize,
    open: char,
    close: char,
) -> Option<(usize, usize)> {
    let mut depth = 0;
    let mut current_row = start_row;
    let mut search_col = start_col;

    loop {
        let line = buffer.line(current_row)?;
        let chars: Vec<(usize, char)> = line[..search_col.min(line.len())].char_indices().collect();

        for (idx, ch) in chars.into_iter().rev() {
            if ch == close {
                depth += 1;
            } else if ch == open {
                if depth == 0 {
                    return Some((current_row, idx));
                }
                depth -= 1;
            }
        }

        // Move to previous line
        if current_row == 0 {
            return None; // Not found
        }
        current_row -= 1;
        search_col = buffer.line(current_row).map(|l| l.len()).unwrap_or(0);
    }
}

/// Find the closing bracket searching forwards.
fn find_closing_bracket(
    buffer: &Buffer,
    start_row: usize,
    start_col: usize,
    open: char,
    close: char,
) -> Option<(usize, usize)> {
    let mut depth = 0;
    let current_row = start_row;

    let line = buffer.line(current_row)?;
    let search_start = start_col + open.len_utf8(); // Skip the opening bracket

    // First search the starting line
    for (idx, ch) in line[search_start.min(line.len())..].char_indices() {
        let actual_idx = search_start + idx;
        if ch == open {
            depth += 1;
        } else if ch == close {
            if depth == 0 {
                return Some((current_row, actual_idx));
            }
            depth -= 1;
        }
    }

    // Search subsequent lines
    for row_idx in (current_row + 1)..buffer.len() {
        let line = buffer.line(row_idx)?;
        for (idx, ch) in line.char_indices() {
            if ch == open {
                depth += 1;
            } else if ch == close {
                if depth == 0 {
                    return Some((row_idx, idx));
                }
                depth -= 1;
            }
        }
    }

    None // Not found
}

/// Resolve a bracket object (a(, a{, a[, a<, i(, i{, i[, i<).
fn resolve_bracket_object(
    ctx: &TextObjectContext,
    kind: TextObjectKind,
    open: char,
    close: char,
) -> Option<MotionRange> {
    let start_row = ctx.cursor.row;
    let start_col = ctx.cursor.col;

    // Check if cursor is on a bracket
    let line = ctx.buffer.line(start_row)?;
    let current_char = line[start_col..].chars().next();

    let (open_pos, close_pos) = if current_char == Some(open) {
        // Cursor is on opening bracket - search forward for matching close
        let close_pos = find_closing_bracket(ctx.buffer, start_row, start_col, open, close)?;
        ((start_row, start_col), close_pos)
    } else if current_char == Some(close) {
        // Cursor is on closing bracket - search backward for matching open
        let open_pos = find_opening_bracket(ctx.buffer, start_row, start_col, open, close)?;
        (open_pos, (start_row, start_col))
    } else {
        // Cursor is not on a bracket - search backward for opening, then forward for closing
        // Use next_grapheme_boundary to advance past the current grapheme safely
        let next_col = if let Some(line) = ctx.buffer.line(start_row) {
            next_grapheme_boundary(line, start_col)
        } else {
            start_col
        };
        let open_pos = find_opening_bracket(ctx.buffer, start_row, next_col, open, close)?;
        let close_pos = find_closing_bracket(ctx.buffer, open_pos.0, open_pos.1, open, close)?;
        (open_pos, close_pos)
    };

    let (start, end) = match kind {
        TextObjectKind::Around => (
            Cursor::new(open_pos.0, open_pos.1),
            Cursor::new(close_pos.0, close_pos.1 + close.len_utf8()),
        ),
        TextObjectKind::Inner => {
            // Inner starts after opening bracket
            let inner_start_col = open_pos.1 + open.len_utf8();

            // Check if inner is empty
            if open_pos.0 == close_pos.0 && inner_start_col >= close_pos.1 {
                // Empty inner on same line
                return Some(MotionRange::new(
                    Cursor::new(open_pos.0, inner_start_col),
                    Cursor::new(close_pos.0, close_pos.1),
                ));
            }

            (
                Cursor::new(open_pos.0, inner_start_col),
                Cursor::new(close_pos.0, close_pos.1),
            )
        }
    };

    Some(MotionRange::new(start, end))
}

/// Parse a text object specifier character.
pub fn parse_text_object_specifier(ch: char) -> Option<TextObject> {
    match ch {
        'w' => Some(TextObject::Word),
        'W' => Some(TextObject::WORD),
        '"' => Some(TextObject::DoubleQuote),
        '\'' => Some(TextObject::SingleQuote),
        '`' => Some(TextObject::Backtick),
        '(' | ')' | 'b' => Some(TextObject::Parenthesis),
        '[' | ']' => Some(TextObject::Bracket),
        '{' | '}' | 'B' => Some(TextObject::Brace),
        '<' | '>' => Some(TextObject::AngleBracket),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_ctx<F, R>(content: &str, row: usize, col: usize, f: F) -> R
    where
        F: FnOnce(&Buffer, &TextObjectContext<'_>) -> R,
    {
        let buffer = Buffer::from_string(content.to_string());
        let ctx = TextObjectContext {
            buffer: &buffer,
            cursor: Cursor::new(row, col),
        };
        f(&buffer, &ctx)
    }

    // Word object tests

    #[test]
    fn test_word_object_basic_inner() {
        with_ctx("hello world", 0, 1, |_, ctx| {
            // Cursor on 'e'
            let result = resolve_word_object(ctx, TextObjectKind::Inner, false, 1).unwrap();
            assert_eq!(result.start.col, 0);
            assert_eq!(result.end.col, 5); // "hello" (exclusive end)
        });
    }

    #[test]
    fn test_word_object_basic_around() {
        with_ctx("hello world", 0, 1, |_, ctx| {
            // Cursor on 'e'
            let result = resolve_word_object(ctx, TextObjectKind::Around, false, 1).unwrap();
            assert_eq!(result.start.col, 0);
            assert_eq!(result.end.col, 6); // "hello " including trailing space
        });
    }

    #[test]
    fn test_word_object_at_word_start() {
        with_ctx("hello world", 0, 0, |_, ctx| {
            // Cursor on 'h'
            let result = resolve_word_object(ctx, TextObjectKind::Inner, false, 1).unwrap();
            assert_eq!(result.start.col, 0);
            assert_eq!(result.end.col, 5);
        });
    }

    #[test]
    fn test_word_object_at_word_end() {
        with_ctx("hello world", 0, 4, |_, ctx| {
            // Cursor on 'o'
            let result = resolve_word_object(ctx, TextObjectKind::Inner, false, 1).unwrap();
            assert_eq!(result.start.col, 0);
            assert_eq!(result.end.col, 5);
        });
    }

    #[test]
    fn test_word_object_with_punctuation() {
        with_ctx("hello,world", 0, 0, |_, ctx| {
            // Cursor on 'h'
            let result = resolve_word_object(ctx, TextObjectKind::Inner, false, 1).unwrap();
            // 'w' motion treats comma as a separate word
            assert_eq!(result.start.col, 0);
            assert_eq!(result.end.col, 5); // "hello"
        });
    }

    #[test]
    fn test_big_word_object() {
        with_ctx("hello,world test", 0, 0, |_, ctx| {
            let result = resolve_word_object(ctx, TextObjectKind::Inner, true, 1).unwrap();
            // WORD treats hello,world as one word
            assert_eq!(result.start.col, 0);
            assert_eq!(result.end.col, 11); // "hello,world"
        });
    }

    #[test]
    fn test_word_object_around_no_trailing_space() {
        with_ctx("hello world", 0, 7, |_, ctx| {
            // Cursor on 'o' in "world"
            let result = resolve_word_object(ctx, TextObjectKind::Around, false, 1).unwrap();
            // No trailing space, should include leading space
            assert_eq!(result.start.col, 5); // Include the space before "world"
            assert_eq!(result.end.col, 11);
        });
    }

    #[test]
    fn test_word_object_unicode() {
        with_ctx("你好 世界", 0, 0, |_, ctx| {
            // Cursor on first CJK char
            let result = resolve_word_object(ctx, TextObjectKind::Inner, false, 1).unwrap();
            // Each CJK char is 3 bytes
            assert_eq!(result.start.col, 0);
            assert_eq!(result.end.col, 6); // "你好"
        });
    }

    #[test]
    fn test_word_object_around_cursor_on_whitespace() {
        with_ctx("hello world", 0, 5, |_, ctx| {
            // Cursor on the space between "hello" and "world"
            let result = resolve_word_object(ctx, TextObjectKind::Around, false, 1).unwrap();
            // Should select the space + "world" (whitespace + following word)
            assert_eq!(result.start.col, 5);
            assert_eq!(result.end.col, 11);
        });
    }

    #[test]
    fn test_word_object_inner_cursor_on_whitespace() {
        with_ctx("hello world", 0, 5, |_, ctx| {
            // Cursor on the space: iw selects only the whitespace
            let result = resolve_word_object(ctx, TextObjectKind::Inner, false, 1).unwrap();
            assert_eq!(result.start.col, 5);
            assert_eq!(result.end.col, 6);
        });
    }

    #[test]
    fn test_word_object_with_count() {
        with_ctx("one two three", 0, 0, |_, ctx| {
            let result = resolve_word_object(ctx, TextObjectKind::Inner, false, 2).unwrap();
            // Should select "one two"
            assert_eq!(result.start.col, 0);
            assert_eq!(result.end.col, 7);
        });
    }

    // Quote object tests

    #[test]
    fn test_quote_object_basic_inner() {
        with_ctx(r#"say "hello" now"#, 0, 6, |_, ctx| {
            // Cursor on 'e'
            let result = resolve_quote_object(ctx, TextObjectKind::Inner, '"').unwrap();
            assert_eq!(result.start.col, 5); // After opening quote
            assert_eq!(result.end.col, 10); // Before closing quote
        });
    }

    #[test]
    fn test_quote_object_basic_around() {
        with_ctx(r#"say "hello" now"#, 0, 6, |_, ctx| {
            // Cursor on 'e'
            let result = resolve_quote_object(ctx, TextObjectKind::Around, '"').unwrap();
            assert_eq!(result.start.col, 4); // Opening quote
            assert_eq!(result.end.col, 11); // After closing quote
        });
    }

    #[test]
    fn test_quote_object_cursor_on_quote() {
        with_ctx(r#"say "hello" now"#, 0, 4, |_, ctx| {
            // Cursor on opening quote
            let result = resolve_quote_object(ctx, TextObjectKind::Inner, '"').unwrap();
            assert_eq!(result.start.col, 5);
            assert_eq!(result.end.col, 10);
        });
    }

    #[test]
    fn test_quote_object_cursor_before() {
        with_ctx(r#"say "hello" now"#, 0, 0, |_, ctx| {
            // Cursor on 's' before quotes
            let result = resolve_quote_object(ctx, TextObjectKind::Inner, '"').unwrap();
            // Should find the first quote pair (search forward)
            assert_eq!(result.start.col, 5);
            assert_eq!(result.end.col, 10);
        });
    }

    #[test]
    fn test_quote_object_empty_quotes() {
        with_ctx(r#"say "" now"#, 0, 5, |_, ctx| {
            // Cursor on or between empty quotes
            let result = resolve_quote_object(ctx, TextObjectKind::Inner, '"').unwrap();
            // Empty inner should return empty range
            assert_eq!(result.start.col, 5);
            assert_eq!(result.end.col, 5);
        });
    }

    #[test]
    fn test_quote_object_escaped_quotes() {
        with_ctx(r#"say "hello \"world\"" now"#, 0, 6, |_, ctx| {
            let result = resolve_quote_object(ctx, TextObjectKind::Inner, '"').unwrap();
            // Should get content between first unescaped pair
            assert_eq!(result.start.col, 5);
            assert_eq!(result.end.col, 20); // hello \"world\"
        });
    }

    #[test]
    fn test_quote_object_single_quotes() {
        with_ctx("say 'hello' now", 0, 6, |_, ctx| {
            let result = resolve_quote_object(ctx, TextObjectKind::Inner, '\'').unwrap();
            assert_eq!(result.start.col, 5);
            assert_eq!(result.end.col, 10);
        });
    }

    #[test]
    fn test_quote_object_backticks() {
        with_ctx("say `hello` now", 0, 6, |_, ctx| {
            let result = resolve_quote_object(ctx, TextObjectKind::Inner, '`').unwrap();
            assert_eq!(result.start.col, 5);
            assert_eq!(result.end.col, 10);
        });
    }

    #[test]
    fn test_quote_object_no_quotes() {
        with_ctx("no quotes here", 0, 0, |_, ctx| {
            let result = resolve_quote_object(ctx, TextObjectKind::Inner, '"');
            assert!(result.is_none());
        });
    }

    #[test]
    fn test_quote_object_single_quote_only() {
        with_ctx(r#"just one " here"#, 0, 10, |_, ctx| {
            let result = resolve_quote_object(ctx, TextObjectKind::Inner, '"');
            assert!(result.is_none()); // Need at least 2 quotes
        });
    }

    #[test]
    fn test_quote_object_double_backslash() {
        // In the string: say \\"hello" now
        // The \\ is an escaped backslash, so the quote after it is NOT escaped.
        // Byte layout: s(0) a(1) y(2) ' '(3) \(4) \(5) "(6) h(7) e(8) l(9) l(10) o(11) "(12) ...
        with_ctx(r#"say \\"hello" now"#, 0, 8, |_, ctx| {
            // Cursor on 'e' inside quotes
            let result = resolve_quote_object(ctx, TextObjectKind::Inner, '"').unwrap();
            // The quote at position 6 is unescaped (preceded by even number of backslashes),
            // and the quote at position 12 is the closing unescaped quote.
            assert_eq!(result.start.col, 7); // After opening quote at col 6
            assert_eq!(result.end.col, 12); // Before closing quote at col 12
        });
    }

    // Bracket object tests

    #[test]
    fn test_bracket_object_basic_inner() {
        with_ctx("fn(abc)", 0, 4, |_, ctx| {
            // Cursor on 'b'
            let result = resolve_bracket_object(ctx, TextObjectKind::Inner, '(', ')').unwrap();
            assert_eq!(result.start.col, 3); // After '('
            assert_eq!(result.end.col, 6); // Before ')'
        });
    }

    #[test]
    fn test_bracket_object_basic_around() {
        with_ctx("fn(abc)", 0, 4, |_, ctx| {
            // Cursor on 'b'
            let result = resolve_bracket_object(ctx, TextObjectKind::Around, '(', ')').unwrap();
            assert_eq!(result.start.col, 2); // Opening '('
            assert_eq!(result.end.col, 7); // After closing ')'
        });
    }

    #[test]
    fn test_bracket_object_cursor_on_open() {
        with_ctx("fn(abc)", 0, 2, |_, ctx| {
            // Cursor on '('
            let result = resolve_bracket_object(ctx, TextObjectKind::Inner, '(', ')').unwrap();
            assert_eq!(result.start.col, 3);
            assert_eq!(result.end.col, 6);
        });
    }

    #[test]
    fn test_bracket_object_cursor_on_close() {
        with_ctx("fn(abc)", 0, 6, |_, ctx| {
            // Cursor on ')'
            let result = resolve_bracket_object(ctx, TextObjectKind::Inner, '(', ')').unwrap();
            assert_eq!(result.start.col, 3);
            assert_eq!(result.end.col, 6);
        });
    }

    #[test]
    fn test_bracket_object_nested() {
        with_ctx("fn(a(b)c)", 0, 5, |_, ctx| {
            // Cursor on 'b'
            let result = resolve_bracket_object(ctx, TextObjectKind::Inner, '(', ')').unwrap();
            // Should select inner "(b)" content
            assert_eq!(result.start.col, 5);
            assert_eq!(result.end.col, 6);
        });
    }

    #[test]
    fn test_bracket_object_multiline() {
        with_ctx("{\n  hello\n}", 1, 2, |_, ctx| {
            // Cursor on 'h' in middle line
            let result = resolve_bracket_object(ctx, TextObjectKind::Inner, '{', '}').unwrap();
            assert_eq!(result.start.row, 0);
            assert_eq!(result.start.col, 1); // After '{'
            assert_eq!(result.end.row, 2);
            assert_eq!(result.end.col, 0); // Before '}'
        });
    }

    #[test]
    fn test_bracket_object_empty() {
        with_ctx("fn()", 0, 2, |_, ctx| {
            // Cursor on '('
            let result = resolve_bracket_object(ctx, TextObjectKind::Inner, '(', ')').unwrap();
            assert_eq!(result.start.col, 3);
            assert_eq!(result.end.col, 3);
        });
    }

    #[test]
    fn test_bracket_object_square() {
        with_ctx("arr[123]", 0, 5, |_, ctx| {
            // Cursor on '2'
            let result = resolve_bracket_object(ctx, TextObjectKind::Inner, '[', ']').unwrap();
            assert_eq!(result.start.col, 4);
            assert_eq!(result.end.col, 7);
        });
    }

    #[test]
    fn test_bracket_object_braces() {
        with_ctx("{ abc }", 0, 3, |_, ctx| {
            // Cursor on 'a'
            let result = resolve_bracket_object(ctx, TextObjectKind::Inner, '{', '}').unwrap();
            assert_eq!(result.start.col, 1);
            assert_eq!(result.end.col, 6);
        });
    }

    #[test]
    fn test_bracket_object_angle() {
        // Test with cursor inside angle brackets: <abc>
        // Position 0: <, Position 1-3: abc, Position 4: >
        with_ctx("<abc>", 0, 2, |_, ctx| {
            // Cursor on 'b'
            let result = resolve_bracket_object(ctx, TextObjectKind::Inner, '<', '>').unwrap();
            assert_eq!(result.start.col, 1); // After '<'
            assert_eq!(result.end.col, 4); // Before '>'
        });
    }

    #[test]
    fn test_bracket_object_angle_around() {
        with_ctx("<abc>", 0, 2, |_, ctx| {
            // Cursor on 'b'
            let result = resolve_bracket_object(ctx, TextObjectKind::Around, '<', '>').unwrap();
            assert_eq!(result.start.col, 0); // Opening '<'
            assert_eq!(result.end.col, 5); // After closing '>'
        });
    }

    #[test]
    fn test_bracket_object_angle_cursor_on_open() {
        with_ctx("<abc>", 0, 0, |_, ctx| {
            // Cursor on '<'
            let result = resolve_bracket_object(ctx, TextObjectKind::Inner, '<', '>').unwrap();
            assert_eq!(result.start.col, 1);
            assert_eq!(result.end.col, 4);
        });
    }

    #[test]
    fn test_bracket_object_not_found() {
        with_ctx("no brackets", 0, 0, |_, ctx| {
            let result = resolve_bracket_object(ctx, TextObjectKind::Inner, '(', ')');
            assert!(result.is_none());
        });
    }

    // Text object specifier parsing tests

    #[test]
    fn test_parse_text_object_specifier_word() {
        assert_eq!(parse_text_object_specifier('w'), Some(TextObject::Word));
        assert_eq!(parse_text_object_specifier('W'), Some(TextObject::WORD));
    }

    #[test]
    fn test_parse_text_object_specifier_quotes() {
        assert_eq!(
            parse_text_object_specifier('"'),
            Some(TextObject::DoubleQuote)
        );
        assert_eq!(
            parse_text_object_specifier('\''),
            Some(TextObject::SingleQuote)
        );
        assert_eq!(parse_text_object_specifier('`'), Some(TextObject::Backtick));
    }

    #[test]
    fn test_parse_text_object_specifier_brackets() {
        assert_eq!(
            parse_text_object_specifier('('),
            Some(TextObject::Parenthesis)
        );
        assert_eq!(
            parse_text_object_specifier(')'),
            Some(TextObject::Parenthesis)
        );
        assert_eq!(
            parse_text_object_specifier('b'),
            Some(TextObject::Parenthesis)
        );
        assert_eq!(parse_text_object_specifier('['), Some(TextObject::Bracket));
        assert_eq!(parse_text_object_specifier(']'), Some(TextObject::Bracket));
        assert_eq!(parse_text_object_specifier('{'), Some(TextObject::Brace));
        assert_eq!(parse_text_object_specifier('}'), Some(TextObject::Brace));
        assert_eq!(parse_text_object_specifier('B'), Some(TextObject::Brace));
        assert_eq!(
            parse_text_object_specifier('<'),
            Some(TextObject::AngleBracket)
        );
        assert_eq!(
            parse_text_object_specifier('>'),
            Some(TextObject::AngleBracket)
        );
    }

    #[test]
    fn test_parse_text_object_specifier_invalid() {
        assert_eq!(parse_text_object_specifier('x'), None);
        assert_eq!(parse_text_object_specifier('z'), None);
        assert_eq!(parse_text_object_specifier('1'), None);
    }

    // resolve_text_object dispatcher tests

    #[test]
    fn test_resolve_text_object_word() {
        with_ctx("hello world", 0, 0, |_, ctx| {
            let range =
                resolve_text_object(ctx, TextObjectKind::Inner, TextObject::Word, 1).unwrap();
            assert_eq!(range.start.col, 0);
            assert_eq!(range.end.col, 5);
        });
    }

    #[test]
    fn test_resolve_text_object_quotes() {
        with_ctx(r#"say "hello" now"#, 0, 6, |_, ctx| {
            let range = resolve_text_object(ctx, TextObjectKind::Inner, TextObject::DoubleQuote, 1)
                .unwrap();
            assert_eq!(range.start.col, 5);
            assert_eq!(range.end.col, 10);
        });
    }

    #[test]
    fn test_resolve_text_object_brackets() {
        with_ctx("fn(abc)", 0, 4, |_, ctx| {
            let range = resolve_text_object(ctx, TextObjectKind::Inner, TextObject::Parenthesis, 1)
                .unwrap();
            assert_eq!(range.start.col, 3);
            assert_eq!(range.end.col, 6);
        });
    }
}

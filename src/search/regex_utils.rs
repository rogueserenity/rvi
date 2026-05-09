//! Vi regex syntax conversion and compiled regex wrapper.
//!
//! Vi uses "basic" regex syntax where `(`, `)`, `+`, and `|` are literal
//! characters unless backslash-escaped, which is the opposite of most modern
//! regex engines. This module converts vi patterns to the `fancy-regex`
//! dialect so the editor can match using standard Rust regex infrastructure.

use crate::error::SearchError;

/// Compiled regex wrapper that stores both the compiled regex and the
/// original vi pattern string for display purposes.
///
/// The inner `fancy_regex::Regex` is always compiled from a converted
/// pattern, ensuring vi-to-regex translation is applied consistently.
///
/// # Examples
///
/// ```
/// use rvi::search::regex_utils::ViRegex;
///
/// let re = ViRegex::compile("hello").unwrap();
/// assert_eq!(re.pattern(), "hello");
/// assert_eq!(re.find_in("say hello world").unwrap(), Some((4, 9)));
/// ```
#[derive(Debug, Clone)]
pub struct ViRegex {
    regex: fancy_regex::Regex,
    pattern: String,
}

impl ViRegex {
    /// Compile a vi-style regex pattern.
    ///
    /// The pattern is first converted from vi basic regex syntax to
    /// standard regex syntax, then compiled with `fancy_regex`.
    ///
    /// # Errors
    ///
    /// Returns `SearchError::InvalidPattern` if the converted pattern
    /// fails to compile.
    pub fn compile(vi_pattern: &str) -> Result<Self, SearchError> {
        let converted = convert_vi_to_regex(vi_pattern);
        let regex = fancy_regex::Regex::new(&converted)
            .map_err(|e| SearchError::InvalidPattern(format!("{}: {}", vi_pattern, e)))?;
        Ok(Self {
            regex,
            pattern: vi_pattern.to_string(),
        })
    }

    /// Compile a vi-style regex pattern with optional case-insensitive flag.
    ///
    /// When `case_insensitive` is true, the pattern is wrapped with `(?i)`
    /// to enable case-insensitive matching.
    ///
    /// # Errors
    ///
    /// Returns `SearchError::InvalidPattern` if the converted pattern
    /// fails to compile.
    pub fn compile_with_flags(
        vi_pattern: &str,
        case_insensitive: bool,
    ) -> Result<Self, SearchError> {
        let converted = convert_vi_to_regex(vi_pattern);
        let pattern = if case_insensitive {
            format!("(?i){}", converted)
        } else {
            converted
        };
        let regex = fancy_regex::Regex::new(&pattern)
            .map_err(|e| SearchError::InvalidPattern(format!("{}: {}", vi_pattern, e)))?;
        Ok(Self {
            regex,
            pattern: vi_pattern.to_string(),
        })
    }

    /// Compile a vi-style regex pattern with case and magic settings.
    ///
    /// When `magic` is false (`:set nomagic`), the metacharacters `.`, `*`,
    /// and `[` become literal unless backslash-escaped. This matches POSIX vi
    /// `nomagic` behavior.
    ///
    /// # Errors
    ///
    /// Returns `SearchError::InvalidPattern` if the converted pattern
    /// fails to compile.
    pub fn compile_with_magic(
        vi_pattern: &str,
        case_insensitive: bool,
        magic: bool,
    ) -> Result<Self, SearchError> {
        let pattern_to_convert = if magic {
            vi_pattern.to_string()
        } else {
            escape_nomagic(vi_pattern)
        };
        Self::compile_with_flags(&pattern_to_convert, case_insensitive)
    }

    /// Find the first match in `text`, returning byte offset range `(start, end)`.
    ///
    /// The end offset is exclusive, matching Rust slice semantics.
    ///
    /// # Errors
    ///
    /// Returns `SearchError::RegexError` if the regex engine encounters
    /// a runtime error (e.g., backtracking limit exceeded).
    pub fn find_in(&self, text: &str) -> Result<Option<(usize, usize)>, SearchError> {
        self.regex
            .find(text)
            .map(|m| m.map(|m| (m.start(), m.end())))
            .map_err(|e| SearchError::RegexError(e.to_string()))
    }

    /// Find all non-overlapping matches in `text`, returning byte offset
    /// ranges `(start, end)`.
    ///
    /// # Errors
    ///
    /// Returns `SearchError::RegexError` if the regex engine encounters
    /// a runtime error on any match attempt.
    pub fn find_all_in(&self, text: &str) -> Result<Vec<(usize, usize)>, SearchError> {
        let mut results = Vec::new();
        for m in self.regex.find_iter(text) {
            let m = m.map_err(|e| SearchError::RegexError(e.to_string()))?;
            results.push((m.start(), m.end()));
        }
        Ok(results)
    }

    /// Find the first capture groups in `text`.
    ///
    /// Returns the `Captures` object from `fancy_regex` for use with
    /// replacement expansion.
    ///
    /// # Errors
    ///
    /// Returns `SearchError::RegexError` if the regex engine encounters
    /// a runtime error during matching.
    pub fn captures_in<'t>(
        &self,
        text: &'t str,
    ) -> Result<Option<fancy_regex::Captures<'t>>, SearchError> {
        self.regex
            .captures(text)
            .map_err(|e| SearchError::RegexError(e.to_string()))
    }

    /// Get an iterator over all capture matches in text.
    ///
    /// Each item is a `fancy_regex::Captures` for use with replacement
    /// expansion.
    pub fn captures_iter<'r, 't>(&'r self, text: &'t str) -> fancy_regex::CaptureMatches<'r, 't> {
        self.regex.captures_iter(text)
    }

    /// Get a reference to the inner `fancy_regex::Regex`.
    ///
    /// Needed for advanced operations like `captures_from_pos`.
    pub fn inner(&self) -> &fancy_regex::Regex {
        &self.regex
    }

    /// Get the original vi pattern string.
    pub fn pattern(&self) -> &str {
        &self.pattern
    }
}

/// Escape metacharacters for `nomagic` mode.
///
/// In nomagic mode, `.`, `*`, and `[` are literal unless preceded by `\`.
/// Conversely, `\.`, `\*`, and `\[` restore their special meaning.
/// `^` at the start and `$` at the end remain special (POSIX requirement).
fn escape_nomagic(pattern: &str) -> String {
    let mut output = String::with_capacity(pattern.len() + 8);
    let mut chars = pattern.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            // Check if next char is a nomagic metachar being re-enabled
            if let Some(&next) = chars.peek() {
                match next {
                    '.' | '*' | '[' | '~' => {
                        // \. in nomagic means special . — pass through as-is
                        output.push(next);
                        chars.next();
                    }
                    _ => {
                        // Pass through other backslash sequences
                        output.push('\\');
                        output.push(next);
                        chars.next();
                    }
                }
            } else {
                output.push('\\');
            }
        } else if ch == '.' || ch == '*' || ch == '[' || ch == '~' {
            // In nomagic, these are literal
            output.push('\\');
            output.push(ch);
        } else {
            output.push(ch);
        }
    }
    output
}

/// Convert a vi basic regex pattern to a `fancy-regex` compatible pattern.
///
/// Vi regex differences from standard (extended) regex:
///
/// | Vi syntax | Standard syntax | Meaning              |
/// |-----------|-----------------|----------------------|
/// | `\(` `\)` | `(` `)`        | Capture group        |
/// | `\+`      | `+`             | One-or-more          |
/// | `\|`      | `\|`            | Alternation          |
/// | `\<` `\>` | `\b`            | Word boundary        |
/// | `(` `)`   | `\(` `\)`      | Literal parentheses  |
/// | `+`       | `\+`            | Literal plus         |
/// | `|`       | `\|`            | Literal pipe         |
///
/// Characters inside a character class `[...]` are passed through unchanged.
/// Backslash sequences not listed above (e.g., `\n`, `\t`, `\d`, `\1`-`\9`)
/// are passed through unchanged.
fn convert_vi_to_regex(vi_pattern: &str) -> String {
    let mut output = String::with_capacity(vi_pattern.len() + 8);
    let mut chars = vi_pattern.chars().peekable();
    let mut in_char_class = false;

    while let Some(ch) = chars.next() {
        if in_char_class {
            output.push(ch);
            // Track end of character class, but not `]` immediately after `[` or `[^`
            if ch == ']' {
                in_char_class = false;
            }
            continue;
        }

        match ch {
            '\\' => {
                if let Some(&next) = chars.peek() {
                    match next {
                        '(' => {
                            // vi \( -> standard (
                            output.push('(');
                            chars.next();
                        }
                        ')' => {
                            // vi \) -> standard )
                            output.push(')');
                            chars.next();
                        }
                        '+' => {
                            // vi \+ -> standard +
                            output.push('+');
                            chars.next();
                        }
                        '|' => {
                            // vi \| -> standard |
                            output.push('|');
                            chars.next();
                        }
                        '<' => {
                            // vi \< -> standard \b (word boundary start)
                            output.push_str("\\b");
                            chars.next();
                        }
                        '>' => {
                            // vi \> -> standard \b (word boundary end)
                            output.push_str("\\b");
                            chars.next();
                        }
                        // All other backslash sequences pass through unchanged:
                        // \1-\9 (backreferences), \n, \t, \s, \d, \w, \., \*, \[, etc.
                        _ => {
                            output.push('\\');
                            output.push(next);
                            chars.next();
                        }
                    }
                } else {
                    // Trailing backslash - pass through as literal
                    output.push('\\');
                }
            }
            '(' => {
                // Bare ( is literal in vi basic regex
                output.push_str("\\(");
            }
            ')' => {
                // Bare ) is literal in vi basic regex
                output.push_str("\\)");
            }
            '+' => {
                // Bare + is literal in vi basic regex
                output.push_str("\\+");
            }
            '|' => {
                // Bare | is literal in vi basic regex
                output.push_str("\\|");
            }
            '[' => {
                output.push('[');
                in_char_class = true;
                // Handle `[^` and `[]` / `[^]` cases where `]` is part of the class
                if let Some(&next) = chars.peek() {
                    if next == '^' {
                        output.push('^');
                        chars.next();
                        // After `[^`, a `]` is literal
                        if let Some(&after) = chars.peek() {
                            if after == ']' {
                                output.push(']');
                                chars.next();
                            }
                        }
                    } else if next == ']' {
                        // `[]` - the `]` is part of the class
                        output.push(']');
                        chars.next();
                    }
                }
            }
            // All other characters pass through unchanged:
            // . * ^ $ ] and regular characters
            _ => {
                output.push(ch);
            }
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // convert_vi_to_regex tests
    // =========================================================================

    #[test]
    fn test_plain_text_passthrough() {
        assert_eq!(convert_vi_to_regex("hello"), "hello");
        assert_eq!(convert_vi_to_regex("abc123"), "abc123");
    }

    #[test]
    fn test_common_regex_chars_passthrough() {
        assert_eq!(convert_vi_to_regex(".*"), ".*");
        assert_eq!(convert_vi_to_regex("^hello$"), "^hello$");
        assert_eq!(convert_vi_to_regex("[a-z]"), "[a-z]");
    }

    #[test]
    fn test_vi_capture_groups() {
        // vi \( \) -> standard ( )
        assert_eq!(convert_vi_to_regex(r"\(foo\)"), "(foo)");
        assert_eq!(convert_vi_to_regex(r"\(a\|b\)"), "(a|b)");
    }

    #[test]
    fn test_vi_one_or_more() {
        // vi \+ -> standard +
        assert_eq!(convert_vi_to_regex(r"a\+"), "a+");
        assert_eq!(convert_vi_to_regex(r"[0-9]\+"), "[0-9]+");
    }

    #[test]
    fn test_vi_alternation() {
        // vi \| -> standard |
        assert_eq!(convert_vi_to_regex(r"foo\|bar"), "foo|bar");
    }

    #[test]
    fn test_vi_word_boundaries() {
        // vi \< \> -> standard \b
        assert_eq!(convert_vi_to_regex(r"\<word\>"), r"\bword\b");
    }

    #[test]
    fn test_bare_parens_become_escaped() {
        // Bare ( ) are literal in vi, so they become \( \) in standard
        assert_eq!(convert_vi_to_regex("(foo)"), r"\(foo\)");
    }

    #[test]
    fn test_bare_plus_becomes_escaped() {
        // Bare + is literal in vi
        assert_eq!(convert_vi_to_regex("a+b"), r"a\+b");
    }

    #[test]
    fn test_bare_pipe_becomes_escaped() {
        // Bare | is literal in vi
        assert_eq!(convert_vi_to_regex("a|b"), r"a\|b");
    }

    #[test]
    fn test_backreferences_passthrough() {
        // \1-\9 are the same in both
        assert_eq!(convert_vi_to_regex(r"\(foo\)\1"), r"(foo)\1");
    }

    #[test]
    fn test_escaped_literals_passthrough() {
        // \. \* \[ etc. stay the same
        assert_eq!(convert_vi_to_regex(r"\."), r"\.");
        assert_eq!(convert_vi_to_regex(r"\*"), r"\*");
        assert_eq!(convert_vi_to_regex(r"\["), r"\[");
    }

    #[test]
    fn test_special_sequences_passthrough() {
        // \n, \t, \s, \d, \w pass through
        assert_eq!(convert_vi_to_regex(r"\n"), r"\n");
        assert_eq!(convert_vi_to_regex(r"\t"), r"\t");
        assert_eq!(convert_vi_to_regex(r"\s"), r"\s");
        assert_eq!(convert_vi_to_regex(r"\d"), r"\d");
        assert_eq!(convert_vi_to_regex(r"\w"), r"\w");
    }

    #[test]
    fn test_character_class_passthrough() {
        // Everything inside [ ] passes through unchanged
        assert_eq!(convert_vi_to_regex(r"[a-z\(]"), r"[a-z\(]");
        assert_eq!(convert_vi_to_regex("[a+b|c]"), "[a+b|c]");
        assert_eq!(convert_vi_to_regex("[()]"), "[()]");
    }

    #[test]
    fn test_negated_character_class() {
        assert_eq!(convert_vi_to_regex("[^abc]"), "[^abc]");
    }

    #[test]
    fn test_character_class_with_closing_bracket() {
        // [] is a class containing ], [^] is a class containing ]
        assert_eq!(convert_vi_to_regex("[]abc]"), "[]abc]");
        assert_eq!(convert_vi_to_regex("[^]abc]"), "[^]abc]");
    }

    #[test]
    fn test_complex_vi_pattern() {
        // vi pattern: \<\(foo\|bar\)\> matches word "foo" or "bar"
        assert_eq!(convert_vi_to_regex(r"\<\(foo\|bar\)\>"), r"\b(foo|bar)\b");
    }

    #[test]
    fn test_trailing_backslash() {
        assert_eq!(convert_vi_to_regex(r"foo\"), r"foo\");
    }

    #[test]
    fn test_empty_pattern() {
        assert_eq!(convert_vi_to_regex(""), "");
    }

    // =========================================================================
    // ViRegex::compile tests
    // =========================================================================

    #[test]
    fn test_compile_valid_pattern() {
        let re = ViRegex::compile("hello").unwrap();
        assert_eq!(re.pattern(), "hello");
    }

    #[test]
    fn test_compile_invalid_pattern() {
        // Unmatched \( without \) becomes unmatched ( in standard regex
        let result = ViRegex::compile(r"\(");
        assert!(result.is_err());
    }

    #[test]
    fn test_compile_vi_group_pattern() {
        let re = ViRegex::compile(r"\(foo\)").unwrap();
        assert_eq!(re.pattern(), r"\(foo\)");
    }

    // =========================================================================
    // ViRegex::compile_with_flags tests
    // =========================================================================

    #[test]
    fn test_compile_with_flags_case_sensitive() {
        let re = ViRegex::compile_with_flags("Hello", false).unwrap();
        assert_eq!(re.find_in("hello world").unwrap(), None);
        assert_eq!(re.find_in("Hello world").unwrap(), Some((0, 5)));
    }

    #[test]
    fn test_compile_with_flags_case_insensitive() {
        let re = ViRegex::compile_with_flags("Hello", true).unwrap();
        assert_eq!(re.find_in("hello world").unwrap(), Some((0, 5)));
        assert_eq!(re.find_in("HELLO world").unwrap(), Some((0, 5)));
    }

    #[test]
    fn test_compile_with_flags_preserves_pattern() {
        let re = ViRegex::compile_with_flags("test", true).unwrap();
        assert_eq!(re.pattern(), "test");
    }

    // =========================================================================
    // ViRegex::captures_in tests
    // =========================================================================

    #[test]
    fn test_captures_in_no_match() {
        let re = ViRegex::compile("xyz").unwrap();
        assert!(re.captures_in("hello").unwrap().is_none());
    }

    #[test]
    fn test_captures_in_with_groups() {
        let re = ViRegex::compile(r"\(foo\)\(bar\)").unwrap();
        let caps = re.captures_in("foobar").unwrap().unwrap();
        assert_eq!(caps.get(0).unwrap().as_str(), "foobar");
        assert_eq!(caps.get(1).unwrap().as_str(), "foo");
        assert_eq!(caps.get(2).unwrap().as_str(), "bar");
    }

    #[test]
    fn test_captures_in_without_groups() {
        let re = ViRegex::compile("hello").unwrap();
        let caps = re.captures_in("say hello").unwrap().unwrap();
        assert_eq!(caps.get(0).unwrap().as_str(), "hello");
        assert!(caps.get(1).is_none());
    }

    // =========================================================================
    // ViRegex::inner tests
    // =========================================================================

    #[test]
    fn test_inner_returns_regex() {
        let re = ViRegex::compile("hello").unwrap();
        let inner = re.inner();
        // Verify the inner regex can find matches
        let m = inner.find("hello world").unwrap().unwrap();
        assert_eq!(m.start(), 0);
        assert_eq!(m.end(), 5);
    }

    // =========================================================================
    // ViRegex::find_in tests
    // =========================================================================

    #[test]
    fn test_find_in_basic() {
        let re = ViRegex::compile("world").unwrap();
        assert_eq!(re.find_in("hello world").unwrap(), Some((6, 11)));
    }

    #[test]
    fn test_find_in_no_match() {
        let re = ViRegex::compile("xyz").unwrap();
        assert_eq!(re.find_in("hello world").unwrap(), None);
    }

    #[test]
    fn test_find_in_at_start() {
        let re = ViRegex::compile("hello").unwrap();
        assert_eq!(re.find_in("hello world").unwrap(), Some((0, 5)));
    }

    #[test]
    fn test_find_in_with_regex() {
        let re = ViRegex::compile("[0-9]\\+").unwrap();
        assert_eq!(re.find_in("abc 42 def").unwrap(), Some((4, 6)));
    }

    #[test]
    fn test_find_in_word_boundary() {
        let re = ViRegex::compile(r"\<the\>").unwrap();
        assert_eq!(re.find_in("the other").unwrap(), Some((0, 3)));
        // Should not match partial words
        assert_eq!(re.find_in("there").unwrap(), None);
    }

    #[test]
    fn test_find_in_empty_text() {
        let re = ViRegex::compile("hello").unwrap();
        assert_eq!(re.find_in("").unwrap(), None);
    }

    #[test]
    fn test_find_in_empty_pattern() {
        let re = ViRegex::compile("").unwrap();
        // Empty pattern matches at position 0
        assert_eq!(re.find_in("hello").unwrap(), Some((0, 0)));
    }

    // =========================================================================
    // ViRegex::find_all_in tests
    // =========================================================================

    #[test]
    fn test_find_all_in_multiple() {
        let re = ViRegex::compile("ab").unwrap();
        let matches = re.find_all_in("ab cd ab ef ab").unwrap();
        assert_eq!(matches, vec![(0, 2), (6, 8), (12, 14)]);
    }

    #[test]
    fn test_find_all_in_no_matches() {
        let re = ViRegex::compile("xyz").unwrap();
        let matches = re.find_all_in("hello world").unwrap();
        assert!(matches.is_empty());
    }

    #[test]
    fn test_find_all_in_overlapping_positions() {
        let re = ViRegex::compile("aa").unwrap();
        // Non-overlapping: "aaa" has match at (0,2) but not (1,3)
        let matches = re.find_all_in("aaa").unwrap();
        assert_eq!(matches, vec![(0, 2)]);
    }

    // =========================================================================
    // Unicode tests
    // =========================================================================

    #[test]
    fn test_find_in_unicode() {
        let re = ViRegex::compile("world").unwrap();
        // CJK chars are 3 bytes each
        assert_eq!(re.find_in("\u{4F60}\u{597D} world").unwrap(), Some((7, 12)));
    }

    #[test]
    fn test_find_in_unicode_pattern() {
        let re = ViRegex::compile("\u{4F60}\u{597D}").unwrap();
        assert_eq!(re.find_in("say \u{4F60}\u{597D}!").unwrap(), Some((4, 10)));
    }

    // =========================================================================
    // escape_nomagic tests
    // =========================================================================

    #[test]
    fn test_nomagic_dot_becomes_literal() {
        // In nomagic, bare . is literal
        let escaped = escape_nomagic("a.b");
        assert_eq!(escaped, "a\\.b");
    }

    #[test]
    fn test_nomagic_star_becomes_literal() {
        let escaped = escape_nomagic("a*b");
        assert_eq!(escaped, "a\\*b");
    }

    #[test]
    fn test_nomagic_bracket_becomes_literal() {
        let escaped = escape_nomagic("a[b");
        assert_eq!(escaped, "a\\[b");
    }

    #[test]
    fn test_nomagic_backslash_dot_restores_special() {
        // \. in nomagic means special dot
        let escaped = escape_nomagic("a\\.b");
        assert_eq!(escaped, "a.b");
    }

    #[test]
    fn test_nomagic_backslash_star_restores_special() {
        let escaped = escape_nomagic("a\\*b");
        assert_eq!(escaped, "a*b");
    }

    #[test]
    fn test_nomagic_plain_text_unchanged() {
        let escaped = escape_nomagic("hello");
        assert_eq!(escaped, "hello");
    }

    #[test]
    fn test_nomagic_caret_dollar_unchanged() {
        // ^ and $ are always special in vi, even in nomagic
        let escaped = escape_nomagic("^hello$");
        assert_eq!(escaped, "^hello$");
    }

    // =========================================================================
    // compile_with_magic tests
    // =========================================================================

    #[test]
    fn test_compile_with_magic_true() {
        // magic=true: dot is special (matches any char)
        let re = ViRegex::compile_with_magic("a.c", false, true).unwrap();
        assert!(re.find_in("abc").unwrap().is_some());
        assert!(re.find_in("axc").unwrap().is_some());
    }

    #[test]
    fn test_compile_with_magic_false() {
        // magic=false: dot is literal
        let re = ViRegex::compile_with_magic("a.c", false, false).unwrap();
        assert!(re.find_in("a.c").unwrap().is_some());
        assert!(re.find_in("abc").unwrap().is_none());
    }

    #[test]
    fn test_compile_with_magic_false_backslash_dot_special() {
        // In nomagic, \. makes dot special
        let re = ViRegex::compile_with_magic("a\\.c", false, false).unwrap();
        assert!(re.find_in("abc").unwrap().is_some());
        assert!(re.find_in("axc").unwrap().is_some());
    }
}

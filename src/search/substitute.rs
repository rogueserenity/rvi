//! Substitute command parsing and execution.
//!
//! This module provides the core logic for the vi `:s/pattern/replacement/flags`
//! command. It handles:
//!
//! 1. Parsing the substitute body (delimiter detection, pattern/replacement/flags)
//! 2. Replacement string expansion (`&`, `\0`, `\1`-`\9`, `\\`, `\&`)
//! 3. Executing substitutions across a line range
//! 4. Returning structured results for status message display

use crate::buffer::unicode::next_char_boundary;
use crate::error::SearchError;
use crate::search::regex_utils::ViRegex;

/// A fully parsed substitute command ready for execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubstituteCommand {
    /// Line range to operate on (0-based, inclusive on both ends).
    pub range: SubstituteRange,
    /// The search pattern (vi regex syntax, pre-conversion).
    pub pattern: String,
    /// The replacement template string.
    pub replacement: String,
    /// Flags controlling substitution behavior.
    pub flags: SubstituteFlags,
}

/// Line range for the substitute command.
///
/// All line numbers in the user-facing syntax are 1-based but stored
/// as 0-based indices internally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubstituteRange {
    /// Current line only (default when no range specified: `:s/...`)
    CurrentLine,
    /// A single specific line: `:5s/...`
    Line(usize),
    /// An inclusive line range: `:5,10s/...`
    Range { start: usize, end: usize },
    /// Entire file: `:%s/...`
    WholeFile,
}

/// Flags that modify substitute behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SubstituteFlags {
    /// Replace all occurrences on each line (not just the first).
    pub global: bool,
    /// Case-insensitive matching.
    pub case_insensitive: bool,
    /// Print the last substituted line after completion (`:s/…/…/p`).
    pub print: bool,
    /// Print the last substituted line in list format (`:s/…/…/l`).
    pub list: bool,
    /// Print the last substituted line with its line number (`:s/…/…/#`).
    pub number: bool,
    /// Confirm each substitution interactively (`:s/…/…/c`).
    pub confirm: bool,
}

/// Result of a successful substitution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubstituteResult {
    /// Total number of individual substitutions made.
    pub substitution_count: usize,
    /// Number of lines where at least one substitution occurred.
    pub line_count: usize,
}

/// Parse the body of a substitute command after the `s`.
///
/// Expects input like: `/pattern/replacement/flags`
///
/// The delimiter is the first character (commonly `/` but any char works,
/// matching vi behavior). Escaped delimiters within pattern/replacement
/// are treated as literal. The trailing delimiter is optional.
///
/// # Returns
///
/// The parsed pattern, replacement, and flags on success.
///
/// # Errors
///
/// Returns an error string if the syntax is invalid (e.g., missing delimiter
/// or unknown flags).
pub fn parse_substitute_body(body: &str) -> Result<(String, String, SubstituteFlags), String> {
    if body.is_empty() {
        return Err("Trailing characters".to_string());
    }

    let mut chars = body.chars();
    let delimiter = chars.next().unwrap(); // safe: body is non-empty

    // Collect the remaining string after the delimiter
    let rest: String = chars.collect();

    // Split by unescaped delimiter into segments
    let segments = split_by_unescaped(&rest, delimiter);

    let pattern = unescape_delimiter(&segments[0], delimiter);

    let replacement = if segments.len() > 1 {
        unescape_delimiter(&segments[1], delimiter)
    } else {
        String::new()
    };

    let flags_str = if segments.len() > 2 {
        segments[2].as_str()
    } else {
        ""
    };

    let flags = parse_flags(flags_str)?;

    Ok((pattern, replacement, flags))
}

/// Split a string by unescaped occurrences of `delimiter`.
///
/// A backslash before the delimiter prevents the split. Returns up to
/// three segments (pattern, replacement, flags).
fn split_by_unescaped(s: &str, delimiter: char) -> Vec<String> {
    let mut segments = Vec::with_capacity(3);
    let mut current = String::new();
    let mut chars = s.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(&next) = chars.peek() {
                if next == delimiter {
                    // Escaped delimiter: consume and keep as escaped
                    current.push('\\');
                    current.push(next);
                    chars.next();
                    continue;
                }
            }
            current.push(ch);
        } else if ch == delimiter {
            segments.push(current);
            current = String::new();
            if segments.len() == 2 {
                // Everything after the third delimiter is flags
                let remaining: String = chars.collect();
                segments.push(remaining);
                return segments;
            }
        } else {
            current.push(ch);
        }
    }

    segments.push(current);
    segments
}

/// Remove delimiter escapes from a segment (e.g., `\/` becomes `/`).
fn unescape_delimiter(s: &str, delimiter: char) -> String {
    let escaped = format!("\\{}", delimiter);
    s.replace(&escaped, &delimiter.to_string())
}

/// Parse the flags string into `SubstituteFlags`.
fn parse_flags(flags_str: &str) -> Result<SubstituteFlags, String> {
    let mut flags = SubstituteFlags::default();

    for ch in flags_str.chars() {
        match ch {
            'g' => flags.global = true,
            'i' | 'I' => flags.case_insensitive = true,
            'p' => flags.print = true,
            'l' => flags.list = true,
            '#' => flags.number = true,
            'c' => flags.confirm = true,
            _ => return Err(format!("Unknown flag: {}", ch)),
        }
    }

    Ok(flags)
}

/// Expand a vi replacement template against captured groups.
///
/// Handles:
/// - `&` or `\0`: entire matched text
/// - `\1` through `\9`: captured group N
/// - `\\`: literal backslash
/// - `\&`: literal ampersand
/// - All other characters: literal
pub fn expand_replacement(template: &str, captures: &fancy_regex::Captures<'_>) -> String {
    let mut result = String::with_capacity(template.len() * 2);
    let mut chars = template.chars().peekable();

    let whole_match = captures.get(0).map(|m| m.as_str()).unwrap_or("");

    while let Some(ch) = chars.next() {
        match ch {
            '&' => {
                result.push_str(whole_match);
            }
            '\\' => {
                if let Some(&next) = chars.peek() {
                    match next {
                        '0' => {
                            result.push_str(whole_match);
                            chars.next();
                        }
                        '1'..='9' => {
                            let group_num = (next as u32 - '0' as u32) as usize;
                            if let Some(m) = captures.get(group_num) {
                                result.push_str(m.as_str());
                            }
                            // If the group did not participate, insert nothing
                            chars.next();
                        }
                        '\\' => {
                            result.push('\\');
                            chars.next();
                        }
                        '&' => {
                            result.push('&');
                            chars.next();
                        }
                        _ => {
                            // Unknown escape: keep the backslash and next char
                            result.push('\\');
                            result.push(next);
                            chars.next();
                        }
                    }
                } else {
                    // Trailing backslash
                    result.push('\\');
                }
            }
            _ => {
                result.push(ch);
            }
        }
    }

    result
}

/// Perform substitution on a single line.
///
/// Returns `None` if no match on this line, or `Some((new_line, match_count))`.
///
/// # Errors
///
/// Returns `SearchError::RegexError` if the regex engine encounters a
/// runtime error during matching.
pub fn substitute_line(
    line: &str,
    regex: &ViRegex,
    replacement: &str,
    global: bool,
) -> Result<Option<(String, usize)>, SearchError> {
    let mut result = String::with_capacity(line.len());
    let mut last_end = 0;
    let mut match_count = 0;

    for cap_result in regex.captures_iter(line) {
        let caps = cap_result.map_err(|e| SearchError::RegexError(e.to_string()))?;

        let whole = caps.get(0).unwrap(); // group 0 always exists on a match
        let start = whole.start();
        let end = whole.end();

        // Append text between the last match and this one
        result.push_str(&line[last_end..start]);

        // Expand the replacement template
        result.push_str(&expand_replacement(replacement, &caps));

        last_end = end;
        match_count += 1;

        // Guard against zero-length matches to prevent infinite loops:
        // if a match is zero-length, advance by one byte
        if start == end {
            if start < line.len() {
                // Push the character at the current position and advance
                let next_boundary = next_char_boundary(line, start);
                result.push_str(&line[start..next_boundary]);
                last_end = next_boundary;
            } else {
                break;
            }
        }

        if !global {
            break;
        }
    }

    if match_count == 0 {
        return Ok(None);
    }

    // Append remainder of line after last match
    result.push_str(&line[last_end..]);

    Ok(Some((result, match_count)))
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // parse_substitute_body tests
    // =========================================================================

    #[test]
    fn test_parse_basic() {
        let (pattern, replacement, flags) = parse_substitute_body("/foo/bar/").unwrap();
        assert_eq!(pattern, "foo");
        assert_eq!(replacement, "bar");
        assert!(!flags.global);
        assert!(!flags.case_insensitive);
    }

    #[test]
    fn test_parse_with_global_flag() {
        let (_, _, flags) = parse_substitute_body("/foo/bar/g").unwrap();
        assert!(flags.global);
        assert!(!flags.case_insensitive);
    }

    #[test]
    fn test_parse_with_case_insensitive_flag() {
        let (_, _, flags) = parse_substitute_body("/foo/bar/i").unwrap();
        assert!(!flags.global);
        assert!(flags.case_insensitive);
    }

    #[test]
    fn test_parse_with_combined_flags() {
        let (_, _, flags) = parse_substitute_body("/foo/bar/gi").unwrap();
        assert!(flags.global);
        assert!(flags.case_insensitive);
    }

    #[test]
    fn test_parse_no_trailing_delimiter() {
        let (pattern, replacement, flags) = parse_substitute_body("/foo/bar").unwrap();
        assert_eq!(pattern, "foo");
        assert_eq!(replacement, "bar");
        assert!(!flags.global);
    }

    #[test]
    fn test_parse_empty_replacement() {
        let (pattern, replacement, _) = parse_substitute_body("/foo//").unwrap();
        assert_eq!(pattern, "foo");
        assert_eq!(replacement, "");
    }

    #[test]
    fn test_parse_empty_pattern() {
        let (pattern, replacement, _) = parse_substitute_body("//bar/").unwrap();
        assert_eq!(pattern, "");
        assert_eq!(replacement, "bar");
    }

    #[test]
    fn test_parse_alternate_delimiter() {
        let (pattern, replacement, _) = parse_substitute_body("#foo#bar#").unwrap();
        assert_eq!(pattern, "foo");
        assert_eq!(replacement, "bar");
    }

    #[test]
    fn test_parse_escaped_delimiter() {
        let (pattern, replacement, _) = parse_substitute_body(r"/foo\/bar/baz/").unwrap();
        assert_eq!(pattern, "foo/bar");
        assert_eq!(replacement, "baz");
    }

    #[test]
    fn test_parse_escaped_delimiter_in_replacement() {
        let (_, replacement, _) = parse_substitute_body(r"/foo/bar\/baz/").unwrap();
        assert_eq!(replacement, "bar/baz");
    }

    #[test]
    fn test_parse_pattern_only() {
        // :s/foo with no second delimiter
        let (pattern, replacement, _) = parse_substitute_body("/foo").unwrap();
        assert_eq!(pattern, "foo");
        assert_eq!(replacement, "");
    }

    #[test]
    fn test_parse_unknown_flag() {
        let result = parse_substitute_body("/foo/bar/z");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown flag"));
    }

    #[test]
    fn test_parse_empty_body() {
        assert!(parse_substitute_body("").is_err());
    }

    // =========================================================================
    // expand_replacement tests
    // =========================================================================

    #[test]
    fn test_expand_literal_replacement() {
        let re = ViRegex::compile("foo").unwrap();
        let caps = re.captures_in("foo bar").unwrap().unwrap();
        assert_eq!(expand_replacement("baz", &caps), "baz");
    }

    #[test]
    fn test_expand_ampersand_whole_match() {
        let re = ViRegex::compile("foo").unwrap();
        let caps = re.captures_in("foo bar").unwrap().unwrap();
        assert_eq!(expand_replacement("[&]", &caps), "[foo]");
    }

    #[test]
    fn test_expand_backslash_zero_whole_match() {
        let re = ViRegex::compile("foo").unwrap();
        let caps = re.captures_in("foo bar").unwrap().unwrap();
        assert_eq!(expand_replacement(r"[\0]", &caps), "[foo]");
    }

    #[test]
    fn test_expand_capture_group() {
        let re = ViRegex::compile(r"\(foo\)\(bar\)").unwrap();
        let caps = re.captures_in("foobar").unwrap().unwrap();
        assert_eq!(expand_replacement(r"\2-\1", &caps), "bar-foo");
    }

    #[test]
    fn test_expand_nonexistent_group() {
        let re = ViRegex::compile("foo").unwrap();
        let caps = re.captures_in("foo").unwrap().unwrap();
        // \1 with no groups inserts nothing
        assert_eq!(expand_replacement(r"\1", &caps), "");
    }

    #[test]
    fn test_expand_escaped_backslash() {
        let re = ViRegex::compile("foo").unwrap();
        let caps = re.captures_in("foo").unwrap().unwrap();
        assert_eq!(expand_replacement(r"\\", &caps), "\\");
    }

    #[test]
    fn test_expand_escaped_ampersand() {
        let re = ViRegex::compile("foo").unwrap();
        let caps = re.captures_in("foo").unwrap().unwrap();
        assert_eq!(expand_replacement(r"\&", &caps), "&");
    }

    #[test]
    fn test_expand_trailing_backslash() {
        let re = ViRegex::compile("foo").unwrap();
        let caps = re.captures_in("foo").unwrap().unwrap();
        assert_eq!(expand_replacement("bar\\", &caps), "bar\\");
    }

    #[test]
    fn test_expand_unknown_escape() {
        let re = ViRegex::compile("foo").unwrap();
        let caps = re.captures_in("foo").unwrap().unwrap();
        assert_eq!(expand_replacement(r"\n", &caps), "\\n");
    }

    // =========================================================================
    // substitute_line tests
    // =========================================================================

    #[test]
    fn test_substitute_line_single_match() {
        let re = ViRegex::compile("foo").unwrap();
        let result = substitute_line("foo bar foo", &re, "baz", false).unwrap();
        let (new_line, count) = result.unwrap();
        assert_eq!(new_line, "baz bar foo");
        assert_eq!(count, 1);
    }

    #[test]
    fn test_substitute_line_global_match() {
        let re = ViRegex::compile("foo").unwrap();
        let result = substitute_line("foo bar foo", &re, "baz", true).unwrap();
        let (new_line, count) = result.unwrap();
        assert_eq!(new_line, "baz bar baz");
        assert_eq!(count, 2);
    }

    #[test]
    fn test_substitute_line_no_match() {
        let re = ViRegex::compile("xyz").unwrap();
        let result = substitute_line("foo bar", &re, "baz", false).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_substitute_line_with_groups() {
        let re = ViRegex::compile(r"\(hello\) \(world\)").unwrap();
        let result = substitute_line("hello world", &re, r"\2 \1", false).unwrap();
        let (new_line, count) = result.unwrap();
        assert_eq!(new_line, "world hello");
        assert_eq!(count, 1);
    }

    #[test]
    fn test_substitute_line_with_ampersand() {
        let re = ViRegex::compile("foo").unwrap();
        let result = substitute_line("foo bar", &re, "[&]", true).unwrap();
        let (new_line, _) = result.unwrap();
        assert_eq!(new_line, "[foo] bar");
    }

    #[test]
    fn test_substitute_line_regex_pattern() {
        let re = ViRegex::compile("[0-9]\\+").unwrap();
        let result = substitute_line("abc 42 def 99", &re, "NUM", true).unwrap();
        let (new_line, count) = result.unwrap();
        assert_eq!(new_line, "abc NUM def NUM");
        assert_eq!(count, 2);
    }

    #[test]
    fn test_substitute_line_case_insensitive() {
        let re = ViRegex::compile_with_flags("hello", true).unwrap();
        let result = substitute_line("Hello HELLO hello", &re, "hi", true).unwrap();
        let (new_line, count) = result.unwrap();
        assert_eq!(new_line, "hi hi hi");
        assert_eq!(count, 3);
    }

    #[test]
    fn test_substitute_line_empty_replacement() {
        let re = ViRegex::compile("foo").unwrap();
        let result = substitute_line("foo bar foo", &re, "", true).unwrap();
        let (new_line, count) = result.unwrap();
        assert_eq!(new_line, " bar ");
        assert_eq!(count, 2);
    }

    #[test]
    fn test_substitute_line_replacement_longer_than_match() {
        let re = ViRegex::compile("a").unwrap();
        let result = substitute_line("abc", &re, "XYZ", false).unwrap();
        let (new_line, _) = result.unwrap();
        assert_eq!(new_line, "XYZbc");
    }

    #[test]
    fn test_substitute_line_unicode() {
        let re = ViRegex::compile("\u{4F60}\u{597D}").unwrap();
        let result =
            substitute_line("\u{4F60}\u{597D}\u{4E16}\u{754C}", &re, "hello", false).unwrap();
        let (new_line, count) = result.unwrap();
        assert_eq!(new_line, "hello\u{4E16}\u{754C}");
        assert_eq!(count, 1);
    }

    // =========================================================================
    // Zero-length match tests
    // =========================================================================

    #[test]
    fn test_substitute_line_zero_length_anchor_start() {
        // Pattern ^ matches only at the start of the line
        let re = ViRegex::compile("^").unwrap();
        let result = substitute_line("abc", &re, "X", false).unwrap();
        let (new_line, count) = result.unwrap();
        assert_eq!(new_line, "Xabc");
        assert_eq!(count, 1);
    }

    #[test]
    fn test_substitute_line_zero_length_anchor_start_global() {
        // Pattern ^ with /g should only match once at start
        let re = ViRegex::compile("^").unwrap();
        let result = substitute_line("abc", &re, "X", true).unwrap();
        let (new_line, count) = result.unwrap();
        assert_eq!(new_line, "Xabc");
        assert_eq!(count, 1);
    }

    #[test]
    fn test_substitute_line_zero_length_anchor_end() {
        // Pattern $ matches only at the end of the line
        let re = ViRegex::compile("$").unwrap();
        let result = substitute_line("abc", &re, "X", false).unwrap();
        let (new_line, count) = result.unwrap();
        assert_eq!(new_line, "abcX");
        assert_eq!(count, 1);
    }

    #[test]
    fn test_substitute_line_zero_length_star_quantifier() {
        // Pattern a* (zero or more 'a') can match zero-length
        let re = ViRegex::compile("a*").unwrap();
        let result = substitute_line("bc", &re, "X", true).unwrap();
        let (new_line, count) = result.unwrap();
        // Should insert X before each character and at end
        assert_eq!(new_line, "XbXcX");
        assert_eq!(count, 3);
    }

    #[test]
    fn test_substitute_line_zero_length_dot_star() {
        // Pattern .* (zero or more any character) - greedy
        let re = ViRegex::compile(".*").unwrap();
        let result = substitute_line("abc", &re, "X", true).unwrap();
        let (new_line, count) = result.unwrap();
        // .* matches entire line greedily once, not followed by zero-length match
        // because our manual advance logic prevents captures_iter from seeing it
        assert_eq!(new_line, "X");
        assert_eq!(count, 1);
    }

    #[test]
    fn test_substitute_line_zero_length_empty_pattern() {
        // Empty pattern matches zero-length everywhere
        let re = ViRegex::compile("").unwrap();
        let result = substitute_line("ab", &re, "X", true).unwrap();
        let (new_line, count) = result.unwrap();
        // Should insert X before each character and at end
        assert_eq!(new_line, "XaXbX");
        assert_eq!(count, 3);
    }

    // =========================================================================
    // SubstituteCommand and related type tests
    // =========================================================================

    #[test]
    fn test_substitute_range_debug() {
        let range = SubstituteRange::Range { start: 0, end: 5 };
        let debug = format!("{:?}", range);
        assert!(debug.contains("Range"));
    }

    #[test]
    fn test_substitute_flags_default() {
        let flags = SubstituteFlags::default();
        assert!(!flags.global);
        assert!(!flags.case_insensitive);
    }

    #[test]
    fn test_substitute_command_construction() {
        let cmd = SubstituteCommand {
            range: SubstituteRange::WholeFile,
            pattern: "foo".to_string(),
            replacement: "bar".to_string(),
            flags: SubstituteFlags {
                global: true,
                ..SubstituteFlags::default()
            },
        };
        assert_eq!(cmd.range, SubstituteRange::WholeFile);
        assert_eq!(cmd.pattern, "foo");
        assert_eq!(cmd.replacement, "bar");
        assert!(cmd.flags.global);
    }

    #[test]
    fn test_substitute_result_construction() {
        let result = SubstituteResult {
            substitution_count: 5,
            line_count: 3,
        };
        assert_eq!(result.substitution_count, 5);
        assert_eq!(result.line_count, 3);
    }
}

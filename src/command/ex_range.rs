//! Ex command range parsing and resolution.
//!
//! Handles the address/range prefix that can appear before any ex command:
//! `1,5d`, `%s/foo/bar/`, `'a,'bd`, `/pattern/+1d`, etc.
//!
//! The entry points are:
//! - [`parse_range_prefix`] — for `:s` which requires the range to end with `s`
//! - [`parse_any_range`] — for all other range-prefixed commands
//! - [`parse_dest_addr`] — for `:m` and `:t` destination addresses
//! - [`resolve_range`] / [`resolve_addr`] — convert symbolic addresses to line numbers

use crate::command::ex_command::ExCommandContext;
use crate::search::substitute::SubstituteRange;

/// Symbolic range before resolution.
#[derive(Debug, Clone)]
pub(super) enum RangeSpec {
    /// No range specified (current line).
    None,
    /// `%` - whole file.
    WholeFile,
    /// A single line address.
    Single(LineAddr),
    /// Two line addresses separated by comma: second is resolved relative to cursor.
    Pair(LineAddr, LineAddr),
    /// Two line addresses separated by semicolon: second is resolved relative to first.
    ///
    /// `1;/foo/d` means "from line 1, find the next /foo/". The first address
    /// temporarily becomes the current line for resolving the second address.
    SemicolonPair(LineAddr, LineAddr),
}

/// A line address in a range specification.
#[derive(Debug, Clone)]
pub(super) enum LineAddr {
    /// A numeric line number (1-based from user input).
    Number(usize),
    /// `.` - current line.
    Current,
    /// `$` - last line.
    Last,
    /// `'a` - line marked with lowercase letter `a`.
    Mark(char),
    /// Address with a relative offset: `base +N` or `base -N`.
    Offset(Box<LineAddr>, i64),
    /// `/pattern/` (forward) or `?pattern?` (backward) — search to the next/previous
    /// matching line. An empty pattern re-uses `ctx.last_pattern`.
    Search { pattern: String, forward: bool },
}

/// Parse the range prefix from the input, returning the range spec and
/// the byte offset where the command character starts.
///
/// Returns `None` if the input does not start with a valid range
/// followed by 's'.
pub(super) fn parse_range_prefix(
    input: &str,
    _ctx: &ExCommandContext,
) -> Option<(RangeSpec, usize)> {
    let bytes = input.as_bytes();
    let len = bytes.len();

    if len == 0 {
        return None;
    }

    // Case: starts with 's' directly (no range)
    if bytes[0] == b's' {
        return Some((RangeSpec::None, 0));
    }

    // Case: starts with '%'
    if bytes[0] == b'%' {
        return Some((RangeSpec::WholeFile, 1));
    }

    // Parse first address
    let (addr1, pos1) = parse_line_addr(input, 0)?;

    if pos1 >= len {
        return None;
    }

    // Check for comma or semicolon separator
    if bytes[pos1] == b',' {
        let (addr2, pos2) = parse_line_addr(input, pos1 + 1)?;
        Some((RangeSpec::Pair(addr1, addr2), pos2))
    } else if bytes[pos1] == b';' {
        let (addr2, pos2) = parse_line_addr(input, pos1 + 1)?;
        Some((RangeSpec::SemicolonPair(addr1, addr2), pos2))
    } else {
        Some((RangeSpec::Single(addr1), pos1))
    }
}

/// Parse the optional range prefix from a general ex command input.
///
/// Unlike `parse_range_prefix`, this works for any command character.
/// Returns `(RangeSpec::None, 0)` if no range is present.
pub(super) fn parse_any_range(input: &str, _ctx: &ExCommandContext) -> (RangeSpec, usize) {
    let bytes = input.as_bytes();
    let len = bytes.len();

    if len == 0 {
        return (RangeSpec::None, 0);
    }

    // Case: starts with '%'
    if bytes[0] == b'%' {
        return (RangeSpec::WholeFile, 1);
    }

    // Try to parse first address
    let (addr1, pos1) = match parse_line_addr(input, 0) {
        Some(x) => x,
        None => return (RangeSpec::None, 0),
    };

    if pos1 >= len {
        return (RangeSpec::Single(addr1), pos1);
    }

    // Check for comma or semicolon separator
    if bytes[pos1] == b',' {
        if let Some((addr2, pos2)) = parse_line_addr(input, pos1 + 1) {
            return (RangeSpec::Pair(addr1, addr2), pos2);
        }
    } else if bytes[pos1] == b';' {
        if let Some((addr2, pos2)) = parse_line_addr(input, pos1 + 1) {
            return (RangeSpec::SemicolonPair(addr1, addr2), pos2);
        }
    }

    (RangeSpec::Single(addr1), pos1)
}

/// Parse a destination address for `:m` and `:t` commands.
///
/// Returns a 0-based insertion index: lines are inserted *before* row `dest`.
/// - `dest = 0` → insert before row 0 (before line 1, i.e. at start of file).
/// - `dest = N` → insert before row N (after 1-based line N, i.e. after row N-1).
/// - `dest = buffer_len` → append after last line.
pub(super) fn parse_dest_addr(arg: &str, ctx: &ExCommandContext) -> Result<usize, String> {
    let arg = arg.trim();
    // Address 0: insert before row 0 (start of file)
    if arg == "0" {
        return Ok(0);
    }
    // Parse as a line address
    match parse_line_addr(arg, 0) {
        Some((addr, pos)) if pos == arg.len() => {
            // For `$` and Number, the insert-after index equals the 0-based row + 1.
            // For all others (marks, offsets) resolve to 0-based row then add 1.
            match addr {
                // `.` (current line, 0-based) → insert after it → dest = current + 1
                LineAddr::Current => Ok(ctx.current_line + 1),
                LineAddr::Last => {
                    if ctx.buffer_len == 0 {
                        Err("Invalid address".to_string())
                    } else {
                        Ok(ctx.buffer_len)
                    }
                }
                LineAddr::Number(n) => {
                    if n > ctx.buffer_len {
                        Err(format!("Invalid address: line {} out of bounds", n))
                    } else {
                        Ok(n) // 1-based line N → insert before row N (= after row N-1)
                    }
                }
                // For marks and offsets: resolve to 0-based row, then insert *after* it
                other => {
                    let row = resolve_addr(other, ctx)?;
                    Ok(row + 1)
                }
            }
        }
        _ => Err(format!("Invalid address: {}", arg)),
    }
}

/// Parse a single line address starting at `pos`.
///
/// Handles:
/// - `.` (current line), `$` (last line), `N` (1-based number)
/// - `'a` (mark a–z)
/// - Optional `+N` or `-N` offset after any base address
///
/// Returns the address and the position after it.
pub(super) fn parse_line_addr(input: &str, pos: usize) -> Option<(LineAddr, usize)> {
    let bytes = input.as_bytes();
    if pos >= bytes.len() {
        return None;
    }

    // Parse base address
    let (base, end) = match bytes[pos] {
        b'.' => (LineAddr::Current, pos + 1),
        b'$' => (LineAddr::Last, pos + 1),
        b'0'..=b'9' => {
            let mut e = pos;
            while e < bytes.len() && bytes[e].is_ascii_digit() {
                e += 1;
            }
            let num: usize = input[pos..e].parse().ok()?;
            (LineAddr::Number(num), e)
        }
        // `'a` — mark address (lowercase a-z, or `<`/`>` for visual marks)
        b'\''
            if pos + 1 < bytes.len()
                && (bytes[pos + 1].is_ascii_lowercase()
                    || bytes[pos + 1] == b'<'
                    || bytes[pos + 1] == b'>') =>
        {
            let c = bytes[pos + 1] as char;
            (LineAddr::Mark(c), pos + 2)
        }
        // `/pattern/` (forward search) or `?pattern?` (backward search).
        // The closing delimiter is optional. `//` and `??` produce an empty
        // pattern, which resolve_addr treats as "reuse last_pattern".
        b'/' | b'?' => {
            let delim = bytes[pos];
            let forward = delim == b'/';
            let mut i = pos + 1;
            while i < bytes.len() && bytes[i] != delim {
                if bytes[i] == b'\\' {
                    i += 1; // skip escaped character
                }
                i += 1;
            }
            // Copy verbatim — the regex engine handles its own escaping
            let pattern = input[pos + 1..i].to_string();
            let end = if i < bytes.len() { i + 1 } else { i }; // consume closing delim
            (LineAddr::Search { pattern, forward }, end)
        }
        _ => return None,
    };

    // Parse optional +N or -N offset
    if end < bytes.len() && (bytes[end] == b'+' || bytes[end] == b'-') {
        let sign: i64 = if bytes[end] == b'+' { 1 } else { -1 };
        let offset_start = end + 1;
        let mut offset_end = offset_start;
        while offset_end < bytes.len() && bytes[offset_end].is_ascii_digit() {
            offset_end += 1;
        }
        if offset_end > offset_start {
            if let Ok(n) = input[offset_start..offset_end].parse::<i64>() {
                let addr = LineAddr::Offset(Box::new(base), sign * n);
                return Some((addr, offset_end));
            }
        } else {
            // Bare `+` or `-` means +1 or -1 (standard vi behavior, e.g. `.+d` = `.+1d`)
            let addr = LineAddr::Offset(Box::new(base), sign);
            return Some((addr, end + 1));
        }
    }

    Some((base, end))
}

/// Resolve a symbolic range into a concrete `SubstituteRange`.
pub(super) fn resolve_range(
    spec: RangeSpec,
    ctx: &ExCommandContext,
) -> Result<SubstituteRange, String> {
    match spec {
        RangeSpec::None => Ok(SubstituteRange::CurrentLine),
        RangeSpec::WholeFile => Ok(SubstituteRange::WholeFile),
        RangeSpec::Single(addr) => {
            let line = resolve_addr(addr, ctx)?;
            Ok(SubstituteRange::Line(line))
        }
        RangeSpec::Pair(addr1, addr2) => {
            let start = resolve_addr(addr1, ctx)?;
            let end = resolve_addr(addr2, ctx)?;
            if start > end {
                return Err("Invalid range".to_string());
            }
            Ok(SubstituteRange::Range { start, end })
        }
        RangeSpec::SemicolonPair(addr1, addr2) => {
            // Resolve addr1 normally, then resolve addr2 with current_line = addr1's result.
            // This makes relative addresses (like /pattern/ searches) in addr2 start from
            // the position of addr1 rather than the cursor.
            let start = resolve_addr(addr1, ctx)?;
            let ctx2 = ExCommandContext {
                current_line: start,
                ..ctx.clone()
            };
            let end = resolve_addr(addr2, &ctx2)?;
            if start > end {
                return Err("Invalid range".to_string());
            }
            Ok(SubstituteRange::Range { start, end })
        }
    }
}

/// Resolve a single line address to a 0-based line index.
pub(super) fn resolve_addr(addr: LineAddr, ctx: &ExCommandContext) -> Result<usize, String> {
    match addr {
        LineAddr::Current => Ok(ctx.current_line),
        LineAddr::Last => {
            if ctx.buffer_len == 0 {
                Err("Invalid range".to_string())
            } else {
                Ok(ctx.buffer_len - 1)
            }
        }
        LineAddr::Number(n) => {
            if n > ctx.buffer_len {
                Err(format!("Invalid range: line {} out of bounds", n))
            } else if n == 0 {
                // POSIX: address 0 means "before line 1"; for range operations
                // this resolves to the first line (0-based index 0).
                Ok(0)
            } else {
                Ok(n - 1) // convert 1-based to 0-based
            }
        }
        LineAddr::Mark(c) => ctx
            .marks
            .get(&c)
            .copied()
            .ok_or_else(|| format!("Mark '{}' not set", c)),
        LineAddr::Offset(base, delta) => {
            let base_line = resolve_addr(*base, ctx)?;
            let result = base_line as i64 + delta;
            if result < 0 || result as usize >= ctx.buffer_len {
                Err(format!("Invalid range: offset {} out of bounds", result))
            } else {
                Ok(result as usize)
            }
        }
        LineAddr::Search { pattern, forward } => {
            let pat_str = if pattern.is_empty() {
                ctx.last_pattern
                    .as_deref()
                    .ok_or_else(|| "E35: No previous search pattern".to_string())?
            } else {
                pattern.as_str()
            };
            let regex =
                crate::search::regex_utils::ViRegex::compile(pat_str).map_err(|e| e.to_string())?;
            let line_count = ctx.buffer_lines.len();
            if line_count == 0 {
                return Err("E486: Pattern not found".to_string());
            }
            if forward {
                for offset in 1..=line_count {
                    let row = (ctx.current_line + offset) % line_count;
                    if regex
                        .find_in(&ctx.buffer_lines[row])
                        .map_err(|e| e.to_string())?
                        .is_some()
                    {
                        return Ok(row);
                    }
                }
            } else {
                for offset in 1..=line_count {
                    let row = (ctx.current_line + line_count - offset) % line_count;
                    if regex
                        .find_in(&ctx.buffer_lines[row])
                        .map_err(|e| e.to_string())?
                        .is_some()
                    {
                        return Ok(row);
                    }
                }
            }
            Err(format!("E486: Pattern not found: {}", pat_str))
        }
    }
}

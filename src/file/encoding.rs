//! Line ending detection and encoding utilities.
//!
//! Handles detection of line ending conventions (LF, CRLF, CR),
//! normalization of line endings for internal storage, and UTF-8 BOM handling.

/// Detected line ending style.
///
/// Stored per-document so writes preserve the original convention.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    /// Unix-style: \n (default)
    #[default]
    Lf,
    /// Windows-style: \r\n
    CrLf,
    /// Classic Mac-style: \r (rare, but vi handles it)
    Cr,
}

impl LineEnding {
    /// Return the byte sequence for this line ending.
    pub fn as_str(&self) -> &'static str {
        match self {
            LineEnding::Lf => "\n",
            LineEnding::CrLf => "\r\n",
            LineEnding::Cr => "\r",
        }
    }

    /// Display name for the status line (e.g., "[dos]" for CrLf).
    pub fn display_name(&self) -> &'static str {
        match self {
            LineEnding::Lf => "",
            LineEnding::CrLf => "[dos]",
            LineEnding::Cr => "[mac]",
        }
    }
}

/// UTF-8 BOM bytes: EF BB BF
const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

/// Strip UTF-8 BOM from the beginning of raw bytes if present.
///
/// Returns (stripped_bytes, had_bom).
pub fn strip_bom(bytes: &[u8]) -> (&[u8], bool) {
    if bytes.starts_with(UTF8_BOM) {
        (&bytes[UTF8_BOM.len()..], true)
    } else {
        (bytes, false)
    }
}

/// Detect the predominant line ending in a string.
///
/// Counts occurrences of each style and returns the most common.
/// Falls back to Lf if the file has no line endings (single-line file).
pub fn detect_line_ending(text: &str) -> LineEnding {
    let mut lf_count = 0u32;
    let mut crlf_count = 0u32;
    let mut cr_count = 0u32;

    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                crlf_count += 1;
                i += 2;
                continue;
            }
            cr_count += 1;
        } else if bytes[i] == b'\n' {
            lf_count += 1;
        }
        i += 1;
    }

    // Tie-break rules (intentional):
    // - CRLF wins over both LF and CR on a tie (safer default for mixed files,
    //   avoids partial CRLF sequences being misidentified as lone CR).
    // - CR wins over LF on a tie (CR-only is uncommon; choosing it avoids
    //   silently discarding carriage returns in files that use them).
    // - LF is the fallback when no line endings are found or LF is dominant.
    if crlf_count >= lf_count && crlf_count >= cr_count && crlf_count > 0 {
        LineEnding::CrLf
    } else if cr_count >= lf_count && cr_count > 0 {
        LineEnding::Cr
    } else {
        LineEnding::Lf
    }
}

/// Normalize all line endings to \n for internal storage.
///
/// The buffer stores lines split on \n. This function converts
/// \r\n and \r to \n so `Buffer::from_string` works correctly.
pub fn normalize_line_endings(text: &str) -> String {
    // Two-pass: first \r\n -> \n, then remaining \r -> \n
    text.replace("\r\n", "\n").replace('\r', "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- LineEnding methods --

    #[test]
    fn test_line_ending_as_str_lf() {
        assert_eq!(LineEnding::Lf.as_str(), "\n");
    }

    #[test]
    fn test_line_ending_as_str_crlf() {
        assert_eq!(LineEnding::CrLf.as_str(), "\r\n");
    }

    #[test]
    fn test_line_ending_as_str_cr() {
        assert_eq!(LineEnding::Cr.as_str(), "\r");
    }

    #[test]
    fn test_line_ending_display_name_lf() {
        assert_eq!(LineEnding::Lf.display_name(), "");
    }

    #[test]
    fn test_line_ending_display_name_crlf() {
        assert_eq!(LineEnding::CrLf.display_name(), "[dos]");
    }

    #[test]
    fn test_line_ending_display_name_cr() {
        assert_eq!(LineEnding::Cr.display_name(), "[mac]");
    }

    #[test]
    fn test_line_ending_default() {
        assert_eq!(LineEnding::default(), LineEnding::Lf);
    }

    // -- detect_line_ending --

    #[test]
    fn test_detect_line_ending_lf() {
        assert_eq!(detect_line_ending("hello\nworld\n"), LineEnding::Lf);
    }

    #[test]
    fn test_detect_line_ending_crlf() {
        assert_eq!(detect_line_ending("hello\r\nworld\r\n"), LineEnding::CrLf);
    }

    #[test]
    fn test_detect_line_ending_cr() {
        assert_eq!(detect_line_ending("hello\rworld\r"), LineEnding::Cr);
    }

    #[test]
    fn test_detect_line_ending_mixed_crlf_majority() {
        // 2 CRLF vs 1 LF => CRLF wins
        assert_eq!(detect_line_ending("a\r\nb\r\nc\n"), LineEnding::CrLf);
    }

    #[test]
    fn test_detect_line_ending_mixed_lf_majority() {
        // 2 LF vs 1 CRLF => LF wins
        assert_eq!(detect_line_ending("a\nb\nc\r\n"), LineEnding::Lf);
    }

    #[test]
    fn test_detect_line_ending_empty() {
        assert_eq!(detect_line_ending(""), LineEnding::Lf);
    }

    #[test]
    fn test_detect_line_ending_no_line_endings() {
        assert_eq!(detect_line_ending("hello world"), LineEnding::Lf);
    }

    #[test]
    fn test_detect_line_ending_single_lf() {
        assert_eq!(detect_line_ending("hello\n"), LineEnding::Lf);
    }

    #[test]
    fn test_detect_line_ending_single_crlf() {
        assert_eq!(detect_line_ending("hello\r\n"), LineEnding::CrLf);
    }

    #[test]
    fn test_detect_line_ending_single_cr() {
        assert_eq!(detect_line_ending("hello\r"), LineEnding::Cr);
    }

    // -- normalize_line_endings --

    #[test]
    fn test_normalize_lf_unchanged() {
        assert_eq!(normalize_line_endings("hello\nworld\n"), "hello\nworld\n");
    }

    #[test]
    fn test_normalize_crlf_to_lf() {
        assert_eq!(
            normalize_line_endings("hello\r\nworld\r\n"),
            "hello\nworld\n"
        );
    }

    #[test]
    fn test_normalize_cr_to_lf() {
        assert_eq!(normalize_line_endings("hello\rworld\r"), "hello\nworld\n");
    }

    #[test]
    fn test_normalize_mixed() {
        assert_eq!(normalize_line_endings("a\r\nb\rc\n"), "a\nb\nc\n");
    }

    #[test]
    fn test_normalize_empty() {
        assert_eq!(normalize_line_endings(""), "");
    }

    #[test]
    fn test_normalize_no_line_endings() {
        assert_eq!(normalize_line_endings("hello"), "hello");
    }

    // -- strip_bom --

    #[test]
    fn test_strip_bom_present() {
        let bytes = &[0xEF, 0xBB, 0xBF, b'h', b'e', b'l', b'l', b'o'];
        let (stripped, had_bom) = strip_bom(bytes);
        assert!(had_bom);
        assert_eq!(stripped, b"hello");
    }

    #[test]
    fn test_strip_bom_absent() {
        let bytes = b"hello";
        let (stripped, had_bom) = strip_bom(bytes);
        assert!(!had_bom);
        assert_eq!(stripped, b"hello");
    }

    #[test]
    fn test_strip_bom_empty() {
        let bytes: &[u8] = &[];
        let (stripped, had_bom) = strip_bom(bytes);
        assert!(!had_bom);
        assert!(stripped.is_empty());
    }

    #[test]
    fn test_strip_bom_only_bom() {
        let bytes = &[0xEF, 0xBB, 0xBF];
        let (stripped, had_bom) = strip_bom(bytes);
        assert!(had_bom);
        assert!(stripped.is_empty());
    }

    #[test]
    fn test_strip_bom_partial_bom() {
        // Only 2 of 3 BOM bytes -- should not be stripped
        let bytes = &[0xEF, 0xBB, b'h'];
        let (stripped, had_bom) = strip_bom(bytes);
        assert!(!had_bom);
        assert_eq!(stripped, bytes);
    }
}

//! File reading with UTF-8 validation and line ending detection.
//!
//! Reads files from disk, strips BOM, validates UTF-8 encoding,
//! detects the line ending convention, and normalizes to LF for
//! internal buffer storage.

use std::fs;
use std::path::Path;

use crate::error::FileError;

use super::encoding::{self, LineEnding};

/// Result of reading a file, containing parsed content and metadata.
#[derive(Debug)]
pub struct ReadResult {
    /// The file content with normalized line endings (all \n).
    pub content: String,
    /// The detected line ending style of the original file.
    pub line_ending: LineEnding,
    /// Whether the file had a UTF-8 BOM.
    pub had_bom: bool,
}

/// Read a file from the given path into a `ReadResult`.
///
/// This function:
/// 1. Reads raw bytes from disk
/// 2. Strips UTF-8 BOM if present
/// 3. Validates UTF-8 encoding
/// 4. Detects the line ending convention
/// 5. Normalizes line endings to \n for internal storage
///
/// # Errors
///
/// Returns `FileError::NotFound` if the file does not exist.
/// Returns `FileError::PermissionDenied` if read permission is denied.
/// Returns `FileError::EncodingError` if the file is not valid UTF-8.
/// Returns `FileError::Io` for other I/O errors.
pub fn read_file(path: &Path) -> Result<ReadResult, FileError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) => {
            return match e.kind() {
                std::io::ErrorKind::NotFound => {
                    Err(FileError::NotFound(path.display().to_string()))
                }
                std::io::ErrorKind::PermissionDenied => {
                    Err(FileError::PermissionDenied(path.display().to_string()))
                }
                _ => Err(FileError::Io(e)),
            };
        }
    };

    // Strip BOM
    let (bytes, had_bom) = encoding::strip_bom(&bytes);

    // Validate UTF-8
    let text = match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(e) => {
            return Err(FileError::EncodingError(format!(
                "{}: invalid UTF-8 at byte offset {}",
                path.display(),
                e.valid_up_to()
            )));
        }
    };

    // Detect line ending before normalizing
    let line_ending = encoding::detect_line_ending(&text);

    // Normalize to \n for Buffer storage
    let content = encoding::normalize_line_endings(&text);

    Ok(ReadResult {
        content,
        line_ending,
        had_bom,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    /// Helper to create a temp file with given bytes.
    fn write_temp_file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("rvi_test_reader");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let mut file = fs::File::create(&path).unwrap();
        file.write_all(bytes).unwrap();
        path
    }

    /// Clean up temp files after tests.
    fn cleanup(path: &Path) {
        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_read_file_basic() {
        let path = write_temp_file("basic.txt", b"hello\nworld\n");
        let result = read_file(&path).unwrap();
        assert_eq!(result.content, "hello\nworld\n");
        assert_eq!(result.line_ending, LineEnding::Lf);
        assert!(!result.had_bom);
        cleanup(&path);
    }

    #[test]
    fn test_read_file_with_bom() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(b"hello\nworld\n");
        let path = write_temp_file("bom.txt", &bytes);
        let result = read_file(&path).unwrap();
        assert_eq!(result.content, "hello\nworld\n");
        assert_eq!(result.line_ending, LineEnding::Lf);
        assert!(result.had_bom);
        cleanup(&path);
    }

    #[test]
    fn test_read_file_crlf() {
        let path = write_temp_file("crlf.txt", b"hello\r\nworld\r\n");
        let result = read_file(&path).unwrap();
        assert_eq!(result.content, "hello\nworld\n");
        assert_eq!(result.line_ending, LineEnding::CrLf);
        assert!(!result.had_bom);
        cleanup(&path);
    }

    #[test]
    fn test_read_file_cr() {
        let path = write_temp_file("cr.txt", b"hello\rworld\r");
        let result = read_file(&path).unwrap();
        assert_eq!(result.content, "hello\nworld\n");
        assert_eq!(result.line_ending, LineEnding::Cr);
        assert!(!result.had_bom);
        cleanup(&path);
    }

    #[test]
    fn test_read_file_not_found() {
        let path = Path::new("/tmp/rvi_test_reader/nonexistent_file_12345.txt");
        let err = read_file(path).unwrap_err();
        assert!(matches!(err, FileError::NotFound(_)));
    }

    #[test]
    fn test_read_file_empty() {
        let path = write_temp_file("empty.txt", b"");
        let result = read_file(&path).unwrap();
        assert_eq!(result.content, "");
        assert_eq!(result.line_ending, LineEnding::Lf);
        assert!(!result.had_bom);
        cleanup(&path);
    }

    #[test]
    fn test_read_file_single_line_no_trailing_newline() {
        let path = write_temp_file("single.txt", b"hello");
        let result = read_file(&path).unwrap();
        assert_eq!(result.content, "hello");
        assert_eq!(result.line_ending, LineEnding::Lf);
        assert!(!result.had_bom);
        cleanup(&path);
    }

    #[test]
    fn test_read_file_unicode() {
        let content = "Hello\nWorld\n";
        let path = write_temp_file("unicode.txt", content.as_bytes());
        let result = read_file(&path).unwrap();
        assert_eq!(result.content, content);
        cleanup(&path);
    }

    #[test]
    fn test_read_file_unicode_cjk() {
        let content = "hello\n world\n";
        let path = write_temp_file("cjk.txt", content.as_bytes());
        let result = read_file(&path).unwrap();
        assert_eq!(result.content, content);
        cleanup(&path);
    }

    #[test]
    fn test_read_file_invalid_utf8() {
        // 0xFF is not valid UTF-8
        let path = write_temp_file("invalid.bin", &[0xFF, 0xFE, 0x00]);
        let err = read_file(&path).unwrap_err();
        assert!(matches!(err, FileError::EncodingError(_)));
        cleanup(&path);
    }

    #[test]
    fn test_read_file_bom_with_crlf() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(b"line1\r\nline2\r\n");
        let path = write_temp_file("bom_crlf.txt", &bytes);
        let result = read_file(&path).unwrap();
        assert_eq!(result.content, "line1\nline2\n");
        assert_eq!(result.line_ending, LineEnding::CrLf);
        assert!(result.had_bom);
        cleanup(&path);
    }
}

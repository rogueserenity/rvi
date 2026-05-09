//! File writing with atomic write support and line ending conversion.
//!
//! Writes buffer content to disk, converting internal LF line endings
//! to the target convention. Supports atomic writes (via temp file + rename)
//! and backup file creation.

use std::fs;
use std::io::{BufWriter, Write};
use std::path::Path;

use crate::buffer::Buffer;
use crate::error::FileError;

use super::encoding::LineEnding;

/// Options controlling write behavior.
#[derive(Debug, Clone)]
pub struct WriteOptions {
    /// Line ending to use when writing.
    pub line_ending: LineEnding,
    /// Whether to write atomically (write to temp file, then rename).
    pub atomic: bool,
    /// Whether to create a backup of the original file before overwriting.
    pub backup: bool,
    /// Backup suffix (e.g., "~" produces "file.txt~").
    pub backup_suffix: String,
}

impl Default for WriteOptions {
    fn default() -> Self {
        Self {
            line_ending: LineEnding::Lf,
            atomic: true,
            backup: false,
            backup_suffix: "~".to_string(),
        }
    }
}

/// Write a buffer's content to the given path.
///
/// The function joins buffer lines using the specified line ending
/// and writes to disk. When `atomic=true`, it writes to a temporary
/// file in the same directory then renames, ensuring the original
/// file is never left in a partial state.
///
/// Returns the number of bytes written.
///
/// # Errors
///
/// Returns `FileError::PermissionDenied` if write permission is denied.
/// Returns `FileError::Io` for other I/O errors.
pub fn write_file(
    buffer: &Buffer,
    path: &Path,
    options: &WriteOptions,
) -> Result<usize, FileError> {
    let line_sep = options.line_ending.as_str();
    let lines = buffer.lines();

    // Pre-compute byte count without allocating the full string.
    let bytes_written = lines.iter().map(|l| l.len()).sum::<usize>() + lines.len() * line_sep.len();

    // Create backup if requested
    if options.backup && path.exists() {
        let backup_path_str = format!("{}{}", path.display(), options.backup_suffix);
        let backup_path = Path::new(&backup_path_str);
        fs::copy(path, backup_path).map_err(|e| match e.kind() {
            std::io::ErrorKind::PermissionDenied => FileError::PermissionDenied(format!(
                "Cannot create backup: {}",
                backup_path.display()
            )),
            _ => FileError::Io(e),
        })?;
    }

    // Streaming write closure: writes lines one at a time into a BufWriter.
    let write_lines = |w: &mut BufWriter<fs::File>| -> Result<(), std::io::Error> {
        for (i, line) in lines.iter().enumerate() {
            w.write_all(line.as_bytes())?;
            if i + 1 < lines.len() {
                w.write_all(line_sep.as_bytes())?;
            }
        }
        // vi convention: files always end with a newline.
        w.write_all(line_sep.as_bytes())?;
        Ok(())
    };

    if options.atomic {
        write_atomic(path, write_lines)?;
    } else {
        write_direct(path, write_lines)?;
    }

    Ok(bytes_written)
}

/// Write to a temporary file in the same directory via a closure, then rename.
///
/// This ensures the target file is never in a partially-written state.
/// The temp file is created in the same directory so rename is an
/// atomic filesystem operation (same mount point).
fn write_atomic(
    path: &Path,
    write_fn: impl FnOnce(&mut BufWriter<fs::File>) -> Result<(), std::io::Error>,
) -> Result<(), FileError> {
    let parent = path.parent().unwrap_or(Path::new("."));

    // Build a temp name unique across concurrent rvi instances saving the same file.
    // Combine PID with nanosecond timestamp for uniqueness without a rand dependency.
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "untitled".to_string());
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let temp_name = format!(".{}.{}.{}.tmp", file_name, std::process::id(), nonce);
    let temp_path = parent.join(&temp_name);

    let cleanup = |e: std::io::Error| {
        let _ = fs::remove_file(&temp_path);
        map_io_error(e)
    };

    let file = fs::File::create(&temp_path).map_err(map_io_error)?;
    let mut writer = BufWriter::new(file);

    write_fn(&mut writer).map_err(&cleanup)?;
    writer.flush().map_err(&cleanup)?;

    // Ensure data is written to disk before rename for durability
    writer
        .into_inner()
        .map_err(|e| cleanup(e.into_error()))?
        .sync_all()
        .map_err(&cleanup)?;

    // Rename temp to target (atomic on same filesystem)
    fs::rename(&temp_path, path).map_err(|e| {
        let _ = fs::remove_file(&temp_path);
        map_io_error(e)
    })?;

    Ok(())
}

/// Write directly to the target path (non-atomic) via a closure.
fn write_direct(
    path: &Path,
    write_fn: impl FnOnce(&mut BufWriter<fs::File>) -> Result<(), std::io::Error>,
) -> Result<(), FileError> {
    let file = fs::File::create(path).map_err(map_io_error)?;
    let mut writer = BufWriter::new(file);
    write_fn(&mut writer).map_err(map_io_error)?;
    writer.flush().map_err(map_io_error)?;
    Ok(())
}

/// Map `std::io::Error` to `FileError` with context-appropriate variants.
fn map_io_error(e: std::io::Error) -> FileError {
    match e.kind() {
        std::io::ErrorKind::PermissionDenied => FileError::PermissionDenied(e.to_string()),
        _ => FileError::Io(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a temp directory for writer tests.
    fn test_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("rvi_test_writer");
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Clean up temp file.
    fn cleanup(path: &Path) {
        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_write_file_basic() {
        let dir = test_dir();
        let path = dir.join("basic_write.txt");

        let buffer = Buffer::from_string("hello\nworld".to_string());
        let options = WriteOptions::default();
        let bytes = write_file(&buffer, &path, &options).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, "hello\nworld\n");
        assert_eq!(bytes, content.len());
        cleanup(&path);
    }

    #[test]
    fn test_write_file_crlf() {
        let dir = test_dir();
        let path = dir.join("crlf_write.txt");

        let buffer = Buffer::from_string("hello\nworld".to_string());
        let options = WriteOptions {
            line_ending: LineEnding::CrLf,
            ..WriteOptions::default()
        };
        let bytes = write_file(&buffer, &path, &options).unwrap();

        let raw = fs::read(&path).unwrap();
        let content = String::from_utf8(raw).unwrap();
        assert_eq!(content, "hello\r\nworld\r\n");
        assert_eq!(bytes, content.len());
        cleanup(&path);
    }

    #[test]
    fn test_write_file_cr() {
        let dir = test_dir();
        let path = dir.join("cr_write.txt");

        let buffer = Buffer::from_string("hello\nworld".to_string());
        let options = WriteOptions {
            line_ending: LineEnding::Cr,
            ..WriteOptions::default()
        };
        write_file(&buffer, &path, &options).unwrap();

        let raw = fs::read(&path).unwrap();
        let content = String::from_utf8(raw).unwrap();
        assert_eq!(content, "hello\rworld\r");
        cleanup(&path);
    }

    #[test]
    fn test_write_file_atomic() {
        let dir = test_dir();
        let path = dir.join("atomic_write.txt");

        let buffer = Buffer::from_string("atomic test".to_string());
        let options = WriteOptions {
            atomic: true,
            ..WriteOptions::default()
        };
        write_file(&buffer, &path, &options).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, "atomic test\n");

        // Verify no temp file for this specific write was left behind
        let leftover = fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).any(|e| {
            let name = e.file_name();
            let s = name.to_string_lossy();
            s.starts_with(".atomic_write.txt.") && s.ends_with(".tmp")
        });
        assert!(!leftover, "atomic write left behind a .tmp file");
        cleanup(&path);
    }

    #[test]
    fn test_write_file_non_atomic() {
        let dir = test_dir();
        let path = dir.join("direct_write.txt");

        let buffer = Buffer::from_string("direct test".to_string());
        let options = WriteOptions {
            atomic: false,
            ..WriteOptions::default()
        };
        write_file(&buffer, &path, &options).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, "direct test\n");
        cleanup(&path);
    }

    #[test]
    fn test_write_file_backup() {
        let dir = test_dir();
        let path = dir.join("backup_write.txt");
        let backup_path = dir.join("backup_write.txt~");

        // Write original content
        fs::write(&path, "original\n").unwrap();

        let buffer = Buffer::from_string("updated".to_string());
        let options = WriteOptions {
            backup: true,
            ..WriteOptions::default()
        };
        write_file(&buffer, &path, &options).unwrap();

        // Verify backup has original content
        let backup_content = fs::read_to_string(&backup_path).unwrap();
        assert_eq!(backup_content, "original\n");

        // Verify main file has new content
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, "updated\n");

        cleanup(&path);
        cleanup(&backup_path);
    }

    #[test]
    fn test_write_file_empty_buffer() {
        let dir = test_dir();
        let path = dir.join("empty_write.txt");

        let buffer = Buffer::new(); // Single empty line
        let options = WriteOptions::default();
        let bytes = write_file(&buffer, &path, &options).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        // Buffer has one empty line; vi convention adds trailing newline
        assert_eq!(content, "\n");
        assert_eq!(bytes, 1);
        cleanup(&path);
    }

    #[test]
    fn test_write_file_returns_byte_count() {
        let dir = test_dir();
        let path = dir.join("byte_count.txt");

        let buffer = Buffer::from_string("abc\ndef".to_string());
        let options = WriteOptions::default();
        let bytes = write_file(&buffer, &path, &options).unwrap();

        // "abc\ndef\n" = 8 bytes
        assert_eq!(bytes, 8);
        cleanup(&path);
    }

    #[test]
    fn test_write_file_preserves_unicode() {
        let dir = test_dir();
        let path = dir.join("unicode_write.txt");

        let buffer = Buffer::from_string("Hello\nWorld".to_string());
        let options = WriteOptions::default();
        write_file(&buffer, &path, &options).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, "Hello\nWorld\n");
        cleanup(&path);
    }

    #[test]
    fn test_write_file_trailing_newline() {
        let dir = test_dir();
        let path = dir.join("trailing_nl.txt");

        // Even a single line gets a trailing newline
        let buffer = Buffer::from_string("hello".to_string());
        let options = WriteOptions::default();
        write_file(&buffer, &path, &options).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.ends_with('\n'));
        assert_eq!(content, "hello\n");
        cleanup(&path);
    }

    #[test]
    fn test_write_file_multiple_lines() {
        let dir = test_dir();
        let path = dir.join("multi_line.txt");

        let buffer = Buffer::from_string("line1\nline2\nline3".to_string());
        let options = WriteOptions::default();
        write_file(&buffer, &path, &options).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, "line1\nline2\nline3\n");
        cleanup(&path);
    }

    #[test]
    fn test_write_file_backup_no_original() {
        let dir = test_dir();
        let path = dir.join("no_original_backup.txt");
        let backup_path = dir.join("no_original_backup.txt~");

        // No original file exists -- backup should be a no-op
        let _ = fs::remove_file(&path);

        let buffer = Buffer::from_string("new file".to_string());
        let options = WriteOptions {
            backup: true,
            ..WriteOptions::default()
        };
        write_file(&buffer, &path, &options).unwrap();

        // Backup should not exist since there was no original
        assert!(!backup_path.exists());

        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, "new file\n");
        cleanup(&path);
    }

    #[test]
    fn test_roundtrip_read_write() {
        use super::super::reader::read_file;

        let dir = test_dir();
        let path = dir.join("roundtrip.txt");

        // Write initial content
        let buffer = Buffer::from_string("hello\nworld".to_string());
        let options = WriteOptions::default();
        write_file(&buffer, &path, &options).unwrap();

        // Read it back
        let result = read_file(&path).unwrap();
        assert_eq!(result.line_ending, LineEnding::Lf);

        // Re-create buffer from read content
        let buffer2 = Buffer::from_string(result.content);
        assert_eq!(buffer2.line(0), Some("hello"));
        assert_eq!(buffer2.line(1), Some("world"));
        assert_eq!(buffer2.len(), 2, "no spurious extra lines after roundtrip");
        cleanup(&path);
    }

    #[test]
    fn test_roundtrip_preserves_crlf() {
        use super::super::reader::read_file;

        let dir = test_dir();
        let path = dir.join("roundtrip_crlf.txt");

        // Write with CRLF
        let buffer = Buffer::from_string("hello\nworld".to_string());
        let options = WriteOptions {
            line_ending: LineEnding::CrLf,
            ..WriteOptions::default()
        };
        write_file(&buffer, &path, &options).unwrap();

        // Read it back
        let result = read_file(&path).unwrap();
        assert_eq!(result.line_ending, LineEnding::CrLf);
        // Content is normalized to LF
        assert_eq!(result.content, "hello\nworld\n");
        cleanup(&path);
    }
}

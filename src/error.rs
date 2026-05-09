//! Error types for the editor.
//!
//! This module defines all error types used throughout the editor,
//! using `thiserror` for derive macros and proper error chaining.

use thiserror::Error;

/// Main error type for the editor.
#[derive(Debug, Error)]
pub enum Error {
    /// Terminal-related errors
    #[error(transparent)]
    Terminal(#[from] TerminalError),

    /// Buffer-related errors
    #[error(transparent)]
    Buffer(#[from] BufferError),

    /// Render-related errors
    #[error(transparent)]
    Render(#[from] RenderError),

    /// Command-related errors
    #[error(transparent)]
    Command(#[from] CommandError),

    /// File-related errors
    #[error(transparent)]
    File(#[from] FileError),

    /// Search-related errors
    #[error(transparent)]
    Search(#[from] SearchError),

    /// Settings-related errors
    #[error(transparent)]
    Settings(#[from] SettingsError),

    /// Undo/redo errors
    #[error(transparent)]
    Undo(#[from] UndoError),

    /// Register-related errors
    #[error(transparent)]
    Register(#[from] RegisterError),

    /// I/O errors
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Result type alias for editor operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Terminal-related errors.
#[derive(Debug, Error)]
pub enum TerminalError {
    /// Failed to enable raw mode
    #[error("Failed to enable raw mode: {0}")]
    RawModeEnable(std::io::Error),

    /// Failed to get terminal size
    #[error("Failed to get terminal size: {0}")]
    GetSize(std::io::Error),

    /// Failed to read input
    #[error("Failed to read input: {0}")]
    ReadInput(std::io::Error),

    /// Failed to write output
    #[error("Failed to write output: {0}")]
    WriteOutput(std::io::Error),
}

/// Buffer-related errors.
#[derive(Debug, Error)]
pub enum BufferError {
    /// Row index out of bounds
    #[error("Row {row} out of bounds (buffer has {len} lines)")]
    RowOutOfBounds {
        /// The requested row
        row: usize,
        /// The buffer length
        len: usize,
    },

    /// Column (byte offset) out of bounds
    #[error("Column {col} out of bounds (line has {len} bytes)")]
    ColOutOfBounds {
        /// The requested column
        col: usize,
        /// The line length
        len: usize,
    },

    /// Invalid byte offset (not at character boundary)
    #[error("Column {col} is not a valid UTF-8 byte boundary")]
    InvalidByteOffset {
        /// The invalid column
        col: usize,
    },

    /// Invalid cursor position
    #[error("Invalid cursor position: row {row}, col {col}")]
    InvalidCursor {
        /// The row
        row: usize,
        /// The column
        col: usize,
    },
}

/// Render-related errors.
#[derive(Debug, Error)]
pub enum RenderError {
    /// Failed to render to terminal
    #[error("Failed to render: {0}")]
    RenderFailed(std::io::Error),

    /// Failed to position cursor
    #[error("Failed to position cursor: {0}")]
    CursorPosition(std::io::Error),
}

/// Command-related errors (Phase 2+).
#[derive(Debug, Error)]
pub enum CommandError {
    /// Unknown command
    #[error("Unknown command: {0}")]
    Unknown(String),

    /// Invalid motion
    #[error("Invalid motion")]
    InvalidMotion,

    /// Invalid count
    #[error("Invalid count")]
    InvalidCount,

    /// Invalid text object
    #[error("Invalid text object")]
    InvalidTextObject,

    /// Command cancelled
    #[error("Command cancelled")]
    Cancelled,
}

/// File-related errors (Phase 3+).
#[derive(Debug, Error)]
pub enum FileError {
    /// File not found
    #[error("File not found: {0}")]
    NotFound(String),

    /// Permission denied
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    /// I/O error during file operation
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// File is read-only
    #[error("File is read-only: {0}")]
    ReadOnly(String),

    /// File modified since last read
    #[error("File modified since last read: {0}")]
    Modified(String),

    /// Encoding error (not valid UTF-8)
    #[error("Encoding error: {0}")]
    EncodingError(String),
}

/// Search-related errors (Phase 2+).
#[derive(Debug, Error)]
pub enum SearchError {
    /// Invalid search pattern
    #[error("Invalid pattern: {0}")]
    InvalidPattern(String),

    /// Pattern not found
    #[error("Pattern not found")]
    NotFound,

    /// Regex compilation error
    #[error("Regex error: {0}")]
    RegexError(String),

    /// Search wrapped around
    #[error("Search wrapped")]
    Wrapped,

    /// Substitute range out of bounds
    #[error("Invalid range")]
    InvalidRange,
}

/// Settings-related errors (Phase 4+).
#[derive(Debug, Error)]
pub enum SettingsError {
    /// Invalid option name
    #[error("Invalid option: {0}")]
    InvalidOption(String),

    /// Invalid option value
    #[error("Invalid value for option '{option}': {value}")]
    InvalidValue {
        /// The option name
        option: String,
        /// The invalid value
        value: String,
    },

    /// Option is read-only
    #[error("Option is read-only: {0}")]
    ReadOnly(String),
}

/// Undo/redo errors (Phase 2+).
#[derive(Debug, Error)]
pub enum UndoError {
    /// Nothing to undo
    #[error("Nothing to undo")]
    NothingToUndo,

    /// Nothing to redo
    #[error("Nothing to redo")]
    NothingToRedo,

    /// Undo history corrupted
    #[error("Undo history corrupted")]
    Corrupted,
}

/// Register-related errors.
#[derive(Debug, Error)]
pub enum RegisterError {
    /// Invalid register character
    #[error("Invalid register: '{0}'")]
    InvalidRegister(char),

    /// Register is empty
    #[error("Register '{0}' is empty")]
    EmptyRegister(char),

    /// Cannot write to read-only register
    #[error("Cannot write to read-only register: '{0}'")]
    ReadOnlyRegister(char),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_error_display() {
        let err = BufferError::RowOutOfBounds { row: 10, len: 5 };
        assert_eq!(err.to_string(), "Row 10 out of bounds (buffer has 5 lines)");
    }

    #[test]
    fn test_error_from_buffer_error() {
        let buf_err = BufferError::RowOutOfBounds { row: 10, len: 5 };
        let err: Error = buf_err.into();
        assert!(matches!(err, Error::Buffer(_)));
    }

    #[test]
    fn test_terminal_error_display() {
        let io_err = std::io::Error::other("test");
        let err = TerminalError::RawModeEnable(io_err);
        assert!(err.to_string().contains("Failed to enable raw mode"));
    }

    #[test]
    fn test_command_error_display() {
        let err = CommandError::Unknown("foo".to_string());
        assert_eq!(err.to_string(), "Unknown command: foo");

        let err = CommandError::InvalidMotion;
        assert_eq!(err.to_string(), "Invalid motion");
    }

    #[test]
    fn test_file_error_display() {
        let err = FileError::NotFound("/tmp/test.txt".to_string());
        assert_eq!(err.to_string(), "File not found: /tmp/test.txt");

        let err = FileError::PermissionDenied("/etc/passwd".to_string());
        assert_eq!(err.to_string(), "Permission denied: /etc/passwd");
    }

    #[test]
    fn test_file_error_encoding_display() {
        let err = FileError::EncodingError("test.bin: invalid UTF-8 at byte offset 5".to_string());
        assert_eq!(
            err.to_string(),
            "Encoding error: test.bin: invalid UTF-8 at byte offset 5"
        );
    }

    #[test]
    fn test_search_error_display() {
        let err = SearchError::InvalidPattern("(unclosed".to_string());
        assert_eq!(err.to_string(), "Invalid pattern: (unclosed");

        let err = SearchError::NotFound;
        assert_eq!(err.to_string(), "Pattern not found");

        let err = SearchError::InvalidRange;
        assert_eq!(err.to_string(), "Invalid range");
    }

    #[test]
    fn test_settings_error_display() {
        let err = SettingsError::InvalidOption("foo".to_string());
        assert_eq!(err.to_string(), "Invalid option: foo");

        let err = SettingsError::InvalidValue {
            option: "tabstop".to_string(),
            value: "abc".to_string(),
        };
        assert_eq!(err.to_string(), "Invalid value for option 'tabstop': abc");
    }

    #[test]
    fn test_undo_error_display() {
        let err = UndoError::NothingToUndo;
        assert_eq!(err.to_string(), "Nothing to undo");

        let err = UndoError::NothingToRedo;
        assert_eq!(err.to_string(), "Nothing to redo");
    }

    #[test]
    fn test_register_error_display() {
        let err = RegisterError::InvalidRegister('!');
        assert_eq!(err.to_string(), "Invalid register: '!'");

        let err = RegisterError::EmptyRegister('a');
        assert_eq!(err.to_string(), "Register 'a' is empty");

        let err = RegisterError::ReadOnlyRegister(':');
        assert_eq!(err.to_string(), "Cannot write to read-only register: ':'");
    }

    #[test]
    fn test_error_from_command_error() {
        let cmd_err = CommandError::InvalidMotion;
        let err: Error = cmd_err.into();
        assert!(matches!(err, Error::Command(_)));
    }

    #[test]
    fn test_error_from_file_error() {
        let file_err = FileError::NotFound("test.txt".to_string());
        let err: Error = file_err.into();
        assert!(matches!(err, Error::File(_)));
    }

    #[test]
    fn test_error_from_search_error() {
        let search_err = SearchError::NotFound;
        let err: Error = search_err.into();
        assert!(matches!(err, Error::Search(_)));
    }

    #[test]
    fn test_error_from_settings_error() {
        let settings_err = SettingsError::InvalidOption("foo".to_string());
        let err: Error = settings_err.into();
        assert!(matches!(err, Error::Settings(_)));
    }

    #[test]
    fn test_error_from_undo_error() {
        let undo_err = UndoError::NothingToUndo;
        let err: Error = undo_err.into();
        assert!(matches!(err, Error::Undo(_)));
    }

    #[test]
    fn test_error_from_register_error() {
        let reg_err = RegisterError::InvalidRegister('!');
        let err: Error = reg_err.into();
        assert!(matches!(err, Error::Register(_)));
    }
}

//! Terminal size detection.
//!
//! Provides functionality to query the current terminal dimensions.

use crate::error::TerminalError;

/// Represents terminal dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    /// Number of columns (width)
    pub cols: u16,
    /// Number of rows (height)
    pub rows: u16,
}

impl Size {
    /// Create a new Size with the given dimensions.
    pub fn new(cols: u16, rows: u16) -> Self {
        Self { cols, rows }
    }
}

/// Get the current terminal size.
///
/// # Errors
///
/// Returns an error if the terminal size cannot be determined.
pub fn terminal_size() -> Result<Size, TerminalError> {
    let (cols, rows) = termion::terminal_size().map_err(TerminalError::GetSize)?;

    Ok(Size { cols, rows })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_size_new() {
        let size = Size::new(80, 24);
        assert_eq!(size.cols, 80);
        assert_eq!(size.rows, 24);
    }
}

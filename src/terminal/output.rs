//! Terminal output operations.
//!
//! Provides functions for cursor movement, screen clearing, and other
//! terminal output operations.

use std::io::{self, stdout, Write};
use termion::{clear, cursor};

/// Wrapper for terminal output operations.
///
/// Provides a convenient interface for writing to the terminal with
/// cursor control and screen manipulation. Generic over the output
/// writer to allow testing with in-memory buffers.
pub struct TerminalOutput<W: Write = io::Stdout> {
    writer: W,
}

impl TerminalOutput<io::Stdout> {
    /// Create a new TerminalOutput instance writing to stdout.
    pub fn new() -> Self {
        Self { writer: stdout() }
    }
}

impl<W: Write> TerminalOutput<W> {
    /// Create a new TerminalOutput instance with a custom writer.
    pub fn with_writer(writer: W) -> Self {
        Self { writer }
    }

    /// Move cursor to the specified position (1-indexed).
    ///
    /// # Arguments
    ///
    /// * `x` - Column position (1-indexed)
    /// * `y` - Row position (1-indexed)
    pub fn goto(&mut self, x: u16, y: u16) -> io::Result<()> {
        write!(self.writer, "{}", cursor::Goto(x, y))
    }

    /// Move cursor up by n lines.
    pub fn move_up(&mut self, n: u16) -> io::Result<()> {
        write!(self.writer, "{}", cursor::Up(n))
    }

    /// Move cursor down by n lines.
    pub fn move_down(&mut self, n: u16) -> io::Result<()> {
        write!(self.writer, "{}", cursor::Down(n))
    }

    /// Move cursor left by n columns.
    pub fn move_left(&mut self, n: u16) -> io::Result<()> {
        write!(self.writer, "{}", cursor::Left(n))
    }

    /// Move cursor right by n columns.
    pub fn move_right(&mut self, n: u16) -> io::Result<()> {
        write!(self.writer, "{}", cursor::Right(n))
    }

    /// Clear the entire screen.
    pub fn clear(&mut self) -> io::Result<()> {
        write!(self.writer, "{}", clear::All)
    }

    /// Clear from cursor to end of screen.
    pub fn clear_to_end_of_screen(&mut self) -> io::Result<()> {
        write!(self.writer, "{}", clear::AfterCursor)
    }

    /// Clear from cursor to beginning of screen.
    pub fn clear_to_beginning_of_screen(&mut self) -> io::Result<()> {
        write!(self.writer, "{}", clear::BeforeCursor)
    }

    /// Clear the current line.
    pub fn clear_line(&mut self) -> io::Result<()> {
        write!(self.writer, "{}", clear::CurrentLine)
    }

    /// Clear from cursor to end of line.
    pub fn clear_to_end_of_line(&mut self) -> io::Result<()> {
        write!(self.writer, "{}", clear::UntilNewline)
    }

    /// Hide the cursor.
    pub fn hide_cursor(&mut self) -> io::Result<()> {
        write!(self.writer, "{}", cursor::Hide)
    }

    /// Show the cursor.
    pub fn show_cursor(&mut self) -> io::Result<()> {
        write!(self.writer, "{}", cursor::Show)
    }

    /// Write a string to the terminal.
    pub fn write(&mut self, s: &str) -> io::Result<()> {
        write!(self.writer, "{}", s)
    }

    /// Write a byte to the terminal.
    pub fn write_byte(&mut self, byte: u8) -> io::Result<()> {
        self.writer.write_all(&[byte])
    }

    /// Flush all buffered output to the terminal.
    pub fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }

    /// Get a mutable reference to the underlying writer for direct writes.
    pub fn writer(&mut self) -> &mut W {
        &mut self.writer
    }
}

impl Default for TerminalOutput<io::Stdout> {
    fn default() -> Self {
        Self::new()
    }
}

impl<W: Write> Write for TerminalOutput<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.writer.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminal_output_new() {
        let _output = TerminalOutput::new();
        // Just test that it can be created
    }
}

//! Raw terminal mode management.
//!
//! Provides functionality to enable and disable raw mode, which disables
//! echo, canonical mode, and other terminal features needed for interactive editing.

use std::io::{stdout, Stdout};
use termion::raw::{IntoRawMode, RawTerminal};

use crate::error::TerminalError;

/// A handle to the terminal in raw mode.
///
/// When dropped, automatically restores the terminal to normal mode.
pub struct RawMode {
    /// The raw terminal handle
    _stdout: RawTerminal<Stdout>,
}

impl RawMode {
    /// Enable raw mode for the terminal.
    ///
    /// # Errors
    ///
    /// Returns an error if raw mode cannot be enabled.
    pub fn enable() -> Result<Self, TerminalError> {
        let raw_stdout = stdout()
            .into_raw_mode()
            .map_err(TerminalError::RawModeEnable)?;

        Ok(Self {
            _stdout: raw_stdout,
        })
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        // RawTerminal automatically restores normal mode when dropped
        // No explicit action needed, but we keep this impl for documentation
    }
}

#[cfg(test)]
mod tests {
    // Note: Raw mode tests are difficult to run in unit tests
    // as they require an actual terminal. Manual testing is recommended.
}

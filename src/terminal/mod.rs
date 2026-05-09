//! Terminal I/O module for handling raw mode, input, output, and terminal size.
//!
//! This module provides an abstraction over termion for terminal operations.

pub mod input;
pub mod output;
pub mod raw_mode;
pub mod signals;
pub mod size;

pub use input::{read_key, Key};
pub use output::TerminalOutput;
pub use raw_mode::RawMode;
pub use signals::{install_sigwinch_handler, sigwinch_received};
pub use size::{terminal_size, Size};

//! File operations module for reading and writing files.
//!
//! This module handles:
//! - Reading files with UTF-8 validation and BOM handling
//! - Writing files with atomic write support
//! - Line ending detection and conversion

pub mod encoding;
pub mod reader;
pub mod writer;

pub use encoding::LineEnding;
pub use reader::{read_file, ReadResult};
pub use writer::{write_file, WriteOptions};

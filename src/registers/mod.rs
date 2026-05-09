//! Vi-style register management.
//!
//! This module implements the register system used by vi for storing and
//! retrieving text during yank, delete, and put operations.
//!
//! # Register Types
//!
//! Vi has several categories of registers:
//!
//! | Register | Name | Description |
//! |----------|------|-------------|
//! | `""` | Unnamed | Default for all operations |
//! | `"a`-`"z` | Named | User-controlled storage |
//! | `"A`-`"Z` | Named (append) | Append to corresponding lowercase |
//! | `"0` | Yank | Most recent yank |
//! | `"1`-`"9` | Numbered | Delete history (1 = most recent) |
//! | `"-` | Small delete | Deletes less than one line |
//!
//! # Content Types
//!
//! Registers track whether content was captured characterwise or linewise,
//! which affects how put commands (`p`, `P`) insert the text:
//!
//! - **Characterwise**: Insert inline at cursor position
//! - **Linewise**: Insert on new line above/below current line
//!
//! # Example
//!
//! ```ignore
//! use rvi::registers::{Registers, RegisterId, RegisterContent, ContentType};
//!
//! let mut regs = Registers::new();
//!
//! // Yank some text
//! let content = RegisterContent::new("hello".to_string(), ContentType::Characterwise);
//! regs.yank(None, content);
//!
//! // Retrieve from unnamed register
//! if let Some(content) = regs.get(None) {
//!     println!("Yanked: {}", content.text());
//! }
//!
//! // Delete to named register "a
//! let del = RegisterContent::linewise("deleted line\n".to_string());
//! regs.delete(RegisterId::parse('a'), Some('a'), del);
//! ```

mod operations;
mod storage;
mod types;

pub use operations::RegisterStorage;
pub use types::{is_append_register, ContentType, RegisterContent, RegisterId, RegisterKind};

/// Main register manager.
///
/// Provides a high-level API for register operations. Wraps `RegisterStorage`
/// with convenience methods that match common vi operations.
#[derive(Debug, Default)]
pub struct Registers {
    /// Internal storage.
    storage: RegisterStorage,
}

impl Registers {
    /// Create new empty register storage.
    pub fn new() -> Self {
        Self {
            storage: RegisterStorage::new(),
        }
    }

    /// Store content from a yank operation.
    ///
    /// - Stores to the specified register (or unnamed if None)
    /// - Always stores to "0 (yank register)
    /// - Does NOT rotate numbered registers
    ///
    /// # Arguments
    ///
    /// * `register` - Target register, or None for unnamed
    /// * `content` - The yanked content
    pub fn yank(&mut self, register: Option<RegisterId>, content: RegisterContent) {
        self.storage.store_yank(register, content);
    }

    /// Store content from a delete operation.
    ///
    /// - Stores to the specified register (or unnamed if None)
    /// - Rotates numbered registers (1-9) for linewise deletes
    /// - Stores to small delete register for small (no newline) deletes
    ///
    /// # Arguments
    ///
    /// * `register` - Target register, or None for unnamed
    /// * `original_char` - The original register character (for A-Z append detection)
    /// * `content` - The deleted content
    pub fn delete(
        &mut self,
        register: Option<RegisterId>,
        original_char: Option<char>,
        content: RegisterContent,
    ) {
        self.storage.store_delete(register, original_char, content);
    }

    /// Store content to a register.
    ///
    /// This is the general-purpose store method. For most cases, prefer
    /// using `yank()` or `delete()` which handle the specific semantics.
    ///
    /// # Arguments
    ///
    /// * `register` - Target register, or None for unnamed
    /// * `original_char` - The original register character (for A-Z append detection)
    /// * `content` - The content to store
    /// * `is_delete` - Whether this is a delete operation (affects rotation)
    pub fn store(
        &mut self,
        register: Option<RegisterId>,
        original_char: Option<char>,
        content: RegisterContent,
        is_delete: bool,
    ) {
        self.storage
            .store(register, original_char, content, is_delete);
    }

    /// Retrieve content from a register.
    ///
    /// Returns None if the register is empty.
    /// For `Clipboard`/`Primary` registers use `get_owned()` instead.
    ///
    /// # Arguments
    ///
    /// * `register` - The register to read, or None for unnamed
    pub fn get(&self, register: Option<RegisterId>) -> Option<&RegisterContent> {
        self.storage.get(register)
    }

    /// Retrieve owned content from a register.
    ///
    /// Like `get()` but returns an owned value, and handles `Clipboard`/`Primary`
    /// by reading from the OS clipboard.
    pub fn get_owned(&self, register: Option<RegisterId>) -> Option<RegisterContent> {
        self.storage.get_owned(register)
    }

    /// Check if a register has content.
    pub fn has(&self, register: Option<RegisterId>) -> bool {
        self.storage.has(register)
    }

    /// Store to the small delete register ("-).
    ///
    /// Called for deletes that are less than one line.
    pub fn store_small_delete(&mut self, content: RegisterContent) {
        self.storage.store_small_delete(content);
    }

    /// Clear a specific register.
    pub fn clear(&mut self, register: Option<RegisterId>) {
        self.storage.clear(register);
    }

    /// Clear all registers.
    pub fn clear_all(&mut self) {
        self.storage.clear_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to create characterwise content
    fn char_content(s: &str) -> RegisterContent {
        RegisterContent::new(s.to_string(), ContentType::Characterwise)
    }

    // Helper to create linewise content
    fn line_content(s: &str) -> RegisterContent {
        RegisterContent::new(s.to_string(), ContentType::Linewise)
    }

    // Integration tests for common workflows

    #[test]
    fn test_yank_and_put_workflow() {
        let mut regs = Registers::new();

        // yy - yank a line
        regs.yank(None, line_content("hello world\n"));

        // p - put from unnamed register
        let content = regs.get(None);
        assert!(content.is_some());
        assert_eq!(content.unwrap().text(), "hello world\n");
        assert!(content.unwrap().is_linewise());
    }

    #[test]
    fn test_delete_and_put_workflow() {
        let mut regs = Registers::new();

        // dd - delete a line
        regs.delete(None, None, line_content("deleted line\n"));

        // p - put from unnamed register
        let content = regs.get(None);
        assert!(content.is_some());
        assert_eq!(content.unwrap().text(), "deleted line\n");
        assert!(content.unwrap().is_linewise());

        // "1p - also available in "1
        let content = regs.get(RegisterId::parse('1'));
        assert!(content.is_some());
        assert_eq!(content.unwrap().text(), "deleted line\n");
    }

    #[test]
    fn test_dw_characterwise_workflow() {
        let mut regs = Registers::new();

        // dw - delete word (characterwise)
        regs.delete(None, None, char_content("word"));

        // p - put inline
        let content = regs.get(None);
        assert!(content.is_some());
        assert_eq!(content.unwrap().text(), "word");
        assert!(content.unwrap().is_characterwise());

        // Small delete should be in "-
        let small = regs.get(RegisterId::parse('-'));
        assert!(small.is_some());
        assert_eq!(small.unwrap().text(), "word");
    }

    #[test]
    fn test_named_register_workflow() {
        let mut regs = Registers::new();

        // "ayy - yank to register a
        regs.yank(RegisterId::parse('a'), line_content("line one\n"));

        // "ap - put from register a
        let content = regs.get(RegisterId::parse('a'));
        assert!(content.is_some());
        assert_eq!(content.unwrap().text(), "line one\n");

        // "0 must NOT be updated when an explicit named register is used (POSIX vi).
        let yank = regs.get(RegisterId::parse('0'));
        assert!(yank.is_none());
    }

    #[test]
    fn test_append_to_named_register_workflow() {
        let mut regs = Registers::new();

        // "ayy - yank first line
        regs.yank(RegisterId::parse('a'), line_content("first\n"));

        // "Ayy - append second line (note uppercase A)
        regs.delete(RegisterId::parse('A'), Some('A'), line_content("second\n"));

        // "ap - should contain both lines
        let content = regs.get(RegisterId::parse('a'));
        assert!(content.is_some());
        assert_eq!(content.unwrap().text(), "first\nsecond\n");
    }

    #[test]
    fn test_yank_preserves_after_delete() {
        let mut regs = Registers::new();

        // yy - yank something
        regs.yank(None, line_content("yanked\n"));

        // dd - delete something else
        regs.delete(None, None, line_content("deleted\n"));

        // Unnamed has delete
        assert_eq!(regs.get(None).unwrap().text(), "deleted\n");

        // "0 still has yank
        assert_eq!(regs.get(RegisterId::parse('0')).unwrap().text(), "yanked\n");

        // "1 has delete
        assert_eq!(
            regs.get(RegisterId::parse('1')).unwrap().text(),
            "deleted\n"
        );
    }

    #[test]
    fn test_numbered_register_history() {
        let mut regs = Registers::new();

        // Delete three lines
        regs.delete(None, None, line_content("first\n"));
        regs.delete(None, None, line_content("second\n"));
        regs.delete(None, None, line_content("third\n"));

        // "1 has most recent
        assert_eq!(regs.get(RegisterId::parse('1')).unwrap().text(), "third\n");
        // "2 has previous
        assert_eq!(regs.get(RegisterId::parse('2')).unwrap().text(), "second\n");
        // "3 has oldest
        assert_eq!(regs.get(RegisterId::parse('3')).unwrap().text(), "first\n");
    }

    #[test]
    fn test_small_delete_does_not_rotate() {
        let mut regs = Registers::new();

        // x - delete single character
        regs.delete(None, None, char_content("x"));

        // Should be in "- not "1
        assert!(regs.get(RegisterId::parse('-')).is_some());
        assert!(regs.get(RegisterId::parse('1')).is_none());
    }

    #[test]
    fn test_register_id_parse_all_valid() {
        // Test all valid register characters
        for c in 'a'..='z' {
            assert!(RegisterId::parse(c).is_some());
        }
        for c in 'A'..='Z' {
            assert!(RegisterId::parse(c).is_some());
        }
        for c in '0'..='9' {
            assert!(RegisterId::parse(c).is_some());
        }
        assert!(RegisterId::parse('-').is_some());
        assert!(RegisterId::parse('"').is_some());
    }

    #[test]
    fn test_register_id_parse_invalid() {
        assert!(RegisterId::parse('!').is_none());
        assert!(RegisterId::parse('@').is_none());
        assert!(RegisterId::parse(' ').is_none());
    }

    #[test]
    fn test_clear_register() {
        let mut regs = Registers::new();

        regs.yank(None, char_content("test"));
        assert!(regs.has(None));

        regs.clear(None);
        assert!(!regs.has(None));
    }

    #[test]
    fn test_clear_all_registers() {
        let mut regs = Registers::new();

        regs.yank(None, char_content("unnamed"));
        regs.yank(RegisterId::parse('a'), char_content("named"));
        regs.delete(None, None, line_content("deleted\n"));

        regs.clear_all();

        assert!(!regs.has(None));
        assert!(!regs.has(RegisterId::parse('a')));
        assert!(!regs.has(RegisterId::parse('0')));
        assert!(!regs.has(RegisterId::parse('1')));
    }
}

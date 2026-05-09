//! Register operations.
//!
//! This module provides the core operations for storing and retrieving
//! register content, including the rotation logic for numbered registers.

use super::storage::{NamedStorage, NumberedStorage};
use super::types::{is_append_register, ContentType, RegisterContent, RegisterId, RegisterKind};

/// Main register storage managing all register types.
///
/// Handles the following register categories:
/// - Unnamed register (`""`): Default for all operations
/// - Named registers (`"a`-`"z`): User-controlled storage
/// - Numbered registers (`"1`-`"9`): Delete history with rotation
/// - Yank register (`"0`): Most recent yank
/// - Small delete register (`"-`): Deletes less than one line
#[derive(Debug, Default)]
pub struct RegisterStorage {
    /// The unnamed register ("").
    pub(crate) unnamed: Option<RegisterContent>,
    /// Named registers (a-z).
    pub(crate) named: NamedStorage,
    /// Numbered delete history (1-9).
    pub(crate) numbered: NumberedStorage,
    /// Yank register (0).
    pub(crate) yank: Option<RegisterContent>,
    /// Small delete register (-).
    pub(crate) small_delete: Option<RegisterContent>,
}

impl RegisterStorage {
    /// Create new empty register storage.
    pub fn new() -> Self {
        Self::default()
    }

    /// Store content to a register.
    ///
    /// # Arguments
    ///
    /// * `register` - The target register (None means unnamed)
    /// * `original_char` - The original register character (to detect A-Z for append)
    /// * `content` - The content to store
    /// * `is_delete` - True if this is a delete operation, false for yank
    ///
    /// # Behavior
    ///
    /// - Always writes to unnamed register
    /// - For delete operations on unnamed register, rotates numbered registers
    /// - For named registers, uppercase (A-Z) appends instead of replacing
    /// - For yank operations, also writes to "0
    pub fn store(
        &mut self,
        register: Option<RegisterId>,
        original_char: Option<char>,
        content: RegisterContent,
        is_delete: bool,
    ) {
        // Black hole register discards all writes silently.
        if matches!(register.map(|r| r.kind()), Some(RegisterKind::BlackHole)) {
            return;
        }

        match register {
            None => {
                // No explicit register — update unnamed register.
                self.unnamed = Some(content.clone());
                // No explicit register - use default behavior
                if is_delete && content.is_linewise() {
                    // Deletes that are linewise (or multi-line characterwise) rotate numbered registers
                    self.numbered.push(content);
                } else if is_delete {
                    // Non-linewise deletes go to small delete register
                    // if they don't contain newlines
                    if !content.text().contains('\n') {
                        self.small_delete = Some(content);
                    } else {
                        self.numbered.push(content);
                    }
                } else {
                    // Yank - store to "0
                    self.yank = Some(content);
                }
            }
            Some(reg_id) => {
                match reg_id.kind() {
                    RegisterKind::Unnamed => {
                        // Explicit unnamed register — update unnamed register, same as no register.
                        self.unnamed = Some(content.clone());
                        if is_delete && content.is_linewise() {
                            self.numbered.push(content);
                        } else if is_delete && !content.text().contains('\n') {
                            self.small_delete = Some(content);
                        } else if is_delete {
                            self.numbered.push(content);
                        } else {
                            self.yank = Some(content);
                        }
                    }
                    RegisterKind::Named(c) => {
                        // Check if original char was uppercase (append)
                        let is_append = original_char.is_some_and(is_append_register);
                        if is_append {
                            self.named.append(c, &content);
                        } else {
                            self.named.store(c, content.clone());
                        }

                        // Named register: unnamed ("") is NOT updated (POSIX vi).
                        // Named register deletes don't rotate numbered,
                        // and named yanks don't update "0 either.
                    }
                    RegisterKind::Numbered(n) => {
                        // Writing to numbered registers directly is unusual.
                        // "0 (yank register) is only updated by the default no-register
                        // yank path, not by direct writes to numbered registers.
                        if n == 0 {
                            self.yank = Some(content);
                        }
                        // "1-"9: direct writes are silently accepted but don't update
                        // any automatic registers.
                    }
                    RegisterKind::SmallDelete => {
                        // Explicit small delete register — only updates "-.
                        self.small_delete = Some(content);
                    }
                    RegisterKind::Clipboard | RegisterKind::Primary => {
                        // Write to OS clipboard; do not update unnamed or numbered.
                        // Errors are silently ignored (clipboard unavailable in some envs).
                        if let Ok(mut cb) = arboard::Clipboard::new() {
                            let _ = cb.set_text(content.text());
                        }
                    }
                    RegisterKind::LastSearch
                    | RegisterKind::Filename
                    | RegisterKind::AltFilename
                    | RegisterKind::BlackHole => {
                        // Read-only / discard virtual registers — writes are silently ignored.
                        // (BlackHole is already handled above, but listed here for exhaustiveness.)
                    }
                }
            }
        }
    }

    /// Read content from the OS clipboard.
    ///
    /// Returns `None` if the clipboard is unavailable or empty.
    /// The content is always characterwise.
    pub fn get_clipboard_content() -> Option<RegisterContent> {
        arboard::Clipboard::new()
            .ok()
            .and_then(|mut cb| cb.get_text().ok())
            .filter(|s| !s.is_empty())
            .map(|s| RegisterContent::new(s, ContentType::Characterwise))
    }

    /// Store a yank operation.
    ///
    /// This is a convenience method that:
    /// - Stores to the specified register (or unnamed if None)
    /// - Always stores to "0 (yank register)
    /// - Does NOT rotate numbered registers
    pub fn store_yank(&mut self, register: Option<RegisterId>, content: RegisterContent) {
        self.store(register, None, content, false);
    }

    /// Store a delete operation.
    ///
    /// This is a convenience method that:
    /// - Stores to the specified register (or unnamed if None)
    /// - Rotates numbered registers (1-9) for linewise deletes
    /// - Stores to small delete register for non-linewise deletes without newlines
    pub fn store_delete(
        &mut self,
        register: Option<RegisterId>,
        original_char: Option<char>,
        content: RegisterContent,
    ) {
        self.store(register, original_char, content, true);
    }

    /// Store to the small delete register ("-).
    ///
    /// Called for deletes that are less than one line.
    pub fn store_small_delete(&mut self, content: RegisterContent) {
        self.small_delete = Some(content.clone());
        self.unnamed = Some(content);
    }

    /// Retrieve content from a register.
    ///
    /// Returns None if the register is empty.
    /// For `Clipboard`/`Primary` registers use `get_owned()` instead.
    pub fn get(&self, register: Option<RegisterId>) -> Option<&RegisterContent> {
        match register {
            None => self.unnamed.as_ref(),
            Some(reg_id) => match reg_id.kind() {
                RegisterKind::Unnamed => self.unnamed.as_ref(),
                RegisterKind::Named(c) => self.named.get(c),
                RegisterKind::Numbered(n) => {
                    if n == 0 {
                        self.yank.as_ref()
                    } else {
                        self.numbered.get(n)
                    }
                }
                RegisterKind::SmallDelete => self.small_delete.as_ref(),
                RegisterKind::Clipboard | RegisterKind::Primary => {
                    // Cannot return a reference to dynamically-fetched clipboard data.
                    // Callers should use get_owned() for clipboard registers.
                    None
                }
                RegisterKind::LastSearch
                | RegisterKind::Filename
                | RegisterKind::AltFilename
                | RegisterKind::BlackHole => {
                    // Virtual/discard registers — always read as empty.
                    None
                }
            },
        }
    }

    /// Retrieve owned content from a register.
    ///
    /// Like `get()` but returns owned `RegisterContent`, and handles
    /// `Clipboard`/`Primary` by reading from the OS clipboard.
    pub fn get_owned(&self, register: Option<RegisterId>) -> Option<RegisterContent> {
        match register {
            None => self.unnamed.clone(),
            Some(reg_id) => match reg_id.kind() {
                RegisterKind::Clipboard | RegisterKind::Primary => Self::get_clipboard_content(),
                _ => self.get(Some(reg_id)).cloned(),
            },
        }
    }

    /// Check if a register has content.
    pub fn has(&self, register: Option<RegisterId>) -> bool {
        self.get(register).is_some()
    }

    /// Clear a specific register.
    pub fn clear(&mut self, register: Option<RegisterId>) {
        match register {
            None => self.unnamed = None,
            Some(reg_id) => match reg_id.kind() {
                RegisterKind::Unnamed => self.unnamed = None,
                RegisterKind::Named(c) => self.named.clear(c),
                RegisterKind::Numbered(n) => {
                    if n == 0 {
                        self.yank = None;
                    }
                    // Can't clear individual numbered registers 1-9
                }
                RegisterKind::SmallDelete => self.small_delete = None,
                RegisterKind::Clipboard | RegisterKind::Primary => {
                    // Clipboard is managed by the OS; nothing to clear locally.
                }
                RegisterKind::LastSearch
                | RegisterKind::Filename
                | RegisterKind::AltFilename
                | RegisterKind::BlackHole => {
                    // Read-only / discard virtual registers — nothing to clear.
                }
            },
        }
    }

    /// Clear all registers.
    pub fn clear_all(&mut self) {
        self.unnamed = None;
        self.named.clear_all();
        self.numbered.clear();
        self.yank = None;
        self.small_delete = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registers::types::ContentType;

    // Helper to create characterwise content
    fn char_content(s: &str) -> RegisterContent {
        RegisterContent::new(s.to_string(), ContentType::Characterwise)
    }

    // Helper to create linewise content
    fn line_content(s: &str) -> RegisterContent {
        RegisterContent::new(s.to_string(), ContentType::Linewise)
    }

    // Basic store and get tests

    #[test]
    fn test_store_to_unnamed() {
        let mut storage = RegisterStorage::new();

        storage.store(None, None, char_content("hello"), false);

        assert_eq!(storage.get(None).unwrap().text(), "hello");
    }

    #[test]
    fn test_store_to_named_register() {
        let mut storage = RegisterStorage::new();

        storage.store(
            RegisterId::parse('a'),
            Some('a'),
            char_content("alpha"),
            false,
        );

        assert_eq!(storage.get(RegisterId::parse('a')).unwrap().text(), "alpha");
        // Named register yank does NOT update unnamed ("") — POSIX vi semantics.
        assert!(storage.get(None).is_none());
    }

    #[test]
    fn test_store_append_to_named_register() {
        let mut storage = RegisterStorage::new();

        // First store with lowercase
        storage.store(
            RegisterId::parse('a'),
            Some('a'),
            char_content("hello"),
            false,
        );

        // Then append with uppercase
        storage.store(
            RegisterId::parse('A'),
            Some('A'),
            char_content(" world"),
            false,
        );

        assert_eq!(
            storage.get(RegisterId::parse('a')).unwrap().text(),
            "hello world"
        );
    }

    #[test]
    fn test_append_to_empty_named_register() {
        let mut storage = RegisterStorage::new();

        // Append to empty register
        storage.store(
            RegisterId::parse('A'),
            Some('A'),
            char_content("hello"),
            false,
        );

        assert_eq!(storage.get(RegisterId::parse('a')).unwrap().text(), "hello");
    }

    // Yank register tests

    #[test]
    fn test_yank_stores_to_register_zero() {
        let mut storage = RegisterStorage::new();

        storage.store_yank(None, char_content("yanked"));

        // Should be in "0
        assert_eq!(
            storage.get(RegisterId::parse('0')).unwrap().text(),
            "yanked"
        );
        // And in unnamed
        assert_eq!(storage.get(None).unwrap().text(), "yanked");
    }

    #[test]
    fn test_yank_to_named_does_not_update_zero() {
        // POSIX vi: "0 is only updated by an unregistered yank, not by "ayy etc.
        let mut storage = RegisterStorage::new();

        storage.store_yank(RegisterId::parse('a'), char_content("yanked"));

        // Should be in "a
        assert_eq!(
            storage.get(RegisterId::parse('a')).unwrap().text(),
            "yanked"
        );
        // "0 must NOT be updated when an explicit named register is used.
        assert!(storage.get(RegisterId::parse('0')).is_none());
        // "" (unnamed) must NOT be updated either.
        assert!(storage.get(None).is_none());
    }

    #[test]
    fn test_yank_does_not_rotate_numbered() {
        let mut storage = RegisterStorage::new();

        // Do some yanks
        storage.store_yank(None, char_content("yank1"));
        storage.store_yank(None, char_content("yank2"));
        storage.store_yank(None, char_content("yank3"));

        // Numbered registers 1-9 should be empty
        assert!(storage.get(RegisterId::parse('1')).is_none());
        assert!(storage.get(RegisterId::parse('2')).is_none());

        // But "0 should have the latest yank
        assert_eq!(storage.get(RegisterId::parse('0')).unwrap().text(), "yank3");
    }

    // Delete register tests

    #[test]
    fn test_linewise_delete_rotates_numbered() {
        let mut storage = RegisterStorage::new();

        storage.store_delete(None, None, line_content("line1\n"));
        storage.store_delete(None, None, line_content("line2\n"));
        storage.store_delete(None, None, line_content("line3\n"));

        // Most recent should be in "1
        assert_eq!(
            storage.get(RegisterId::parse('1')).unwrap().text(),
            "line3\n"
        );
        // Previous in "2
        assert_eq!(
            storage.get(RegisterId::parse('2')).unwrap().text(),
            "line2\n"
        );
        // Oldest in "3
        assert_eq!(
            storage.get(RegisterId::parse('3')).unwrap().text(),
            "line1\n"
        );
    }

    #[test]
    fn test_small_delete_goes_to_small_delete_register() {
        let mut storage = RegisterStorage::new();

        // Delete without newline -> small delete
        storage.store_delete(None, None, char_content("word"));

        // Should be in "-
        assert_eq!(storage.get(RegisterId::parse('-')).unwrap().text(), "word");
        // Should NOT be in "1
        assert!(storage.get(RegisterId::parse('1')).is_none());
    }

    #[test]
    fn test_delete_with_newline_rotates_numbered() {
        let mut storage = RegisterStorage::new();

        // Characterwise content but contains newline
        storage.store_delete(None, None, char_content("line1\nline2"));

        // Should be in "1, not "-
        assert_eq!(
            storage.get(RegisterId::parse('1')).unwrap().text(),
            "line1\nline2"
        );
    }

    #[test]
    fn test_delete_to_named_does_not_rotate() {
        let mut storage = RegisterStorage::new();

        // Delete to named register
        storage.store_delete(RegisterId::parse('a'), Some('a'), line_content("deleted\n"));

        // Should be in "a
        assert_eq!(
            storage.get(RegisterId::parse('a')).unwrap().text(),
            "deleted\n"
        );
        // Should NOT be in "1
        assert!(storage.get(RegisterId::parse('1')).is_none());
    }

    #[test]
    fn test_numbered_rotation_at_capacity() {
        let mut storage = RegisterStorage::new();

        // Fill up with 9 deletes
        for i in 1..=9 {
            storage.store_delete(None, None, line_content(&format!("delete{}\n", i)));
        }

        assert_eq!(
            storage.get(RegisterId::parse('1')).unwrap().text(),
            "delete9\n"
        );
        assert_eq!(
            storage.get(RegisterId::parse('9')).unwrap().text(),
            "delete1\n"
        );

        // One more delete should discard delete1
        storage.store_delete(None, None, line_content("delete10\n"));

        assert_eq!(
            storage.get(RegisterId::parse('1')).unwrap().text(),
            "delete10\n"
        );
        assert_eq!(
            storage.get(RegisterId::parse('9')).unwrap().text(),
            "delete2\n"
        );
    }

    // Combined yank and delete tests

    #[test]
    fn test_yank_then_delete() {
        let mut storage = RegisterStorage::new();

        // Yank something
        storage.store_yank(None, char_content("yanked"));
        assert_eq!(
            storage.get(RegisterId::parse('0')).unwrap().text(),
            "yanked"
        );

        // Delete something
        storage.store_delete(None, None, line_content("deleted\n"));

        // Unnamed should have delete
        assert_eq!(storage.get(None).unwrap().text(), "deleted\n");
        // "0 should still have yank
        assert_eq!(
            storage.get(RegisterId::parse('0')).unwrap().text(),
            "yanked"
        );
        // "1 should have delete
        assert_eq!(
            storage.get(RegisterId::parse('1')).unwrap().text(),
            "deleted\n"
        );
    }

    #[test]
    fn test_delete_then_yank() {
        let mut storage = RegisterStorage::new();

        // Delete something
        storage.store_delete(None, None, line_content("deleted\n"));

        // Yank something
        storage.store_yank(None, char_content("yanked"));

        // Unnamed should have yank
        assert_eq!(storage.get(None).unwrap().text(), "yanked");
        // "0 should have yank
        assert_eq!(
            storage.get(RegisterId::parse('0')).unwrap().text(),
            "yanked"
        );
        // "1 should have delete
        assert_eq!(
            storage.get(RegisterId::parse('1')).unwrap().text(),
            "deleted\n"
        );
    }

    // Clear tests

    #[test]
    fn test_clear_unnamed() {
        let mut storage = RegisterStorage::new();
        storage.store(None, None, char_content("test"), false);

        storage.clear(None);

        assert!(storage.get(None).is_none());
    }

    #[test]
    fn test_clear_named() {
        let mut storage = RegisterStorage::new();
        storage.store(
            RegisterId::parse('a'),
            Some('a'),
            char_content("test"),
            false,
        );

        storage.clear(RegisterId::parse('a'));

        assert!(storage.get(RegisterId::parse('a')).is_none());
    }

    #[test]
    fn test_clear_all() {
        let mut storage = RegisterStorage::new();
        storage.store(None, None, char_content("unnamed"), false);
        storage.store(
            RegisterId::parse('a'),
            Some('a'),
            char_content("named"),
            false,
        );
        storage.store_delete(None, None, line_content("deleted\n"));

        storage.clear_all();

        assert!(storage.get(None).is_none());
        assert!(storage.get(RegisterId::parse('a')).is_none());
        assert!(storage.get(RegisterId::parse('0')).is_none());
        assert!(storage.get(RegisterId::parse('1')).is_none());
    }

    // has() tests

    #[test]
    fn test_has() {
        let mut storage = RegisterStorage::new();

        assert!(!storage.has(None));
        assert!(!storage.has(RegisterId::parse('a')));

        storage.store(None, None, char_content("test"), false);

        assert!(storage.has(None));
    }

    // store_small_delete tests

    #[test]
    fn test_store_small_delete_directly() {
        let mut storage = RegisterStorage::new();

        storage.store_small_delete(char_content("small"));

        assert_eq!(storage.get(RegisterId::parse('-')).unwrap().text(), "small");
        assert_eq!(storage.get(None).unwrap().text(), "small");
    }
}

//! Internal register storage implementations.
//!
//! This module provides the underlying storage types for different register categories:
//! - `NamedStorage`: HashMap-based storage for named registers (a-z)
//! - `NumberedStorage`: VecDeque-based storage for numbered registers (1-9) with rotation

use std::collections::{HashMap, VecDeque};

use super::types::RegisterContent;

/// Storage for named registers (a-z).
///
/// Uses a HashMap with the register character as key.
/// Maximum of 26 entries (one per letter).
#[derive(Debug, Default)]
pub struct NamedStorage {
    /// Map from register character to content.
    registers: HashMap<char, RegisterContent>,
}

impl NamedStorage {
    /// Store content to a named register.
    pub fn store(&mut self, name: char, content: RegisterContent) {
        debug_assert!(
            name.is_ascii_lowercase(),
            "named register must be lowercase"
        );
        self.registers.insert(name, content);
    }

    /// Append content to a named register.
    ///
    /// If the register is empty, this is equivalent to store.
    /// Otherwise, appends to existing content.
    pub fn append(&mut self, name: char, content: &RegisterContent) {
        debug_assert!(
            name.is_ascii_lowercase(),
            "named register must be lowercase"
        );

        if let Some(existing) = self.registers.get_mut(&name) {
            existing.append(content);
        } else {
            self.registers.insert(name, content.clone());
        }
    }

    /// Get content from a named register.
    pub fn get(&self, name: char) -> Option<&RegisterContent> {
        self.registers.get(&name)
    }

    /// Clear a named register.
    pub fn clear(&mut self, name: char) {
        self.registers.remove(&name);
    }

    /// Clear all named registers.
    pub fn clear_all(&mut self) {
        self.registers.clear();
    }
}

/// Maximum number of entries in numbered delete history (registers 1-9).
const NUMBERED_HISTORY_SIZE: usize = 9;

/// Storage for numbered registers (1-9).
///
/// Implements rotation behavior:
/// - On delete, content shifts: 1->2, 2->3, ..., 8->9, new->1
/// - Register 9 is discarded when full
///
/// Note: Register 0 is handled separately (yank register).
#[derive(Debug, Default)]
pub struct NumberedStorage {
    /// Delete history, index 0 is register "1", index 8 is register "9".
    history: VecDeque<RegisterContent>,
}

impl NumberedStorage {
    /// Push new content to register "1, rotating existing content.
    ///
    /// After this operation:
    /// - "1 contains the new content
    /// - "2 contains what was in "1
    /// - "3 contains what was in "2
    /// - etc.
    /// - Old "9 content is discarded
    pub fn push(&mut self, content: RegisterContent) {
        // If at capacity, remove the oldest (register "9)
        if self.history.len() >= NUMBERED_HISTORY_SIZE {
            self.history.pop_back();
        }
        // Push new content to front (register "1)
        self.history.push_front(content);
    }

    /// Get content from a numbered register (1-9).
    ///
    /// Returns `None` if the register index is invalid or empty.
    pub fn get(&self, num: u8) -> Option<&RegisterContent> {
        if num == 0 || num > 9 {
            return None;
        }
        // Register "1 is at index 0, "2 at index 1, etc.
        let index = (num - 1) as usize;
        self.history.get(index)
    }

    /// Clear all numbered registers.
    pub fn clear(&mut self) {
        self.history.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registers::types::ContentType;

    // Test helper: create new NamedStorage
    fn new_named_storage() -> NamedStorage {
        NamedStorage::default()
    }

    // Test helper: create new NumberedStorage
    fn new_numbered_storage() -> NumberedStorage {
        NumberedStorage::default()
    }

    // Test helper: check if named storage has a register
    fn named_has(storage: &NamedStorage, name: char) -> bool {
        storage.get(name).is_some()
    }

    // Test helper: check if numbered storage has a register
    fn numbered_has(storage: &NumberedStorage, num: u8) -> bool {
        storage.get(num).is_some()
    }

    // Test helper: get numbered storage length
    fn numbered_len(storage: &NumberedStorage) -> usize {
        (1..=9).filter(|&n| storage.get(n).is_some()).count()
    }

    // Test helper: check if numbered storage is empty
    fn numbered_is_empty(storage: &NumberedStorage) -> bool {
        numbered_len(storage) == 0
    }

    // NamedStorage tests

    #[test]
    fn test_named_storage_new() {
        let storage = new_named_storage();
        assert!(storage.get('a').is_none());
    }

    #[test]
    fn test_named_storage_store_and_get() {
        let mut storage = new_named_storage();
        let content = RegisterContent::new("hello".to_string(), ContentType::Characterwise);

        storage.store('a', content);

        let retrieved = storage.get('a');
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().text(), "hello");
    }

    #[test]
    fn test_named_storage_store_multiple() {
        let mut storage = new_named_storage();

        storage.store('a', RegisterContent::characterwise("alpha".to_string()));
        storage.store('b', RegisterContent::characterwise("beta".to_string()));
        storage.store('z', RegisterContent::characterwise("zeta".to_string()));

        assert_eq!(storage.get('a').unwrap().text(), "alpha");
        assert_eq!(storage.get('b').unwrap().text(), "beta");
        assert_eq!(storage.get('z').unwrap().text(), "zeta");
    }

    #[test]
    fn test_named_storage_overwrite() {
        let mut storage = new_named_storage();

        storage.store('a', RegisterContent::characterwise("first".to_string()));
        storage.store('a', RegisterContent::characterwise("second".to_string()));

        assert_eq!(storage.get('a').unwrap().text(), "second");
    }

    #[test]
    fn test_named_storage_append_to_existing() {
        let mut storage = new_named_storage();

        storage.store('a', RegisterContent::characterwise("hello".to_string()));
        storage.append('a', &RegisterContent::characterwise(" world".to_string()));

        assert_eq!(storage.get('a').unwrap().text(), "hello world");
    }

    #[test]
    fn test_named_storage_append_to_empty() {
        let mut storage = new_named_storage();

        storage.append('a', &RegisterContent::characterwise("hello".to_string()));

        assert_eq!(storage.get('a').unwrap().text(), "hello");
    }

    #[test]
    fn test_named_storage_has() {
        let mut storage = new_named_storage();

        assert!(!named_has(&storage, 'a'));
        storage.store('a', RegisterContent::characterwise("test".to_string()));
        assert!(named_has(&storage, 'a'));
        assert!(!named_has(&storage, 'b'));
    }

    #[test]
    fn test_named_storage_clear() {
        let mut storage = new_named_storage();

        storage.store('a', RegisterContent::characterwise("test".to_string()));
        assert!(named_has(&storage, 'a'));

        storage.clear('a');
        assert!(!named_has(&storage, 'a'));
    }

    #[test]
    fn test_named_storage_clear_all() {
        let mut storage = new_named_storage();

        storage.store('a', RegisterContent::characterwise("alpha".to_string()));
        storage.store('b', RegisterContent::characterwise("beta".to_string()));

        storage.clear_all();

        assert!(!named_has(&storage, 'a'));
        assert!(!named_has(&storage, 'b'));
    }

    // NumberedStorage tests

    #[test]
    fn test_numbered_storage_new() {
        let storage = new_numbered_storage();
        assert!(numbered_is_empty(&storage));
        assert_eq!(numbered_len(&storage), 0);
    }

    #[test]
    fn test_numbered_storage_push_single() {
        let mut storage = new_numbered_storage();

        storage.push(RegisterContent::characterwise("first".to_string()));

        assert_eq!(numbered_len(&storage), 1);
        assert_eq!(storage.get(1).unwrap().text(), "first");
        assert!(storage.get(2).is_none());
    }

    #[test]
    fn test_numbered_storage_push_multiple() {
        let mut storage = new_numbered_storage();

        storage.push(RegisterContent::characterwise("first".to_string()));
        storage.push(RegisterContent::characterwise("second".to_string()));
        storage.push(RegisterContent::characterwise("third".to_string()));

        // Most recent is "1, older entries rotate
        assert_eq!(storage.get(1).unwrap().text(), "third");
        assert_eq!(storage.get(2).unwrap().text(), "second");
        assert_eq!(storage.get(3).unwrap().text(), "first");
    }

    #[test]
    fn test_numbered_storage_rotation_at_capacity() {
        let mut storage = new_numbered_storage();

        // Push 9 items to fill capacity
        for i in 1..=9 {
            storage.push(RegisterContent::characterwise(format!("item{}", i)));
        }

        assert_eq!(numbered_len(&storage), 9);
        assert_eq!(storage.get(1).unwrap().text(), "item9");
        assert_eq!(storage.get(9).unwrap().text(), "item1");

        // Push one more - should discard oldest (item1)
        storage.push(RegisterContent::characterwise("item10".to_string()));

        assert_eq!(numbered_len(&storage), 9);
        assert_eq!(storage.get(1).unwrap().text(), "item10");
        assert_eq!(storage.get(9).unwrap().text(), "item2");
        // item1 is gone
    }

    #[test]
    fn test_numbered_storage_get_invalid_index() {
        let mut storage = new_numbered_storage();
        storage.push(RegisterContent::characterwise("test".to_string()));

        // Index 0 is not valid for numbered delete history
        assert!(storage.get(0).is_none());
        // Index > 9 is not valid
        assert!(storage.get(10).is_none());
    }

    #[test]
    fn test_numbered_storage_has() {
        let mut storage = new_numbered_storage();

        assert!(!numbered_has(&storage, 1));

        storage.push(RegisterContent::characterwise("test".to_string()));

        assert!(numbered_has(&storage, 1));
        assert!(!numbered_has(&storage, 2));
        assert!(!numbered_has(&storage, 0)); // 0 is not valid
        assert!(!numbered_has(&storage, 10)); // 10 is not valid
    }

    #[test]
    fn test_numbered_storage_clear() {
        let mut storage = new_numbered_storage();

        storage.push(RegisterContent::characterwise("first".to_string()));
        storage.push(RegisterContent::characterwise("second".to_string()));

        storage.clear();

        assert!(numbered_is_empty(&storage));
        assert!(storage.get(1).is_none());
    }

    #[test]
    fn test_numbered_storage_preserves_content_type() {
        let mut storage = new_numbered_storage();

        storage.push(RegisterContent::linewise("line\n".to_string()));

        let content = storage.get(1).unwrap();
        assert!(content.is_linewise());
        assert_eq!(content.text(), "line\n");
    }
}

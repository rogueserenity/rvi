//! Register type definitions.
//!
//! This module defines the core types for vi-style registers:
//! - `RegisterId`: Type-safe register identifier
//! - `RegisterKind`: The category of register
//! - `ContentType`: Characterwise vs linewise content
//! - `RegisterContent`: The actual content stored in a register

/// The kind of register.
///
/// Vi has several register categories with different behaviors:
/// - Unnamed: Default register for all operations
/// - Named: User-controlled storage (a-z)
/// - Numbered: Automatic delete history (0-9)
/// - SmallDelete: Deletes less than one line
/// - Clipboard: System clipboard (`"+`)
/// - Primary: Primary selection (`"*`); on macOS same as Clipboard
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegisterKind {
    /// The unnamed register (""). Default for all operations.
    Unnamed,
    /// Named registers ("a-"z). Stored as lowercase.
    Named(char),
    /// Numbered registers ("0-"9).
    /// "0 holds most recent yank, "1-"9 hold delete history.
    Numbered(u8),
    /// Small delete register ("-). Holds deletes less than one line.
    SmallDelete,
    /// System clipboard register (`"+`).
    Clipboard,
    /// Primary selection register (`"*`). On macOS treated as Clipboard.
    Primary,
    /// Last search pattern register (`"/`). Read-only, computed dynamically.
    LastSearch,
    /// Current filename register (`"%`). Read-only, computed dynamically.
    Filename,
    /// Alternate filename register (`"#`). Read-only, computed dynamically.
    AltFilename,
    /// Black hole register (`"_`). Writes are discarded; reads return empty.
    BlackHole,
}

/// Type-safe register identifier.
///
/// Uses parse-don't-validate pattern: can only be constructed from valid
/// register characters, ensuring invalid registers cannot be referenced.
///
/// # Examples
///
/// ```ignore
/// let reg = RegisterId::parse('a').expect("valid register");
/// assert!(!is_append_register('a'));
///
/// assert!(is_append_register('A'));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RegisterId(RegisterKind);

impl RegisterId {
    /// Parse a register character into a RegisterId.
    ///
    /// Valid register characters:
    /// - `a-z`: Named registers (lowercase)
    /// - `A-Z`: Named registers (append mode)
    /// - `0-9`: Numbered registers
    /// - `-`: Small delete register
    /// - `"`: Unnamed register
    ///
    /// Returns `None` for invalid characters.
    pub fn parse(c: char) -> Option<Self> {
        match c {
            '"' => Some(Self(RegisterKind::Unnamed)),
            'a'..='z' => Some(Self(RegisterKind::Named(c))),
            'A'..='Z' => Some(Self(RegisterKind::Named(c.to_ascii_lowercase()))),
            '0'..='9' => Some(Self(RegisterKind::Numbered(c as u8 - b'0'))),
            '-' => Some(Self(RegisterKind::SmallDelete)),
            '+' => Some(Self(RegisterKind::Clipboard)),
            '*' => Some(Self(RegisterKind::Primary)),
            '_' => Some(Self(RegisterKind::BlackHole)),
            // Read-only virtual registers
            '/' => Some(Self(RegisterKind::LastSearch)),
            '%' => Some(Self(RegisterKind::Filename)),
            '#' => Some(Self(RegisterKind::AltFilename)),
            _ => None,
        }
    }

    /// Get the unnamed register.
    pub fn unnamed() -> Self {
        Self(RegisterKind::Unnamed)
    }

    /// Get the register kind.
    pub fn kind(&self) -> RegisterKind {
        self.0
    }

    /// Check if this is the unnamed register.
    pub fn is_unnamed(&self) -> bool {
        matches!(self.0, RegisterKind::Unnamed)
    }

    /// Check if this is a named register (a-z).
    pub fn is_named(&self) -> bool {
        matches!(self.0, RegisterKind::Named(_))
    }

    /// Check if this is a numbered register (0-9).
    pub fn is_numbered(&self) -> bool {
        matches!(self.0, RegisterKind::Numbered(_))
    }

    /// Check if this is the small delete register (-).
    pub fn is_small_delete(&self) -> bool {
        matches!(self.0, RegisterKind::SmallDelete)
    }

    /// Get the named register character, if this is a named register.
    pub fn named_char(&self) -> Option<char> {
        match self.0 {
            RegisterKind::Named(c) => Some(c),
            _ => None,
        }
    }

    /// Get the numbered register index, if this is a numbered register.
    pub fn numbered_index(&self) -> Option<u8> {
        match self.0 {
            RegisterKind::Numbered(n) => Some(n),
            _ => None,
        }
    }
}

/// Check if a register character represents an append operation (A-Z).
///
/// This is a standalone function because RegisterId normalizes to lowercase.
pub fn is_append_register(c: char) -> bool {
    c.is_ascii_uppercase()
}

/// Content type affects how put operations work.
///
/// - Characterwise: Insert inline at cursor position
/// - Linewise: Insert on new line above/below
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContentType {
    /// Content is characterwise (from dw, x, yw, etc.).
    /// Put inserts inline at cursor position.
    #[default]
    Characterwise,
    /// Content is linewise (from dd, yy, etc.).
    /// Put inserts on a new line above or below.
    Linewise,
    /// Content is block-wise (from Ctrl-v visual block yank/delete).
    /// Each '\n'-separated segment is one column of the block.
    /// Put inserts the corresponding segment on each successive line.
    Block,
}

/// Register content with text and type.
///
/// Stores both the text content and whether it was yanked/deleted
/// as a characterwise or linewise operation, which affects put behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterContent {
    /// The text content.
    text: String,
    /// How the content was captured (affects put behavior).
    content_type: ContentType,
}

impl RegisterContent {
    /// Create new register content.
    pub fn new(text: String, content_type: ContentType) -> Self {
        Self { text, content_type }
    }

    /// Create characterwise content.
    pub fn characterwise(text: String) -> Self {
        Self::new(text, ContentType::Characterwise)
    }

    /// Create linewise content.
    pub fn linewise(text: String) -> Self {
        Self::new(text, ContentType::Linewise)
    }

    /// Get the text content.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Get the content type.
    pub fn content_type(&self) -> ContentType {
        self.content_type
    }

    /// Check if this is linewise content.
    pub fn is_linewise(&self) -> bool {
        self.content_type == ContentType::Linewise
    }

    /// Check if this is characterwise content.
    pub fn is_characterwise(&self) -> bool {
        self.content_type == ContentType::Characterwise
    }

    /// Check if this is block-wise content.
    pub fn is_block(&self) -> bool {
        self.content_type == ContentType::Block
    }

    /// Append text from another RegisterContent.
    ///
    /// Used for uppercase register operations (A-Z).
    /// If either content is linewise, ensures proper line separation.
    pub fn append(&mut self, other: &RegisterContent) {
        // If the existing content is linewise or doesn't end with newline,
        // and we're appending linewise content, add a newline separator.
        if self.content_type == ContentType::Linewise || other.content_type == ContentType::Linewise
        {
            if !self.text.ends_with('\n') {
                self.text.push('\n');
            }
            self.text.push_str(other.text());
            // Result is linewise if either was linewise
            self.content_type = ContentType::Linewise;
        } else {
            // Both characterwise - just concatenate
            self.text.push_str(other.text());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // RegisterId::parse tests

    #[test]
    fn test_parse_lowercase_named_registers() {
        for c in 'a'..='z' {
            let reg = RegisterId::parse(c);
            assert!(reg.is_some(), "should parse '{}'", c);
            let reg = reg.unwrap();
            assert!(reg.is_named());
            assert_eq!(reg.named_char(), Some(c));
        }
    }

    #[test]
    fn test_parse_uppercase_named_registers() {
        for c in 'A'..='Z' {
            let reg = RegisterId::parse(c);
            assert!(reg.is_some(), "should parse '{}'", c);
            let reg = reg.unwrap();
            assert!(reg.is_named());
            // Uppercase is normalized to lowercase
            assert_eq!(reg.named_char(), Some(c.to_ascii_lowercase()));
        }
    }

    #[test]
    fn test_parse_numbered_registers() {
        for c in '0'..='9' {
            let reg = RegisterId::parse(c);
            assert!(reg.is_some(), "should parse '{}'", c);
            let reg = reg.unwrap();
            assert!(reg.is_numbered());
            assert_eq!(reg.numbered_index(), Some(c as u8 - b'0'));
        }
    }

    #[test]
    fn test_parse_unnamed_register() {
        let reg = RegisterId::parse('"');
        assert!(reg.is_some());
        let reg = reg.unwrap();
        assert!(reg.is_unnamed());
        assert_eq!(reg.kind(), RegisterKind::Unnamed);
    }

    #[test]
    fn test_parse_small_delete_register() {
        let reg = RegisterId::parse('-');
        assert!(reg.is_some());
        let reg = reg.unwrap();
        assert!(reg.is_small_delete());
        assert_eq!(reg.kind(), RegisterKind::SmallDelete);
    }

    #[test]
    fn test_parse_virtual_registers() {
        // "/" = LastSearch
        let reg = RegisterId::parse('/').unwrap();
        assert_eq!(reg.kind(), RegisterKind::LastSearch);

        // "%" = Filename
        let reg = RegisterId::parse('%').unwrap();
        assert_eq!(reg.kind(), RegisterKind::Filename);

        // "#" = AltFilename
        let reg = RegisterId::parse('#').unwrap();
        assert_eq!(reg.kind(), RegisterKind::AltFilename);
    }

    #[test]
    fn test_parse_invalid_registers() {
        // Invalid characters should return None
        assert!(RegisterId::parse('!').is_none());
        assert!(RegisterId::parse('@').is_none());
        assert!(RegisterId::parse(' ').is_none());
        assert!(RegisterId::parse('\n').is_none());
        assert!(RegisterId::parse('\t').is_none());
        // '#' is now valid (AltFilename register)
        assert!(RegisterId::parse('#').is_some());
    }

    #[test]
    fn test_unnamed_constructor() {
        let reg = RegisterId::unnamed();
        assert!(reg.is_unnamed());
        assert_eq!(reg, RegisterId::parse('"').unwrap());
    }

    // is_append_register tests

    #[test]
    fn test_is_append_register_uppercase() {
        for c in 'A'..='Z' {
            assert!(is_append_register(c), "should be append: '{}'", c);
        }
    }

    #[test]
    fn test_is_append_register_lowercase() {
        for c in 'a'..='z' {
            assert!(!is_append_register(c), "should not be append: '{}'", c);
        }
    }

    #[test]
    fn test_is_append_register_other() {
        assert!(!is_append_register('0'));
        assert!(!is_append_register('-'));
        assert!(!is_append_register('"'));
    }

    // ContentType tests

    #[test]
    fn test_content_type_default() {
        let ct: ContentType = Default::default();
        assert_eq!(ct, ContentType::Characterwise);
    }

    // RegisterContent tests

    #[test]
    fn test_register_content_new() {
        let content = RegisterContent::new("hello".to_string(), ContentType::Characterwise);
        assert_eq!(content.text(), "hello");
        assert_eq!(content.content_type(), ContentType::Characterwise);
        assert!(content.is_characterwise());
        assert!(!content.is_linewise());
    }

    #[test]
    fn test_register_content_characterwise() {
        let content = RegisterContent::characterwise("world".to_string());
        assert_eq!(content.text(), "world");
        assert!(content.is_characterwise());
    }

    #[test]
    fn test_register_content_linewise() {
        let content = RegisterContent::linewise("line\n".to_string());
        assert_eq!(content.text(), "line\n");
        assert!(content.is_linewise());
    }

    #[test]
    fn test_register_content_append_both_characterwise() {
        let mut content = RegisterContent::characterwise("hello".to_string());
        let other = RegisterContent::characterwise(" world".to_string());
        content.append(&other);
        assert_eq!(content.text(), "hello world");
        assert!(content.is_characterwise());
    }

    #[test]
    fn test_register_content_append_linewise_to_characterwise() {
        let mut content = RegisterContent::characterwise("hello".to_string());
        let other = RegisterContent::linewise("world\n".to_string());
        content.append(&other);
        assert_eq!(content.text(), "hello\nworld\n");
        assert!(content.is_linewise());
    }

    #[test]
    fn test_register_content_append_characterwise_to_linewise() {
        let mut content = RegisterContent::linewise("hello\n".to_string());
        let other = RegisterContent::characterwise("world".to_string());
        content.append(&other);
        assert_eq!(content.text(), "hello\nworld");
        assert!(content.is_linewise());
    }

    #[test]
    fn test_register_content_append_both_linewise() {
        let mut content = RegisterContent::linewise("line1\n".to_string());
        let other = RegisterContent::linewise("line2\n".to_string());
        content.append(&other);
        assert_eq!(content.text(), "line1\nline2\n");
        assert!(content.is_linewise());
    }

    #[test]
    fn test_register_content_append_linewise_no_trailing_newline() {
        let mut content = RegisterContent::linewise("hello".to_string());
        let other = RegisterContent::linewise("world".to_string());
        content.append(&other);
        assert_eq!(content.text(), "hello\nworld");
        assert!(content.is_linewise());
    }

    #[test]
    fn test_register_kind_equality() {
        assert_eq!(RegisterKind::Unnamed, RegisterKind::Unnamed);
        assert_eq!(RegisterKind::Named('a'), RegisterKind::Named('a'));
        assert_ne!(RegisterKind::Named('a'), RegisterKind::Named('b'));
        assert_eq!(RegisterKind::Numbered(0), RegisterKind::Numbered(0));
        assert_ne!(RegisterKind::Numbered(0), RegisterKind::Numbered(1));
        assert_eq!(RegisterKind::SmallDelete, RegisterKind::SmallDelete);
    }

    #[test]
    fn test_register_id_equality() {
        assert_eq!(RegisterId::parse('a'), RegisterId::parse('a'));
        assert_ne!(RegisterId::parse('a'), RegisterId::parse('b'));
        // A and a parse to the same register (both are named 'a')
        assert_eq!(RegisterId::parse('A'), RegisterId::parse('a'));
    }
}

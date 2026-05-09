//! Tag file parsing and lookup for vi-style source-code navigation.
//!
//! Supports the standard ctags format:
//! ```text
//! tagname\tfilename\taddress
//! ```
//! where `address` is either a 1-based line number or an ex search pattern
//! (`/^void foo(/`).
//!
//! Lines beginning with `!` are metadata comments and are skipped.

/// A parsed tag entry from a `tags` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub name: String,
    pub filename: String,
    pub address: TagAddress,
}

/// The location within a file that a tag points to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagAddress {
    /// 1-based line number.
    Line(usize),
    /// Regex pattern (stripped of surrounding `/` delimiters and optional `;"` suffix).
    Pattern(String),
}

/// Parse a `tags` file at `path` into a `Vec<Tag>`.
///
/// Skips lines beginning with `!` (ctags metadata) and any malformed lines.
/// Returns `Err` only on I/O failure; malformed entries are silently skipped.
pub fn parse_tags_file(path: &str) -> Result<Vec<Tag>, String> {
    let content = std::fs::read_to_string(path).map_err(|e| format!("{}: {}", path, e))?;

    let mut tags = Vec::new();
    for line in content.lines() {
        if line.starts_with('!') || line.is_empty() {
            continue;
        }
        if let Some(tag) = parse_tags_line(line) {
            tags.push(tag);
        }
        // Silently skip malformed lines — real ctags files occasionally contain
        // unusual entries and one bad line should not abort the whole file.
    }
    Ok(tags)
}

/// Parse a single tab-separated tags line.
///
/// Returns `None` if the line does not have the expected three-field format.
fn parse_tags_line(line: &str) -> Option<Tag> {
    let mut fields = line.splitn(3, '\t');
    let name = fields.next()?.to_string();
    let filename = fields.next()?.to_string();
    let raw_address = fields.next()?;

    // Strip the extended ctags `;"` suffix (and anything after it)
    let address_str = if let Some(idx) = raw_address.find(";\"") {
        &raw_address[..idx]
    } else {
        raw_address
    };
    let address_str = address_str.trim();

    let address = if address_str.starts_with('/') {
        // Forward pattern address: /^void foo(/ — strip leading and trailing /
        let inner = address_str
            .strip_prefix('/')
            .and_then(|s| s.strip_suffix('/'))
            .unwrap_or_else(|| address_str.strip_prefix('/').unwrap_or(address_str));
        TagAddress::Pattern(inner.to_string())
    } else if address_str.starts_with('?') {
        // Backward pattern address: ?^void foo(? — strip leading and trailing ?
        let inner = address_str
            .strip_prefix('?')
            .and_then(|s| s.strip_suffix('?'))
            .unwrap_or_else(|| address_str.strip_prefix('?').unwrap_or(address_str));
        TagAddress::Pattern(inner.to_string())
    } else if let Ok(n) = address_str.parse::<usize>() {
        TagAddress::Line(n)
    } else {
        return None;
    };

    Some(Tag {
        name,
        filename,
        address,
    })
}

/// Find the first tag matching `name` (exact match).
///
/// Returns `None` if no tag with that name exists.
pub fn find_tag<'a>(tags: &'a [Tag], name: &str) -> Option<&'a Tag> {
    tags.iter().find(|t| t.name == name)
}

/// Find the first tag whose name starts with `prefix`.
///
/// Used when `taglength` is set: the lookup key is truncated and
/// the first tag whose name begins with the truncated key is returned.
pub fn find_tag_prefix<'a>(tags: &'a [Tag], prefix: &str) -> Option<&'a Tag> {
    tags.iter().find(|t| t.name.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Write content to a temp file and return its path.
    fn write_temp(name: &str, content: &str) -> PathBuf {
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn test_parse_tags_line_number() {
        let tag = parse_tags_line("foo\tsrc/foo.rs\t42").unwrap();
        assert_eq!(tag.name, "foo");
        assert_eq!(tag.filename, "src/foo.rs");
        assert_eq!(tag.address, TagAddress::Line(42));
    }

    #[test]
    fn test_parse_tags_pattern() {
        let tag = parse_tags_line("foo\tsrc/foo.rs\t/^void foo(/").unwrap();
        assert_eq!(tag.name, "foo");
        assert_eq!(tag.filename, "src/foo.rs");
        assert_eq!(tag.address, TagAddress::Pattern("^void foo(".to_string()));
    }

    #[test]
    fn test_parse_tags_pattern_with_semicolon_suffix() {
        let tag = parse_tags_line("bar\tsrc/bar.rs\t/^fn bar(/;\"").unwrap();
        assert_eq!(tag.address, TagAddress::Pattern("^fn bar(".to_string()));
    }

    #[test]
    fn test_parse_tags_skips_metadata() {
        let content = "!_TAG_FILE_FORMAT\t2\n!_TAG_PROGRAM_NAME\tctags\nfoo\tfile.rs\t1\n";
        let path = write_temp("rvi_test_tags_meta.tags", content);
        let tags = parse_tags_file(path.to_str().unwrap()).unwrap();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].name, "foo");
    }

    #[test]
    fn test_find_tag_exact() {
        let tags = vec![
            Tag {
                name: "foo".to_string(),
                filename: "a.rs".to_string(),
                address: TagAddress::Line(1),
            },
            Tag {
                name: "bar".to_string(),
                filename: "b.rs".to_string(),
                address: TagAddress::Line(2),
            },
        ];
        let t = find_tag(&tags, "bar").unwrap();
        assert_eq!(t.name, "bar");
    }

    #[test]
    fn test_find_tag_not_found() {
        let tags = vec![Tag {
            name: "foo".to_string(),
            filename: "a.rs".to_string(),
            address: TagAddress::Line(1),
        }];
        assert!(find_tag(&tags, "baz").is_none());
    }

    #[test]
    fn test_find_tag_prefix_match() {
        let tags = vec![
            Tag {
                name: "foobar".to_string(),
                filename: "a.rs".to_string(),
                address: TagAddress::Line(1),
            },
            Tag {
                name: "baz".to_string(),
                filename: "b.rs".to_string(),
                address: TagAddress::Line(2),
            },
        ];
        let t = find_tag_prefix(&tags, "foo").unwrap();
        assert_eq!(t.name, "foobar");
    }

    #[test]
    fn test_find_tag_prefix_no_match() {
        let tags = vec![Tag {
            name: "foo".to_string(),
            filename: "a.rs".to_string(),
            address: TagAddress::Line(1),
        }];
        assert!(find_tag_prefix(&tags, "bar").is_none());
    }

    #[test]
    fn test_find_tag_prefix_exact_still_matches() {
        let tags = vec![Tag {
            name: "foo".to_string(),
            filename: "a.rs".to_string(),
            address: TagAddress::Line(1),
        }];
        let t = find_tag_prefix(&tags, "foo").unwrap();
        assert_eq!(t.name, "foo");
    }

    #[test]
    fn test_parse_tags_file() {
        let content = "!_TAG_FILE_FORMAT\t2\nfoo\tsrc/foo.rs\t/^fn foo(/\nbar\tsrc/bar.rs\t10\n";
        let path = write_temp("rvi_test_tags_roundtrip.tags", content);
        let tags = parse_tags_file(path.to_str().unwrap()).unwrap();
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].name, "foo");
        assert_eq!(tags[0].address, TagAddress::Pattern("^fn foo(".to_string()));
        assert_eq!(tags[1].name, "bar");
        assert_eq!(tags[1].address, TagAddress::Line(10));
    }
}

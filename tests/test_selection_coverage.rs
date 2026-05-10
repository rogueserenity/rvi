//! Integration tests for `Selection` coverage gaps.
//!
//! These tests target behaviour that is not directly exercised by the unit
//! tests in `src/buffer/selection.rs`, including:
//! - Line selection `contains_cursor` (both directions).
//! - Multi-line character selection bounds (first/middle/last row gating).
//! - `text()` clamping when columns or rows exceed line/buffer length.
//! - Block selection text on a single row, single column, and rows where
//!   `start.col` exceeds line length.
//! - `normalize()` no-op cases.
//! - Reversed selections going through `normalize()` inside `text()`.

use rvi::buffer::selection::{Selection, SelectionType};
use rvi::buffer::{Buffer, Cursor};

#[test]
fn test_normalize_already_ordered() {
    let s = Selection::new(
        Cursor::new(0, 0),
        Cursor::new(2, 5),
        SelectionType::Character,
    );
    let n = s.normalize();
    assert_eq!(n.start, Cursor::new(0, 0));
    assert_eq!(n.end, Cursor::new(2, 5));
}

#[test]
fn test_normalize_reversed_rows() {
    let s = Selection::new(
        Cursor::new(3, 1),
        Cursor::new(1, 4),
        SelectionType::Character,
    );
    let n = s.normalize();
    assert_eq!(n.start, Cursor::new(1, 4));
    assert_eq!(n.end, Cursor::new(3, 1));
}

#[test]
fn test_normalize_line_selection_swaps_rows() {
    let s = Selection::new(Cursor::new(5, 0), Cursor::new(2, 0), SelectionType::Line);
    let n = s.normalize();
    assert_eq!(n.start.row, 2);
    assert_eq!(n.end.row, 5);
    assert_eq!(n.selection_type, SelectionType::Line);
}

#[test]
fn test_block_normalize_already_ordered() {
    let s = Selection::new(Cursor::new(0, 1), Cursor::new(2, 3), SelectionType::Block);
    let n = s.normalize();
    assert_eq!(n.start, Cursor::new(0, 1));
    assert_eq!(n.end, Cursor::new(2, 3));
}

#[test]
fn test_block_normalize_bottom_left_anchor() {
    let s = Selection::new(Cursor::new(2, 1), Cursor::new(0, 3), SelectionType::Block);
    let n = s.normalize();
    assert_eq!(n.start, Cursor::new(0, 1));
    assert_eq!(n.end, Cursor::new(2, 3));
}

#[test]
fn test_contains_cursor_character_multiline() {
    let s = Selection::new(
        Cursor::new(1, 2),
        Cursor::new(3, 4),
        SelectionType::Character,
    );
    // First row: only cols >= 2 are inside.
    assert!(!s.contains_cursor(&Cursor::new(1, 1)));
    assert!(s.contains_cursor(&Cursor::new(1, 2)));
    // Middle row: any column inside.
    assert!(s.contains_cursor(&Cursor::new(2, 0)));
    assert!(s.contains_cursor(&Cursor::new(2, 999)));
    // Last row: only cols <= 4 are inside.
    assert!(s.contains_cursor(&Cursor::new(3, 4)));
    assert!(!s.contains_cursor(&Cursor::new(3, 5)));
    // Outside row range.
    assert!(!s.contains_cursor(&Cursor::new(0, 2)));
    assert!(!s.contains_cursor(&Cursor::new(4, 0)));
}

#[test]
fn test_contains_cursor_line_selection() {
    let s = Selection::new(Cursor::new(2, 0), Cursor::new(4, 0), SelectionType::Line);
    assert!(s.contains_cursor(&Cursor::new(2, 0)));
    assert!(s.contains_cursor(&Cursor::new(3, 100)));
    assert!(s.contains_cursor(&Cursor::new(4, 50)));
    assert!(!s.contains_cursor(&Cursor::new(1, 0)));
    assert!(!s.contains_cursor(&Cursor::new(5, 0)));
}

#[test]
fn test_contains_cursor_line_selection_reversed() {
    // Reversed selection: normalize() handles ordering inside contains_cursor.
    let s = Selection::new(Cursor::new(4, 0), Cursor::new(2, 0), SelectionType::Line);
    assert!(s.contains_cursor(&Cursor::new(3, 0)));
    assert!(!s.contains_cursor(&Cursor::new(1, 0)));
}

#[test]
fn test_text_character_single_line_clamps_end_col() {
    // end.col larger than line length must be clamped to avoid panics.
    let buffer = Buffer::from_string("hi".to_string());
    let s = Selection::new(
        Cursor::new(0, 0),
        Cursor::new(0, 100),
        SelectionType::Character,
    );
    assert_eq!(s.text(&buffer), "hi");
}

#[test]
fn test_text_character_start_col_past_line_length() {
    // start.col > line.len() is clamped, yielding empty content.
    let buffer = Buffer::from_string("hi".to_string());
    let s = Selection::new(
        Cursor::new(0, 100),
        Cursor::new(0, 200),
        SelectionType::Character,
    );
    assert_eq!(s.text(&buffer), "");
}

#[test]
fn test_text_character_unicode_inclusive_end() {
    // CJK chars are 3 bytes; end col 3 points at the second char's start.
    // Inclusive end advances past it via next_grapheme_boundary to byte 6.
    let buffer = Buffer::from_string("\u{4F60}\u{597D}\u{4E16}".to_string());
    let s = Selection::new(
        Cursor::new(0, 0),
        Cursor::new(0, 3),
        SelectionType::Character,
    );
    assert_eq!(s.text(&buffer), "\u{4F60}\u{597D}");
}

#[test]
fn test_text_line_single_row() {
    let buffer = Buffer::from_string("alpha\nbeta\ngamma".to_string());
    let s = Selection::new(Cursor::new(1, 0), Cursor::new(1, 0), SelectionType::Line);
    assert_eq!(s.text(&buffer), "beta");
}

#[test]
fn test_text_line_clamped_to_buffer_end() {
    // end.row beyond the buffer is clamped via saturating_sub to last valid row.
    let buffer = Buffer::from_string("a\nb".to_string());
    let s = Selection::new(Cursor::new(0, 0), Cursor::new(99, 0), SelectionType::Line);
    assert_eq!(s.text(&buffer), "a\nb");
}

#[test]
fn test_text_line_all_lines() {
    let buffer = Buffer::from_string("one\ntwo\nthree".to_string());
    let s = Selection::new(Cursor::new(0, 0), Cursor::new(2, 0), SelectionType::Line);
    assert_eq!(s.text(&buffer), "one\ntwo\nthree");
}

#[test]
fn test_text_block_single_row() {
    let buffer = Buffer::from_string("abcdef".to_string());
    let s = Selection::new(Cursor::new(0, 1), Cursor::new(0, 3), SelectionType::Block);
    // [1..next_grapheme_boundary("abcdef", 3)=4] = "bcd"
    assert_eq!(s.text(&buffer), "bcd\n");
}

#[test]
fn test_text_block_zero_width_single_column() {
    // start.col == end.col selects a single column (one grapheme per row).
    let buffer = Buffer::from_string("abc\ndef\nghi".to_string());
    let s = Selection::new(Cursor::new(0, 1), Cursor::new(2, 1), SelectionType::Block);
    assert_eq!(s.text(&buffer), "b\ne\nh\n");
}

#[test]
fn test_text_block_start_col_past_short_line() {
    // Lines shorter than start.col yield empty content; a '\n' is still emitted for each row.
    let buffer = Buffer::from_string("a\nlonger\nb".to_string());
    let s = Selection::new(Cursor::new(0, 3), Cursor::new(2, 5), SelectionType::Block);
    // Row 0 "a"     (len=1): s=1, e=1 -> "" + '\n'
    // Row 1 "longer"(len=6): s=3, e=6 -> "ger" + '\n'
    // Row 2 "b"     (len=1): s=1, e=1 -> "" + '\n'
    assert_eq!(s.text(&buffer), "\nger\n\n");
}

#[test]
fn test_text_character_multiline_last_line_clamped() {
    // end.col on the last line larger than that line's length should clamp.
    let buffer = Buffer::from_string("abcde\nfg".to_string());
    let s = Selection::new(
        Cursor::new(0, 2),
        Cursor::new(1, 100),
        SelectionType::Character,
    );
    // First line: "cde"; last line clamped -> next_grapheme_boundary("fg", 2)=2 -> "fg"
    assert_eq!(s.text(&buffer), "cde\nfg");
}

#[test]
fn test_text_character_reversed_selection_normalized_in_text() {
    // Reversed (start > end): text() should normalize internally.
    let buffer = Buffer::from_string("hello".to_string());
    let s = Selection::new(
        Cursor::new(0, 4),
        Cursor::new(0, 1),
        SelectionType::Character,
    );
    assert_eq!(s.text(&buffer), "ello");
}

#[test]
fn test_selection_clone_and_eq() {
    // Coverage for derived Clone/PartialEq impls.
    let s1 = Selection::new(
        Cursor::new(0, 0),
        Cursor::new(1, 2),
        SelectionType::Character,
    );
    let s2 = s1.clone();
    assert_eq!(s1, s2);

    let s3 = Selection::new(Cursor::new(0, 0), Cursor::new(1, 2), SelectionType::Line);
    assert_ne!(s1, s3);
}

#[test]
fn test_selection_type_eq_and_debug() {
    assert_eq!(SelectionType::Character, SelectionType::Character);
    assert_ne!(SelectionType::Character, SelectionType::Block);
    let _ = format!("{:?}", SelectionType::Line);
}

#[test]
fn test_selection_type_copy() {
    // SelectionType is Copy: assignment does not move.
    let t = SelectionType::Block;
    let _t2 = t;
    let _t3 = t;
    assert_eq!(t, SelectionType::Block);
}

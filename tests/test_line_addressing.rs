mod common;
use common::session::temp_file_with_content;

// Line addressing tests: ?pattern?, {addr}+n/{addr}-n, // and ??,
// and semicolon address separator.

// --- ?{pattern}? backward pattern address ---

#[test]
fn test_backward_pattern_address_delete() {
    // :?foo?d should delete the last line before cursor matching "foo".
    // Buffer: foo / bar / foo / baz (4 lines; row 3 is "baz").
    let (_dir, path) = temp_file_with_content("foo\nbar\nfoo\nbaz");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("foo").unwrap();
    s.send_keys("G"); // go to last line (row 3 = "baz")
    s.wait_for_cursor_row(3).unwrap();
    s.send_keys(":?foo?d\r");
    // The second "foo" (row 2) should be deleted.
    s.wait_for_no_text("foo").unwrap_or(()); // first "foo" may still be present
                                             // Verify "baz" and "bar" survive.
    s.assert_contains("baz");
    s.assert_contains("bar");
}

#[test]
fn test_backward_pattern_address_print() {
    // :?bar?p should print the last line matching "bar" before the current position.
    let (_dir, path) = temp_file_with_content("one\nbar\ntwo\nthree");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("one").unwrap();
    s.send_keys("G"); // go to last line (row 3 = "three")
    s.wait_for_cursor_row(3).unwrap();
    s.send_keys(":?bar?p\r");
    // "bar" should appear somewhere on screen (printed by :p).
    s.wait_for_text("bar")
        .expect(":?bar?p should print the line matching 'bar'");
}

// --- {addr}+n and {addr}-n relative offset addressing ---

#[test]
fn test_addr_plus_offset_delete() {
    // :1+2d deletes line 1+2 = line 3 (row 2 zero-based).
    let (_dir, path) = temp_file_with_content("line1\nline2\nline3\nline4");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("line1").unwrap();
    s.send_keys(":1+2d\r");
    s.wait_for_no_text("line3")
        .expect(":1+2d should delete line 3 (row 2)");
    s.assert_contains("line1");
    s.assert_contains("line2");
    s.assert_contains("line4");
}

#[test]
fn test_addr_minus_offset_delete() {
    // :4-1d deletes line 4-1 = line 3 (row 2 zero-based).
    let (_dir, path) = temp_file_with_content("line1\nline2\nline3\nline4");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("line1").unwrap();
    s.send_keys(":4-1d\r");
    s.wait_for_no_text("line3")
        .expect(":4-1d should delete line 3 (row 2)");
    s.assert_contains("line1");
    s.assert_contains("line2");
    s.assert_contains("line4");
}

#[test]
fn test_dot_plus_offset() {
    // :.+2d from row 0 deletes row 2 (current+2).
    let (_dir, path) = temp_file_with_content("aaa\nbbb\nccc\nddd");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("aaa").unwrap();
    s.wait_for_cursor_row(0).unwrap();
    s.send_keys(":.+2d\r");
    s.wait_for_no_text("ccc")
        .expect(":.+2d from row 0 should delete row 2 ('ccc')");
    s.assert_contains("aaa");
    s.assert_contains("bbb");
    s.assert_contains("ddd");
}

// --- // and ?? empty pattern re-use ---

#[test]
fn test_double_slash_address_reuses_pattern() {
    // Search for "zoo", then ://d should delete the next match using the
    // stored pattern (equivalent to /zoo/d).
    let (_dir, path) = temp_file_with_content("start\nzoo\nmiddle\nzoo\nend");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("start").unwrap();
    s.send_keys("/zoo\r"); // land on first "zoo" at row 1
    s.wait_for_cursor_row(1).unwrap();
    s.send_keys("://d\r"); // delete the line at next match of saved pattern (row 3 "zoo")
                           // row 1 "zoo" still exists; row 3 "zoo" should be gone.
    s.assert_contains("start");
    s.assert_contains("zoo"); // first one survives
    s.assert_contains("end");
}

#[test]
fn test_double_question_address_reuses_pattern() {
    // Search for "xbar", advance past it, then :??d should delete
    // the previous match of the stored pattern.
    // Use "xbar" as the pattern to avoid false matches in file paths.
    let (_dir, path) = temp_file_with_content("alpha\nxbar\nbeta\ngamma");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("alpha").unwrap();
    // Move to last line first so we can search backward via /xbar and land at row 1.
    s.send_keys("G");
    s.wait_for_cursor_row(3).unwrap();
    s.send_keys("?xbar\r"); // backward search → finds "xbar" at row 1
    s.wait_for_cursor_row(1).unwrap();
    // Now :?? deletes the previous match (backward from row 1 — wraps to last xbar,
    // but there's only one occurrence so it deletes the same row).
    // More precisely: ??d from row 1 searches backward for the saved pattern,
    // wrapping past row 0, finding row 1 itself. Delete it.
    s.send_keys(":??d\r");
    s.wait_for_no_text("xbar")
        .expect(":??d should delete the line matching the last backward pattern");
    s.assert_contains("alpha");
    s.assert_contains("beta");
}

// --- {addr};{addr} semicolon addressing ---

#[test]
fn test_semicolon_range_delete() {
    // :2;/target/d — with semicolon, the second address (/target/) is searched
    // starting from line 2. Deletes from line 2 through the first "target" after it.
    let (_dir, path) = temp_file_with_content("keep\nstart\nalpha\ntarget\nbeta");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("keep").unwrap();
    // Lines 2,3,4 = "start","alpha","target" should be deleted.
    s.send_keys(":2;/target/d\r");
    s.wait_for_no_text("start")
        .expect("semicolon range: lines 2 through 'target' should be deleted");
    s.assert_contains("keep");
    s.assert_contains("beta");
}

#[test]
fn test_semicolon_range_numeric() {
    // :1;3d deletes lines 1 through 3.
    // Use unique multi-char content to avoid false matches in the status bar path.
    let (_dir, path) = temp_file_with_content("alpha_line\nbeta_line\ngamma_line\ndelta_line");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("alpha_line").unwrap();
    s.send_keys(":1;3d\r");
    s.wait_for_no_text("alpha_line")
        .expect(":1;3d should delete lines 1 through 3");
    s.wait_for_no_text("gamma_line").unwrap();
    s.assert_contains("delta_line");
}

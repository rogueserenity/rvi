mod common;
use common::session::temp_file_with_content;

// --- ma / `a: set mark and jump to exact position ---

#[test]
fn test_set_and_jump_mark() {
    // File has three lines; cursor starts at row 0, col 0.
    // Move to row 1, col 3, set mark 'a', move away, then jump back with `a.
    let (_dir, path) = temp_file_with_content("hello world\nfoo bar baz\nthird line\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello world").unwrap();
    s.wait_for_cursor(0, 0).unwrap();

    // Move to row 1, col 3 and set mark 'a'
    s.send_keys("j");
    s.wait_for_cursor(1, 0).unwrap();
    s.send_keys("lll"); // col 3 → ' '
    s.wait_for_cursor(1, 3).unwrap();
    s.send_keys("ma"); // set mark 'a' at (row 1, col 3)

    // Move to a different position
    s.send_keys("j$"); // row 2, end of line
    s.wait_for_cursor_row(2).unwrap();

    // Jump back to mark 'a' exact position
    s.send_keys("`a");
    s.wait_for_cursor(1, 3)
        .expect("`a should jump to exact row and col of mark 'a'");
}

// --- ma / 'a: jump to first non-blank of mark's line ---

#[test]
fn test_jump_to_mark_line() {
    // Row 0 has leading spaces so col 0 != first non-blank col.
    // Set mark 'a' somewhere on row 0, move away, then 'a jumps to first
    // non-blank of that line (col 2, the 'h' in "  hello").
    let (_dir, path) = temp_file_with_content("  hello world\nsecond line\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello world").unwrap();

    // Cursor is at row 0, col 0. Set mark 'a' here (on this line).
    s.send_keys("ma");

    // Move to row 1, col 4 — clearly away from the mark's line.
    s.send_keys("j");
    s.wait_for_cursor(1, 0).unwrap();
    s.send_keys("llll");
    s.wait_for_cursor(1, 4).unwrap();

    // 'a should jump to first non-blank of row 0, which is col 2 ('h')
    s.send_keys("'a");
    s.wait_for_cursor(0, 2)
        .expect("'a should jump to first non-blank of mark's line (col 2, skipping two spaces)");
}

// --- `` : jump back to position before last jump ---

#[test]
fn test_mark_before_last_jump() {
    // Set mark 'a' on row 2, then jump to it with `a (which pushes the
    // pre-jump position onto the jump stack). Then `` should return to the
    // position we were at before the `a jump.
    let (_dir, path) = temp_file_with_content("line one\nline two\nline three\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("line one").unwrap();
    s.wait_for_cursor(0, 0).unwrap();

    // Move to row 2 and set mark 'a'
    s.send_keys("jj");
    s.wait_for_cursor(2, 0).unwrap();
    s.send_keys("ma");

    // Return to row 0, col 4 — this will be our pre-jump position
    s.send_keys("gg");
    s.wait_for_cursor(0, 0).unwrap();
    s.send_keys("llll");
    s.wait_for_cursor(0, 4).unwrap();

    // Jump to mark 'a' — this pushes (row 0, col 4) onto the jump stack
    s.send_keys("`a");
    s.wait_for_cursor(2, 0).unwrap();

    // `` should return to the pre-jump position: row 0, col 4
    s.send_keys("``");
    s.wait_for_cursor(0, 4)
        .expect("`` should jump back to position before the last jump");
}

// --- mark in ex range: 'a,'ad deletes from mark to mark ---

#[test]
fn test_mark_ex_range() {
    // File has four lines. Set mark 'a' on line 2 (row index 1), then
    // move to line 1 (row index 0) and run :'a,'ad to delete line 2.
    let (_dir, path) = temp_file_with_content("first line\nsecond line\nthird line\nfourth line\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("first line").unwrap();

    // Move to row 1 ("second line") and set mark 'a'
    s.send_keys("j");
    s.wait_for_cursor(1, 0).unwrap();
    s.send_keys("ma");

    // Move back to row 0
    s.send_keys("k");
    s.wait_for_cursor(0, 0).unwrap();

    // Use 'a,'a as an ex range to delete the marked line
    s.send_keys(":'a,'ad\r");

    // "second line" should be gone; the other lines remain
    s.wait_for_no_text("second line")
        .expect(":'a,'ad should delete the line that mark 'a' points to");
    s.assert_contains("first line");
    s.assert_contains("third line");
}

// --- `< / `> — jump to exact start/end position of last visual selection ---

#[test]
fn test_visual_mark_jump_start() {
    // Use a 3-line file; select on row 1 so we can cleanly move away to row 0.
    // `< (backtick + <) jumps to the exact byte position of the selection start.
    let (_dir, path) = temp_file_with_content("first\nsecond\nthird\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("first").unwrap();
    s.wait_for_cursor(0, 0).unwrap();
    // Move to row 1 col 0 and enter visual, then select a few chars
    s.send_keys("j");
    s.wait_for_cursor_row(1).unwrap();
    s.send_keys("v");
    s.wait_for_status("VISUAL").unwrap();
    s.send_keys("ll"); // extend selection: cursor at (1, 2), anchor at (1, 0)
    s.send_keys("\x1b"); // exit — sets `< mark at (1, 0), `> mark at (1, 2)
    s.wait_for_status("NORMAL").unwrap();
    // Move away to row 0
    s.send_keys("gg");
    s.wait_for_cursor(0, 0).unwrap();
    // `< should jump to exact start of last visual selection: row 1, col 0
    s.send_keys("`<");
    s.wait_for_cursor(1, 0)
        .expect("`< should jump to exact start of visual selection (row 1, col 0)");
}

#[test]
fn test_visual_mark_jump_end() {
    // Use a 3-line file; select on row 1, then `> jumps to exact end position.
    let (_dir, path) = temp_file_with_content("first\nsecond\nthird\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("first").unwrap();
    s.wait_for_cursor(0, 0).unwrap();
    // Move to row 1 col 0 and enter visual, extend selection to col 2
    s.send_keys("j");
    s.wait_for_cursor_row(1).unwrap();
    s.send_keys("v");
    s.wait_for_status("VISUAL").unwrap();
    s.send_keys("ll"); // cursor at (1, 2), anchor at (1, 0)
    s.send_keys("\x1b"); // exit — `> mark at (1, 2)
    s.wait_for_status("NORMAL").unwrap();
    // Move away to row 0
    s.send_keys("gg");
    s.wait_for_cursor(0, 0).unwrap();
    // `> should jump to exact end of last visual selection: row 1, col 2
    s.send_keys("`>");
    s.wait_for_cursor(1, 2)
        .expect("`> should jump to exact end of visual selection (row 1, col 2)");
}

// --- '< and '> as ex range addresses (C1) ---

#[test]
fn test_visual_marks_as_ex_range() {
    // After a visual selection, :'<,'> should be usable as an ex range.
    // Select lines 1–2 (rows 1 and 2), then :'<,'>d should delete them.
    let (_dir, path) = temp_file_with_content("alpha\nbeta\ngamma\ndelta\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("alpha").unwrap();
    s.send_keys("j");               // row 1 ("beta")
    s.wait_for_cursor_row(1).unwrap();
    s.send_keys("V");               // visual line
    s.wait_for_status("VISUAL LINE").unwrap();
    s.send_keys("j");               // extend to row 2 ("gamma")
    s.send_keys("\x1b");            // exit visual — sets '< to row 1, '> to row 2
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys(":'<,'>d\r");
    s.wait_for_no_text("beta")
        .expect(":'<,'>d should delete the visually selected lines");
    s.wait_for_no_text("gamma")
        .expect(":'<,'>d should delete the visually selected lines");
    s.wait_for_text("alpha").unwrap();
    s.wait_for_text("delta").unwrap();
}

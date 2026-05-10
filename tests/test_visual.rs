mod common;
use common::session::temp_file_with_content;

#[test]
fn test_visual_char_delete() {
    let (_dir, path) = temp_file_with_content("hello world");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello world").unwrap();
    s.send_keys("v4l");
    s.wait_for_status("VISUAL").unwrap();
    s.send_keys("d");
    s.wait_for_no_text("hello")
        .expect("d in visual char mode deletes the selection");
    s.assert_contains("world");
}

#[test]
fn test_visual_line_delete() {
    let (_dir, path) = temp_file_with_content("keep\ndelete\nkeep");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("delete").unwrap();
    s.send_keys("j");
    s.wait_for_cursor(1, 0).unwrap();
    s.send_keys("Vd");
    s.wait_for_no_text("delete")
        .expect("V + d deletes the selected line");
    s.assert_contains("keep");
}

#[test]
fn test_visual_yank_put() {
    let (_dir, path) = temp_file_with_content("hello\nworld");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    // Select "hello" and yank; sync through VISUAL to ensure yank completes
    s.send_keys("v");
    s.wait_for_status("VISUAL").unwrap();
    s.send_keys("4ly");
    s.wait_for_status("NORMAL").unwrap();
    // Move to end of last line and paste after last char
    s.send_keys("G$p");
    s.wait_for_text("worldhello")
        .expect("p pastes the yanked 'hello' after the last char of 'world'");
}

#[test]
fn test_visual_line_yank_put() {
    let (_dir, path) = temp_file_with_content("alpha\nbeta");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("alpha").unwrap();
    // Sync through VISUAL LINE to ensure yank completes before checking
    s.send_keys("V");
    s.wait_for_status("VISUAL LINE").unwrap();
    s.send_keys("y");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys("p"); // put "alpha" as new line below row 0
                      // After linewise put, cursor moves to the newly inserted line (row 1)
    s.wait_for_cursor(1, 0)
        .expect("cursor moves to the newly put line");
    let r1 = s.screen_row(1);
    assert!(
        r1.contains("alpha"),
        "p after V yank puts 'alpha' as a new line below, got row 1: {r1:?}"
    );
}

#[test]
fn test_visual_change() {
    let (_dir, path) = temp_file_with_content("hello world");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello world").unwrap();
    s.send_keys("v4lc");
    s.wait_for_status("INSERT").unwrap();
    s.send_keys("goodbye\x1b");
    s.wait_for_text("goodbye world")
        .expect("c in visual mode replaces selection with typed text");
}

#[test]
fn test_visual_toggle_case() {
    let (_dir, path) = temp_file_with_content("hello WORLD");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello WORLD").unwrap();
    s.send_keys("v4l~");
    s.wait_for_text("HELLO WORLD")
        .expect("~ toggles case of selected characters");
}

#[test]
fn test_visual_indent() {
    let (_dir, path) = temp_file_with_content("one\ntwo\nthree");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("one").unwrap();
    s.send_keys("V");
    s.wait_for_status("VISUAL LINE").unwrap();
    s.send_keys("2j>");
    s.wait_for_status("NORMAL").unwrap();
    let row0 = s.screen_row(0);
    assert!(
        row0.starts_with('\t') || row0.starts_with(' '),
        "row 0 should be indented after >, got: {row0:?}"
    );
}

#[test]
fn test_visual_dedent() {
    let (_dir, path) = temp_file_with_content("\tone\n\ttwo");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("one").unwrap();
    s.send_keys("V");
    s.wait_for_status("VISUAL LINE").unwrap();
    s.send_keys("j<");
    s.wait_for_status("NORMAL").unwrap();
    let row0 = s.screen_row(0);
    assert!(
        row0.starts_with('o'),
        "row 0 should start with 'o' after dedent, got: {row0:?}"
    );
}

#[test]
fn test_visual_join_lines() {
    let (_dir, path) = temp_file_with_content("one\ntwo\nthree");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("one").unwrap();
    s.send_keys("VjJ");
    s.wait_for_text("one two")
        .expect("J joins selected lines with a space");
}

#[test]
fn test_visual_command_line_range() {
    // ':' in visual mode pre-fills '<,'> so the ex command runs only on the selection.
    let (_dir, path) = temp_file_with_content("foo\nbar\nfoo");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("foo").unwrap();
    s.send_keys("Vj"); // select first two lines
    s.wait_for_status("VISUAL LINE").unwrap();
    s.send_keys(":s/foo/baz/\r");
    s.wait_for_text("baz").unwrap();
    s.assert_contains("foo"); // third line untouched
    s.assert_contains("baz"); // first line changed
}

#[test]
fn test_visual_backward_selection() {
    // Selecting backward (anchor after cursor) should still delete correctly.
    let (_dir, path) = temp_file_with_content("hello world");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello world").unwrap();
    s.send_keys("4l"); // cursor at col 4 ('o' of "hello")
    s.wait_for_cursor(0, 4).unwrap();
    s.send_keys("v0"); // select backward to col 0
    s.wait_for_status("VISUAL").unwrap();
    s.send_keys("d");
    s.wait_for_no_text("hello")
        .expect("backward visual selection deletes correctly");
    s.assert_contains("world");
}

#[test]
fn test_visual_block_insert() {
    // Ctrl-V block select a column, then I inserts text on every selected row.
    let (_dir, path) = temp_file_with_content("one\ntwo\nthree");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("one").unwrap();
    s.send_keys("\x162j"); // Ctrl-V + 2j: block select col 0 across 3 rows
    s.wait_for_status("VISUAL BLOCK").unwrap();
    s.send_keys("I> \x1b"); // insert "> " at start of each row
    s.wait_for_text("> one")
        .expect("I in visual block inserts text on first selected row");
    s.wait_for_text("> two")
        .expect("I in visual block inserts text on second selected row");
    s.wait_for_text("> three")
        .expect("I in visual block inserts text on third selected row");
}

#[test]
fn test_visual_put_replaces_selection() {
    // p in visual mode replaces the selection with register contents.
    let (_dir, path) = temp_file_with_content("hello world");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello world").unwrap();
    // Move to "world" (col 6) and yank it; sync through VISUAL to ensure yank completes
    s.send_keys("w");
    s.wait_for_cursor(0, 6).unwrap();
    s.send_keys("v");
    s.wait_for_status("VISUAL").unwrap();
    s.send_keys("4ly"); // select "world" (4 rights: cols 6-10), then yank
    s.wait_for_status("NORMAL").unwrap();
    // Go back to col 0
    s.send_keys("0");
    s.wait_for_cursor(0, 0).unwrap();
    // Select "hello" (cols 0-4) and replace with register
    s.send_keys("v");
    s.wait_for_status("VISUAL").unwrap();
    s.send_keys("4lp"); // select "hello" (4 rights: cols 0-4), then put
    s.wait_for_text("world world")
        .expect("p in visual mode replaces selection with register contents");
}

#[test]
fn test_visual_uppercase() {
    let (_dir, path) = temp_file_with_content("hello world");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello world").unwrap();
    s.send_keys("v4lgU");
    s.wait_for_text("HELLO world")
        .expect("gU in visual mode uppercases the selection");
}

#[test]
fn test_visual_lowercase() {
    let (_dir, path) = temp_file_with_content("HELLO world");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("HELLO world").unwrap();
    s.send_keys("v4lgu");
    s.wait_for_text("hello world")
        .expect("gu in visual mode lowercases the selection");
}

#[test]
fn test_visual_block_delete() {
    // Ctrl-V block select first column across 3 rows, then d deletes that column.
    let (_dir, path) = temp_file_with_content("one\ntwo\nthree");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("one").unwrap();
    s.send_keys("\x162j"); // Ctrl-V + 2j: block select col 0 across 3 rows
    s.wait_for_status("VISUAL BLOCK").unwrap();
    s.send_keys("d"); // delete the selected column
    s.wait_for_text("ne")
        .expect("d in visual block removes the selected column from each row");
    s.assert_contains("wo");
    s.assert_contains("hree");
}

#[test]
fn test_visual_block_change() {
    // c in visual block: deletes block from ALL rows, inserts typed text at top-left.
    // Col 0 is removed from both rows; "X" is typed only at the first row.
    let (_dir, path) = temp_file_with_content("one\ntwo");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("one").unwrap();
    s.send_keys("\x16j"); // Ctrl-V + j: block select col 0 across 2 rows
    s.wait_for_status("VISUAL BLOCK").unwrap();
    s.send_keys("c"); // change block → delete col from all rows, enter insert mode
    s.wait_for_status("INSERT").unwrap();
    s.send_keys("X\x1b"); // type "X" and leave insert mode
                          // Row 0: col 0 deleted, "X" inserted → "Xne"
                          // Row 1: col 0 deleted only → "wo"
    s.wait_for_text("Xne")
        .expect("c in visual block: row 0 gets typed text after block delete");
    s.assert_contains("wo"); // row 1 has its block column deleted, no insertion
}

#[test]
fn test_visual_block_yank_ragged_lines() {
    // Block yank over ragged lines: cols 5-8 across "long line here", "hi", "long line here".
    // The short middle line ("hi") has no chars at cols 5-8; when put, that row should
    // contribute nothing — the pasted block should contain only the two long-line slices.
    let (_dir, path) = temp_file_with_content("long line here\nhi\nlong line here\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("long line here").unwrap();
    s.send_keys("5l"); // col 5
    s.wait_for_cursor(0, 5).unwrap();
    s.send_keys("\x162j3l"); // Ctrl-V + 2j + 3l: cols 5-8 across rows 0-2
    s.wait_for_status("VISUAL BLOCK").unwrap();
    s.send_keys("y"); // yank block
    s.wait_for_status("NORMAL").unwrap();
    // Move to end of buffer and put the block below
    s.send_keys("Gp");
    s.wait_for_status("NORMAL").unwrap();
    // The two long lines should contribute "line" (cols 5-8); the short line contributes nothing
    s.assert_contains("line");
}

#[test]
fn test_visual_block_change_ragged_lines() {
    // c in visual block over ragged lines: cols 5-8 across "long line here", "hi", "long line here".
    // The short middle line ("hi") falls outside the block columns and must remain unchanged.
    let (_dir, path) = temp_file_with_content("long line here\nhi\nlong line here");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("long line here").unwrap();
    s.send_keys("5l"); // col 5
    s.wait_for_cursor(0, 5).unwrap();
    s.send_keys("\x162j3l"); // Ctrl-V + 2j + 3l: cols 5-8 across rows 0-2
    s.wait_for_status("VISUAL BLOCK").unwrap();
    s.send_keys("c"); // change block
    s.wait_for_status("INSERT").unwrap();
    s.send_keys("X\x1b"); // replace block with "X"
    s.wait_for_status("NORMAL").unwrap();
    // Short line "hi" should be unaffected
    s.assert_contains("hi");
    // Long lines should have the block replaced with "X"
    s.assert_contains("long X here");
}

#[test]
fn test_visual_block_mixed_length_lines() {
    // When the block column extends past the end of a shorter line,
    // that line should be skipped / unchanged during the operation.
    let (_dir, path) = temp_file_with_content("long line here\nhi\nlong line here");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("long line here").unwrap();
    // Move to col 5 ('l' in "line"), select block down 2 rows over cols 5-8
    s.send_keys("5l"); // cursor at col 5
    s.wait_for_cursor(0, 5).unwrap();
    s.send_keys("\x162l2j"); // Ctrl-V + 2l + 2j: cols 5-7 across 3 rows
    s.wait_for_status("VISUAL BLOCK").unwrap();
    s.send_keys("d"); // delete cols 5-7 from rows 0, 1, 2
                      // Row 1 ("hi") has no col 5-7 → stays unchanged
    s.wait_for_status("NORMAL").unwrap();
    s.assert_contains("hi"); // short row unaffected
}

#[test]
fn test_visual_put_unnamed_register_holds_deleted_text() {
    // After `Vp` (visual-line put), the unnamed register should hold the
    // *deleted* line, not the pasted text. This enables the cycling idiom.
    let (_dir, path) = temp_file_with_content("aaa\nbbb\nccc\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("aaa").unwrap();
    // Yank line 0 ("aaa") with `yy`.
    s.send_keys("yy");
    s.wait_for_status("NORMAL").unwrap();
    // Visual-line select line 1 ("bbb") and put → line 1 replaced with "aaa".
    s.send_keys("j");
    s.wait_for_cursor_row(1).unwrap();
    s.send_keys("V");
    s.wait_for_status("VISUAL LINE").unwrap();
    s.send_keys("p");
    s.wait_for_status("NORMAL").unwrap();
    // Line 1 should now be "aaa"; "bbb" should be gone.
    s.wait_for_no_text("bbb")
        .expect("Vp: line 1 should be replaced by the yanked line 'aaa'");
    // Now `p` should paste "bbb" (the deleted line), not "aaa" again.
    // `p` in normal mode inserts below the current line.
    s.send_keys("p");
    s.wait_for_text("bbb")
        .expect("after Vp, unnamed register should hold the deleted line 'bbb'");
}

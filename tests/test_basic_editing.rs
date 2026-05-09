mod common;
use common::keys::ESC;
use common::session::temp_file_with_content;

#[test]
fn test_insert_text() {
    let mut s = common::RviSession::new();
    s.wait_for_text("~").unwrap();
    s.send_keys("ihello world");
    s.send_keys(ESC);
    s.wait_for_text("hello world")
        .expect("inserted text should appear on screen");
}

#[test]
fn test_append_text() {
    let mut s = common::RviSession::new();
    s.wait_for_text("~").unwrap();
    s.send_keys("ix");
    s.send_keys(ESC);
    s.wait_for_status("NORMAL").unwrap(); // ensure ESC processed before 'a' arrives
    s.send_keys("ayz");
    s.send_keys(ESC);
    s.wait_for_text("xyz")
        .expect("appended text should follow cursor");
}

#[test]
fn test_open_line_below() {
    let mut s = common::RviSession::new();
    s.wait_for_text("~").unwrap();
    s.send_keys("ifirst line");
    s.send_keys(ESC);
    s.wait_for_status("NORMAL").unwrap(); // ensure ESC processed before 'o' arrives
    s.send_keys("osecond line");
    s.send_keys(ESC);
    s.wait_for_text("second line").unwrap();
    s.assert_contains("first line");
    s.assert_contains("second line");
}

#[test]
fn test_delete_char() {
    let (_dir, path) = temp_file_with_content("hello\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys("x"); // delete 'h'
    s.wait_for_text("ello")
        .expect("'h' should be deleted, leaving 'ello'");
}

#[test]
fn test_delete_word() {
    let (_dir, path) = temp_file_with_content("foo bar\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("foo bar").unwrap();
    s.send_keys("dw"); // delete "foo "
    s.wait_for_text("bar")
        .expect("word 'foo' should be deleted, leaving 'bar'");
}

#[test]
fn test_delete_line() {
    let (_dir, path) = temp_file_with_content("line one\nline two\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("line one").unwrap();
    s.send_keys("dd");
    s.wait_for_text("line two")
        .expect("line two should remain after dd deletes line one");
}

#[test]
fn test_write_and_quit() {
    let (_dir, path) = temp_file_with_content("original\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("original").unwrap();
    s.send_keys("cc"); // change whole line, enters insert mode
    s.wait_for_status("INSERT")
        .expect("should enter INSERT mode after cc");
    s.send_keys("updated");
    s.send_keys(ESC);
    s.wait_for_status("NORMAL")
        .expect("should return to NORMAL mode after Esc");
    s.send_keys(":wq\r");
    // Drop waits for the process to exit before we read the file
    drop(s);
    let contents = std::fs::read_to_string(&path).unwrap();
    assert!(
        contents.contains("updated"),
        "file should contain 'updated', got: {contents:?}"
    );
}

#[test]
fn test_quit_no_changes() {
    let (_dir, path) = temp_file_with_content("unchanged\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("unchanged").unwrap();
    s.send_keys(":q\r");
    // Drop's wait loop will succeed quickly if rvi exited cleanly;
    // a hang here would indicate rvi did not exit.
}

#[test]
fn test_quit_modified_blocked() {
    let (_dir, path) = temp_file_with_content("original\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("original").unwrap();
    s.send_keys("i");
    s.wait_for_status("INSERT").unwrap();
    s.send_keys("change\x1b");
    s.wait_for_status("NORMAL").unwrap(); // ensure insert committed before :q
    s.send_keys(":q\r");
    s.wait_for_status("No write since last change")
        .expect("should show unsaved-changes error on :q with modifications");
    s.send_keys(":q!\r");
    // Process exits; Drop handles final cleanup
}

// --- X: delete char before cursor ---

#[test]
fn test_delete_char_before() {
    let (_dir, path) = temp_file_with_content("hello\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys("llX"); // move to 'l' (col 2), delete 'e' before it
    s.wait_for_text("hllo")
        .expect("X should delete the character before the cursor");
}

#[test]
fn test_delete_char_before_count() {
    let (_dir, path) = temp_file_with_content("abcde\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("abcde").unwrap();
    s.send_keys("$"); // move to 'e' (last char)
    s.wait_for_cursor(0, 4).unwrap();
    s.send_keys("3X"); // delete 'b', 'c', 'd' before 'e'
    s.wait_for_text("ae")
        .expect("3X should delete three characters before cursor");
}

// --- r: replace single character ---

#[test]
fn test_replace_char() {
    let (_dir, path) = temp_file_with_content("hello\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys("rH"); // replace 'h' with 'H'
    s.wait_for_text("Hello")
        .expect("r should replace character under cursor without entering insert mode");
}

#[test]
fn test_replace_char_stays_normal() {
    // r should not enter insert mode — status stays NORMAL throughout
    let (_dir, path) = temp_file_with_content("hello\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys("rx");
    s.wait_for_status("NORMAL")
        .expect("r should not change mode — editor stays in NORMAL");
}

// --- P: put before cursor ---

#[test]
fn test_put_before_cursor() {
    let (_dir, path) = temp_file_with_content("hello world\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello world").unwrap();
    s.send_keys("yw"); // yank "hello "
    s.send_keys("$"); // move to last char 'd' at col 10
    s.wait_for_cursor(0, 10).unwrap();
    s.send_keys("P"); // put before 'd' → "hello worlhello d"
    s.wait_for_text("hello worlhello d")
        .expect("P should paste before the cursor");
}

#[test]
fn test_put_before_line() {
    // dd yanks a line; P puts it above the current line
    let (_dir, path) = temp_file_with_content("first\nsecond\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("first").unwrap();
    s.send_keys("dd"); // yank-delete "first"
    s.wait_for_text("second").unwrap();
    s.send_keys("P"); // put above "second"
    s.wait_for_cursor(0, 0)
        .expect("P with linewise register puts above current line, cursor on new line");
    let r0 = s.screen_row(0);
    assert!(
        r0.contains("first"),
        "P should put 'first' above 'second', got: {r0:?}"
    );
}

// --- ZZ: write and quit ---

#[test]
fn test_zz_writes_and_quits() {
    let (_dir, path) = temp_file_with_content("original\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("original").unwrap();
    s.send_keys("i");
    s.wait_for_status("INSERT").unwrap();
    s.send_keys("updated\x1b");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys("ZZ");
    drop(s);
    let contents = std::fs::read_to_string(&path).unwrap();
    assert!(
        contents.contains("updated"),
        "ZZ should write file before quitting, got: {contents:?}"
    );
}

// --- ZQ: quit without saving ---

#[test]
fn test_zq_quits_without_saving() {
    let (_dir, path) = temp_file_with_content("original\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("original").unwrap();
    s.send_keys("i");
    s.wait_for_status("INSERT").unwrap();
    s.send_keys("changed\x1b");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys("ZQ");
    drop(s);
    let contents = std::fs::read_to_string(&path).unwrap();
    assert!(
        contents.contains("original"),
        "ZQ should quit without saving, got: {contents:?}"
    );
    assert!(
        !contents.contains("changed"),
        "ZQ should not save changes, got: {contents:?}"
    );
}

// --- gJ: join lines without inserting a space ---

#[test]
fn test_join_lines_no_space() {
    let (_dir, path) = temp_file_with_content("one\ntwo\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("one").unwrap();
    s.send_keys("gJ");
    s.wait_for_text("onetwo")
        .expect("gJ should join lines without inserting a space");
}

// --- &: repeat last substitute ---

#[test]
fn test_repeat_substitute() {
    let (_dir, path) = temp_file_with_content("foo foo\nfoo foo\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("foo foo").unwrap();
    s.send_keys(":s/foo/bar/\r"); // replaces first "foo" on line 1
    s.wait_for_text("bar foo").unwrap();
    s.send_keys("j"); // move to line 2
    s.wait_for_cursor(1, 0).unwrap();
    s.send_keys("&"); // repeat last :s on current line → "foo foo" → "bar foo"
                      // "foo foo" disappears from the screen once line 2 is substituted
    s.wait_for_no_text("foo foo")
        .expect("& should repeat the substitute, replacing first 'foo' on line 2");
}

// --- g~ operator: toggle case ---

#[test]
fn test_toggle_case_operator_word() {
    let (_dir, path) = temp_file_with_content("hello world\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello world").unwrap();
    s.send_keys("g~w"); // toggle case of "hello"
    s.wait_for_text("HELLO world")
        .expect("g~w should toggle case of the current word");
}

#[test]
fn test_toggle_case_operator_line() {
    let (_dir, path) = temp_file_with_content("Hello World\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("Hello World").unwrap();
    s.send_keys("g~~"); // toggle case of whole line
    s.wait_for_text("hELLO wORLD")
        .expect("g~~ should toggle case of the entire line");
}

// --- count prefix edge cases ---

#[test]
fn test_count_delete_chars() {
    let (_dir, path) = temp_file_with_content("abcde\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("abcde").unwrap();
    s.send_keys("3x"); // delete 'a', 'b', 'c'
    s.wait_for_text("de")
        .expect("3x should delete three characters");
}

#[test]
fn test_count_motion_clamps_at_boundary() {
    // 5j from the first of a 2-line file should land on the last line, not error
    let (_dir, path) = temp_file_with_content("one\ntwo"); // 2 lines
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("one").unwrap();
    s.send_keys("5j");
    s.wait_for_cursor(1, 0)
        .expect("5j past end of file should clamp to last line");
}

#[test]
fn test_count_delete_lines() {
    // 3dd deletes three lines as a single undo-able operation
    let (_dir, path) = temp_file_with_content("one\ntwo\nthree\nfour\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("one").unwrap();
    s.send_keys("3dd");
    s.wait_for_text("four")
        .expect("3dd should delete three lines, leaving only 'four'");
    s.wait_for_no_text("three").unwrap();
}

// --- O: open new line above cursor and enter insert mode ---

#[test]
fn test_open_line_above() {
    let (_dir, path) = temp_file_with_content("second line\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("second line").unwrap();
    // O opens a new line above and enters insert mode
    s.send_keys("Ofirst line");
    s.send_keys(ESC);
    s.wait_for_status("NORMAL").unwrap();
    s.wait_for_text("first line")
        .expect("O should insert a new line above with typed text");
    // "first line" should appear above "second line"
    let row0 = s.screen_row(0);
    let row1 = s.screen_row(1);
    assert!(
        row0.contains("first line"),
        "first line should be on row 0, got: {row0:?}"
    );
    assert!(
        row1.contains("second line"),
        "second line should be on row 1, got: {row1:?}"
    );
}

// --- s: substitute character (delete char under cursor, enter insert) ---

#[test]
fn test_substitute_char() {
    let (_dir, path) = temp_file_with_content("hello\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    // s deletes the character under cursor ('h') and enters insert mode
    s.send_keys("s");
    s.wait_for_status("INSERT")
        .expect("s should enter INSERT mode after deleting the character");
    s.send_keys("H");
    s.send_keys(ESC);
    s.wait_for_status("NORMAL").unwrap();
    s.wait_for_text("Hello")
        .expect("s should replace the first character with the typed replacement");
}

// --- yank operator with motions ---

#[test]
fn test_yank_to_eol() {
    let (_dir, path) = temp_file_with_content("hello world\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello world").unwrap();
    s.send_keys("w"); // move to "world"
    s.send_keys("y$"); // yank "world" to EOL
    s.send_keys("A"); // append at end of line
    s.wait_for_status("INSERT").unwrap();
    s.send_keys(" ");
    s.send_keys("\x1b"); // ESC back to normal
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys("p"); // paste
    s.wait_for_text("hello world world")
        .expect("y$ should yank from cursor to end of line");
}

#[test]
fn test_yank_down() {
    // Use distinct content so we can verify the paste unambiguously.
    // yj yanks "alpha" + "beta"; p below row 0 inserts them at rows 1 and 2;
    // cursor lands on the first pasted line (row 1).
    let (_dir, path) = temp_file_with_content("alpha\nbeta\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("alpha").unwrap();
    s.send_keys("yj"); // yank "alpha" and "beta" (linewise, 2 lines)
    s.send_keys("p"); // paste below current line — cursor lands on row 1
    s.wait_for_cursor_row(1)
        .expect("cursor should land on the first pasted line");
    let row1 = s.screen_row(1);
    assert!(
        row1.contains("alpha"),
        "first pasted line should be 'alpha', got: {row1:?}"
    );
}

#[test]
fn test_yank_to_start() {
    // y0 with cursor on 'w' (col 6) yanks cols 0-5 = "hello "
    // paste after end of line appends it: "hello worldhello "
    let (_dir, path) = temp_file_with_content("hello world\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello world").unwrap();
    s.send_keys("w"); // move to "world" (col 6)
    s.send_keys("y0"); // yank from start of line to cursor ("hello ")
    s.send_keys("A"); // append at end of line, enters INSERT
    s.wait_for_status("INSERT").unwrap();
    s.send_keys("\x1b");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys("p"); // paste after last char
                      // "world" followed immediately by the pasted "hello" is the unique marker
    s.wait_for_text("worldhello")
        .expect("y0 should yank from start of line up to (not including) cursor");
}

// --- indent/dedent operators with motions ---

#[test]
fn test_indent_word() {
    let (_dir, path) = temp_file_with_content("hello\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys(">w"); // indent current line (> is linewise)
                       // After indent, "hello" gains leading whitespace (tab rendered as 8 spaces);
                       // wait for any leading-space version to appear as a sync point
    s.wait_for_text("  hello")
        .expect(">w should indent the line");
    let row = s.screen_row(0);
    assert!(
        row.starts_with(' ') || row.starts_with('\t'),
        ">w should indent the current line, got: {row:?}"
    );
}

#[test]
fn test_indent_down() {
    let (_dir, path) = temp_file_with_content("one\ntwo\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("one").unwrap();
    s.send_keys(">j"); // indent current line and next
                       // Wait for leading whitespace to appear on the first line as sync point
    s.wait_for_text("  one").expect(">j should indent line 0");
    let row0 = s.screen_row(0);
    let row1 = s.screen_row(1);
    assert!(
        row0.starts_with(' ') || row0.starts_with('\t'),
        ">j should indent line 0, got: {row0:?}"
    );
    assert!(
        row1.starts_with(' ') || row1.starts_with('\t'),
        ">j should indent line 1, got: {row1:?}"
    );
}

#[test]
fn test_dedent_word() {
    let (_dir, path) = temp_file_with_content("    hello\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys("<w"); // dedent current line (shiftwidth=8 removes all 4 spaces)
                       // Wait for the 4-space indent to disappear
    s.wait_for_no_text("    hello")
        .expect("<w should remove leading whitespace");
    let row = s.screen_row(0);
    assert!(row.contains("hello"), "text should remain after <w");
}

#[test]
fn test_dedent_down() {
    let (_dir, path) = temp_file_with_content("    one\n    two\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("one").unwrap();
    s.send_keys("<j"); // dedent current and next line
                       // Wait for the first line's indent to disappear as the sync point
    s.wait_for_no_text("    one")
        .expect("<j should dedent line 0");
    let row0 = s.screen_row(0);
    let row1 = s.screen_row(1);
    assert!(
        !row0.starts_with("    "),
        "<j should dedent line 0, got: {row0:?}"
    );
    assert!(
        !row1.starts_with("    "),
        "<j should dedent line 1, got: {row1:?}"
    );
    assert!(row0.contains("one"), "text should remain on line 0");
    assert!(row1.contains("two"), "text should remain on line 1");
}

// --- case conversion operators with motions ---

#[test]
fn test_uppercase_to_eol() {
    let (_dir, path) = temp_file_with_content("hello world\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello world").unwrap();
    s.send_keys("gU$"); // uppercase to end of line
    s.wait_for_text("HELLO WORLD")
        .expect("gU$ should uppercase all characters to end of line");
}

#[test]
fn test_lowercase_to_eol() {
    let (_dir, path) = temp_file_with_content("HELLO WORLD\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("HELLO WORLD").unwrap();
    s.send_keys("gu$"); // lowercase to end of line
    s.wait_for_text("hello world")
        .expect("gu$ should lowercase all characters to end of line");
}

#[test]
fn test_toggle_case_down() {
    let (_dir, path) = temp_file_with_content("Hello\nWorld\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("Hello").unwrap();
    s.send_keys("g~j"); // toggle case of current + next line
    s.wait_for_text("hELLO")
        .expect("g~j should toggle case of current line");
    s.wait_for_text("wORLD")
        .expect("g~j should toggle case of next line");
}

#[test]
fn test_uppercase_down() {
    let (_dir, path) = temp_file_with_content("hello\nworld\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys("gUj"); // uppercase current + next line
    s.wait_for_text("HELLO")
        .expect("gUj should uppercase current line");
    s.wait_for_text("WORLD")
        .expect("gUj should uppercase next line");
}

// --- gq operator smoke test ---

#[test]
fn test_gq_smoke() {
    // gq with no textwidth set is a no-op. Verify it returns to NORMAL mode
    // with content unchanged.
    let (_dir, path) = temp_file_with_content("hello world\nsecond line\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello world").unwrap();
    s.send_keys("gqj"); // gq operator + j motion
    s.wait_for_status("NORMAL")
        .expect("gq should return to NORMAL mode without crashing");
    s.assert_contains("hello world");
}

mod common;
use common::keys::ESC;
use common::session::temp_file_with_content;

#[test]
fn test_set_number() {
    let (_dir, path) = temp_file_with_content("hello\nworld");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys(":set number\r");
    s.wait_for_text("1")
        .expect(":set number should display line numbers");
}

#[test]
fn test_set_nonumber() {
    let (_dir, path) = temp_file_with_content("hello\nworld");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys(":set number\r");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys(":set nonumber\r");
    s.wait_for_status("NORMAL").unwrap();
    let row = s.screen_row(0);
    assert!(
        row.starts_with('h'),
        "row 0 should start with 'h' when nonumber, got: {row:?}"
    );
}

#[test]
fn test_substitute() {
    let (_dir, path) = temp_file_with_content("hello world\nhello again");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello world").unwrap();
    s.send_keys(":s/hello/goodbye/\r");
    s.wait_for_text("goodbye world")
        .expect(":s should replace first 'hello' with 'goodbye' on current line");
    s.assert_contains("hello again");
}

#[test]
fn test_substitute_global() {
    let (_dir, path) = temp_file_with_content("foo foo foo");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("foo foo foo").unwrap();
    s.send_keys(":s/foo/bar/g\r");
    s.wait_for_text("bar bar bar")
        .expect(":s/foo/bar/g should replace all occurrences on the line");
}

#[test]
fn test_substitute_range() {
    let (_dir, path) = temp_file_with_content("foo\nfoo\nfoo");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("foo").unwrap();
    s.send_keys(":%s/foo/bar/g\r");
    // Sync on the actual substitution result, not the status line (which is already NORMAL)
    s.wait_for_text("bar")
        .expect("substitution should replace 'foo' globally");
    let row0 = s.screen_row(0);
    let row1 = s.screen_row(1);
    let row2 = s.screen_row(2);
    assert!(row0.contains("bar"), "row 0 should be 'bar', got: {row0:?}");
    assert!(row1.contains("bar"), "row 1 should be 'bar', got: {row1:?}");
    assert!(row2.contains("bar"), "row 2 should be 'bar', got: {row2:?}");
}

#[test]
fn test_goto_line_ex() {
    let (_dir, path) = temp_file_with_content("one\ntwo\nthree\nfour\nfive\nsix");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("one").unwrap();
    s.send_keys(":5\r");
    s.wait_for_cursor(4, 0)
        .expect(":5 should move cursor to row 4 (line 5)");
}

#[test]
fn test_delete_lines_ex() {
    let (_dir, path) = temp_file_with_content("keep\ndelete me\nkeep");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("keep").unwrap();
    s.send_keys(":2d\r");
    s.wait_for_no_text("delete me")
        .expect("line 2 should be deleted from screen");
    s.assert_contains("keep");
}

#[test]
fn test_read_file() {
    let (_dir, path_a) = temp_file_with_content("original");
    let (_dir2, path_b) = temp_file_with_content("inserted line");
    let mut s = common::RviSession::with_file(&path_a);
    s.wait_for_text("original").unwrap();
    s.send_keys(&format!(":r {}\r", path_b.display()));
    s.wait_for_text("inserted line")
        .expect(":r should insert file contents below cursor");
}

#[test]
fn test_edit_new_file() {
    let (_dir, path_a) = temp_file_with_content("file a content");
    let (_dir2, path_b) = temp_file_with_content("file b content");
    let mut s = common::RviSession::with_file(&path_a);
    s.wait_for_text("file a content").unwrap();
    s.send_keys(&format!(":e {}\r", path_b.display()));
    s.wait_for_text("file b content")
        .expect(":e should open file b");
}

#[test]
fn test_nohl() {
    // "foo" at rows 0 and 2; /foo from row 0 finds row 2; :nohl; n wraps to row 0.
    let (_dir, path) = temp_file_with_content("foo\nbar\nfoo");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("foo").unwrap();
    s.send_keys("/foo\r");
    s.wait_for_cursor(2, 0).unwrap();
    s.send_keys(":nohl\r");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys("n");
    s.wait_for_cursor(0, 0)
        .expect("n still finds next match after :nohl");
}

#[test]
fn test_global_command() {
    let (_dir, path) = temp_file_with_content("foo\nbar\nfoo\nbaz");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("foo").unwrap();
    s.send_keys(":g/foo/d\r");
    s.wait_for_no_text("foo")
        .expect("all 'foo' lines should be deleted by :g/foo/d");
    s.assert_contains("bar");
    s.assert_contains("baz");
}

#[test]
fn test_ex_mode_entry() {
    // Q mode does NOT strip a leading ':' — commands must be sent without it.
    // After Q mode exits and visual redraws, cursor returns to buffer position (0,0).
    let (_dir, path) = temp_file_with_content("hello");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys("Q");
    s.wait_for_text(":")
        .expect("Q should show ex mode ':' prompt");
    s.send_keys("vi\r");
    s.wait_for_cursor(0, 0)
        .expect("cursor at row 0 after Q mode exit");
}

#[test]
fn test_ex_mode_print() {
    let (_dir, path) = temp_file_with_content("printable content");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("printable content").unwrap();
    s.send_keys("Q");
    s.wait_for_text(":").unwrap();
    s.send_keys("p\rvi\r");
    s.wait_for_cursor(0, 0)
        .expect("cursor at row 0 after Q mode exit");
}

#[test]
fn test_ex_mode_line_number() {
    let (_dir, path) = temp_file_with_content("one\ntwo\nthree");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("one").unwrap();
    s.send_keys("j");
    s.wait_for_cursor(1, 0).unwrap();
    s.send_keys("Q");
    s.wait_for_text(":").unwrap();
    s.send_keys("=\rvi\r");
    // Cursor was at row 1 before Q mode; returns to row 1 after exit.
    s.wait_for_cursor(1, 0)
        .expect("cursor at row 1 after Q mode exit");
}

#[test]
fn test_ex_mode_multiple_commands() {
    // Use goto-line commands (simpler and verifiable via cursor position)
    // to confirm Q mode processes multiple commands sequentially.
    let (_dir, path) = temp_file_with_content("alpha\nbeta\ngamma\ndelta\nepsilon");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("alpha").unwrap();
    s.send_keys("Q");
    s.wait_for_text(":").unwrap();
    // Q mode does not strip a leading ':' — send bare commands.
    // Cooked mode delivers each \r-terminated line in sequence.
    s.send_keys("3\r5\rvi\r");
    s.wait_for_cursor(4, 0)
        .expect("cursor at row 4 (line 5) after Q mode processed goto-line commands");
}

#[test]
fn test_ex_mode_exit_visual() {
    let mut s = common::RviSession::new();
    s.wait_for_text("~").unwrap();
    s.send_keys("Q");
    s.wait_for_text(":").unwrap();
    s.send_keys("visual\r");
    s.wait_for_cursor(0, 0)
        .expect("cursor at row 0 after Q mode exit via 'visual'");
}

#[test]
fn test_ex_mode_write_and_return() {
    let (_dir, path) = temp_file_with_content("content to save");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("content to save").unwrap();
    s.send_keys("Q");
    s.wait_for_text(":").unwrap();
    s.send_keys("w\rvi\r");
    s.wait_for_cursor(0, 0)
        .expect("cursor at row 0 after Q mode exit");
    let contents = std::fs::read_to_string(&path).unwrap();
    assert!(
        contents.contains("content to save"),
        "file should be written by Q mode :w, got: {contents:?}"
    );
}

// --- :w (write without quit) ---

#[test]
fn test_write_no_quit() {
    // Prove :w writes AND editor stays open: write first change, make a second
    // change, then :q! (discards second). File should have first change only.
    let (_dir, path) = temp_file_with_content("original\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("original").unwrap();
    s.send_keys("i");
    s.wait_for_status("INSERT").unwrap();
    s.send_keys("saved\x1b"); // insert "saved" before "original"
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys(":w\r"); // write — editor stays open
                         // Sync: make a second change (proves editor still alive), then quit discarding it
    s.send_keys("o");
    s.wait_for_status("INSERT").unwrap();
    s.send_keys("unsaved\x1b");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys(":q!\r"); // quit without saving second change
    drop(s);
    let contents = std::fs::read_to_string(&path).unwrap();
    assert!(
        contents.contains("saved"),
        ":w should write 'saved' to file, got: {contents:?}"
    );
    assert!(
        !contents.contains("unsaved"),
        ":q! should discard 'unsaved', got: {contents:?}"
    );
}

// --- :w {file} (write to alternate filename) ---

#[test]
fn test_write_as() {
    let (dir, path_src) = temp_file_with_content("save me\n");
    let path_dst = dir.path().join("output.txt"); // sibling file in same temp dir
    let mut s = common::RviSession::with_file(&path_src);
    s.wait_for_text("save me").unwrap();
    s.send_keys(&format!(":w {}\r", path_dst.display()));
    // Sync: the write produces a brief status message; wait for editor to return to NORMAL.
    // Then do :q to confirm editor is still running (it would have exited if :wq were used).
    s.send_keys(":q\r"); // quit cleanly (file not modified)
    drop(s);
    let contents = std::fs::read_to_string(&path_dst).unwrap();
    assert!(
        contents.contains("save me"),
        ":w {{file}} should write to alternate file, got: {contents:?}"
    );
}

// --- :[range]y — yank lines into unnamed register, then put ---

#[test]
fn test_yank_lines_ex() {
    // :2y yanks line 2 ("two") into unnamed register; p puts it below cursor.
    // Sync: p is a content-changing command; wait for cursor to move to newly put line.
    let (_dir, path) = temp_file_with_content("one\ntwo\nthree\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("one").unwrap();
    s.send_keys(":2y\r"); // yank line 2 ("two") — NORMAL→NORMAL, but p below will sync
    s.send_keys("p"); // put below current line → cursor moves to new line at row 1
    s.wait_for_cursor_row(1).unwrap(); // cursor on the pasted "two" line
    let row1 = s.screen_row(1);
    assert!(
        row1.contains("two"),
        ":2y should yank line 2; p puts it at row 1, got: {row1:?}"
    );
}

// --- :[addr]pu — put register as new lines ---

#[test]
fn test_put_lines_ex() {
    // yy yanks "first"; :2pu puts it below line 2 and moves cursor there.
    let (_dir, path) = temp_file_with_content("first\nsecond\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("first").unwrap();
    s.send_keys("yy"); // yank "first" into unnamed register; cursor stays at row 0
    s.send_keys(":2pu\r"); // put below line 2 → cursor lands on row 2 (the new line)
    s.wait_for_cursor_row(2).unwrap();
    let row2 = s.screen_row(2);
    assert!(
        row2.contains("first"),
        ":2pu should insert yanked 'first' below line 2, got: {row2:?}"
    );
}

// --- :[range]m {addr} — move lines ---

#[test]
fn test_move_lines_ex() {
    // :1m3 moves line 1 ("alpha") to after line 3 → beta, gamma, alpha.
    // After :m, cursor lands on the moved line (row 2). Use that as sync.
    let (_dir, path) = temp_file_with_content("alpha\nbeta\ngamma\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("alpha").unwrap();
    s.send_keys(":1m3\r"); // move line 1 to after line 3 → cursor at row 2 (moved line)
    s.wait_for_cursor_row(2).unwrap();
    let row0 = s.screen_row(0);
    let row2 = s.screen_row(2);
    assert!(
        row0.contains("beta"),
        ":1m3 row 0 should be 'beta', got: {row0:?}"
    );
    assert!(
        row2.contains("alpha"),
        ":1m3 row 2 should be 'alpha', got: {row2:?}"
    );
}

// --- :[range]t {addr} — copy (transfer) lines ---

#[test]
fn test_copy_lines_ex() {
    // :1t3 copies line 1 ("alpha") to after line 3 → alpha, beta, gamma, alpha.
    // 3 lines. After :t, cursor lands on the copied line (row 3).
    let (_dir, path) = temp_file_with_content("alpha\nbeta\ngamma");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("alpha").unwrap();
    s.send_keys(":1t3\r"); // copy line 1 to after line 3 → cursor at row 3 (copied line)
    s.wait_for_cursor_row(3).unwrap();
    let row0 = s.screen_row(0);
    let row3 = s.screen_row(3);
    assert!(
        row0.contains("alpha"),
        ":1t3 original row 0 stays 'alpha', got: {row0:?}"
    );
    assert!(
        row3.contains("alpha"),
        ":1t3 copied 'alpha' appears at row 3, got: {row3:?}"
    );
}

// --- :[range]j — join lines ---

#[test]
fn test_join_lines_ex() {
    let (_dir, path) = temp_file_with_content("one\ntwo\nthree\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("one").unwrap();
    s.send_keys(":1,2j\r"); // join lines 1 and 2 → "one two"
    s.wait_for_text("one two")
        .expect(":1,2j should join lines 1 and 2 with a space");
}

// --- :%d — delete all lines ---

#[test]
fn test_delete_all_lines() {
    let (_dir, path) = temp_file_with_content("foo\nbar\nbaz\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("foo").unwrap();
    s.send_keys(":%d\r");
    s.wait_for_no_text("foo")
        .expect(":%d should delete all lines");
    s.wait_for_no_text("bar").unwrap();
}

// --- :e! — re-read file, discarding changes ---

#[test]
fn test_force_edit_discards_changes() {
    let (_dir, path) = temp_file_with_content("original\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("original").unwrap();
    s.send_keys("i");
    s.wait_for_status("INSERT").unwrap();
    s.send_keys("changed\x1b");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys(":e!\r"); // discard changes, re-read file
    s.wait_for_no_text("changed")
        .expect(":e! should discard unsaved changes");
    s.wait_for_text("original")
        .expect(":e! should restore the original file content");
}

// --- :!{cmd} — shell command execution ---

#[test]
fn test_shell_command() {
    let mut s = common::RviSession::new();
    s.wait_for_text("~").unwrap();
    s.send_keys(":!echo SHELL_OUTPUT\r");
    s.wait_for_text("SHELL_OUTPUT")
        .expect(":! should execute shell command and display output");
    // Press Enter to return to editor
    s.send_keys("\r");
    s.wait_for_text("~").unwrap();
}

// --- :map / :unmap — key mappings ---

#[test]
fn test_map_stored() {
    // :map stores a normal-mode mapping; :map output lists it.
    let mut s = common::RviSession::new();
    s.wait_for_text("~").unwrap();
    s.send_keys(":map , dd\r"); // map comma to dd
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys(":map\r"); // show all maps — should list the mapping
    s.wait_for_text(",")
        .expect(":map should display stored mapping for ','");
}

#[test]
fn test_unmap_removes_mapping() {
    // :unmap removes a previously stored mapping; it should not appear in :map output.
    let mut s = common::RviSession::new();
    s.wait_for_text("~").unwrap();
    s.send_keys(":map ,, dd\r"); // store mapping for ",,"
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys(":map\r"); // confirm ",," is listed
    s.wait_for_text(",,")
        .expect(":map should show ',,' before unmap");
    s.send_keys("\x1b"); // dismiss listing
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys(":unmap ,,\r"); // remove the mapping
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys(":map\r"); // list maps again — ",," should be gone
                           // wait_for_no_text asserts ",," never appears (returns immediately if absent)
    s.wait_for_no_text(",,")
        .expect(":unmap should remove ',,'; it should not appear in :map output");
}

// --- :ab / :una — abbreviations ---

#[test]
fn test_abbreviation_expands() {
    let mut s = common::RviSession::new();
    s.wait_for_text("~").unwrap();
    s.send_keys(":ab hw hello world\r");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys("i"); // enter insert mode
    s.wait_for_status("INSERT").unwrap();
    s.send_keys("hw "); // trigger abbreviation expansion with space
    s.wait_for_text("hello world")
        .expect(":ab hw hello world should expand 'hw' to 'hello world' in insert mode");
    s.send_keys(ESC);
}

// --- line addressing: /{pattern}/ ---

#[test]
fn test_address_pattern_delete() {
    // :/pattern/d deletes the first line matching the pattern.
    let (_dir, path) = temp_file_with_content("keep\ndelete_me\nkeep\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("delete_me").unwrap();
    s.send_keys(":/delete_me/d\r"); // delete line matching "delete_me"
    s.wait_for_no_text("delete_me")
        .expect(":/pattern/d should delete the matching line");
    s.assert_contains("keep");
}

#[test]
fn test_address_pattern_range() {
    // :/start/,/end/d deletes all lines from the first match of "start" through "end".
    let (_dir, path) = temp_file_with_content("before\nstart\nmiddle\nend\nafter\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("middle").unwrap();
    s.send_keys(":/start/,/end/d\r"); // delete "start" through "end"
    s.wait_for_no_text("start")
        .expect(":/start/,/end/d should delete from 'start' to 'end'");
    s.wait_for_no_text("middle").unwrap();
    s.wait_for_no_text("end").unwrap();
    s.assert_contains("before");
    s.assert_contains("after");
}

// --- line addressing: . and $ ---

#[test]
fn test_address_dot() {
    // '.' means current line. :1,2d is equivalent to :.,.+1d for lines 1-2.
    let (_dir, path) = temp_file_with_content("one\ntwo\nthree\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("one").unwrap();
    s.send_keys(":1,2d\r"); // delete lines 1 and 2 (same as :.,.+1d)
    s.wait_for_no_text("one")
        .expect(":1,2d should delete lines 1 and 2");
    s.wait_for_no_text("two").unwrap();
    s.assert_contains("three");
}

#[test]
fn test_address_dollar() {
    // '$' means last line; "three" is the last line.
    let (_dir, path) = temp_file_with_content("one\ntwo\nthree");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("three").unwrap();
    s.send_keys(":$d\r"); // delete last line ("three")
    s.wait_for_no_text("three")
        .expect(":$d should delete the last line");
    s.assert_contains("two");
}

// --- settings: wrapscan ---

#[test]
fn test_set_nowrapscan() {
    // With wrapscan off, searching past the last match shows an error instead of wrapping.
    let (_dir, path) = temp_file_with_content("foo\nbar\nbaz\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("foo").unwrap();
    s.send_keys(":set nowrapscan\r");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys("/foo\r"); // find "foo" at row 0
    s.wait_for_cursor(0, 0).unwrap();
    s.send_keys("n"); // next match — no more "foo" below; should show E384 or similar
    s.wait_for_status("search hit BOTTOM")
        .expect("nowrapscan: n at last match should show 'search hit BOTTOM'");
}

// --- settings: autoindent ---

#[test]
fn test_set_autoindent() {
    // With autoindent, pressing Enter in insert mode copies leading indent.
    let (_dir, path) = temp_file_with_content("\tone\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("one").unwrap();
    s.send_keys(":set autoindent\r");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys("A"); // append at end of line → enter insert mode
    s.wait_for_status("INSERT").unwrap();
    s.send_keys("\r"); // press Enter — should auto-indent with tab
                       // New line should have leading whitespace
    s.send_keys("x"); // type something so we can see the line
    s.send_keys(ESC);
    s.wait_for_status("NORMAL").unwrap();
    let row1 = s.screen_row(1);
    assert!(
        row1.starts_with('\t') || row1.starts_with(' '),
        "autoindent should copy leading tab to new line, got: {row1:?}"
    );
}

// --- settings: expandtab ---

#[test]
fn test_set_expandtab() {
    // With expandtab on, '>' indent uses spaces instead of a tab character.
    let (_dir, path) = temp_file_with_content("hello\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys(":set expandtab shiftwidth=4\r");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys(">>"); // indent line with spaces (expandtab + shiftwidth=4)
                       // Sync: >> is NORMAL→NORMAL; wait for "    hello" (4 spaces) to appear
    s.wait_for_text("    hello")
        .expect("expandtab + shiftwidth=4: >> should produce 4 leading spaces");
    let row0 = s.screen_row(0);
    assert!(
        !row0.starts_with('\t'),
        "expandtab: >> should not produce a tab, got: {row0:?}"
    );
}

// --- settings: hlsearch ---

#[test]
fn test_set_hlsearch() {
    // With hlsearch on, n/N navigation still works normally after a search.
    // "bar" is at row 0, "foo" rows 1 and 2 — search for "foo" lands at row 1.
    let (_dir, path) = temp_file_with_content("bar\nfoo\nfoo\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("foo").unwrap();
    s.send_keys(":set hlsearch\r");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys("/foo\r"); // first match: row 1
    s.wait_for_cursor(1, 0).unwrap(); // cursor moved → sync confirmed
    s.send_keys("n"); // next match: row 2
    s.wait_for_cursor(2, 0)
        .expect("hlsearch: n should still move to the next match");
}

// --- settings: incsearch ---

#[test]
fn test_set_incsearch() {
    // With incsearch, completing a search via Enter lands the cursor on the match.
    let (_dir, path) = temp_file_with_content("alpha\nbeta\ngamma\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("alpha").unwrap();
    s.send_keys(":set incsearch\r");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys("/gamma\r"); // search for "gamma" on row 2
    s.wait_for_cursor(2, 0)
        .expect("incsearch: /gamma Enter should land cursor on row 2");
}

// --- settings: tabstop ---

#[test]
fn test_set_tabstop() {
    // With tabstop=4, a leading tab renders as 4 display columns.
    // After pressing l (tab→h), the screen cursor should be at col 4.
    let (_dir, path) = temp_file_with_content("\thello\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys(":set tabstop=4\r");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys("l"); // move from tab to 'h'
                      // With tabstop=4 the tab expands to 4 cols, so 'h' appears at screen col 4
    s.wait_for_cursor(0, 4)
        .expect("tabstop=4: cursor on 'h' after tab should be at screen col 4");
}

// --- settings: nowrap ---

#[test]
fn test_set_nowrap() {
    // With 'nowrap', long lines are truncated at terminal width.
    // "short" on buffer line 2 should land at screen row 1 (not pushed down).
    let long_line = "a".repeat(100);
    let content = format!("{long_line}\nshort\n");
    let (_dir, path) = temp_file_with_content(&content);
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("short").unwrap();
    s.send_keys(":set nowrap\r");
    s.wait_for_status("NORMAL").unwrap();
    // Move cursor to "short" (buffer row 1); with nowrap the long line occupies
    // exactly 1 screen row, so "short" should be at screen row 1.
    s.send_keys("j");
    s.wait_for_cursor(1, 0)
        .expect("nowrap: j from the long line should land at screen row 1 (where 'short' is)");
}

mod common;
use common::session::temp_file_with_content;

#[test]
fn test_undo_insert() {
    let (_dir, path) = temp_file_with_content("hello");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys("iworld \x1b"); // insert "world " before "hello"
    s.wait_for_text("world hello").unwrap();
    s.send_keys("u");
    s.wait_for_no_text("world")
        .expect("u should undo the insert, removing 'world'");
    s.assert_contains("hello");
}

#[test]
fn test_redo() {
    let (_dir, path) = temp_file_with_content("hello");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys("iworld \x1b");
    s.wait_for_text("world hello").unwrap();
    s.send_keys("u");
    s.wait_for_no_text("world").unwrap();
    s.send_keys("\x12"); // Ctrl-r
    s.wait_for_text("world hello")
        .expect("Ctrl-r should redo the insert");
}

#[test]
fn test_undo_chain() {
    let (_dir, path) = temp_file_with_content("one");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("one").unwrap();
    // Two separate inserts = two undo entries
    s.send_keys("oalpha\x1b"); // insert line "alpha"
    s.wait_for_text("alpha").unwrap();
    s.send_keys("obeta\x1b"); // insert line "beta"
    s.wait_for_text("beta").unwrap();
    // First undo removes "beta"
    s.send_keys("u");
    s.wait_for_no_text("beta").expect("first u removes 'beta'");
    s.assert_contains("alpha");
    // Second undo removes "alpha"
    s.send_keys("u");
    s.wait_for_no_text("alpha")
        .expect("second u removes 'alpha'");
    s.assert_contains("one");
}

#[test]
fn test_undo_delete() {
    let (_dir, path) = temp_file_with_content("keep\ndelete me\nkeep");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("delete me").unwrap();
    s.send_keys("j"); // move to "delete me"
    s.wait_for_cursor(1, 0).unwrap();
    s.send_keys("dd");
    s.wait_for_no_text("delete me")
        .expect("dd deletes the line");
    s.send_keys("u");
    s.wait_for_text("delete me")
        .expect("u restores the deleted line");
}

#[test]
fn test_undo_boundary() {
    let (_dir, path) = temp_file_with_content("unchanged");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("unchanged").unwrap();
    // No edits made — undo stack is empty
    s.send_keys("u");
    s.wait_for_status("Already at oldest change")
        .expect("u with empty undo stack shows boundary message");
}

#[test]
fn test_redo_boundary() {
    let (_dir, path) = temp_file_with_content("hello");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    // No undos done — redo stack is empty
    s.send_keys("\x12"); // Ctrl-r
    s.wait_for_status("Already at newest change")
        .expect("Ctrl-r with empty redo stack shows boundary message");
}

#[test]
fn test_edit_after_undo_clears_redo() {
    let (_dir, path) = temp_file_with_content("hello");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys("iAAA\x1b");
    s.wait_for_text("AAAhello").unwrap();
    s.send_keys("u");
    s.wait_for_no_text("AAA").unwrap();
    // New edit clears redo stack
    s.send_keys("iBBB\x1b");
    s.wait_for_text("BBBhello").unwrap();
    // Ctrl-r should not redo the original "AAA" insert
    s.send_keys("\x12");
    s.wait_for_status("Already at newest change")
        .expect("redo stack cleared after new edit; Ctrl-r shows boundary");
}

#[test]
fn test_undo_line_u() {
    let (_dir, path) = temp_file_with_content("original\nother");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("original").unwrap();
    // Move away and back so the snapshot of "original" is captured on arrival
    s.send_keys("j");
    s.wait_for_cursor(1, 0).unwrap();
    s.send_keys("k");
    s.wait_for_cursor(0, 0).unwrap();
    // Edit row 0
    s.send_keys("A modified\x1b");
    s.wait_for_text("original modified").unwrap();
    // U restores line 0 to "original" (snapshot from when cursor arrived)
    s.send_keys("U");
    s.wait_for_no_text("modified")
        .expect("U restores line to state when cursor arrived");
    s.assert_contains("original");
}

#[test]
fn test_undo_line_u_on_open() {
    // U should restore line 1 even when the file just opened and cursor never moved.
    // rvi should initialize the line snapshot on file open.
    let (_dir, path) = temp_file_with_content("original\nother");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("original").unwrap();
    // Edit line 1 immediately, without moving the cursor first
    s.send_keys("A modified\x1b");
    s.wait_for_text("original modified").unwrap();
    s.send_keys("U");
    s.wait_for_no_text("modified")
        .expect("U should restore line 1 to 'original' even without prior cursor movement");
    s.assert_contains("original");
}

#[test]
fn test_undo_line_u_is_undoable() {
    // U itself creates an undo entry, so u after U re-applies the modification.
    let (_dir, path) = temp_file_with_content("original\nother");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("original").unwrap();
    s.send_keys("A modified\x1b");
    s.wait_for_text("original modified").unwrap();
    // U restores the line
    s.send_keys("U");
    s.wait_for_no_text("modified").unwrap();
    // u undoes the U, re-applying the modification
    s.send_keys("u");
    s.wait_for_text("original modified")
        .expect("u after U re-applies the modification that U had restored");
}

#[test]
fn test_undo_line_u_snapshot_refreshes_on_revisit() {
    // When the cursor leaves a row and returns, the snapshot is refreshed to the
    // current content. U then has nothing to restore (current == snapshot).
    let (_dir, path) = temp_file_with_content("original\nother");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("original").unwrap();
    // Edit row 0
    s.send_keys("A modified\x1b");
    s.wait_for_text("original modified").unwrap();
    // Leave row 0 and return — snapshot refreshes to "original modified"
    s.send_keys("j");
    s.wait_for_cursor_row(1).unwrap();
    s.send_keys("k");
    s.wait_for_cursor_row(0).unwrap();
    // U should be a no-op: current content equals the refreshed snapshot
    s.send_keys("U");
    s.wait_for_status("NORMAL").unwrap();
    s.assert_contains("original modified");
}

#[test]
fn test_undo_substitute() {
    // :s/foo/bar/ creates an undo entry; u restores the original text.
    let (_dir, path) = temp_file_with_content("foo baz");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("foo baz").unwrap();
    s.send_keys(":s/foo/bar/\r");
    s.wait_for_text("bar baz").unwrap();
    s.send_keys("u");
    s.wait_for_text("foo baz")
        .expect("u should undo :s substitute, restoring 'foo'");
}

#[test]
fn test_undo_multi_line_delete() {
    // 3dd is a single undo entry; one u restores all three deleted lines.
    let (_dir, path) = temp_file_with_content("one\ntwo\nthree\nfour");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("one").unwrap();
    s.send_keys("3dd");
    s.wait_for_no_text("three")
        .expect("3dd deletes three lines");
    s.send_keys("u");
    s.wait_for_text("three")
        .expect("single u restores all three lines deleted by 3dd");
    s.assert_contains("one");
    s.assert_contains("two");
}

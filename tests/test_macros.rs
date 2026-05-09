mod common;
use common::session::temp_file_with_content;

// --- q{a-z}: record macro, q: stop, @{a-z}: play ---

#[test]
fn test_macro_record_and_play() {
    // Record macro 'a': delete current line (dd). Play it back on next line.
    let (_dir, path) = temp_file_with_content("delete me\nkeep\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("delete me").unwrap();
    s.send_keys("qa"); // start recording into register 'a'
    s.wait_for_status("recording @a").unwrap();
    s.send_keys("dd"); // record: delete current line
    s.send_keys("q"); // stop recording
    s.wait_for_status("NORMAL").unwrap();
    // Now on "keep" — play the macro to delete it too
    s.send_keys("@a"); // play macro 'a'
    s.wait_for_no_text("keep")
        .expect("@a macro should execute dd on current line");
}

#[test]
fn test_macro_repeat_at_at() {
    // @@ repeats the last played macro.
    let (_dir, path) = temp_file_with_content("one\ntwo\nthree\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("one").unwrap();
    s.send_keys("qa"); // record into 'a'
    s.wait_for_status("recording @a").unwrap();
    s.send_keys("dd"); // delete current line
    s.send_keys("q"); // stop recording
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys("@a"); // play: deletes "two" (now line 1)
    s.wait_for_no_text("one").unwrap(); // "one" was deleted during recording
                                        // Wait for "two" to be deleted by the first playback
    s.wait_for_no_text("two").unwrap();
    s.send_keys("@@"); // repeat last macro: deletes "three"
    s.wait_for_no_text("three")
        .expect("@@ should repeat the last played macro");
}

#[test]
fn test_macro_with_count() {
    // 3@a plays macro 'a' three times: deletes 3 lines.
    // Use multi-char content to avoid single letters appearing in status bar paths.
    let (_dir, path) = temp_file_with_content("aaa\nbbb\nccc\nddd\neee\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("aaa").unwrap();
    s.send_keys("qa"); // record
    s.wait_for_status("recording @a").unwrap();
    s.send_keys("dd"); // delete one line
    s.send_keys("q"); // stop
    s.wait_for_status("NORMAL").unwrap();
    // "aaa" was deleted during recording; now on "bbb"
    s.send_keys("3@a"); // delete 3 more lines: bbb, ccc, ddd
    s.wait_for_no_text("bbb").unwrap();
    s.wait_for_no_text("ccc").unwrap();
    s.wait_for_no_text("ddd")
        .expect("3@a should play macro 3 times, deleting 3 lines");
    s.assert_contains("eee"); // "eee" should remain
}

#[test]
fn test_macro_with_insert_mode() {
    // Record a macro that enters insert mode, types text, then returns to normal.
    let (_dir, path) = temp_file_with_content("line one\nline two\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("line one").unwrap();
    s.send_keys("qb"); // record into 'b'
    s.wait_for_status("recording @b").unwrap();
    s.send_keys("A"); // append at end of line
    s.wait_for_status("INSERT").unwrap();
    s.send_keys("!\x1b"); // type '!' and exit insert mode
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys("j"); // move to next line
    s.wait_for_cursor_row(1).unwrap();
    s.send_keys("q"); // stop recording
    s.wait_for_status("NORMAL").unwrap();
    // Play macro on "line two"
    s.send_keys("@b");
    s.wait_for_text("line two!")
        .expect("macro with insert mode should append '!' to second line");
}

// --- macro containing a long-range jump motion ---
//
// NOTE: Macros do not currently record CommandLine-mode keystrokes (:cmds,
// /searches). The tests below verify macros that stay within Normal/Insert/
// Operator-Pending mode, which is the range that is actually recorded.

#[test]
fn test_macro_with_jump_motion() {
    // Record a macro that jumps to the last line (G) and deletes it (dd).
    // Playback repeats the jump-and-delete on the new last line.
    // G lands on "four" (row 3), the last line.
    let (_dir, path) = temp_file_with_content("one\ntwo\nthree\nfour");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("one").unwrap();
    s.send_keys("qc"); // record into 'c'
    s.wait_for_status("recording @c").unwrap();
    s.send_keys("G"); // jump to last line ("four", row 3)
    s.wait_for_cursor_row(3).unwrap();
    s.send_keys("dd"); // delete "four"
    s.wait_for_no_text("four").unwrap(); // confirm deletion before stopping
    s.send_keys("q"); // stop recording
    s.wait_for_status("NORMAL").unwrap();
    // Remaining buffer: [one, two, three]. Cursor at row 2. @c: G→row2, dd.
    s.send_keys("@c");
    s.wait_for_no_text("three")
        .expect("macro with G + dd should jump to last line and delete it");
}

// --- macro containing a character replacement (r) ---

#[test]
fn test_macro_with_char_replace() {
    // Record a macro that replaces the first character (rX) then moves down (j).
    // Playback applies the same replacement to the second line.
    let (_dir, path) = temp_file_with_content("abc\nabc\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("abc").unwrap();
    s.send_keys("qd"); // record into 'd'
    s.wait_for_status("recording @d").unwrap();
    s.send_keys("rX"); // replace 'a' → 'X' on row 0
    s.wait_for_text("Xbc").unwrap(); // confirm replacement visible
    s.send_keys("j"); // move down to row 1
    s.wait_for_cursor_row(1).unwrap();
    s.send_keys("q"); // stop recording
    s.wait_for_status("NORMAL").unwrap();
    // Play macro: rX on row 1 → 'a' → 'X'; j moves down (clamped)
    s.send_keys("@d");
    s.wait_for_no_text("abc") // both rows are now "Xbc" — no "abc" remains
        .expect("macro with r should replace the first char on the second line too");
}

#[test]
fn test_macro_with_substitute() {
    // Record a macro that performs :s/old/new/ on the current line.
    // Playback applies the same substitution to the second line.
    let (_dir, path) = temp_file_with_content("old text\nold text\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("old text").unwrap();
    s.send_keys("qe"); // record into 'e'
    s.wait_for_status("recording @e").unwrap();
    s.send_keys(":s/old/new/\r");
    s.wait_for_text("new text").unwrap(); // confirm substitution happened during recording
    s.send_keys("j"); // move to line 2
    s.wait_for_cursor_row(1).unwrap();
    s.send_keys("q"); // stop recording
    s.wait_for_status("NORMAL").unwrap();
    // Play macro: :s/old/new/ on line 2
    s.send_keys("@e");
    s.wait_for_no_text("old text")
        .expect("macro with :s should substitute on the second line too");
}

mod common;
use common::keys::ctrl;
use common::session::temp_file_with_content;

#[test]
fn test_hjkl() {
    let (_dir, path) = temp_file_with_content("abcde\nfghij\nklmno\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("abcde").unwrap();
    s.wait_for_cursor(0, 0).unwrap();
    s.send_keys("ll"); // l right twice → col 2
    s.wait_for_cursor(0, 2).unwrap();
    s.send_keys(" "); // Space = l → col 3
    s.wait_for_cursor(0, 3).unwrap();
    s.send_keys("h"); // h left → col 2
    s.wait_for_cursor(0, 2).unwrap();
    s.send_keys(&ctrl('h')); // Ctrl-h = h → col 1
    s.wait_for_cursor(0, 1).unwrap();
    s.send_keys("j"); // j down → row 1
    s.wait_for_cursor_row(1).unwrap();
    s.send_keys("k"); // k up → row 0
    s.wait_for_cursor_row(0).unwrap();
}

#[test]
fn test_word_motions() {
    // "foo.bar baz\n"
    // word boundaries: f=0, '.'=3, b=4, ' '=7, b=8
    // WORD boundaries: f=0 (WORD1 "foo.bar"), b=8 (WORD2 "baz")
    let (_dir, path) = temp_file_with_content("foo.bar baz\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("foo.bar").unwrap();
    s.wait_for_cursor(0, 0).unwrap();
    s.send_keys("w"); // → '.' at col 3
    s.wait_for_cursor(0, 3).unwrap();
    s.send_keys("w"); // → 'b' of "bar" at col 4
    s.wait_for_cursor(0, 4).unwrap();
    s.send_keys("e"); // → 'r' end of "bar" at col 6
    s.wait_for_cursor(0, 6).unwrap();
    s.send_keys("b"); // ← back to 'b' at col 4
    s.wait_for_cursor(0, 4).unwrap();
    s.send_keys("b"); // ← back to '.' at col 3
    s.wait_for_cursor(0, 3).unwrap();
    // WORD motions
    s.send_keys("0"); // reset to col 0
    s.wait_for_cursor(0, 0).unwrap();
    s.send_keys("W"); // skip whole "foo.bar" WORD → 'b' of "baz" at col 8
    s.wait_for_cursor(0, 8).unwrap();
    s.send_keys("B"); // ← back to start of "foo.bar" WORD at col 0
    s.wait_for_cursor(0, 0).unwrap();
    s.send_keys("E"); // end of "foo.bar" WORD = 'r' at col 6
    s.wait_for_cursor(0, 6).unwrap();
}

#[test]
fn test_line_motions() {
    // "  hello world\n  second line\n"
    // row 0: ' '=0, ' '=1, 'h'=2, ..., 'd'=12
    // row 1: ' '=0, ' '=1, 's'=2
    let (_dir, path) = temp_file_with_content("  hello world\n  second line\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello world").unwrap();
    s.send_keys("$"); // end of line → col 12
    s.wait_for_cursor(0, 12).unwrap();
    s.send_keys("0"); // start of line → col 0
    s.wait_for_cursor(0, 0).unwrap();
    s.send_keys("^"); // first non-blank → col 2
    s.wait_for_cursor(0, 2).unwrap();
    s.send_keys("_"); // first non-blank of current line → col 2 (stays)
    s.wait_for_cursor(0, 2).unwrap();
    s.send_keys("+"); // first non-blank of next line → row 1, col 2
    s.wait_for_cursor(1, 2).unwrap();
    s.send_keys("-"); // first non-blank of prev line → row 0, col 2
    s.wait_for_cursor(0, 2).unwrap();
    s.send_keys("\r"); // Enter = same as + → row 1, col 2
    s.wait_for_cursor(1, 2).unwrap();
    // Column motion: 5| → screen col 5 (1-indexed) = 0-indexed col 4
    s.send_keys("5|");
    s.wait_for_cursor(1, 4).unwrap();
}

#[test]
fn test_goto_line() {
    // 6 lines; G should land on row 5 ("line6").
    let (_dir, path) = temp_file_with_content("line1\nline2\nline3\nline4\nline5\nline6");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("line1").unwrap();
    s.wait_for_cursor_row(0).unwrap();
    s.send_keys("G"); // last line → row 5
    s.wait_for_cursor_row(5).unwrap();
    s.send_keys("gg"); // first line → row 0
    s.wait_for_cursor_row(0).unwrap();
    s.send_keys("5G"); // line 5 → row 4
    s.wait_for_cursor_row(4).unwrap();
    s.send_keys("3gg"); // line 3 via gg → row 2
    s.wait_for_cursor_row(2).unwrap();
}

#[test]
fn test_paragraph_motion() {
    // rows: 0="para one", 1="", 2="para two", 3="", 4="para three"
    let (_dir, path) = temp_file_with_content("para one\n\npara two\n\npara three\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("para one").unwrap();
    s.wait_for_cursor_row(0).unwrap();
    s.send_keys("}"); // forward to blank line at row 1
    s.wait_for_cursor_row(1).unwrap();
    s.send_keys("}"); // forward to blank line at row 3
    s.wait_for_cursor_row(3).unwrap();
    s.send_keys("{"); // back to blank line at row 1
    s.wait_for_cursor_row(1).unwrap();
    s.send_keys("{"); // back to row 0
    s.wait_for_cursor_row(0).unwrap();
}

#[test]
fn test_sentence_motion() {
    // "Hello world.  Foo bar.  Baz.\n"
    //  0123456789012345678901234567
    //                ^col14  ^col23
    let (_dir, path) = temp_file_with_content("Hello world.  Foo bar.  Baz.\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("Hello world").unwrap();
    s.wait_for_cursor(0, 0).unwrap();
    s.send_keys(")"); // next sentence → 'F' at col 14
    s.wait_for_cursor(0, 14).unwrap();
    s.send_keys(")"); // next sentence → 'B' at col 24
    s.wait_for_cursor(0, 24).unwrap();
    s.send_keys("("); // prev sentence → col 14
    s.wait_for_cursor(0, 14).unwrap();
    s.send_keys("("); // prev sentence → col 0
    s.wait_for_cursor(0, 0).unwrap();
}

#[test]
fn test_find_char() {
    // "abcabc\n"  cols: a=0 b=1 c=2 a=3 b=4 c=5
    let (_dir, path) = temp_file_with_content("abcabc\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("abcabc").unwrap();
    s.send_keys("fb"); // f → first 'b' at col 1
    s.wait_for_cursor(0, 1).unwrap();
    s.send_keys(";"); // repeat → next 'b' at col 4
    s.wait_for_cursor(0, 4).unwrap();
    s.send_keys(","); // reverse → back to col 1
    s.wait_for_cursor(0, 1).unwrap();
    s.send_keys("0"); // reset to col 0
    s.wait_for_cursor(0, 0).unwrap();
    s.send_keys("tc"); // t → one before 'c' (col 2) → land at col 1
    s.wait_for_cursor(0, 1).unwrap();
    s.send_keys("$"); // go to end col 5
    s.wait_for_cursor(0, 5).unwrap();
    s.send_keys("Fb"); // F backward → 'b' at col 4
    s.wait_for_cursor(0, 4).unwrap();
    s.send_keys("Tc"); // T backward → one after 'c' (col 2) → col 3
    s.wait_for_cursor(0, 3).unwrap();
}

#[test]
fn test_bracket_matching() {
    // "func(arg1, arg2) { body }\n"
    //  0123456789012345678901234567
    //      ^col4          ^col15 ^col17              ^col25
    let (_dir, path) = temp_file_with_content("func(arg1, arg2) { body }\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("func").unwrap();
    s.send_keys("f("); // move to '(' at col 4
    s.wait_for_cursor(0, 4).unwrap();
    s.send_keys("%"); // jump to matching ')' at col 15
    s.wait_for_cursor(0, 15).unwrap();
    s.send_keys("%"); // jump back to '(' at col 4
    s.wait_for_cursor(0, 4).unwrap();
    s.send_keys("f{"); // move to '{' at col 17
    s.wait_for_cursor(0, 17).unwrap();
    s.send_keys("%"); // jump to matching '}' at col 24
    s.wait_for_cursor(0, 24).unwrap();
}

#[test]
fn test_screen_relative_motions() {
    // 30-line file fills the 24-row PTY (23 buffer rows visible, row 23 = status line).
    // H → row 0, L → row 22, M → row 11.
    let content = (1..=30)
        .map(|n| format!("line {:02}\n", n))
        .collect::<String>();
    let (_dir, path) = temp_file_with_content(&content);
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("line 01").unwrap();
    s.send_keys("H"); // top of screen → row 0
    s.wait_for_cursor_row(0).unwrap();
    s.send_keys("L"); // bottom of screen → row 22
    s.wait_for_cursor_row(22).unwrap();
    s.send_keys("M"); // middle of screen → row 11
    s.wait_for_cursor_row(11).unwrap();
}

// --- scrolling: Ctrl-D / Ctrl-U ---

#[test]
fn test_scroll_half_screen() {
    // 40-line file, 24-row PTY (23 usable). half = 11.
    // Ctrl-D shifts viewport and cursor down together — screen cursor stays at row 0,
    // but "line 01" scrolls off and "line 12" appears at top.
    // Ctrl-U brings "line 01" back to top.
    let content = (1..=40)
        .map(|n| format!("line {:02}\n", n))
        .collect::<String>();
    let (_dir, path) = temp_file_with_content(&content);
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("line 01").unwrap();
    s.send_keys(&ctrl('d')); // Ctrl-D: scroll viewport + cursor down half screen
    s.wait_for_no_text("line 01")
        .expect("Ctrl-D should scroll 'line 01' off screen");
    s.assert_contains("line 12"); // new top of viewport
    s.send_keys(&ctrl('u')); // Ctrl-U: scroll viewport + cursor back up
    s.wait_for_text("line 01")
        .expect("Ctrl-U should scroll 'line 01' back to top");
}

// --- scrolling: Ctrl-F / Ctrl-B ---

#[test]
fn test_scroll_full_screen() {
    // Ctrl-F scrolls forward one full screen (height-2 = 21 lines).
    // Cursor moves to top of new viewport; "line 01" scrolls off.
    // Ctrl-B scrolls back; "line 01" returns.
    let content = (1..=40)
        .map(|n| format!("line {:02}\n", n))
        .collect::<String>();
    let (_dir, path) = temp_file_with_content(&content);
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("line 01").unwrap();
    s.send_keys(&ctrl('f')); // Ctrl-F: forward one full screen
    s.wait_for_no_text("line 01")
        .expect("Ctrl-F should scroll 'line 01' off screen");
    s.send_keys(&ctrl('b')); // Ctrl-B: back one full screen
    s.wait_for_text("line 01")
        .expect("Ctrl-B should restore 'line 01' to screen");
}

// --- scrolling: Ctrl-E / Ctrl-Y (viewport shifts, cursor clamped) ---

#[test]
fn test_scroll_one_line() {
    // Ctrl-E shifts viewport down 1 line; cursor at buffer row 0 is off-screen,
    // so it clamps to buffer row 1 (new top). "line 01" disappears from screen.
    // Ctrl-Y shifts viewport back up; "line 01" reappears.
    let content = (1..=40)
        .map(|n| format!("line {:02}\n", n))
        .collect::<String>();
    let (_dir, path) = temp_file_with_content(&content);
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("line 01").unwrap();
    s.send_keys(&ctrl('e')); // Ctrl-E: scroll down 1 line, "line 01" goes off screen
    s.wait_for_no_text("line 01")
        .expect("Ctrl-E should scroll 'line 01' off screen");
    s.send_keys(&ctrl('y')); // Ctrl-Y: scroll up 1 line, "line 01" comes back
    s.wait_for_text("line 01")
        .expect("Ctrl-Y should bring 'line 01' back on screen");
}

// --- view positioning: z commands ---

#[test]
fn test_view_position_z() {
    // z commands reposition the viewport without moving the cursor in the buffer.
    let content = (1..=40)
        .map(|n| format!("line {:02}\n", n))
        .collect::<String>();
    let (_dir, path) = temp_file_with_content(&content);
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("line 01").unwrap();
    s.send_keys("15G"); // go to line 15 (row 14)
    s.wait_for_cursor_row(14).unwrap();
    s.send_keys("zt"); // cursor line to top — buffer row unchanged
    s.wait_for_cursor_row(14).unwrap();
    s.send_keys("zb"); // cursor line to bottom — buffer row unchanged
    s.wait_for_cursor_row(14).unwrap();
    s.send_keys("zz"); // cursor line to center — buffer row unchanged
    s.wait_for_cursor_row(14).unwrap();
}

// --- jump list: Ctrl-O / Ctrl-I ---

#[test]
fn test_jump_list() {
    // G and gg are jump commands that push to the jump list.
    // Ctrl-O goes back through the list; Ctrl-I goes forward.
    // 30-line file, 23-row viewport: after G, last line is at screen bottom (row 22).
    let content = (1..=30)
        .map(|n| format!("line {:02}\n", n))
        .collect::<String>();
    let (_dir, path) = temp_file_with_content(&content);
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("line 01").unwrap();
    s.wait_for_cursor_row(0).unwrap();
    s.send_keys("G"); // jump to last line
    s.wait_for_text("line 30").unwrap(); // last line is visible
    s.wait_for_cursor_row(22).unwrap(); // viewport: top=7, row30 at screen 22
    s.send_keys("gg"); // jump back to first line
    s.wait_for_cursor_row(0).unwrap();
    s.send_keys(&ctrl('o')); // Ctrl-O: back to previous jump (last line)
    s.wait_for_cursor_row(22).unwrap();
    s.send_keys(&ctrl('i')); // Ctrl-I: forward to next jump (first line)
    s.wait_for_cursor_row(0).unwrap();
}

// --- Ctrl-^: alternate file ---

#[test]
fn test_alternate_file() {
    // :e file2 makes file1 the alternate. Ctrl-^ switches back to file1.
    let (_dir1, path1) = temp_file_with_content("file one content\n");
    let (_dir2, path2) = temp_file_with_content("file two content\n");
    let mut s = common::RviSession::with_file(&path1);
    s.wait_for_text("file one content").unwrap();
    // edit file2 — file1 becomes alternate
    let cmd = format!(":e {}\r", path2.display());
    s.send_keys(&cmd);
    s.wait_for_text("file two content").unwrap();
    // Ctrl-^ switches back to the alternate (file1)
    s.send_keys(&ctrl('^'));
    s.wait_for_text("file one content")
        .expect("Ctrl-^ should switch to the alternate file");
}

// --- count prefix on word motion ---

#[test]
fn test_count_word_motion() {
    // "one two three four\n": word starts at 0, 4, 8, 14
    let (_dir, path) = temp_file_with_content("one two three four\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("one two three").unwrap();
    s.send_keys("3w"); // skip 3 words: 'o'→'t'→'t'→'f'
    s.wait_for_cursor(0, 14)
        .expect("3w from col 0 should land on 'four' at col 14");
}

// --- Ctrl-G: file info ---

#[test]
fn test_ctrl_g_shows_file_info() {
    let (_dir, path) = temp_file_with_content("one\ntwo\nthree\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("one").unwrap();
    s.send_keys(&ctrl('g')); // show file info in status
                             // File info format: "filename"  N lines  --Top--  line M
                             // Status starts with a double-quote (opening quote of filename).
    s.wait_for_status_prefix("\"")
        .expect("Ctrl-G should show file info starting with the filename in quotes");
}

// --- {count}| column motion ---

#[test]
fn test_column_motion() {
    // {count}| moves to screen column {count} (1-based); bare | defaults to column 1.
    let (_dir, path) = temp_file_with_content("abcdef\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("abcdef").unwrap();
    s.wait_for_cursor(0, 0).unwrap();
    s.send_keys("4|"); // column 4 (1-based) → byte offset 3
    s.wait_for_cursor(0, 3)
        .expect("4| should move cursor to column 4 (byte offset 3)");
    s.send_keys("|"); // bare | → default column 1 → byte offset 0
    s.wait_for_cursor(0, 0)
        .expect("bare | should move to column 1 (byte offset 0)");
    s.send_keys("6|"); // last column of "abcdef"
    s.wait_for_cursor(0, 5)
        .expect("6| should move to column 6 (byte offset 5)");
}

#[test]
fn test_column_motion_with_tabs() {
    // A tab at tabstop=8 (default) occupies screen columns 0–7 (8 cells wide).
    // The character after the tab ('a') is at screen column 8.
    // |n uses 1-based screen columns, so 9| moves to screen column 8 (0-based).
    // wait_for_cursor checks terminal screen position, not byte offset.
    let (_dir, path) = temp_file_with_content("\tabc\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("abc").unwrap();
    s.send_keys("9|"); // 1-based col 9 = screen col 8 (0-based) = 'a' after tab
    s.wait_for_cursor(0, 8)
        .expect("9| should land at screen column 8 (first char after tab at tabstop=8)");
    s.send_keys("|"); // bare | = column 1 = screen col 0 = the tab itself
    s.wait_for_cursor(0, 0)
        .expect("| should land at screen column 0 (the tab character)");
}

// --- Ctrl-L: screen redraw ---

#[test]
fn test_ctrl_l_redraws() {
    let (_dir, path) = temp_file_with_content("hello world\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello world").unwrap();
    s.send_keys(&ctrl('l')); // request full redraw
                             // Content and mode should be unchanged after Ctrl-L
    s.wait_for_text("hello world")
        .expect("Ctrl-L should redraw the screen without changing content");
    s.wait_for_status("NORMAL").unwrap();
}

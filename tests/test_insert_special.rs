mod common;
use common::keys::ESC;
use common::session::temp_file_with_content;

// --- Ctrl-W: delete word before cursor in insert mode ---

#[test]
fn test_insert_ctrl_w() {
    // Use a file with content so the buffer is non-empty; position at end with A
    let (_dir, path) = temp_file_with_content("hello world\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello world").unwrap();
    // A moves to end of line (after 'd'), Ctrl-W deletes "world", ESC exits insert
    s.send_keys("A\x17\x1b");
    s.wait_for_status("NORMAL").unwrap();
    s.wait_for_no_text("world")
        .expect("Ctrl-W should delete the word 'world' before the cursor");
    s.assert_contains("hello");
}

// --- Ctrl-U: delete to start of line in insert mode ---

#[test]
fn test_insert_ctrl_u() {
    let mut s = common::RviSession::new();
    s.wait_for_text("~").unwrap();
    s.send_keys("ihello");
    // Ctrl-U (\x15) deletes everything back to the start of the line
    s.send_keys("\x15");
    s.send_keys(ESC);
    s.wait_for_status("NORMAL").unwrap();
    s.wait_for_no_text("hello")
        .expect("Ctrl-U should delete all text typed so far on the line");
}

// --- Ctrl-R {reg}: insert register contents in insert mode ---

#[test]
fn test_insert_ctrl_r_register() {
    let (_dir, path) = temp_file_with_content("hello\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    // Yank the word "hello" into register 0 via `yw`
    s.send_keys("yw");
    // Move to end of line and enter insert mode
    s.send_keys("A");
    s.wait_for_status("INSERT").unwrap();
    s.send_keys(" ");
    // Ctrl-R (\x12) followed by register name '0' inserts yank register
    s.send_keys("\x120");
    s.send_keys(ESC);
    s.wait_for_status("NORMAL").unwrap();
    s.wait_for_text("hello hello")
        .expect("Ctrl-R 0 should insert the contents of register 0");
}

// --- Ctrl-T: indent current line by shiftwidth in insert mode ---

#[test]
fn test_insert_ctrl_t() {
    let (_dir, path) = temp_file_with_content("hello\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys("i");
    s.wait_for_status("INSERT").unwrap();
    // Ctrl-T (\x14) indents the line by one shiftwidth
    s.send_keys("\x14");
    s.send_keys(ESC);
    s.wait_for_status("NORMAL").unwrap();
    // After indenting, the line should start with whitespace followed by "hello"
    s.wait_for_text("hello")
        .expect("text should still be present after Ctrl-T indent");
    // The line in row 0 should be indented (not start at column 0)
    let row = s.screen_row(0);
    assert!(
        row.starts_with(' ') || row.starts_with('\t'),
        "Ctrl-T should indent the line with leading whitespace, got: {row:?}"
    );
}

// --- Ctrl-D: dedent current line in insert mode ---

#[test]
fn test_insert_ctrl_d() {
    // Start with an indented line so there is whitespace to remove
    let (_dir, path) = temp_file_with_content("    hello\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys("i");
    s.wait_for_status("INSERT").unwrap();
    // Ctrl-D (\x04) dedents the line by one shiftwidth
    s.send_keys("\x04");
    s.send_keys(ESC);
    s.wait_for_status("NORMAL").unwrap();
    s.wait_for_text("hello")
        .expect("text should still be present after Ctrl-D dedent");
    let row = s.screen_row(0);
    // After one dedent of a 4-space indent the leading spaces should be fewer
    assert!(
        !row.starts_with("    hello"),
        "Ctrl-D should remove leading whitespace, got: {row:?}"
    );
}

// --- Ctrl-@: re-insert last inserted text in insert mode ---

#[test]
fn test_insert_ctrl_at() {
    let mut s = common::RviSession::new();
    s.wait_for_text("~").unwrap();
    // First insert: type "hello" and exit
    s.send_keys("ihello");
    s.send_keys(ESC);
    s.wait_for_status("NORMAL").unwrap();
    // Move to end of line and open a new line below
    s.send_keys("o");
    s.wait_for_status("INSERT").unwrap();
    // Ctrl-@ (\x00) re-inserts last inserted text and exits insert mode
    s.send_keys("\x00");
    s.wait_for_status("NORMAL")
        .expect("Ctrl-@ should exit insert mode after replaying");
    s.wait_for_text("hello")
        .expect("Ctrl-@ should have re-inserted the last typed text");
}

// --- 0 Ctrl-D: delete all indentation, reset autoindent level ---

#[test]
fn test_insert_zero_ctrl_d() {
    let (_dir, path) = temp_file_with_content("        hello\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys("i");
    s.wait_for_status("INSERT").unwrap();
    // Type '0' then Ctrl-D: removes ALL leading whitespace
    s.send_keys("0\x04");
    s.send_keys(ESC);
    s.wait_for_status("NORMAL").unwrap();
    let row = s.screen_row(0);
    assert!(
        !row.starts_with(' ') && !row.starts_with('\t'),
        "0 Ctrl-D should remove all leading whitespace, got: {row:?}"
    );
    assert!(row.contains("hello"), "text should remain after 0 Ctrl-D");
}

// --- ^ Ctrl-D: delete all indentation this line, restore on next ---

#[test]
fn test_insert_caret_ctrl_d() {
    let (_dir, path) = temp_file_with_content("        hello\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys("A"); // go to end of line
    s.wait_for_status("INSERT").unwrap();
    // Type '^' then Ctrl-D: removes all leading whitespace on this line
    s.send_keys("^\x04");
    // Wait for the leading whitespace to be removed before asserting
    s.wait_for_no_text("        hello")
        .expect("^ Ctrl-D should remove all leading whitespace");
    let row = s.screen_row(0);
    assert!(
        !row.starts_with(' ') && !row.starts_with('\t'),
        "^ Ctrl-D should remove all leading whitespace, got: {row:?}"
    );
    // Press Enter: the saved indent should be restored on the new line
    s.send_keys("\r");
    s.send_keys("x"); // type a char so we can see the indent
    s.send_keys(ESC);
    s.wait_for_status("NORMAL").unwrap();
    let new_row = s.screen_row(1);
    assert!(
        new_row.starts_with(' ') || new_row.starts_with('\t'),
        "^ Ctrl-D should restore indent on the next line, got: {new_row:?}"
    );
}

// --- Delete key: forward-delete character at cursor in insert mode ---

#[test]
fn test_insert_delete_key() {
    let (_dir, path) = temp_file_with_content("hello world\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello world").unwrap();
    // Enter insert mode at start of line (cursor on 'h')
    s.send_keys("i");
    s.wait_for_status("INSERT").unwrap();
    // Delete key (ESC [3~) deletes the character AT the cursor ('h')
    s.send_keys("\x1b[3~");
    s.send_keys(ESC);
    s.wait_for_status("NORMAL").unwrap();
    s.wait_for_text("ello world")
        .expect("Delete key should remove character at cursor");
    s.wait_for_no_text("hello world")
        .expect("original text should be modified");
}

// --- Ctrl-V: insert next character literally in insert mode ---

#[test]
fn test_insert_ctrl_v_literal() {
    let mut s = common::RviSession::new();
    s.wait_for_text("~").unwrap();
    s.send_keys("i");
    s.wait_for_status("INSERT").unwrap();
    // Ctrl-V (\x16) followed by 'k' inserts a literal 'k' (not a motion)
    s.send_keys("\x16k");
    s.send_keys(ESC);
    s.wait_for_status("NORMAL").unwrap();
    s.wait_for_text("k")
        .expect("Ctrl-V k should insert the literal character 'k'");
}

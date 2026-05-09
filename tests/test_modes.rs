mod common;
use common::keys::{ctrl, ESC};
use common::session::temp_file_with_content;

#[test]
fn test_insert_mode_entry_commands() {
    let (_dir, path) = temp_file_with_content("hello world\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello world").unwrap();

    for key in &["i", "I", "a", "A", "o", "O", "s"] {
        s.send_keys(key);
        s.wait_for_status("INSERT")
            .unwrap_or_else(|e| panic!("key {key:?} did not enter INSERT: {e}"));
        s.send_keys(ESC);
        s.wait_for_status("NORMAL")
            .unwrap_or_else(|e| panic!("ESC after {key:?} did not return to NORMAL: {e}"));
    }
}

#[test]
fn test_insert_to_normal_esc() {
    let mut s = common::RviSession::new();
    s.wait_for_text("~").unwrap();
    s.send_keys("i");
    s.wait_for_status("INSERT").unwrap();
    s.send_keys("hello");
    s.wait_for_text("hello").unwrap();
    s.send_keys(ESC);
    s.wait_for_status("NORMAL").unwrap();
    s.assert_contains("hello"); // text persists after Esc
}

#[test]
fn test_replace_mode() {
    let (_dir, path) = temp_file_with_content("abcdef\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("abcdef").unwrap();
    s.send_keys("R");
    s.wait_for_status("REPLACE").unwrap();
    s.send_keys("XYZ");
    s.send_keys(ESC);
    s.wait_for_status("NORMAL").unwrap();
    // "abc" overwritten with "XYZ", "def" unchanged → "XYZdef"
    s.wait_for_text("XYZdef")
        .expect("Replace mode should overwrite 'abc' with 'XYZ', leaving 'XYZdef'");
}

#[test]
fn test_replace_mode_at_eol_extends_line() {
    // R at the last character of a line overwrites it; subsequent characters
    // append beyond the original EOL rather than wrapping to the next line.
    let (_dir, path) = temp_file_with_content("ab\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("ab").unwrap();
    s.send_keys("$"); // move to 'b' (the last character)
    s.wait_for_cursor(0, 1).unwrap();
    s.send_keys("R");
    s.wait_for_status("REPLACE").unwrap();
    s.send_keys("XY"); // 'X' overwrites 'b'; 'Y' extends past original EOL
    s.send_keys(ESC);
    s.wait_for_status("NORMAL").unwrap();
    s.wait_for_text("aXY")
        .expect("R at last char should overwrite it then extend the line");
}

#[test]
fn test_replace_mode_undo() {
    // u after a Replace-mode session restores all overwritten characters.
    let (_dir, path) = temp_file_with_content("abcdef\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("abcdef").unwrap();
    s.send_keys("RXYZ");
    s.send_keys(ESC);
    s.wait_for_text("XYZdef").unwrap();
    s.send_keys("u"); // undo the entire Replace-mode session
    s.wait_for_text("abcdef")
        .expect("u after Replace mode should restore all overwritten characters");
}

#[test]
fn test_visual_char_mode() {
    let (_dir, path) = temp_file_with_content("hello world\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello world").unwrap();
    s.send_keys("v");
    s.wait_for_status("VISUAL").unwrap();
    s.send_keys(ESC);
    s.wait_for_status("NORMAL").unwrap();
}

#[test]
fn test_visual_line_mode() {
    let (_dir, path) = temp_file_with_content("hello world\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello world").unwrap();
    s.send_keys("V");
    s.wait_for_status("VISUAL LINE").unwrap();
    s.send_keys(ESC);
    s.wait_for_status("NORMAL").unwrap();
}

#[test]
fn test_visual_block_mode() {
    let (_dir, path) = temp_file_with_content("hello world\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello world").unwrap();
    s.send_keys(&ctrl('v'));
    s.wait_for_status("VISUAL BLOCK").unwrap();
    s.send_keys(ESC);
    s.wait_for_status("NORMAL").unwrap();
}

#[test]
fn test_command_line_mode() {
    let mut s = common::RviSession::new();
    s.wait_for_text("~").unwrap();
    s.send_keys(":");
    s.wait_for_status(":").unwrap();
    s.send_keys(ESC);
    s.wait_for_status("NORMAL").unwrap();
}

#[test]
fn test_search_mode() {
    let mut s = common::RviSession::new();
    s.wait_for_text("~").unwrap();
    s.send_keys("/");
    s.wait_for_status_prefix("/").unwrap();
    s.send_keys(ESC);
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys("?");
    s.wait_for_status_prefix("?").unwrap();
    s.send_keys(ESC);
    s.wait_for_status("NORMAL").unwrap();
}

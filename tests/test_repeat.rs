mod common;
use common::session::temp_file_with_content;

// test_repeat_delete_char: x deletes a char, . repeats it on the next char
#[test]
fn test_repeat_delete_char() {
    let (_dir, path) = temp_file_with_content("abcde\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("abcde").unwrap();
    s.send_keys("x"); // delete 'a' → "bcde"
    s.wait_for_no_text("abcde").unwrap();
    s.send_keys("."); // repeat x → delete 'b' → "cde"
    s.wait_for_no_text("bcde")
        .expect(". should repeat x, deleting the next character");
    s.assert_contains("cde");
}

// test_repeat_delete_word: dw deletes a word, . repeats on the next word
#[test]
fn test_repeat_delete_word() {
    let (_dir, path) = temp_file_with_content("one two three\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("one two three").unwrap();
    s.send_keys("dw"); // delete "one " → "two three"
    s.wait_for_no_text("one two three").unwrap();
    s.send_keys("."); // repeat dw → delete "two " → "three"
    s.wait_for_no_text("two three")
        .expect(". should repeat dw, deleting the next word");
    s.assert_contains("three");
}

// test_repeat_delete_line: dd deletes a line, . deletes the next line
#[test]
fn test_repeat_delete_line() {
    let (_dir, path) = temp_file_with_content("line one\nline two\nline three\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("line one").unwrap();
    s.send_keys("dd"); // delete "line one"
    s.wait_for_no_text("line one").unwrap();
    s.send_keys("."); // repeat dd → delete "line two"
    s.wait_for_no_text("line two")
        .expect(". should repeat dd, deleting the next line");
    s.assert_contains("line three");
}

// test_repeat_change_word: cw changes a word to "NEW", . repeats on the next word
#[test]
fn test_repeat_change_word() {
    let (_dir, path) = temp_file_with_content("foo bar baz\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("foo bar baz").unwrap();
    s.send_keys("cwNEW\x1b"); // change "foo" to "NEW", exit insert mode
    s.wait_for_text("NEW bar baz").unwrap(); // sync on content change
                                             // Batch w and . together to avoid ESC poll window race
    s.send_keys("w."); // w moves to "bar", . repeats cw → change "bar" to "NEW"
    s.wait_for_text("NEW NEW")
        .expect(". should repeat cw, replacing next word with 'NEW'");
}

// test_repeat_insert_a: A+text+ESC appends to end of line, . repeats on next line
#[test]
fn test_repeat_insert_a() {
    let (_dir, path) = temp_file_with_content("first\nsecond\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("first").unwrap();
    s.send_keys("A!\x1b"); // append '!' to end of "first" → "first!"
    s.wait_for_text("first!").unwrap(); // sync on content change
                                        // Batch j and . together to avoid ESC poll window race
    s.send_keys("j."); // j moves to "second", . repeats A! → "second!"
    s.wait_for_text("second!")
        .expect(". should repeat A+text+ESC, appending to end of next line");
}

// test_repeat_with_count: 3. repeats the last command 3 times
#[test]
fn test_repeat_with_count() {
    let (_dir, path) = temp_file_with_content("abcdefg\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("abcdefg").unwrap();
    s.send_keys("x"); // delete 'a' → "bcdefg"
    s.wait_for_no_text("abcdefg").unwrap();
    s.send_keys("3."); // repeat x 3 times → delete 'b','c','d' → "efg"
    s.wait_for_text("efg")
        .expect("3. should repeat x three times, deleting three more characters");
    s.wait_for_no_text("bcdefg").unwrap();
}

// test_repeat_replace_char: rX replaces char under cursor, . repeats on next char
#[test]
fn test_repeat_replace_char() {
    let (_dir, path) = temp_file_with_content("abcde\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("abcde").unwrap();
    s.send_keys("rX"); // replace 'a' with 'X' → "Xbcde"
    s.wait_for_text("Xbcde").unwrap();
    s.send_keys("l"); // move to 'b'
    s.wait_for_cursor(0, 1).unwrap();
    s.send_keys("."); // repeat rX → replace 'b' with 'X' → "XXcde"
    s.wait_for_text("XXcde")
        .expect(". should repeat rX, replacing the next character with 'X'");
}

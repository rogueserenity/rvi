mod common;
use common::keys::ESC;
use common::session::temp_file_with_content;

#[test]
fn test_forward_search() {
    // Cursor starts at (0,0) on "alpha"; /foo skips forward past row 0 and finds "foo" at row 1.
    let (_dir, path) = temp_file_with_content("alpha\nfoo bar\nbaz");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("alpha").unwrap();
    s.send_keys("/foo\r");
    s.wait_for_cursor(1, 0)
        .expect("cursor should be on 'foo' at row 1, col 0");
}

#[test]
fn test_backward_search() {
    // From (0,0), ?foo wraps backward to find "foo" at row 1.
    let (_dir, path) = temp_file_with_content("alpha\nfoo bar\nbaz");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("alpha").unwrap();
    s.send_keys("?foo\r");
    s.wait_for_cursor(1, 0)
        .expect("backward search wraps and finds 'foo' at row 1, col 0");
}

#[test]
fn test_n_next_match() {
    // Two occurrences: row 1 and row 3. /foo lands on row 1; n advances to row 3.
    let (_dir, path) = temp_file_with_content("alpha\nfoo\nbeta\nfoo");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("alpha").unwrap();
    s.send_keys("/foo\r");
    s.wait_for_cursor(1, 0).expect("first match at row 1");
    s.send_keys("n");
    s.wait_for_cursor(3, 0)
        .expect("n moves to second match at row 3");
}

#[test]
fn test_n_reverses_search() {
    // After /foo lands on row 1, N goes backward (wraps) to the last match at row 3.
    let (_dir, path) = temp_file_with_content("alpha\nfoo\nbeta\nfoo");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("alpha").unwrap();
    s.send_keys("/foo\r");
    s.wait_for_cursor(1, 0).expect("first match at row 1");
    s.send_keys("N");
    s.wait_for_cursor(3, 0)
        .expect("N reverses direction, wraps to last match at row 3");
}

#[test]
fn test_search_wrap() {
    // Single occurrence at last line. n wraps around and returns to it.
    let (_dir, path) = temp_file_with_content("alpha\nbeta\nfoo");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("alpha").unwrap();
    s.send_keys("/foo\r");
    s.wait_for_cursor(2, 0).expect("first match at row 2");
    s.send_keys("n");
    s.wait_for_cursor(2, 0)
        .expect("n wraps around, back to same only match");
}

#[test]
fn test_star_word_search() {
    // Cursor on "hello" at row 0; * searches forward and lands on "hello" at row 1.
    let (_dir, path) = temp_file_with_content("hello world\nhello again");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello world").unwrap();
    s.send_keys("*");
    s.wait_for_cursor(1, 0)
        .expect("* finds next 'hello' at row 1, col 0");
}

#[test]
fn test_hash_word_search() {
    // Move to row 1; # searches backward for "hello" and lands on row 0.
    let (_dir, path) = temp_file_with_content("hello world\nhello again");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello world").unwrap();
    s.send_keys("j");
    s.wait_for_cursor(1, 0).expect("j moves to row 1");
    s.send_keys("#");
    s.wait_for_cursor(0, 0)
        .expect("# finds previous 'hello' at row 0, col 0");
}

#[test]
fn test_empty_pattern_reuse() {
    // /foo finds row 1; gg returns to row 0; /\r reuses last pattern and finds row 1 again.
    let (_dir, path) = temp_file_with_content("alpha\nfoo\nbeta\nfoo");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("alpha").unwrap();
    s.send_keys("/foo\r");
    s.wait_for_cursor(1, 0)
        .expect("first search lands at row 1");
    s.send_keys("gg");
    s.wait_for_cursor(0, 0).expect("gg returns to row 0");
    s.send_keys("/\r");
    s.wait_for_cursor(1, 0)
        .expect("empty pattern reuses 'foo', finds row 1");
}

#[test]
fn test_search_ignorecase() {
    // :set ignorecase makes /hello match "Hello" (capital H) at row 1.
    let (_dir, path) = temp_file_with_content("world\nHello\nhello");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("world").unwrap();
    s.send_keys(":set ignorecase\r");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys("/hello\r");
    s.wait_for_cursor(1, 0)
        .expect("ignorecase: /hello matches 'Hello' at row 1");
}

#[test]
fn test_search_regex_dot_anchor() {
    // /^.at matches any line that starts with a char followed by "at".
    // From (0,0) forward: skips "cat" at row 0, finds "bat" at row 1. n wraps to "cat" at row 0.
    let (_dir, path) = temp_file_with_content("cat\nbat\ndog");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("cat").unwrap();
    s.send_keys("/^.at\r");
    s.wait_for_cursor(1, 0)
        .expect("regex ^.at matches 'bat' at row 1 (skips row 0 starting pos)");
    s.send_keys("n");
    s.wait_for_cursor(0, 0).expect("n wraps to 'cat' at row 0");
}

#[test]
fn test_search_prompt_visible() {
    // / and ? should display the correct prompt at the start of the status line.
    // Uses wait_for_status_prefix to avoid false positive from "1/1" in the NORMAL ruler.
    let mut s = common::RviSession::new();
    s.wait_for_text("~").unwrap();
    s.send_keys("/");
    s.wait_for_status_prefix("/")
        .expect("/ should show forward search prompt at start of status");
    s.send_keys(ESC);
    s.wait_for_status("NORMAL")
        .expect("ESC cancels forward search");
    s.send_keys("?");
    s.wait_for_status_prefix("?")
        .expect("? should show backward search prompt at start of status");
    s.send_keys(ESC);
    s.wait_for_status("NORMAL")
        .expect("ESC cancels backward search");
}

// --- T7: search forward when cursor is already on a match ---

#[test]
fn test_search_skips_current_match() {
    // `/word` when cursor is already on "word" should find the *next* occurrence,
    // not stay on the current position.
    let (_dir, path) = temp_file_with_content("word and then word again\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("word").unwrap();
    // Cursor starts at col 0, on the first "word".
    s.wait_for_cursor(0, 0).unwrap();
    s.send_keys("/word\r");
    // Should advance to the second "word" at col 14.
    s.wait_for_cursor(0, 14)
        .expect("/word from col 0 (on 'word') should skip to next occurrence at col 14");
}

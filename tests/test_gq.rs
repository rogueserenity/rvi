mod common;
use common::session::temp_file_with_content;

// --- gqq: reflow current line ---

#[test]
fn test_gqq_reflows_current_line() {
    // A long line that exceeds 20 cols; gqq with textwidth=20 should split it.
    let (_dir, path) = temp_file_with_content("one two three four five six\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("one two three four five six").unwrap();
    s.send_keys(":set textwidth=20\r");
    s.send_keys("gqq"); // reformat current line
                        // The original long line should be gone; content is preserved across lines
    s.wait_for_no_text("one two three four five six")
        .expect("gqq should reflow the long line");
    s.assert_contains("one two three four");
}

// --- gqj: reflow current + next line ---

#[test]
fn test_gqj_reflows_two_lines() {
    // Two short lines that together fit within textwidth when joined and re-wrapped.
    let (_dir, path) = temp_file_with_content("hello\nworld\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys(":set textwidth=40\r");
    s.send_keys("gqj"); // reformat current + next line
                        // Both words should now appear on a single line
    s.wait_for_text("hello world")
        .expect("gqj should join short lines that fit within textwidth");
}

// --- gq respects textwidth ---

#[test]
fn test_gq_respects_textwidth() {
    // Set textwidth=10; a line with words that won't fit together gets split.
    let (_dir, path) = temp_file_with_content("alpha beta gamma\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("alpha beta gamma").unwrap();
    s.send_keys(":set textwidth=10\r");
    s.send_keys("gqq");
    // "alpha beta" fits in 10; "gamma" must be on its own line
    s.wait_for_text("alpha beta")
        .expect("gq should break at textwidth=10");
    s.wait_for_text("gamma")
        .expect("overflow word should appear on next line");
    s.wait_for_no_text("alpha beta gamma")
        .expect("original long line should be gone after reflow");
}

// --- gq preserves leading indent ---

#[test]
fn test_gq_preserves_indent() {
    // Indented long line; gqq should preserve the indent on all reflowed lines.
    let (_dir, path) = temp_file_with_content("    one two three four five\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("one two three four five").unwrap();
    s.send_keys(":set textwidth=16\r");
    s.send_keys("gqq");
    // The indent (4 spaces) should appear on the continuation line
    s.wait_for_text("    one two")
        .expect("gq should preserve leading indent on reflowed lines");
}

// --- gq no-op when textwidth=0 ---

#[test]
fn test_gq_noop_without_textwidth() {
    // With textwidth=0 (default) and no wrapmargin, gq should be a no-op.
    let (_dir, path) = temp_file_with_content("hello world\nsecond line\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello world").unwrap();
    s.send_keys("gqj"); // gq operator + j motion
    s.wait_for_status("NORMAL")
        .expect("gq should return to NORMAL mode without crashing");
    s.assert_contains("hello world");
    s.assert_contains("second line");
}

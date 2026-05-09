mod common;
use common::session::temp_file_with_content;

// Section motions: `[[` moves to the previous section start,
// `]]` moves to the next section start.
// Section boundaries are lines beginning with `{` (or a form-feed character).

#[test]
fn test_bracket_bracket_forward_to_next_section() {
    // `]]` from row 0 should jump to the next `{`-started line at row 2.
    let (_dir, path) = temp_file_with_content("intro\nalpha\n{\nsection_body\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("intro").unwrap();
    s.send_keys("]]");
    s.wait_for_cursor_row(2)
        .expect("]] should jump to next section start (line beginning with '{') at row 2");
}

#[test]
fn test_bracket_bracket_backward_to_prev_section() {
    // `[[` from row 3 should jump back to the `{`-started line at row 0.
    let (_dir, path) = temp_file_with_content("{\nalpha\nbeta\ngamma");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("alpha").unwrap();
    // Move to the last line first.
    s.send_keys("G");
    s.wait_for_cursor_row(3).unwrap();
    s.send_keys("[[");
    s.wait_for_cursor_row(0)
        .expect("[[ should jump back to previous section start ('{') at row 0");
}

#[test]
fn test_bracket_bracket_forward_multiple_sections() {
    // Two section boundaries: row 1 and row 3.
    // `]]` from row 0 lands on row 1; a second `]]` lands on row 3.
    let (_dir, path) = temp_file_with_content("intro\n{\nfoo\n{\nbar\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("intro").unwrap();
    s.send_keys("]]");
    s.wait_for_cursor_row(1).expect("first ]] jumps to row 1");
    s.send_keys("]]");
    s.wait_for_cursor_row(3).expect("second ]] jumps to row 3");
}

#[test]
fn test_bracket_bracket_backward_from_section() {
    // Standing on a `{` line: `[[` should go to the *previous* `{` line.
    let (_dir, path) = temp_file_with_content("{\nalpha\n{\nbeta\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("alpha").unwrap();
    // Move to row 2 (second `{`).
    s.send_keys("jj");
    s.wait_for_cursor_row(2).unwrap();
    s.send_keys("[[");
    s.wait_for_cursor_row(0)
        .expect("[[ from second section should land on first section at row 0");
}

#[test]
fn test_bracket_bracket_count() {
    // `2]]` should skip the first section boundary and land on the second.
    let (_dir, path) = temp_file_with_content("intro\n{\nfoo\n{\nbar\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("intro").unwrap();
    s.send_keys("2]]");
    s.wait_for_cursor_row(3)
        .expect("2]] skips first section and lands on second at row 3");
}

mod common;

// Tag navigation integration tests: Ctrl-], Ctrl-t, :tag {name}, :pop.
//
// Each test creates a temporary directory with:
//   - a `tags` file (standard ctags format: tagname\tfilename\taddress)
//   - one or more source files referenced by the tags
// rvi is launched via `RviSession::with_cwd` so the relative paths in the
// tags file resolve against that directory.

/// Write a tags file and source files into `dir`, then return `dir`.
/// Layout:
///   src.txt:    "header line\nmy_func body\nfooter line"
///   caller.txt: "my_func call\nother line"
///   tags:       my_func -> src.txt line 2
///               other_sym -> src.txt line 3
fn write_tag_env(dir: &std::path::Path) {
    std::fs::write(
        dir.join("src.txt"),
        "header line\nmy_func body\nfooter line",
    )
    .unwrap();
    std::fs::write(dir.join("caller.txt"), "my_func call\nother line").unwrap();
    std::fs::write(
        dir.join("tags"),
        "my_func\tsrc.txt\t2\nother_sym\tsrc.txt\t3\n",
    )
    .unwrap();
}

// --- :tag {name} — jump to a named tag by line address ---

#[test]
fn test_tag_jump_by_name() {
    let dir = tempfile::TempDir::new().unwrap();
    write_tag_env(dir.path());

    // Open caller.txt from the directory that contains the tags file.
    let mut s = common::RviSession::with_cwd(dir.path(), &["caller.txt"]);
    s.wait_for_text("my_func call").unwrap();

    s.send_keys(":tag my_func\r");
    // src.txt line 2 (0-indexed row 1) contains "my_func body".
    s.wait_for_text("my_func body")
        .expect(":tag my_func should jump to src.txt line 2");
    s.wait_for_cursor_row(1)
        .expect(":tag my_func should place cursor on row 1 (line 2)");
}

#[test]
fn test_tag_abbreviated_ta() {
    // :ta is the minimum abbreviation for :tag.
    let dir = tempfile::TempDir::new().unwrap();
    write_tag_env(dir.path());

    let mut s = common::RviSession::with_cwd(dir.path(), &["caller.txt"]);
    s.wait_for_text("my_func call").unwrap();

    s.send_keys(":ta my_func\r");
    s.wait_for_text("my_func body")
        .expect(":ta should behave like :tag");
}

#[test]
fn test_tag_second_entry() {
    // Jump to other_sym which is at line 3 of src.txt.
    let dir = tempfile::TempDir::new().unwrap();
    write_tag_env(dir.path());

    let mut s = common::RviSession::with_cwd(dir.path(), &["caller.txt"]);
    s.wait_for_text("other line").unwrap();

    s.send_keys(":tag other_sym\r");
    s.wait_for_text("footer line")
        .expect(":tag other_sym should jump to src.txt line 3 (\"footer line\")");
    s.wait_for_cursor_row(2)
        .expect(":tag other_sym should place cursor on row 2 (line 3)");
}

#[test]
fn test_tag_not_found_shows_error() {
    let dir = tempfile::TempDir::new().unwrap();
    write_tag_env(dir.path());

    let mut s = common::RviSession::with_cwd(dir.path(), &["caller.txt"]);
    s.wait_for_text("my_func call").unwrap();

    s.send_keys(":tag nonexistent_tag\r");
    // rvi should display an error message containing "not found" or the tag name.
    s.wait_for_status("not found")
        .or_else(|_| s.wait_for_status("nonexistent_tag"))
        .or_else(|_| s.wait_for_status("Tag"))
        .expect(":tag with unknown name should show an error");
}

// --- Ctrl-] — jump to tag under cursor ---

#[test]
fn test_ctrl_bracket_jumps_to_tag() {
    let dir = tempfile::TempDir::new().unwrap();
    write_tag_env(dir.path());

    // caller.txt line 1 begins with "my_func" — cursor starts there.
    let mut s = common::RviSession::with_cwd(dir.path(), &["caller.txt"]);
    s.wait_for_text("my_func call").unwrap();
    // Cursor is at (0, 0) = on "my_func".
    s.send_keys("\x1d"); // Ctrl-]
    s.wait_for_text("my_func body")
        .expect("Ctrl-] should jump to the definition of the word under cursor");
    s.wait_for_cursor_row(1)
        .expect("Ctrl-] should land on row 1 (line 2 of src.txt)");
}

#[test]
fn test_ctrl_bracket_no_tag_shows_error() {
    let dir = tempfile::TempDir::new().unwrap();
    write_tag_env(dir.path());

    // Move to "other line" in caller.txt — "other" is not in tags.
    let mut s = common::RviSession::with_cwd(dir.path(), &["caller.txt"]);
    s.wait_for_text("other line").unwrap();
    s.send_keys("j"); // row 1: "other line"
    s.wait_for_cursor_row(1).unwrap();
    s.send_keys("\x1d"); // Ctrl-] on "other" which is not a tag
                         // Expect an error in the status line.
    s.wait_for_status("not found")
        .or_else(|_| s.wait_for_status("No tag"))
        .or_else(|_| s.wait_for_status("tag"))
        .expect("Ctrl-] on an unknown word should show an error");
}

// --- :pop / Ctrl-t — return from tag jump ---

#[test]
fn test_pop_returns_after_tag_jump() {
    let dir = tempfile::TempDir::new().unwrap();
    write_tag_env(dir.path());

    let mut s = common::RviSession::with_cwd(dir.path(), &["caller.txt"]);
    s.wait_for_text("my_func call").unwrap();

    // Jump to tag, then pop back.
    s.send_keys(":tag my_func\r");
    s.wait_for_text("my_func body").unwrap();

    s.send_keys(":pop\r");
    s.wait_for_text("my_func call")
        .expect(":pop should return to caller.txt after :tag jump");
}

#[test]
fn test_ctrl_t_returns_after_tag_jump() {
    // Ctrl-t is equivalent to :pop.
    let dir = tempfile::TempDir::new().unwrap();
    write_tag_env(dir.path());

    let mut s = common::RviSession::with_cwd(dir.path(), &["caller.txt"]);
    s.wait_for_text("my_func call").unwrap();

    s.send_keys(":tag my_func\r");
    s.wait_for_text("my_func body").unwrap();

    s.send_keys("\x14"); // Ctrl-t
    s.wait_for_text("my_func call")
        .expect("Ctrl-t should return to the caller file after a :tag jump");
}

#[test]
fn test_pop_empty_stack_shows_error() {
    let dir = tempfile::TempDir::new().unwrap();
    write_tag_env(dir.path());

    let mut s = common::RviSession::with_cwd(dir.path(), &["caller.txt"]);
    s.wait_for_text("my_func call").unwrap();

    // No prior tag jump — stack should be empty.
    s.send_keys(":pop\r");
    s.wait_for_status("empty")
        .or_else(|_| s.wait_for_status("stack"))
        .or_else(|_| s.wait_for_status("tag"))
        .expect(":pop on an empty tag stack should show an error");
}

#[test]
fn test_ctrl_bracket_then_ctrl_t() {
    // Full round-trip with Ctrl-] + Ctrl-t.
    let dir = tempfile::TempDir::new().unwrap();
    write_tag_env(dir.path());

    let mut s = common::RviSession::with_cwd(dir.path(), &["caller.txt"]);
    s.wait_for_text("my_func call").unwrap();

    s.send_keys("\x1d"); // Ctrl-] on "my_func"
    s.wait_for_text("my_func body").unwrap();

    s.send_keys("\x14"); // Ctrl-t — back to caller.txt
    s.wait_for_text("my_func call")
        .expect("Ctrl-] followed by Ctrl-t should return to the original file");
}

// --- :tag with pattern address ---

#[test]
fn test_tag_pattern_address() {
    let dir = tempfile::TempDir::new().unwrap();

    // Source file with a recognisable function definition.
    std::fs::write(
        dir.path().join("defs.txt"),
        "preamble\nfn unique_fn_def() {}\npostamble",
    )
    .unwrap();

    // Tags file using a pattern address.
    std::fs::write(
        dir.path().join("tags"),
        "unique_fn_def\tdefs.txt\t/^fn unique_fn_def/\n",
    )
    .unwrap();

    let mut s = common::RviSession::with_cwd(dir.path(), &["defs.txt"]);
    s.wait_for_text("preamble").unwrap();

    // Move away from line 2, then jump via pattern tag.
    s.send_keys("G");
    s.wait_for_cursor_row(2).unwrap();

    s.send_keys(":tag unique_fn_def\r");
    s.wait_for_text("unique_fn_def")
        .expect(":tag with pattern address should find and display the definition line");
    // Cursor should be on row 1 (line 2 = "fn unique_fn_def() {}").
    s.wait_for_cursor_row(1)
        .expect(":tag with pattern address should place cursor on the matching line");
}

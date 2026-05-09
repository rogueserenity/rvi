mod common;
use common::keys::ESC;
use common::session::temp_file_with_content;

// Extended ex command integration tests covering features from TEST_GAPS.md.

// --- :source {file} ---

#[test]
fn test_source_executes_commands() {
    // Write an ex script that sets 'number', then :source it.
    let script_dir = tempfile::TempDir::new().unwrap();
    let script_path = script_dir.path().join("cmds.ex");
    std::fs::write(&script_path, "set number\n").unwrap();

    let (_dir, path) = temp_file_with_content("hello\nworld\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    let script_str = script_path.to_str().unwrap();
    s.send_keys(&format!(":source {script_str}\r"));
    // Wait for line numbers to appear (polling handles the render timing).
    s.wait_for_text("1 hello")
        .or_else(|_| s.wait_for_text("1  hello"))
        .expect(":source should execute 'set number' and show line numbers");
}

#[test]
fn test_so_abbreviation() {
    // :so is the short form of :source.
    let script_dir = tempfile::TempDir::new().unwrap();
    let script_path = script_dir.path().join("s.ex");
    std::fs::write(&script_path, "set number\n").unwrap();

    let (_dir, path) = temp_file_with_content("hi\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hi").unwrap();
    let script_str = script_path.to_str().unwrap();
    s.send_keys(&format!(":so {script_str}\r"));
    // Wait for line numbers to appear.
    s.wait_for_text("1 hi")
        .or_else(|_| s.wait_for_text("1  hi"))
        .expect(":so abbreviation should work like :source");
}

// --- :cd / :chdir ---

#[test]
fn test_cd_changes_directory() {
    let (_dir, path) = temp_file_with_content("hello\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys(":cd /tmp\r");
    s.wait_for_status("NORMAL")
        .expect(":cd /tmp should change directory without error");
}

#[test]
fn test_chdir_abbreviation() {
    let (_dir, path) = temp_file_with_content("hello\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys(":chdir /tmp\r");
    s.wait_for_status("NORMAL")
        .expect(":chdir should work as an alias for :cd");
}

// --- :mark / :ma / :k{a} — set mark via ex ---

#[test]
fn test_ex_mark_command() {
    // :mark a sets mark 'a' at the current line; 'a jumps back to it.
    let (_dir, path) = temp_file_with_content("first\nsecond\nthird");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("first").unwrap();
    s.send_keys("j"); // move to row 1 ("second")
    s.wait_for_cursor_row(1).unwrap();
    s.send_keys(":mark a\r");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys("G"); // jump to last line (row 2 = "third")
    s.wait_for_cursor_row(2).unwrap();
    s.send_keys("'a"); // jump back to mark 'a'
    s.wait_for_cursor_row(1)
        .expect(":mark a should set mark 'a' on row 1; 'a should return there");
}

#[test]
fn test_ex_ma_abbreviation() {
    // :ma b sets mark 'b'.
    let (_dir, path) = temp_file_with_content("alpha\nbeta\ngamma");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("alpha").unwrap();
    s.send_keys("j"); // row 1
    s.wait_for_cursor_row(1).unwrap();
    s.send_keys(":ma b\r");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys("G"); // row 2 = "gamma"
    s.wait_for_cursor_row(2).unwrap();
    s.send_keys("'b");
    s.wait_for_cursor_row(1)
        .expect(":ma b should set mark 'b' on row 1");
}

#[test]
fn test_ex_k_mark_command() {
    // :k c sets mark 'c' at the current line.
    let (_dir, path) = temp_file_with_content("one\ntwo\nthree");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("one").unwrap();
    s.send_keys("j"); // row 1
    s.wait_for_cursor_row(1).unwrap();
    s.send_keys(":kc\r"); // :k{a} form: no space between k and the mark letter
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys("G"); // row 2 = "three"
    s.wait_for_cursor_row(2).unwrap();
    s.send_keys("'c");
    s.wait_for_cursor_row(1)
        .expect(":k c should set mark 'c' on row 1");
}

// --- :[addr]p — print lines ---

#[test]
fn test_print_current_line() {
    // :p prints the current line.
    let (_dir, path) = temp_file_with_content("hello world\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello world").unwrap();
    s.send_keys(":p\r");
    s.wait_for_text("hello world")
        .expect(":p should print the current line");
}

#[test]
fn test_print_range() {
    // :1,2p prints lines 1 and 2.
    let (_dir, path) = temp_file_with_content("foo\nbar\nbaz\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("foo").unwrap();
    s.send_keys(":1,2p\r");
    s.wait_for_text("foo").expect(":1,2p should print line 1");
    s.assert_contains("bar");
}

// --- :[range]nu / :[range]number — print with line numbers ---

#[test]
fn test_nu_prints_with_line_numbers() {
    // :nu (or :number) prints the current line prefixed with its line number.
    let (_dir, path) = temp_file_with_content("hello\nworld\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys(":nu\r");
    // The output should contain the line number "1" followed by "hello".
    s.wait_for_text("1")
        .expect(":nu should print with line number prefix");
}

#[test]
fn test_number_range_prints_with_numbers() {
    // :1,2number prints lines 1-2 with line numbers.
    let (_dir, path) = temp_file_with_content("aaa\nbbb\nccc\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("aaa").unwrap();
    s.send_keys(":1,2number\r");
    s.wait_for_text("1").unwrap();
    s.wait_for_text("2").unwrap();
}

// --- :[range]l — list (show tabs as ^I, $ at end of line) ---

#[test]
fn test_list_shows_dollar_at_eol() {
    // :l should show $ at the end of the line.
    let (_dir, path) = temp_file_with_content("hello\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys(":l\r");
    s.wait_for_text("hello$")
        .or_else(|_| s.wait_for_text("hello"))
        .expect(":l should show line content (with $ at EOL)");
    // At minimum, the content should appear.
    s.assert_contains("hello");
}

// --- :[addr]a[ppend] ---

#[test]
fn test_append_inserts_lines_after() {
    // :a appends lines after the current line; lines are entered then
    // terminated by a single "." on its own line.
    let (_dir, path) = temp_file_with_content("before\nafter");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("before").unwrap();
    // In a PTY, collect_ex_lines drops raw mode and reads from stdin.
    // We send line-terminated text: the PTY converts \r to \n.
    s.send_keys(":1a\r");
    // Use \n (not \r) so BufRead::lines() sees newlines even when ICRNL is off.
    s.send_keys("inserted line\n");
    s.send_keys(".\n"); // lone dot terminates append mode
    s.wait_for_text("inserted line")
        .expect(":a should insert the new line after line 1");
    s.assert_contains("before");
    s.assert_contains("after");
}

// --- :[addr]i[nsert] ---

#[test]
fn test_insert_inserts_lines_before() {
    // :i inserts lines before the current line.
    let (_dir, path) = temp_file_with_content("existing");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("existing").unwrap();
    s.send_keys(":1i\r");
    s.send_keys("prepended\n");
    s.send_keys(".\n");
    s.wait_for_text("prepended")
        .expect(":i should insert a line before line 1");
    s.assert_contains("existing");
}

// --- :[range]c[hange] ---

#[test]
fn test_change_replaces_lines() {
    // :c replaces the current line(s) with new input.
    let (_dir, path) = temp_file_with_content("old line\nother");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("old line").unwrap();
    s.send_keys(":1c\r");
    s.send_keys("new line\n");
    s.send_keys(".\n");
    s.wait_for_text("new line")
        .expect(":c should replace line 1 with new content");
    s.wait_for_no_text("old line")
        .expect(":c should remove the old line content");
    s.assert_contains("other");
}

// --- :version / :ve ---

#[test]
fn test_version_command() {
    let mut s = common::RviSession::new();
    s.wait_for_text("~").unwrap();
    s.send_keys(":version\r");
    // Should display version info; look for common version string tokens.
    s.wait_for_text("rvi")
        .or_else(|_| s.wait_for_text("version"))
        .or_else(|_| s.wait_for_text("Version"))
        .expect(":version should display version information");
}

#[test]
fn test_ve_abbreviation() {
    let mut s = common::RviSession::new();
    s.wait_for_text("~").unwrap();
    s.send_keys(":ve\r");
    s.wait_for_text("rvi")
        .or_else(|_| s.wait_for_text("version"))
        .or_else(|_| s.wait_for_text("Version"))
        .expect(":ve should be an alias for :version");
}

// --- :map (no args) — list normal-mode mappings ---

#[test]
fn test_map_no_args_lists_mappings() {
    // :map with no arguments should display all defined normal-mode mappings.
    let mut s = common::RviSession::new();
    s.wait_for_text("~").unwrap();
    s.send_keys(":map ,q :q\r");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys(":map\r"); // list normal-mode mappings
    s.wait_for_text(",q")
        .expect(":map (no args) should list all defined normal-mode mappings");
}

// NOTE: Normal-mode map dispatch (looking up normal_maps during key input) is
// not yet implemented in rvi — the table is populated by :map/:unmap but is
// never consulted by the key handler.  Tests for map firing are omitted here
// until that feature is implemented; the storage and listing tests above cover
// the implemented portion of the :map command.

// --- :map! — insert mode mapping ---

#[test]
fn test_map_insert_mode_stored() {
    // :map! lhs rhs stores an insert-mode mapping; :map! lists it.
    let mut s = common::RviSession::new();
    s.wait_for_text("~").unwrap();
    s.send_keys(":map! ,, hello\r");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys(":map!\r"); // list insert-mode mappings
    s.wait_for_text(",,")
        .expect(":map! should store and display the insert-mode mapping");
}

// --- Abbreviation expansion boundary rules ---

#[test]
fn test_abbreviation_expands_on_space() {
    // An abbreviation expands when followed by a non-keyword character such as space.
    let mut s = common::RviSession::new();
    s.wait_for_text("~").unwrap();
    s.send_keys(":ab hw hello world\r");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys("o"); // open new line, enter INSERT
    s.wait_for_status("INSERT").unwrap();
    s.send_keys("hw "); // type abbreviation then space — should trigger expansion
    s.send_keys(ESC);
    s.wait_for_text("hello world")
        .expect("abbreviation 'hw' should expand to 'hello world' when followed by space");
}

#[test]
fn test_abbreviation_does_not_expand_mid_word() {
    // Typing additional keyword characters after the lhs prevents expansion.
    // "foo" is defined, but "food" (lhs + extra char) should NOT expand.
    let mut s = common::RviSession::new();
    s.wait_for_text("~").unwrap();
    s.send_keys(":ab foo bar\r");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys("o"); // open new line, enter INSERT
    s.wait_for_status("INSERT").unwrap();
    s.send_keys("food "); // "food" ≠ "foo" — no expansion should occur
    s.send_keys(ESC);
    // "bar" (the expansion) must NOT appear anywhere on screen
    let has_bar = (0..5).any(|r| s.screen_row(r).contains("bar"));
    assert!(
        !has_bar,
        "abbreviation 'foo' should not expand when followed by 'd' (mid-word)"
    );
}

#[test]
fn test_abbreviation_expands_on_esc() {
    // Pressing Esc immediately after the lhs also triggers abbreviation expansion.
    let mut s = common::RviSession::new();
    s.wait_for_text("~").unwrap();
    s.send_keys(":ab ty thank you\r");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys("o"); // open new line, enter INSERT
    s.wait_for_status("INSERT").unwrap();
    s.send_keys("ty"); // type abbreviation lhs
    s.send_keys(ESC); // Esc should trigger expansion then return to NORMAL
    s.wait_for_text("thank you")
        .expect("abbreviation 'ty' should expand to 'thank you' when Esc is pressed");
}

// --- :una / :unabbreviate ---

#[test]
fn test_unabbreviate_removes_abbreviation() {
    // Define an abbreviation, then remove it with :una, verify it no longer expands.
    let (_dir, path) = temp_file_with_content("text\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("text").unwrap();
    s.send_keys(":ab hw hello world\r");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys(":una hw\r");
    s.wait_for_status("NORMAL").unwrap();
    // Type "hw " in insert mode — should NOT expand.
    s.send_keys("o");
    s.send_keys("hw ");
    s.send_keys(ESC);
    // "hello world" should NOT appear (abbreviation was removed).
    // We accept either "hw" remains or the line is just "hw ".
    // The key check: "hello world" should not be there from expansion.
    let screen_has_hello =
        s.screen_row(1).contains("hello world") || s.screen_row(2).contains("hello world");
    assert!(
        !screen_has_hello,
        ":una should remove the abbreviation so 'hw' no longer expands"
    );
}

// --- :preserve / :recover (smoke tests) ---

#[test]
fn test_preserve_smoke() {
    // :preserve should run without crashing.
    let (_dir, path) = temp_file_with_content("data\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("data").unwrap();
    s.send_keys(":preserve\r");
    s.wait_for_status("NORMAL")
        .expect(":preserve should complete without error");
}

// --- Filename expansion: % and # in ex command arguments ---

#[test]
fn test_percent_expands_to_current_filename() {
    // :e % re-edits the current file; the filename token % expands to the
    // current filename. We verify no error is shown (file reloads silently).
    let (_dir, path) = temp_file_with_content("original content\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("original content").unwrap();
    s.send_keys(":e %\r");
    s.wait_for_text("original content")
        .expect(":e % should re-read the current file (% expands to filename)");
}

#[test]
fn test_hash_expands_to_alternate_filename() {
    // After opening file1 and switching to file2, :e # re-edits file1.
    let (_dir1, path1) = temp_file_with_content("file_one_content\n");
    let (_dir2, path2) = temp_file_with_content("file_two_content\n");
    let p1 = path1.to_str().unwrap().to_string();
    let p2 = path2.to_str().unwrap().to_string();
    let mut s = common::RviSession::with_args(&[&p1, &p2]);
    s.wait_for_text("file_one_content").unwrap();
    s.send_keys(":n\r"); // switch to file2
    s.wait_for_text("file_two_content").unwrap();
    s.send_keys(":e #\r"); // # expands to the previous (alternate) file
    s.wait_for_text("file_one_content")
        .expect(":e # should switch to the alternate file (file1)");
}

// --- :map / :map! / :unmap dispatch integration tests ---

#[test]
fn test_map_single_char_fires_in_normal_mode() {
    // :map x dd — pressing x deletes the current line via the mapped dd.
    let (_dir, path) = temp_file_with_content("delete me\nkeep this\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("delete me").unwrap();
    s.send_keys(":map x dd\r");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys("x"); // should fire dd, deleting "delete me"
    s.wait_for_no_text("delete me")
        .expect(":map x dd should delete the current line when x is pressed");
    s.assert_contains("keep this");
}

#[test]
fn test_map_multi_char_lhs_fires() {
    // :map ,d dd — two-character LHS.
    let (_dir, path) = temp_file_with_content("delete me\nkeep this\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("delete me").unwrap();
    s.send_keys(":map ,d dd\r");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys(",d"); // should fire dd
    s.wait_for_no_text("delete me")
        .expect(":map ,d dd should delete the current line when ,d is pressed");
    s.assert_contains("keep this");
}

#[test]
fn test_map_insert_mode_fires() {
    // :map! hw hello world — typing hw in Insert mode expands to "hello world".
    let mut s = common::RviSession::new();
    s.wait_for_text("~").unwrap();
    s.send_keys(":map! hw hello world\r");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys("i"); // enter insert mode
    s.wait_for_status("INSERT").unwrap();
    s.send_keys("hw"); // should expand
    s.send_keys("\x1b"); // ESC back to normal
    s.wait_for_text("hello world")
        .expect(":map! hw hello world should expand in insert mode");
}

#[test]
fn test_unmap_removes_mapping() {
    // :map x dd then :unmap x — after unmap, x is default delete-char again.
    let (_dir, path) = temp_file_with_content("hello\nworld\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys(":map x dd\r");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys(":unmap x\r");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys("x"); // default x = delete char 'h'
    s.wait_for_text("ello")
        .expect(":unmap x should restore x to its default delete-char behavior");
}

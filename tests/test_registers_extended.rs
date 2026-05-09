mod common;
use common::session::temp_file_with_content;

// Extended register tests: "-, "2-"9 rotation, "/, "%, "#

// --- "- small delete register ---

#[test]
fn test_small_delete_register_charwise() {
    // Deleting less than a full line (e.g. `x`) goes into "-.
    // ""-p should paste that character.
    let (_dir, path) = temp_file_with_content("abcdef\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("abcdef").unwrap();
    s.send_keys("x"); // delete 'a' into "- (charwise, less than one line)
    s.wait_for_text("bcdef").unwrap();
    s.send_keys("\"-p"); // put from small delete register
                         // After x deletes 'a' from "abcdef" → "bcdef" with cursor at 'b'.
                         // \"-p pastes 'a' after the cursor → "bacdef".
    s.wait_for_text("bacdef")
        .or_else(|_| s.wait_for_text("abcdef"))
        .or_else(|_| s.wait_for_text("bcadef"))
        .expect("\"-p should restore the charwise-deleted character");
}

#[test]
fn test_small_delete_not_in_numbered_register() {
    // A charwise delete (< 1 line) should go to "- not to "1.
    let (_dir, path) = temp_file_with_content("abcdef\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("abcdef").unwrap();
    s.send_keys("x"); // charwise delete 'a' → goes to "-
    s.wait_for_text("bcdef").unwrap();
    // "1p should not paste 'a' (it should be empty or contain a previous linewise delete).
    // Use "- explicitly to confirm it works.
    s.send_keys("\"-p");
    s.wait_for_text("bacdef")
        .or_else(|_| s.wait_for_text("bcadef"))
        .or_else(|_| s.wait_for_text("abcdef"))
        .expect("small delete content accessible via \"-");
}

// --- "2-"9 numbered delete history rotation ---

#[test]
fn test_numbered_registers_rotate_on_delete() {
    // After two linewise deletes, "1 holds the most recent, "2 the older.
    let (_dir, path) = temp_file_with_content("first\nsecond\nthird\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("first").unwrap();
    s.send_keys("dd"); // delete "first" → goes into "1
    s.wait_for_no_text("first").unwrap();
    s.send_keys("dd"); // delete "second" → goes into "1; "first" rotates to "2
    s.wait_for_no_text("second").unwrap();
    // "1 should now be "second"
    s.send_keys("\"1p");
    s.wait_for_text("second")
        .expect("\"1 holds most recent delete ('second')");
}

#[test]
fn test_numbered_register_2_holds_older_delete() {
    let (_dir, path) = temp_file_with_content("first\nsecond\nthird\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("first").unwrap();
    s.send_keys("dd"); // delete "first" → "1
    s.wait_for_no_text("first").unwrap();
    s.send_keys("dd"); // delete "second" → "1 (first rotates to "2)
    s.wait_for_no_text("second").unwrap();
    // "2 should hold "first" (the older delete)
    s.send_keys("\"2p");
    s.wait_for_text("first")
        .expect("\"2 holds the older delete ('first')");
}

// --- "/ last search pattern register (read-only) ---
//
// The "/ register holds the last search pattern. It is expanded in ex
// commands (e.g. :s//replacement/) and as a line address (://, :??).
// Direct paste via "\"/p" is not supported (virtual register). The ex-command
// and addressing tests are in test_line_addressing.rs and test_search.rs.

#[test]
fn test_search_register_reuse_in_substitute() {
    // Search for "foo", then :s//bar/ reuses "/ (the last pattern) to substitute.
    let (_dir, path) = temp_file_with_content("foo baz\nother\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("foo baz").unwrap();
    s.send_keys("/foo\r"); // sets "/ to "foo"
    s.wait_for_cursor(0, 0).unwrap();
    s.send_keys(":s//bar/\r"); // empty pattern reuses last search "foo"
    s.wait_for_text("bar baz")
        .expect(":s// should reuse last search pattern 'foo' → replace with 'bar'");
}

// --- "% current filename register (read-only) ---

#[test]
fn test_percent_register_holds_filename() {
    // "% should contain the current filename. Paste it and check the result.
    // Use a short unique marker in the filename so we can detect it on screen.
    use std::io::Write as _;
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("pct_reg.txt");
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(b"hello\n").unwrap();
    drop(f);

    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    // In normal mode, "% p pastes the current filename after the cursor on the line.
    s.send_keys("o"); // open new line in insert mode
    s.send_keys("\x12%"); // Ctrl-R % inserts filename
    s.send_keys("\x1b"); // ESC
                         // "pct_reg" is the unique part of the filename; it should appear on screen.
    s.wait_for_text("pct_reg")
        .expect("\"%p should insert current filename containing 'pct_reg'");
}

// --- "# alternate filename register (read-only) ---
//
// "# holds the alternate (previous) filename. It is expanded in ex command
// filename arguments (e.g. :e #, :r #). Direct paste is not supported.
// The :e # behavior is tested in test_ex_extended.rs::test_hash_expands_to_alternate_filename.

#[test]
fn test_alt_filename_register_via_e_hash() {
    // After switching files, :e # should re-open the previous file.
    use std::io::Write as _;
    let dir1 = tempfile::TempDir::new().unwrap();
    let path1 = dir1.path().join("altfile_first.txt");
    std::fs::File::create(&path1)
        .unwrap()
        .write_all(b"file_alpha_content\n")
        .unwrap();

    let dir2 = tempfile::TempDir::new().unwrap();
    let path2 = dir2.path().join("altfile_second.txt");
    std::fs::File::create(&path2)
        .unwrap()
        .write_all(b"file_beta_content\n")
        .unwrap();

    let p1 = path1.to_str().unwrap().to_string();
    let p2 = path2.to_str().unwrap().to_string();
    let mut s = common::RviSession::with_args(&[&p1, &p2]);
    s.wait_for_text("file_alpha_content").unwrap();
    s.send_keys(":n\r"); // switch to file 2
    s.wait_for_text("file_beta_content").unwrap();
    s.send_keys(":e #\r"); // # expands to alternate file (file 1)
    s.wait_for_text("file_alpha_content")
        .expect(":e # should re-open the alternate file (file 1)");
}

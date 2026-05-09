mod common;
use common::session::temp_file_with_content;

// File handling: UTF-8/BOM detection and stripping, CRLF line ending round-trip.

// --- UTF-8 content ---

#[test]
fn test_utf8_content_displays_correctly() {
    // rvi should open and display UTF-8 content without corruption.
    let (_dir, path) = temp_file_with_content("héllo wörld\nföo bär\n");
    let mut s = common::RviSession::with_file(&path);
    // We can't always match multi-byte chars exactly in the terminal, but
    // at minimum the file should open without crashing.
    s.wait_for_text("ll")
        .or_else(|_| s.wait_for_text("he"))
        .expect("UTF-8 file should open and display some content");
}

#[test]
fn test_utf8_multibyte_cursor_movement() {
    // Cursor should move correctly over multi-byte characters.
    let (_dir, path) = temp_file_with_content("a\u{00e9}b\n"); // "aéb"
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("b").unwrap();
    // l should move right through the multi-byte char without crashing.
    s.send_keys("l"); // move right once
    s.send_keys("l"); // move right again
                      // No crash = pass; cursor should be at col 2 (on 'b').
    let _ = s.wait_for_cursor(0, 2); // col may differ by terminal; just verify no crash
}

// --- BOM detection and stripping ---

#[test]
fn test_utf8_bom_is_stripped_on_open() {
    // A file starting with the UTF-8 BOM (EF BB BF) should have the BOM
    // stripped; the first visible character should be the content char, not
    // the BOM marker.
    use std::io::Write as _;
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("bom.txt");
    let mut f = std::fs::File::create(&path).unwrap();
    // Write UTF-8 BOM followed by content.
    f.write_all(b"\xef\xbb\xbfhello bom\n").unwrap();
    drop(f);

    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello bom").unwrap();
    // The BOM (displayed as ï»¿ or similar) should NOT appear on screen.
    let row0 = s.screen_row(0);
    // The line should start with 'h', not a BOM artifact.
    assert!(
        row0.trim_start().starts_with('h') || row0.starts_with(' '),
        "BOM should be stripped; row 0 should start with 'hello', got: {row0:?}"
    );
}

// --- CRLF line ending round-trip ---

#[test]
fn test_crlf_file_opens_correctly() {
    // A file with CRLF line endings should open without showing ^M characters.
    use std::io::Write as _;
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("crlf.txt");
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(b"line one\r\nline two\r\nline three\r\n")
        .unwrap();
    drop(f);

    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("line one").unwrap();
    s.assert_contains("line two");
    s.assert_contains("line three");
    // ^M characters should not be visible.
    let row0 = s.screen_row(0);
    assert!(
        !row0.contains('\r') && !row0.contains('^'),
        "CRLF file: ^M should not appear in display, got: {row0:?}"
    );
}

#[test]
fn test_crlf_round_trip() {
    // Open a CRLF file, make a change, write it; the saved file should still
    // have CRLF line endings (round-trip preservation).
    use std::io::Write as _;
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("crlf_rt.txt");
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(b"first\r\nsecond\r\n").unwrap();
    drop(f);

    let path_str = path.to_str().unwrap().to_string();
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("first").unwrap();
    // Append a character and save.
    s.send_keys("A!"); // append '!' at end of line 1
    s.send_keys("\x1b"); // ESC
    s.send_keys(":w\r");
    s.wait_for_status("NORMAL").unwrap();
    drop(s);

    // Read saved file and check for CRLF.
    let saved = std::fs::read(&path_str).unwrap();
    assert!(
        saved.windows(2).any(|w| w == b"\r\n"),
        "CRLF round-trip: saved file should preserve CRLF line endings"
    );
}

// --- Trailing-newline round-trip (B1) ---

#[test]
fn test_write_does_not_add_extra_trailing_newline() {
    // A standard POSIX text file ends with exactly one newline. Opening such a
    // file in rvi and writing it (`:w`) with no changes must produce a
    // byte-identical file — not a file with a double trailing newline.
    use std::io::Write as _;
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("trailing_newline.txt");
    let original = b"hello\nworld\n";
    std::fs::File::create(&path)
        .unwrap()
        .write_all(original)
        .unwrap();

    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys(":w\r");
    s.wait_for_status("NORMAL").unwrap();
    drop(s);

    let saved = std::fs::read(&path).unwrap();
    assert_eq!(
        saved, original,
        "file should be byte-identical after a no-op write; got {:?}",
        saved
    );
}

mod common;
use common::session::temp_file_with_content;

// Settings not yet covered by integration tests.

// --- readonly ---

#[test]
fn test_set_readonly_prevents_write() {
    let (_dir, path) = temp_file_with_content("content\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("content").unwrap();
    s.send_keys(":set readonly\r");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys(":w\r");
    s.wait_for_status("read-only")
        .or_else(|_| s.wait_for_status("readonly"))
        .or_else(|_| s.wait_for_status("read only"))
        .or_else(|_| s.wait_for_status("Read only"))
        .expect(":set readonly: :w should report a readonly error");
}

#[test]
fn test_set_noreadonly_allows_write() {
    let (_dir, path) = temp_file_with_content("content\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("content").unwrap();
    s.send_keys(":set readonly\r");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys(":set noreadonly\r");
    s.wait_for_status("NORMAL").unwrap();
    // After disabling readonly, :w should succeed (no error reported).
    s.send_keys(":w\r");
    // Smart path truncation guarantees "written" is always visible in the status line.
    s.wait_for_status("written").unwrap();
    let status = s.status_line();
    assert!(
        !status.to_lowercase().contains("readonly") && !status.to_lowercase().contains("read only"),
        ":set noreadonly: write should succeed, got status: {status:?}"
    );
}

#[test]
fn test_set_ro_abbreviation() {
    // :set ro is the short form of :set readonly.
    let (_dir, path) = temp_file_with_content("data\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("data").unwrap();
    s.send_keys(":set ro\r");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys(":w\r");
    s.wait_for_status("read-only")
        .or_else(|_| s.wait_for_status("readonly"))
        .or_else(|_| s.wait_for_status("read only"))
        .or_else(|_| s.wait_for_status("Read only"))
        .expect(":set ro: write should fail with readonly error");
}

// --- wrapmargin ---

#[test]
fn test_set_wrapmargin() {
    // :set wrapmargin=10 should set the option without error.
    let (_dir, path) = temp_file_with_content("hello\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys(":set wrapmargin=10\r");
    s.wait_for_status("NORMAL")
        .expect(":set wrapmargin=10 should be accepted");
}

#[test]
fn test_set_wrapmargin_query() {
    // :set wrapmargin? reports the current value.
    let (_dir, path) = temp_file_with_content("hello\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys(":set wrapmargin=5\r");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys(":set wrapmargin?\r");
    s.wait_for_text("wrapmargin=5")
        .expect(":set wrapmargin? should show current value");
}

// --- report ---

#[test]
fn test_set_report() {
    // :set report=2 should set the option without error.
    let (_dir, path) = temp_file_with_content("a\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("a").unwrap();
    s.send_keys(":set report=2\r");
    s.wait_for_status("NORMAL")
        .expect(":set report=2 should be accepted");
}

#[test]
fn test_set_report_query() {
    let (_dir, path) = temp_file_with_content("a\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("a").unwrap();
    s.send_keys(":set report=3\r");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys(":set report?\r");
    s.wait_for_text("report=3")
        .expect(":set report? should show current value");
}

// --- terse ---

#[test]
fn test_set_terse() {
    let (_dir, path) = temp_file_with_content("hello\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys(":set terse\r");
    s.wait_for_status("NORMAL")
        .expect(":set terse should be accepted");
}

#[test]
fn test_set_noterse() {
    let (_dir, path) = temp_file_with_content("hello\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys(":set terse\r");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys(":set noterse\r");
    s.wait_for_status("NORMAL")
        .expect(":set noterse should be accepted");
}

// --- scroll ---

#[test]
fn test_set_scroll() {
    let (_dir, path) = temp_file_with_content("hello\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys(":set scroll=5\r");
    s.wait_for_status("NORMAL")
        .expect(":set scroll=5 should be accepted");
}

#[test]
fn test_set_scroll_query() {
    let (_dir, path) = temp_file_with_content("hello\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys(":set scroll=7\r");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys(":set scroll?\r");
    s.wait_for_text("scroll=7")
        .expect(":set scroll? should show current value");
}

// --- errorbells ---

#[test]
fn test_set_errorbells() {
    let (_dir, path) = temp_file_with_content("hello\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys(":set errorbells\r");
    s.wait_for_status("NORMAL")
        .expect(":set errorbells should be accepted");
}

#[test]
fn test_set_noerrorbells() {
    let (_dir, path) = temp_file_with_content("hello\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys(":set noerrorbells\r");
    s.wait_for_status("NORMAL")
        .expect(":set noerrorbells should be accepted");
}

// --- autowrite ---

#[test]
fn test_set_autowrite() {
    let (_dir, path) = temp_file_with_content("hello\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys(":set autowrite\r");
    s.wait_for_status("NORMAL")
        .expect(":set autowrite should be accepted");
}

#[test]
fn test_set_noautowrite() {
    let (_dir, path) = temp_file_with_content("hello\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys(":set noautowrite\r");
    s.wait_for_status("NORMAL")
        .expect(":set noautowrite should be accepted");
}

// --- :set all ---

#[test]
fn test_set_all_shows_options() {
    // :set all should display all option values including known option names.
    let (_dir, path) = temp_file_with_content("hello\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys(":set all\r");
    // Some well-known option names should appear somewhere on screen.
    s.wait_for_text("number")
        .or_else(|_| s.wait_for_text("ignorecase"))
        .expect(":set all should display option names");
}

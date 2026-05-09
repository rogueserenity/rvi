mod common;
use common::session::temp_file_with_content;

// --- Named registers: "a through "z ---

#[test]
fn test_named_register_yank_put() {
    // "ayy yanks "alpha" into register 'a'; "ap puts it elsewhere.
    let (_dir, path) = temp_file_with_content("alpha\nbeta\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("alpha").unwrap();
    s.send_keys("\"ayy"); // yank line into register 'a'
    s.send_keys("j"); // move to "beta"
    s.wait_for_cursor(1, 0).unwrap();
    s.send_keys("\"ap"); // put register 'a' below "beta"
                         // Cursor lands on pasted line (row 2); row 2 should contain "alpha"
    s.wait_for_cursor_row(2).unwrap();
    let row2 = s.screen_row(2);
    assert!(
        row2.contains("alpha"),
        "\"ap should paste register 'a' content, got: {row2:?}"
    );
}

#[test]
fn test_named_register_delete_put() {
    // "add deletes "alpha" into register 'a', then "ap puts it elsewhere.
    let (_dir, path) = temp_file_with_content("alpha\nbeta\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("alpha").unwrap();
    s.send_keys("\"add"); // delete "alpha" into register 'a'
    s.wait_for_no_text("alpha").unwrap();
    s.send_keys("\"ap"); // put register 'a' below "beta"
    s.wait_for_text("alpha")
        .expect("\"ap should paste the deleted line from register 'a'");
}

// --- Unnamed register (") ---

#[test]
fn test_unnamed_register_survives_delete() {
    // After yy, the unnamed register holds the line.
    // A subsequent dd puts the DELETED line in the unnamed register,
    // overwriting the yank. So the old yank is only in "0.
    let (_dir, path) = temp_file_with_content("yanked\ndeleted\nend\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("yanked").unwrap();
    s.send_keys("yy"); // yank "yanked" → unnamed register and register 0
    s.send_keys("j"); // move to "deleted"
    s.wait_for_cursor(1, 0).unwrap();
    s.send_keys("dd"); // delete "deleted" → goes into unnamed register
    s.wait_for_no_text("deleted").unwrap();
    s.send_keys("p"); // put unnamed register → should paste "deleted"
    s.wait_for_text("deleted")
        .expect("unnamed register holds most recent delete after dd");
}

// --- Yank register "0 — preserved after deletes ---

#[test]
fn test_yank_register_zero() {
    // Register "0 always holds the last yanked text, even after deletes.
    let (_dir, path) = temp_file_with_content("yank_me\ndelete_me\nend\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("yank_me").unwrap();
    s.send_keys("yy"); // yank "yank_me" → register 0
    s.send_keys("j"); // move to "delete_me"
    s.wait_for_cursor(1, 0).unwrap();
    s.send_keys("dd"); // delete "delete_me" → overwrites unnamed but NOT register 0
    s.wait_for_no_text("delete_me").unwrap();
    s.send_keys("\"0p"); // put from register 0 → should paste "yank_me"
    s.wait_for_text("yank_me")
        .expect("register 0 retains last yank even after a delete");
}

// --- Black hole register "_d — doesn't overwrite unnamed ---

#[test]
fn test_black_hole_register() {
    // "_dd deletes a line without storing it in the unnamed register.
    let (_dir, path) = temp_file_with_content("keep\nthrow_away\nend\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("keep").unwrap();
    s.send_keys("yy"); // yank "keep" into unnamed register
    s.send_keys("j"); // move to "throw_away"
    s.wait_for_cursor(1, 0).unwrap();
    s.send_keys("\"_dd"); // delete "throw_away" into black hole register
    s.wait_for_no_text("throw_away").unwrap();
    s.send_keys("p"); // put from unnamed register (should still be "keep")
    s.wait_for_text("keep")
        .expect("\"_dd should not overwrite unnamed register; p pastes prior yank");
}

// --- Uppercase register append ("A) ---

#[test]
fn test_uppercase_register_appends() {
    // "ayy yanks line 1 into 'a'; "Ayy APPENDS line 2 to 'a'.
    // Then "ap puts both lines.
    let (_dir, path) = temp_file_with_content("first\nsecond\nend\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("first").unwrap();
    s.send_keys("\"ayy"); // yank "first" into register 'a'
    s.send_keys("j"); // move to "second"
    s.wait_for_cursor(1, 0).unwrap();
    s.send_keys("\"Ayy"); // APPEND "second" to register 'a'
    s.send_keys("G"); // go to last line
    s.send_keys("\"ap"); // put register 'a' (should contain "first\nsecond")
    s.wait_for_text("first").unwrap();
    s.wait_for_text("second").unwrap();
    // Verify both lines were pasted by checking they appear after "end"
    s.assert_contains("first");
    s.assert_contains("second");
}

// --- Numbered register "1 (most recent delete) ---

#[test]
fn test_numbered_register_delete_history() {
    // "1 holds the most recent linewise delete.
    // After dd, "1 should contain the deleted line.
    let (_dir, path) = temp_file_with_content("alpha\nbeta\ngamma\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("alpha").unwrap();
    s.send_keys("dd"); // delete "alpha" → goes into "1
    s.wait_for_no_text("alpha").unwrap();
    s.send_keys("\"1p"); // put from numbered register 1
    s.wait_for_text("alpha")
        .expect("\"1 should hold the most recently deleted line");
}

// --- "" explicit unnamed register prefix ---

#[test]
fn test_unnamed_register_explicit_prefix() {
    // `""p` pastes from the unnamed register explicitly — same as bare `p`.
    let (_dir, path) = temp_file_with_content("hello\nworld\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys("yy"); // yank "hello" into unnamed register
    s.send_keys("j"); // move to "world"
    s.wait_for_cursor_row(1).unwrap();
    s.send_keys("\"\"p"); // explicit unnamed register paste
    s.wait_for_text("hello")
        .expect("\"\"p should paste from the unnamed register");
}

#[test]
fn test_unnamed_register_explicit_yank() {
    // `""yy` yanks into the unnamed register explicitly — same as bare `yy`.
    let (_dir, path) = temp_file_with_content("first\nsecond\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("first").unwrap();
    s.send_keys("\"\"yy"); // yank "first" via explicit unnamed register
    s.send_keys("j");
    s.wait_for_cursor_row(1).unwrap();
    s.send_keys("p"); // paste with bare p
    s.wait_for_text("first")
        .expect("\"\"yy should yank into the unnamed register");
}

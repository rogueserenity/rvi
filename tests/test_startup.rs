mod common;
use common::session::temp_file_with_content;

// --- -R (read-only) flag ---

#[test]
fn test_readonly_flag_prevents_write() {
    // rvi -R file: editor should open in read-only mode.
    // Attempting :w should show an error containing "read" and some variant of "only/only".
    let (_dir, path) = temp_file_with_content("hello\n");
    let path_str = path.to_str().unwrap().to_string();
    let mut s = common::RviSession::with_args(&["-R", &path_str]);
    s.wait_for_text("hello").unwrap();
    s.send_keys(":w\r");
    // Status may say "read-only", "readonly", "Read only", etc.
    s.wait_for_status("read-only")
        .or_else(|_| s.wait_for_status("readonly"))
        .or_else(|_| s.wait_for_status("read only"))
        .or_else(|_| s.wait_for_status("Read only"))
        .expect("-R flag: :w should report a readonly error");
}

#[test]
fn test_readonly_flag_set_option() {
    // rvi -R: :set should show readonly (or ro) as enabled.
    let (_dir, path) = temp_file_with_content("text\n");
    let path_str = path.to_str().unwrap().to_string();
    let mut s = common::RviSession::with_args(&["-R", &path_str]);
    s.wait_for_text("text").unwrap();
    s.send_keys(":set ro?\r");
    s.wait_for_status("readonly")
        .expect("-R flag: :set ro? should confirm readonly is set");
}

// --- +{N} (go to line N after load) ---

#[test]
fn test_plus_n_goto_line() {
    // rvi +3 file: cursor should start on line 3 (row 2 zero-based).
    let (_dir, path) = temp_file_with_content("line1\nline2\nline3\nline4\nline5\n");
    let path_str = path.to_str().unwrap().to_string();
    let mut s = common::RviSession::with_args(&["+3", &path_str]);
    s.wait_for_text("line3").unwrap();
    s.wait_for_cursor_row(2)
        .expect("+3 flag: cursor should be on row 2 (line 3)");
}

// --- + (go to last line after load) ---

#[test]
fn test_plus_goto_last_line() {
    // rvi + file: cursor should start on the last line.
    // 3 lines; row 2 ("ccc") is the last row.
    let (_dir, path) = temp_file_with_content("aaa\nbbb\nccc");
    let path_str = path.to_str().unwrap().to_string();
    let mut s = common::RviSession::with_args(&["+", &path_str]);
    s.wait_for_text("aaa").unwrap();
    // Last line is row 2 (0-based) for "ccc".
    s.wait_for_cursor_row(2)
        .expect("+ flag: cursor should be on last line (row 2)");
}

// --- -c {cmd} (execute ex command after load) ---

#[test]
fn test_dash_c_execute_command() {
    // rvi -c "set number" file: line numbers should be visible.
    let (_dir, path) = temp_file_with_content("alpha\nbeta\n");
    let path_str = path.to_str().unwrap().to_string();
    let mut s = common::RviSession::with_args(&["-c", "set number", &path_str]);
    s.wait_for_text("alpha").unwrap();
    // Line numbers should be rendered (look for "1" at the start of row 0).
    let row0 = s.screen_row(0);
    assert!(
        row0.trim_start().starts_with('1') || row0.contains("1 "),
        "-c 'set number': row 0 should start with line number, got: {row0:?}"
    );
}

#[test]
fn test_dash_c_goto_line() {
    // rvi -c "4" file: cursor should land on line 4 (row 3).
    let (_dir, path) = temp_file_with_content("a\nb\nc\nd\ne\n");
    let path_str = path.to_str().unwrap().to_string();
    let mut s = common::RviSession::with_args(&["-c", "4", &path_str]);
    s.wait_for_text("a").unwrap();
    s.wait_for_cursor_row(3)
        .expect("-c '4': cursor should be on row 3 (line 4)");
}

// --- EXINIT environment variable ---

#[test]
fn test_exinit_set_number() {
    // EXINIT="set number" rvi file: line numbers should be on at startup.
    let (_dir, path) = temp_file_with_content("hello\nworld\n");
    let path_str = path.to_str().unwrap().to_string();
    let mut s = common::RviSession::with_args_env(&[&path_str], &[("EXINIT", "set number")]);
    s.wait_for_text("hello").unwrap();
    let row0 = s.screen_row(0);
    assert!(
        row0.trim_start().starts_with('1') || row0.contains("1 "),
        "EXINIT='set number': line numbers should be visible, got: {row0:?}"
    );
}

#[test]
fn test_exinit_set_ignorecase() {
    // EXINIT="set ignorecase" rvi file: case-insensitive search should be active.
    let (_dir, path) = temp_file_with_content("World\nhello\n");
    let path_str = path.to_str().unwrap().to_string();
    let mut s = common::RviSession::with_args_env(&[&path_str], &[("EXINIT", "set ignorecase")]);
    s.wait_for_text("World").unwrap();
    // /world should match "World" (case-insensitive).
    s.send_keys("/world\r");
    s.wait_for_cursor_row(0)
        .expect("EXINIT ignorecase: /world should match 'World' at row 0");
}

// --- XDG config file ---

#[test]
fn test_xdg_config_set_number() {
    // Write a config file with "set number" and point XDG_CONFIG_HOME at it.
    let config_dir = tempfile::TempDir::new().unwrap();
    let rvi_config_dir = config_dir.path().join("rvi");
    std::fs::create_dir_all(&rvi_config_dir).unwrap();
    let config_file = rvi_config_dir.join("config");
    std::fs::write(&config_file, "set number\n").unwrap();

    let (_dir, path) = temp_file_with_content("foo\nbar\n");
    let path_str = path.to_str().unwrap().to_string();
    let xdg_home = config_dir.path().to_str().unwrap().to_string();
    let mut s = common::RviSession::with_args_env(&[&path_str], &[("XDG_CONFIG_HOME", &xdg_home)]);
    s.wait_for_text("foo").unwrap();
    let row0 = s.screen_row(0);
    assert!(
        row0.trim_start().starts_with('1') || row0.contains("1 "),
        "XDG config 'set number': line numbers should be visible, got: {row0:?}"
    );
}

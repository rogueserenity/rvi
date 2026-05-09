mod common;
use common::session::temp_file_with_content;

// Multi-file navigation: :n (next), :N (previous), :args (show list), :rewind

#[test]
fn test_n_moves_to_next_file() {
    // Open two files; :n should switch to the second file.
    let (_dir1, path1) = temp_file_with_content("file_one\n");
    let (_dir2, path2) = temp_file_with_content("file_two\n");
    let p1 = path1.to_str().unwrap().to_string();
    let p2 = path2.to_str().unwrap().to_string();
    let mut s = common::RviSession::with_args(&[&p1, &p2]);
    s.wait_for_text("file_one").unwrap();
    s.send_keys(":n\r");
    s.wait_for_text("file_two")
        .expect(":n should switch to the second file");
}

#[test]
fn test_uppercase_n_moves_to_prev_file() {
    // Open two files, advance to file 2, then :N returns to file 1.
    let (_dir1, path1) = temp_file_with_content("file_one\n");
    let (_dir2, path2) = temp_file_with_content("file_two\n");
    let p1 = path1.to_str().unwrap().to_string();
    let p2 = path2.to_str().unwrap().to_string();
    let mut s = common::RviSession::with_args(&[&p1, &p2]);
    s.wait_for_text("file_one").unwrap();
    s.send_keys(":n\r");
    s.wait_for_text("file_two").unwrap();
    s.send_keys(":N\r");
    s.wait_for_text("file_one")
        .expect(":N should go back to the first file");
}

#[test]
fn test_args_shows_file_list() {
    // :args should display both filenames on screen.
    let (_dir1, path1) = temp_file_with_content("aaa\n");
    let (_dir2, path2) = temp_file_with_content("bbb\n");
    let p1 = path1.to_str().unwrap().to_string();
    let p2 = path2.to_str().unwrap().to_string();
    // Extract just the filenames for matching (paths may be long).
    let name1 = path1.file_name().unwrap().to_str().unwrap().to_string();
    let name2 = path2.file_name().unwrap().to_str().unwrap().to_string();
    let mut s = common::RviSession::with_args(&[&p1, &p2]);
    s.wait_for_text("aaa").unwrap();
    s.send_keys(":args\r");
    // Both filenames should appear somewhere on the screen.
    s.wait_for_text(&name1)
        .expect(":args should display first filename");
    s.assert_contains(&name2);
}

#[test]
fn test_rewind_returns_to_first_file() {
    // Open three files, advance twice, :rewind goes back to file 1.
    let (_dir1, path1) = temp_file_with_content("first_file\n");
    let (_dir2, path2) = temp_file_with_content("second_file\n");
    let (_dir3, path3) = temp_file_with_content("third_file\n");
    let p1 = path1.to_str().unwrap().to_string();
    let p2 = path2.to_str().unwrap().to_string();
    let p3 = path3.to_str().unwrap().to_string();
    let mut s = common::RviSession::with_args(&[&p1, &p2, &p3]);
    s.wait_for_text("first_file").unwrap();
    s.send_keys(":n\r");
    s.wait_for_text("second_file").unwrap();
    s.send_keys(":n\r");
    s.wait_for_text("third_file").unwrap();
    s.send_keys(":rewind\r");
    s.wait_for_text("first_file")
        .expect(":rewind should go back to the first file");
}

#[test]
fn test_rew_abbreviation() {
    // :rew is the short form of :rewind.
    let (_dir1, path1) = temp_file_with_content("alpha_file\n");
    let (_dir2, path2) = temp_file_with_content("beta_file\n");
    let p1 = path1.to_str().unwrap().to_string();
    let p2 = path2.to_str().unwrap().to_string();
    let mut s = common::RviSession::with_args(&[&p1, &p2]);
    s.wait_for_text("alpha_file").unwrap();
    s.send_keys(":n\r");
    s.wait_for_text("beta_file").unwrap();
    s.send_keys(":rew\r");
    s.wait_for_text("alpha_file")
        .expect(":rew should be an alias for :rewind");
}

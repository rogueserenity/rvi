mod common;

#[test]
fn test_smoke_spawn_and_quit() {
    let mut s = common::RviSession::new();
    s.wait_for_text("~")
        .expect("tilde lines should appear on empty buffer");
    s.send_keys(":q!\r");
    // Drop impl sends :q! and waits for clean exit
}

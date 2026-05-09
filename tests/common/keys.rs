#![allow(dead_code)]
pub const ESC: &str = "\x1b";
pub const ENTER: &str = "\r";
pub const CTRL_C: &str = "\x03";
pub const UP: &str = "\x1b[A";
pub const DOWN: &str = "\x1b[B";
pub const RIGHT: &str = "\x1b[C";
pub const LEFT: &str = "\x1b[D";

pub fn ctrl(c: char) -> String {
    let code = (c as u8) & 0x1f;
    String::from_utf8(vec![code]).unwrap()
}

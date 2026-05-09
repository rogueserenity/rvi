//! Keyboard input capture and parsing.
//!
//! Provides functionality to read and parse keyboard input, including
//! regular characters, control sequences, and special keys.

use std::io::{stdin, Read};
use std::os::unix::io::AsFd;

use nix::poll::{poll, PollFd, PollFlags};

use crate::error::TerminalError;

/// Represents a key press event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    /// A regular character
    Char(char),
    /// A Ctrl-key combination (e.g., Ctrl+C)
    Ctrl(char),
    /// An Alt-key combination (e.g., Alt+A)
    Alt(char),
    /// Up arrow key
    Up,
    /// Down arrow key
    Down,
    /// Left arrow key
    Left,
    /// Right arrow key
    Right,
    /// Home key
    Home,
    /// End key
    End,
    /// Page Up key
    PageUp,
    /// Page Down key
    PageDown,
    /// Function key F1-F12
    F(u8),
    /// Enter/Return key
    Enter,
    /// Tab key
    Tab,
    /// Backspace key
    Backspace,
    /// Delete key
    Delete,
    /// Escape key
    Esc,
    /// Insert key
    Insert,
    /// Unknown escape sequence (stored as bytes for debugging)
    UnknownEsc(Vec<u8>),
}

/// Read a single key from stdin.
///
/// This function blocks until a key is pressed. It handles escape sequences
/// for special keys like arrows and function keys.
///
/// # Errors
///
/// Returns an error if the input cannot be read.
pub fn read_key() -> Result<Key, TerminalError> {
    // Use blocking read for the first byte
    let mut buf = [0u8; 1];
    stdin()
        .lock()
        .read_exact(&mut buf)
        .map_err(TerminalError::ReadInput)?;

    match buf[0] {
        // Escape character - could be the Esc key or start of an escape sequence
        b'\x1b' => {
            let stdin_handle = stdin();
            let mut sequence = vec![b'\x1b'];
            let mut temp_buf = [0u8; 1];

            // Poll for follow-up bytes with a short timeout (5ms).
            // Returns immediately if bytes are ready; times out if this is a bare Esc.
            loop {
                let mut pollfd = [PollFd::new(stdin_handle.as_fd(), PollFlags::POLLIN)];
                let ready = poll(&mut pollfd, 5u16).unwrap_or(0);
                if ready == 0 {
                    // Timeout — no more bytes, this is a bare Esc
                    break;
                }
                match stdin_handle.lock().read(&mut temp_buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        sequence.push(temp_buf[0]);
                        // Stop if we get a terminating character
                        if let b'A'..=b'Z' | b'a'..=b'z' | b'~' = temp_buf[0] {
                            break;
                        }
                        // Limit sequence length
                        if sequence.len() > 20 {
                            break;
                        }
                    }
                }
            }

            // If only Esc byte, return Esc key
            if sequence.len() == 1 {
                return Ok(Key::Esc);
            }

            // Parse the escape sequence
            Ok(parse_escape_sequence(&sequence))
        }
        // Special control characters (handle before Ctrl range)
        b'\r' | b'\n' => Ok(Key::Enter),
        b'\t' => Ok(Key::Tab),
        b'\x08' => Ok(Key::Backspace), // Backspace character
        b'\x7f' => Ok(Key::Backspace), // DEL character
        // NUL character: Ctrl-@
        b'\x00' => Ok(Key::Ctrl('@')),
        // Control characters (Ctrl+A through Ctrl+Z, excluding special ones)
        b'\x01'..=b'\x1a' => {
            // Ctrl+A through Ctrl+Z (0x01 = Ctrl+A, 0x1a = Ctrl+Z)
            let c = (buf[0] + b'a' - 1) as char;
            Ok(Key::Ctrl(c))
        }
        b'\x1c' => Ok(Key::Ctrl('\\')),
        b'\x1d' => Ok(Key::Ctrl(']')),
        b'\x1e' => Ok(Key::Ctrl('^')),
        b'\x1f' => Ok(Key::Ctrl('_')),
        // Regular printable characters
        byte => {
            match char::from(byte) {
                c if c.is_ascii() => Ok(Key::Char(c)),
                _ => {
                    // Multi-byte UTF-8 character - read more bytes
                    let mut utf8_buf = vec![byte];
                    let mut temp_buf = [0u8; 1];

                    // UTF-8 continuation bytes arrive immediately, use blocking read
                    while stdin().lock().read_exact(&mut temp_buf).is_ok() {
                        utf8_buf.push(temp_buf[0]);
                        if let Ok(s) = std::str::from_utf8(&utf8_buf) {
                            if let Some(ch) = s.chars().next() {
                                return Ok(Key::Char(ch));
                            }
                        }
                        // If we have 4 bytes and still can't parse, give up
                        if utf8_buf.len() >= 4 {
                            break;
                        }
                    }
                    // Fallback: return as a character if possible
                    if let Ok(s) = std::str::from_utf8(&utf8_buf) {
                        if let Some(ch) = s.chars().next() {
                            Ok(Key::Char(ch))
                        } else {
                            Ok(Key::Char(char::REPLACEMENT_CHARACTER))
                        }
                    } else {
                        Ok(Key::Char(char::REPLACEMENT_CHARACTER))
                    }
                }
            }
        }
    }
}

fn parse_escape_sequence(sequence: &[u8]) -> Key {
    if sequence.len() < 2 {
        return Key::Esc;
    }

    match sequence[1] {
        b'[' => {
            // CSI (Control Sequence Introducer) sequences: ESC [
            if sequence.len() < 3 {
                return Key::UnknownEsc(sequence.to_vec());
            }

            match sequence[2] {
                b'A' => Key::Up,
                b'B' => Key::Down,
                b'C' => Key::Right,
                b'D' => Key::Left,
                b'H' => Key::Home,
                b'F' => Key::End,
                b'1' | b'7' => {
                    // Home (ESC [1~ or ESC [7~) or could be part of F1-F4
                    if sequence.len() >= 4 && sequence[3] == b'~' {
                        match sequence[2] {
                            b'1' | b'7' => Key::Home,
                            _ => Key::UnknownEsc(sequence.to_vec()),
                        }
                    } else if sequence.len() >= 4 && sequence[3] == b'1' && sequence.len() >= 5 {
                        // F1-F4: ESC [1 1 ~, ESC [1 2 ~, etc.
                        match sequence[4] {
                            b'1'..=b'4' if sequence.len() >= 6 && sequence[5] == b'~' => {
                                Key::F(sequence[4] - b'0')
                            }
                            _ => Key::UnknownEsc(sequence.to_vec()),
                        }
                    } else {
                        Key::UnknownEsc(sequence.to_vec())
                    }
                }
                b'2' => {
                    // Insert key (ESC [2~)
                    if sequence.len() >= 4 && sequence[3] == b'~' {
                        Key::Insert
                    } else {
                        Key::UnknownEsc(sequence.to_vec())
                    }
                }
                b'3' => {
                    // Delete key (ESC [3~)
                    if sequence.len() >= 4 && sequence[3] == b'~' {
                        Key::Delete
                    } else {
                        Key::UnknownEsc(sequence.to_vec())
                    }
                }
                b'4'..=b'6' => {
                    // End, Page Up, Page Down
                    if sequence.len() >= 4 && sequence[3] == b'~' {
                        match sequence[2] {
                            b'4' => Key::End,
                            b'5' => Key::PageUp,
                            b'6' => Key::PageDown,
                            _ => Key::UnknownEsc(sequence.to_vec()),
                        }
                    } else {
                        Key::UnknownEsc(sequence.to_vec())
                    }
                }
                b'8' => {
                    // End (ESC [8~)
                    if sequence.len() >= 4 && sequence[3] == b'~' {
                        Key::End
                    } else {
                        Key::UnknownEsc(sequence.to_vec())
                    }
                }
                b'0' | b'9' => {
                    // Could be F1-F4 or part of extended sequence
                    if sequence.len() >= 5 && sequence[sequence.len() - 1] == b'~' {
                        // Try to parse as function key
                        if let Ok(num_str) = std::str::from_utf8(&sequence[2..sequence.len() - 1]) {
                            if let Ok(num) = num_str.parse::<u8>() {
                                if (1..=12).contains(&num) {
                                    return Key::F(num);
                                }
                            }
                        }
                    }
                    Key::UnknownEsc(sequence.to_vec())
                }
                _ => {
                    // Try to parse function keys: ESC [1 1 ~, ESC [1 2 ~, etc.
                    if sequence.len() >= 5 {
                        let nums: Vec<u8> = sequence[2..]
                            .iter()
                            .take_while(|&&b| b.is_ascii_digit())
                            .copied()
                            .collect();
                        if !nums.is_empty() && sequence.len() > 2 + nums.len() {
                            if let Ok(num_str) = std::str::from_utf8(&nums) {
                                if let Ok(num) = num_str.parse::<u8>() {
                                    if (1..=12).contains(&num) {
                                        return Key::F(num);
                                    }
                                }
                            }
                        }
                    }
                    Key::UnknownEsc(sequence.to_vec())
                }
            }
        }
        b'O' => {
            // SS3 (Single Shift 3) sequences: ESC O (usually F1-F4)
            if sequence.len() >= 3 {
                match sequence[2] {
                    b'P' => Key::F(1),
                    b'Q' => Key::F(2),
                    b'R' => Key::F(3),
                    b'S' => Key::F(4),
                    _ => Key::UnknownEsc(sequence.to_vec()),
                }
            } else {
                Key::UnknownEsc(sequence.to_vec())
            }
        }
        // Alt-key combinations: ESC followed by a character
        byte if byte.is_ascii() && !byte.is_ascii_control() => Key::Alt(byte as char),
        _ => Key::UnknownEsc(sequence.to_vec()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_escape_sequence_arrows() {
        assert_eq!(parse_escape_sequence(&[0x1b, b'[', b'A']), Key::Up);
        assert_eq!(parse_escape_sequence(&[0x1b, b'[', b'B']), Key::Down);
        assert_eq!(parse_escape_sequence(&[0x1b, b'[', b'C']), Key::Right);
        assert_eq!(parse_escape_sequence(&[0x1b, b'[', b'D']), Key::Left);
    }

    #[test]
    fn test_parse_escape_sequence_function_keys() {
        assert_eq!(parse_escape_sequence(&[0x1b, b'O', b'P']), Key::F(1));
        assert_eq!(parse_escape_sequence(&[0x1b, b'O', b'Q']), Key::F(2));
    }

    #[test]
    fn test_parse_escape_sequence_special_keys() {
        assert_eq!(parse_escape_sequence(&[0x1b, b'[', b'H']), Key::Home);
        assert_eq!(parse_escape_sequence(&[0x1b, b'[', b'F']), Key::End);
        assert_eq!(
            parse_escape_sequence(&[0x1b, b'[', b'3', b'~']),
            Key::Delete
        );
        assert_eq!(
            parse_escape_sequence(&[0x1b, b'[', b'2', b'~']),
            Key::Insert
        );
    }
}

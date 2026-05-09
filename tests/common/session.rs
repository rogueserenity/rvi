use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const DEFAULT_COLS: u16 = 80;
const DEFAULT_ROWS: u16 = 24;
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const WAIT_TIMEOUT: Duration = Duration::from_secs(8);

/// Serialize PTY sessions within each test binary to avoid two problems:
///   1. macOS PTY pool exhaustion (default kernel limit: 127 pairs). With
///      ~28 test binaries in parallel, 1 per binary → max 28 system-wide.
///   2. CPU contention: concurrent rvi child processes under heavy parallel
///      load cause wait_for_* timeouts even at generous durations.
const MAX_CONCURRENT_SESSIONS: usize = 1;

static PTY_SEMAPHORE: OnceLock<(Mutex<usize>, Condvar)> = OnceLock::new();

fn acquire_pty_permit() {
    let (lock, cvar) = PTY_SEMAPHORE.get_or_init(|| (Mutex::new(0), Condvar::new()));
    let mut count = lock.lock().unwrap();
    while *count >= MAX_CONCURRENT_SESSIONS {
        count = cvar.wait(count).unwrap();
    }
    *count += 1;
}

fn release_pty_permit() {
    let (lock, cvar) = PTY_SEMAPHORE.get_or_init(|| (Mutex::new(0), Condvar::new()));
    let mut count = lock.lock().unwrap();
    *count -= 1;
    cvar.notify_one();
}

pub struct RviSession {
    child: Box<dyn portable_pty::Child + Send>,
    writer: Box<dyn Write + Send>,
    parser: Arc<Mutex<vt100::Parser>>,
    cols: u16,
    rows: u16,
    _reader_thread: JoinHandle<()>,
}

#[allow(dead_code)]
impl RviSession {
    pub fn new() -> Self {
        Self::with_size(DEFAULT_COLS, DEFAULT_ROWS)
    }

    pub fn with_size(cols: u16, rows: u16) -> Self {
        Self::spawn(cols, rows, None)
    }

    pub fn with_file(path: &Path) -> Self {
        Self::spawn(DEFAULT_COLS, DEFAULT_ROWS, Some(path))
    }

    /// Spawn rvi with arbitrary CLI args (flags and/or filenames) and optional
    /// environment variable overrides for the child process.
    pub fn with_args_env(args: &[&str], env_vars: &[(&str, &str)]) -> Self {
        Self::spawn_args_env(DEFAULT_COLS, DEFAULT_ROWS, args, env_vars)
    }

    /// Spawn rvi with arbitrary CLI args (flags and/or filenames).
    pub fn with_args(args: &[&str]) -> Self {
        Self::spawn_args_env(DEFAULT_COLS, DEFAULT_ROWS, args, &[])
    }

    fn spawn(cols: u16, rows: u16, file: Option<&Path>) -> Self {
        let binary = std::env::var("RVI_TEST_BINARY")
            .unwrap_or_else(|_| env!("CARGO_BIN_EXE_rvi").to_string());
        let mut cmd = CommandBuilder::new(&binary);
        if let Some(path) = file {
            cmd.arg(path);
        }
        Self::do_spawn(cols, rows, cmd)
    }

    /// Spawn rvi from a specific working directory.
    pub fn with_cwd(cwd: &Path, args: &[&str]) -> Self {
        let binary = std::env::var("RVI_TEST_BINARY")
            .unwrap_or_else(|_| env!("CARGO_BIN_EXE_rvi").to_string());
        let mut cmd = CommandBuilder::new(&binary);
        cmd.cwd(cwd);
        for arg in args {
            cmd.arg(arg);
        }
        Self::do_spawn(DEFAULT_COLS, DEFAULT_ROWS, cmd)
    }

    fn spawn_args_env(cols: u16, rows: u16, args: &[&str], env_vars: &[(&str, &str)]) -> Self {
        let binary = std::env::var("RVI_TEST_BINARY")
            .unwrap_or_else(|_| env!("CARGO_BIN_EXE_rvi").to_string());
        let mut cmd = CommandBuilder::new(&binary);
        for arg in args {
            cmd.arg(arg);
        }
        for (key, val) in env_vars {
            cmd.env(key, val);
        }
        Self::do_spawn(cols, rows, cmd)
    }

    fn do_spawn(cols: u16, rows: u16, cmd: CommandBuilder) -> Self {
        acquire_pty_permit();
        let pty_system = NativePtySystem::default();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("failed to open PTY");

        let child = pair.slave.spawn_command(cmd).expect("failed to spawn rvi");

        // Drop slave so master gets EOF when child exits
        drop(pair.slave);

        let writer = pair.master.take_writer().expect("failed to get PTY writer");
        let mut reader = pair
            .master
            .try_clone_reader()
            .expect("failed to get PTY reader");

        let parser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 0)));
        let parser_clone = Arc::clone(&parser);

        let reader_thread = thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let mut p = parser_clone.lock().unwrap();
                        p.process(&buf[..n]);
                    }
                }
            }
        });

        RviSession {
            child,
            writer,
            parser,
            cols,
            rows,
            _reader_thread: reader_thread,
        }
    }

    pub fn send_keys(&mut self, keys: &str) {
        self.writer
            .write_all(keys.as_bytes())
            .expect("failed to write to PTY");
        self.writer.flush().expect("failed to flush PTY writer");
    }

    pub fn wait_for_no_text(&mut self, text: &str) -> Result<(), String> {
        let start = Instant::now();
        loop {
            if !self.screen_contains(text) {
                return Ok(());
            }
            if start.elapsed() >= WAIT_TIMEOUT {
                return Err(format!(
                    "timeout waiting for {:?} to disappear\nScreen:\n{}",
                    text,
                    self.full_screen_dump()
                ));
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    pub fn wait_for_text(&mut self, text: &str) -> Result<(), String> {
        let start = Instant::now();
        loop {
            if self.screen_contains(text) {
                return Ok(());
            }
            if start.elapsed() >= WAIT_TIMEOUT {
                return Err(format!(
                    "timeout waiting for {:?}\nScreen:\n{}",
                    text,
                    self.full_screen_dump()
                ));
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    pub fn wait_for_cursor(&mut self, row: usize, col: usize) -> Result<(), String> {
        let start = Instant::now();
        loop {
            let pos = self.cursor_pos();
            if pos == (row, col) {
                return Ok(());
            }
            if start.elapsed() >= WAIT_TIMEOUT {
                return Err(format!(
                    "timeout waiting for cursor at ({row}, {col}), got {pos:?}\nScreen:\n{}",
                    self.full_screen_dump()
                ));
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    pub fn wait_for_cursor_row(&mut self, row: usize) -> Result<(), String> {
        let start = Instant::now();
        loop {
            let (r, _) = self.cursor_pos();
            if r == row {
                return Ok(());
            }
            if start.elapsed() >= WAIT_TIMEOUT {
                return Err(format!(
                    "timeout waiting for cursor row {row}, got {r}\nScreen:\n{}",
                    self.full_screen_dump()
                ));
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    pub fn wait_for_status_prefix(&mut self, prefix: &str) -> Result<(), String> {
        let start = Instant::now();
        loop {
            if self.status_line().starts_with(prefix) {
                return Ok(());
            }
            if start.elapsed() >= WAIT_TIMEOUT {
                return Err(format!(
                    "timeout waiting for status prefix {:?}\nStatus line: {:?}\nScreen:\n{}",
                    prefix,
                    self.status_line(),
                    self.full_screen_dump()
                ));
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    pub fn wait_for_status(&mut self, text: &str) -> Result<(), String> {
        let start = Instant::now();
        loop {
            if self.status_line().contains(text) {
                return Ok(());
            }
            if start.elapsed() >= WAIT_TIMEOUT {
                return Err(format!(
                    "timeout waiting for status {:?}\nStatus line: {:?}\nScreen:\n{}",
                    text,
                    self.status_line(),
                    self.full_screen_dump()
                ));
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    fn screen_contains(&self, text: &str) -> bool {
        let p = self.parser.lock().unwrap();
        let screen = p.screen();
        for row in 0..self.rows as usize {
            let row_text = screen_row_str(screen, row, self.cols as usize);
            if row_text.contains(text) {
                return true;
            }
        }
        false
    }

    pub fn screen_row(&self, row: usize) -> String {
        let p = self.parser.lock().unwrap();
        screen_row_str(p.screen(), row, self.cols as usize)
    }

    pub fn status_line(&self) -> String {
        self.screen_row((self.rows - 1) as usize)
    }

    pub fn cursor_pos(&self) -> (usize, usize) {
        let p = self.parser.lock().unwrap();
        let (row, col) = p.screen().cursor_position();
        (row as usize, col as usize)
    }

    pub fn assert_contains(&self, text: &str) {
        if !self.screen_contains(text) {
            panic!(
                "screen does not contain {:?}\nScreen:\n{}",
                text,
                self.full_screen_dump()
            );
        }
    }

    pub fn assert_row(&self, row: usize, expected: &str) {
        let actual = self.screen_row(row);
        if actual != expected {
            panic!(
                "row {} mismatch\n  expected: {:?}\n  actual:   {:?}\nScreen:\n{}",
                row,
                expected,
                actual,
                self.full_screen_dump()
            );
        }
    }

    fn full_screen_dump(&self) -> String {
        let p = self.parser.lock().unwrap();
        let screen = p.screen();
        let mut out = String::new();
        for row in 0..self.rows as usize {
            out.push_str(&screen_row_str(screen, row, self.cols as usize));
            out.push('\n');
        }
        out
    }
}

impl Drop for RviSession {
    fn drop(&mut self) {
        let _ = self.writer.write_all(b":q!\r");
        let _ = self.writer.flush();
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            if let Ok(Some(_)) = self.child.try_wait() {
                break;
            }
            if Instant::now() >= deadline {
                let _ = self.child.kill();
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        release_pty_permit();
    }
}

fn screen_row_str(screen: &vt100::Screen, row: usize, cols: usize) -> String {
    let mut s = String::new();
    for col in 0..cols {
        if let Some(cell) = screen.cell(row as u16, col as u16) {
            let ch = cell.contents();
            if ch.is_empty() {
                s.push(' ');
            } else {
                s.push_str(&ch);
            }
        }
    }
    s.trim_end().to_string()
}

/// Create a temp file with given content. The returned `TempDir` must be kept
/// alive for the duration of the test to prevent early cleanup.
#[allow(dead_code)]
pub fn temp_file_with_content(content: &str) -> (tempfile::TempDir, PathBuf) {
    use std::io::Write as _;
    let dir = tempfile::TempDir::new().expect("failed to create temp dir");
    let path = dir.path().join("test_file.txt");
    let mut f = std::fs::File::create(&path).expect("failed to create temp file");
    f.write_all(content.as_bytes())
        .expect("failed to write temp file");
    (dir, path)
}

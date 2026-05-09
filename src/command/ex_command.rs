//! Ex command parsing and command-line state management.
//!
//! This module provides:
//! - `CommandLineState` for managing the command-line input buffer, cursor, and prompt
//! - `ExCommand` enum representing parsed ex commands (:w, :q, :wq, :s, :set, etc.)
//! - `ExCommandContext` for buffer state needed during parsing
//! - `parse_ex_command()` for parsing command-line input into `ExCommand` variants

use std::sync::Arc;

use crate::search::substitute::{parse_substitute_body, SubstituteCommand, SubstituteRange};
use crate::settings::{parse_set_command, SetCommand};

use super::ex_range::{
    parse_any_range, parse_dest_addr, parse_range_prefix, resolve_addr, resolve_range, LineAddr,
    RangeSpec,
};

/// State for the command-line input at the bottom of the screen.
///
/// Manages a text buffer with cursor position and a prompt character.
/// This is separate from `ParseState` because it manages a text buffer
/// rather than a command sequence.
///
/// # Examples
///
/// ```
/// use rvi::command::ex_command::CommandLineState;
///
/// let mut state = CommandLineState::new(':');
/// state.insert_char('w');
/// state.insert_char('q');
/// assert_eq!(state.buffer(), "wq");
/// assert_eq!(state.prompt(), ':');
/// ```
#[derive(Debug, Clone, Default)]
pub struct CommandLineState {
    /// The prompt character (typically ':')
    prompt: char,
    /// The input buffer (characters typed after the prompt)
    buffer: String,
    /// Cursor position within the buffer (byte offset)
    cursor: usize,
}

impl CommandLineState {
    /// Create a new command-line state with the given prompt character.
    pub fn new(prompt: char) -> Self {
        Self {
            prompt,
            buffer: String::new(),
            cursor: 0,
        }
    }

    /// Get the prompt character.
    pub fn prompt(&self) -> char {
        self.prompt
    }

    /// Get the current input buffer contents.
    pub fn buffer(&self) -> &str {
        &self.buffer
    }

    /// Get the cursor position (byte offset into buffer).
    pub fn cursor_pos(&self) -> usize {
        self.cursor
    }

    /// Insert a character at the cursor position.
    pub fn insert_char(&mut self, ch: char) {
        self.buffer.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    /// Delete the character before the cursor (backspace).
    ///
    /// Returns `true` if a character was deleted, `false` if the cursor
    /// was already at the beginning of the buffer.
    pub fn delete_char_before_cursor(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        // Find the previous character boundary
        let prev = self.buffer[..self.cursor]
            .char_indices()
            .last()
            .map(|(i, _)| i)
            .unwrap_or(0);
        self.buffer.remove(prev);
        self.cursor = prev;
        true
    }

    /// Move cursor left one character.
    pub fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor = self.buffer[..self.cursor]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
    }

    /// Move cursor right one character.
    pub fn move_right(&mut self) {
        if self.cursor < self.buffer.len() {
            self.cursor = self.buffer[self.cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor + i)
                .unwrap_or(self.buffer.len());
        }
    }

    /// Move cursor to start of buffer.
    pub fn move_to_start(&mut self) {
        self.cursor = 0;
    }

    /// Move cursor to end of buffer.
    pub fn move_to_end(&mut self) {
        self.cursor = self.buffer.len();
    }

    /// Check if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Take the buffer contents, resetting cursor to zero.
    ///
    /// The buffer is left empty after this call.
    pub fn take_buffer(&mut self) -> String {
        self.cursor = 0;
        std::mem::take(&mut self.buffer)
    }
}

/// Context needed for parsing ex commands that reference buffer state.
///
/// Some commands (like `:s`) need to know the current cursor position,
/// buffer size, and mark positions for range resolution.
#[derive(Debug, Clone)]
pub struct ExCommandContext {
    /// Current cursor line (0-based).
    pub current_line: usize,
    /// Total number of lines in the buffer.
    pub buffer_len: usize,
    /// Mark positions (letter → 0-based row), used for `'a` addresses.
    pub marks: std::collections::HashMap<char, usize>,
    /// All buffer lines, used for `/pattern/` and `?pattern?` address resolution.
    /// Stored as `Arc<[String]>` so cloning the context (for semicolon ranges) is O(1).
    pub buffer_lines: Arc<[String]>,
    /// Most recent search pattern, used for `//` and `??` empty-pattern addresses.
    pub last_pattern: Option<String>,
}

impl ExCommandContext {
    /// Create a new context with no marks.
    pub fn new(current_line: usize, buffer_len: usize) -> Self {
        Self {
            current_line,
            buffer_len,
            marks: std::collections::HashMap::new(),
            buffer_lines: Arc::from([]),
            last_pattern: None,
        }
    }

    /// Create a context with marks from the editor state.
    pub fn with_marks(
        current_line: usize,
        buffer_len: usize,
        marks: std::collections::HashMap<char, usize>,
    ) -> Self {
        Self {
            current_line,
            buffer_len,
            marks,
            buffer_lines: Arc::from([]),
            last_pattern: None,
        }
    }
}

impl Default for ExCommandContext {
    fn default() -> Self {
        Self {
            current_line: 0,
            buffer_len: 1,
            marks: std::collections::HashMap::new(),
            buffer_lines: Arc::from([]),
            last_pattern: None,
        }
    }
}

/// Parsed ex command ready for execution.
///
/// Each variant corresponds to a recognized ex command. Commands that
/// take an optional filename argument have separate variants for the
/// no-argument and with-argument forms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExCommand {
    /// `:w` - write current file.
    Write,
    /// `:w!` - force write (bypasses readonly).
    ForceWrite,
    /// `:w {filename}` - write to a specific file.
    WriteAs(String),
    /// `:w! {filename}` - force write to a specific file (bypasses readonly).
    ForceWriteAs(String),
    /// `:q` - quit (fails if unsaved changes exist).
    Quit,
    /// `:q!` - force quit (discard unsaved changes).
    ForceQuit,
    /// `:wq` - write and quit.
    WriteQuit,
    /// `:wq {filename}` - write to file and quit.
    WriteQuitAs(String),
    /// `:wq!` - force write (even if read-only) and quit.
    ForceWriteQuit,
    /// `:x` - write if modified, then quit.
    WriteQuitIfModified,
    /// `:e {filename}` - edit (open) a file.
    Edit(String),
    /// `:e!` - re-read current file (discard changes).
    ForceEdit,
    /// `:s/pattern/replacement/flags` - substitute text.
    Substitute(SubstituteCommand),
    /// `:set` - change or query editor settings.
    Set(SetCommand),
    /// `:nohl` / `:nohlsearch` - suppress search highlights until next search.
    NoHighlight,
    /// `:{n}` - jump to line n (1-based).
    GotoLine(usize),
    /// `:g/pattern/cmd` - execute cmd on all lines matching pattern.
    /// `:v/pattern/cmd` / `:g!/pattern/cmd` - execute cmd on non-matching lines.
    Global {
        pattern: String,
        command: String,
        inverse: bool,
    },
    /// `:!{cmd}` - execute a shell command and display its output.
    Shell(String),
    /// `:r {filename}` - read file and insert below current line.
    ReadFile(String),
    /// `:[addr]=` - print line number of the addressed line (default: last line).
    /// The `usize` is the 0-based line index to display (as 1-based to the user).
    PrintLineNumber(usize),
    /// `:[range]d [register]` - delete lines in range.
    DeleteLines {
        range: SubstituteRange,
        register: Option<char>,
    },
    /// `:[range]y [register]` - yank lines in range into register.
    YankLines {
        range: SubstituteRange,
        register: Option<char>,
    },
    /// `:[range]m {address}` - move lines to after address line.
    MoveLines { range: SubstituteRange, dest: usize },
    /// `:[range]t {address}` (copy) - copy lines to after address line.
    CopyLines { range: SubstituteRange, dest: usize },
    /// `:[range]j` - join lines in range.
    JoinLines { range: SubstituteRange },
    /// `:n [file]` — go to next file in argument list (or open a specific file).
    NextFile(Option<String>),
    /// `:N` — go to previous file in argument list.
    PrevFile,
    /// `:args` — display the argument list.
    ShowArgs,
    /// `:rewind` / `:rew` — go to first file in argument list.
    RewindArgs,
    /// `:r !{cmd}` — read output of shell command into buffer.
    ReadShellCommand { cmd: String },
    /// `:[range]p` — print lines.
    PrintLines { range: SubstituteRange },
    /// `:[range]l` — print lines in list format (tabs as `^I`, `$` at end).
    PrintListLines { range: SubstituteRange },
    /// `:[range]nu` / `:[range]#` — print lines with line numbers.
    PrintNumberedLines { range: SubstituteRange },
    /// `:sh` — start an interactive shell.
    ShellInteractive,
    /// `:cd {dir}` / `:chdir {dir}` — change working directory.
    ChangeDir(String),
    /// `:source {file}` / `:so {file}` — execute ex commands from file.
    Source(String),
    /// `:mark {a}` / `:ma {a}` / `:k{a}` — set mark at current line.
    SetMark(char),
    /// `:suspend` / `:stop` / `:su` / `:st` — suspend the editor (SIGTSTP).
    Suspend,
    /// `:[addr]a[ppend]` — collect lines from the user and insert after addr.
    ///
    /// `insert_at` is the 0-based row at which to insert (i.e., after the
    /// addressed line, which is already computed at parse time).
    AppendLines { insert_at: usize },
    /// `:[addr]i[nsert]` — collect lines from the user and insert before addr.
    ///
    /// `insert_at` is the 0-based row at which to insert.
    InsertLines { insert_at: usize },
    /// `:[range]c[hange]` — delete range then collect replacement lines.
    ///
    /// `start`/`end` are 0-based. The addressed lines are deleted first, then
    /// new lines are collected until a lone `.` terminates input.
    ChangeLines { start: usize, end: usize },
    /// `:version` / `:ve` — display editor version.
    Version,
    /// `:map lhs rhs` — map keys in normal mode.
    Map {
        insert_mode: bool,
        lhs: String,
        rhs: String,
    },
    /// `:unmap lhs` — remove a key mapping.
    Unmap { insert_mode: bool, lhs: String },
    /// Bare `:map` or `:map!` — show all mappings.
    ShowMaps { insert_mode: bool },
    /// `:ab[breviate] lhs rhs` — define an insert-mode abbreviation.
    Abbreviate { lhs: String, rhs: String },
    /// `:una[bbreviate] lhs` — remove an abbreviation.
    Unabbreviate { lhs: String },
    /// Bare `:ab[breviate]` — show all abbreviations.
    ShowAbbreviations,
    /// `:vi[sual][!]` — exit ex mode and return to full-screen visual mode.
    Visual,
    /// `:pre[serve]` — write buffer to a crash-recovery temp file.
    Preserve,
    /// `:rec[over] [file]` — reload buffer from a crash-recovery temp file.
    Recover(Option<String>),
    /// `:ta[g] {name}` — jump to the named tag.
    Tag(String),
    /// `:po[p]` — pop the tag stack (return from `Ctrl-]` / `:tag` jump).
    PopTag,
    /// `:[addr]pu[t] [x]` — put contents of register `x` after the addressed line.
    ///
    /// `after_row` is the 0-based row to insert after (buffer row, not 1-based line number).
    /// If the register holds character-wise content, it is treated as a single line.
    Put {
        after_row: usize,
        register: Option<char>,
    },
    /// `:[range]~` — repeat the last `:s` replacement using the most recent search pattern.
    ///
    /// Differs from `&` which reuses the full last `:s` command (pattern + replacement).
    /// `:~` uses whatever pattern was most recently set (from `/`, `?`, `:s`, etc.) but
    /// only the replacement string from the last `:s`.
    RepeatSubstituteTilde { range: SubstituteRange },
    /// `:[range]s` or `:[range]&` — repeat the last substitute command (POSIX).
    /// Like the normal-mode `&`, but accepts a range.
    RepeatSubstitute { range: SubstituteRange },
}

/// Parse a command-line string (without the leading ':') into an `ExCommand`.
///
/// The input is trimmed of leading and trailing whitespace before parsing.
/// Uses the provided context for resolving line ranges in substitute commands.
///
/// # Returns
///
/// - `Ok(ExCommand)` on success
/// - `Err(String)` with a descriptive error message on failure
/// - `Err(String::new())` for empty input (signals "do nothing")
///
/// # Examples
///
/// ```
/// use rvi::command::ex_command::{parse_ex_command, ExCommand, ExCommandContext};
///
/// let ctx = ExCommandContext::default();
/// assert_eq!(parse_ex_command("w", &ctx), Ok(ExCommand::Write));
/// assert_eq!(parse_ex_command("q!", &ctx), Ok(ExCommand::ForceQuit));
/// assert_eq!(
///     parse_ex_command("w foo.txt", &ctx),
///     Ok(ExCommand::WriteAs("foo.txt".to_string()))
/// );
/// ```
pub fn parse_ex_command(input: &str, ctx: &ExCommandContext) -> Result<ExCommand, String> {
    let input = input.trim();

    if input.is_empty() {
        return Err(String::new());
    }

    // Try to parse as substitute command first (handles range prefixes)
    if let Some(result) = try_parse_substitute(input, ctx) {
        return result;
    }

    // Check for :set commands (must be before exact matches to handle "set" prefix)
    // Accept any whitespace after "set" or "se" (space or tab)
    if let Some(body) = input
        .strip_prefix("set")
        .or_else(|| input.strip_prefix("se"))
    {
        let trimmed = body.trim_start();
        if trimmed.is_empty() {
            // Bare "set" or "se" with no arguments shows all settings
            return Ok(ExCommand::Set(SetCommand::QueryAll));
        } else {
            return parse_set_command(trimmed).map(ExCommand::Set);
        }
    }

    // :nohl / :nohlsearch (check before exact match table to avoid conflict)
    if input == "nohl" || input == "nohlsearch" || input == "noh" {
        return Ok(ExCommand::NoHighlight);
    }

    // Bare line number: :42 jumps to line 42
    if input.chars().all(|c| c.is_ascii_digit()) {
        if let Ok(n) = input.parse::<usize>() {
            return Ok(ExCommand::GotoLine(n));
        }
    }

    // :!{cmd} — execute shell command
    if let Some(shell_cmd) = input.strip_prefix('!') {
        return Ok(ExCommand::Shell(shell_cmd.to_string()));
    }

    // :g/pattern/cmd — global command; :g!/pattern/cmd or :v/pattern/cmd — inverse global
    if let Some(rest) = input.strip_prefix('g') {
        if let Some(rest2) = rest.strip_prefix('!') {
            // :g!/pattern/cmd — inverse global
            if let Some(result) = try_parse_global(rest2) {
                return result.map(|cmd| {
                    if let ExCommand::Global {
                        pattern, command, ..
                    } = cmd
                    {
                        ExCommand::Global {
                            pattern,
                            command,
                            inverse: true,
                        }
                    } else {
                        cmd
                    }
                });
            }
        } else if let Some(result) = try_parse_global(rest) {
            return result;
        }
    }

    // :v/pattern/cmd — inverse global (equivalent to :g!/pattern/cmd)
    if let Some(rest) = input.strip_prefix('v') {
        if let Some(result) = try_parse_global(rest) {
            return result.map(|cmd| {
                if let ExCommand::Global {
                    pattern, command, ..
                } = cmd
                {
                    ExCommand::Global {
                        pattern,
                        command,
                        inverse: true,
                    }
                } else {
                    cmd
                }
            });
        }
    }

    // Try exact matches first
    match input {
        "vi" | "vis" | "visu" | "visua" | "visual" | "vi!" | "vis!" | "visu!" | "visua!"
        | "visual!" => return Ok(ExCommand::Visual),
        "q" | "quit" => return Ok(ExCommand::Quit),
        "q!" | "quit!" => return Ok(ExCommand::ForceQuit),
        "w" | "write" => return Ok(ExCommand::Write),
        "w!" | "write!" => return Ok(ExCommand::ForceWrite),
        "wq" => return Ok(ExCommand::WriteQuit),
        "wq!" => return Ok(ExCommand::ForceWriteQuit),
        "x" | "xit" | "exit" => return Ok(ExCommand::WriteQuitIfModified),
        "e!" | "edit!" => return Ok(ExCommand::ForceEdit),
        // :e without a filename is an error (requires argument)
        "e" | "edit" => return Err("No file name".to_string()),
        "ve" | "version" => return Ok(ExCommand::Version),
        "sh" => return Ok(ExCommand::ShellInteractive),
        "suspend" | "su" | "stop" | "st" => return Ok(ExCommand::Suspend),
        _ => {}
    }

    // Commands with arguments: try prefix + space + argument
    if let Some(filename) = input
        .strip_prefix("w ")
        .or_else(|| input.strip_prefix("write "))
    {
        let filename = filename.trim_start();
        if filename.is_empty() {
            return Ok(ExCommand::Write);
        }
        return Ok(ExCommand::WriteAs(filename.to_string()));
    }

    if let Some(filename) = input
        .strip_prefix("w! ")
        .or_else(|| input.strip_prefix("write! "))
    {
        let filename = filename.trim_start();
        if filename.is_empty() {
            return Ok(ExCommand::ForceWrite);
        }
        return Ok(ExCommand::ForceWriteAs(filename.to_string()));
    }

    if let Some(filename) = input.strip_prefix("wq ") {
        let filename = filename.trim_start();
        if filename.is_empty() {
            return Ok(ExCommand::WriteQuit);
        }
        return Ok(ExCommand::WriteQuitAs(filename.to_string()));
    }

    if let Some(filename) = input
        .strip_prefix("e ")
        .or_else(|| input.strip_prefix("edit "))
    {
        let filename = filename.trim_start();
        if filename.is_empty() {
            return Err("No file name".to_string());
        }
        return Ok(ExCommand::Edit(filename.to_string()));
    }

    // :cd / :chdir — change working directory
    if let Some(dir) = input
        .strip_prefix("cd ")
        .or_else(|| input.strip_prefix("chdir "))
    {
        let dir = dir.trim_start();
        if dir.is_empty() {
            return Err("No directory name".to_string());
        }
        return Ok(ExCommand::ChangeDir(dir.to_string()));
    }
    if input == "cd" || input == "chdir" {
        return Err("No directory name".to_string());
    }

    // :source / :so — execute ex commands from file
    if let Some(file) = input
        .strip_prefix("source ")
        .or_else(|| input.strip_prefix("so "))
    {
        let file = file.trim_start();
        if file.is_empty() {
            return Err("No file name".to_string());
        }
        return Ok(ExCommand::Source(file.to_string()));
    }
    if input == "source" || input == "so" {
        return Err("No file name".to_string());
    }

    // :mark {a} / :ma {a} — set mark (with space before mark letter)
    if let Some(rest) = input
        .strip_prefix("mark ")
        .or_else(|| input.strip_prefix("ma "))
    {
        let rest = rest.trim_start();
        let ch = rest
            .chars()
            .next()
            .ok_or_else(|| "Missing mark name".to_string())?;
        if !ch.is_ascii_lowercase() {
            return Err(format!("Invalid mark name: '{}'", ch));
        }
        if !rest[ch.len_utf8()..].trim().is_empty() {
            return Err("Trailing characters".to_string());
        }
        return Ok(ExCommand::SetMark(ch));
    }
    if input == "mark" || input == "ma" {
        return Err("Missing mark name".to_string());
    }

    // :k{a} — compact form: prefix 'k' immediately followed by a single lowercase letter
    if let Some(rest) = input.strip_prefix('k') {
        if rest.is_empty() {
            return Err("Missing mark name".to_string());
        }
        if rest.len() == 1 {
            let ch = rest.chars().next().unwrap();
            if ch.is_ascii_lowercase() {
                return Ok(ExCommand::SetMark(ch));
            }
            return Err(format!("Invalid mark name: '{}'", ch));
        }
        // rest.len() > 1 — fall through (could be another command like :kbd etc.)
    }

    // :r {filename} or :r !{cmd} — read file or shell command output
    if let Some(filename) = input
        .strip_prefix("r ")
        .or_else(|| input.strip_prefix("read "))
    {
        let filename = filename.trim_start();
        if filename.is_empty() {
            return Err("No file name".to_string());
        }
        if let Some(cmd) = filename.strip_prefix('!') {
            return Ok(ExCommand::ReadShellCommand {
                cmd: cmd.to_string(),
            });
        }
        return Ok(ExCommand::ReadFile(filename.to_string()));
    }

    // :map / :map! / :unmap / :unmap!
    if input == "map" {
        return Ok(ExCommand::ShowMaps { insert_mode: false });
    }
    if input == "map!" {
        return Ok(ExCommand::ShowMaps { insert_mode: true });
    }
    if let Some(rest) = input.strip_prefix("map! ") {
        return parse_map_command(rest, true);
    }
    if let Some(rest) = input.strip_prefix("map ") {
        return parse_map_command(rest, false);
    }
    if input == "unmap!" || input.starts_with("unmap! ") {
        let lhs = input.strip_prefix("unmap!").unwrap_or("").trim();
        if lhs.is_empty() {
            return Err("E474: Invalid argument".to_string());
        }
        return Ok(ExCommand::Unmap {
            insert_mode: true,
            lhs: lhs.to_string(),
        });
    }
    if let Some(rest) = input.strip_prefix("unmap ") {
        let lhs = rest.trim();
        if lhs.is_empty() {
            return Err("E474: Invalid argument".to_string());
        }
        return Ok(ExCommand::Unmap {
            insert_mode: false,
            lhs: lhs.to_string(),
        });
    }

    // :ab[breviate] / :una[bbreviate]
    if input == "ab" || input == "abbreviate" {
        return Ok(ExCommand::ShowAbbreviations);
    }
    if let Some(rest) = input
        .strip_prefix("abbreviate ")
        .or_else(|| input.strip_prefix("ab "))
    {
        return parse_abbreviate_command(rest);
    }
    if let Some(rest) = input
        .strip_prefix("unabbreviate ")
        .or_else(|| input.strip_prefix("una "))
    {
        let lhs = rest.trim();
        if lhs.is_empty() {
            return Err("E474: Invalid argument".to_string());
        }
        return Ok(ExCommand::Unabbreviate {
            lhs: lhs.to_string(),
        });
    }

    // :ta[g] {name} — jump to tag (must be before try_parse_range_command which handles 't')
    if let Some(rest) = input
        .strip_prefix("tag")
        .filter(|s| s.starts_with([' ', '\t']))
        .or_else(|| {
            input
                .strip_prefix("ta")
                .filter(|s| s.starts_with([' ', '\t']))
        })
    {
        let name = rest.trim();
        if name.is_empty() {
            return Err("Usage: :tag {name}".to_string());
        }
        return Ok(ExCommand::Tag(name.to_string()));
    }
    if input == "tag" || input == "ta" {
        return Err("Usage: :tag {name}".to_string());
    }

    // :po[p] — pop tag stack
    if input == "pop" || input == "po" {
        return Ok(ExCommand::PopTag);
    }

    // Try range-based commands: d, y, m, t, j, =, p
    if let Some(result) = try_parse_range_command(input, ctx) {
        return result;
    }

    // :n [file] — next file / open file
    if input == "n" || input == "next" {
        return Ok(ExCommand::NextFile(None));
    }
    if let Some(rest) = input
        .strip_prefix("n ")
        .or_else(|| input.strip_prefix("next "))
    {
        let f = rest.trim();
        if f.is_empty() {
            return Ok(ExCommand::NextFile(None));
        }
        return Ok(ExCommand::NextFile(Some(f.to_string())));
    }

    // :N — previous file
    if input == "N" || input == "prev" || input == "previous" {
        return Ok(ExCommand::PrevFile);
    }

    // :args — show argument list
    if input == "args" || input == "ar" {
        return Ok(ExCommand::ShowArgs);
    }

    // :rewind / :rew — go to first file
    if input == "rewind" || input == "rew" {
        return Ok(ExCommand::RewindArgs);
    }

    // :pre[serve] — crash-recovery preserve
    if matches!(input, "pre" | "pres" | "prese" | "preserv" | "preserve") {
        return Ok(ExCommand::Preserve);
    }

    // :rec[over] [file] — crash-recovery recover
    if matches!(input, "rec" | "reco" | "recov" | "recove" | "recover") {
        return Ok(ExCommand::Recover(None));
    }
    if let Some(filename) = input
        .strip_prefix("recover ")
        .or_else(|| input.strip_prefix("recove "))
        .or_else(|| input.strip_prefix("recov "))
        .or_else(|| input.strip_prefix("reco "))
        .or_else(|| input.strip_prefix("rec "))
    {
        let filename = filename.trim_start();
        if filename.is_empty() {
            return Ok(ExCommand::Recover(None));
        }
        return Ok(ExCommand::Recover(Some(filename.to_string())));
    }

    Err(format!("Not an editor command: {}", input))
}

/// Try to parse `:g/pattern/cmd` style global commands.
///
/// `rest` is the string after the leading `g` character (e.g. `/foo/d`).
/// Returns `None` if the string doesn't look like a global command.
fn try_parse_global(rest: &str) -> Option<Result<ExCommand, String>> {
    let bytes = rest.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let delimiter = bytes[0] as char;
    if delimiter.is_alphanumeric() || delimiter == ' ' {
        return None;
    }

    // Split on delimiter: /pattern/cmd
    let after_open = &rest[1..];
    let delim_str: &[char] = &[delimiter];
    let mut parts = after_open.splitn(2, delim_str);
    let pattern = parts.next()?;
    let command = parts.next().unwrap_or("").trim().to_string();

    Some(Ok(ExCommand::Global {
        pattern: pattern.to_string(),
        command,
        inverse: false,
    }))
}

/// Try to parse the input as a substitute command with optional range prefix.
///
/// Detects patterns like:
/// - `s/...`         - current line
/// - `%s/...`        - whole file
/// - `5s/...`        - line 5
/// - `5,10s/...`     - lines 5-10
/// - `.s/...`        - current line (explicit)
/// - `.,$s/...`      - current line to end
/// - `1,$s/...`      - whole file (alternative to %)
///
/// Returns `None` if the input does not look like a substitute command.
fn try_parse_substitute(input: &str, ctx: &ExCommandContext) -> Option<Result<ExCommand, String>> {
    // Find where 's' appears, preceded by optional range
    let bytes = input.as_bytes();

    // Quick check: must contain 's' followed by a non-alphanumeric delimiter
    let (range, body_start) = parse_range_prefix(input, ctx)?;

    // Verify we have 's' at body_start
    if body_start >= input.len() || bytes[body_start] != b's' {
        return None;
    }

    let after_s = body_start + 1;

    // After 's' there must be a delimiter character (non-alphanumeric), or the
    // command is a bare :s (with optional range) which repeats the last substitute (POSIX).
    if after_s >= input.len() {
        let resolved_range = match resolve_range(range, ctx) {
            Ok(r) => r,
            Err(e) => return Some(Err(e)),
        };
        return Some(Ok(ExCommand::RepeatSubstitute {
            range: resolved_range,
        }));
    }

    let next_char = input[after_s..].chars().next().unwrap();
    if next_char.is_alphanumeric() || next_char == ' ' {
        return None;
    }

    // Parse the substitute body
    let body = &input[after_s..];
    let (pattern, replacement, flags) = match parse_substitute_body(body) {
        Ok(result) => result,
        Err(e) => return Some(Err(e)),
    };

    // Validate range
    let resolved_range = match resolve_range(range, ctx) {
        Ok(r) => r,
        Err(e) => return Some(Err(e)),
    };

    Some(Ok(ExCommand::Substitute(SubstituteCommand {
        range: resolved_range,
        pattern,
        replacement,
        flags,
    })))
}

/// Parse range-prefixed ex commands: `:d`, `:y`, `:m`, `:t`, `:j`, `:=`.
///
/// Handles commands of the form `[range]cmd [arg]`, where the range is
/// optional and follows standard vi address syntax.
fn try_parse_range_command(
    input: &str,
    ctx: &ExCommandContext,
) -> Option<Result<ExCommand, String>> {
    let bytes = input.as_bytes();
    if bytes.is_empty() {
        return None;
    }

    // Parse the optional range prefix (may be empty)
    let (range_spec, cmd_start) = parse_any_range(input, ctx);

    if cmd_start >= input.len() {
        // Bare address with no command — move cursor to that line (POSIX vi).
        // A lone `$` moves to the last line; `.` stays on current line; `N` goes to line N.
        let line_1based = match range_spec {
            RangeSpec::None => return None, // empty input, handled elsewhere
            RangeSpec::WholeFile => ctx.buffer_len, // `%` alone — go to last line
            RangeSpec::Single(ref addr) => match addr {
                LineAddr::Last => ctx.buffer_len,
                LineAddr::Current => ctx.current_line + 1,
                LineAddr::Number(n) => *n,
                _ => match resolve_addr(addr.clone(), ctx) {
                    Ok(row) => row + 1,
                    Err(e) => return Some(Err(e)),
                },
            },
            RangeSpec::Pair(_, ref addr2) => match addr2 {
                LineAddr::Last => ctx.buffer_len,
                LineAddr::Current => ctx.current_line + 1,
                LineAddr::Number(n) => *n,
                _ => match resolve_addr(addr2.clone(), ctx) {
                    Ok(row) => row + 1,
                    Err(e) => return Some(Err(e)),
                },
            },
            // For a bare semicolon-pair (no command), go to the last address,
            // resolving it relative to addr1.
            RangeSpec::SemicolonPair(ref addr1, ref addr2) => {
                let start = match resolve_addr(addr1.clone(), ctx) {
                    Ok(row) => row,
                    Err(e) => return Some(Err(e)),
                };
                let ctx2 = ExCommandContext {
                    current_line: start,
                    ..ctx.clone()
                };
                match resolve_addr(addr2.clone(), &ctx2) {
                    Ok(row) => row + 1,
                    Err(e) => return Some(Err(e)),
                }
            }
        };
        return Some(Ok(ExCommand::GotoLine(line_1based)));
    }

    let cmd_char = bytes[cmd_start] as char;
    let after_cmd = &input[cmd_start + 1..];
    let arg = after_cmd.trim();

    match cmd_char {
        // :d [register] — delete lines
        'd' => {
            let register = parse_optional_register(arg);
            let range = match resolve_range(range_spec, ctx) {
                Ok(r) => r,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(ExCommand::DeleteLines { range, register }))
        }
        // :y [register] — yank lines
        'y' => {
            let register = parse_optional_register(arg);
            let range = match resolve_range(range_spec, ctx) {
                Ok(r) => r,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(ExCommand::YankLines { range, register }))
        }
        // :m {addr} — move lines to after addr
        'm' if !arg.is_empty() => {
            let dest = match parse_dest_addr(arg, ctx) {
                Ok(d) => d,
                Err(e) => return Some(Err(e)),
            };
            let range = match resolve_range(range_spec, ctx) {
                Ok(r) => r,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(ExCommand::MoveLines { range, dest }))
        }
        // :t {addr} (also :co) — copy lines to after addr
        't' if !arg.is_empty() => {
            let dest = match parse_dest_addr(arg, ctx) {
                Ok(d) => d,
                Err(e) => return Some(Err(e)),
            };
            let range = match resolve_range(range_spec, ctx) {
                Ok(r) => r,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(ExCommand::CopyLines { range, dest }))
        }
        // :m without address — error
        'm' => Some(Err("E14: Address required".to_string())),
        // :t without address — error
        't' => Some(Err("E14: Address required".to_string())),
        // :j — join lines
        'j' => {
            let range = match resolve_range(range_spec, ctx) {
                Ok(r) => r,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(ExCommand::JoinLines { range }))
        }
        // :p — print lines (only bare 'p', not 'prev' etc.)
        'p' if arg.is_empty() => {
            let range = match resolve_range(range_spec, ctx) {
                Ok(r) => r,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(ExCommand::PrintLines { range }))
        }
        // :pu[t] [x] — put register contents after addressed line
        'p' if arg == "u" || arg == "ut" || arg.starts_with("u ") || arg.starts_with("ut ") => {
            // Strip "u" or "ut" prefix, then optional whitespace, to get register char
            let rest = if let Some(r) = arg.strip_prefix("ut") {
                r
            } else {
                arg.strip_prefix('u').unwrap_or(arg)
            };
            let rest = rest.trim();
            let register = if rest.is_empty() {
                None
            } else {
                parse_optional_register(rest)
            };
            // Resolve the after-row: the addressed line (0-based), default = current line
            let after_row = match range_spec {
                RangeSpec::None => ctx.current_line,
                _ => match resolve_range(range_spec, ctx) {
                    Ok(SubstituteRange::CurrentLine) => ctx.current_line,
                    Ok(SubstituteRange::Line(n)) => n,
                    Ok(SubstituteRange::Range { end, .. }) => end,
                    Ok(SubstituteRange::WholeFile) => ctx.buffer_len.saturating_sub(1),
                    Err(e) => return Some(Err(e)),
                },
            };
            Some(Ok(ExCommand::Put {
                after_row,
                register,
            }))
        }
        // :l — print lines in list format (tabs as ^I, $ at end)
        'l' if arg.is_empty() => {
            let range = match resolve_range(range_spec, ctx) {
                Ok(r) => r,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(ExCommand::PrintListLines { range }))
        }
        // :# — print lines with line numbers (synonym for :nu)
        '#' => {
            let range = match resolve_range(range_spec, ctx) {
                Ok(r) => r,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(ExCommand::PrintNumberedLines { range }))
        }
        // :nu / :number — print lines with line numbers
        'n' if arg == "u" || arg == "umber" => {
            let range = match resolve_range(range_spec, ctx) {
                Ok(r) => r,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(ExCommand::PrintNumberedLines { range }))
        }
        // := — print line number of last line in range (default: last line of file)
        '=' => {
            // Resolve the range and pick the last line; fall back to last buffer line.
            let target = match resolve_range(range_spec, ctx) {
                Ok(SubstituteRange::CurrentLine) => ctx.current_line,
                Ok(SubstituteRange::Line(n)) => n,
                Ok(SubstituteRange::Range { end, .. }) => end,
                Ok(SubstituteRange::WholeFile) => ctx.buffer_len.saturating_sub(1),
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(ExCommand::PrintLineNumber(target)))
        }

        // :~ — repeat last :s replacement with current search pattern
        '~' if arg.is_empty() => {
            let range = match resolve_range(range_spec, ctx) {
                Ok(r) => r,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(ExCommand::RepeatSubstituteTilde { range }))
        }

        // :a[ppend] — insert lines after addressed line
        'a' if arg.is_empty() || arg == "ppend" => {
            // Address 0 is valid for :a (means "before line 1", insert at top).
            let insert_at = match range_spec {
                RangeSpec::Single(LineAddr::Number(0)) => 0,
                RangeSpec::None => ctx.current_line + 1,
                _ => {
                    let range = match resolve_range(range_spec, ctx) {
                        Ok(r) => r,
                        Err(e) => return Some(Err(e)),
                    };
                    match range {
                        SubstituteRange::CurrentLine => ctx.current_line + 1,
                        SubstituteRange::Line(n) => n + 1,
                        SubstituteRange::Range { end, .. } => end + 1,
                        SubstituteRange::WholeFile => ctx.buffer_len,
                    }
                }
            };
            Some(Ok(ExCommand::AppendLines { insert_at }))
        }

        // :i[nsert] — insert lines before addressed line
        'i' if arg.is_empty() || arg == "nsert" => {
            let insert_at = match range_spec {
                RangeSpec::Single(LineAddr::Number(0)) => 0,
                RangeSpec::None => ctx.current_line,
                _ => {
                    let range = match resolve_range(range_spec, ctx) {
                        Ok(r) => r,
                        Err(e) => return Some(Err(e)),
                    };
                    match range {
                        SubstituteRange::CurrentLine => ctx.current_line,
                        SubstituteRange::Line(n) => n,
                        SubstituteRange::Range { end, .. } => end,
                        SubstituteRange::WholeFile => ctx.buffer_len.saturating_sub(1),
                    }
                }
            };
            Some(Ok(ExCommand::InsertLines { insert_at }))
        }

        // :c[hange] — delete range and insert replacement lines
        'c' if arg.is_empty() || arg == "hange" => {
            let range = match resolve_range(range_spec, ctx) {
                Ok(r) => r,
                Err(e) => return Some(Err(e)),
            };
            let (start, end) = match range {
                SubstituteRange::CurrentLine => (ctx.current_line, ctx.current_line),
                SubstituteRange::Line(n) => (n, n),
                SubstituteRange::Range { start, end } => (start, end),
                SubstituteRange::WholeFile => (0, ctx.buffer_len.saturating_sub(1)),
            };
            Some(Ok(ExCommand::ChangeLines { start, end }))
        }

        _ => None,
    }
}

/// Parse an optional single register character from a command argument.
///
/// Returns `Some(char)` if the argument is a single valid register letter/digit,
/// or `None` if the argument is empty or not a valid register character.
fn parse_optional_register(arg: &str) -> Option<char> {
    let mut chars = arg.chars();
    let c = chars.next()?;
    if chars.next().is_none() && (c.is_ascii_alphabetic() || c.is_ascii_digit() || c == '"') {
        Some(c)
    } else {
        None
    }
}

/// Parse `:abbreviate lhs rhs` command body.
fn parse_abbreviate_command(rest: &str) -> Result<ExCommand, String> {
    let rest = rest.trim();
    if rest.is_empty() {
        return Ok(ExCommand::ShowAbbreviations);
    }
    let mut parts = rest.splitn(2, char::is_whitespace);
    let lhs = parts.next().unwrap_or("").to_string();
    let rhs = parts.next().unwrap_or("").trim().to_string();
    if rhs.is_empty() {
        // Only lhs given — treat as show (matching vi behavior)
        return Ok(ExCommand::ShowAbbreviations);
    }
    Ok(ExCommand::Abbreviate { lhs, rhs })
}

/// Parse `:map lhs rhs` or `:map! lhs rhs` command body.
fn parse_map_command(rest: &str, insert_mode: bool) -> Result<ExCommand, String> {
    let rest = rest.trim();
    if rest.is_empty() {
        return Ok(ExCommand::ShowMaps { insert_mode });
    }
    // Split into lhs and rhs at first whitespace
    let mut parts = rest.splitn(2, char::is_whitespace);
    let lhs = parts.next().unwrap_or("").to_string();
    let rhs = parts.next().unwrap_or("").trim().to_string();
    if rhs.is_empty() {
        // Show mapping for specific lhs (treat as show)
        return Ok(ExCommand::ShowMaps { insert_mode });
    }
    Ok(ExCommand::Map {
        insert_mode,
        lhs,
        rhs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // CommandLineState tests
    // =========================================================================

    #[test]
    fn test_new_state() {
        let state = CommandLineState::new(':');
        assert_eq!(state.prompt(), ':');
        assert_eq!(state.buffer(), "");
        assert_eq!(state.cursor_pos(), 0);
        assert!(state.is_empty());
    }

    #[test]
    fn test_default_state() {
        let state = CommandLineState::default();
        assert_eq!(state.prompt(), '\0');
        assert_eq!(state.buffer(), "");
        assert_eq!(state.cursor_pos(), 0);
    }

    #[test]
    fn test_insert_char_at_start() {
        let mut state = CommandLineState::new(':');
        state.insert_char('a');
        assert_eq!(state.buffer(), "a");
        assert_eq!(state.cursor_pos(), 1);
    }

    #[test]
    fn test_insert_char_at_end() {
        let mut state = CommandLineState::new(':');
        state.insert_char('a');
        state.insert_char('b');
        state.insert_char('c');
        assert_eq!(state.buffer(), "abc");
        assert_eq!(state.cursor_pos(), 3);
    }

    #[test]
    fn test_insert_char_in_middle() {
        let mut state = CommandLineState::new(':');
        state.insert_char('a');
        state.insert_char('c');
        state.move_left();
        state.insert_char('b');
        assert_eq!(state.buffer(), "abc");
        assert_eq!(state.cursor_pos(), 2);
    }

    #[test]
    fn test_insert_unicode_char() {
        let mut state = CommandLineState::new(':');
        state.insert_char('\u{4F60}'); // CJK character (3 bytes)
        assert_eq!(state.buffer(), "\u{4F60}");
        assert_eq!(state.cursor_pos(), 3);
        state.insert_char('a');
        assert_eq!(state.buffer(), "\u{4F60}a");
        assert_eq!(state.cursor_pos(), 4);
    }

    #[test]
    fn test_delete_char_at_start_returns_false() {
        let mut state = CommandLineState::new(':');
        assert!(!state.delete_char_before_cursor());
    }

    #[test]
    fn test_delete_char_at_end() {
        let mut state = CommandLineState::new(':');
        state.insert_char('a');
        state.insert_char('b');
        assert!(state.delete_char_before_cursor());
        assert_eq!(state.buffer(), "a");
        assert_eq!(state.cursor_pos(), 1);
    }

    #[test]
    fn test_delete_char_in_middle() {
        let mut state = CommandLineState::new(':');
        state.insert_char('a');
        state.insert_char('b');
        state.insert_char('c');
        state.move_left();
        assert!(state.delete_char_before_cursor());
        assert_eq!(state.buffer(), "ac");
        assert_eq!(state.cursor_pos(), 1);
    }

    #[test]
    fn test_delete_unicode_char() {
        let mut state = CommandLineState::new(':');
        state.insert_char('\u{4F60}');
        state.insert_char('\u{597D}');
        assert!(state.delete_char_before_cursor());
        assert_eq!(state.buffer(), "\u{4F60}");
        assert_eq!(state.cursor_pos(), 3);
    }

    #[test]
    fn test_move_left_at_start() {
        let mut state = CommandLineState::new(':');
        state.move_left(); // Should do nothing
        assert_eq!(state.cursor_pos(), 0);
    }

    #[test]
    fn test_move_left() {
        let mut state = CommandLineState::new(':');
        state.insert_char('a');
        state.insert_char('b');
        state.move_left();
        assert_eq!(state.cursor_pos(), 1);
        state.move_left();
        assert_eq!(state.cursor_pos(), 0);
    }

    #[test]
    fn test_move_right_at_end() {
        let mut state = CommandLineState::new(':');
        state.insert_char('a');
        state.move_right(); // Should do nothing, already at end
        assert_eq!(state.cursor_pos(), 1);
    }

    #[test]
    fn test_move_right() {
        let mut state = CommandLineState::new(':');
        state.insert_char('a');
        state.insert_char('b');
        state.move_to_start();
        state.move_right();
        assert_eq!(state.cursor_pos(), 1);
    }

    #[test]
    fn test_move_to_start() {
        let mut state = CommandLineState::new(':');
        state.insert_char('a');
        state.insert_char('b');
        state.move_to_start();
        assert_eq!(state.cursor_pos(), 0);
    }

    #[test]
    fn test_move_to_end() {
        let mut state = CommandLineState::new(':');
        state.insert_char('a');
        state.insert_char('b');
        state.move_to_start();
        state.move_to_end();
        assert_eq!(state.cursor_pos(), 2);
    }

    #[test]
    fn test_take_buffer() {
        let mut state = CommandLineState::new(':');
        state.insert_char('w');
        state.insert_char('q');
        let taken = state.take_buffer();
        assert_eq!(taken, "wq");
        assert_eq!(state.buffer(), "");
        assert_eq!(state.cursor_pos(), 0);
        assert!(state.is_empty());
    }

    #[test]
    fn test_is_empty() {
        let mut state = CommandLineState::new(':');
        assert!(state.is_empty());
        state.insert_char('x');
        assert!(!state.is_empty());
    }

    // =========================================================================
    // parse_ex_command tests
    // =========================================================================

    fn ctx() -> ExCommandContext {
        ExCommandContext::default()
    }

    #[test]
    fn test_parse_empty_input() {
        assert_eq!(parse_ex_command("", &ctx()), Err(String::new()));
    }

    #[test]
    fn test_parse_whitespace_only() {
        assert_eq!(parse_ex_command("  ", &ctx()), Err(String::new()));
    }

    #[test]
    fn test_parse_write() {
        assert_eq!(parse_ex_command("w", &ctx()), Ok(ExCommand::Write));
        assert_eq!(parse_ex_command("write", &ctx()), Ok(ExCommand::Write));
    }

    #[test]
    fn test_parse_write_with_leading_whitespace() {
        assert_eq!(parse_ex_command("  w", &ctx()), Ok(ExCommand::Write));
    }

    #[test]
    fn test_parse_write_as() {
        assert_eq!(
            parse_ex_command("w foo.txt", &ctx()),
            Ok(ExCommand::WriteAs("foo.txt".to_string()))
        );
    }

    #[test]
    fn test_parse_write_as_long_form() {
        assert_eq!(
            parse_ex_command("write foo.txt", &ctx()),
            Ok(ExCommand::WriteAs("foo.txt".to_string()))
        );
    }

    #[test]
    fn test_parse_write_as_with_spaces_in_filename() {
        assert_eq!(
            parse_ex_command("w  multiple  spaces.txt", &ctx()),
            Ok(ExCommand::WriteAs("multiple  spaces.txt".to_string()))
        );
    }

    #[test]
    fn test_parse_write_with_trailing_space_only() {
        // "w " with only spaces after -> Write (no filename)
        assert_eq!(parse_ex_command("w ", &ctx()), Ok(ExCommand::Write));
    }

    #[test]
    fn test_parse_force_write() {
        assert_eq!(parse_ex_command("w!", &ctx()), Ok(ExCommand::ForceWrite));
        assert_eq!(
            parse_ex_command("write!", &ctx()),
            Ok(ExCommand::ForceWrite)
        );
    }

    #[test]
    fn test_parse_force_write_as() {
        assert_eq!(
            parse_ex_command("w! foo.txt", &ctx()),
            Ok(ExCommand::ForceWriteAs("foo.txt".to_string()))
        );
        assert_eq!(
            parse_ex_command("write! foo.txt", &ctx()),
            Ok(ExCommand::ForceWriteAs("foo.txt".to_string()))
        );
    }

    #[test]
    fn test_parse_quit() {
        assert_eq!(parse_ex_command("q", &ctx()), Ok(ExCommand::Quit));
        assert_eq!(parse_ex_command("quit", &ctx()), Ok(ExCommand::Quit));
    }

    #[test]
    fn test_parse_force_quit() {
        assert_eq!(parse_ex_command("q!", &ctx()), Ok(ExCommand::ForceQuit));
        assert_eq!(parse_ex_command("quit!", &ctx()), Ok(ExCommand::ForceQuit));
    }

    #[test]
    fn test_parse_write_quit() {
        assert_eq!(parse_ex_command("wq", &ctx()), Ok(ExCommand::WriteQuit));
    }

    #[test]
    fn test_parse_write_quit_force() {
        assert_eq!(
            parse_ex_command("wq!", &ctx()),
            Ok(ExCommand::ForceWriteQuit)
        );
    }

    #[test]
    fn test_parse_write_quit_as() {
        assert_eq!(
            parse_ex_command("wq foo.txt", &ctx()),
            Ok(ExCommand::WriteQuitAs("foo.txt".to_string()))
        );
    }

    #[test]
    fn test_parse_exit() {
        assert_eq!(
            parse_ex_command("x", &ctx()),
            Ok(ExCommand::WriteQuitIfModified)
        );
        assert_eq!(
            parse_ex_command("xit", &ctx()),
            Ok(ExCommand::WriteQuitIfModified)
        );
        assert_eq!(
            parse_ex_command("exit", &ctx()),
            Ok(ExCommand::WriteQuitIfModified)
        );
    }

    #[test]
    fn test_parse_edit() {
        assert_eq!(
            parse_ex_command("e foo.txt", &ctx()),
            Ok(ExCommand::Edit("foo.txt".to_string()))
        );
        assert_eq!(
            parse_ex_command("edit foo.txt", &ctx()),
            Ok(ExCommand::Edit("foo.txt".to_string()))
        );
    }

    #[test]
    fn test_parse_edit_no_filename() {
        assert_eq!(
            parse_ex_command("e ", &ctx()),
            Err("No file name".to_string())
        );
    }

    #[test]
    fn test_parse_edit_bare() {
        // :e without any argument is also "No file name"
        assert_eq!(
            parse_ex_command("e", &ctx()),
            Err("No file name".to_string())
        );
    }

    #[test]
    fn test_parse_force_edit() {
        assert_eq!(parse_ex_command("e!", &ctx()), Ok(ExCommand::ForceEdit));
        assert_eq!(parse_ex_command("edit!", &ctx()), Ok(ExCommand::ForceEdit));
    }

    #[test]
    fn test_parse_unknown_command() {
        assert_eq!(
            parse_ex_command("nonsense", &ctx()),
            Err("Not an editor command: nonsense".to_string())
        );
    }

    #[test]
    fn test_parse_shell_command() {
        // :!ls runs "ls" as a shell command
        assert_eq!(
            parse_ex_command("!ls", &ctx()),
            Ok(ExCommand::Shell("ls".to_string()))
        );
        // :!! runs "!" as a shell command (last command repeat — shell handles it)
        assert_eq!(
            parse_ex_command("!!", &ctx()),
            Ok(ExCommand::Shell("!".to_string()))
        );
        // :!make -j4 preserves the full command string
        assert_eq!(
            parse_ex_command("!make -j4", &ctx()),
            Ok(ExCommand::Shell("make -j4".to_string()))
        );
    }

    // =========================================================================
    // Substitute parsing tests
    // =========================================================================

    #[test]
    fn test_parse_substitute_basic() {
        let ctx = ExCommandContext::new(0, 10);
        let result = parse_ex_command("s/foo/bar/", &ctx).unwrap();
        match result {
            ExCommand::Substitute(cmd) => {
                assert_eq!(cmd.range, SubstituteRange::CurrentLine);
                assert_eq!(cmd.pattern, "foo");
                assert_eq!(cmd.replacement, "bar");
                assert!(!cmd.flags.global);
            }
            _ => panic!("Expected Substitute"),
        }
    }

    #[test]
    fn test_parse_substitute_with_flags() {
        let ctx = ExCommandContext::new(0, 10);
        let result = parse_ex_command("s/foo/bar/gi", &ctx).unwrap();
        match result {
            ExCommand::Substitute(cmd) => {
                assert!(cmd.flags.global);
                assert!(cmd.flags.case_insensitive);
            }
            _ => panic!("Expected Substitute"),
        }
    }

    #[test]
    fn test_parse_substitute_whole_file() {
        let ctx = ExCommandContext::new(5, 20);
        let result = parse_ex_command("%s/foo/bar/g", &ctx).unwrap();
        match result {
            ExCommand::Substitute(cmd) => {
                assert_eq!(cmd.range, SubstituteRange::WholeFile);
            }
            _ => panic!("Expected Substitute"),
        }
    }

    #[test]
    fn test_parse_substitute_single_line() {
        let ctx = ExCommandContext::new(0, 10);
        let result = parse_ex_command("5s/foo/bar/", &ctx).unwrap();
        match result {
            ExCommand::Substitute(cmd) => {
                assert_eq!(cmd.range, SubstituteRange::Line(4)); // 1-based -> 0-based
            }
            _ => panic!("Expected Substitute"),
        }
    }

    #[test]
    fn test_parse_substitute_range() {
        let ctx = ExCommandContext::new(0, 20);
        let result = parse_ex_command("5,10s/foo/bar/", &ctx).unwrap();
        match result {
            ExCommand::Substitute(cmd) => {
                assert_eq!(cmd.range, SubstituteRange::Range { start: 4, end: 9 });
            }
            _ => panic!("Expected Substitute"),
        }
    }

    #[test]
    fn test_parse_substitute_current_line_explicit() {
        let ctx = ExCommandContext::new(7, 20);
        let result = parse_ex_command(".s/foo/bar/", &ctx).unwrap();
        match result {
            ExCommand::Substitute(cmd) => {
                assert_eq!(cmd.range, SubstituteRange::Line(7));
            }
            _ => panic!("Expected Substitute"),
        }
    }

    #[test]
    fn test_parse_substitute_current_to_end() {
        let ctx = ExCommandContext::new(5, 10);
        let result = parse_ex_command(".,$s/foo/bar/", &ctx).unwrap();
        match result {
            ExCommand::Substitute(cmd) => {
                assert_eq!(cmd.range, SubstituteRange::Range { start: 5, end: 9 });
            }
            _ => panic!("Expected Substitute"),
        }
    }

    #[test]
    fn test_parse_substitute_empty_pattern() {
        let ctx = ExCommandContext::new(0, 10);
        let result = parse_ex_command("s//bar/", &ctx).unwrap();
        match result {
            ExCommand::Substitute(cmd) => {
                assert_eq!(cmd.pattern, "");
                assert_eq!(cmd.replacement, "bar");
            }
            _ => panic!("Expected Substitute"),
        }
    }

    #[test]
    fn test_parse_substitute_out_of_range() {
        let ctx = ExCommandContext::new(0, 5);
        let result = parse_ex_command("10s/foo/bar/", &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_substitute_inverted_range() {
        let ctx = ExCommandContext::new(0, 20);
        let result = parse_ex_command("10,5s/foo/bar/", &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_substitute_alternate_delimiter() {
        let ctx = ExCommandContext::new(0, 10);
        let result = parse_ex_command("s#foo#bar#", &ctx).unwrap();
        match result {
            ExCommand::Substitute(cmd) => {
                assert_eq!(cmd.pattern, "foo");
                assert_eq!(cmd.replacement, "bar");
            }
            _ => panic!("Expected Substitute"),
        }
    }

    // =========================================================================
    // ExCommandContext tests
    // =========================================================================

    #[test]
    fn test_ex_command_context_default() {
        let ctx = ExCommandContext::default();
        assert_eq!(ctx.current_line, 0);
        assert_eq!(ctx.buffer_len, 1);
    }

    #[test]
    fn test_ex_command_context_new() {
        let ctx = ExCommandContext::new(5, 100);
        assert_eq!(ctx.current_line, 5);
        assert_eq!(ctx.buffer_len, 100);
    }

    // =========================================================================
    // Set command integration tests
    // =========================================================================

    #[test]
    fn test_parse_set_number() {
        assert_eq!(
            parse_ex_command("set number", &ctx()),
            Ok(ExCommand::Set(SetCommand::Enable("number".to_string())))
        );
    }

    #[test]
    fn test_parse_set_abbreviation() {
        assert_eq!(
            parse_ex_command("se nu", &ctx()),
            Ok(ExCommand::Set(SetCommand::Enable("number".to_string())))
        );
    }

    #[test]
    fn test_parse_bare_set() {
        assert_eq!(
            parse_ex_command("set", &ctx()),
            Ok(ExCommand::Set(SetCommand::QueryAll))
        );
    }

    #[test]
    fn test_parse_bare_se() {
        assert_eq!(
            parse_ex_command("se", &ctx()),
            Ok(ExCommand::Set(SetCommand::QueryAll))
        );
    }

    #[test]
    fn test_parse_set_tabstop_assign() {
        assert_eq!(
            parse_ex_command("set tabstop=4", &ctx()),
            Ok(ExCommand::Set(SetCommand::Assign {
                option: "tabstop".to_string(),
                value: "4".to_string(),
            }))
        );
    }

    #[test]
    fn test_parse_set_nonumber() {
        assert_eq!(
            parse_ex_command("set nonumber", &ctx()),
            Ok(ExCommand::Set(SetCommand::Disable("number".to_string())))
        );
    }

    #[test]
    fn test_parse_set_toggle() {
        assert_eq!(
            parse_ex_command("set number!", &ctx()),
            Ok(ExCommand::Set(SetCommand::Toggle("number".to_string())))
        );
    }

    #[test]
    fn test_parse_set_query() {
        assert_eq!(
            parse_ex_command("set number?", &ctx()),
            Ok(ExCommand::Set(SetCommand::Query("number".to_string())))
        );
    }

    #[test]
    fn test_parse_set_all() {
        assert_eq!(
            parse_ex_command("set all", &ctx()),
            Ok(ExCommand::Set(SetCommand::QueryAll))
        );
    }

    #[test]
    fn test_parse_se_with_abbreviation_and_assign() {
        assert_eq!(
            parse_ex_command("se ts=4", &ctx()),
            Ok(ExCommand::Set(SetCommand::Assign {
                option: "tabstop".to_string(),
                value: "4".to_string(),
            }))
        );
    }

    #[test]
    fn test_parse_set_with_tab_separator() {
        // Should accept tab character after "set"
        assert_eq!(
            parse_ex_command("set\tnumber", &ctx()),
            Ok(ExCommand::Set(SetCommand::Enable("number".to_string())))
        );
    }

    #[test]
    fn test_parse_set_bare_with_no_space() {
        // Bare "set" should still work
        assert_eq!(
            parse_ex_command("set", &ctx()),
            Ok(ExCommand::Set(SetCommand::QueryAll))
        );
    }

    #[test]
    fn test_parse_setnumber_no_space() {
        // Vim compatibility: :setnumber (no space) should work
        assert_eq!(
            parse_ex_command("setnumber", &ctx()),
            Ok(ExCommand::Set(SetCommand::Enable("number".to_string())))
        );
    }

    #[test]
    fn test_parse_senu_no_space() {
        // :senu (abbreviation with no space) should also work
        assert_eq!(
            parse_ex_command("senu", &ctx()),
            Ok(ExCommand::Set(SetCommand::Enable("number".to_string())))
        );
    }

    // =========================================================================
    // Gap 15: :d, :y, :m, :t, :j, := parsing tests
    // =========================================================================

    #[test]
    fn test_parse_delete_lines_no_range() {
        assert_eq!(
            parse_ex_command("d", &ctx()),
            Ok(ExCommand::DeleteLines {
                range: SubstituteRange::CurrentLine,
                register: None,
            })
        );
    }

    #[test]
    fn test_parse_delete_lines_with_register() {
        assert_eq!(
            parse_ex_command("d a", &ctx()),
            Ok(ExCommand::DeleteLines {
                range: SubstituteRange::CurrentLine,
                register: Some('a'),
            })
        );
    }

    #[test]
    fn test_parse_delete_lines_with_range() {
        let c = ExCommandContext::new(4, 20);
        assert_eq!(
            parse_ex_command("1,3d", &c),
            Ok(ExCommand::DeleteLines {
                range: SubstituteRange::Range { start: 0, end: 2 },
                register: None,
            })
        );
    }

    #[test]
    fn test_parse_yank_lines() {
        assert_eq!(
            parse_ex_command("y", &ctx()),
            Ok(ExCommand::YankLines {
                range: SubstituteRange::CurrentLine,
                register: None,
            })
        );
    }

    #[test]
    fn test_parse_yank_lines_with_register() {
        assert_eq!(
            parse_ex_command("y b", &ctx()),
            Ok(ExCommand::YankLines {
                range: SubstituteRange::CurrentLine,
                register: Some('b'),
            })
        );
    }

    #[test]
    fn test_parse_move_lines() {
        let c = ExCommandContext::new(2, 10);
        assert_eq!(
            parse_ex_command("m 5", &c),
            Ok(ExCommand::MoveLines {
                range: SubstituteRange::CurrentLine,
                dest: 5,
            })
        );
    }

    #[test]
    fn test_parse_copy_lines() {
        let c = ExCommandContext::new(2, 10);
        assert_eq!(
            parse_ex_command("t 5", &c),
            Ok(ExCommand::CopyLines {
                range: SubstituteRange::CurrentLine,
                dest: 5,
            })
        );
    }

    #[test]
    fn test_parse_join_lines() {
        assert_eq!(
            parse_ex_command("j", &ctx()),
            Ok(ExCommand::JoinLines {
                range: SubstituteRange::CurrentLine,
            })
        );
    }

    #[test]
    fn test_parse_join_lines_with_range() {
        let c = ExCommandContext::new(0, 10);
        assert_eq!(
            parse_ex_command("1,3j", &c),
            Ok(ExCommand::JoinLines {
                range: SubstituteRange::Range { start: 0, end: 2 },
            })
        );
    }

    #[test]
    fn test_parse_print_line_number() {
        // Bare := with no range: resolves to current line (0-based 0 → shows line 1).
        assert_eq!(
            parse_ex_command("=", &ctx()),
            Ok(ExCommand::PrintLineNumber(0))
        );
        // :5= with a range: resolves to line 5 (0-based 4).
        let c = ExCommandContext::new(0, 10);
        assert_eq!(
            parse_ex_command("5=", &c),
            Ok(ExCommand::PrintLineNumber(4))
        );
    }

    #[test]
    fn test_parse_whole_file_delete() {
        assert_eq!(
            parse_ex_command("%d", &ctx()),
            Ok(ExCommand::DeleteLines {
                range: SubstituteRange::WholeFile,
                register: None,
            })
        );
    }

    // =========================================================================
    // Gap 16: :n, :N, :args, :rewind parsing tests
    // =========================================================================

    #[test]
    fn test_parse_next_file_no_arg() {
        assert_eq!(parse_ex_command("n", &ctx()), Ok(ExCommand::NextFile(None)));
        assert_eq!(
            parse_ex_command("next", &ctx()),
            Ok(ExCommand::NextFile(None))
        );
    }

    #[test]
    fn test_parse_next_file_with_arg() {
        assert_eq!(
            parse_ex_command("n foo.txt", &ctx()),
            Ok(ExCommand::NextFile(Some("foo.txt".to_string())))
        );
    }

    #[test]
    fn test_parse_prev_file() {
        assert_eq!(parse_ex_command("N", &ctx()), Ok(ExCommand::PrevFile));
        assert_eq!(parse_ex_command("prev", &ctx()), Ok(ExCommand::PrevFile));
        assert_eq!(
            parse_ex_command("previous", &ctx()),
            Ok(ExCommand::PrevFile)
        );
    }

    #[test]
    fn test_parse_show_args() {
        assert_eq!(parse_ex_command("args", &ctx()), Ok(ExCommand::ShowArgs));
        assert_eq!(parse_ex_command("ar", &ctx()), Ok(ExCommand::ShowArgs));
    }

    #[test]
    fn test_parse_rewind_args() {
        assert_eq!(
            parse_ex_command("rewind", &ctx()),
            Ok(ExCommand::RewindArgs)
        );
        assert_eq!(parse_ex_command("rew", &ctx()), Ok(ExCommand::RewindArgs));
    }

    // =========================================================================
    // Gap 11: Extended ex range address syntax tests
    // =========================================================================

    #[test]
    fn test_mark_address_delete() {
        let mut marks = std::collections::HashMap::new();
        marks.insert('a', 4usize); // mark 'a' is on line 4 (0-based)
        let c = ExCommandContext::with_marks(5, 10, marks);
        assert_eq!(
            parse_ex_command("'ad", &c),
            Ok(ExCommand::DeleteLines {
                range: SubstituteRange::Line(4),
                register: None,
            })
        );
    }

    #[test]
    fn test_mark_range_delete() {
        let mut marks = std::collections::HashMap::new();
        marks.insert('a', 2usize);
        marks.insert('b', 5usize);
        let c = ExCommandContext::with_marks(5, 10, marks);
        assert_eq!(
            parse_ex_command("'a,'bd", &c),
            Ok(ExCommand::DeleteLines {
                range: SubstituteRange::Range { start: 2, end: 5 },
                register: None,
            })
        );
    }

    #[test]
    fn test_mark_not_set_error() {
        let c = ExCommandContext::new(5, 10);
        // Mark 'z' not in context — should fail to resolve
        assert!(parse_ex_command("'zd", &c).is_err());
    }

    #[test]
    fn test_offset_address_plus() {
        // `.+2d` — delete current line + 2
        let c = ExCommandContext::new(3, 10); // current line = 3
        assert_eq!(
            parse_ex_command(".+2d", &c),
            Ok(ExCommand::DeleteLines {
                range: SubstituteRange::Line(5), // 3 + 2 = 5
                register: None,
            })
        );
    }

    #[test]
    fn test_offset_address_minus() {
        // `$-1d` — delete last line minus 1
        let c = ExCommandContext::new(0, 10); // buffer_len = 10, last = 9
        assert_eq!(
            parse_ex_command("$-1d", &c),
            Ok(ExCommand::DeleteLines {
                range: SubstituteRange::Line(8), // 9 - 1 = 8
                register: None,
            })
        );
    }

    #[test]
    fn test_offset_address_out_of_range() {
        // `.+100d` — offset beyond buffer end should error
        let c = ExCommandContext::new(0, 10);
        assert!(parse_ex_command(".+100d", &c).is_err());
    }

    #[test]
    fn test_offset_from_mark() {
        let mut marks = std::collections::HashMap::new();
        marks.insert('a', 3usize);
        let c = ExCommandContext::with_marks(5, 10, marks);
        assert_eq!(
            parse_ex_command("'a+1d", &c),
            Ok(ExCommand::DeleteLines {
                range: SubstituteRange::Line(4), // mark 'a'=3, +1 = 4
                register: None,
            })
        );
    }

    #[test]
    fn test_percent_range() {
        // `%y` — yank whole file
        assert_eq!(
            parse_ex_command("%y", &ctx()),
            Ok(ExCommand::YankLines {
                range: SubstituteRange::WholeFile,
                register: None,
            })
        );
    }

    #[test]
    fn test_bare_plus_offset() {
        // `.+d` means delete current line + 1 (bare + = +1)
        let c = ExCommandContext::new(3, 10); // current line = 3
        assert_eq!(
            parse_ex_command(".+d", &c),
            Ok(ExCommand::DeleteLines {
                range: SubstituteRange::Line(4), // 3 + 1 = 4
                register: None,
            })
        );
    }

    #[test]
    fn test_bare_minus_offset() {
        // `$-d` means delete last line - 1 (bare - = -1)
        let c = ExCommandContext::new(0, 10); // last line = 9
        assert_eq!(
            parse_ex_command("$-d", &c),
            Ok(ExCommand::DeleteLines {
                range: SubstituteRange::Line(8), // 9 - 1 = 8
                register: None,
            })
        );
    }

    #[test]
    fn test_offset_below_zero_errors() {
        // `.` on line 0, `-5` goes negative — should error
        let c = ExCommandContext::new(0, 10);
        assert!(parse_ex_command(".-5d", &c).is_err());
    }

    #[test]
    fn test_offset_from_number() {
        // `3+2d` — offset from a literal line number
        let c = ExCommandContext::new(0, 10);
        assert_eq!(
            parse_ex_command("3+2d", &c),
            Ok(ExCommand::DeleteLines {
                range: SubstituteRange::Line(4), // line 3 (1-based=2 0-based) + 2 = 4
                register: None,
            })
        );
    }

    #[test]
    fn test_mark_as_move_destination() {
        // `1,3m'a` — move lines 1-3 to after mark 'a'
        let mut marks = std::collections::HashMap::new();
        marks.insert('a', 7usize); // mark 'a' is on 0-based line 7
        let c = ExCommandContext::with_marks(5, 20, marks);
        assert_eq!(
            parse_ex_command("1,3m'a", &c),
            Ok(ExCommand::MoveLines {
                range: SubstituteRange::Range { start: 0, end: 2 },
                dest: 8, // insert after line 7 (0-based) = index 8
            })
        );
    }

    #[test]
    fn test_mark_range_in_substitute() {
        use crate::search::substitute::{SubstituteCommand, SubstituteFlags};
        let mut marks = std::collections::HashMap::new();
        marks.insert('a', 1usize);
        marks.insert('b', 4usize);
        let c = ExCommandContext::with_marks(5, 10, marks);
        assert_eq!(
            parse_ex_command("'a,'bs/foo/bar/", &c),
            Ok(ExCommand::Substitute(SubstituteCommand {
                range: SubstituteRange::Range { start: 1, end: 4 },
                pattern: "foo".to_string(),
                replacement: "bar".to_string(),
                flags: SubstituteFlags::default(),
            }))
        );
    }

    // =========================================================================
    // :r !cmd (read shell command)
    // =========================================================================

    #[test]
    fn test_parse_read_shell_command() {
        let result = parse_ex_command("r !ls -la", &ctx()).unwrap();
        assert_eq!(
            result,
            ExCommand::ReadShellCommand {
                cmd: "ls -la".to_string()
            }
        );
    }

    #[test]
    fn test_parse_read_shell_command_trim() {
        let result = parse_ex_command("r !  echo hello  ", &ctx()).unwrap();
        assert_eq!(
            result,
            ExCommand::ReadShellCommand {
                cmd: "  echo hello".to_string()
            }
        );
    }

    // =========================================================================
    // :{range}p (print lines)
    // =========================================================================

    #[test]
    fn test_parse_print_lines_bare() {
        let result = parse_ex_command("p", &ctx()).unwrap();
        assert_eq!(
            result,
            ExCommand::PrintLines {
                range: SubstituteRange::CurrentLine
            }
        );
    }

    #[test]
    fn test_parse_print_lines_with_range() {
        let big_ctx = ExCommandContext {
            current_line: 0,
            buffer_len: 10,
            marks: std::collections::HashMap::new(),
            ..ExCommandContext::default()
        };
        let result = parse_ex_command("1,5p", &big_ctx).unwrap();
        assert_eq!(
            result,
            ExCommand::PrintLines {
                range: SubstituteRange::Range { start: 0, end: 4 }
            }
        );
    }

    // =========================================================================
    // :nu / :number / :#
    // =========================================================================

    #[test]
    fn test_parse_print_numbered_lines_nu() {
        let result = parse_ex_command("nu", &ctx()).unwrap();
        assert_eq!(
            result,
            ExCommand::PrintNumberedLines {
                range: SubstituteRange::CurrentLine
            }
        );
    }

    #[test]
    fn test_parse_print_numbered_lines_number() {
        let result = parse_ex_command("number", &ctx()).unwrap();
        assert_eq!(
            result,
            ExCommand::PrintNumberedLines {
                range: SubstituteRange::CurrentLine
            }
        );
    }

    #[test]
    fn test_parse_print_numbered_lines_hash() {
        let result = parse_ex_command("#", &ctx()).unwrap();
        assert_eq!(
            result,
            ExCommand::PrintNumberedLines {
                range: SubstituteRange::CurrentLine
            }
        );
    }

    #[test]
    fn test_parse_print_numbered_lines_with_range() {
        let big_ctx = ExCommandContext {
            current_line: 0,
            buffer_len: 10,
            marks: std::collections::HashMap::new(),
            ..ExCommandContext::default()
        };
        let result = parse_ex_command("1,3nu", &big_ctx).unwrap();
        assert_eq!(
            result,
            ExCommand::PrintNumberedLines {
                range: SubstituteRange::Range { start: 0, end: 2 }
            }
        );
    }

    // =========================================================================
    // :[range]l
    // =========================================================================

    #[test]
    fn test_parse_print_list_lines_bare() {
        let result = parse_ex_command("l", &ctx()).unwrap();
        assert_eq!(
            result,
            ExCommand::PrintListLines {
                range: SubstituteRange::CurrentLine
            }
        );
    }

    #[test]
    fn test_parse_print_list_lines_with_range() {
        let big_ctx = ExCommandContext {
            current_line: 0,
            buffer_len: 10,
            marks: std::collections::HashMap::new(),
            ..ExCommandContext::default()
        };
        let result = parse_ex_command("1,3l", &big_ctx).unwrap();
        assert_eq!(
            result,
            ExCommand::PrintListLines {
                range: SubstituteRange::Range { start: 0, end: 2 }
            }
        );
    }

    // =========================================================================
    // :map / :unmap
    // =========================================================================

    #[test]
    fn test_parse_map_command() {
        let result = parse_ex_command("map qq :wq<CR>", &ctx()).unwrap();
        match result {
            ExCommand::Map {
                insert_mode,
                lhs,
                rhs,
            } => {
                assert!(!insert_mode);
                assert_eq!(lhs, "qq");
                assert_eq!(rhs, ":wq<CR>");
            }
            other => panic!("Expected Map, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_map_insert_mode() {
        let result = parse_ex_command("map! jj <Esc>", &ctx()).unwrap();
        match result {
            ExCommand::Map {
                insert_mode,
                lhs,
                rhs,
            } => {
                assert!(insert_mode);
                assert_eq!(lhs, "jj");
                assert_eq!(rhs, "<Esc>");
            }
            other => panic!("Expected Map, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_unmap_command() {
        let result = parse_ex_command("unmap qq", &ctx()).unwrap();
        assert_eq!(
            result,
            ExCommand::Unmap {
                insert_mode: false,
                lhs: "qq".to_string()
            }
        );
    }

    #[test]
    fn test_parse_show_maps() {
        let result = parse_ex_command("map", &ctx()).unwrap();
        assert_eq!(result, ExCommand::ShowMaps { insert_mode: false });
    }

    #[test]
    fn test_parse_show_maps_insert() {
        let result = parse_ex_command("map!", &ctx()).unwrap();
        assert_eq!(result, ExCommand::ShowMaps { insert_mode: true });
    }

    #[test]
    fn test_parse_version() {
        assert_eq!(parse_ex_command("version", &ctx()), Ok(ExCommand::Version));
        assert_eq!(parse_ex_command("ve", &ctx()), Ok(ExCommand::Version));
    }

    #[test]
    fn test_parse_sh() {
        assert_eq!(
            parse_ex_command("sh", &ctx()),
            Ok(ExCommand::ShellInteractive)
        );
    }

    // =========================================================================
    // :cd / :chdir
    // =========================================================================

    #[test]
    fn test_parse_cd_with_dir() {
        assert_eq!(
            parse_ex_command("cd /tmp", &ctx()),
            Ok(ExCommand::ChangeDir("/tmp".to_string()))
        );
    }

    #[test]
    fn test_parse_chdir_with_dir() {
        assert_eq!(
            parse_ex_command("chdir /tmp", &ctx()),
            Ok(ExCommand::ChangeDir("/tmp".to_string()))
        );
    }

    #[test]
    fn test_parse_cd_no_arg_is_error() {
        assert!(parse_ex_command("cd", &ctx()).is_err());
    }

    #[test]
    fn test_parse_chdir_no_arg_is_error() {
        assert!(parse_ex_command("chdir", &ctx()).is_err());
    }

    // =========================================================================
    // :source / :so
    // =========================================================================

    #[test]
    fn test_parse_source_with_file() {
        assert_eq!(
            parse_ex_command("source /etc/foo", &ctx()),
            Ok(ExCommand::Source("/etc/foo".to_string()))
        );
    }

    #[test]
    fn test_parse_so_with_file() {
        assert_eq!(
            parse_ex_command("so /etc/foo", &ctx()),
            Ok(ExCommand::Source("/etc/foo".to_string()))
        );
    }

    #[test]
    fn test_parse_source_no_arg_is_error() {
        assert!(parse_ex_command("source", &ctx()).is_err());
    }

    #[test]
    fn test_parse_so_no_arg_is_error() {
        assert!(parse_ex_command("so", &ctx()).is_err());
    }

    // =========================================================================
    // :mark / :ma / :k
    // =========================================================================

    #[test]
    fn test_parse_mark_long() {
        assert_eq!(
            parse_ex_command("mark a", &ctx()),
            Ok(ExCommand::SetMark('a'))
        );
        assert_eq!(
            parse_ex_command("mark z", &ctx()),
            Ok(ExCommand::SetMark('z'))
        );
    }

    #[test]
    fn test_parse_mark_abbrev() {
        assert_eq!(
            parse_ex_command("ma a", &ctx()),
            Ok(ExCommand::SetMark('a'))
        );
    }

    #[test]
    fn test_parse_mark_k_form() {
        assert_eq!(parse_ex_command("ka", &ctx()), Ok(ExCommand::SetMark('a')));
        assert_eq!(parse_ex_command("kz", &ctx()), Ok(ExCommand::SetMark('z')));
    }

    #[test]
    fn test_parse_mark_no_arg_is_error() {
        assert!(parse_ex_command("mark", &ctx()).is_err());
        assert!(parse_ex_command("ma", &ctx()).is_err());
    }

    #[test]
    fn test_parse_mark_invalid_char_is_error() {
        assert!(parse_ex_command("mark A", &ctx()).is_err());
        assert!(parse_ex_command("mark 1", &ctx()).is_err());
    }

    #[test]
    fn test_parse_mark_trailing_chars_is_error() {
        assert!(parse_ex_command("mark ab", &ctx()).is_err());
        assert!(parse_ex_command("mark a xyz", &ctx()).is_err());
    }

    #[test]
    fn test_parse_mark_k_no_letter_is_error() {
        assert!(parse_ex_command("k", &ctx()).is_err());
    }

    #[test]
    fn test_parse_mark_k_invalid_char_is_error() {
        assert!(parse_ex_command("kA", &ctx()).is_err());
    }

    // =========================================================================
    // :suspend / :stop
    // =========================================================================

    #[test]
    fn test_parse_suspend_long() {
        assert_eq!(parse_ex_command("suspend", &ctx()), Ok(ExCommand::Suspend));
    }

    #[test]
    fn test_parse_suspend_abbrev_su() {
        assert_eq!(parse_ex_command("su", &ctx()), Ok(ExCommand::Suspend));
    }

    #[test]
    fn test_parse_stop_long() {
        assert_eq!(parse_ex_command("stop", &ctx()), Ok(ExCommand::Suspend));
    }

    #[test]
    fn test_parse_stop_abbrev_st() {
        assert_eq!(parse_ex_command("st", &ctx()), Ok(ExCommand::Suspend));
    }

    // =========================================================================
    // :append / :insert / :change
    // =========================================================================

    #[test]
    fn test_parse_append_bare() {
        // No address → insert after current line (row 0 in default ctx) → insert_at = 1
        assert_eq!(
            parse_ex_command("a", &ctx()),
            Ok(ExCommand::AppendLines { insert_at: 1 })
        );
    }

    #[test]
    fn test_parse_append_long() {
        assert_eq!(
            parse_ex_command("append", &ctx()),
            Ok(ExCommand::AppendLines { insert_at: 1 })
        );
    }

    #[test]
    fn test_parse_append_with_address() {
        // :3a → after 1-based line 3 (0-based row 2) → insert_at = 3
        let c = ExCommandContext {
            current_line: 0,
            buffer_len: 10,
            marks: std::collections::HashMap::new(),
            ..ExCommandContext::default()
        };
        assert_eq!(
            parse_ex_command("3a", &c),
            Ok(ExCommand::AppendLines { insert_at: 3 })
        );
    }

    #[test]
    fn test_parse_append_zero_address() {
        // :0a → insert at top of buffer
        let c = ExCommandContext {
            current_line: 0,
            buffer_len: 10,
            marks: std::collections::HashMap::new(),
            ..ExCommandContext::default()
        };
        assert_eq!(
            parse_ex_command("0a", &c),
            Ok(ExCommand::AppendLines { insert_at: 0 })
        );
    }

    #[test]
    fn test_parse_insert_bare() {
        // No address → insert before current line (row 0) → insert_at = 0
        assert_eq!(
            parse_ex_command("i", &ctx()),
            Ok(ExCommand::InsertLines { insert_at: 0 })
        );
    }

    #[test]
    fn test_parse_insert_long() {
        assert_eq!(
            parse_ex_command("insert", &ctx()),
            Ok(ExCommand::InsertLines { insert_at: 0 })
        );
    }

    #[test]
    fn test_parse_insert_with_address() {
        // :5i → before 1-based line 5 (0-based row 4) → insert_at = 4
        let c = ExCommandContext {
            current_line: 0,
            buffer_len: 10,
            marks: std::collections::HashMap::new(),
            ..ExCommandContext::default()
        };
        assert_eq!(
            parse_ex_command("5i", &c),
            Ok(ExCommand::InsertLines { insert_at: 4 })
        );
    }

    #[test]
    fn test_parse_change_bare() {
        assert_eq!(
            parse_ex_command("c", &ctx()),
            Ok(ExCommand::ChangeLines { start: 0, end: 0 })
        );
    }

    #[test]
    fn test_parse_change_long() {
        assert_eq!(
            parse_ex_command("change", &ctx()),
            Ok(ExCommand::ChangeLines { start: 0, end: 0 })
        );
    }

    #[test]
    fn test_parse_change_with_range() {
        let c = ExCommandContext {
            current_line: 0,
            buffer_len: 10,
            marks: std::collections::HashMap::new(),
            ..ExCommandContext::default()
        };
        assert_eq!(
            parse_ex_command("2,4c", &c),
            Ok(ExCommand::ChangeLines { start: 1, end: 3 })
        );
    }

    // =========================================================================
    // Bare address as goto-line command ($, ., N)
    // =========================================================================

    #[test]
    fn test_bare_dollar_goes_to_last_line() {
        let c = ExCommandContext {
            current_line: 2,
            buffer_len: 10,
            marks: std::collections::HashMap::new(),
            ..ExCommandContext::default()
        };
        assert_eq!(parse_ex_command("$", &c), Ok(ExCommand::GotoLine(10)));
    }

    #[test]
    fn test_bare_dot_goes_to_current_line() {
        let c = ExCommandContext {
            current_line: 4,
            buffer_len: 10,
            marks: std::collections::HashMap::new(),
            ..ExCommandContext::default()
        };
        // current_line is 0-based (row 4) → 1-based = 5
        assert_eq!(parse_ex_command(".", &c), Ok(ExCommand::GotoLine(5)));
    }

    #[test]
    fn test_bare_number_goes_to_line() {
        let c = ExCommandContext {
            current_line: 0,
            buffer_len: 10,
            marks: std::collections::HashMap::new(),
            ..ExCommandContext::default()
        };
        assert_eq!(parse_ex_command("7", &c), Ok(ExCommand::GotoLine(7)));
    }

    // =========================================================================
    // :abbreviate / :unabbreviate tests
    // =========================================================================

    #[test]
    fn test_parse_abbreviate_lhs_rhs() {
        assert_eq!(
            parse_ex_command("ab foo foobar", &ctx()),
            Ok(ExCommand::Abbreviate {
                lhs: "foo".to_string(),
                rhs: "foobar".to_string(),
            })
        );
    }

    #[test]
    fn test_parse_abbreviate_long_form() {
        assert_eq!(
            parse_ex_command("abbreviate hw hello world", &ctx()),
            Ok(ExCommand::Abbreviate {
                lhs: "hw".to_string(),
                rhs: "hello world".to_string(),
            })
        );
    }

    #[test]
    fn test_parse_abbreviate_bare_shows_all() {
        assert_eq!(
            parse_ex_command("ab", &ctx()),
            Ok(ExCommand::ShowAbbreviations)
        );
        assert_eq!(
            parse_ex_command("abbreviate", &ctx()),
            Ok(ExCommand::ShowAbbreviations)
        );
    }

    #[test]
    fn test_parse_unabbreviate() {
        assert_eq!(
            parse_ex_command("una foo", &ctx()),
            Ok(ExCommand::Unabbreviate {
                lhs: "foo".to_string(),
            })
        );
        assert_eq!(
            parse_ex_command("unabbreviate foo", &ctx()),
            Ok(ExCommand::Unabbreviate {
                lhs: "foo".to_string(),
            })
        );
    }

    // =========================================================================
    // :vi / :visual — exit ex mode
    // =========================================================================

    #[test]
    fn test_parse_vi_command() {
        assert_eq!(parse_ex_command("vi", &ctx()), Ok(ExCommand::Visual));
    }

    #[test]
    fn test_parse_visual_command() {
        assert_eq!(parse_ex_command("visual", &ctx()), Ok(ExCommand::Visual));
    }

    #[test]
    fn test_parse_vi_bang_command() {
        assert_eq!(parse_ex_command("vi!", &ctx()), Ok(ExCommand::Visual));
        assert_eq!(parse_ex_command("visual!", &ctx()), Ok(ExCommand::Visual));
    }

    // =========================================================================
    // :preserve / :recover — crash recovery
    // =========================================================================

    #[test]
    fn test_parse_preserve() {
        for cmd in &["pre", "pres", "prese", "preserv", "preserve"] {
            assert_eq!(
                parse_ex_command(cmd, &ctx()),
                Ok(ExCommand::Preserve),
                "failed for :{}",
                cmd
            );
        }
    }

    #[test]
    fn test_parse_recover_no_arg() {
        for cmd in &["rec", "reco", "recov", "recove", "recover"] {
            assert_eq!(
                parse_ex_command(cmd, &ctx()),
                Ok(ExCommand::Recover(None)),
                "failed for :{}",
                cmd
            );
        }
    }

    #[test]
    fn test_parse_recover_with_filename() {
        assert_eq!(
            parse_ex_command("rec foo.txt", &ctx()),
            Ok(ExCommand::Recover(Some("foo.txt".to_string())))
        );
        assert_eq!(
            parse_ex_command("recover foo.txt", &ctx()),
            Ok(ExCommand::Recover(Some("foo.txt".to_string())))
        );
    }

    #[test]
    fn test_parse_recover_with_whitespace_only() {
        assert_eq!(
            parse_ex_command("rec  ", &ctx()),
            Ok(ExCommand::Recover(None))
        );
    }

    // =========================================================================
    // :ta[g] — tag jump
    // =========================================================================

    #[test]
    fn test_parse_tag_with_name() {
        assert_eq!(
            parse_ex_command("tag foo", &ctx()),
            Ok(ExCommand::Tag("foo".to_string()))
        );
    }

    #[test]
    fn test_parse_tag_abbreviated() {
        assert_eq!(
            parse_ex_command("ta foo", &ctx()),
            Ok(ExCommand::Tag("foo".to_string()))
        );
    }

    #[test]
    fn test_parse_tag_no_arg_error() {
        assert!(parse_ex_command("tag", &ctx()).is_err());
        assert!(parse_ex_command("ta", &ctx()).is_err());
    }

    // =========================================================================
    // /pattern/ and ?pattern? as ex addresses
    // =========================================================================

    fn ctx_with_lines(lines: &[&str], current: usize) -> ExCommandContext {
        let buffer_lines: Arc<[String]> = lines.iter().map(|s| s.to_string()).collect();
        ExCommandContext {
            current_line: current,
            buffer_len: lines.len(),
            marks: std::collections::HashMap::new(),
            buffer_lines,
            last_pattern: None,
        }
    }

    #[test]
    fn test_search_addr_forward_delete() {
        // /bar/d should delete the first "bar" line after line 0
        let c = ctx_with_lines(&["foo", "bar", "baz", "bar"], 0);
        let result = parse_ex_command("/bar/d", &c).unwrap();
        assert_eq!(
            result,
            ExCommand::DeleteLines {
                range: SubstituteRange::Line(1),
                register: None,
            }
        );
    }

    #[test]
    fn test_search_addr_backward_delete() {
        // ?bar?d from line 3 should find line 1 (going backward)
        let c = ctx_with_lines(&["foo", "bar", "baz", "qux"], 3);
        let result = parse_ex_command("?bar?d", &c).unwrap();
        assert_eq!(
            result,
            ExCommand::DeleteLines {
                range: SubstituteRange::Line(1),
                register: None,
            }
        );
    }

    #[test]
    fn test_search_addr_in_range() {
        // 1,/baz/d should delete lines 1-3 (0-based 0..2)
        let c = ctx_with_lines(&["foo", "bar", "baz", "qux"], 0);
        let result = parse_ex_command("1,/baz/d", &c).unwrap();
        assert_eq!(
            result,
            ExCommand::DeleteLines {
                range: SubstituteRange::Range { start: 0, end: 2 },
                register: None,
            }
        );
    }

    #[test]
    fn test_empty_search_addr_reuses_last_pattern() {
        let mut c = ctx_with_lines(&["foo", "bar", "baz"], 0);
        c.last_pattern = Some("bar".to_string());
        let result = parse_ex_command("//d", &c).unwrap();
        assert_eq!(
            result,
            ExCommand::DeleteLines {
                range: SubstituteRange::Line(1),
                register: None,
            }
        );
    }

    #[test]
    fn test_search_addr_wraps_forward() {
        // /foo/ from last line should wrap to line 0
        let c = ctx_with_lines(&["foo", "bar", "baz"], 2);
        let result = parse_ex_command("/foo/d", &c).unwrap();
        assert_eq!(
            result,
            ExCommand::DeleteLines {
                range: SubstituteRange::Line(0),
                register: None,
            }
        );
    }

    #[test]
    fn test_search_addr_not_found_error() {
        let c = ctx_with_lines(&["foo", "bar", "baz"], 0);
        let result = parse_ex_command("/nomatch/d", &c);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("E486"));
    }

    // =========================================================================
    // ; (semicolon) address separator
    // =========================================================================

    #[test]
    fn test_semicolon_simple_range() {
        // 1;3d — same as 1,3d when both addresses are absolute
        let c = ctx_with_lines(&["a", "b", "c", "d", "e"], 4);
        let result = parse_ex_command("1;3d", &c).unwrap();
        assert_eq!(
            result,
            ExCommand::DeleteLines {
                range: SubstituteRange::Range { start: 0, end: 2 },
                register: None,
            }
        );
    }

    #[test]
    fn test_semicolon_search_relative_to_first() {
        // 3;/d/p — from line 3 (index 2), find next /d/ (index 3)
        // With comma: cursor is at 4 (index 4), so /d/ would wrap to index 3
        // With semicolon: current_line for second addr = index 2, so /d/ finds index 3
        let c = ctx_with_lines(&["a", "b", "c", "d", "e"], 4);
        let result = parse_ex_command("3;/d/p", &c).unwrap();
        assert_eq!(
            result,
            ExCommand::PrintLines {
                range: SubstituteRange::Range { start: 2, end: 3 },
            }
        );
    }

    #[test]
    fn test_semicolon_vs_comma_difference() {
        // Buffer: ["target", "b", "c", "d", "target"], cursor at last line (4)
        // 1,/target/d — comma: /target/ searches from cursor (line 4), wraps to line 0
        // 1;/target/d — semicolon: /target/ searches from line 1 (index 0), finds line 4
        let c = ctx_with_lines(&["target", "b", "c", "d", "target"], 4);

        let comma_result = parse_ex_command("1,/target/d", &c).unwrap();
        // comma: /target/ from cursor (4) wraps → finds line 0 (index 0)
        assert_eq!(
            comma_result,
            ExCommand::DeleteLines {
                range: SubstituteRange::Range { start: 0, end: 0 },
                register: None,
            }
        );

        let semi_result = parse_ex_command("1;/target/d", &c).unwrap();
        // semicolon: /target/ from line 1 (index 0) → finds line 4 (index 4, the *next* match)
        assert_eq!(
            semi_result,
            ExCommand::DeleteLines {
                range: SubstituteRange::Range { start: 0, end: 4 },
                register: None,
            }
        );
    }
}

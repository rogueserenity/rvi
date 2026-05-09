//! Editor core module.
//!
//! This module contains the main Editor struct that coordinates all editor
//! components and handles the main event loop.
//!
//! The module is split into:
//! - `mod.rs`: Core Editor struct, Document, EditorState, and event loop
//! - `movement.rs`: Cursor movement methods
//! - `editing.rs`: Text editing methods (insert, delete, etc.)
//! - `key_handlers.rs`: Key event handling for each mode

mod editing;
mod key_handlers;
mod movement;

/// Maximum number of keys to record during insert mode for dot repeat.
///
/// This prevents unbounded memory growth when pasting large amounts of text.
/// If exceeded, the insert session becomes non-repeatable.
const MAX_INSERT_KEYS: usize = 10_000;

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;

use crate::buffer::{Buffer, Cursor, Selection};
use crate::command::{CommandLineState, ExCommand, ParseState, RepeatableChange};
use crate::error::{Error, FileError};
use crate::file::{LineEnding, WriteOptions};
use crate::mode::Mode;
use crate::registers::{RegisterContent, RegisterId, Registers};
use crate::render::{Renderer, Viewport};
use crate::search::regex_utils::ViRegex;
use crate::search::substitute::{substitute_line, SubstituteCommand, SubstituteRange};
use crate::search::SearchState;
use crate::settings::{SetCommand, SetResult, Settings};
use crate::terminal::{
    install_sigwinch_handler, read_key, sigwinch_received, terminal_size, Key, RawMode,
    TerminalOutput,
};
use crate::undo::{EditAction, UndoHistory};

/// A message displayed on the status line, with error/info distinction for `errorbells`.
#[derive(Debug, Clone)]
pub enum StatusMessage {
    /// An informational message (success, query output, etc.). Never rings the bell.
    Info(String),
    /// An error message. Rings the bell when `errorbells` is on.
    Error(String),
    /// A message referencing a file path, rendered as `"<path>" <suffix>` with
    /// the path smart-truncated to fit the available terminal width. Always info-level.
    WithPath { path: String, suffix: String },
}

impl StatusMessage {
    /// Returns `true` if this is an error message.
    pub fn is_error(&self) -> bool {
        matches!(self, StatusMessage::Error(_))
    }

    /// Returns the message text regardless of variant.
    ///
    /// For `WithPath`, formats as `"<path>"<suffix>` (used by unit tests and
    /// any callers that need a plain string without smart truncation).
    pub fn as_str_formatted(&self) -> String {
        match self {
            StatusMessage::Info(s) | StatusMessage::Error(s) => s.clone(),
            StatusMessage::WithPath { path, suffix } => format!("\"{path}\"{suffix}"),
        }
    }
}

/// Resolve the XDG config file path for rvi.
///
/// Returns `$XDG_CONFIG_HOME/rvi/config` if `XDG_CONFIG_HOME` is set,
/// otherwise falls back to `$HOME/.config/rvi/config`.
/// Returns `None` if neither env var is available.
fn xdg_config_path() -> Option<PathBuf> {
    let config_dir = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|home| format!("{}/.config", home))
        })?;
    Some(PathBuf::from(config_dir).join("rvi").join("config"))
}

/// Compute the preserve file path for a given filename and process ID.
///
/// All preserve files are named `{dir}/rvi.{basename}.{pid}` — the `rvi.`
/// prefix scopes them to rvi so `list_preserve_files` can distinguish them
/// from unrelated `/tmp` files created by other applications.
fn preserve_path(dir: &str, filename: Option<&str>, pid: u32) -> PathBuf {
    let basename = filename
        .and_then(|f| std::path::Path::new(f).file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("unnamed");
    PathBuf::from(format!("{}/rvi.{}.{}", dir, basename, pid))
}

/// Parse a key notation string into a sequence of Key values.
///
/// Supports `<CR>` for Enter, `<Esc>` for Escape, `<BS>` for Backspace,
/// and literal characters as Key::Char(c).
fn parse_key_notation(s: &str) -> Vec<Key> {
    let mut keys = Vec::new();
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '<' {
            // Collect until '>'
            let mut tag = String::new();
            for c in chars.by_ref() {
                if c == '>' {
                    break;
                }
                tag.push(c);
            }
            match tag.to_uppercase().as_str() {
                "CR" | "ENTER" | "RETURN" => keys.push(Key::Enter),
                "ESC" | "ESCAPE" => keys.push(Key::Esc),
                "BS" | "BACKSPACE" => keys.push(Key::Backspace),
                "TAB" => keys.push(Key::Tab),
                "SPACE" => keys.push(Key::Char(' ')),
                _ => {
                    // Unknown tag, push as literal chars
                    keys.push(Key::Char('<'));
                    for c in tag.chars() {
                        keys.push(Key::Char(c));
                    }
                    keys.push(Key::Char('>'));
                }
            }
        } else {
            keys.push(Key::Char(ch));
        }
    }
    keys
}

/// Convert a sequence of Key values back to display notation.
fn keys_to_notation(keys: &[Key]) -> String {
    let mut s = String::new();
    for key in keys {
        match key {
            Key::Char(c) => s.push(*c),
            Key::Enter => s.push_str("<CR>"),
            Key::Esc => s.push_str("<Esc>"),
            Key::Backspace => s.push_str("<BS>"),
            Key::Tab => s.push_str("<Tab>"),
            Key::Ctrl(c) => {
                s.push_str("<C-");
                s.push(*c);
                s.push('>');
            }
            _ => s.push_str("<?>"),
        }
    }
    s
}

/// Document state - the content being edited.
pub struct Document {
    /// The text buffer
    pub buffer: Buffer,
    /// Current cursor position
    pub cursor: Cursor,
    /// Current selection (if any)
    pub selection: Option<Selection>,
    /// Filename (if opened from file)
    pub filename: Option<String>,
    /// File path (as provided by user, may be relative or absolute)
    pub file_path: Option<PathBuf>,
    /// Whether the buffer has been modified since last save/load
    pub modified: bool,
    /// Detected line ending from the file (preserved on save)
    pub line_ending: LineEnding,
}

impl Document {
    /// Create a new empty document.
    pub fn new() -> Self {
        Self {
            buffer: Buffer::new(),
            cursor: Cursor::new(0, 0),
            selection: None,
            filename: None,
            file_path: None,
            modified: false,
            line_ending: LineEnding::default(),
        }
    }

    /// Create a document from a string.
    pub fn from_string(content: String) -> Self {
        Self {
            buffer: Buffer::from_string(content),
            cursor: Cursor::new(0, 0),
            selection: None,
            filename: None,
            file_path: None,
            modified: false,
            line_ending: LineEnding::default(),
        }
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

/// Snapshot of editor position saved when entering incremental search.
///
/// Allows Escape to return the cursor and viewport to exactly where the
/// user was before pressing `/` or `?`.
pub struct IncSearchSaved {
    /// Search state before the `/` or `?` was pressed.
    pub search: SearchState,
    /// Cursor position before the `/` or `?` was pressed.
    pub cursor: Cursor,
    /// Viewport offset before the `/` or `?` was pressed.
    pub viewport: Viewport,
}

/// Macro recording and playback state.
#[derive(Debug, Default)]
pub struct MacroState {
    /// If recording a macro, the register name being recorded into.
    pub recording: Option<char>,
    /// Keystroke buffer accumulating during the current macro recording.
    pub buffer: Vec<Key>,
    /// Current macro playback nesting depth (for recursion guard).
    pub playback_depth: usize,
    /// The register name of the most recently played macro, for `@@` replay.
    pub last_macro: Option<char>,
}

/// Insert mode transient state (dot recording, literal-next, replace originals).
#[derive(Debug, Default)]
pub struct InsertState {
    /// Keys typed during the current insert mode session (for dot recording).
    pub insert_keys: Vec<Key>,
    /// True while replaying a dot command.
    pub replaying_dot: bool,
    /// How the current insert mode session was entered, if applicable.
    pub insert_entry_kind: Option<crate::command::ModeChangeKind>,
    /// The count from when insert mode was entered (e.g. 3 for 3ifoo<Esc>).
    pub insert_count: usize,
    /// When `true`, the next key in insert mode is inserted literally.
    pub insert_literal_next: bool,
    /// When `true`, the next key in insert mode is treated as a register name.
    pub insert_register_next: bool,
    /// Indent string to use for the next `Enter` keypress in insert mode.
    pub saved_indent: Option<String>,
    /// Original graphemes overwritten during a Replace mode session.
    pub replace_originals: Vec<Option<String>>,
    /// When set by visual block `I`, replay insert_keys on rows (start+1)..=end
    /// at the given column when Esc exits insert mode.
    pub block_insert: Option<(usize, usize, usize)>,
}

impl InsertState {
    /// Record `key` into `insert_keys` for dot repeat, unless we are currently
    /// replaying a dot command or the buffer is already at capacity.
    pub fn maybe_record_key(&mut self, key: Key) {
        if !self.replaying_dot && self.insert_keys.len() < MAX_INSERT_KEYS {
            self.insert_keys.push(key);
        }
    }
}

/// Navigation state: marks, jump list, line snapshot.
#[derive(Debug, Default)]
pub struct NavigationState {
    /// File-local marks set by `m{a-z}`.
    pub marks: HashMap<char, Cursor>,
    /// Jump list -- ring buffer of cursor positions before major jumps.
    pub jump_list: VecDeque<Cursor>,
    /// Current position in the jump list (index from the end; 0 = newest).
    pub jump_list_idx: usize,
    /// Cursor position saved when first pressing Ctrl-O, so Ctrl-I can restore it.
    pub jump_origin: Option<Cursor>,
    /// Snapshot of current line content taken when cursor enters a new line.
    pub line_snapshot: Option<(usize, String)>,
}

/// Last substitute command and replacement string for `&` and `:~` repeat.
#[derive(Debug, Default)]
pub struct SubstituteRepeat {
    /// Last substitute command for `&` repeat.
    pub last_substitute: Option<crate::search::substitute::SubstituteCommand>,
    /// Last replacement string from `:s` for `:~` repeat.
    pub last_replacement: Option<String>,
}

/// File argument list navigation state.
#[derive(Debug, Default)]
pub struct FileNavigation {
    /// The list of filenames given on the command line.
    pub arg_list: Vec<String>,
    /// Index into `arg_list` of the currently edited file.
    pub arg_idx: usize,
    /// Alternate file for Ctrl-^ switching.
    pub alternate_file: Option<String>,
}

/// Key mappings for normal and insert modes, plus abbreviations.
#[derive(Debug, Default)]
pub struct MappingsState {
    /// Key mappings for normal mode: (lhs keys, rhs keys).
    pub normal_maps: Vec<(Vec<crate::terminal::Key>, Vec<crate::terminal::Key>)>,
    /// Key mappings for insert mode: (lhs keys, rhs keys).
    pub insert_maps: Vec<(Vec<crate::terminal::Key>, Vec<crate::terminal::Key>)>,
    /// Insert-mode abbreviations: (lhs, rhs) pairs.
    pub abbreviations: Vec<(String, String)>,
    /// Keys buffered while resolving a multi-key map sequence.
    pub map_pending: Vec<crate::terminal::Key>,
    /// Current map-dispatch recursion depth (guards against recursive maps).
    pub map_depth: usize,
}

/// One entry on the tag stack: the file and cursor position from *before* a tag jump.
#[derive(Debug, Clone)]
pub struct TagStackEntry {
    /// File path before the jump (None if no file was open).
    pub file_path: Option<std::path::PathBuf>,
    /// Cursor position before the jump.
    pub cursor: Cursor,
}

/// Tag file cache and tag stack for ctags navigation.
#[derive(Debug, Default)]
pub struct TagNavigation {
    /// Cached parsed tags file, invalidated when the file's mtime changes.
    pub tags_cache: Option<(std::time::SystemTime, Vec<crate::tags::Tag>)>,
    /// Tag stack for `Ctrl-]` / `Ctrl-T` navigation.
    pub tag_stack: Vec<TagStackEntry>,
}

/// Editor state - mode and other transient state.
///
/// Contains mode, registers, undo history, settings, viewport, and search state.
/// Related fields are grouped into sub-structs for organization.
pub struct EditorState {
    /// Current editing mode
    pub mode: Mode,
    /// Viewport state tracking the visible portion of the buffer
    pub viewport: Viewport,
    /// Whether the editor should quit
    pub should_quit: bool,
    /// Status message to display (shown on the bottom line for one keypress cycle)
    pub status_message: Option<StatusMessage>,
    /// Command parser state for multi-key sequences
    pub parse_state: ParseState,
    /// Vi-style registers for yank/delete/put operations
    pub registers: Registers,
    /// Command-line state (active when mode == CommandLine)
    pub command_line: CommandLineState,
    /// Undo/redo history for reversible editing operations
    pub undo_history: UndoHistory,
    /// The last buffer-modifying command, for dot repeat.
    pub last_change: Option<RepeatableChange>,
    /// Persistent search state for `/`, `?`, `n`, and `N` commands.
    pub search: SearchState,
    /// Snapshot saved when entering a `/` or `?` command line with `incsearch` on.
    pub incsearch_saved: Option<IncSearchSaved>,
    /// Editor settings controlled by `:set` commands.
    pub settings: Settings,
    /// The fixed anchor point for the visual selection.
    pub visual_anchor: Option<Cursor>,
    /// When `true`, the next `render()` call will emit a BEL character.
    pub bell_pending: bool,
    /// Half-screen scroll size for Ctrl-d/Ctrl-u.
    pub scroll_half_size: Option<usize>,
    /// Pending filter range from `!{motion}` operator, awaiting shell command.
    pub pending_filter_range: Option<(usize, usize)>,
    /// Macro recording and playback state.
    pub macro_state: MacroState,
    /// Insert mode transient state.
    pub insert_state: InsertState,
    /// Navigation state: marks, jump list, line snapshot.
    pub navigation: NavigationState,
    /// Last substitute command and replacement for repeat.
    pub substitute_repeat: SubstituteRepeat,
    /// File argument list navigation state.
    pub file_navigation: FileNavigation,
    /// Key mappings and abbreviations.
    pub mappings: MappingsState,
    /// Tag file cache and tag stack.
    pub tag_navigation: TagNavigation,
}

impl EditorState {
    /// Create new editor state with the given viewport dimensions.
    pub fn new(viewport_height: usize, viewport_width: usize) -> Self {
        Self {
            mode: Mode::Normal,
            viewport: Viewport::new(0, viewport_height, viewport_width),
            should_quit: false,
            status_message: None,
            parse_state: ParseState::new(),
            registers: Registers::new(),
            command_line: CommandLineState::default(),
            undo_history: UndoHistory::new(),
            last_change: None,
            search: SearchState::new(),
            incsearch_saved: None,
            settings: Settings::default(),
            visual_anchor: None,
            bell_pending: false,
            scroll_half_size: None,
            pending_filter_range: None,
            macro_state: MacroState::default(),
            insert_state: InsertState::default(),
            navigation: NavigationState::default(),
            substitute_repeat: SubstituteRepeat::default(),
            file_navigation: FileNavigation::default(),
            mappings: MappingsState::default(),
            tag_navigation: TagNavigation::default(),
        }
    }
}

/// Main editor struct coordinating all components.
pub struct Editor {
    /// The document being edited
    pub document: Document,
    /// Editor state
    pub state: EditorState,
    /// Renderer for display
    renderer: Renderer,
    /// Raw mode handle (kept alive for duration of editor; None in test builds)
    _raw_mode: Option<RawMode>,
}

impl Editor {
    /// Create a new editor instance.
    pub fn new() -> Result<Self, Error> {
        let raw_mode = RawMode::enable()?;
        let size = terminal_size()?;
        let output = TerminalOutput::new();
        let renderer = Renderer::new(output, size);

        // Reserve one line for status line
        let viewport_height = (size.rows as usize).saturating_sub(1);
        let viewport_width = size.cols as usize;

        let mut editor = Self {
            document: Document::new(),
            state: EditorState::new(viewport_height, viewport_width),
            renderer,
            _raw_mode: Some(raw_mode),
        };

        // Load EXINIT env var before XDG config (POSIX order)
        editor.load_exinit();
        // Load XDG config file on startup
        editor.load_xdg_config();
        // Source .exrc in cwd if the exrc setting is enabled
        editor.load_exrc();

        // If errorbells is on and startup produced an error, arm the bell so it
        // fires on the very first render (before the event loop begins).
        if editor.state.settings.errorbells
            && editor
                .state
                .status_message
                .as_ref()
                .is_some_and(|m| m.is_error())
        {
            editor.state.bell_pending = true;
        }

        Ok(editor)
    }

    /// Execute a single ex command parsed from `EXINIT`, blocking dangerous variants.
    ///
    /// Shell execution (`:!`, `:sh`, `:r !`) and `:source` are refused to prevent
    /// arbitrary code execution via a manipulated environment variable.
    ///
    /// Note: `:map` is intentionally allowed. A mapping could theoretically bind a
    /// key to a shell command, but that requires the user to subsequently press the
    /// key — it does not execute on startup. Blocking `:map` would break the most
    /// common legitimate use of EXINIT. Traditional vi also permits `:map` in EXINIT.
    fn execute_exinit_command(&mut self, cmd: ExCommand) {
        match cmd {
            ExCommand::Shell(_)
            | ExCommand::ShellInteractive
            | ExCommand::ReadShellCommand { .. }
            | ExCommand::Source(_) => {
                self.state.status_message = Some(StatusMessage::Error(
                    "EXINIT: shell and :source commands are not allowed".to_string(),
                ));
            }
            other => self.execute_ex_command(other),
        }
    }

    /// Apply a string of ex commands sourced from `EXINIT`.
    ///
    /// Segments are separated by `|` or newlines. Lines starting with `"` are
    /// comments. Shell commands and `:source` are blocked for security.
    ///
    /// Factored out of `load_exinit` so tests can call it directly without
    /// touching the process environment.
    fn apply_exinit_value(&mut self, value: &str) {
        let ctx = crate::command::ExCommandContext::new(
            self.document.cursor.row,
            self.document.buffer.len(),
        );
        for segment in value.split(['|', '\n']) {
            let segment = segment.trim();
            if segment.is_empty() || segment.starts_with('"') {
                continue;
            }
            // parse_ex_command returns Err("") for unrecognised-but-empty input;
            // only surface non-empty error strings as status messages.
            match crate::command::parse_ex_command(segment, &ctx) {
                Ok(cmd) => self.execute_exinit_command(cmd),
                Err(e) if !e.is_empty() => {
                    self.state.status_message =
                        Some(StatusMessage::Error(format!("EXINIT: {}", e)));
                }
                _ => {}
            }
        }
    }

    /// Parse and execute the `EXINIT` environment variable as ex commands.
    fn load_exinit(&mut self) {
        let Ok(value) = std::env::var("EXINIT") else {
            return;
        };
        self.apply_exinit_value(&value);
    }

    /// Load the XDG config file (`$XDG_CONFIG_HOME/rvi/config` or
    /// `~/.config/rvi/config`) and execute each line as an ex command.
    ///
    /// Lines starting with `"` are treated as comments and skipped.
    /// Errors in individual commands are silently ignored (matching vi's
    /// `.exrc` behavior: a bad line doesn't abort loading).
    fn load_xdg_config(&mut self) {
        let config_path = xdg_config_path();
        let config_path = match config_path {
            Some(p) => p,
            None => return,
        };
        // Silently ignore missing/unreadable config file
        let _ = self.source_file(config_path.to_string_lossy().as_ref());
    }

    /// Source `.exrc` in the current working directory if the `exrc` setting
    /// is enabled and the file exists.
    fn load_exrc(&mut self) {
        if !self.state.settings.exrc {
            return;
        }
        let exrc_path = ".exrc";
        if std::path::Path::new(exrc_path).is_file() {
            let _ = self.source_file(exrc_path);
        }
    }

    /// Execute each line of `path` as an ex command (`:source` implementation).
    ///
    /// Lines starting with `"` are treated as comments and skipped.
    /// Returns an error string if the file cannot be read; errors in individual
    /// commands are silently ignored.
    fn source_file(&mut self, path: &str) -> Result<(), String> {
        self.source_file_depth(path, 0)
    }

    fn source_file_depth(&mut self, path: &str, depth: usize) -> Result<(), String> {
        const MAX_SOURCE_DEPTH: usize = 10;
        if depth >= MAX_SOURCE_DEPTH {
            return Err("source: nested :source too deep".to_string());
        }

        let content = std::fs::read_to_string(path).map_err(|e| format!("{}: {}", path, e))?;

        for line in content.lines() {
            let line = line.trim();
            // Skip empty lines and comments (lines starting with `"`)
            if line.is_empty() || line.starts_with('"') {
                continue;
            }
            // Rebuild context each iteration so ranges resolve against current state
            let ctx = crate::command::ExCommandContext::new(
                self.document.cursor.row,
                self.document.buffer.len(),
            );
            match crate::command::parse_ex_command(line, &ctx) {
                Ok(crate::command::ExCommand::Source(nested_path)) => {
                    // Handle :source specially to thread the depth counter
                    let _ = self.source_file_depth(&nested_path, depth + 1);
                }
                Ok(cmd) => {
                    self.execute_ex_command(cmd);
                }
                Err(_) => {}
            }
        }
        Ok(())
    }

    /// Navigate forward in the argument list by `count` files.
    pub fn navigate_args(&mut self, count: usize) {
        if self.state.file_navigation.arg_list.is_empty() {
            self.state.status_message =
                Some(StatusMessage::Error("(no argument list)".to_string()));
            return;
        }
        if self.document.modified && !self.state.settings.autowrite {
            self.state.status_message = Some(StatusMessage::Error(
                "No write since last change (add ! to override)".to_string(),
            ));
            return;
        }
        if self.document.modified && self.state.settings.autowrite {
            if let Err(e) = self.save_file() {
                self.state.status_message = Some(StatusMessage::Error(e.to_string()));
                return;
            }
        }
        let new_idx = self.state.file_navigation.arg_idx + count;
        if new_idx >= self.state.file_navigation.arg_list.len() {
            self.state.status_message = Some(StatusMessage::Error(
                "Last file in argument list".to_string(),
            ));
            return;
        }
        let prev = self.document.filename.clone();
        let path = self.state.file_navigation.arg_list[new_idx].clone();
        match self.open_file(&path) {
            Ok(()) => {
                self.state.file_navigation.arg_idx = new_idx;
                self.state.file_navigation.alternate_file = prev;
            }
            Err(e) => {
                self.state.status_message = Some(StatusMessage::Error(e.to_string()));
            }
        }
    }

    /// Navigate backward in the argument list by `count` files.
    pub fn navigate_args_backward(&mut self, count: usize) {
        if self.state.file_navigation.arg_list.is_empty() {
            self.state.status_message =
                Some(StatusMessage::Error("(no argument list)".to_string()));
            return;
        }
        if self.document.modified && !self.state.settings.autowrite {
            self.state.status_message = Some(StatusMessage::Error(
                "No write since last change (add ! to override)".to_string(),
            ));
            return;
        }
        if self.document.modified && self.state.settings.autowrite {
            if let Err(e) = self.save_file() {
                self.state.status_message = Some(StatusMessage::Error(e.to_string()));
                return;
            }
        }
        if self.state.file_navigation.arg_idx < count {
            self.state.status_message = Some(StatusMessage::Error(
                "First file in argument list".to_string(),
            ));
            return;
        }
        let new_idx = self.state.file_navigation.arg_idx - count;
        let prev = self.document.filename.clone();
        let path = self.state.file_navigation.arg_list[new_idx].clone();
        match self.open_file(&path) {
            Ok(()) => {
                self.state.file_navigation.arg_idx = new_idx;
                self.state.file_navigation.alternate_file = prev;
            }
            Err(e) => {
                self.state.status_message = Some(StatusMessage::Error(e.to_string()));
            }
        }
    }

    /// Set the read-only flag (used by `-R` command-line option).
    pub fn set_readonly(&mut self, value: bool) {
        self.state.settings.readonly = value;
    }

    /// Set the argument list (files passed on the command line).
    ///
    /// Opens the first file in the list, then stores the full list for
    /// `:n`, `:N`, and `:args` navigation.
    pub fn set_arg_list(&mut self, files: Vec<String>) -> Result<(), Error> {
        if files.is_empty() {
            return Ok(());
        }
        self.open_file(&files[0])?;
        self.state.file_navigation.arg_list = files;
        self.state.file_navigation.arg_idx = 0;
        Ok(())
    }

    /// Execute a list of ex command strings at startup.
    ///
    /// Called after the first file is loaded (if any). Each string is parsed
    /// and executed as an ex command; errors are shown as status messages.
    /// This implements the `-c {cmd}` and `+{cmd}` startup flags.
    pub fn execute_startup_commands(&mut self, commands: &[String]) {
        for cmd in commands {
            let ctx = self.make_ex_context();
            match crate::command::parse_ex_command(cmd.trim(), &ctx) {
                Ok(ex_cmd) => self.execute_ex_command(ex_cmd),
                Err(e) if !e.is_empty() => {
                    self.state.status_message = Some(StatusMessage::Error(format!("-c: {}", e)));
                }
                _ => {}
            }
        }
    }

    /// Create an editor with initial content.
    pub fn with_content(content: String) -> Result<Self, Error> {
        let mut editor = Self::new()?;
        editor.document = Document::from_string(content);
        Ok(editor)
    }

    /// Create a minimal editor for unit tests.
    ///
    /// Skips raw-mode terminal setup so tests run without a real TTY.
    /// Only available in test builds.
    #[cfg(test)]
    pub(crate) fn for_testing(content: &str) -> Self {
        use crate::terminal::{Size, TerminalOutput};

        let size = Size { rows: 24, cols: 80 };
        let output = TerminalOutput::new();
        let renderer = Renderer::new(output, size);
        let viewport_height = (size.rows as usize).saturating_sub(1);
        let viewport_width = size.cols as usize;

        Self {
            document: Document::from_string(content.to_string()),
            state: EditorState::new(viewport_height, viewport_width),
            renderer,
            _raw_mode: None,
        }
    }

    /// Open a file, replacing the current document content.
    pub fn open_file(&mut self, path: &str) -> Result<(), Error> {
        let path_buf = PathBuf::from(path);

        if !path_buf.exists() {
            // New file: clear buffer and reset state, then set filename
            self.document.buffer = Buffer::new();
            self.document.cursor = Cursor::new(0, 0);
            self.document.selection = None;
            self.document.filename = Some(path.to_string());
            self.document.file_path = Some(path_buf);
            self.document.modified = false;
            self.document.line_ending = LineEnding::default();
            self.state.undo_history = UndoHistory::new();
            return Ok(());
        }

        let result = crate::file::read_file(&path_buf)?;

        self.document.buffer = Buffer::from_string(result.content);
        self.document.cursor = Cursor::new(0, 0);
        self.document.selection = None;
        self.document.filename = Some(path.to_string());
        self.document.file_path = Some(path_buf);
        self.document.modified = false;
        self.document.line_ending = result.line_ending;

        // Clear undo history — history from the previous file must not bleed into this one
        self.state.undo_history = UndoHistory::new();

        // Initialize the U-command snapshot for row 0 so that pressing U immediately
        // after opening a file (without moving the cursor first) works correctly.
        let row0_content = self.document.buffer.line(0).unwrap_or("").to_string();
        self.state.navigation.line_snapshot = Some((0, row0_content));

        // Ensure cursor visible
        self.state
            .viewport
            .ensure_visible(0, self.document.buffer.len());

        Ok(())
    }

    /// Save the current document to its file path.
    ///
    /// Returns the number of bytes written. Blocked by `readonly`; use
    /// `save_file_force` to bypass.
    pub fn save_file(&mut self) -> Result<usize, Error> {
        if self.state.settings.readonly {
            return Err(Error::File(FileError::ReadOnly(
                "File is read-only (add ! to override)".to_string(),
            )));
        }
        self.save_file_force()
    }

    /// Save the current document, bypassing the `readonly` setting (`:w!`).
    pub fn save_file_force(&mut self) -> Result<usize, Error> {
        let path = self
            .document
            .file_path
            .clone()
            .ok_or_else(|| FileError::NotFound("No filename".to_string()))?;

        let options = WriteOptions {
            line_ending: self.document.line_ending,
            atomic: true,
            backup: false,
            ..WriteOptions::default()
        };

        let bytes = crate::file::write_file(&self.document.buffer, &path, &options)?;
        self.document.modified = false;
        self.state.status_message = Some(StatusMessage::WithPath {
            path: path.display().to_string(),
            suffix: format!(" written, {} bytes", bytes),
        });

        Ok(bytes)
    }

    /// Save to a specific path (for `:w filename`).
    ///
    /// Blocked by `readonly`; use `save_file_as_force` to bypass.
    pub fn save_file_as(&mut self, path: &str) -> Result<usize, Error> {
        if self.state.settings.readonly {
            return Err(Error::File(FileError::ReadOnly(
                "File is read-only (add ! to override)".to_string(),
            )));
        }
        self.save_file_as_force(path)
    }

    /// Save to a specific path, bypassing `readonly` (`:w! filename`).
    pub fn save_file_as_force(&mut self, path: &str) -> Result<usize, Error> {
        let path_buf = PathBuf::from(path);

        let options = WriteOptions {
            line_ending: self.document.line_ending,
            atomic: true,
            backup: false,
            ..WriteOptions::default()
        };

        let bytes = crate::file::write_file(&self.document.buffer, &path_buf, &options)?;
        self.document.file_path = Some(path_buf);
        self.document.filename = Some(path.to_string());
        self.document.modified = false;
        self.state.status_message = Some(StatusMessage::WithPath {
            path: path.to_string(),
            suffix: format!(" written, {} bytes", bytes),
        });

        Ok(bytes)
    }

    /// Write the current buffer to a crash-recovery temp file (`/tmp/{basename}.{pid}`).
    ///
    /// Uses non-atomic write so the pid-named file is written directly.
    /// Does not check `readonly` — the preserve destination is `/tmp`, not the
    /// original file, so the setting is not relevant here.
    /// Sets `status_message` on success or failure.
    fn preserve_buffer(&mut self) -> Result<(), Error> {
        let pid = std::process::id();
        let path = preserve_path(
            &self.state.settings.directory,
            self.document.filename.as_deref(),
            pid,
        );

        let options = WriteOptions {
            line_ending: self.document.line_ending,
            atomic: false,
            backup: false,
            ..WriteOptions::default()
        };

        let bytes =
            crate::file::write_file(&self.document.buffer, &path, &options).map_err(Error::File)?;

        self.state.status_message = Some(StatusMessage::WithPath {
            path: path.display().to_string(),
            suffix: format!(
                " {} lines, {} bytes preserved",
                self.document.buffer.len(),
                bytes
            ),
        });

        Ok(())
    }

    /// Reload the buffer from the most recently modified crash-recovery temp file.
    ///
    /// If `filename` is `Some(f)`, that basename is used to search `/tmp/{basename}.*`.
    /// If `None`, the document's current filename is used.
    /// On success the buffer is replaced, cursor and undo history reset,
    /// and the document is marked modified (unsaved vs. the original file).
    pub fn recover_buffer(&mut self, filename: Option<String>) -> Result<(), Error> {
        let base_name = filename
            .as_deref()
            .or(self.document.filename.as_deref())
            .and_then(|f| std::path::Path::new(f).file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("unnamed")
            .to_string();

        let preserve_file = Self::find_preserve_file(&self.state.settings.directory, &base_name)?;

        let result = crate::file::read_file(&preserve_file).map_err(Error::File)?;

        self.document.buffer = Buffer::from_string(result.content);
        self.document.cursor = Cursor::new(0, 0);
        self.document.selection = None;
        self.document.line_ending = result.line_ending;
        // Update filename if an explicit target was given.
        if let Some(ref fname) = filename {
            self.document.filename = Some(fname.clone());
            self.document.file_path = Some(PathBuf::from(fname));
        }
        // Recovered content differs from what's on disk — mark modified.
        self.document.modified = true;
        self.state.undo_history = UndoHistory::new();
        self.state
            .viewport
            .ensure_visible(0, self.document.buffer.len());

        self.state.status_message = Some(StatusMessage::Info(format!(
            "\"{}\" {} lines recovered",
            preserve_file.display(),
            self.document.buffer.len()
        )));

        Ok(())
    }

    /// Scan `/tmp` for the most recently modified preserve file matching `{base_name}.{pid}`.
    ///
    /// Returns `FileError::NotFound` if no matching file exists.
    fn find_preserve_file(dir: &str, base_name: &str) -> Result<PathBuf, Error> {
        let tmp = std::path::Path::new(dir);
        let prefix = format!("rvi.{}.", base_name);

        let mut candidates: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();

        let entries = std::fs::read_dir(tmp).map_err(|e| Error::File(FileError::Io(e)))?;
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with(&prefix) {
                let suffix = &name_str[prefix.len()..];
                if !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()) {
                    if let Ok(metadata) = entry.metadata() {
                        if let Ok(modified) = metadata.modified() {
                            candidates.push((entry.path(), modified));
                        }
                    }
                }
            }
        }

        candidates
            .into_iter()
            .max_by_key(|(_, t)| *t)
            .map(|(path, _)| path)
            .ok_or_else(|| {
                Error::File(FileError::NotFound(format!(
                    "No preserve file found for \"{}\" in {}",
                    base_name, dir
                )))
            })
    }

    /// List all rvi preserve files in `/tmp`, sorted by modification time (newest first).
    ///
    /// Matches files of the form `/tmp/rvi.{basename}.{pid}` — the `rvi.` prefix
    /// ensures only rvi-created files are returned, not unrelated `/tmp` files.
    ///
    /// Returns a `Vec` of display strings. Returns an empty `Vec` if `/tmp`
    /// cannot be read or contains no rvi preserve files.
    pub fn list_preserve_files() -> Vec<String> {
        const PREFIX: &str = "rvi.";
        let tmp = std::path::Path::new("/tmp");
        let Ok(entries) = std::fs::read_dir(tmp) else {
            return Vec::new();
        };

        let mut candidates: Vec<(PathBuf, std::time::SystemTime, String)> = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            // Must start with "rvi." and end with ".{digits}".
            if let Some(rest) = name_str.strip_prefix(PREFIX) {
                if let Some(dot) = rest.rfind('.') {
                    let suffix = &rest[dot + 1..];
                    if !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()) {
                        let basename = rest[..dot].to_string();
                        if let Ok(metadata) = entry.metadata() {
                            if let Ok(modified) = metadata.modified() {
                                candidates.push((entry.path(), modified, basename));
                            }
                        }
                    }
                }
            }
        }

        // Sort newest first.
        candidates.sort_by_key(|b| std::cmp::Reverse(b.1));
        candidates
            .into_iter()
            .map(|(path, _, basename)| {
                format!("  {}   (original file: {})", path.display(), basename)
            })
            .collect()
    }

    /// Expand `%` and `#` tokens in an ex command filename argument.
    ///
    /// - `%` → current filename (errors if no current file is set)
    /// - `#` → alternate filename (errors if no alternate file is set)
    /// - `\%` and `\#` → literal `%` / `#` (escaped forms)
    ///
    /// Returns `Ok(expanded)` or `Err(message)` if a token cannot be resolved.
    fn expand_filename(&self, arg: &str) -> Result<String, String> {
        let mut result = String::with_capacity(arg.len());
        let mut chars = arg.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '\\' => match chars.peek() {
                    Some('%') | Some('#') => {
                        result.push(chars.next().unwrap());
                    }
                    _ => result.push('\\'),
                },
                '%' => {
                    let name = self
                        .document
                        .filename
                        .as_deref()
                        .ok_or_else(|| "E499: Empty file name for '%'".to_string())?;
                    result.push_str(name);
                }
                '#' => {
                    let name = self
                        .state
                        .file_navigation
                        .alternate_file
                        .as_deref()
                        .ok_or_else(|| {
                            "E499: No alternate file name to substitute for '#'".to_string()
                        })?;
                    result.push_str(name);
                }
                other => result.push(other),
            }
        }
        Ok(result)
    }

    /// Execute a parsed ex command.
    ///
    /// Side effects (quit, save, status messages) are applied to the editor
    /// state. Errors from file operations are converted to status messages
    /// rather than propagated, to avoid crashing the editor on a failed :w.
    pub fn execute_ex_command(&mut self, cmd: ExCommand) {
        match cmd {
            ExCommand::Write => {
                if let Err(e) = self.save_file() {
                    self.state.status_message = Some(StatusMessage::Error(e.to_string()));
                }
            }
            ExCommand::ForceWrite => {
                if let Err(e) = self.save_file_force() {
                    self.state.status_message = Some(StatusMessage::Error(e.to_string()));
                }
            }
            ExCommand::WriteAs(filename) => {
                let filename = match self.expand_filename(&filename) {
                    Ok(f) => f,
                    Err(e) => {
                        self.state.status_message = Some(StatusMessage::Error(e));
                        return;
                    }
                };
                if let Err(e) = self.save_file_as(&filename) {
                    self.state.status_message = Some(StatusMessage::Error(e.to_string()));
                }
            }
            ExCommand::ForceWriteAs(filename) => {
                let filename = match self.expand_filename(&filename) {
                    Ok(f) => f,
                    Err(e) => {
                        self.state.status_message = Some(StatusMessage::Error(e));
                        return;
                    }
                };
                if let Err(e) = self.save_file_as_force(&filename) {
                    self.state.status_message = Some(StatusMessage::Error(e.to_string()));
                }
            }
            ExCommand::Quit => {
                if self.document.modified && self.state.settings.autowrite {
                    if let Err(e) = self.save_file() {
                        self.state.status_message = Some(StatusMessage::Error(e.to_string()));
                        return;
                    }
                }
                if self.document.modified {
                    self.state.status_message = Some(StatusMessage::Error(
                        "No write since last change (add ! to override)".to_string(),
                    ));
                } else {
                    self.state.should_quit = true;
                }
            }
            ExCommand::ForceQuit => {
                self.state.should_quit = true;
            }
            ExCommand::WriteQuit => {
                if let Err(e) = self.save_file() {
                    self.state.status_message = Some(StatusMessage::Error(e.to_string()));
                } else {
                    self.state.should_quit = true;
                }
            }
            ExCommand::ForceWriteQuit => {
                if let Err(e) = self.save_file_force() {
                    self.state.status_message = Some(StatusMessage::Error(e.to_string()));
                } else {
                    self.state.should_quit = true;
                }
            }
            ExCommand::WriteQuitAs(filename) => {
                let filename = match self.expand_filename(&filename) {
                    Ok(f) => f,
                    Err(e) => {
                        self.state.status_message = Some(StatusMessage::Error(e));
                        return;
                    }
                };
                if let Err(e) = self.save_file_as(&filename) {
                    self.state.status_message = Some(StatusMessage::Error(e.to_string()));
                } else {
                    self.state.should_quit = true;
                }
            }
            ExCommand::WriteQuitIfModified => {
                if self.document.modified {
                    if let Err(e) = self.save_file() {
                        self.state.status_message = Some(StatusMessage::Error(e.to_string()));
                        return;
                    }
                }
                self.state.should_quit = true;
            }
            ExCommand::Edit(filename) => {
                let filename = match self.expand_filename(&filename) {
                    Ok(f) => f,
                    Err(e) => {
                        self.state.status_message = Some(StatusMessage::Error(e));
                        return;
                    }
                };
                if self.document.modified && self.state.settings.autowrite {
                    if let Err(e) = self.save_file() {
                        self.state.status_message = Some(StatusMessage::Error(e.to_string()));
                        return;
                    }
                }
                if self.document.modified {
                    self.state.status_message = Some(StatusMessage::Error(
                        "No write since last change (add ! to override)".to_string(),
                    ));
                } else {
                    let prev = self.document.filename.clone();
                    if let Err(e) = self.open_file(&filename) {
                        self.state.status_message = Some(StatusMessage::Error(e.to_string()));
                    } else {
                        self.state.file_navigation.alternate_file = prev;
                    }
                }
            }
            ExCommand::ForceEdit => {
                if let Some(path) = self.document.filename.clone() {
                    if let Err(e) = self.open_file(&path) {
                        self.state.status_message = Some(StatusMessage::Error(e.to_string()));
                    }
                } else {
                    self.state.status_message =
                        Some(StatusMessage::Error("No file name".to_string()));
                }
            }
            ExCommand::Substitute(cmd) => {
                self.state.substitute_repeat.last_substitute = Some(cmd.clone());
                self.execute_substitute(cmd);
            }
            ExCommand::RepeatSubstitute { range } => {
                // Repeat the full last :s command (pattern + replacement + flags) on the range.
                if let Some(mut cmd) = self.state.substitute_repeat.last_substitute.clone() {
                    cmd.range = range;
                    self.execute_substitute(cmd);
                } else {
                    self.state.status_message =
                        Some(StatusMessage::Error("No previous substitute".to_string()));
                }
            }
            ExCommand::RepeatSubstituteTilde { range } => {
                // Use the current search pattern with the last replacement string.
                let pattern = match self.state.search.last_pattern() {
                    Some(p) => p.to_string(),
                    None => {
                        self.state.status_message =
                            Some(StatusMessage::Error("No previous pattern".to_string()));
                        return;
                    }
                };
                let replacement = match self.state.substitute_repeat.last_replacement.clone() {
                    Some(r) => r,
                    None => {
                        self.state.status_message =
                            Some(StatusMessage::Error("No previous substitute".to_string()));
                        return;
                    }
                };
                let cmd = SubstituteCommand {
                    range,
                    pattern,
                    replacement,
                    flags: crate::search::substitute::SubstituteFlags::default(),
                };
                self.execute_substitute(cmd);
            }
            ExCommand::Set(cmd) => {
                self.execute_set_command(cmd);
            }
            ExCommand::NoHighlight => {
                self.state.search.suppress_highlights = true;
            }
            ExCommand::GotoLine(n) => {
                let total = self.document.buffer.len();
                if n == 0 || n > total {
                    self.state.status_message =
                        Some(StatusMessage::Error(format!("Invalid line number: {}", n)));
                } else {
                    self.push_jump();
                    self.document.cursor.row = n - 1; // 1-based -> 0-based
                    self.document.cursor.col = 0;
                    // Move to first non-blank
                    if let Some(line) = self.document.buffer.line(self.document.cursor.row) {
                        let col = line
                            .char_indices()
                            .find(|(_, ch)| !ch.is_whitespace())
                            .map(|(idx, _)| idx)
                            .unwrap_or(0);
                        self.document.cursor.col = col;
                    }
                    self.state
                        .viewport
                        .ensure_visible(self.document.cursor.row, total);
                }
            }
            ExCommand::Global {
                pattern,
                command,
                inverse,
            } => {
                self.execute_global_command(pattern, command, inverse);
            }
            ExCommand::Shell(cmd) => {
                // autowrite: save before shell command
                if self.document.modified && self.state.settings.autowrite {
                    if let Err(e) = self.save_file() {
                        self.state.status_message = Some(StatusMessage::Error(e.to_string()));
                        return;
                    }
                }
                // warn: show warning before shell command if buffer modified
                if self.document.modified
                    && self.state.settings.warn
                    && !self.state.settings.autowrite
                {
                    self.state.status_message = Some(StatusMessage::Error(
                        "No write since last change".to_string(),
                    ));
                }
                if let Err(e) = self.execute_shell_command(&cmd) {
                    self.state.status_message = Some(StatusMessage::Error(e.to_string()));
                }
            }
            ExCommand::ReadFile(filename) => {
                let filename = match self.expand_filename(&filename) {
                    Ok(f) => f,
                    Err(e) => {
                        self.state.status_message = Some(StatusMessage::Error(e));
                        return;
                    }
                };
                self.execute_read_file(&filename);
            }
            ExCommand::PrintLineNumber(row) => {
                let total = self.document.buffer.len();
                self.state.status_message =
                    Some(StatusMessage::Info(format!("{}/{}", row + 1, total)));
            }
            ExCommand::DeleteLines { range, register } => {
                self.execute_delete_lines(range, register);
            }
            ExCommand::YankLines { range, register } => {
                self.execute_yank_lines(range, register);
            }
            ExCommand::Put {
                after_row,
                register,
            } => {
                self.execute_put_lines(after_row, register);
            }
            ExCommand::MoveLines { range, dest } => {
                self.execute_move_lines(range, dest);
            }
            ExCommand::CopyLines { range, dest } => {
                self.execute_copy_lines(range, dest);
            }
            ExCommand::JoinLines { range } => {
                self.execute_join_lines_range(range);
            }
            ExCommand::NextFile(filename) => {
                if let Some(f) = filename {
                    // :n {file} — open a specific file
                    let f = match self.expand_filename(&f) {
                        Ok(expanded) => expanded,
                        Err(e) => {
                            self.state.status_message = Some(StatusMessage::Error(e));
                            return;
                        }
                    };
                    if self.document.modified && self.state.settings.autowrite {
                        if let Err(e) = self.save_file() {
                            self.state.status_message = Some(StatusMessage::Error(e.to_string()));
                            return;
                        }
                    }
                    if self.document.modified {
                        self.state.status_message = Some(StatusMessage::Error(
                            "No write since last change (add ! to override)".to_string(),
                        ));
                    } else if let Err(e) = self.open_file(&f) {
                        self.state.status_message = Some(StatusMessage::Error(e.to_string()));
                    }
                } else {
                    // :n — advance to next arg
                    self.navigate_args(1);
                }
            }
            ExCommand::PrevFile => {
                self.navigate_args_backward(1);
            }
            ExCommand::ShowArgs => {
                let args = &self.state.file_navigation.arg_list;
                if args.is_empty() {
                    self.state.status_message =
                        Some(StatusMessage::Error("(no argument list)".to_string()));
                } else {
                    let display: Vec<String> = args
                        .iter()
                        .enumerate()
                        .map(|(i, f)| {
                            if i == self.state.file_navigation.arg_idx {
                                format!("[{}]", f)
                            } else {
                                f.clone()
                            }
                        })
                        .collect();
                    self.state.status_message = Some(StatusMessage::Info(display.join("  ")));
                }
            }
            ExCommand::RewindArgs => {
                if self.state.file_navigation.arg_list.is_empty() {
                    self.state.status_message =
                        Some(StatusMessage::Error("(no argument list)".to_string()));
                } else if self.document.modified && !self.state.settings.autowrite {
                    self.state.status_message = Some(StatusMessage::Error(
                        "No write since last change (add ! to override)".to_string(),
                    ));
                } else {
                    if self.document.modified && self.state.settings.autowrite {
                        if let Err(e) = self.save_file() {
                            self.state.status_message = Some(StatusMessage::Error(e.to_string()));
                            return;
                        }
                    }
                    let prev = self.document.filename.clone();
                    let first = self.state.file_navigation.arg_list[0].clone();
                    if let Err(e) = self.open_file(&first) {
                        self.state.status_message = Some(StatusMessage::Error(e.to_string()));
                    } else {
                        self.state.file_navigation.arg_idx = 0;
                        self.state.file_navigation.alternate_file = prev;
                    }
                }
            }
            ExCommand::ReadShellCommand { cmd } => {
                self.execute_read_shell_command(&cmd);
            }
            ExCommand::PrintLines { range } => {
                self.execute_print_lines(range);
            }
            ExCommand::PrintNumberedLines { range } => {
                self.execute_print_numbered_lines(range);
            }
            ExCommand::PrintListLines { range } => {
                self.execute_print_list_lines(range);
            }
            ExCommand::Map {
                insert_mode,
                lhs,
                rhs,
            } => {
                self.execute_map(insert_mode, &lhs, &rhs);
            }
            ExCommand::Unmap { insert_mode, lhs } => {
                self.execute_unmap(insert_mode, &lhs);
            }
            ExCommand::ShowMaps { insert_mode } => {
                self.execute_show_maps(insert_mode);
            }
            ExCommand::Abbreviate { lhs, rhs } => {
                self.execute_abbreviate(&lhs, &rhs);
            }
            ExCommand::Unabbreviate { lhs } => {
                self.execute_unabbreviate(&lhs);
            }
            ExCommand::ShowAbbreviations => {
                self.execute_show_abbreviations();
            }
            ExCommand::ShellInteractive => {
                if let Err(e) = self.execute_shell_interactive() {
                    self.state.status_message = Some(StatusMessage::Error(e.to_string()));
                }
            }
            ExCommand::ChangeDir(dir) => match std::env::set_current_dir(&dir) {
                Ok(()) => {
                    self.state.status_message = None;
                }
                Err(e) => {
                    self.state.status_message =
                        Some(StatusMessage::Error(format!("{}: {}", dir, e)));
                }
            },
            ExCommand::Source(file) => {
                let file = match self.expand_filename(&file) {
                    Ok(f) => f,
                    Err(e) => {
                        self.state.status_message = Some(StatusMessage::Error(e));
                        return;
                    }
                };
                match self.source_file(&file) {
                    Ok(()) => {
                        self.state.status_message = None;
                    }
                    Err(e) => {
                        self.state.status_message = Some(StatusMessage::Error(e));
                    }
                }
            }
            ExCommand::SetMark(ch) => {
                self.state.navigation.marks.insert(ch, self.document.cursor);
                self.state.status_message = None;
            }
            ExCommand::Suspend => {
                if let Err(e) = self.suspend_editor() {
                    self.state.status_message =
                        Some(StatusMessage::Error(format!("suspend: {}", e)));
                }
            }
            ExCommand::AppendLines { insert_at } => match self.collect_ex_lines() {
                Ok(lines) => self.insert_lines_at(insert_at, lines),
                Err(e) => self.state.status_message = Some(StatusMessage::Error(e)),
            },
            ExCommand::InsertLines { insert_at } => match self.collect_ex_lines() {
                Ok(lines) => self.insert_lines_at(insert_at, lines),
                Err(e) => self.state.status_message = Some(StatusMessage::Error(e)),
            },
            ExCommand::ChangeLines { start, end } => match self.collect_ex_lines() {
                Ok(new_lines) => self.execute_change_lines(start, end, new_lines),
                Err(e) => self.state.status_message = Some(StatusMessage::Error(e)),
            },
            ExCommand::Version => {
                self.state.status_message = Some(StatusMessage::Info(format!(
                    "rvi version {}",
                    env!("CARGO_PKG_VERSION")
                )));
            }
            ExCommand::Visual => {
                // Handled by run_ex_mode() — no-op if somehow called outside ex mode.
            }
            ExCommand::Preserve => {
                if let Err(e) = self.preserve_buffer() {
                    self.state.status_message = Some(StatusMessage::Error(e.to_string()));
                }
            }
            ExCommand::Recover(filename) => {
                if let Err(e) = self.recover_buffer(filename) {
                    self.state.status_message = Some(StatusMessage::Error(e.to_string()));
                }
            }
            ExCommand::Tag(name) => {
                if let Err(e) = self.execute_tag_jump(&name) {
                    self.state.status_message = Some(StatusMessage::Error(e));
                }
            }
            ExCommand::PopTag => {
                if let Err(e) = self.execute_tag_pop() {
                    self.state.status_message = Some(StatusMessage::Error(e));
                }
            }
        }
    }

    /// Return `true` if `a` and `b` refer to the same file on disk.
    ///
    /// Attempts `std::fs::canonicalize` on both paths and compares the results.
    /// Falls back to a plain string comparison when canonicalization fails (e.g.
    /// for a new file that does not yet exist on disk).
    fn same_file(a: &str, b: &str) -> bool {
        if a == b {
            return true;
        }
        let ca = std::fs::canonicalize(a);
        let cb = std::fs::canonicalize(b);
        match (ca, cb) {
            (Ok(pa), Ok(pb)) => pa == pb,
            _ => false,
        }
    }

    /// Load the `tags` file, using a cached copy when it has not changed on disk.
    ///
    /// Re-reads the file only when its mtime has advanced since the last parse.
    /// Returns `Err` with a message string on I/O failure.
    fn load_tags(&mut self) -> Result<&[crate::tags::Tag], String> {
        // Try each path in the colon-separated tags setting
        let tags_paths: Vec<&str> = self.state.settings.tags.split(':').collect();

        // Find the first tags file that exists and get its mtime
        let mut found_path: Option<&str> = None;
        let mut found_mtime = None;
        for path in &tags_paths {
            if let Ok(meta) = std::fs::metadata(path) {
                if let Ok(mtime) = meta.modified() {
                    found_path = Some(path);
                    found_mtime = Some(mtime);
                    break;
                }
            }
        }

        let tags_path = found_path
            .ok_or_else(|| format!("{}: No such file or directory", self.state.settings.tags))?;
        // found_mtime is set in the same branch as found_path, so it is always Some when found_path is Some
        let mtime = found_mtime.expect("found_mtime is set whenever found_path is set");

        // Invalidate cache if mtime changed
        if let Some((cached_mtime, _)) = &self.state.tag_navigation.tags_cache {
            if *cached_mtime != mtime {
                self.state.tag_navigation.tags_cache = None;
            }
        }

        if self.state.tag_navigation.tags_cache.is_none() {
            let tags = crate::tags::parse_tags_file(tags_path)?;
            self.state.tag_navigation.tags_cache = Some((mtime, tags));
        }

        // tags_cache is guaranteed Some by the block above that populates it when None
        Ok(&self
            .state
            .tag_navigation
            .tags_cache
            .as_ref()
            .expect("tags_cache was just populated")
            .1)
    }

    /// Jump to the tag named `tagstring`.
    ///
    /// Returns `Ok(())` on success or `Err(message)` on failure so that callers
    /// such as `main` can surface the error on stderr.
    ///
    /// Algorithm:
    /// 1. Load (or re-use cached) `tags` file from the current working directory.
    /// 2. Look up `tagstring` (exact match).
    /// 3. Record current position in the jump list.
    /// 4. If the tag is in a different file, switch to that file (respecting
    ///    `autowrite`).
    /// 5. Reposition the cursor per the tag's address (line number or pattern).
    pub fn execute_tag_jump(&mut self, tagstring: &str) -> Result<(), String> {
        // 1. Load tags (cached)
        let tl = self.state.settings.taglength;
        let tag = {
            let tags = self.load_tags()?;
            let found = if tl > 0 {
                // Truncate lookup key to taglength chars, then prefix-match
                let truncated: String = tagstring.chars().take(tl).collect();
                crate::tags::find_tag_prefix(tags, &truncated)
            } else {
                crate::tags::find_tag(tags, tagstring)
            };
            match found {
                Some(t) => t.clone(),
                None => return Err(format!("Tag not found: {}", tagstring)),
            }
        };

        // 2. Record current position in jump list and tag stack
        self.push_jump();
        self.state.tag_navigation.tag_stack.push(TagStackEntry {
            file_path: self.document.file_path.clone(),
            cursor: self.document.cursor,
        });

        // 3. Switch file if needed — compare using canonicalized paths so that
        //    relative vs absolute paths to the same file do not trigger a reload.
        let current = self
            .document
            .file_path
            .as_ref()
            .and_then(|p| p.to_str())
            .unwrap_or("");
        if !Self::same_file(&tag.filename, current) {
            if self.document.modified && !self.state.settings.autowrite {
                return Err("No write since last change (add ! to override)".to_string());
            }
            if self.document.modified && self.state.settings.autowrite {
                if let Err(e) = self.save_file() {
                    return Err(e.to_string());
                }
            }
            if let Err(e) = self.open_file(&tag.filename) {
                return Err(e.to_string());
            }
        }

        // 4. Position cursor per address
        let total = self.document.buffer.len();
        match &tag.address {
            crate::tags::TagAddress::Line(n) => {
                let row = n.saturating_sub(1).min(total.saturating_sub(1));
                self.document.cursor.row = row;
                self.document.cursor.col = 0;
                if let Some(line) = self.document.buffer.line(row) {
                    let col = line
                        .char_indices()
                        .find(|(_, ch)| !ch.is_whitespace())
                        .map(|(idx, _)| idx)
                        .unwrap_or(0);
                    self.document.cursor.col = col;
                }
            }
            crate::tags::TagAddress::Pattern(pat) => {
                let regex = ViRegex::compile(pat).map_err(|e| format!("Bad tag pattern: {}", e))?;
                let start = crate::buffer::Cursor::new(0, 0);
                match crate::search::find_next(
                    &self.document.buffer,
                    &start,
                    &regex,
                    crate::search::SearchDirection::Forward,
                    true,
                ) {
                    Ok(Some((m, _))) => {
                        self.document.cursor.row = m.row;
                        self.document.cursor.col = m.col_start;
                    }
                    Ok(None) => return Err("Pattern not found".to_string()),
                    Err(e) => return Err(e.to_string()),
                }
            }
        }

        self.state
            .viewport
            .ensure_visible(self.document.cursor.row, total);
        Ok(())
    }

    /// Pop the tag stack (`Ctrl-T` / `:pop`).
    ///
    /// Returns to the file and cursor position saved when the most recent
    /// `Ctrl-]` or `:tag` jump was made. If the saved file differs from the
    /// current file, switches back to it (respecting `autowrite`).
    pub fn execute_tag_pop(&mut self) -> Result<(), String> {
        let entry = match self.state.tag_navigation.tag_stack.pop() {
            Some(e) => e,
            None => return Err("tag stack empty".to_string()),
        };

        // Autowrite if we are about to leave a modified buffer
        let current = self
            .document
            .file_path
            .as_ref()
            .and_then(|p| p.to_str())
            .unwrap_or("");
        let target = entry
            .file_path
            .as_ref()
            .and_then(|p| p.to_str())
            .unwrap_or("");

        if !Self::same_file(current, target) {
            if self.document.modified && !self.state.settings.autowrite {
                // Push the entry back so we don't lose it
                self.state.tag_navigation.tag_stack.push(entry);
                return Err("No write since last change (add ! to override)".to_string());
            }
            if self.document.modified && self.state.settings.autowrite {
                if let Err(e) = self.save_file() {
                    self.state.tag_navigation.tag_stack.push(entry);
                    return Err(e.to_string());
                }
            }
            match &entry.file_path {
                Some(p) => {
                    let path_str = p.to_string_lossy().into_owned();
                    if let Err(e) = self.open_file(&path_str) {
                        return Err(e.to_string());
                    }
                }
                None => {
                    // No file was open before the jump — open an empty buffer
                    self.document = Document::default();
                }
            }
        }

        // Restore cursor
        let total = self.document.buffer.len();
        self.document.cursor.row = entry.cursor.row.min(total.saturating_sub(1));
        self.document.cursor.col = entry.cursor.col;
        self.state
            .viewport
            .ensure_visible(self.document.cursor.row, total);
        Ok(())
    }

    /// Execute a `:g/pattern/cmd` or `:v/pattern/cmd` global command.
    ///
    /// Supports: `d` (delete), `p` (print), `y[reg]` (yank), `j` (join),
    /// and `s/pat/rep/flags` (substitute on each matching line).
    fn execute_global_command(&mut self, pattern: String, command: String, inverse: bool) {
        let case_insensitive = self.state.settings.ignorecase;
        let magic = self.state.settings.magic;
        let regex = match ViRegex::compile_with_magic(&pattern, case_insensitive, magic) {
            Ok(r) => r,
            Err(e) => {
                self.state.status_message = Some(StatusMessage::Error(e.to_string()));
                return;
            }
        };

        // Collect matching row indices before any mutations.
        let mut matching: Vec<usize> = Vec::new();
        for row in 0..self.document.buffer.len() {
            if let Some(line) = self.document.buffer.line(row) {
                let matched = regex.find_in(line).unwrap_or(None).is_some();
                if matched != inverse {
                    matching.push(row);
                }
            }
        }

        if matching.is_empty() {
            self.state.status_message = Some(StatusMessage::Error("Pattern not found".to_string()));
            return;
        }

        let cmd_trimmed = command.trim();

        // Empty sub-command: just move cursor to each matching line (last one wins).
        if cmd_trimmed.is_empty() {
            if let Some(&last) = matching.last() {
                self.document.cursor.row = last;
                self.document.cursor.col = 0;
                self.move_to_first_non_blank();
                let total = self.document.buffer.len();
                self.state.viewport.ensure_visible(last, total);
            }
            return;
        }

        // General executor: for each matching row, set the cursor to that line
        // (adjusting for any deletions/insertions that have already occurred)
        // and run the sub-command via the normal ex command pipeline.
        //
        // Row-adjustment strategy: track a signed offset that accumulates as lines
        // are inserted (+) or deleted (-). This keeps the originally-collected
        // row indices valid after mutations.
        //
        // Each sub-command manages its own undo group (via execute_ex_command),
        // so individual steps are separately undoable — consistent with most vi
        // implementations.
        let mut row_offset: i64 = 0;
        let mut last_error: Option<String> = None;

        for orig_row in matching {
            let adjusted = orig_row as i64 + row_offset;
            if adjusted < 0 || adjusted as usize >= self.document.buffer.len() {
                continue;
            }
            let row = adjusted as usize;

            // Move cursor to the matching line so that commands using `.` work.
            self.document.cursor.row = row;
            self.document.cursor.col = 0;

            let buf_len_before = self.document.buffer.len();

            // Build a full context for this iteration so /pattern/ addresses work.
            let ctx = self.make_ex_context();
            let parsed = crate::command::parse_ex_command(cmd_trimmed, &ctx);
            match parsed {
                Ok(cmd) => {
                    self.execute_ex_command(cmd);
                    // If the status message is an error, surface it and stop.
                    if let Some(StatusMessage::Error(ref msg)) = self.state.status_message {
                        last_error = Some(msg.clone());
                        break;
                    }
                }
                Err(msg) if !msg.is_empty() => {
                    last_error = Some(msg);
                    break;
                }
                _ => {}
            }

            let buf_len_after = self.document.buffer.len();
            row_offset += buf_len_after as i64 - buf_len_before as i64;
        }

        if let Some(e) = last_error {
            self.state.status_message = Some(StatusMessage::Error(e));
        } else {
            // Clear any per-iteration status message; the operation as a whole succeeded.
            self.state.status_message = None;
        }

        let new_len = self.document.buffer.len();
        if self.document.cursor.row >= new_len {
            self.document.cursor.row = new_len.saturating_sub(1);
        }
        self.state
            .viewport
            .ensure_visible(self.document.cursor.row, new_len);
    }

    /// Return the shell path to use for `:!` and `:sh` commands.
    ///
    /// If the `shell` setting is non-empty, use it. Otherwise fall back to
    /// the `$SHELL` environment variable, then `/bin/sh`.
    fn shell_path(&self) -> String {
        if !self.state.settings.shell.is_empty() {
            self.state.settings.shell.clone()
        } else {
            std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
        }
    }

    /// Execute a `:r {filename}` command — read file and insert below current line.
    /// Execute a shell command (`:!{cmd}`).
    ///
    /// Suspends raw mode, runs the command with inherited stdio, waits for the
    /// user to press Enter, then restores raw mode and redraws the screen.
    fn execute_shell_command(&mut self, cmd: &str) -> Result<(), Error> {
        use std::io::Write;

        // Drop raw mode to restore normal terminal behavior
        self._raw_mode = None;

        // Move to a fresh line and show the command being run
        print!("\r\n:!{}\r\n", cmd);
        let _ = std::io::stdout().flush();

        // Run command via sh -c with inherited stdio
        let status = std::process::Command::new(self.shell_path())
            .arg("-c")
            .arg(cmd)
            .status();

        match status {
            Ok(exit) => {
                if !exit.success() {
                    if let Some(code) = exit.code() {
                        print!("\r\nshell returned {}", code);
                    }
                }
            }
            Err(e) => {
                print!("\r\nError running command: {}", e);
            }
        }

        print!("\r\n[Press Enter to continue]");
        let _ = std::io::stdout().flush();

        // Wait for Enter key
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);

        // Restore raw mode — if this fails the editor cannot function, so propagate
        // the error as fatal rather than leaving the terminal in cooked mode.
        self._raw_mode = Some(RawMode::enable().map_err(Error::Terminal)?);

        // Force a full redraw
        self.state.status_message = None;
        Ok(())
    }

    /// Execute `:sh` — launch `$SHELL` (or `/bin/sh`) interactively.
    ///
    /// Drops raw mode, hands the terminal to the shell, waits for it to exit,
    /// then restores raw mode and forces a full redraw.
    fn execute_shell_interactive(&mut self) -> Result<(), Error> {
        use std::io::Write;

        // Drop raw mode to restore normal terminal behavior
        self._raw_mode = None;

        print!("\r\n");
        let _ = std::io::stdout().flush();

        let shell = self.shell_path();
        let status = std::process::Command::new(&shell).status();

        match status {
            Ok(exit) => {
                if !exit.success() {
                    if let Some(code) = exit.code() {
                        print!("\r\nshell returned {}", code);
                        let _ = std::io::stdout().flush();
                    }
                }
            }
            Err(e) => {
                print!("\r\nError launching shell: {}", e);
                let _ = std::io::stdout().flush();
            }
        }

        // Restore raw mode
        self._raw_mode = Some(RawMode::enable().map_err(Error::Terminal)?);

        // Force a full redraw
        self.state.status_message = None;
        Ok(())
    }

    /// Suspend the editor by sending SIGTSTP to the current process.
    ///
    /// Drops raw mode first, then uses `nix::sys::signal::kill` to send
    /// `SIGTSTP` to the current process. After the shell resumes the process
    /// with `SIGCONT`, re-enables raw mode and forces a full redraw.
    fn suspend_editor(&mut self) -> Result<(), Error> {
        use nix::sys::signal::{kill, Signal};
        use nix::unistd::Pid;
        use std::io::Write;

        // Drop raw mode so the terminal is restored before the stop
        self._raw_mode = None;
        let _ = std::io::stdout().flush();

        // Send SIGTSTP to ourselves; the kernel suspends us here
        kill(Pid::this(), Signal::SIGTSTP)
            .map_err(|e| Error::Io(std::io::Error::other(e.to_string())))?;

        // We resume here after SIGCONT
        self._raw_mode = Some(RawMode::enable().map_err(Error::Terminal)?);
        self.state.status_message = None;
        Ok(())
    }

    /// Collect lines from the user until a lone `.` is entered.
    ///
    /// Drops raw mode and reads lines from stdin until a line containing only
    /// `.` is entered (POSIX vi convention for ex line-input commands).
    /// Raw mode is always restored before returning, even on I/O error.
    fn collect_ex_lines(&mut self) -> Result<Vec<String>, String> {
        use std::io::{BufRead, Write};

        self._raw_mode = None;
        print!("\r\n");
        let _ = std::io::stdout().flush();

        let stdin = std::io::stdin();
        let mut lines = Vec::new();
        let mut io_err: Option<String> = None;
        for raw in stdin.lock().lines() {
            match raw {
                Err(e) => {
                    io_err = Some(e.to_string());
                    break;
                }
                Ok(line) => {
                    if line == "." {
                        break;
                    }
                    lines.push(line);
                }
            }
        }

        // Always restore raw mode before returning
        match RawMode::enable() {
            Ok(rm) => self._raw_mode = Some(rm),
            Err(e) => return Err(Error::Terminal(e).to_string()),
        }

        if let Some(e) = io_err {
            return Err(e);
        }
        Ok(lines)
    }

    /// Print the current buffer line to stdout (used by ex mode autoprint).
    fn print_current_line_ex(&self) {
        if let Some(line) = self.document.buffer.line(self.document.cursor.row) {
            println!("{}", line);
        }
    }

    /// Drain `status_message` to stdout.
    ///
    /// Returns `true` if a message was printed (caller should skip autoprint).
    fn flush_ex_status(&mut self) -> bool {
        if let Some(msg) = self.state.status_message.take() {
            println!("{}", msg.as_str_formatted());
            return true;
        }
        false
    }

    /// Build an `ExCommandContext` from the current editor state.
    fn make_ex_context(&self) -> crate::command::ExCommandContext {
        let marks = self
            .state
            .navigation
            .marks
            .iter()
            .map(|(&c, v)| (c, v.row))
            .collect();
        let buffer_lines: std::sync::Arc<[String]> = self.document.buffer.lines().into();
        let last_pattern = self.state.search.last_pattern().map(|s| s.to_string());
        crate::command::ExCommandContext {
            current_line: self.document.cursor.row,
            buffer_len: self.document.buffer.len(),
            marks,
            buffer_lines,
            last_pattern,
        }
    }

    /// Run the ex mode command loop (entered via `Q`).
    ///
    /// Drops raw mode, presents a `:` prompt, reads and executes ex commands
    /// line by line, and returns when `:vi`/`:visual`/EOF is seen.
    fn run_ex_mode(&mut self) -> Result<(), Error> {
        use crate::command::parse_ex_command;
        use crate::command::ExCommand;
        use std::io::{BufRead, Write};

        self._raw_mode = None;
        print!("\r\n");
        let _ = std::io::stdout().flush();

        let stdin = std::io::stdin();
        loop {
            print!(":");
            let _ = std::io::stdout().flush();

            let mut line = String::new();
            match stdin.lock().read_line(&mut line) {
                Ok(0) | Err(_) => break, // EOF or I/O error → exit ex mode
                Ok(_) => {}
            }
            let input = line.trim_end_matches(['\n', '\r']);

            if input.is_empty() {
                if self.state.settings.autoprint {
                    self.print_current_line_ex();
                }
                continue;
            }

            let ctx = self.make_ex_context();
            match parse_ex_command(input, &ctx) {
                Ok(ExCommand::Visual) => break,
                Ok(cmd) => {
                    self.execute_ex_command(cmd);
                    // Commands like :sh, :a, :i, :c, :suspend restore raw mode
                    // internally. Drop it again so read_line() stays in cooked mode.
                    self._raw_mode = None;
                    let had_message = self.flush_ex_status();
                    if !had_message && self.state.settings.autoprint {
                        self.print_current_line_ex();
                    }
                }
                Err(e) if !e.is_empty() => println!("{}", e),
                Err(_) => {
                    if self.state.settings.autoprint {
                        self.print_current_line_ex();
                    }
                }
            }

            if self.state.should_quit {
                break;
            }
        }

        self._raw_mode = Some(RawMode::enable().map_err(Error::Terminal)?);
        self.state.status_message = None;
        Ok(())
    }

    /// Insert `lines` into the buffer at `row`, recording undo and updating state.
    fn insert_lines_at(&mut self, mut row: usize, lines: Vec<String>) {
        if lines.is_empty() {
            self.state.status_message = None;
            return;
        }
        let buf_len = self.document.buffer.len();
        // Clamp to valid insertion point
        row = row.min(buf_len);

        self.state
            .undo_history
            .begin_group(self.document.cursor, self.document.modified);

        for (i, line) in lines.iter().enumerate() {
            let insert_row = row + i;
            self.state.undo_history.record(EditAction::InsertLine {
                row: insert_row,
                content: line.clone(),
            });
            let _ = self.document.buffer.insert_line(insert_row, line.clone());
        }

        self.document.modified = true;

        // Leave cursor on last inserted line
        let last_row = (row + lines.len()).saturating_sub(1);
        self.document.cursor.row = last_row.min(self.document.buffer.len().saturating_sub(1));
        self.document.cursor.col = 0;
        self.state
            .viewport
            .ensure_visible(self.document.cursor.row, self.document.buffer.len());

        // Record cursor_after with the final position
        self.state.undo_history.end_group(self.document.cursor);
        self.state.status_message = None;
    }

    /// Delete lines `start..=end` then insert `new_lines` at `start`.
    ///
    /// Handles the edge case where `Buffer::remove_line` on the last remaining
    /// line clears it rather than removing it (leaving a phantom empty line).
    /// In that case, `ReplaceLine` is used for the last delete to keep undo
    /// consistent.
    fn execute_change_lines(&mut self, start: usize, end: usize, new_lines: Vec<String>) {
        let buf_len = self.document.buffer.len();
        if start >= buf_len {
            self.state.status_message = Some(StatusMessage::Error("Invalid range".to_string()));
            return;
        }
        let end = end.min(buf_len.saturating_sub(1));
        // True when deleting all lines — buffer will keep a phantom empty line.
        let deleting_all = end - start + 1 == buf_len;

        self.state
            .undo_history
            .begin_group(self.document.cursor, self.document.modified);

        // Delete the addressed lines in reverse order.
        // When deleting all lines, the very last remove_line call clears the
        // line instead of removing it, so record ReplaceLine for that row.
        for row in (start..=end).rev() {
            let content = self.document.buffer.line(row).unwrap_or("").to_string();
            if deleting_all && row == start {
                // Buffer will clear this line, not remove it; use ReplaceLine.
                let first_new = new_lines.first().cloned().unwrap_or_default();
                self.state.undo_history.record(EditAction::ReplaceLine {
                    row,
                    old_content: content,
                    new_content: first_new.clone(),
                });
                let _ = self.document.buffer.remove_line(row); // clears the line to ""
                                                               // Set the phantom empty line to the first replacement line
                if let Some(line_ref) = self.document.buffer.line_mut(row) {
                    *line_ref = first_new;
                }
                // Insert the rest of new_lines after row
                for (i, line) in new_lines.iter().enumerate().skip(1) {
                    let insert_row = row + i;
                    self.state.undo_history.record(EditAction::InsertLine {
                        row: insert_row,
                        content: line.clone(),
                    });
                    let _ = self.document.buffer.insert_line(insert_row, line.clone());
                }
            } else {
                self.state
                    .undo_history
                    .record(EditAction::RemoveLine { row, content });
                let _ = self.document.buffer.remove_line(row);
            }
        }

        // Insert replacement lines (non-deleting-all path only)
        if !deleting_all {
            for (i, line) in new_lines.iter().enumerate() {
                let insert_row = start + i;
                self.state.undo_history.record(EditAction::InsertLine {
                    row: insert_row,
                    content: line.clone(),
                });
                let _ = self.document.buffer.insert_line(insert_row, line.clone());
            }
        }

        self.document.modified = true;

        // Leave cursor on first changed line, clamped
        let new_buf_len = self.document.buffer.len();
        self.document.cursor.row = start.min(new_buf_len.saturating_sub(1));
        self.document.cursor.col = 0;
        self.state
            .viewport
            .ensure_visible(self.document.cursor.row, new_buf_len);

        // Record cursor_after with the final position
        self.state.undo_history.end_group(self.document.cursor);
        self.state.status_message = None;
    }

    fn execute_read_file(&mut self, filename: &str) {
        use crate::file::reader::read_file;
        use crate::undo::EditAction;

        let path = std::path::PathBuf::from(filename);
        let result = read_file(&path);
        match result {
            Ok(read_result) => {
                let insert_row = self.document.cursor.row + 1;
                let lines: Vec<String> =
                    read_result.content.lines().map(|l| l.to_string()).collect();
                let line_count = lines.len();

                self.state
                    .undo_history
                    .begin_group(self.document.cursor, self.document.modified);

                for (i, line) in lines.into_iter().enumerate() {
                    let row = insert_row + i;
                    let action = EditAction::InsertLine {
                        row,
                        content: line.clone(),
                    };
                    self.state.undo_history.record(action);
                    if let Err(e) = self.document.buffer.insert_line(row, line) {
                        self.state.status_message = Some(StatusMessage::Error(e.to_string()));
                        self.state.undo_history.end_group(self.document.cursor);
                        return;
                    }
                }

                self.document.modified = true;
                self.document.cursor.row = insert_row + line_count.saturating_sub(1);
                self.document.cursor.col = 0;
                self.state
                    .viewport
                    .ensure_visible(self.document.cursor.row, self.document.buffer.len());
                self.state.undo_history.end_group(self.document.cursor);
                self.state.status_message = Some(StatusMessage::Info(format!(
                    "\"{}\" {} lines",
                    filename, line_count
                )));
            }
            Err(e) => {
                self.state.status_message = Some(StatusMessage::Error(format!(
                    "E484: Can't open file {}: {}",
                    filename, e
                )));
            }
        }
    }

    /// Execute a parsed `:set` command.
    ///
    /// On success, sets a status message for query results.
    /// On failure, sets an error status message.
    fn execute_set_command(&mut self, cmd: SetCommand) {
        match self.state.settings.apply(&cmd) {
            Ok(SetResult::Message(msg)) => {
                self.state.status_message = Some(StatusMessage::Info(msg));
            }
            Ok(SetResult::Warning(msg)) => {
                self.state.status_message = Some(StatusMessage::Error(msg));
            }
            Ok(SetResult::Changed) => {
                // Sync scroll setting into runtime state.
                // scroll=0 means "use half viewport" (the default); any other
                // value overrides it explicitly.
                let s = self.state.settings.scroll;
                self.state.scroll_half_size = if s == 0 { None } else { Some(s) };
            }
            Err(e) => {
                self.state.status_message = Some(StatusMessage::Error(e.to_string()));
            }
        }
    }

    /// Resolve a `SubstituteRange` to a concrete `(start, end)` row range (0-based, inclusive).
    ///
    /// Returns `None` and sets an error status message if the range is invalid.
    fn resolve_line_range(&mut self, range: SubstituteRange) -> Option<(usize, usize)> {
        let buf_len = self.document.buffer.len();
        if buf_len == 0 {
            self.state.status_message = Some(StatusMessage::Error("Empty buffer".to_string()));
            return None;
        }
        let (start, end) = match range {
            SubstituteRange::CurrentLine => {
                let r = self.document.cursor.row;
                (r, r)
            }
            SubstituteRange::Line(n) => (n, n),
            SubstituteRange::Range { start, end } => (start, end),
            SubstituteRange::WholeFile => (0, buf_len - 1),
        };
        if end >= buf_len {
            self.state.status_message = Some(StatusMessage::Error("Invalid range".to_string()));
            return None;
        }
        Some((start, end))
    }

    /// Execute `:[range]d [register]` — delete lines.
    fn execute_delete_lines(&mut self, range: SubstituteRange, register: Option<char>) {
        let Some((start, end)) = self.resolve_line_range(range) else {
            return;
        };

        // Collect the lines to delete
        let lines: Vec<String> = (start..=end)
            .filter_map(|r| self.document.buffer.line(r).map(str::to_string))
            .collect();
        if lines.is_empty() {
            return;
        }
        let count = lines.len();

        // Store deleted content into register
        let text = lines.join("\n") + "\n";
        let content = RegisterContent::linewise(text);
        let reg_id = register.and_then(RegisterId::parse);
        self.state.registers.delete(reg_id, register, content);

        // Record undo and delete in reverse order
        self.state
            .undo_history
            .begin_group(self.document.cursor, self.document.modified);
        for row in (start..=end).rev() {
            let line_content = self.document.buffer.line(row).unwrap_or("").to_string();
            self.state.undo_history.record(EditAction::RemoveLine {
                row,
                content: line_content,
            });
            let _ = self.document.buffer.remove_line(row);
        }
        self.state.undo_history.end_group(self.document.cursor);
        self.document.modified = true;

        // Move cursor to first line of deleted range, clamped
        let buf_len = self.document.buffer.len();
        self.document.cursor.row = start.min(buf_len.saturating_sub(1));
        self.document.cursor.col = 0;
        self.state
            .viewport
            .ensure_visible(self.document.cursor.row, buf_len);
        self.report_lines(count, "lines deleted");
    }

    /// Execute `:[range]y [register]` — yank lines into register.
    fn execute_yank_lines(&mut self, range: SubstituteRange, register: Option<char>) {
        let Some((start, end)) = self.resolve_line_range(range) else {
            return;
        };

        let lines: Vec<String> = (start..=end)
            .filter_map(|r| self.document.buffer.line(r).map(str::to_string))
            .collect();
        if lines.is_empty() {
            return;
        }
        let count = lines.len();
        let text = lines.join("\n") + "\n";
        let content = RegisterContent::linewise(text);
        let reg_id = register.and_then(RegisterId::parse);
        self.state.registers.yank(reg_id, content);
        self.report_lines(count, "lines yanked");
    }

    /// Execute `:[addr]pu[t] [x]` — put register contents after the addressed line.
    ///
    /// `after_row` is the 0-based row to insert after. The register content is always
    /// placed as whole lines (POSIX behavior). Character-wise content is treated as a
    /// single line.
    fn execute_put_lines(&mut self, after_row: usize, register: Option<char>) {
        let reg_id = register.and_then(RegisterId::parse);
        let content = self.state.registers.get_owned(reg_id);
        let Some(content) = content else {
            return;
        };

        let insert_row = (after_row + 1).min(self.document.buffer.len());
        let text = content.text();
        let lines: Vec<&str> = if content.is_linewise() {
            // Strip trailing newline that is part of the linewise sentinel
            let trimmed = text.trim_end_matches('\n');
            trimmed.lines().collect()
        } else {
            // Character-wise: treat the entire content as a single line
            text.lines().collect()
        };

        if lines.is_empty() {
            return;
        }

        self.state
            .undo_history
            .begin_group(self.document.cursor, self.document.modified);

        for (i, line) in lines.iter().enumerate() {
            let action = EditAction::InsertLine {
                row: insert_row + i,
                content: line.to_string(),
            };
            self.state.undo_history.record(action);
            let _ = self
                .document
                .buffer
                .insert_line(insert_row + i, line.to_string());
        }

        // Move cursor to the first inserted line before ending the undo group
        self.document.cursor.row = insert_row;
        self.document.cursor.col = 0;
        self.move_to_first_non_blank();
        self.state.undo_history.end_group(self.document.cursor);
        self.document.modified = true;
    }

    /// Execute `:[range]m {dest}` — move lines to after dest.
    ///
    /// `dest` is the 0-based insertion index: lines are inserted *before* row `dest`.
    /// A `dest` of 0 inserts before the first line; `dest` equal to buffer length
    /// appends after the last line.
    fn execute_move_lines(&mut self, range: SubstituteRange, dest: usize) {
        let Some((start, end)) = self.resolve_line_range(range) else {
            return;
        };
        let buf_len = self.document.buffer.len();
        // dest is 1-based; convert to 0-based insertion point
        // dest=0: insert before row 0; dest=N: insert after row N-1 (i.e., before row N)
        let insert_at = dest; // 0-based: insert before this index
        if insert_at > buf_len || (insert_at > start && insert_at <= end + 1) {
            self.state.status_message = Some(StatusMessage::Error("Invalid address".to_string()));
            return;
        }

        let count = end - start + 1;
        // Collect lines
        let lines: Vec<String> = (start..=end)
            .filter_map(|r| self.document.buffer.line(r).map(str::to_string))
            .collect();

        self.state
            .undo_history
            .begin_group(self.document.cursor, self.document.modified);

        // Remove lines in reverse order, adjusting target if needed
        for row in (start..=end).rev() {
            let line_content = self.document.buffer.line(row).unwrap_or("").to_string();
            self.state.undo_history.record(EditAction::RemoveLine {
                row,
                content: line_content,
            });
            let _ = self.document.buffer.remove_line(row);
        }

        // Adjust insert_at for removed lines
        let adjusted_insert = if insert_at > end {
            insert_at - count
        } else {
            insert_at
        };

        // Insert lines at the adjusted position
        for (i, line) in lines.iter().enumerate() {
            self.state.undo_history.record(EditAction::InsertLine {
                row: adjusted_insert + i,
                content: line.clone(),
            });
            let _ = self
                .document
                .buffer
                .insert_line(adjusted_insert + i, line.clone());
        }

        self.state.undo_history.end_group(self.document.cursor);
        self.document.modified = true;

        let new_buf_len = self.document.buffer.len();
        let last_inserted = (adjusted_insert + count - 1).min(new_buf_len.saturating_sub(1));
        self.document.cursor.row = last_inserted;
        self.document.cursor.col = 0;
        self.state
            .viewport
            .ensure_visible(self.document.cursor.row, new_buf_len);
        self.report_lines(count, "lines moved");
    }

    /// Execute `:[range]t {dest}` — copy lines to after dest.
    fn execute_copy_lines(&mut self, range: SubstituteRange, dest: usize) {
        let Some((start, end)) = self.resolve_line_range(range) else {
            return;
        };
        let buf_len = self.document.buffer.len();
        let insert_at = dest; // 0-based: insert before this index

        if insert_at > buf_len {
            self.state.status_message = Some(StatusMessage::Error("Invalid address".to_string()));
            return;
        }

        let count = end - start + 1;
        let lines: Vec<String> = (start..=end)
            .filter_map(|r| self.document.buffer.line(r).map(str::to_string))
            .collect();

        self.state
            .undo_history
            .begin_group(self.document.cursor, self.document.modified);

        // If inserting after the source range, no adjustment needed
        for (i, line) in lines.iter().enumerate() {
            let row = insert_at + i;
            self.state.undo_history.record(EditAction::InsertLine {
                row,
                content: line.clone(),
            });
            let _ = self.document.buffer.insert_line(row, line.clone());
        }

        self.state.undo_history.end_group(self.document.cursor);
        self.document.modified = true;

        let new_buf_len = self.document.buffer.len();
        let last_inserted = (insert_at + count - 1).min(new_buf_len.saturating_sub(1));
        self.document.cursor.row = last_inserted;
        self.document.cursor.col = 0;
        self.state
            .viewport
            .ensure_visible(self.document.cursor.row, new_buf_len);
        self.report_lines(count, "lines copied");
    }

    /// Execute `:[range]j` — join lines in range.
    fn execute_join_lines_range(&mut self, range: SubstituteRange) {
        let Some((start, end)) = self.resolve_line_range(range) else {
            return;
        };

        if start == end {
            // Single line: join with next line if exists
            let buf_len = self.document.buffer.len();
            if start + 1 >= buf_len {
                return; // nothing to join
            }
        }

        self.state
            .undo_history
            .begin_group(self.document.cursor, self.document.modified);

        // Join lines from start to end: repeatedly join line `start` with `start+1`.
        // Single-line range (start == end) joins with the next line (1 join needed).
        let joins = if start == end { 1 } else { end - start };
        let mut joined = 0;
        for _ in 0..joins {
            let first = self.document.buffer.line(start).unwrap_or("").to_string();
            let second = self.document.buffer.line(start + 1);
            if second.is_none() {
                break;
            }
            // Record the join col = length of first line (after stripping trailing ws)
            let join_col = first.trim_end().len();
            self.state.undo_history.record(EditAction::JoinLines {
                row: start,
                col: join_col,
            });

            // Perform the join
            let trimmed_second = self
                .document
                .buffer
                .line(start + 1)
                .unwrap_or("")
                .trim_start()
                .to_string();
            if let Some(line) = self.document.buffer.line_mut(start) {
                let trimmed_first = line.trim_end().to_string();
                line.clear();
                if trimmed_second.is_empty() {
                    line.push_str(&trimmed_first);
                } else {
                    line.push_str(&trimmed_first);
                    line.push(' ');
                    line.push_str(&trimmed_second);
                }
            }
            let _ = self.document.buffer.remove_line(start + 1);
            joined += 1;
        }

        self.state.undo_history.end_group(self.document.cursor);
        if joined > 0 {
            self.document.modified = true;
            self.document.cursor.row = start;
            self.document.cursor.col = 0;
            let buf_len = self.document.buffer.len();
            self.state
                .viewport
                .ensure_visible(self.document.cursor.row, buf_len);
        }
    }

    /// Execute a substitute command across the specified line range.
    ///
    /// Compiles the pattern (using the last search pattern if empty),
    /// iterates over lines in the range, and replaces matches. The
    /// `ignorecase` setting is merged with any explicit `/i` flag from
    /// the substitute command using logical OR. Records `ReplaceLine`
    /// undo actions so the entire substitution is one undoable unit.
    /// Sets a status message with the result count.
    fn execute_substitute(&mut self, cmd: SubstituteCommand) {
        // Resolve empty pattern to last search pattern
        let pattern = if cmd.pattern.is_empty() {
            match self.state.search.last_pattern() {
                Some(p) => p.to_string(),
                None => {
                    self.state.status_message =
                        Some(StatusMessage::Error("No previous pattern".to_string()));
                    return;
                }
            }
        } else {
            cmd.pattern.clone()
        };

        // Merge explicit 'i' flag with the ignorecase setting
        let case_insensitive = cmd.flags.case_insensitive || self.state.settings.ignorecase;

        // Compile regex
        let magic = self.state.settings.magic;
        let regex = match ViRegex::compile_with_magic(&pattern, case_insensitive, magic) {
            Ok(r) => r,
            Err(e) => {
                self.state.status_message = Some(StatusMessage::Error(e.to_string()));
                return;
            }
        };

        // Determine line range (0-based, inclusive)
        let (start_line, end_line) = match cmd.range {
            SubstituteRange::CurrentLine => {
                let row = self.document.cursor.row;
                (row, row)
            }
            SubstituteRange::Line(n) => (n, n),
            SubstituteRange::Range { start, end } => (start, end),
            SubstituteRange::WholeFile => (0, self.document.buffer.len().saturating_sub(1)),
        };

        // Validate range against buffer
        if end_line >= self.document.buffer.len() {
            self.state.status_message = Some(StatusMessage::Error("Invalid range".to_string()));
            return;
        }

        // Begin undo group
        self.state
            .undo_history
            .begin_group(self.document.cursor, self.document.modified);

        let flags = cmd.flags;
        let replacement_str = cmd.replacement.clone();

        let result = if flags.confirm {
            self.execute_substitute_confirm(
                &regex,
                &cmd.replacement,
                flags.global,
                start_line,
                end_line,
            )
        } else {
            self.execute_substitute_lines(
                &regex,
                &cmd.replacement,
                cmd.flags.global,
                start_line,
                end_line,
            )
        };

        // End undo group
        self.state.undo_history.end_group(self.document.cursor);

        match result {
            Err(e) => {
                self.state.status_message = Some(StatusMessage::Error(e));
            }
            Ok((total_substitutions, lines_changed)) => {
                if total_substitutions == 0 {
                    self.state.status_message =
                        Some(StatusMessage::Error("Pattern not found".to_string()));
                } else {
                    self.document.modified = true;
                    if lines_changed > 1 {
                        self.state.status_message = Some(StatusMessage::Info(format!(
                            "{} substitutions on {} lines",
                            total_substitutions, lines_changed
                        )));
                    } else {
                        self.state.status_message = Some(StatusMessage::Info(format!(
                            "{} substitution(s) on 1 line",
                            total_substitutions
                        )));
                    }

                    // Update last search pattern so n/N can reuse it.
                    let _ = self
                        .state
                        .search
                        .set_pattern(&pattern, crate::search::SearchDirection::Forward);

                    // Track last replacement for :~ (repeat with current search pattern).
                    self.state.substitute_repeat.last_replacement = Some(replacement_str.clone());

                    // Print flags: print the last substituted line (cursor row after substitution).
                    if flags.print || flags.list || flags.number {
                        let row = self.document.cursor.row;
                        if let Some(line) = self.document.buffer.line(row).map(str::to_string) {
                            let printed = if flags.list {
                                let listed = line.replace('\t', "^I");
                                format!("{}$", listed)
                            } else if flags.number {
                                format!("{:6}  {}", row + 1, line)
                            } else {
                                line
                            };
                            // Append printed line as an extra info message; in ex-mode output
                            // it would go to stdout, here we show it in the status line.
                            self.state.status_message = Some(StatusMessage::Info(printed));
                        }
                    }
                }
            }
        }
    }

    /// Apply substitute to lines `start_line..=end_line` without managing an undo group.
    ///
    /// Returns `Ok((total_substitutions, lines_changed))` on success, or
    /// `Err(message)` if an error occurs on a line (partial work already recorded).
    fn execute_substitute_lines(
        &mut self,
        regex: &ViRegex,
        replacement: &str,
        global_flag: bool,
        start_line: usize,
        end_line: usize,
    ) -> Result<(usize, usize), String> {
        let mut total_substitutions = 0usize;
        let mut lines_changed = 0usize;

        for row in start_line..=end_line {
            let old_content = match self.document.buffer.line(row) {
                Some(l) => l.to_string(),
                None => continue,
            };

            match substitute_line(&old_content, regex, replacement, global_flag) {
                Ok(Some((new_content, match_count))) => {
                    let action = EditAction::ReplaceLine {
                        row,
                        old_content: old_content.clone(),
                        new_content: new_content.clone(),
                    };
                    self.state.undo_history.record(action);

                    if let Some(line) = self.document.buffer.line_mut(row) {
                        line.clear();
                        line.push_str(&new_content);
                    }

                    total_substitutions += match_count;
                    lines_changed += 1;
                }
                Ok(None) => {}
                Err(e) => return Err(e.to_string()),
            }
        }

        Ok((total_substitutions, lines_changed))
    }

    /// Perform interactive confirm-substitution across `start_line..=end_line`.
    ///
    /// For each match found on each line, temporarily leaves raw mode, prints
    /// the line with the matched text highlighted and a prompt of the form:
    ///
    /// ```text
    ///   line content
    ///   ^^^^^^^^^^^^
    ///   replace with "replacement"? [y/n/a/q/l]
    /// ```
    ///
    /// Then reads a single keypress:
    /// - `y` — replace this match and continue
    /// - `n` — skip this match and continue
    /// - `a` — replace this and all remaining matches without further prompting
    /// - `q` / `Esc` — stop substituting, keep changes made so far
    /// - `l` — replace this match then stop
    ///
    /// Returns `Ok((total_substitutions, lines_changed))`.
    fn execute_substitute_confirm(
        &mut self,
        regex: &ViRegex,
        replacement: &str,
        global: bool,
        start_line: usize,
        end_line: usize,
    ) -> Result<(usize, usize), String> {
        use crate::buffer::unicode::next_char_boundary;
        use crate::search::substitute::expand_replacement;
        use std::io::Write;

        let mut total_substitutions = 0usize;
        let mut lines_changed = 0usize;
        let mut accept_all = false;
        let mut done = false;

        for row in start_line..=end_line {
            if done {
                break;
            }

            let old_content = match self.document.buffer.line(row) {
                Some(l) => l.to_string(),
                None => continue,
            };

            // Collect all matches on this line first
            let mut matches: Vec<(usize, usize, String)> = Vec::new(); // (start, end, expanded)
            for cap_result in regex.captures_iter(&old_content) {
                let caps = cap_result.map_err(|e| e.to_string())?;
                let whole = caps.get(0).unwrap();
                let start = whole.start();
                let end = whole.end();
                let expanded = expand_replacement(replacement, &caps);
                matches.push((start, end, expanded));
                if !global {
                    break;
                }
            }

            if matches.is_empty() {
                continue;
            }

            // Process matches in reverse so byte offsets stay valid as we mutate
            // the line. But for prompting we go forward. We'll build the new line
            // incrementally, confirming each match.
            let mut confirmed: Vec<bool> = vec![false; matches.len()];

            if !accept_all {
                for (i, (mstart, mend, expanded)) in matches.iter().enumerate() {
                    // Drop raw mode to print prompt
                    self._raw_mode = None;

                    print!("\r\n");
                    // Print the line with a caret indicator under the match
                    print!("{}\r\n", old_content);
                    let prefix_display = (*mstart).min(old_content.len());
                    let caret_pad = " ".repeat(prefix_display);
                    let match_len = mend.saturating_sub(*mstart).max(1);
                    let carets = "^".repeat(match_len);
                    print!("{}{}\r\n", caret_pad, carets);
                    print!("replace with \"{}\"? [y/n/a/q/l] ", expanded);
                    let _ = std::io::stdout().flush();

                    // Restore raw mode to read a single keypress
                    self._raw_mode = Some(RawMode::enable().map_err(|e| e.to_string())?);

                    let key = read_key().map_err(|e| e.to_string())?;

                    match key {
                        Key::Char('y') => {
                            confirmed[i] = true;
                        }
                        Key::Char('n') => {
                            // skip
                        }
                        Key::Char('a') => {
                            confirmed[i] = true;
                            accept_all = true;
                            break;
                        }
                        Key::Char('l') => {
                            confirmed[i] = true;
                            done = true;
                            break;
                        }
                        Key::Char('q') | Key::Esc => {
                            done = true;
                            break;
                        }
                        _ => {
                            // treat any other key as 'n'
                        }
                    }
                }
            } else {
                // accept_all: confirm all remaining matches on this line
                for c in confirmed.iter_mut() {
                    *c = true;
                }
            }

            // If in accept_all mode, mark remaining unconfirmed matches too
            if accept_all {
                for c in confirmed.iter_mut() {
                    *c = true;
                }
            }

            // Now build the new line by applying confirmed substitutions
            let any_confirmed = confirmed.iter().any(|&c| c);
            if !any_confirmed {
                continue;
            }

            let mut new_line = String::with_capacity(old_content.len());
            let mut last_end = 0usize;
            let mut sub_count = 0usize;

            for (i, (mstart, mend, expanded)) in matches.iter().enumerate() {
                if *mstart < last_end {
                    // Overlapping match (shouldn't happen with non-overlapping regex); skip
                    continue;
                }
                // Append text between last match end and this match start
                new_line.push_str(&old_content[last_end..*mstart]);

                if confirmed[i] {
                    new_line.push_str(expanded);
                    sub_count += 1;
                } else {
                    // Not replacing: keep the original matched text
                    new_line.push_str(&old_content[*mstart..*mend]);
                }
                last_end = *mend;

                // Advance past zero-length match to prevent infinite loops
                if mstart == mend && *mstart < old_content.len() {
                    let nb = next_char_boundary(&old_content, *mstart);
                    new_line.push_str(&old_content[*mend..nb]);
                    last_end = nb;
                }
            }
            new_line.push_str(&old_content[last_end..]);

            if sub_count > 0 {
                let action = EditAction::ReplaceLine {
                    row,
                    old_content: old_content.clone(),
                    new_content: new_line.clone(),
                };
                self.state.undo_history.record(action);
                if let Some(line) = self.document.buffer.line_mut(row) {
                    line.clear();
                    line.push_str(&new_line);
                }
                self.document.cursor.row = row;
                total_substitutions += sub_count;
                lines_changed += 1;
            }
        }

        // Force a full redraw after confirm interaction
        self.state.status_message = None;
        Ok((total_substitutions, lines_changed))
    }

    /// Execute `:r !{cmd}` -- read shell command output into buffer.
    fn execute_read_shell_command(&mut self, cmd: &str) {
        use std::io::Write;
        use std::process::{Command, Stdio};

        // Drop raw mode for shell interaction
        self._raw_mode = None;
        print!("\r\n");
        let _ = std::io::stdout().flush();

        let output = Command::new(self.shell_path())
            .arg("-c")
            .arg(cmd)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .output();

        // Restore raw mode
        match crate::terminal::RawMode::enable() {
            Ok(rm) => self._raw_mode = Some(rm),
            Err(e) => {
                self.state.status_message = Some(StatusMessage::Error(format!(
                    "Failed to restore terminal: {}",
                    e
                )));
                return;
            }
        }

        match output {
            Ok(out) => {
                let text = String::from_utf8_lossy(&out.stdout);
                let insert_row = self.document.cursor.row + 1;
                let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
                let line_count = lines.len();

                self.state
                    .undo_history
                    .begin_group(self.document.cursor, self.document.modified);

                for (i, line) in lines.into_iter().enumerate() {
                    let row = insert_row + i;
                    let action = crate::undo::EditAction::InsertLine {
                        row,
                        content: line.clone(),
                    };
                    self.state.undo_history.record(action);
                    if let Err(e) = self.document.buffer.insert_line(row, line) {
                        self.state.status_message = Some(StatusMessage::Error(e.to_string()));
                        self.state.undo_history.end_group(self.document.cursor);
                        return;
                    }
                }

                self.document.modified = true;
                self.document.cursor.row = insert_row + line_count.saturating_sub(1);
                self.document.cursor.col = 0;
                self.state
                    .viewport
                    .ensure_visible(self.document.cursor.row, self.document.buffer.len());
                self.state.undo_history.end_group(self.document.cursor);
                self.state.status_message = Some(StatusMessage::Info(format!(
                    "!{} {} lines",
                    cmd, line_count
                )));
            }
            Err(e) => {
                self.state.status_message = Some(StatusMessage::Error(format!(
                    "Error running command: {}",
                    e
                )));
            }
        }
    }

    /// Execute `:[range]p` and `:[range]nu`/`:[range]#`.
    ///
    /// Formats each line with an optional line number prefix, sets the status
    /// message, and moves the cursor to the last line of the range (POSIX).
    fn execute_print_impl(&mut self, range: SubstituteRange, numbered: bool) {
        let Some((start, end)) = self.resolve_line_range(range) else {
            return;
        };
        let lines: Vec<String> = (start..=end)
            .filter_map(|r| {
                self.document.buffer.line(r).map(|l| {
                    if numbered {
                        format!("{:6}\t{}", r + 1, l)
                    } else {
                        l.to_string()
                    }
                })
            })
            .collect();
        if lines.is_empty() {
            return;
        }
        self.state.status_message = Some(StatusMessage::Info(lines.join("\n")));
        // POSIX: cursor moves to last line of range.
        self.document.cursor.row = end;
        self.document.cursor.col = 0;
    }

    /// Format a line for list display: tabs become `^I`, `$` appended at end.
    fn format_list_line(line: &str) -> String {
        let mut s = line.replace('\t', "^I");
        s.push('$');
        s
    }

    /// Set a status message reporting `count` changed lines if `count` meets
    /// or exceeds the `report` threshold. Suppresses the message otherwise.
    fn report_lines(&mut self, count: usize, noun: &str) {
        if !self.state.settings.terse && count >= self.state.settings.report {
            self.state.status_message = Some(StatusMessage::Info(format!("{} {}", count, noun)));
        }
    }

    fn execute_print_lines(&mut self, range: SubstituteRange) {
        self.execute_print_impl(range, false);
    }

    fn execute_print_numbered_lines(&mut self, range: SubstituteRange) {
        self.execute_print_impl(range, true);
    }

    fn execute_print_list_lines(&mut self, range: SubstituteRange) {
        let Some((start, end)) = self.resolve_line_range(range) else {
            return;
        };
        let lines: Vec<String> = (start..=end)
            .filter_map(|r| self.document.buffer.line(r).map(Self::format_list_line))
            .collect();
        if lines.is_empty() {
            return;
        }
        self.state.status_message = Some(StatusMessage::Info(lines.join("\n")));
        // POSIX: cursor moves to last line of range.
        self.document.cursor.row = end;
        self.document.cursor.col = 0;
    }

    /// Execute `:map lhs rhs`.
    fn execute_map(&mut self, insert_mode: bool, lhs: &str, rhs: &str) {
        let lhs_keys = parse_key_notation(lhs);
        let rhs_keys = parse_key_notation(rhs);
        let maps = if insert_mode {
            &mut self.state.mappings.insert_maps
        } else {
            &mut self.state.mappings.normal_maps
        };
        // Remove existing mapping for this lhs
        maps.retain(|(k, _)| *k != lhs_keys);
        maps.push((lhs_keys, rhs_keys));
    }

    /// Execute `:unmap lhs`.
    fn execute_unmap(&mut self, insert_mode: bool, lhs: &str) {
        let lhs_keys = parse_key_notation(lhs);
        let maps = if insert_mode {
            &mut self.state.mappings.insert_maps
        } else {
            &mut self.state.mappings.normal_maps
        };
        let before = maps.len();
        maps.retain(|(k, _)| *k != lhs_keys);
        if maps.len() == before {
            self.state.status_message =
                Some(StatusMessage::Error("E31: No such mapping".to_string()));
        }
    }

    /// Execute bare `:map` -- show all mappings.
    fn execute_show_maps(&mut self, insert_mode: bool) {
        let maps = if insert_mode {
            &self.state.mappings.insert_maps
        } else {
            &self.state.mappings.normal_maps
        };
        if maps.is_empty() {
            self.state.status_message = Some(StatusMessage::Error("No mappings".to_string()));
        } else {
            let display: Vec<String> = maps
                .iter()
                .map(|(lhs, rhs)| format!("{} -> {}", keys_to_notation(lhs), keys_to_notation(rhs)))
                .collect();
            self.state.status_message = Some(StatusMessage::Info(display.join("  ")));
        }
    }

    /// Execute `:abbreviate lhs rhs`.
    fn execute_abbreviate(&mut self, lhs: &str, rhs: &str) {
        self.state.mappings.abbreviations.retain(|(k, _)| k != lhs);
        self.state
            .mappings
            .abbreviations
            .push((lhs.to_string(), rhs.to_string()));
    }

    /// Execute `:unabbreviate lhs`.
    fn execute_unabbreviate(&mut self, lhs: &str) {
        let before = self.state.mappings.abbreviations.len();
        self.state.mappings.abbreviations.retain(|(k, _)| k != lhs);
        if self.state.mappings.abbreviations.len() == before {
            self.state.status_message = Some(StatusMessage::Error(
                "E31: No such abbreviation".to_string(),
            ));
        }
    }

    /// Execute bare `:abbreviate` — show all abbreviations.
    fn execute_show_abbreviations(&mut self) {
        if self.state.mappings.abbreviations.is_empty() {
            self.state.status_message = Some(StatusMessage::Error("No abbreviations".to_string()));
        } else {
            let display: Vec<String> = self
                .state
                .mappings
                .abbreviations
                .iter()
                .map(|(lhs, rhs)| format!("{} -> {}", lhs, rhs))
                .collect();
            self.state.status_message = Some(StatusMessage::Info(display.join("  ")));
        }
    }

    /// Execute the filter operator: pipe lines through shell command.
    fn execute_filter(&mut self, start_row: usize, end_row: usize, cmd: &str) {
        use std::io::Write;
        use std::process::{Command, Stdio};

        // Collect the lines to filter
        let lines: Vec<String> = (start_row..=end_row)
            .filter_map(|r| self.document.buffer.line(r).map(str::to_string))
            .collect();
        let input_text = lines.join("\n") + "\n";

        // Drop raw mode for shell interaction
        self._raw_mode = None;
        print!("\r\n");
        let _ = std::io::stdout().flush();

        let child = Command::new(self.shell_path())
            .arg("-c")
            .arg(cmd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn();

        let output = match child {
            Ok(mut child) => {
                if let Some(mut stdin) = child.stdin.take() {
                    let _ = stdin.write_all(input_text.as_bytes());
                }
                child.wait_with_output()
            }
            Err(e) => {
                // Restore raw mode
                self._raw_mode = crate::terminal::RawMode::enable().ok();
                self.state.status_message = Some(StatusMessage::Error(format!(
                    "Error running command: {}",
                    e
                )));
                return;
            }
        };

        // Restore raw mode
        match crate::terminal::RawMode::enable() {
            Ok(rm) => self._raw_mode = Some(rm),
            Err(e) => {
                self.state.status_message = Some(StatusMessage::Error(format!(
                    "Failed to restore terminal: {}",
                    e
                )));
                return;
            }
        }

        match output {
            Ok(out) => {
                let text = String::from_utf8_lossy(&out.stdout);
                let new_lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();

                self.state
                    .undo_history
                    .begin_group(self.document.cursor, self.document.modified);

                // Remove old lines in reverse order
                for row in (start_row..=end_row).rev() {
                    let line_content = self.document.buffer.line(row).unwrap_or("").to_string();
                    self.state
                        .undo_history
                        .record(crate::undo::EditAction::RemoveLine {
                            row,
                            content: line_content,
                        });
                    let _ = self.document.buffer.remove_line(row);
                }

                // Insert new lines
                for (i, line) in new_lines.iter().enumerate() {
                    let row = start_row + i;
                    self.state
                        .undo_history
                        .record(crate::undo::EditAction::InsertLine {
                            row,
                            content: line.clone(),
                        });
                    let _ = self.document.buffer.insert_line(row, line.clone());
                }

                self.state.undo_history.end_group(self.document.cursor);
                self.document.modified = true;
                self.document.cursor.row = start_row;
                self.document.cursor.col = 0;
                let buf_len = self.document.buffer.len();
                self.state
                    .viewport
                    .ensure_visible(self.document.cursor.row, buf_len);
                let line_diff = new_lines.len() as isize - (end_row - start_row + 1) as isize;
                self.report_lines(new_lines.len(), "lines filtered");
                let _ = line_diff;
            }
            Err(e) => {
                self.state.status_message = Some(StatusMessage::Error(format!(
                    "Error running command: {}",
                    e
                )));
            }
        }
    }

    /// Run the main editor event loop.
    pub fn run(&mut self) -> Result<(), Error> {
        // Install SIGWINCH handler so resize detection is event-driven
        install_sigwinch_handler().map_err(Error::Terminal)?;

        // Initial render
        self.render()?;

        loop {
            // Check for terminal resize before reading key
            self.check_resize()?;

            let key = read_key()?;
            self.handle_key(key)?;

            if self.state.should_quit {
                break;
            }

            // Ring BEL once if errorbells is on and a status message was set.
            if self.state.settings.errorbells
                && self
                    .state
                    .status_message
                    .as_ref()
                    .is_some_and(|m| m.is_error())
            {
                self.state.bell_pending = true;
            }

            self.render()?;
        }

        // Clear screen on exit
        self.renderer.clear_screen()?;

        Ok(())
    }

    /// Check if the terminal was resized and update accordingly.
    ///
    /// Returns immediately without a syscall if no SIGWINCH has been received.
    fn check_resize(&mut self) -> Result<(), Error> {
        if !sigwinch_received() {
            return Ok(());
        }
        let new_size = terminal_size()?;
        let current_height = self.state.viewport.height();
        let current_width = self.state.viewport.width();

        // Reserve one line for status line
        let new_height = (new_size.rows as usize).saturating_sub(1);
        let new_width = new_size.cols as usize;

        if new_height != current_height || new_width != current_width {
            // Terminal was resized - update viewport and renderer
            self.state.viewport.set_height(new_height);
            self.state.viewport.set_width(new_width);
            self.renderer.handle_resize(new_size);

            // Ensure cursor is still visible after resize
            self.state
                .viewport
                .ensure_visible(self.document.cursor.row, self.document.buffer.len());
        }

        Ok(())
    }

    /// Render the editor display.
    ///
    /// Renders either the command line, a status message, or the normal
    /// status line at the bottom of the screen, depending on the current
    /// editor state. When `hlsearch` is enabled and a search pattern exists,
    /// all matches in the visible viewport are highlighted in reverse video.
    /// When `number` is enabled, a line number gutter is displayed.
    /// When `wrap` is enabled, long lines are soft-wrapped across multiple rows.
    fn render(&mut self) -> Result<(), Error> {
        // Emit BEL before rendering if errorbells fired this cycle.
        if self.state.bell_pending {
            self.state.bell_pending = false;
            self.renderer.ring_bell()?;
        }

        // Sync viewport from state to renderer before rendering
        *self.renderer.viewport_mut() = self.state.viewport.clone();

        self.renderer.render_full(
            crate::render::RenderContext {
                buffer: &self.document.buffer,
                cursor: &self.document.cursor,
                mode: self.state.mode.as_str(),
                filename: self.document.filename.as_deref(),
                modified: self.document.modified,
                line_ending: &self.document.line_ending,
            },
            crate::render::RenderOptions {
                command_line: if self.state.mode == Mode::CommandLine {
                    Some(&self.state.command_line)
                } else {
                    None
                },
                status_message: self.state.status_message.as_ref().map(|m| match m {
                    StatusMessage::Info(s) | StatusMessage::Error(s) => {
                        crate::render::StatusDisplay::Text(s)
                    }
                    StatusMessage::WithPath { path, suffix } => {
                        crate::render::StatusDisplay::WithPath { path, suffix }
                    }
                }),
                search: crate::render::SearchDisplay::new(
                    self.state.search.compiled(),
                    self.state.settings.hlsearch && !self.state.search.suppress_highlights,
                ),
                tabstop: self.state.settings.tabstop,
                number: self.state.settings.number,
                wrap: self.state.settings.wrap,
                list: self.state.settings.list,
                selection: self.document.selection.as_ref(),
            },
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod exinit_tests {
    use super::*;

    /// Helper: build an editor, apply `EXINIT` value, return it.
    ///
    /// Calls `apply_exinit_value` directly so tests exercise the real parsing
    /// and dispatch path without touching the process environment.
    fn editor_with_exinit(value: &str) -> Editor {
        let mut editor = Editor::for_testing("");
        editor.apply_exinit_value(value);
        editor
    }

    #[test]
    fn test_exinit_set_option() {
        let editor = editor_with_exinit("set number");
        assert!(editor.state.settings.number, "number should be enabled");
    }

    #[test]
    fn test_exinit_shell_command_blocked() {
        let editor = editor_with_exinit("!echo hi");
        let msg = editor
            .state
            .status_message
            .as_ref()
            .map(|m| m.as_str_formatted())
            .unwrap_or_default();
        assert!(
            msg.contains("not allowed"),
            "expected 'not allowed' in status, got: {msg:?}"
        );
    }

    #[test]
    fn test_exinit_sh_blocked() {
        let editor = editor_with_exinit("sh");
        let msg = editor
            .state
            .status_message
            .as_ref()
            .map(|m| m.as_str_formatted())
            .unwrap_or_default();
        assert!(
            msg.contains("not allowed"),
            "expected 'not allowed' in status, got: {msg:?}"
        );
    }

    #[test]
    fn test_exinit_read_shell_blocked() {
        let editor = editor_with_exinit("r !echo hi");
        let msg = editor
            .state
            .status_message
            .as_ref()
            .map(|m| m.as_str_formatted())
            .unwrap_or_default();
        assert!(
            msg.contains("not allowed"),
            "expected 'not allowed' in status, got: {msg:?}"
        );
    }

    #[test]
    fn test_exinit_source_blocked() {
        let editor = editor_with_exinit("source /tmp/foo");
        let msg = editor
            .state
            .status_message
            .as_ref()
            .map(|m| m.as_str_formatted())
            .unwrap_or_default();
        assert!(
            msg.contains("not allowed"),
            "expected 'not allowed' in status, got: {msg:?}"
        );
    }

    #[test]
    fn test_exinit_multiple_commands() {
        let editor = editor_with_exinit("set number|set ignorecase");
        assert!(editor.state.settings.number, "number should be enabled");
        assert!(
            editor.state.settings.ignorecase,
            "ignorecase should be enabled"
        );
    }

    #[test]
    fn test_exinit_comment_skipped() {
        // A segment starting with `"` is a comment and must not produce an error.
        let editor = editor_with_exinit("\" this is a comment");
        assert!(
            editor.state.status_message.is_none(),
            "comment should produce no status message"
        );
    }

    #[test]
    fn test_exinit_empty_value() {
        // Calling load_exinit on an empty string should be a no-op.
        let editor = editor_with_exinit("");
        assert!(editor.state.status_message.is_none());
    }

    // =========================================================================
    // Ex mode helpers
    // =========================================================================

    #[test]
    fn test_print_current_line_ex_empty_buffer() {
        // Empty buffer: line(0) returns Some("") so println! prints an empty line.
        // We just verify it doesn't panic.
        let editor = Editor::for_testing("");
        editor.print_current_line_ex();
    }

    #[test]
    fn test_make_ex_context_cursor_row() {
        let mut editor = Editor::for_testing("line1\nline2\nline3\n");
        editor.document.cursor.row = 1;
        let ctx = editor.make_ex_context();
        assert_eq!(ctx.current_line, 1);
        assert_eq!(ctx.buffer_len, 3);
    }

    #[test]
    fn test_flush_ex_status_with_message() {
        let mut editor = Editor::for_testing("hello\n");
        editor.state.status_message = Some(StatusMessage::Info("test msg".to_string()));
        let had = editor.flush_ex_status();
        assert!(had);
        assert!(editor.state.status_message.is_none());
    }

    #[test]
    fn test_flush_ex_status_no_message() {
        let mut editor = Editor::for_testing("hello\n");
        let had = editor.flush_ex_status();
        assert!(!had);
    }

    #[test]
    fn test_execute_ex_visual_is_noop() {
        let mut editor = Editor::for_testing("hello\n");
        editor.execute_ex_command(crate::command::ExCommand::Visual);
        assert!(editor.state.status_message.is_none());
        assert!(!editor.state.should_quit);
    }

    // =========================================================================
    // preserve_path
    // =========================================================================

    #[test]
    fn test_preserve_path_named_file() {
        assert_eq!(
            preserve_path("/tmp", Some("foo.txt"), 12345),
            PathBuf::from("/tmp/rvi.foo.txt.12345")
        );
    }

    #[test]
    fn test_preserve_path_strips_directory() {
        assert_eq!(
            preserve_path("/tmp", Some("/home/user/projects/bar.rs"), 99),
            PathBuf::from("/tmp/rvi.bar.rs.99")
        );
    }

    #[test]
    fn test_preserve_path_unnamed() {
        assert_eq!(
            preserve_path("/tmp", None, 1),
            PathBuf::from("/tmp/rvi.unnamed.1")
        );
    }

    #[test]
    fn test_preserve_path_empty_string() {
        // Empty string: file_name() returns None → falls back to "unnamed".
        assert_eq!(
            preserve_path("/tmp", Some(""), 5),
            PathBuf::from("/tmp/rvi.unnamed.5")
        );
    }

    #[test]
    fn test_preserve_path_custom_directory() {
        assert_eq!(
            preserve_path("/var/tmp", Some("foo.txt"), 42),
            PathBuf::from("/var/tmp/rvi.foo.txt.42")
        );
    }

    // =========================================================================
    // preserve_buffer / recover_buffer roundtrip
    // =========================================================================

    #[test]
    fn test_preserve_and_recover_roundtrip() {
        let mut editor = Editor::for_testing("hello\nworld\n");
        // Use a unique filename to avoid clashing with other test runs.
        let test_name = format!("rvi-test-preserve-{}", std::process::id());
        editor.document.filename = Some(test_name.clone());

        editor.preserve_buffer().expect("preserve should succeed");

        let pid = std::process::id();
        let expected_path = format!("/tmp/rvi.{}.{}", test_name, pid);
        assert!(
            std::path::Path::new(&expected_path).exists(),
            "preserve file should exist at {}",
            expected_path
        );

        // Clear buffer, then recover.
        editor.document.buffer = Buffer::new();
        editor
            .recover_buffer(Some(test_name.clone()))
            .expect("recover should succeed");
        assert_eq!(editor.document.buffer.line(0), Some("hello"));
        assert_eq!(editor.document.buffer.line(1), Some("world"));
        assert!(
            editor.document.modified,
            "recovered buffer should be marked modified"
        );

        // Cleanup.
        let _ = std::fs::remove_file(&expected_path);
    }

    #[test]
    fn test_list_preserve_files_includes_created_file() {
        // Create a proper rvi preserve file and verify list_preserve_files finds it.
        let pid = std::process::id();
        let test_name = format!("rvi-test-list-{}", pid);
        // Must use the rvi. prefix to be recognised.
        let path = format!("/tmp/rvi.{}.{}", test_name, pid);
        std::fs::write(&path, "test content\n").expect("write test preserve file");

        let entries = Editor::list_preserve_files();
        assert!(
            entries.iter().any(|e| e.contains(&path)),
            "list should include {}",
            path
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_list_preserve_files_ignores_non_rvi_files() {
        // A file without the rvi. prefix must NOT appear in the list,
        // even if its suffix is all digits.
        let pid = std::process::id();
        let path = format!("/tmp/other-app-file.{}", pid);
        std::fs::write(&path, "").ok();

        let entries = Editor::list_preserve_files();
        assert!(
            !entries.iter().any(|e| e.contains("other-app-file")),
            "list should not include non-rvi files"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_list_preserve_files_ignores_non_pid_suffixes() {
        // rvi. prefix but non-digit suffix — should not appear.
        let path = "/tmp/rvi.test-notapid.notdigits";
        std::fs::write(path, "").ok();

        let entries = Editor::list_preserve_files();
        assert!(
            !entries.iter().any(|e| e.contains("notdigits")),
            "list should not include non-pid files"
        );

        let _ = std::fs::remove_file(path);
    }
}

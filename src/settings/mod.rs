//! Settings module for `:set` command infrastructure.
//!
//! This module provides the typed `Settings` struct, the `SetResult` enum
//! for command feedback, and the `SetCommand` parser. Settings affect editor
//! behavior including search case-sensitivity and match highlighting.
//!
//! # Architecture
//!
//! - `parser.rs`: Syntactic parsing of `:set` commands and abbreviation resolution
//! - `mod.rs`: Typed settings struct, semantic validation, and apply/query methods
//!
//! The two-phase approach follows parse-don't-validate: the parser validates
//! syntax (is this a well-formed `:set` command?) while `Settings::apply()`
//! validates semantics (is this a real option? is the value in range?).

pub mod parser;

pub use parser::{parse_set_command, SetCommand};

use crate::error::SettingsError;

/// Maximum value for capped numeric settings (tabstop, shiftwidth).
///
/// This prevents absurdly large values that could cause rendering issues
/// while still feeling "unlimited" for practical use.
const MAX_NUMERIC_SETTING: usize = 64;

/// Maximum value for uncapped numeric settings (window, taglength).
///
/// These settings need larger values to accommodate large terminals and
/// long tag names, but still need some upper bound for sanity.
const MAX_NUMERIC_SETTING_LARGE: usize = 10000;

/// Result of applying or querying a setting.
///
/// Distinguishes between silent mutations (the user just did `:set number`)
/// and commands that produce display output (`:set number?` or `:set all`).
///
/// # Examples
///
/// ```
/// use rvi::settings::SetResult;
///
/// let result = SetResult::Changed;
/// assert_eq!(result, SetResult::Changed);
///
/// let result = SetResult::Message("number".to_string());
/// assert_eq!(result, SetResult::Message("number".to_string()));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetResult {
    /// A message to display to the user (query output).
    Message(String),
    /// The setting was changed silently (no display needed).
    Changed,
    /// The setting was accepted but has no effect in this implementation.
    Warning(String),
}

/// Editor settings controlled by `:set` commands.
///
/// All fields use vi-compatible defaults. Currently implemented settings
/// affect search behavior (`ignorecase`, `hlsearch`). Display and indentation
/// settings (`number`, `tabstop`, etc.) are stored but not yet active.
///
/// # Examples
///
/// ```
/// use rvi::settings::{Settings, SetCommand, SetResult};
///
/// let mut settings = Settings::default();
/// assert!(!settings.number);
/// assert_eq!(settings.tabstop, 8);
///
/// let result = settings.apply(&SetCommand::Enable("number".to_string())).unwrap();
/// assert_eq!(result, SetResult::Changed);
/// assert!(settings.number);
/// ```
#[derive(Debug, Clone)]
pub struct Settings {
    // Display
    /// Show line numbers. Default: `false`.
    pub number: bool,
    /// Wrap long lines. Default: `true`.
    pub wrap: bool,
    /// Display tabs as `^I` and append `$` at end of each line. Default: `false`.
    pub list: bool,

    // Tabs
    /// Number of spaces a tab character occupies. Default: `8`.
    pub tabstop: usize,
    /// Use spaces instead of tabs when inserting. Default: `false`.
    pub expandtab: bool,

    // Indentation
    /// Copy indent from the current line when starting a new line. Default: `false`.
    pub autoindent: bool,
    /// Number of spaces used for each step of (auto)indent. Default: `8`.
    pub shiftwidth: usize,

    // Margins
    /// Wrap margin: insert newline when typing past this distance from right edge. Default: `0` (off).
    pub wrapmargin: usize,
    /// Maximum line width for `gq` reformat. Default: `0` (disabled; falls back to `wrapmargin`).
    pub textwidth: usize,

    // Reporting
    /// Minimum number of lines changed before reporting on the status line. Default: `5`.
    pub report: usize,

    // Write
    /// Automatically write modified buffer before certain commands. Default: `false`.
    pub autowrite: bool,
    /// File is read-only; `:w` fails unless `!` is used. Default: `false`.
    pub readonly: bool,

    // Search
    /// When true, `.`, `*`, `[`, `~` are special in patterns. Default: `true`.
    pub magic: bool,
    /// Highlight all search matches. Default: `false`.
    pub hlsearch: bool,
    /// Ignore case in search patterns. Default: `false`.
    pub ignorecase: bool,
    /// Show matches incrementally as the search pattern is typed. Default: `false`.
    pub incsearch: bool,
    /// Wrap searches around end/beginning of buffer. Default: `true`.
    pub wrapscan: bool,
    /// Suppress verbose messages (line counts, etc.). Default: `false`.
    pub terse: bool,

    // Scrolling
    /// Half-screen scroll size for Ctrl-d/Ctrl-u. Default: `0` (half the viewport height).
    pub scroll: usize,

    // Bells
    /// Ring the terminal bell (BEL, `\x07`) before error messages. Default: `false`.
    pub errorbells: bool,
    /// Print the current line after each modifying ex command in ex mode. Default: `true`.
    pub autoprint: bool,

    // POSIX boolean settings (store only, no behavior wired)
    /// Discard control characters on input. Default: `false`.
    pub beautify: bool,
    /// Remember the last replacement pattern flags. Default: `false`.
    pub edcompatible: bool,
    /// Source `.exrc` in the current directory on startup. Default: `false`.
    pub exrc: bool,
    /// Hardware tab stops. Default: `false`.
    pub hardtabs: bool,
    /// Enable Lisp-mode indentation. Default: `false`.
    pub lisp: bool,
    /// Allow messages from other users. Default: `false`.
    pub mesg: bool,
    /// Enable novice mode (verbose prompts). Default: `false`.
    pub novice: bool,
    /// Allow open and visual modes from ex. Default: `true` (POSIX default).
    pub open: bool,
    /// Optimize terminal output for slow terminals. Default: `false`.
    pub optimize: bool,
    /// Simulate a smart terminal on a dumb one. Default: `false`.
    pub redraw: bool,
    /// Delay updates during inserts on slow terminals. Default: `false`.
    pub slowopen: bool,
    /// Warn before `:!` if the buffer has been modified. Default: `true` (POSIX default).
    pub warn: bool,
    /// Allow writing to any file without requiring `!`. Default: `false`.
    pub writeany: bool,

    // POSIX numeric settings
    /// Truncate tag names to this many chars for lookup; 0 means full match. Default: `0`.
    pub taglength: usize,
    /// Override viewport height; 0 means use terminal height. Default: `0`.
    pub window: usize,

    // POSIX string settings
    /// Directory for preserve (crash recovery) files. Default: `"/tmp"`.
    pub directory: String,
    /// Shell to use for `:!` and `:sh`. Empty means use `$SHELL` or `/bin/sh`. Default: `""`.
    pub shell: String,
    /// Colon-separated list of tag file paths. Default: `"tags"`.
    pub tags: String,
    /// Terminal type (informational). Default: `""`.
    pub term: String,
    /// Paragraph delimiter macro names. Default: `"IPLPPPQPP LIpplpipbp"`.
    pub paragraphs: String,
    /// Section delimiter macro names. Default: `"NHSHH HUnhsh"`.
    pub sections: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            number: false,
            wrap: true,
            list: false,
            tabstop: 8,
            expandtab: false,
            autoindent: false,
            shiftwidth: 8,
            wrapmargin: 0,
            textwidth: 0,
            report: 5,
            autowrite: false,
            readonly: false,
            magic: true,
            hlsearch: false,
            ignorecase: false,
            incsearch: false,
            wrapscan: true,
            terse: false,
            scroll: 0,
            errorbells: false,
            autoprint: true,
            beautify: false,
            edcompatible: false,
            exrc: false,
            hardtabs: false,
            lisp: false,
            mesg: false,
            novice: false,
            open: true,
            optimize: false,
            redraw: false,
            slowopen: false,
            warn: true,
            writeany: false,
            taglength: 0,
            window: 0,
            directory: "/tmp".to_string(),
            shell: String::new(),
            tags: "tags".to_string(),
            term: String::new(),
            paragraphs: "IPLPPPQPP LIpplpipbp".to_string(),
            sections: "NHSHH HUnhsh".to_string(),
        }
    }
}

/// All known boolean option names.
const BOOLEAN_OPTIONS: &[&str] = &[
    "number",
    "wrap",
    "list",
    "expandtab",
    "autoindent",
    "autowrite",
    "readonly",
    "magic",
    "hlsearch",
    "ignorecase",
    "incsearch",
    "wrapscan",
    "terse",
    "errorbells",
    "autoprint",
    "beautify",
    "edcompatible",
    "exrc",
    "hardtabs",
    "lisp",
    "mesg",
    "novice",
    "open",
    "optimize",
    "redraw",
    "slowopen",
    "warn",
    "writeany",
];

/// All known numeric option names.
const NUMERIC_OPTIONS: &[&str] = &[
    "tabstop",
    "shiftwidth",
    "wrapmargin",
    "textwidth",
    "report",
    "scroll",
    "taglength",
    "window",
];

/// Numeric options that allow zero and have a large upper bound.
const LARGE_NUMERIC_OPTIONS: &[&str] = &["taglength", "window"];

/// All known string option names.
const STRING_OPTIONS: &[&str] = &[
    "directory",
    "shell",
    "tags",
    "term",
    "paragraphs",
    "sections",
];

/// Boolean options that are parsed and stored but have no behavioral effect
/// in this implementation. Setting them is accepted but a warning is shown.
const UNIMPLEMENTED_OPTIONS: &[&str] = &[
    "beautify",
    "edcompatible",
    "exrc",
    "hardtabs",
    "lisp",
    "mesg",
    "novice",
    "open",
    "optimize",
    "redraw",
    "slowopen",
    "writeany",
];

/// Check whether the given canonical name is a boolean option.
fn is_boolean_option(name: &str) -> bool {
    BOOLEAN_OPTIONS.contains(&name)
}

/// Check whether the option is recognized but not yet implemented.
fn is_unimplemented_option(name: &str) -> bool {
    UNIMPLEMENTED_OPTIONS.contains(&name)
}

/// Check whether the given canonical name is a numeric option.
fn is_numeric_option(name: &str) -> bool {
    NUMERIC_OPTIONS.contains(&name)
}

/// Check whether the given canonical name is a string option.
fn is_string_option(name: &str) -> bool {
    STRING_OPTIONS.contains(&name)
}

impl Settings {
    /// Apply a parsed `SetCommand` to these settings.
    ///
    /// Returns `Ok(SetResult::Changed)` for silent mutations,
    /// `Ok(SetResult::Message(..))` for queries, or
    /// `Err(SettingsError)` for invalid option names or values.
    ///
    /// # Errors
    ///
    /// - `SettingsError::InvalidOption` if the option name is not recognized
    /// - `SettingsError::InvalidValue` if the value is out of range or
    ///   incompatible with the option type (e.g., enabling a numeric option)
    pub fn apply(&mut self, cmd: &SetCommand) -> Result<SetResult, SettingsError> {
        match cmd {
            SetCommand::Enable(name) => self.apply_enable(name),
            SetCommand::Disable(name) => self.apply_disable(name),
            SetCommand::Toggle(name) => self.apply_toggle(name),
            SetCommand::Query(name) => {
                let msg = self.query(name)?;
                Ok(SetResult::Message(msg))
            }
            SetCommand::Assign { option, value } => self.apply_assign(option, value),
            SetCommand::QueryAll => Ok(SetResult::Message(self.query_all())),
        }
    }

    /// Query the current value of a single option.
    ///
    /// Returns a formatted string suitable for display:
    /// - Boolean true: `"name"` (e.g., `"number"`)
    /// - Boolean false: `"noname"` (e.g., `"nonumber"`)
    /// - Numeric: `"name=value"` (e.g., `"tabstop=8"`)
    /// - String: `"name=value"` (e.g., `"tags=tags"`)
    ///
    /// # Errors
    ///
    /// Returns `SettingsError::InvalidOption` if the name is not recognized.
    pub fn query(&self, name: &str) -> Result<String, SettingsError> {
        if is_boolean_option(name) {
            let value = self.get_bool(name)?;
            if value {
                Ok(name.to_string())
            } else {
                Ok(format!("no{}", name))
            }
        } else if is_numeric_option(name) {
            let value = self.get_numeric(name)?;
            Ok(format!("{}={}", name, value))
        } else if is_string_option(name) {
            let value = self.get_string(name)?;
            Ok(format!("{}={}", name, value))
        } else {
            Err(SettingsError::InvalidOption(name.to_string()))
        }
    }

    /// Query all settings, returning a space-separated string in struct field order.
    ///
    /// The output format matches the order of fields in `Settings`:
    /// display options, tab options, indentation options, then search options,
    /// followed by POSIX boolean, numeric, and string settings.
    pub fn query_all(&self) -> String {
        use std::fmt::Write;
        let mut result = String::new();

        // Original boolean/numeric settings
        write!(result, "{} ", Self::format_bool("number", self.number)).unwrap();
        write!(result, "{} ", Self::format_bool("wrap", self.wrap)).unwrap();
        write!(result, "{} ", Self::format_bool("list", self.list)).unwrap();
        write!(result, "tabstop={} ", self.tabstop).unwrap();
        write!(
            result,
            "{} ",
            Self::format_bool("expandtab", self.expandtab)
        )
        .unwrap();
        write!(
            result,
            "{} ",
            Self::format_bool("autoindent", self.autoindent)
        )
        .unwrap();
        write!(result, "shiftwidth={} ", self.shiftwidth).unwrap();
        write!(result, "wrapmargin={} ", self.wrapmargin).unwrap();
        write!(result, "textwidth={} ", self.textwidth).unwrap();
        write!(result, "report={} ", self.report).unwrap();
        write!(
            result,
            "{} ",
            Self::format_bool("autowrite", self.autowrite)
        )
        .unwrap();
        write!(result, "{} ", Self::format_bool("readonly", self.readonly)).unwrap();
        write!(result, "{} ", Self::format_bool("magic", self.magic)).unwrap();
        write!(result, "{} ", Self::format_bool("hlsearch", self.hlsearch)).unwrap();
        write!(
            result,
            "{} ",
            Self::format_bool("ignorecase", self.ignorecase)
        )
        .unwrap();
        write!(
            result,
            "{} ",
            Self::format_bool("incsearch", self.incsearch)
        )
        .unwrap();
        write!(result, "{} ", Self::format_bool("wrapscan", self.wrapscan)).unwrap();
        write!(result, "{} ", Self::format_bool("terse", self.terse)).unwrap();
        write!(result, "scroll={} ", self.scroll).unwrap();
        write!(
            result,
            "{} ",
            Self::format_bool("errorbells", self.errorbells)
        )
        .unwrap();
        write!(
            result,
            "{} ",
            Self::format_bool("autoprint", self.autoprint)
        )
        .unwrap();

        // New POSIX boolean settings
        write!(result, "{} ", Self::format_bool("beautify", self.beautify)).unwrap();
        write!(
            result,
            "{} ",
            Self::format_bool("edcompatible", self.edcompatible)
        )
        .unwrap();
        write!(result, "{} ", Self::format_bool("exrc", self.exrc)).unwrap();
        write!(result, "{} ", Self::format_bool("hardtabs", self.hardtabs)).unwrap();
        write!(result, "{} ", Self::format_bool("lisp", self.lisp)).unwrap();
        write!(result, "{} ", Self::format_bool("mesg", self.mesg)).unwrap();
        write!(result, "{} ", Self::format_bool("novice", self.novice)).unwrap();
        write!(result, "{} ", Self::format_bool("open", self.open)).unwrap();
        write!(result, "{} ", Self::format_bool("optimize", self.optimize)).unwrap();
        write!(result, "{} ", Self::format_bool("redraw", self.redraw)).unwrap();
        write!(result, "{} ", Self::format_bool("slowopen", self.slowopen)).unwrap();
        write!(result, "{} ", Self::format_bool("warn", self.warn)).unwrap();
        write!(result, "{} ", Self::format_bool("writeany", self.writeany)).unwrap();

        // New POSIX numeric settings
        write!(result, "taglength={} ", self.taglength).unwrap();
        write!(result, "window={} ", self.window).unwrap();

        // String settings
        write!(result, "directory={} ", self.directory).unwrap();
        write!(result, "shell={} ", self.shell).unwrap();
        write!(result, "tags={} ", self.tags).unwrap();
        write!(result, "term={} ", self.term).unwrap();
        write!(result, "paragraphs={} ", self.paragraphs).unwrap();
        write!(result, "sections={}", self.sections).unwrap();

        result
    }

    /// Format a boolean option for display.
    fn format_bool(name: &str, value: bool) -> String {
        if value {
            name.to_string()
        } else {
            format!("no{}", name)
        }
    }

    /// Enable a boolean option.
    fn apply_enable(&mut self, name: &str) -> Result<SetResult, SettingsError> {
        if is_numeric_option(name) || is_string_option(name) {
            return Err(SettingsError::InvalidValue {
                option: name.to_string(),
                value: "cannot enable a numeric option".to_string(),
            });
        }
        self.set_bool(name, true)?;
        if is_unimplemented_option(name) {
            return Ok(SetResult::Warning(format!(
                "'{}' is set but not implemented",
                name
            )));
        }
        Ok(SetResult::Changed)
    }

    /// Disable a boolean option.
    fn apply_disable(&mut self, name: &str) -> Result<SetResult, SettingsError> {
        if is_numeric_option(name) || is_string_option(name) {
            return Err(SettingsError::InvalidValue {
                option: name.to_string(),
                value: "cannot disable a numeric option".to_string(),
            });
        }
        self.set_bool(name, false)?;
        if is_unimplemented_option(name) {
            return Ok(SetResult::Warning(format!(
                "'{}' is set but not implemented",
                name
            )));
        }
        Ok(SetResult::Changed)
    }

    /// Toggle a boolean option.
    fn apply_toggle(&mut self, name: &str) -> Result<SetResult, SettingsError> {
        if is_numeric_option(name) || is_string_option(name) {
            return Err(SettingsError::InvalidValue {
                option: name.to_string(),
                value: "cannot toggle a numeric option".to_string(),
            });
        }
        let current = self.get_bool(name)?;
        self.set_bool(name, !current)?;
        if is_unimplemented_option(name) {
            return Ok(SetResult::Warning(format!(
                "'{}' is set but not implemented",
                name
            )));
        }
        Ok(SetResult::Changed)
    }

    /// Assign a value to an option.
    fn apply_assign(&mut self, name: &str, value: &str) -> Result<SetResult, SettingsError> {
        if is_boolean_option(name) {
            let bool_val = match value {
                "true" | "1" => true,
                "false" | "0" => false,
                _ => {
                    return Err(SettingsError::InvalidValue {
                        option: name.to_string(),
                        value: format!(
                            "'{}' is not valid for boolean option (use true/false/0/1)",
                            value
                        ),
                    });
                }
            };
            self.set_bool(name, bool_val)?;
            Ok(SetResult::Changed)
        } else if is_numeric_option(name) {
            let num_val: usize = value.parse().map_err(|_| SettingsError::InvalidValue {
                option: name.to_string(),
                value: format!("'{}' is not a valid number", value),
            })?;

            let is_large = LARGE_NUMERIC_OPTIONS.contains(&name);
            // Options that allow zero: wrapmargin, textwidth, report, scroll, taglength, window
            let allows_zero = name == "wrapmargin"
                || name == "textwidth"
                || name == "report"
                || name == "scroll"
                || is_large;

            if num_val == 0 && !allows_zero {
                return Err(SettingsError::InvalidValue {
                    option: name.to_string(),
                    value: "value must be greater than 0".to_string(),
                });
            }

            let max = if is_large {
                MAX_NUMERIC_SETTING_LARGE
            } else {
                MAX_NUMERIC_SETTING
            };

            if num_val > max {
                return Err(SettingsError::InvalidValue {
                    option: name.to_string(),
                    value: format!("value must be at most {}", max),
                });
            }
            self.set_numeric(name, num_val)?;
            Ok(SetResult::Changed)
        } else if is_string_option(name) {
            self.set_string(name, value)?;
            Ok(SetResult::Changed)
        } else {
            Err(SettingsError::InvalidOption(name.to_string()))
        }
    }

    /// Get the value of a boolean option by name.
    fn get_bool(&self, name: &str) -> Result<bool, SettingsError> {
        match name {
            "number" => Ok(self.number),
            "wrap" => Ok(self.wrap),
            "list" => Ok(self.list),
            "expandtab" => Ok(self.expandtab),
            "autoindent" => Ok(self.autoindent),
            "hlsearch" => Ok(self.hlsearch),
            "ignorecase" => Ok(self.ignorecase),
            "incsearch" => Ok(self.incsearch),
            "wrapscan" => Ok(self.wrapscan),
            "terse" => Ok(self.terse),
            "autowrite" => Ok(self.autowrite),
            "readonly" => Ok(self.readonly),
            "magic" => Ok(self.magic),
            "errorbells" => Ok(self.errorbells),
            "autoprint" => Ok(self.autoprint),
            "beautify" => Ok(self.beautify),
            "edcompatible" => Ok(self.edcompatible),
            "exrc" => Ok(self.exrc),
            "hardtabs" => Ok(self.hardtabs),
            "lisp" => Ok(self.lisp),
            "mesg" => Ok(self.mesg),
            "novice" => Ok(self.novice),
            "open" => Ok(self.open),
            "optimize" => Ok(self.optimize),
            "redraw" => Ok(self.redraw),
            "slowopen" => Ok(self.slowopen),
            "warn" => Ok(self.warn),
            "writeany" => Ok(self.writeany),
            _ => Err(SettingsError::InvalidOption(name.to_string())),
        }
    }

    /// Set a boolean option by name.
    fn set_bool(&mut self, name: &str, value: bool) -> Result<(), SettingsError> {
        match name {
            "number" => self.number = value,
            "wrap" => self.wrap = value,
            "list" => self.list = value,
            "expandtab" => self.expandtab = value,
            "autoindent" => self.autoindent = value,
            "hlsearch" => self.hlsearch = value,
            "ignorecase" => self.ignorecase = value,
            "incsearch" => self.incsearch = value,
            "wrapscan" => self.wrapscan = value,
            "terse" => self.terse = value,
            "autowrite" => self.autowrite = value,
            "readonly" => self.readonly = value,
            "magic" => self.magic = value,
            "errorbells" => self.errorbells = value,
            "autoprint" => self.autoprint = value,
            "beautify" => self.beautify = value,
            "edcompatible" => self.edcompatible = value,
            "exrc" => self.exrc = value,
            "hardtabs" => self.hardtabs = value,
            "lisp" => self.lisp = value,
            "mesg" => self.mesg = value,
            "novice" => self.novice = value,
            "open" => self.open = value,
            "optimize" => self.optimize = value,
            "redraw" => self.redraw = value,
            "slowopen" => self.slowopen = value,
            "warn" => self.warn = value,
            "writeany" => self.writeany = value,
            _ => return Err(SettingsError::InvalidOption(name.to_string())),
        }
        Ok(())
    }

    /// Get the value of a numeric option by name.
    fn get_numeric(&self, name: &str) -> Result<usize, SettingsError> {
        match name {
            "tabstop" => Ok(self.tabstop),
            "shiftwidth" => Ok(self.shiftwidth),
            "wrapmargin" => Ok(self.wrapmargin),
            "textwidth" => Ok(self.textwidth),
            "report" => Ok(self.report),
            "scroll" => Ok(self.scroll),
            "taglength" => Ok(self.taglength),
            "window" => Ok(self.window),
            _ => Err(SettingsError::InvalidOption(name.to_string())),
        }
    }

    /// Set a numeric option by name.
    fn set_numeric(&mut self, name: &str, value: usize) -> Result<(), SettingsError> {
        match name {
            "tabstop" => self.tabstop = value,
            "shiftwidth" => self.shiftwidth = value,
            "wrapmargin" => self.wrapmargin = value,
            "textwidth" => self.textwidth = value,
            "report" => self.report = value,
            "scroll" => self.scroll = value,
            "taglength" => self.taglength = value,
            "window" => self.window = value,
            _ => return Err(SettingsError::InvalidOption(name.to_string())),
        }
        Ok(())
    }

    /// Get the value of a string option by name.
    fn get_string(&self, name: &str) -> Result<&str, SettingsError> {
        match name {
            "directory" => Ok(&self.directory),
            "shell" => Ok(&self.shell),
            "tags" => Ok(&self.tags),
            "term" => Ok(&self.term),
            "paragraphs" => Ok(&self.paragraphs),
            "sections" => Ok(&self.sections),
            _ => Err(SettingsError::InvalidOption(name.to_string())),
        }
    }

    /// Set a string option by name.
    fn set_string(&mut self, name: &str, value: &str) -> Result<(), SettingsError> {
        match name {
            "directory" => self.directory = value.to_string(),
            "shell" => self.shell = value.to_string(),
            "tags" => self.tags = value.to_string(),
            "term" => self.term = value.to_string(),
            "paragraphs" => self.paragraphs = value.to_string(),
            "sections" => self.sections = value.to_string(),
            _ => return Err(SettingsError::InvalidOption(name.to_string())),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a Settings with specific overrides.
    fn settings_with(f: impl FnOnce(&mut Settings)) -> Settings {
        let mut s = Settings::default();
        f(&mut s);
        s
    }

    // =========================================================================
    // Default values
    // =========================================================================

    #[test]
    fn test_default_values() {
        let s = Settings::default();
        assert!(!s.number);
        assert!(s.wrap);
        assert_eq!(s.tabstop, 8);
        assert!(!s.expandtab);
        assert!(!s.autoindent);
        assert_eq!(s.shiftwidth, 8);
        assert_eq!(s.report, 5);
        assert!(!s.readonly);
        assert!(!s.hlsearch);
        assert!(!s.ignorecase);
        assert!(!s.incsearch);
        assert_eq!(s.scroll, 0);
        assert!(!s.errorbells);
        assert!(s.autoprint);
    }

    #[test]
    fn test_new_boolean_defaults() {
        let s = Settings::default();
        assert!(!s.beautify);
        assert!(!s.edcompatible);
        assert!(!s.exrc);
        assert!(!s.hardtabs);
        assert!(!s.lisp);
        assert!(!s.mesg);
        assert!(!s.novice);
        assert!(s.open);
        assert!(!s.optimize);
        assert!(!s.redraw);
        assert!(!s.slowopen);
        assert!(s.warn);
        assert!(!s.writeany);
    }

    #[test]
    fn test_new_numeric_defaults() {
        let s = Settings::default();
        assert_eq!(s.taglength, 0);
        assert_eq!(s.window, 0);
    }

    #[test]
    fn test_new_string_defaults() {
        let s = Settings::default();
        assert_eq!(s.directory, "/tmp");
        assert_eq!(s.shell, "");
        assert_eq!(s.tags, "tags");
        assert_eq!(s.term, "");
        assert_eq!(s.paragraphs, "IPLPPPQPP LIpplpipbp");
        assert_eq!(s.sections, "NHSHH HUnhsh");
    }

    // =========================================================================
    // Enable / Disable / Toggle
    // =========================================================================

    #[test]
    fn test_apply_enable_bool() {
        let mut s = Settings::default();
        let result = s.apply(&SetCommand::Enable("number".to_string())).unwrap();
        assert_eq!(result, SetResult::Changed);
        assert!(s.number);
    }

    #[test]
    fn test_apply_disable_bool() {
        let mut s = settings_with(|s| s.wrap = true);
        let result = s.apply(&SetCommand::Disable("wrap".to_string())).unwrap();
        assert_eq!(result, SetResult::Changed);
        assert!(!s.wrap);
    }

    #[test]
    fn test_apply_toggle_bool() {
        let mut s = Settings::default();
        assert!(!s.number);
        s.apply(&SetCommand::Toggle("number".to_string())).unwrap();
        assert!(s.number);
        s.apply(&SetCommand::Toggle("number".to_string())).unwrap();
        assert!(!s.number);
    }

    #[test]
    fn test_enable_numeric_option_fails() {
        let mut s = Settings::default();
        let result = s.apply(&SetCommand::Enable("tabstop".to_string()));
        assert!(result.is_err());
        match result.unwrap_err() {
            SettingsError::InvalidValue { option, .. } => {
                assert_eq!(option, "tabstop");
            }
            other => panic!("Expected InvalidValue, got {:?}", other),
        }
    }

    #[test]
    fn test_disable_numeric_option_fails() {
        let mut s = Settings::default();
        let result = s.apply(&SetCommand::Disable("shiftwidth".to_string()));
        assert!(result.is_err());
    }

    #[test]
    fn test_toggle_numeric_option_fails() {
        let mut s = Settings::default();
        let result = s.apply(&SetCommand::Toggle("tabstop".to_string()));
        assert!(result.is_err());
    }

    #[test]
    fn test_enable_string_option_fails() {
        let mut s = Settings::default();
        let result = s.apply(&SetCommand::Enable("shell".to_string()));
        assert!(result.is_err());
    }

    #[test]
    fn test_disable_string_option_fails() {
        let mut s = Settings::default();
        let result = s.apply(&SetCommand::Disable("directory".to_string()));
        assert!(result.is_err());
    }

    #[test]
    fn test_toggle_string_option_fails() {
        let mut s = Settings::default();
        let result = s.apply(&SetCommand::Toggle("tags".to_string()));
        assert!(result.is_err());
    }

    // =========================================================================
    // Assign
    // =========================================================================

    #[test]
    fn test_assign_numeric_valid() {
        let mut s = Settings::default();
        let result = s
            .apply(&SetCommand::Assign {
                option: "tabstop".to_string(),
                value: "4".to_string(),
            })
            .unwrap();
        assert_eq!(result, SetResult::Changed);
        assert_eq!(s.tabstop, 4);
    }

    #[test]
    fn test_assign_numeric_zero_fails() {
        let mut s = Settings::default();
        let result = s.apply(&SetCommand::Assign {
            option: "tabstop".to_string(),
            value: "0".to_string(),
        });
        assert!(result.is_err());
        match result.unwrap_err() {
            SettingsError::InvalidValue { option, .. } => {
                assert_eq!(option, "tabstop");
            }
            other => panic!("Expected InvalidValue, got {:?}", other),
        }
    }

    #[test]
    fn test_assign_numeric_non_number_fails() {
        let mut s = Settings::default();
        let result = s.apply(&SetCommand::Assign {
            option: "tabstop".to_string(),
            value: "abc".to_string(),
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_assign_shiftwidth() {
        let mut s = Settings::default();
        s.apply(&SetCommand::Assign {
            option: "shiftwidth".to_string(),
            value: "2".to_string(),
        })
        .unwrap();
        assert_eq!(s.shiftwidth, 2);
    }

    #[test]
    fn test_assign_bool_with_0() {
        let mut s = settings_with(|s| s.number = true);
        s.apply(&SetCommand::Assign {
            option: "number".to_string(),
            value: "0".to_string(),
        })
        .unwrap();
        assert!(!s.number);
    }

    #[test]
    fn test_assign_bool_with_1() {
        let mut s = Settings::default();
        s.apply(&SetCommand::Assign {
            option: "number".to_string(),
            value: "1".to_string(),
        })
        .unwrap();
        assert!(s.number);
    }

    #[test]
    fn test_assign_bool_with_true() {
        let mut s = Settings::default();
        s.apply(&SetCommand::Assign {
            option: "number".to_string(),
            value: "true".to_string(),
        })
        .unwrap();
        assert!(s.number);
    }

    #[test]
    fn test_assign_bool_with_false() {
        let mut s = settings_with(|s| s.number = true);
        s.apply(&SetCommand::Assign {
            option: "number".to_string(),
            value: "false".to_string(),
        })
        .unwrap();
        assert!(!s.number);
    }

    #[test]
    fn test_assign_bool_with_invalid_value() {
        let mut s = Settings::default();
        let result = s.apply(&SetCommand::Assign {
            option: "number".to_string(),
            value: "yes".to_string(),
        });
        assert!(result.is_err());
    }

    // =========================================================================
    // String assign and query
    // =========================================================================

    #[test]
    fn test_assign_string_directory() {
        let mut s = Settings::default();
        s.apply(&SetCommand::Assign {
            option: "directory".to_string(),
            value: "/var/tmp".to_string(),
        })
        .unwrap();
        assert_eq!(s.directory, "/var/tmp");
    }

    #[test]
    fn test_assign_string_shell() {
        let mut s = Settings::default();
        s.apply(&SetCommand::Assign {
            option: "shell".to_string(),
            value: "/bin/zsh".to_string(),
        })
        .unwrap();
        assert_eq!(s.shell, "/bin/zsh");
    }

    #[test]
    fn test_assign_string_tags() {
        let mut s = Settings::default();
        s.apply(&SetCommand::Assign {
            option: "tags".to_string(),
            value: "tags:TAGS:.tags".to_string(),
        })
        .unwrap();
        assert_eq!(s.tags, "tags:TAGS:.tags");
    }

    #[test]
    fn test_assign_string_term() {
        let mut s = Settings::default();
        s.apply(&SetCommand::Assign {
            option: "term".to_string(),
            value: "xterm-256color".to_string(),
        })
        .unwrap();
        assert_eq!(s.term, "xterm-256color");
    }

    #[test]
    fn test_assign_string_paragraphs() {
        let mut s = Settings::default();
        s.apply(&SetCommand::Assign {
            option: "paragraphs".to_string(),
            value: "IPLP".to_string(),
        })
        .unwrap();
        assert_eq!(s.paragraphs, "IPLP");
    }

    #[test]
    fn test_assign_string_sections() {
        let mut s = Settings::default();
        s.apply(&SetCommand::Assign {
            option: "sections".to_string(),
            value: "NHSH".to_string(),
        })
        .unwrap();
        assert_eq!(s.sections, "NHSH");
    }

    #[test]
    fn test_query_string_directory() {
        let s = Settings::default();
        assert_eq!(s.query("directory").unwrap(), "directory=/tmp");
    }

    #[test]
    fn test_query_string_shell() {
        let s = Settings::default();
        assert_eq!(s.query("shell").unwrap(), "shell=");
    }

    #[test]
    fn test_query_string_tags() {
        let s = Settings::default();
        assert_eq!(s.query("tags").unwrap(), "tags=tags");
    }

    #[test]
    fn test_query_string_term() {
        let s = Settings::default();
        assert_eq!(s.query("term").unwrap(), "term=");
    }

    // =========================================================================
    // Query
    // =========================================================================

    #[test]
    fn test_query_bool_true() {
        let mut s = settings_with(|s| s.number = true);
        let result = s.apply(&SetCommand::Query("number".to_string())).unwrap();
        assert_eq!(result, SetResult::Message("number".to_string()));
    }

    #[test]
    fn test_query_bool_false() {
        let s = Settings::default();
        let result = s.query("number").unwrap();
        assert_eq!(result, "nonumber");
    }

    #[test]
    fn test_query_numeric() {
        let s = Settings::default();
        let result = s.query("tabstop").unwrap();
        assert_eq!(result, "tabstop=8");
    }

    #[test]
    fn test_query_unknown_option() {
        let s = Settings::default();
        let result = s.query("foobar");
        assert!(result.is_err());
    }

    // =========================================================================
    // QueryAll
    // =========================================================================

    #[test]
    fn test_query_all_default() {
        let s = Settings::default();
        let result = s.query_all();
        assert_eq!(
            result,
            "nonumber wrap nolist tabstop=8 noexpandtab noautoindent shiftwidth=8 \
             wrapmargin=0 textwidth=0 report=5 noautowrite noreadonly magic nohlsearch noignorecase \
             noincsearch wrapscan noterse scroll=0 noerrorbells autoprint nobeautify noedcompatible noexrc \
             nohardtabs nolisp nomesg nonovice open nooptimize noredraw noslowopen warn \
             nowriteany taglength=0 window=0 directory=/tmp shell= tags=tags term= \
             paragraphs=IPLPPPQPP LIpplpipbp sections=NHSHH HUnhsh"
        );
    }

    #[test]
    fn test_query_all_after_changes() {
        let s = settings_with(|s| {
            s.number = true;
            s.tabstop = 4;
            s.expandtab = true;
        });
        let result = s.query_all();
        // Just check that the changed values appear correctly
        assert!(result.starts_with("number wrap nolist tabstop=4 expandtab"));
    }

    #[test]
    fn test_apply_query_all() {
        let mut s = Settings::default();
        let result = s.apply(&SetCommand::QueryAll).unwrap();
        match result {
            SetResult::Message(msg) => {
                assert!(msg.contains("nonumber"));
                assert!(msg.contains("tabstop=8"));
                assert!(msg.contains("tags=tags"));
                assert!(msg.contains("directory=/tmp"));
            }
            SetResult::Changed | SetResult::Warning(_) => panic!("Expected Message"),
        }
    }

    // =========================================================================
    // Unknown options
    // =========================================================================

    #[test]
    fn test_apply_unknown_option_enable() {
        let mut s = Settings::default();
        let result = s.apply(&SetCommand::Enable("foobar".to_string()));
        assert!(result.is_err());
        match result.unwrap_err() {
            SettingsError::InvalidOption(name) => assert_eq!(name, "foobar"),
            other => panic!("Expected InvalidOption, got {:?}", other),
        }
    }

    #[test]
    fn test_apply_unknown_option_disable() {
        let mut s = Settings::default();
        let result = s.apply(&SetCommand::Disable("foobar".to_string()));
        assert!(result.is_err());
    }

    #[test]
    fn test_apply_unknown_option_toggle() {
        let mut s = Settings::default();
        let result = s.apply(&SetCommand::Toggle("foobar".to_string()));
        assert!(result.is_err());
    }

    #[test]
    fn test_apply_unknown_option_assign() {
        let mut s = Settings::default();
        let result = s.apply(&SetCommand::Assign {
            option: "foobar".to_string(),
            value: "42".to_string(),
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_apply_unknown_option_query() {
        let mut s = Settings::default();
        let result = s.apply(&SetCommand::Query("foobar".to_string()));
        assert!(result.is_err());
    }

    // =========================================================================
    // All boolean options
    // =========================================================================

    #[test]
    fn test_enable_all_bool_options() {
        let mut s = Settings::default();
        let options = [
            "number",
            "wrap",
            "expandtab",
            "autoindent",
            "hlsearch",
            "ignorecase",
            "incsearch",
            "wrapscan",
            "errorbells",
            "autoprint",
            "beautify",
            "edcompatible",
            "exrc",
            "hardtabs",
            "lisp",
            "mesg",
            "novice",
            "open",
            "optimize",
            "redraw",
            "slowopen",
            "warn",
            "writeany",
        ];
        for name in &options {
            s.apply(&SetCommand::Enable(name.to_string())).unwrap();
        }
        assert!(s.number);
        assert!(s.wrap);
        assert!(s.expandtab);
        assert!(s.autoindent);
        assert!(s.hlsearch);
        assert!(s.ignorecase);
        assert!(s.incsearch);
        assert!(s.errorbells);
        assert!(s.autoprint);
        assert!(s.beautify);
        assert!(s.edcompatible);
        assert!(s.exrc);
        assert!(s.hardtabs);
        assert!(s.lisp);
        assert!(s.mesg);
        assert!(s.novice);
        assert!(s.open);
        assert!(s.optimize);
        assert!(s.redraw);
        assert!(s.slowopen);
        assert!(s.warn);
        assert!(s.writeany);
    }

    #[test]
    fn test_disable_all_bool_options() {
        let mut s = settings_with(|s| {
            s.number = true;
            s.wrap = true;
            s.expandtab = true;
            s.autoindent = true;
            s.hlsearch = true;
            s.ignorecase = true;
            s.incsearch = true;
            s.errorbells = true;
            s.autoprint = true;
            s.beautify = true;
            s.edcompatible = true;
            s.exrc = true;
            s.hardtabs = true;
            s.lisp = true;
            s.mesg = true;
            s.novice = true;
            s.open = true;
            s.optimize = true;
            s.redraw = true;
            s.slowopen = true;
            s.warn = true;
            s.writeany = true;
        });

        let options = [
            "number",
            "wrap",
            "expandtab",
            "autoindent",
            "hlsearch",
            "ignorecase",
            "incsearch",
            "wrapscan",
            "errorbells",
            "autoprint",
            "beautify",
            "edcompatible",
            "exrc",
            "hardtabs",
            "lisp",
            "mesg",
            "novice",
            "open",
            "optimize",
            "redraw",
            "slowopen",
            "warn",
            "writeany",
        ];
        for name in &options {
            s.apply(&SetCommand::Disable(name.to_string())).unwrap();
        }
        assert!(!s.number);
        assert!(!s.wrap);
        assert!(!s.expandtab);
        assert!(!s.autoindent);
        assert!(!s.hlsearch);
        assert!(!s.ignorecase);
        assert!(!s.incsearch);
        assert!(!s.errorbells);
        assert!(!s.autoprint);
        assert!(!s.beautify);
        assert!(!s.edcompatible);
        assert!(!s.exrc);
        assert!(!s.hardtabs);
        assert!(!s.lisp);
        assert!(!s.mesg);
        assert!(!s.novice);
        assert!(!s.open);
        assert!(!s.optimize);
        assert!(!s.redraw);
        assert!(!s.slowopen);
        assert!(!s.warn);
        assert!(!s.writeany);
    }

    // =========================================================================
    // report
    // =========================================================================

    #[test]
    fn test_report_default() {
        let s = Settings::default();
        assert_eq!(s.report, 5);
    }

    #[test]
    fn test_report_assign() {
        let mut s = Settings::default();
        s.apply(&SetCommand::Assign {
            option: "report".to_string(),
            value: "2".to_string(),
        })
        .unwrap();
        assert_eq!(s.report, 2);
    }

    #[test]
    fn test_report_assign_zero() {
        // 0 means always report
        let mut s = Settings::default();
        s.apply(&SetCommand::Assign {
            option: "report".to_string(),
            value: "0".to_string(),
        })
        .unwrap();
        assert_eq!(s.report, 0);
    }

    #[test]
    fn test_report_query() {
        let s = Settings::default();
        assert_eq!(s.query("report").unwrap(), "report=5");
    }

    // =========================================================================
    // list
    // =========================================================================

    #[test]
    fn test_list_default_off() {
        let s = Settings::default();
        assert!(!s.list);
    }

    #[test]
    fn test_list_enable() {
        let mut s = Settings::default();
        s.apply(&SetCommand::Enable("list".to_string())).unwrap();
        assert!(s.list);
    }

    #[test]
    fn test_list_disable() {
        let mut s = Settings {
            list: true,
            ..Default::default()
        };
        s.apply(&SetCommand::Disable("list".to_string())).unwrap();
        assert!(!s.list);
    }

    #[test]
    fn test_list_query() {
        let s = Settings::default();
        assert_eq!(s.query("list").unwrap(), "nolist");
        let s2 = Settings {
            list: true,
            ..Default::default()
        };
        assert_eq!(s2.query("list").unwrap(), "list");
    }

    // =========================================================================
    // readonly
    // =========================================================================

    #[test]
    fn test_readonly_default_off() {
        let s = Settings::default();
        assert!(!s.readonly);
    }

    #[test]
    fn test_enable_readonly() {
        let mut s = Settings::default();
        s.apply(&SetCommand::Enable("readonly".to_string()))
            .unwrap();
        assert!(s.readonly);
    }

    #[test]
    fn test_disable_readonly() {
        let mut s = settings_with(|s| s.readonly = true);
        s.apply(&SetCommand::Disable("readonly".to_string()))
            .unwrap();
        assert!(!s.readonly);
    }

    #[test]
    fn test_query_readonly() {
        let mut s = settings_with(|s| s.readonly = true);
        let result = s.apply(&SetCommand::Query("readonly".to_string())).unwrap();
        assert_eq!(result, SetResult::Message("readonly".to_string()));
    }

    // =========================================================================
    // Clone and Debug
    // =========================================================================

    #[test]
    fn test_settings_clone() {
        let s = settings_with(|s| {
            s.number = true;
            s.tabstop = 4;
        });
        let cloned = s.clone();
        assert!(cloned.number);
        assert_eq!(cloned.tabstop, 4);
    }

    #[test]
    fn test_settings_debug() {
        let s = Settings::default();
        let debug = format!("{:?}", s);
        assert!(debug.contains("Settings"));
        assert!(debug.contains("number"));
        assert!(debug.contains("tabstop"));
    }

    #[test]
    fn test_set_result_debug() {
        let r = SetResult::Changed;
        assert_eq!(format!("{:?}", r), "Changed");
        let r = SetResult::Message("test".to_string());
        assert!(format!("{:?}", r).contains("Message"));
    }

    #[test]
    fn test_assign_tabstop_upper_bound() {
        let mut s = Settings::default();
        // Should accept 64
        s.apply(&SetCommand::Assign {
            option: "tabstop".to_string(),
            value: "64".to_string(),
        })
        .unwrap();
        assert_eq!(s.tabstop, 64);

        // Should reject 65
        let result = s.apply(&SetCommand::Assign {
            option: "tabstop".to_string(),
            value: "65".to_string(),
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_assign_shiftwidth_upper_bound() {
        let mut s = Settings::default();
        // Should reject very large value
        let result = s.apply(&SetCommand::Assign {
            option: "shiftwidth".to_string(),
            value: "999999".to_string(),
        });
        assert!(result.is_err());
    }

    // =========================================================================
    // scroll
    // =========================================================================

    #[test]
    fn test_scroll_default_zero() {
        let s = Settings::default();
        assert_eq!(s.scroll, 0);
    }

    #[test]
    fn test_scroll_assign() {
        let mut s = Settings::default();
        s.apply(&SetCommand::Assign {
            option: "scroll".to_string(),
            value: "12".to_string(),
        })
        .unwrap();
        assert_eq!(s.scroll, 12);
    }

    #[test]
    fn test_scroll_assign_zero_accepted() {
        // scroll=0 means "use half viewport" — zero must be accepted unlike tabstop
        let mut s = Settings::default();
        s.apply(&SetCommand::Assign {
            option: "scroll".to_string(),
            value: "0".to_string(),
        })
        .unwrap();
        assert_eq!(s.scroll, 0);
    }

    #[test]
    fn test_scroll_query() {
        let s = Settings::default();
        assert_eq!(s.query("scroll").unwrap(), "scroll=0");
        let s2 = settings_with(|s| s.scroll = 15);
        assert_eq!(s2.query("scroll").unwrap(), "scroll=15");
    }

    #[test]
    fn test_scroll_abbreviation() {
        use crate::settings::parse_set_command;
        assert_eq!(
            parse_set_command("scr=10"),
            Ok(SetCommand::Assign {
                option: "scroll".to_string(),
                value: "10".to_string(),
            })
        );
    }

    // =========================================================================
    // errorbells
    // =========================================================================

    #[test]
    fn test_errorbells_default_off() {
        let s = Settings::default();
        assert!(!s.errorbells);
    }

    #[test]
    fn test_errorbells_enable() {
        let mut s = Settings::default();
        s.apply(&SetCommand::Enable("errorbells".to_string()))
            .unwrap();
        assert!(s.errorbells);
    }

    #[test]
    fn test_errorbells_disable() {
        let mut s = settings_with(|s| s.errorbells = true);
        s.apply(&SetCommand::Disable("errorbells".to_string()))
            .unwrap();
        assert!(!s.errorbells);
    }

    #[test]
    fn test_errorbells_query() {
        let s = Settings::default();
        assert_eq!(s.query("errorbells").unwrap(), "noerrorbells");
        let s2 = settings_with(|s| s.errorbells = true);
        assert_eq!(s2.query("errorbells").unwrap(), "errorbells");
    }

    #[test]
    fn test_errorbells_abbreviation() {
        use crate::settings::parse_set_command;
        assert_eq!(
            parse_set_command("eb"),
            Ok(SetCommand::Enable("errorbells".to_string()))
        );
        assert_eq!(
            parse_set_command("noeb"),
            Ok(SetCommand::Disable("errorbells".to_string()))
        );
    }

    // =========================================================================
    // autoprint
    // =========================================================================

    #[test]
    fn test_autoprint_default_on() {
        let s = Settings::default();
        assert!(s.autoprint);
    }

    #[test]
    fn test_autoprint_disable() {
        let mut s = Settings::default();
        s.apply(&SetCommand::Disable("autoprint".to_string()))
            .unwrap();
        assert!(!s.autoprint);
    }

    #[test]
    fn test_autoprint_enable() {
        let mut s = settings_with(|s| s.autoprint = false);
        s.apply(&SetCommand::Enable("autoprint".to_string()))
            .unwrap();
        assert!(s.autoprint);
    }

    #[test]
    fn test_autoprint_query() {
        let s = Settings::default();
        assert_eq!(s.query("autoprint").unwrap(), "autoprint");
        let s2 = settings_with(|s| s.autoprint = false);
        assert_eq!(s2.query("autoprint").unwrap(), "noautoprint");
    }

    #[test]
    fn test_autoprint_abbreviation() {
        use crate::settings::parse_set_command;
        assert_eq!(
            parse_set_command("ap"),
            Ok(SetCommand::Enable("autoprint".to_string()))
        );
        assert_eq!(
            parse_set_command("noap"),
            Ok(SetCommand::Disable("autoprint".to_string()))
        );
    }

    // =========================================================================
    // New POSIX boolean settings
    // =========================================================================

    #[test]
    fn test_beautify_enable_disable_query() {
        let mut s = Settings::default();
        assert!(!s.beautify);
        s.apply(&SetCommand::Enable("beautify".to_string()))
            .unwrap();
        assert!(s.beautify);
        assert_eq!(s.query("beautify").unwrap(), "beautify");
        s.apply(&SetCommand::Disable("beautify".to_string()))
            .unwrap();
        assert!(!s.beautify);
        assert_eq!(s.query("beautify").unwrap(), "nobeautify");
    }

    #[test]
    fn test_edcompatible_enable_disable_query() {
        let mut s = Settings::default();
        assert!(!s.edcompatible);
        s.apply(&SetCommand::Enable("edcompatible".to_string()))
            .unwrap();
        assert!(s.edcompatible);
        assert_eq!(s.query("edcompatible").unwrap(), "edcompatible");
    }

    #[test]
    fn test_exrc_enable_disable_query() {
        let mut s = Settings::default();
        assert!(!s.exrc);
        s.apply(&SetCommand::Enable("exrc".to_string())).unwrap();
        assert!(s.exrc);
        assert_eq!(s.query("exrc").unwrap(), "exrc");
    }

    #[test]
    fn test_hardtabs_enable_disable_query() {
        let mut s = Settings::default();
        s.apply(&SetCommand::Enable("hardtabs".to_string()))
            .unwrap();
        assert!(s.hardtabs);
        assert_eq!(s.query("hardtabs").unwrap(), "hardtabs");
    }

    #[test]
    fn test_lisp_enable_disable_query() {
        let mut s = Settings::default();
        s.apply(&SetCommand::Enable("lisp".to_string())).unwrap();
        assert!(s.lisp);
        assert_eq!(s.query("lisp").unwrap(), "lisp");
    }

    #[test]
    fn test_mesg_enable_disable_query() {
        let mut s = Settings::default();
        s.apply(&SetCommand::Enable("mesg".to_string())).unwrap();
        assert!(s.mesg);
        assert_eq!(s.query("mesg").unwrap(), "mesg");
    }

    #[test]
    fn test_novice_enable_disable_query() {
        let mut s = Settings::default();
        s.apply(&SetCommand::Enable("novice".to_string())).unwrap();
        assert!(s.novice);
        assert_eq!(s.query("novice").unwrap(), "novice");
    }

    #[test]
    fn test_open_default_on() {
        let s = Settings::default();
        assert!(s.open);
        assert_eq!(s.query("open").unwrap(), "open");
    }

    #[test]
    fn test_open_disable() {
        let mut s = Settings::default();
        s.apply(&SetCommand::Disable("open".to_string())).unwrap();
        assert!(!s.open);
        assert_eq!(s.query("open").unwrap(), "noopen");
    }

    #[test]
    fn test_optimize_enable_disable_query() {
        let mut s = Settings::default();
        s.apply(&SetCommand::Enable("optimize".to_string()))
            .unwrap();
        assert!(s.optimize);
        assert_eq!(s.query("optimize").unwrap(), "optimize");
    }

    #[test]
    fn test_redraw_enable_disable_query() {
        let mut s = Settings::default();
        s.apply(&SetCommand::Enable("redraw".to_string())).unwrap();
        assert!(s.redraw);
        assert_eq!(s.query("redraw").unwrap(), "redraw");
    }

    #[test]
    fn test_slowopen_enable_disable_query() {
        let mut s = Settings::default();
        s.apply(&SetCommand::Enable("slowopen".to_string()))
            .unwrap();
        assert!(s.slowopen);
        assert_eq!(s.query("slowopen").unwrap(), "slowopen");
    }

    #[test]
    fn test_warn_default_on() {
        let s = Settings::default();
        assert!(s.warn);
        assert_eq!(s.query("warn").unwrap(), "warn");
    }

    #[test]
    fn test_warn_disable() {
        let mut s = Settings::default();
        s.apply(&SetCommand::Disable("warn".to_string())).unwrap();
        assert!(!s.warn);
        assert_eq!(s.query("warn").unwrap(), "nowarn");
    }

    #[test]
    fn test_writeany_enable_disable_query() {
        let mut s = Settings::default();
        s.apply(&SetCommand::Enable("writeany".to_string()))
            .unwrap();
        assert!(s.writeany);
        assert_eq!(s.query("writeany").unwrap(), "writeany");
    }

    // =========================================================================
    // New POSIX numeric settings
    // =========================================================================

    #[test]
    fn test_taglength_default_zero() {
        let s = Settings::default();
        assert_eq!(s.taglength, 0);
        assert_eq!(s.query("taglength").unwrap(), "taglength=0");
    }

    #[test]
    fn test_taglength_assign() {
        let mut s = Settings::default();
        s.apply(&SetCommand::Assign {
            option: "taglength".to_string(),
            value: "5".to_string(),
        })
        .unwrap();
        assert_eq!(s.taglength, 5);
    }

    #[test]
    fn test_taglength_assign_zero() {
        let mut s = Settings::default();
        s.apply(&SetCommand::Assign {
            option: "taglength".to_string(),
            value: "0".to_string(),
        })
        .unwrap();
        assert_eq!(s.taglength, 0);
    }

    #[test]
    fn test_taglength_large_value() {
        let mut s = Settings::default();
        s.apply(&SetCommand::Assign {
            option: "taglength".to_string(),
            value: "256".to_string(),
        })
        .unwrap();
        assert_eq!(s.taglength, 256);
    }

    #[test]
    fn test_window_default_zero() {
        let s = Settings::default();
        assert_eq!(s.window, 0);
        assert_eq!(s.query("window").unwrap(), "window=0");
    }

    #[test]
    fn test_window_assign() {
        let mut s = Settings::default();
        s.apply(&SetCommand::Assign {
            option: "window".to_string(),
            value: "24".to_string(),
        })
        .unwrap();
        assert_eq!(s.window, 24);
    }

    #[test]
    fn test_window_assign_zero() {
        let mut s = Settings::default();
        s.apply(&SetCommand::Assign {
            option: "window".to_string(),
            value: "0".to_string(),
        })
        .unwrap();
        assert_eq!(s.window, 0);
    }

    #[test]
    fn test_window_large_value() {
        let mut s = Settings::default();
        s.apply(&SetCommand::Assign {
            option: "window".to_string(),
            value: "200".to_string(),
        })
        .unwrap();
        assert_eq!(s.window, 200);
    }
}

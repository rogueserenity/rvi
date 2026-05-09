//! Parser for `:set` command syntax.
//!
//! This module handles the syntactic parsing of `:set` commands, producing
//! a `SetCommand` enum that represents the user's intent without yet
//! validating whether the option name or value is semantically correct.
//! Semantic validation is done by `Settings::apply()` in `mod.rs`.
//!
//! Abbreviation resolution is also handled here, so downstream code
//! always works with canonical option names.

/// A parsed `:set` command, validated syntactically but not yet applied.
///
/// Each variant captures the user's intent: enable, disable, toggle,
/// query, assign, or show all settings. Option names are already
/// resolved through the abbreviation table.
///
/// # Examples
///
/// ```
/// use rvi::settings::SetCommand;
/// use rvi::settings::parse_set_command;
///
/// let cmd = parse_set_command("number").unwrap();
/// assert_eq!(cmd, SetCommand::Enable("number".to_string()));
///
/// let cmd = parse_set_command("ts=4").unwrap();
/// assert_eq!(cmd, SetCommand::Assign {
///     option: "tabstop".to_string(),
///     value: "4".to_string(),
/// });
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetCommand {
    /// `:set option` - enable a boolean option.
    Enable(String),
    /// `:set nooption` - disable a boolean option.
    Disable(String),
    /// `:set option!` - toggle a boolean option.
    Toggle(String),
    /// `:set option?` - query current value.
    Query(String),
    /// `:set option=value` - assign a value.
    Assign {
        /// The canonical option name.
        option: String,
        /// The raw value string (not yet parsed to a typed value).
        value: String,
    },
    /// `:set all` or bare `:set` - show all settings.
    QueryAll,
}

/// Abbreviation-to-canonical-name mapping.
///
/// Sorted by abbreviation for readability. Linear scan is optimal
/// for this small table.
///
/// Note: `wrap` has no abbreviation (already short, matches vim behavior).
const ABBREVIATIONS: &[(&str, &str)] = &[
    ("ai", "autoindent"),
    ("ap", "autoprint"),
    ("aw", "autowrite"),
    ("bf", "beautify"),
    ("di", "directory"),
    ("eb", "errorbells"),
    ("ed", "edcompatible"),
    ("et", "expandtab"),
    ("ex", "exrc"),
    ("hls", "hlsearch"),
    ("ht", "hardtabs"),
    ("ic", "ignorecase"),
    ("is", "incsearch"),
    ("nu", "number"),
    ("op", "optimize"),
    ("pa", "paragraphs"),
    ("re", "redraw"),
    ("ro", "readonly"),
    ("scr", "scroll"),
    ("se", "sections"),
    ("sh", "shell"),
    ("sl", "slowopen"),
    ("sw", "shiftwidth"),
    ("te", "terse"),
    ("tg", "tags"),
    ("tl", "taglength"),
    ("ts", "tabstop"),
    ("tw", "textwidth"),
    ("wa", "writeany"),
    ("wi", "window"),
    ("wm", "wrapmargin"),
    ("ws", "wrapscan"),
];

/// Resolve an option name through the abbreviation table.
///
/// Returns the canonical name if the input matches a known abbreviation,
/// or the input unchanged otherwise. This allows `Settings::apply()` to
/// reject truly unknown names.
fn resolve_name(name: &str) -> &str {
    for &(abbrev, canonical) in ABBREVIATIONS {
        if name == abbrev {
            return canonical;
        }
    }
    name
}

/// Parse the body of a `:set` command (everything after "set" or "se").
///
/// Returns `Ok(SetCommand)` on valid syntax, `Err(String)` on parse failure.
/// The input is trimmed before parsing.
///
/// # Parsing rules (applied in order)
///
/// 1. Empty input -> `Err` (nothing to do)
/// 2. `"all"` -> `QueryAll`
/// 3. Contains `'='` -> `Assign` (split at first `'='`)
/// 4. Ends with `'?'` -> `Query`
/// 5. Ends with `'!'` -> `Toggle`
/// 6. Starts with `"no"` and remainder >= 2 chars -> `Disable`
/// 7. Otherwise -> `Enable`
///
/// All option names are passed through `resolve_name()` for abbreviation
/// expansion before being stored in the returned `SetCommand`.
///
/// # Examples
///
/// ```
/// use rvi::settings::parse_set_command;
/// use rvi::settings::SetCommand;
///
/// assert!(parse_set_command("").is_err());
/// assert_eq!(
///     parse_set_command("nonumber"),
///     Ok(SetCommand::Disable("number".to_string()))
/// );
/// ```
pub fn parse_set_command(input: &str) -> Result<SetCommand, String> {
    let input = input.trim();

    if input.is_empty() {
        return Err(String::new());
    }

    if input == "all" {
        return Ok(SetCommand::QueryAll);
    }

    // Assign: contains '='
    if let Some(eq_pos) = input.find('=') {
        let lhs = &input[..eq_pos];
        let rhs = &input[eq_pos + 1..];
        if lhs.is_empty() {
            return Err("Empty option name before '='".to_string());
        }
        return Ok(SetCommand::Assign {
            option: resolve_name(lhs).to_string(),
            value: rhs.to_string(),
        });
    }

    // Query: ends with '?'
    if let Some(rest) = input.strip_suffix('?') {
        return Ok(SetCommand::Query(resolve_name(rest).to_string()));
    }

    // Toggle: ends with '!'
    if let Some(rest) = input.strip_suffix('!') {
        return Ok(SetCommand::Toggle(resolve_name(rest).to_string()));
    }

    // Disable: starts with "no" and remainder is at least 2 chars
    if let Some(rest) = input.strip_prefix("no") {
        if rest.len() >= 2 {
            return Ok(SetCommand::Disable(resolve_name(rest).to_string()));
        }
    }

    // Enable: everything else
    Ok(SetCommand::Enable(resolve_name(input).to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // parse_set_command basic variants
    // =========================================================================

    #[test]
    fn test_parse_enable() {
        assert_eq!(
            parse_set_command("number"),
            Ok(SetCommand::Enable("number".to_string()))
        );
    }

    #[test]
    fn test_parse_disable() {
        assert_eq!(
            parse_set_command("nonumber"),
            Ok(SetCommand::Disable("number".to_string()))
        );
    }

    #[test]
    fn test_parse_toggle() {
        assert_eq!(
            parse_set_command("number!"),
            Ok(SetCommand::Toggle("number".to_string()))
        );
    }

    #[test]
    fn test_parse_query() {
        assert_eq!(
            parse_set_command("number?"),
            Ok(SetCommand::Query("number".to_string()))
        );
    }

    #[test]
    fn test_parse_assign() {
        assert_eq!(
            parse_set_command("tabstop=4"),
            Ok(SetCommand::Assign {
                option: "tabstop".to_string(),
                value: "4".to_string(),
            })
        );
    }

    #[test]
    fn test_parse_query_all() {
        assert_eq!(parse_set_command("all"), Ok(SetCommand::QueryAll));
    }

    #[test]
    fn test_parse_empty() {
        assert!(parse_set_command("").is_err());
    }

    #[test]
    fn test_parse_whitespace_only() {
        assert!(parse_set_command("   ").is_err());
    }

    // =========================================================================
    // Abbreviation resolution
    // =========================================================================

    #[test]
    fn test_abbreviation_enable() {
        assert_eq!(
            parse_set_command("nu"),
            Ok(SetCommand::Enable("number".to_string()))
        );
    }

    #[test]
    fn test_abbreviation_disable() {
        assert_eq!(
            parse_set_command("nonu"),
            Ok(SetCommand::Disable("number".to_string()))
        );
    }

    #[test]
    fn test_abbreviation_assign() {
        assert_eq!(
            parse_set_command("ts=4"),
            Ok(SetCommand::Assign {
                option: "tabstop".to_string(),
                value: "4".to_string(),
            })
        );
    }

    #[test]
    fn test_abbreviation_query() {
        assert_eq!(
            parse_set_command("ts?"),
            Ok(SetCommand::Query("tabstop".to_string()))
        );
    }

    #[test]
    fn test_abbreviation_toggle() {
        assert_eq!(
            parse_set_command("nu!"),
            Ok(SetCommand::Toggle("number".to_string()))
        );
    }

    #[test]
    fn test_abbreviation_autoindent() {
        assert_eq!(
            parse_set_command("ai"),
            Ok(SetCommand::Enable("autoindent".to_string()))
        );
    }

    #[test]
    fn test_abbreviation_expandtab() {
        assert_eq!(
            parse_set_command("et"),
            Ok(SetCommand::Enable("expandtab".to_string()))
        );
    }

    #[test]
    fn test_abbreviation_hlsearch() {
        assert_eq!(
            parse_set_command("hls"),
            Ok(SetCommand::Enable("hlsearch".to_string()))
        );
    }

    #[test]
    fn test_abbreviation_ignorecase() {
        assert_eq!(
            parse_set_command("ic"),
            Ok(SetCommand::Enable("ignorecase".to_string()))
        );
    }

    #[test]
    fn test_abbreviation_incsearch() {
        assert_eq!(
            parse_set_command("is"),
            Ok(SetCommand::Enable("incsearch".to_string()))
        );
    }

    #[test]
    fn test_abbreviation_shiftwidth() {
        assert_eq!(
            parse_set_command("sw=4"),
            Ok(SetCommand::Assign {
                option: "shiftwidth".to_string(),
                value: "4".to_string(),
            })
        );
    }

    // =========================================================================
    // New P35 abbreviations
    // =========================================================================

    #[test]
    fn test_abbreviation_beautify() {
        assert_eq!(
            parse_set_command("bf"),
            Ok(SetCommand::Enable("beautify".to_string()))
        );
        assert_eq!(
            parse_set_command("nobf"),
            Ok(SetCommand::Disable("beautify".to_string()))
        );
    }

    #[test]
    fn test_abbreviation_edcompatible() {
        assert_eq!(
            parse_set_command("ed"),
            Ok(SetCommand::Enable("edcompatible".to_string()))
        );
    }

    #[test]
    fn test_abbreviation_exrc() {
        assert_eq!(
            parse_set_command("ex"),
            Ok(SetCommand::Enable("exrc".to_string()))
        );
    }

    #[test]
    fn test_abbreviation_hardtabs() {
        assert_eq!(
            parse_set_command("ht"),
            Ok(SetCommand::Enable("hardtabs".to_string()))
        );
    }

    #[test]
    fn test_abbreviation_optimize() {
        assert_eq!(
            parse_set_command("op"),
            Ok(SetCommand::Enable("optimize".to_string()))
        );
    }

    #[test]
    fn test_abbreviation_redraw() {
        assert_eq!(
            parse_set_command("re"),
            Ok(SetCommand::Enable("redraw".to_string()))
        );
    }

    #[test]
    fn test_abbreviation_slowopen() {
        assert_eq!(
            parse_set_command("sl"),
            Ok(SetCommand::Enable("slowopen".to_string()))
        );
    }

    #[test]
    fn test_abbreviation_writeany() {
        assert_eq!(
            parse_set_command("wa"),
            Ok(SetCommand::Enable("writeany".to_string()))
        );
    }

    #[test]
    fn test_abbreviation_taglength() {
        assert_eq!(
            parse_set_command("tl=5"),
            Ok(SetCommand::Assign {
                option: "taglength".to_string(),
                value: "5".to_string(),
            })
        );
    }

    #[test]
    fn test_abbreviation_window() {
        assert_eq!(
            parse_set_command("wi=24"),
            Ok(SetCommand::Assign {
                option: "window".to_string(),
                value: "24".to_string(),
            })
        );
    }

    #[test]
    fn test_abbreviation_directory() {
        assert_eq!(
            parse_set_command("di=/var/tmp"),
            Ok(SetCommand::Assign {
                option: "directory".to_string(),
                value: "/var/tmp".to_string(),
            })
        );
    }

    #[test]
    fn test_abbreviation_shell() {
        assert_eq!(
            parse_set_command("sh=/bin/zsh"),
            Ok(SetCommand::Assign {
                option: "shell".to_string(),
                value: "/bin/zsh".to_string(),
            })
        );
    }

    #[test]
    fn test_abbreviation_tags() {
        assert_eq!(
            parse_set_command("tg=mytags"),
            Ok(SetCommand::Assign {
                option: "tags".to_string(),
                value: "mytags".to_string(),
            })
        );
    }

    #[test]
    fn test_abbreviation_paragraphs() {
        assert_eq!(
            parse_set_command("pa=IPLP"),
            Ok(SetCommand::Assign {
                option: "paragraphs".to_string(),
                value: "IPLP".to_string(),
            })
        );
    }

    #[test]
    fn test_abbreviation_sections() {
        assert_eq!(
            parse_set_command("se=NHSH"),
            Ok(SetCommand::Assign {
                option: "sections".to_string(),
                value: "NHSH".to_string(),
            })
        );
    }

    // =========================================================================
    // Edge cases
    // =========================================================================

    #[test]
    fn test_no_prefix_short_name() {
        // "no" by itself should be Enable("no"), not Disable("")
        assert_eq!(
            parse_set_command("no"),
            Ok(SetCommand::Enable("no".to_string()))
        );
    }

    #[test]
    fn test_no_prefix_single_char_after() {
        // "nox" has only 1 char after "no" (< 2), so treated as Enable
        assert_eq!(
            parse_set_command("nox"),
            Ok(SetCommand::Enable("nox".to_string()))
        );
    }

    #[test]
    fn test_assign_empty_value() {
        assert_eq!(
            parse_set_command("tabstop="),
            Ok(SetCommand::Assign {
                option: "tabstop".to_string(),
                value: String::new(),
            })
        );
    }

    #[test]
    fn test_assign_multiple_equals() {
        // Split at first '=' only
        assert_eq!(
            parse_set_command("option=a=b"),
            Ok(SetCommand::Assign {
                option: "option".to_string(),
                value: "a=b".to_string(),
            })
        );
    }

    #[test]
    fn test_trimmed_input() {
        assert_eq!(
            parse_set_command("  number  "),
            Ok(SetCommand::Enable("number".to_string()))
        );
    }

    #[test]
    fn test_unknown_option_still_parses() {
        // Parser does not validate option names; that is Settings::apply()'s job
        assert_eq!(
            parse_set_command("foobar"),
            Ok(SetCommand::Enable("foobar".to_string()))
        );
    }

    #[test]
    fn test_disable_with_abbreviation_noai() {
        assert_eq!(
            parse_set_command("noai"),
            Ok(SetCommand::Disable("autoindent".to_string()))
        );
    }

    #[test]
    fn test_disable_with_abbreviation_noet() {
        assert_eq!(
            parse_set_command("noet"),
            Ok(SetCommand::Disable("expandtab".to_string()))
        );
    }

    #[test]
    fn test_disable_with_abbreviation_nohls() {
        assert_eq!(
            parse_set_command("nohls"),
            Ok(SetCommand::Disable("hlsearch".to_string()))
        );
    }

    #[test]
    fn test_disable_full_name() {
        assert_eq!(
            parse_set_command("noautoindent"),
            Ok(SetCommand::Disable("autoindent".to_string()))
        );
    }

    // =========================================================================
    // resolve_name unit tests
    // =========================================================================

    #[test]
    fn test_resolve_known_abbreviation() {
        assert_eq!(resolve_name("nu"), "number");
        assert_eq!(resolve_name("ts"), "tabstop");
        assert_eq!(resolve_name("et"), "expandtab");
        assert_eq!(resolve_name("ai"), "autoindent");
        assert_eq!(resolve_name("sw"), "shiftwidth");
        assert_eq!(resolve_name("hls"), "hlsearch");
        assert_eq!(resolve_name("ic"), "ignorecase");
        assert_eq!(resolve_name("is"), "incsearch");
    }

    #[test]
    fn test_resolve_unknown_passthrough() {
        assert_eq!(resolve_name("number"), "number");
        assert_eq!(resolve_name("unknown"), "unknown");
        assert_eq!(resolve_name("wrap"), "wrap");
    }

    #[test]
    fn test_empty_option_name_in_assign() {
        // :set =value should error
        assert!(parse_set_command("=value").is_err());
    }

    #[test]
    fn test_abbreviation_readonly() {
        assert_eq!(
            parse_set_command("ro"),
            Ok(SetCommand::Enable("readonly".to_string()))
        );
        assert_eq!(
            parse_set_command("noro"),
            Ok(SetCommand::Disable("readonly".to_string()))
        );
    }
}

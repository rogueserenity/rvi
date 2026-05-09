mod common;
use common::session::temp_file_with_content;

// Advanced substitute tests: backreferences, & replacement, :s///c confirm,
// magic / nomagic settings.

// --- Backreferences \(…\) / \1 ---

#[test]
fn test_substitute_backreference_swap_words() {
    // :s/\(hello\) \(world\)/\2 \1/ — swap two captured groups.
    let (_dir, path) = temp_file_with_content("hello world\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello world").unwrap();
    s.send_keys(":s/\\(hello\\) \\(world\\)/\\2 \\1/\r");
    s.wait_for_text("world hello")
        .expect(r":s with \(…\)/\1\2 should swap captured groups");
}

#[test]
fn test_substitute_backreference_single_group() {
    // :s/\(foo\)/[\1]/ — wrap the captured group in brackets.
    let (_dir, path) = temp_file_with_content("foo bar\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("foo bar").unwrap();
    s.send_keys(":s/\\(foo\\)/[\\1]/\r");
    s.wait_for_text("[foo] bar")
        .expect(r":s with \(…\)/[\1] should wrap captured text");
}

#[test]
fn test_substitute_backreference_global() {
    // :s/\(ab\)/(\1)/g — apply capture replacement to all occurrences.
    let (_dir, path) = temp_file_with_content("ab cd ab ef\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("ab cd ab ef").unwrap();
    s.send_keys(":s/\\(ab\\)/(\\1)/g\r");
    s.wait_for_text("(ab) cd (ab) ef")
        .expect(r":s with \(…\)/(\1)/g should replace all occurrences");
}

// --- & in replacement (whole match) ---

#[test]
fn test_substitute_ampersand_replacement() {
    // :s/foo/[&]/ — & inserts the whole matched text.
    let (_dir, path) = temp_file_with_content("foo bar\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("foo bar").unwrap();
    s.send_keys(":s/foo/[&]/\r");
    s.wait_for_text("[foo] bar")
        .expect(":s with & replacement should insert whole match in brackets");
}

#[test]
fn test_substitute_ampersand_global() {
    // :s/[aeiou]/(&)/g — wrap every vowel in parentheses.
    let (_dir, path) = temp_file_with_content("hello\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello").unwrap();
    s.send_keys(":s/[aeiou]/(&)/g\r");
    s.wait_for_text("h(e)ll(o)")
        .expect(":s with & global should wrap each matched character");
}

#[test]
fn test_substitute_escaped_ampersand_is_literal() {
    // :s/foo/\&/ — \& is a literal ampersand, not the whole match.
    let (_dir, path) = temp_file_with_content("foo bar\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("foo bar").unwrap();
    s.send_keys(":s/foo/\\&/\r");
    s.wait_for_text("& bar")
        .expect(r":s with \& should insert a literal ampersand");
}

// --- :s///c interactive confirm ---

#[test]
fn test_substitute_confirm_accept() {
    // :s/foo/bar/c — confirm prompt appears; pressing 'y' replaces.
    let (_dir, path) = temp_file_with_content("foo baz\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("foo baz").unwrap();
    s.send_keys(":s/foo/bar/c\r");
    // Wait for the confirm prompt.
    s.wait_for_text("replace with")
        .or_else(|_| s.wait_for_text("[y/n"))
        .expect(":s///c should show a confirm prompt");
    s.send_keys("y"); // accept
    s.wait_for_text("bar baz")
        .expect(":s///c with 'y' should perform the replacement");
}

#[test]
fn test_substitute_confirm_reject() {
    // :s/foo/bar/c — pressing 'n' skips the replacement.
    let (_dir, path) = temp_file_with_content("foo baz\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("foo baz").unwrap();
    s.send_keys(":s/foo/bar/c\r");
    s.wait_for_text("replace with")
        .or_else(|_| s.wait_for_text("[y/n"))
        .expect(":s///c should show a confirm prompt");
    s.send_keys("n"); // reject
                      // "foo" should still be present (not replaced).
    s.wait_for_text("foo baz")
        .expect(":s///c with 'n' should skip the replacement");
}

#[test]
fn test_substitute_confirm_accept_all() {
    // :s/x/y/gc — pressing 'a' accepts all remaining matches.
    let (_dir, path) = temp_file_with_content("x x x\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("x x x").unwrap();
    s.send_keys(":s/x/y/gc\r");
    s.wait_for_text("replace with")
        .or_else(|_| s.wait_for_text("[y/n"))
        .expect(":s///gc should show a confirm prompt");
    s.send_keys("a"); // accept all
    s.wait_for_text("y y y")
        .expect(":s///gc with 'a' should replace all matches");
}

// --- magic / nomagic settings ---

#[test]
fn test_search_nomagic_dot_is_literal() {
    // With :set nomagic, bare . in a search is literal, not a wildcard.
    let (_dir, path) = temp_file_with_content("a.b\naXb\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("a.b").unwrap();
    s.send_keys(":set nomagic\r");
    s.wait_for_status("NORMAL").unwrap();
    // /a.b should only match "a.b" literally, not "aXb".
    s.send_keys("/a.b\r");
    s.wait_for_cursor_row(0)
        .expect("nomagic: /a.b should match literal 'a.b' at row 0, not 'aXb'");
}

#[test]
fn test_search_nomagic_star_is_literal() {
    // With :set nomagic, bare * is literal.
    let (_dir, path) = temp_file_with_content("a*b\naab\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("a*b").unwrap();
    s.send_keys(":set nomagic\r");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys("/a*b\r");
    // Should land on "a*b" (row 0), not "aab" (which would match in magic mode).
    s.wait_for_cursor_row(0)
        .expect("nomagic: /a*b should match literal 'a*b'");
}

#[test]
fn test_search_magic_restored() {
    // :set magic restores the default regex behavior: . matches any char.
    let (_dir, path) = temp_file_with_content("aXb\na.b\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("aXb").unwrap();
    s.send_keys(":set nomagic\r");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys(":set magic\r");
    s.wait_for_status("NORMAL").unwrap();
    // Confirm cursor is at row 0 before searching, so the result is deterministic.
    // With magic on, /a.b matches both rows; starting from (0,0) the forward
    // search skips col 0 and finds "a.b" at row 1 first.
    s.wait_for_cursor_row(0).unwrap();
    s.send_keys("/a.b\r");
    s.wait_for_cursor_row(1)
        .expect("magic: /a.b should match 'a.b' at row 1 (. = any char matching X)");
}

// --- :~ command (repeat last replacement with most recent search pattern) ---

#[test]
fn test_tilde_repeats_replacement_with_new_search_pattern() {
    // :~ uses the most recent search pattern but the last :s replacement string.
    // Distinct from & which replays the entire last :s (both pattern and replacement).
    let (_dir, path) = temp_file_with_content("aaa and bbb\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("aaa and bbb").unwrap();
    // Step 1: substitute — sets last replacement to "xxx" and last :s pattern to "aaa"
    s.send_keys(":s/aaa/xxx/\r");
    s.wait_for_text("xxx and bbb").unwrap();
    // Step 2: plain search — updates last search pattern to "bbb" without touching :s
    s.send_keys("/bbb\r");
    s.wait_for_status("NORMAL").unwrap();
    // Step 3: :~ — should apply replacement "xxx" using most-recent pattern "bbb"
    s.send_keys(":~\r");
    s.wait_for_text("xxx and xxx")
        .expect(":~ should apply last replacement 'xxx' with most-recent search pattern 'bbb'");
}

#[test]
fn test_tilde_differs_from_ampersand() {
    // & repeats the entire last :s (same pattern + same replacement), so when
    // the pattern no longer matches it is a no-op. :~ uses the updated search
    // pattern instead and therefore does match.
    let (_dir, path) = temp_file_with_content("aaa and bbb\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("aaa and bbb").unwrap();
    s.send_keys(":s/aaa/xxx/\r");
    s.wait_for_text("xxx and bbb").unwrap();
    // Update last search pattern to "bbb" without another :s
    s.send_keys("/bbb\r");
    s.wait_for_status("NORMAL").unwrap();
    // & repeats :s/aaa/xxx/ — "aaa" is gone, so the line is unchanged
    s.send_keys("&");
    s.wait_for_text("xxx and bbb")
        .expect("& should repeat last :s with original pattern 'aaa', leaving 'bbb' unchanged");
}

#[test]
fn test_nomagic_backslash_dot_matches_any() {
    // With :set nomagic, \. is special (matches any char), so /a\.b matches
    // both "aXb" and "a.b". Start the cursor on row 1 ("a.b") so that a
    // forward search unambiguously wraps around to find "aXb" at row 0.
    let (_dir, path) = temp_file_with_content("aXb\na.b\n");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("aXb").unwrap();
    s.send_keys("j"); // move to row 1 ("a.b")
    s.wait_for_cursor_row(1).unwrap();
    s.send_keys(":set nomagic\r");
    s.wait_for_status("NORMAL").unwrap();
    s.send_keys("/a\\.b\r"); // wraps around; first match from row 1 forward is "aXb" at row 0
    s.wait_for_cursor_row(0)
        .expect("nomagic: /a\\.b should wrap around and match 'aXb' at row 0");
}

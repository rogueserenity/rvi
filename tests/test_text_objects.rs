mod common;
use common::session::temp_file_with_content;

// --- diw / daw — inner/around word ---

#[test]
fn test_diw_inner_word() {
    let (_dir, path) = temp_file_with_content("hello world");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello world").unwrap();
    s.send_keys("diw"); // delete inner word "hello"
    s.wait_for_no_text("hello")
        .expect("diw should delete the word under cursor");
    s.assert_contains("world");
}

#[test]
fn test_daw_around_word() {
    // daw deletes word + surrounding space
    let (_dir, path) = temp_file_with_content("one two three");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("one two three").unwrap();
    s.send_keys("w"); // move to "two"
    s.wait_for_cursor(0, 4).unwrap();
    s.send_keys("daw"); // delete "two " (word + trailing space)
    s.wait_for_no_text("two")
        .expect("daw should delete the word and surrounding space");
    s.assert_contains("one");
    s.assert_contains("three");
}

// --- ci" / di" — inside/around double quotes ---

#[test]
fn test_ci_double_quote() {
    let (_dir, path) = temp_file_with_content("say \"hello world\" now");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("say").unwrap();
    // Move cursor inside the quotes
    s.send_keys("f\"l"); // move to 'h' inside the quotes
    s.send_keys("ci\""); // change inside double quotes
    s.wait_for_status("INSERT").unwrap();
    s.send_keys("goodbye\x1b");
    s.wait_for_text("say \"goodbye\" now")
        .expect("ci\" should replace content inside double quotes");
}

#[test]
fn test_di_double_quote() {
    let (_dir, path) = temp_file_with_content("x = \"remove me\"");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("remove me").unwrap();
    s.send_keys("f\"l"); // move inside the quotes
    s.send_keys("di\""); // delete inside double quotes
    s.wait_for_no_text("remove me")
        .expect("di\" should delete the content inside double quotes");
    s.assert_contains("x = \"\""); // quotes remain, content gone
}

// --- di( / dib — inside parentheses ---

#[test]
fn test_di_paren() {
    let (_dir, path) = temp_file_with_content("foo(bar, baz)");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("foo(bar, baz)").unwrap();
    s.send_keys("f(l"); // move inside parens
    s.send_keys("di("); // delete inside parens
    s.wait_for_no_text("bar")
        .expect("di( should delete content inside parentheses");
    s.assert_contains("foo()");
}

// --- di{ / diB — inside curly braces ---

#[test]
fn test_di_brace() {
    let (_dir, path) = temp_file_with_content("fn { body }");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("fn { body }").unwrap();
    s.send_keys("f{l"); // move inside braces
    s.send_keys("di{"); // delete inside braces
    s.wait_for_no_text("body")
        .expect("di{ should delete content inside curly braces");
    s.assert_contains("fn {}");
}

// --- di[ — inside square brackets ---

#[test]
fn test_di_bracket() {
    let (_dir, path) = temp_file_with_content("arr[1, 2, 3]");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("arr[1, 2, 3]").unwrap();
    s.send_keys("f[l"); // move inside brackets
    s.send_keys("di["); // delete inside brackets
    s.wait_for_no_text("1, 2, 3")
        .expect("di[ should delete content inside square brackets");
    s.assert_contains("arr[]");
}

// --- diW / daW — inner/around WORD (whitespace-delimited) ---

#[test]
fn test_diw_inner_word_big() {
    // WORD treats "hello,world" as one unit (no punctuation boundary)
    let (_dir, path) = temp_file_with_content("hello,world foo");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("hello,world foo").unwrap();
    s.send_keys("diW"); // delete inner WORD "hello,world"
    s.wait_for_no_text("hello,world")
        .expect("diW should delete the WORD under cursor");
    s.assert_contains("foo");
}

#[test]
fn test_daw_around_word_big() {
    // daW deletes WORD + surrounding space
    let (_dir, path) = temp_file_with_content("one foo,bar three");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("one foo,bar three").unwrap();
    s.send_keys("w"); // move to "foo,bar"
    s.send_keys("daW"); // delete "foo,bar " (WORD + trailing space)
    s.wait_for_no_text("foo,bar")
        .expect("daW should delete the WORD and surrounding space");
    s.assert_contains("one");
    s.assert_contains("three");
}

// --- di< / da< / ci< / ca< — angle bracket objects ---

#[test]
fn test_di_angle_bracket() {
    let (_dir, path) = temp_file_with_content("<hello>");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("<hello>").unwrap();
    s.send_keys("f<l"); // move inside angle brackets
    s.send_keys("di<");
    s.wait_for_no_text("hello")
        .expect("di< should delete content inside angle brackets");
    s.assert_contains("<>");
}

#[test]
fn test_da_angle_bracket() {
    let (_dir, path) = temp_file_with_content("foo <bar> baz");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("foo <bar> baz").unwrap();
    s.send_keys("f<l"); // move inside angle brackets
    s.send_keys("da<");
    s.wait_for_no_text("<bar>")
        .expect("da< should delete angle brackets and content");
    s.assert_contains("foo");
    s.assert_contains("baz");
}

#[test]
fn test_ci_angle_bracket() {
    let (_dir, path) = temp_file_with_content("<old>");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("<old>").unwrap();
    s.send_keys("f<l"); // move inside angle brackets
    s.send_keys("ci<");
    s.wait_for_status("INSERT").unwrap();
    s.send_keys("new\x1b");
    s.wait_for_text("<new>")
        .expect("ci< should replace content inside angle brackets");
}

#[test]
fn test_ca_angle_bracket() {
    let (_dir, path) = temp_file_with_content("x <old> y");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("x <old> y").unwrap();
    s.send_keys("f<l"); // move inside angle brackets
    s.send_keys("ca<");
    s.wait_for_status("INSERT").unwrap();
    s.send_keys("new\x1b");
    s.wait_for_text("x new y")
        .expect("ca< should replace angle brackets and content");
}

// --- di` / da` / ci` / ca` — backtick objects ---

#[test]
fn test_di_backtick() {
    let (_dir, path) = temp_file_with_content("`hello`");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("`hello`").unwrap();
    s.send_keys("f`l"); // move inside backticks
    s.send_keys("di`");
    s.wait_for_no_text("hello")
        .expect("di` should delete content inside backticks");
    s.assert_contains("``");
}

#[test]
fn test_da_backtick() {
    let (_dir, path) = temp_file_with_content("foo `bar` baz");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("foo `bar` baz").unwrap();
    s.send_keys("f`l"); // move inside backticks
    s.send_keys("da`");
    s.wait_for_no_text("`bar`")
        .expect("da` should delete backticks and content");
    s.assert_contains("foo");
    s.assert_contains("baz");
}

#[test]
fn test_ci_backtick() {
    let (_dir, path) = temp_file_with_content("`old`");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("`old`").unwrap();
    s.send_keys("f`l"); // move inside backticks
    s.send_keys("ci`");
    s.wait_for_status("INSERT").unwrap();
    s.send_keys("new\x1b");
    s.wait_for_text("`new`")
        .expect("ci` should replace content inside backticks");
}

#[test]
fn test_ca_backtick() {
    let (_dir, path) = temp_file_with_content("x `old` y");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("x `old` y").unwrap();
    s.send_keys("f`l"); // move inside backticks
    s.send_keys("ca`");
    s.wait_for_status("INSERT").unwrap();
    s.send_keys("new\x1b");
    s.wait_for_text("x new y")
        .expect("ca` should replace backticks and content");
}

// --- yiw + put — yank inner word, put elsewhere ---

#[test]
fn test_yiw_and_put() {
    let (_dir, path) = temp_file_with_content("copy me here");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("copy me here").unwrap();
    s.send_keys("yiw"); // yank "copy"
    s.send_keys("$"); // move to last char 'e' (col 11 in "copy me here")
    s.wait_for_cursor(0, 11).unwrap();
    s.send_keys("p"); // paste after last char
    s.wait_for_text("copy me herecopy")
        .expect("yiw yanks inner word; p pastes it after last char");
}

// --- ciw — change inner word ---

#[test]
fn test_ciw() {
    let (_dir, path) = temp_file_with_content("old value here");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("old value here").unwrap();
    s.send_keys("ciw"); // change inner word "old"
    s.wait_for_status("INSERT").unwrap();
    s.send_keys("new\x1b");
    s.wait_for_text("new value here")
        .expect("ciw should replace the word under cursor");
}

// --- da' / da( / da[ / da{ — "around" delimiter variants ---

#[test]
fn test_da_single_quote() {
    let (_dir, path) = temp_file_with_content("x 'hello' y");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("x 'hello' y").unwrap();
    s.send_keys("f'l"); // move inside single quotes
    s.send_keys("da'");
    s.wait_for_no_text("'hello'")
        .expect("da' should delete single quotes and content");
    s.assert_contains("x");
    s.assert_contains("y");
}

#[test]
fn test_da_paren() {
    let (_dir, path) = temp_file_with_content("foo(bar)baz");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("foo(bar)baz").unwrap();
    s.send_keys("f(l"); // move inside parens
    s.send_keys("da(");
    s.wait_for_no_text("(bar)")
        .expect("da( should delete parentheses and content");
    s.assert_contains("foobaz");
}

#[test]
fn test_da_bracket() {
    let (_dir, path) = temp_file_with_content("arr[1,2]end");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("arr[1,2]end").unwrap();
    s.send_keys("f[l"); // move inside brackets
    s.send_keys("da[");
    s.wait_for_no_text("[1,2]")
        .expect("da[ should delete brackets and content");
    s.assert_contains("arrend");
}

#[test]
fn test_da_brace() {
    let (_dir, path) = temp_file_with_content("fn{body}rest");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("fn{body}rest").unwrap();
    s.send_keys("f{l"); // move inside braces
    s.send_keys("da{");
    s.wait_for_no_text("{body}")
        .expect("da{ should delete braces and content");
    s.assert_contains("fnrest");
}

// --- ci' — inside single quotes ---

#[test]
fn test_ci_single_quote() {
    let (_dir, path) = temp_file_with_content("key = 'value'");
    let mut s = common::RviSession::with_file(&path);
    s.wait_for_text("key = 'value'").unwrap();
    s.send_keys("f'l"); // move inside single quotes
    s.send_keys("ci'"); // change inside single quotes
    s.wait_for_status("INSERT").unwrap();
    s.send_keys("new\x1b");
    s.wait_for_text("key = 'new'")
        .expect("ci' should replace content inside single quotes");
}

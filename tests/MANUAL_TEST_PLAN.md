# rvi Manual Test Plan

Features that cannot be validated by the automated PTY integration tests.
Each section explains **why** automation is not feasible and gives precise
step-by-step reproduction steps.

Run these tests in a real terminal emulator (not in CI) before releases.

---

## 1. System Clipboard Registers (`"+` / `"*`)

**Why not automated:** The PTY tests run in an isolated subprocess with no
connection to the OS clipboard service. `arboard` (the clipboard library)
requires a real display session (X11, Wayland, or macOS Quartz).

### 1a. Copy to clipboard with `"+y`

1. Open a file with several lines of text: `rvi /tmp/clip_test.txt`
2. Position the cursor on a line with known content, e.g. `hello clipboard`.
3. Yank the line to the clipboard register: `"+yy`
4. Switch to another application (e.g. a text editor or browser address bar).
5. Paste with `Cmd-V` (macOS) or `Ctrl-V` (Linux).
6. **Expected:** `hello clipboard` is pasted.

### 1b. Paste from clipboard with `"+p`

1. Copy some text to the OS clipboard from another application.
2. Open rvi: `rvi /tmp/clip_test.txt`
3. In Normal mode press `"+p`.
4. **Expected:** The clipboard contents are inserted below the cursor.

### 1c. Primary selection register `"*` (Linux only)

1. Highlight text in another application using the mouse (X11 primary selection).
2. In rvi press `"*p`.
3. **Expected:** The highlighted text is inserted below the cursor.
4. Yank a line in rvi with `"*yy`, then middle-click in another X11 application.
5. **Expected:** The yanked text is pasted via primary selection.

---

## 2. `:sh` — Interactive Shell

**Why not automated:** `:sh` replaces the foreground process with an interactive
shell. The PTY test harness sends all keystrokes to rvi; there is no safe way
to detect "rvi has handed control to $SHELL" and resume rvi after `exit`. A
partially-automated test would be unreliable and can leave orphan processes.

### Steps

1. Open rvi on any file: `rvi /tmp/sh_test.txt`
2. Type `:sh` and press Enter.
3. **Expected:** The terminal drops to a shell prompt (e.g. `$` or `%`).
4. Run a command in the shell, e.g. `echo "shell works"`.
5. Type `exit` and press Enter.
6. **Expected:** rvi resumes with the screen redrawn exactly as it was before
   `:sh`.

---

## 3. Suspend / Resume (`:suspend` / `Ctrl-Z`)

**Why not automated:** SIGTSTP suspends the entire process group. In a PTY
harness the signal is either ignored (the child is not a foreground process
group leader) or it terminates rather than suspending. The `fg` shell built-in
is unavailable inside a Rust test.

### Steps

1. Open rvi in an interactive terminal: `rvi /tmp/suspend_test.txt`
2. Press `Ctrl-Z` (or type `:suspend` and Enter).
3. **Expected:** rvi is suspended; the shell prompt reappears with a message
   like `[1]+ Stopped rvi …`.
4. Run a command in the shell, e.g. `ls`.
5. Resume with `fg`.
6. **Expected:** rvi redraws the screen and resumes exactly where it was.
7. Repeat using `:su`, `:stop`, and `:st` — all should behave identically.

---

## 4. Terminal Bell (`errorbells`)

**Why not automated:** The bell character (`\x07`) is written to the PTY output
stream, but the vt100 screen-state parser used in tests does not expose whether
`\x07` was emitted. Verifying the bell requires either listening to audio output
or observing a visual bell in the terminal emulator.

### Steps

1. Open rvi: `rvi /tmp/bell_test.txt`
2. Enable error bells: `:set errorbells`
3. Trigger an error: press `l` at end of line, or try `:w` on a read-only file.
4. **Expected:** The terminal emulator produces an audible or visible bell.
5. Disable bells: `:set noerrorbells`
6. Trigger the same error again.
7. **Expected:** No bell.

---

## 5. Terminal Resize (SIGWINCH)

**Why not automated:** Changing the terminal dimensions mid-test requires
sending a `TIOCSWINSZ` ioctl and then SIGWINCH to the child process. While
technically scriptable, correctly verifying the resulting re-render (no
truncation, no stale rows, correct line count) requires visual inspection;
automated pixel-level screenshot diffing is out of scope.

### Steps

1. Open rvi on a file with 30+ lines: `rvi /tmp/resize_test.txt`
2. Observe the screen fully rendered.
3. Drag the terminal window to make it **narrower** (e.g. 40 columns).
4. **Expected:** rvi redraws immediately; lines wrap or truncate correctly;
   the status line remains at the bottom.
5. Resize **taller** (e.g. 50 rows).
6. **Expected:** Additional buffer lines become visible; status line moves to
   the new bottom row.
7. Resize back to the original size.
8. **Expected:** Display matches the original layout.

---

## 6. `incsearch` / `hlsearch` Color Highlighting

**Why not automated:** The PTY tests read cell *text* content. Color attributes
(SGR escape sequences) are tracked in the vt100 parser but asserting specific
RGB values or attribute combinations is fragile and terminal-dependent.
Correctness of the visual highlight appearance requires human judgment.

### 6a. `incsearch` — highlight as you type

1. Open a file with repeated occurrences of a word, e.g. `foo`.
2. `:set incsearch`
3. Press `/` and start typing `fo`.
4. **Expected:** The first match is highlighted (cursor jumps to it) as you type
   each character.
5. Type the full pattern `foo` and press Enter.
6. **Expected:** Cursor lands on the first `foo`; incremental highlight resolves
   to the final match position.

### 6b. `hlsearch` — persist all match highlights

1. `:set hlsearch`
2. Search for a word: `/foo` Enter.
3. **Expected:** Every occurrence of `foo` in the visible buffer is highlighted.
4. Press `n` to move to the next match; all highlights remain.
5. `:nohlsearch` (or `:noh`).
6. **Expected:** All highlights disappear without moving the cursor.

---

## 7. Wide Characters, CJK, and Emoji

**Why not automated:** Display-width correctness (cursor alignment, cell
occupancy) depends on the terminal emulator's own Unicode width tables.
Two-column wide characters (CJK ideographs, full-width forms) must be
visually verified to confirm that the cursor does not slip between the two
cells of a wide glyph.

### Steps

1. Create a test file:
   ```
   echo -e "abc\n日本語テスト\n emoji: 🎉 end\nzz" > /tmp/wide_test.txt
   ```
2. Open it: `rvi /tmp/wide_test.txt`
3. **Expected:** Each line displays without garbling; wide characters occupy
   exactly two terminal columns.
4. Navigate with `l` across the CJK line.
5. **Expected:** The cursor skips over the right half of each wide character
   (cursor never rests "inside" a wide glyph).
6. Press `x` on a wide character.
7. **Expected:** The entire glyph is deleted (not half of it), and the
   remainder of the line shifts left by two columns.
8. Open the emoji line; move the cursor over `🎉` with `l`.
9. **Expected:** Cursor advances by 2 columns (emoji is rendered as wide).

---

## 8. Crash Recovery (`-r` / `:preserve` / `:recover`)

**Why not automated:** A genuine crash test requires killing the editor process
mid-edit and then verifying recovery from a separately launched instance.
Coordinating two separate rvi processes with meaningful state is outside what
a single PTY test can reliably achieve, and the recovery file path
(`/tmp/{basename}.{pid}`) includes a non-deterministic PID.

### Steps

1. Open a file and make unsaved edits: `rvi /tmp/recover_test.txt`
2. Type some text in Insert mode, then press Escape (do **not** save).
3. Force-kill the editor from another terminal: `kill -9 $(pgrep rvi)`
4. Relaunch rvi with the recovery flag: `rvi -r /tmp/recover_test.txt`
5. **Expected:** rvi opens the preserved buffer containing the unsaved edits.
6. Save the file: `:w`
7. **Expected:** File on disk contains the recovered content.

### Bonus: `:preserve` smoke test (partial automation exists)

The automated suite verifies `:preserve` does not crash. Manual follow-up:
after `:preserve`, locate the temp file in `/tmp` and confirm it contains
the current buffer contents.

---

## 9. `-t {tagstring}` Startup Flag

**Why not automated:** The `-t` flag performs a tag jump immediately at
startup, before the editor enters its event loop. The automated tag
navigation tests cover in-session tag jumps; the startup-time variant
requires a correctly placed `tags` file in the working directory at launch.

### Steps

1. Create a tags environment:
   ```
   mkdir /tmp/tag_start && cd /tmp/tag_start
   echo -e "line one\nmy_func here\nline three" > src.txt
   echo -e "my_func\tsrc.txt\t2" > tags
   ```
2. Launch rvi with the `-t` flag:
   ```
   rvi -t my_func
   ```
3. **Expected:** rvi opens `src.txt` with the cursor on line 2 (`my_func here`),
   not at line 1.

---

## 10. Shell Command Execution

**Why not automated:** All three variants below hand off to a real shell
subprocess and interact with the terminal in cooked mode.  The PTY test harness
can send the keystrokes, but reliably detecting the "Press Enter to return"
prompt and the subsequent screen redraw is fragile across different shell
environments and command outputs.

### 10a. `:!{cmd}` — run command and return

1. Open rvi on any file.
2. Type `:!echo hello from shell` and press Enter.
3. **Expected:** The terminal clears, shows `hello from shell`, then prompts
   `Press Enter to continue`.
4. Press Enter.
5. **Expected:** rvi redraws the screen correctly.

### 10b. `:r !{cmd}` — insert command output into buffer

1. Open rvi on any file, cursor on line 1.
2. Type `:r !echo inserted line` and press Enter.
3. **Expected:** The text `inserted line` is inserted as a new line below
   the cursor.

### 10c. `!{motion}` — filter lines through shell

1. Open rvi on a file with several lines.
2. Position cursor on line 1, press `!!` (operator self-applied to current
   line) — the status bar should show a `!` prompt.
3. Type `sort` and press Enter.
4. **Expected:** The current line is replaced with the output of `sort` applied
   to it (for a single line this is unchanged; try `!jsort` on two lines to see
   them reordered).

---

## 11. `wrapmargin` — Automatic Line Breaking While Typing

**Why not automated:** `wrapmargin` inserts a newline automatically as you type
past the right-margin threshold.  Detecting the exact moment the editor inserts
that newline requires monitoring the buffer mid-keystroke sequence, which is not
reliably achievable in the PTY harness without race conditions.

### Steps

1. Open rvi on a new empty file: `rvi /tmp/wm_test.txt`
2. Enable wrapmargin: `:set wrapmargin=10`
3. Enter Insert mode: `i`
4. Type a long line of text that would exceed the right margin — e.g., type
   `the quick brown fox jumps over the lazy dog` without pressing Enter.
5. **Expected:** When the cursor comes within 10 columns of the right edge of
   the terminal, rvi automatically inserts a newline and continues the text on
   the next line.
6. Disable: `:set wrapmargin=0`
7. Type another long sentence in Insert mode.
8. **Expected:** No automatic line break is inserted.

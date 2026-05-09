# rvi — Feature Reference

rvi is a memory-safe vi clone written in Rust. This document describes every implemented feature,
organized by category.

- Features with no marker are standard POSIX vi behavior.
- Features marked **[vim]** are vim extensions not present in POSIX vi.
- Features marked **[rvi]** are rvi's own design choices, distinct from both vi and vim.

---

## Modes

| Mode | Entry | Description |
|------|-------|-------------|
| Normal | Default / `Esc` | Navigation and command mode |
| Insert | `i`, `I`, `a`, `A`, `o`, `O`, `s` | Text entry |
| Replace | `R` | Overwrite existing text character by character |
| Operator-Pending | After an operator (`d`, `y`, `c`, …) | Awaiting a motion or text object |
| Visual Character | `v` | Character-wise selection **[vim]** |
| Visual Line | `V` | Line-wise selection **[vim]** |
| Visual Block | `Ctrl-v` | Rectangular block selection **[vim]** |
| Command-Line | `:`, `/`, `?` | Ex command or search entry |

---

## Normal Mode — Motion Commands

### Character Motions

| Key | Action |
|-----|--------|
| `h` | Move left |
| `Backspace` / `Ctrl-h` | Move left (same as `h`) |
| `l` | Move right |
| `Space` | Move right (same as `l`) |
| `k` | Move up |
| `j` | Move down |

### Word Motions

| Key | Action |
|-----|--------|
| `w` | Forward to start of next word |
| `W` | Forward to start of next WORD (whitespace-delimited) |
| `b` | Backward to start of previous word |
| `B` | Backward to start of previous WORD |
| `e` | Forward to end of current/next word |
| `E` | Forward to end of current/next WORD |

### Line Motions

| Key | Action |
|-----|--------|
| `0` | Start of line |
| `^` | First non-blank character on line |
| `$` | End of line |
| `Enter` / `+` | First non-blank of next line |
| `-` | First non-blank of previous line |
| `_` | First non-blank of current line (`{n}_` goes to line n-1 below) |

### Document Motions

| Key | Action |
|-----|--------|
| `gg` | Beginning of file **[vim]** (vi uses `1G`) |
| `{count}gg` | Go to line `{count}` **[vim]** |
| `G` | End of file |
| `{count}G` | Go to line `{count}` |

### Character Search (within line)

| Key | Action |
|-----|--------|
| `f{char}` | Find `{char}` forward on current line |
| `F{char}` | Find `{char}` backward on current line |
| `t{char}` | Move to just before `{char}` forward |
| `T{char}` | Move to just after `{char}` backward |
| `;` | Repeat last `f`/`F`/`t`/`T` in same direction |
| `,` | Repeat last `f`/`F`/`t`/`T` in opposite direction |

### Bracket Matching

| Key | Action |
|-----|--------|
| `%` | Jump to matching bracket (`(`, `)`, `[`, `]`, `{`, `}`) |

### Paragraph and Sentence Motions

| Key | Action |
|-----|--------|
| `{` | Move to start of previous paragraph (blank-line or nroff macro from `paragraphs` setting) |
| `}` | Move to start of next paragraph |
| `(` | Move to start of previous sentence |
| `)` | Move to start of next sentence |

### Section Motions

| Key | Action |
|-----|--------|
| `[[` | Previous section start (`{`, form-feed, or nroff macro from `sections` setting) |
| `]]` | Next section start |

### Column Motion

| Key | Action |
|-----|--------|
| `{count}\|` | Move to screen column `{count}` (default: column 1) |

### Screen-Relative Motions

| Key | Action |
|-----|--------|
| `H` | Move to top line of screen (+ `{count}` offset) |
| `M` | Move to middle line of screen |
| `L` | Move to bottom line of screen (- `{count}` offset) |

All motions support a count prefix (e.g., `3w`, `5j`). All motions are valid targets for operators.

---

## Normal Mode — Editing Commands

### Insert Mode Entry

| Key | Action |
|-----|--------|
| `i` | Insert before cursor |
| `I` | Insert at first non-blank of line |
| `a` | Append after cursor |
| `A` | Append at end of line |
| `o` | Open new line below and insert |
| `O` | Open new line above and insert |
| `s` | Substitute character (delete + insert) |

### Operators (require motion or text object)

| Key | Action |
|-----|--------|
| `d` | Delete |
| `y` | Yank (copy) |
| `c` | Change (delete + enter Insert mode) |
| `<` | Decrease indent |
| `>` | Increase indent |
| `gU` | Convert to uppercase **[vim]** |
| `gu` | Convert to lowercase **[vim]** |
| `g~` | Toggle case **[vim]** |
| `gq` | Reformat / reflow text **[vim]** |
| `!` | Filter lines through shell command (prompts for command) |

Operators double as line-wise self-operations: `dd`, `yy`, `cc`, `>>`, `<<`, `gUU`, `guu`, `g~~`, `gqq`.

### Single-Key Editing

| Key | Action |
|-----|--------|
| `x` | Delete character under cursor |
| `X` | Delete character before cursor |
| `r{char}` | Replace character under cursor with `{char}` |
| `R` | Enter Replace mode |
| `~` | Toggle case of character under cursor and advance |
| `J` | Join current line with next (single space between) |
| `gJ` | Join current line with next without inserting space **[vim]** |
| `p` | Put (paste) after cursor |
| `P` | Put (paste) before cursor |
| `ZZ` | Write file (if modified) and quit — equivalent to `:x` |
| `ZQ` | Quit without saving — equivalent to `:q!` |
| `.` | Repeat last change |
| `&` | Repeat last `:substitute` on current line |
| `Ctrl-^` | Edit alternate file (most recently edited file) |
| `Q` | Enter ex mode (line-oriented): `:` prompt, read/execute commands in a loop; `:vi`/`:visual` or EOF returns to full-screen mode |

### Undo / Redo

| Key | Action |
|-----|--------|
| `u` | Undo last change |
| `U` | Restore current line to state when cursor arrived on it |
| `Ctrl-r` | Redo **[vim]** |

---

## Normal Mode — Search

| Key | Action |
|-----|--------|
| `/` | Enter forward search |
| `?` | Enter backward search |
| `n` | Repeat last search in same direction |
| `N` | Repeat last search in opposite direction |
| `*` | Search forward for word under cursor **[vim]** |
| `#` | Search backward for word under cursor **[vim]** |

- Searches wrap around end/start of file (controlled by `wrapscan`; default on).
- Empty `/` or `?` pattern reuses the last search pattern.
- `n`/`N` always reuse last pattern.

---

## Normal Mode — Scrolling

| Key | Action |
|-----|--------|
| `Ctrl-f` | Scroll forward one full screen (keeps 2-line overlap) |
| `Ctrl-b` | Scroll backward one full screen (keeps 2-line overlap) |
| `Ctrl-d` | Scroll down half screen (`{count}` sets new half size) |
| `Ctrl-u` | Scroll up half screen (`{count}` sets new half size) |
| `Ctrl-e` | Scroll down one line, cursor stays on screen |
| `Ctrl-y` | Scroll up one line, cursor stays on screen |

---

## Normal Mode — View Positioning

| Key | Action |
|-----|--------|
| `z↵` (`z` + Enter) | Redraw with cursor line at top of screen |
| `z.` | Redraw with cursor line at center of screen |
| `z-` | Redraw with cursor line at bottom of screen |
| `zt` | Same as `z↵` **[vim]** |
| `zz` | Same as `z.` **[vim]** |
| `zb` | Same as `z-` **[vim]** |

---

## Normal Mode — Miscellaneous

| Key | Action |
|-----|--------|
| `Ctrl-g` | Display file name, line count, position percentage |
| `Ctrl-l` | Redraw the screen |
| `"{reg}` | Select register `{reg}` for next operation |
| `Ctrl-o` | Jump to older position in jump list **[vim]** |
| `Ctrl-i` / `Tab` | Jump to newer position in jump list **[vim]** |
| `Ctrl-]` | Jump to tag under cursor (looks up `./tags` file); pushes to tag stack |
| `Ctrl-t` | Pop tag stack — return to position before last `Ctrl-]` / `:tag` jump |

---

## Marks

| Key | Action |
|-----|--------|
| `m{a-z}` | Set mark `{a-z}` at cursor position |
| `` `{a-z} `` | Jump to exact position (row and column) of mark `{a-z}` |
| `'{a-z}` | Jump to first non-blank of mark `{a-z}`'s line |
| ` `` ` | Jump to exact position before last jump |
| `''` | Jump to first non-blank of line before last jump |
| `'<` | Start of last visual selection (set automatically on leaving visual mode) |
| `'>` | End of last visual selection (set automatically on leaving visual mode) |

Marks are also usable as ex command range addresses: `'a,'bd` deletes from mark `a` to mark `b`.
`'<,'>` addresses the last visual selection.

---

## Registers

### Named Registers

- `"a` – `"z`: 26 named registers for explicit storage.
- `"A` – `"Z`: Uppercase names **append** to the corresponding lowercase register.

### Automatic Registers

| Register | Contents |
|----------|----------|
| `""` | Unnamed: last delete, change, or yank |
| `"0` | Yank register: last explicit yank |
| `"1`–`"9` | Numbered delete history: `"1` = most recent line-wise delete, rotates down |
| `"-` | Small delete: most recent delete shorter than one line |

### Special Registers

| Register | Contents |
|----------|----------|
| `"_` | Black hole: discards all writes; reads return empty |
| `"+` | System clipboard **[vim]** |
| `"*` | Primary selection (macOS: same as clipboard) **[vim]** |
| `"/` | Last search pattern (read-only) **[vim]** |
| `"%` | Current filename (read-only) **[vim]** |
| `"#` | Alternate (previously edited) filename (read-only) **[vim]** |

---

## Text Objects [vim]

All text objects work with operators (`d`, `y`, `c`, `gU`, etc.) and in Visual mode.

### Word Objects

| Object | Inner | Around |
|--------|-------|--------|
| word | `iw` | `aw` |
| WORD | `iW` | `aW` |

### Quote Objects

| Object | Inner | Around |
|--------|-------|--------|
| double quotes | `i"` | `a"` |
| single quotes | `i'` | `a'` |
| backticks | `` i` `` | `` a` `` |

### Bracket/Delimiter Objects

| Object | Inner | Around |
|--------|-------|--------|
| parentheses `()` | `ib` / `i(` | `ab` / `a(` |
| square brackets `[]` | `i[` / `i]` | `a[` / `a]` |
| curly braces `{}` | `iB` / `i{` | `aB` / `a{` |
| angle brackets `<>` | `i<` / `i>` | `a<` / `a>` |

---

## Visual Mode [vim]

### Entry

| Key | Mode |
|-----|------|
| `v` | Character-wise |
| `V` | Line-wise |
| `Ctrl-v` | Block (rectangular) |

### Operations in Visual Mode

| Key | Action |
|-----|--------|
| `d` | Delete selection |
| `y` | Yank selection |
| `c` | Change selection |
| `~` | Toggle case |
| `<` | Decrease indent |
| `>` | Increase indent |
| `gU` | Uppercase **[vim]** |
| `gu` | Lowercase **[vim]** |
| `:` | Enter ex command for selected range |

---

## Insert Mode

### Special Key Bindings

| Key | Action |
|-----|--------|
| `Esc` | Return to Normal mode |
| `Backspace` / `Ctrl-H` | Delete character before cursor |
| `Delete` | Delete character at cursor (forward delete) |
| `Enter` | Insert newline |
| `Ctrl-w` | Delete word before cursor |
| `Ctrl-u` | Delete to start of line |
| `Ctrl-v` | Insert next character literally (e.g., `Ctrl-v Ctrl-c` inserts `^C`) |
| `Ctrl-r{reg}` | Insert contents of register `{reg}` at cursor **[vim]** |
| `Ctrl-t` | Indent current line by `shiftwidth` |
| `Ctrl-d` | Dedent current line by `shiftwidth` |
| `Ctrl-@` / `NUL` | Re-insert last inserted text and return to Normal mode |
| `0 Ctrl-d` | Delete all indentation, reset autoindent level |
| `^ Ctrl-d` | Delete all indentation this line only, restore on next |

---

## Macros

| Key | Action |
|-----|--------|
| `q{a-z}` | Start recording macro into register `{a-z}` |
| `q` | Stop recording |
| `@{a-z}` | Play back macro from register `{a-z}` |
| `@@` | Repeat last macro playback **[vim]** |

---

## Ex Commands (Command-Line Mode)

### File Commands

| Command | Action |
|---------|--------|
| `:w` | Write (save) current file |
| `:w {file}` | Write to `{file}` |
| `:wq` / `:x` / `:exit` | Write and quit |
| `:wq!` | Force write and quit |
| `:q` | Quit (fails if unsaved changes) |
| `:q!` | Force quit (discard changes) |
| `:vi` / `:visual` | Exit ex mode and return to full-screen visual mode |
| `:pre[serve]` | Write buffer to crash-recovery temp file (`/tmp/{basename}.{pid}`) |
| `:rec[over] [file]` | Reload buffer from crash-recovery temp file |
| `:ta[g] {name}` | Jump to tag `{name}` (looks up `./tags` file); pushes to tag stack |
| `:po[p]` | Pop tag stack — return to position before last `:tag` / `Ctrl-]` jump |
| `:e {file}` | Edit `{file}` |
| `:e!` | Re-read current file, discarding changes |
| `:r {file}` | Read `{file}` and insert below cursor |
| `:r !{cmd}` | Run `{cmd}` and insert its stdout below cursor |
| `:!{cmd}` | Execute shell command; output shown, press Enter to return |
| `:sh` | Start interactive shell; resume editor on exit |
| `:cd {dir}` / `:chdir {dir}` | Change working directory |
| `:source {file}` / `:so {file}` | Execute ex commands from file |
| `:mark {a}` / `:ma {a}` / `:k{a}` | Set mark at current line |
| `:suspend` / `:su` / `:stop` / `:st` | Suspend editor (SIGTSTP); resume with `fg` |
| `:[addr]a[ppend]` | Collect lines after addr; terminate with `.` alone |
| `:[addr]i[nsert]` | Collect lines before addr; terminate with `.` alone |
| `:[range]c[hange]` | Delete range, then collect replacement lines |
| `:version` / `:ve` | Display editor version string |

### Multi-File Navigation

| Command | Action |
|---------|--------|
| `:n` / `:next` | Go to next file in argument list |
| `:n {file}` | Edit `{file}` and set as new arg list |
| `:N` / `:prev` / `:previous` | Go to previous file in argument list |
| `:args` / `:ar` | Display argument list |
| `:rewind` / `:rew` | Return to first file in argument list |

Multiple files can be opened at launch: `rvi file1 file2 file3`.

### Filename Expansion

In filename arguments (`:w`, `:e`, `:r`, `:wq`, `:n`, `:so`, etc.):

| Token | Expands to |
|-------|-----------|
| `%` | Current filename |
| `#` | Alternate (previously edited) filename |
| `\%` / `\#` | Literal `%` / `#` (escaped) |

Example: `:w %.bak` writes a copy with `.bak` appended; `:e #` re-opens the alternate file.

### Line Addressing

Ranges can be specified before most commands:

| Address | Meaning |
|---------|---------|
| `{n}` | Line `n` (1-based) |
| `.` | Current line |
| `$` | Last line |
| `%` | Entire file (equivalent to `1,$`) |
| `{a},{b}` | Lines `a` through `b` |
| `{a};{b}` | Lines `a` through `b`, with `{b}` resolved relative to `{a}` (e.g. `1;/foo/` finds `/foo/` from line 1, not the cursor) |
| `'a` | Line marked with mark `a` |
| `/{pattern}/` | Next line matching `{pattern}` (wraps around) |
| `?{pattern}?` | Previous line matching `{pattern}` (wraps around) |
| `//` | Next line matching most recent search pattern |
| `??` | Previous line matching most recent search pattern |
| `{addr}+{n}` | `{addr}` plus `n` lines |
| `{addr}-{n}` | `{addr}` minus `n` lines |
| `{addr}+` | `{addr}` plus 1 (bare `+`) |
| `{addr}-` | `{addr}` minus 1 (bare `-`) |

### Substitute

```vim
:[range]s/{pattern}/{replacement}/[flags]
```

| Flag | Meaning |
|------|---------|
| `g` | Replace all occurrences on each line (not just first) |
| `i` | Case-insensitive matching |
| `c` | Confirm each substitution interactively (`y`=yes, `n`=no, `a`=all, `q`=quit, `l`=last) |
| `p` | Print the last substituted line after the command |
| `l` | Print the last substituted line in list format (tabs as `^I`, `$` at end) |
| `#` | Print the last substituted line with its line number |

- Empty pattern (`:s//replacement/`) reuses the last search pattern.
- Vi regex syntax is supported (including `\(…\)` groups, `\1`–`\9` back-references).
- `:[range]~` repeats the last replacement string with the most recent search pattern (differs from `&` which repeats the entire last `:s` command).

### Line Editing Commands

| Command | Action |
|---------|--------|
| `:[range]d [reg]` | Delete lines (optionally into register) |
| `:[range]y [reg]` | Yank lines (optionally into register) |
| `:[addr]pu[t] [reg]` | Put register contents as new lines after `{addr}` (default: current line) |
| `:[range]m {addr}` | Move lines to after `{addr}` |
| `:[range]t {addr}` | Copy (transfer) lines to after `{addr}` |
| `:[range]j` | Join lines in range |
| `:[range]p` | Print lines to terminal |
| `:[range]l` | Print lines in list format (tabs as `^I`, `$` at end of each line) |
| `:[range]nu` / `:[range]number` / `:[range]#` | Print lines with line numbers |
| `:=` / `:[range]=` | Print line number of last line in range |

### Global Command

| Command | Action |
|---------|--------|
| `:g/{pattern}/{cmd}` | Execute any ex command on lines matching `{pattern}` |
| `:v/{pattern}/{cmd}` | Execute `{cmd}` on lines **not** matching `{pattern}` |
| `:g!/{pattern}/{cmd}` | Same as `:v` |

Any ex command may follow `g/pattern/` — including `:d`, `:p`, `:y`, `:m`, `:t`, `:s`, `:a`, `:i`, `:c`, `:r`, `:w`, etc. The cursor is moved to each matching line before executing the sub-command, so commands using `.` (the current line) work correctly.

### Navigation

| Command | Action |
|---------|--------|
| `:{n}` | Go to line `n` |

### Key Mappings

| Command | Action |
|---------|--------|
| `:map {lhs} {rhs}` | Map key `{lhs}` to `{rhs}` in Normal mode |
| `:map! {lhs} {rhs}` | Map key `{lhs}` to `{rhs}` in Insert mode |
| `:unmap {lhs}` | Remove Normal mode mapping |
| `:unmap! {lhs}` | Remove Insert mode mapping |
| `:map` / `:map!` | Show all current mappings |

Key notation: `<CR>`, `<Esc>`, `<BS>`, `<C-x>` (e.g., `:map ,w :w<CR>`).

### Abbreviations

| Command | Action |
|---------|--------|
| `:ab {lhs} {rhs}` | Define insert-mode abbreviation |
| `:abbreviate {lhs} {rhs}` | Same (long form) |
| `:una {lhs}` | Remove abbreviation |
| `:unabbreviate {lhs}` | Same (long form) |
| `:ab` / `:abbreviate` | Show all abbreviations |

Abbreviations expand when a non-keyword character (space, punctuation) is typed or
when `Esc`/`Enter` is pressed immediately after the lhs word. Only the word directly
before the cursor is checked; typing more keyword characters after the lhs prevents
expansion (e.g., `foo` is an abbrev but `food` is not expanded until the `d` is
followed by a non-keyword char).

### Settings

| Command | Action |
|---------|--------|
| `:set {option}` | Enable boolean option |
| `:set no{option}` | Disable boolean option |
| `:set {option}!` | Toggle boolean option |
| `:set {option}?` | Query current value |
| `:set {option}={value}` | Set option to value |
| `:set` / `:set all` | Show all option values |
| `:nohl` / `:nohlsearch` | Clear search highlights until next search **[vim]** |

### Supported Options

| Option | Short | Type | Default | Description |
|--------|-------|------|---------|-------------|
| `number` | `nu` | bool | off | Show line numbers |
| `wrap` | — | bool | on | Wrap long lines |
| `autoindent` | `ai` | bool | off | Copy indent from previous line on `Enter` |
| `tabstop` | `ts` | number | 8 | Display width of a tab character |
| `shiftwidth` | `sw` | number | 8 | Spaces per indent step (`<`/`>`) |
| `wrapmargin` | `wm` | number | 0 | Auto-insert newline when typing within N columns of right margin |
| `textwidth` | `tw` | number | 0 | Maximum line width for `gq` reformat (`0` = use `wrapmargin`; both `0` = no-op) **[vim]** |
| `report` | — | number | 5 | Min lines changed before showing a count message (`0` = always) |
| `list` | — | bool | off | Display tabs as `^I` and append `$` at end of each line |
| `autowrite` | `aw` | bool | off | Auto-save before `:e`, `:next`, `:rewind`, `:q`, `:!`, `Ctrl-^` |
| `readonly` | `ro` | bool | off | Block `:w` (use `:w!` to force); set by `-R` flag |
| `magic` | — | bool | on | `.`, `*`, `[`, `~` are regex metacharacters; `nomagic` makes only `^`/`$` special |
| `expandtab` | `et` | bool | off | Use spaces instead of tab characters **[vim]** |
| `hlsearch` | `hls` | bool | off | Highlight all search matches **[vim]** |
| `ignorecase` | `ic` | bool | off | Case-insensitive search |
| `incsearch` | `is` | bool | off | Incremental search highlighting as you type **[vim]** |
| `wrapscan` | `ws` | bool | on | Wrap searches around end/beginning of buffer |
| `terse` | `te` | bool | off | Suppress verbose messages (line counts after edits, etc.) |
| `scroll` | `scr` | number | 0 | Lines scrolled by `Ctrl-d`/`Ctrl-u` (`0` = half viewport height) |
| `errorbells` | `eb` | bool | off | Ring terminal bell (`\x07`) before error messages |

---

## Search Features

- Forward search with `/`, backward with `?`.
- Searches wrap around end/start of file (controlled by `wrapscan`; default on). When `nowrapscan`, shows E384/E385 instead of wrapping.
- `n` / `N` repeat in same / opposite direction.
- `*` / `#` search for word under cursor (forward / backward). **[vim]**
- `hlsearch`: highlights all matches after a successful search. **[vim]**
- `incsearch`: jumps to first match as you type. **[vim]**
- `ignorecase`: case-insensitive matching.
- `magic` (default on): `.`, `*`, `[`, `~` are special. `nomagic` makes only `^`/`$` special.
- Empty pattern reuses last search pattern.
- Vi extended regex syntax (including `\(…\)` capture groups and `\1`–`\9` back-references).

---

## Startup Flags

| Flag | Description |
|------|-------------|
| `-R` | Open in read-only mode (sets `readonly` option) |
| `-t {tagstring}` | Look up `{tagstring}` in `./tags` and open its file at its address |
| `-r` | List all recoverable files in `/tmp` and exit |
| `-r {file}` | Recover `{file}` from its crash-recovery temp file |
| `-c {cmd}` | Execute ex command `{cmd}` after the first file is loaded |
| `+{cmd}` | Execute ex command `{cmd}` after the first file is loaded |
| `+{N}` | Go to line N after the first file is loaded (shorthand for `-c {N}`) |
| `+` | Go to the last line after the first file is loaded |

---

## Startup Configuration

### `EXINIT` environment variable

rvi reads the `EXINIT` environment variable on startup and executes its contents as ex
commands. Segments are separated by `|` or newlines. Lines starting with `"` are comments.

```sh
EXINIT='set number|set ignorecase' rvi file.txt
```

**Security restriction:** Shell commands (`:!{cmd}`, `:sh`, `:r !{cmd}`) and `:source` are
blocked when sourced from `EXINIT`. All other ex commands (`:set`, `:map`, line jumps, etc.)
work normally. This prevents a manipulated environment variable from executing arbitrary code.

**Residual risk — `:map`:** Key mappings are allowed because blocking them would break the
most common legitimate use of EXINIT. A mapping like `:map ,x :!evil<CR>` does not execute
on startup — it only fires if the user subsequently presses the mapped key. Users who share
or inherit shell environments from untrusted sources should audit their `EXINIT` value.

### XDG config file

rvi loads ex commands from `$XDG_CONFIG_HOME/rvi/config` on startup (falling back to
`~/.config/rvi/config`), after `EXINIT`. This follows the XDG Base Directory Specification.
Traditional vi reads `~/.exrc`; vim reads `~/.vimrc`. rvi's XDG path is its own convention.

Each line is executed as an ex command. Lines starting with `"` are comments (vi convention).

Example `~/.config/rvi/config`:

```vim
" My rvi config
set number
set autoindent
set expandtab
set tabstop=4
set shiftwidth=4
```

---

## File Handling

- UTF-8 encoding with BOM detection and stripping.
- Line ending auto-detection: LF, CRLF, CR — stored internally as LF, written back in original convention.

---

## Rendering

- Only visible lines are rendered (efficient for large files).
- Full Unicode support: multi-byte characters, grapheme clusters, wide characters (CJK, emoji).
- Status line shows: mode, file name, modified flag, cursor position.
- `Ctrl-g` shows: filename, modified status, total lines, position percentage, current line number.
- Line numbers with `:set number`.
- Configurable line wrapping with `:set wrap`.
- Search match highlighting with `:set hlsearch`.

---

## vim Extensions Implemented

These features are not part of POSIX vi but are common enough to be expected:

| Feature | Description |
|---------|-------------|
| `Ctrl-r` redo | Linear redo stack (POSIX vi `Ctrl-r` is redraw; vim repurposed it for redo) |
| `Ctrl-o` / `Ctrl-i` jump list | Navigate backward/forward through jump history |
| Visual mode (`v`, `V`, `Ctrl-v`) | Character, line, and block selection |
| Text objects (`iw`, `a"`, `i{`, …) | Structured selection targets |
| `*` / `#` word search | Search for word under cursor |
| `gU` / `gu` / `g~` | Case conversion operators |
| `gg` | Go to first line (vi uses `1G`) |
| `gJ` | Join lines without inserting a space |
| `gq` reformat operator | Reflow text to fit `textwidth` (or `wrapmargin`) columns |
| `@@` macro repeat | Repeat last macro without naming the register |
| Insert `Ctrl-r{reg}` | Insert register contents without leaving insert mode |
| `hlsearch` / `incsearch` | Search highlighting and incremental search |
| `expandtab` | Use spaces instead of tab characters |
| System clipboard (`"+`, `"*`) | Integration with OS clipboard |
| Read-only virtual registers (`"/`, `"%`, `"#`) | Last search, filename, alternate file |
| `zz` / `zt` / `zb` shorthand | Viewport positioning aliases (vi uses `z.`, `z↵`, `z-`) |

---

## rvi-Specific Features

These are rvi's own design choices, distinct from both POSIX vi and vim:

| Feature | Description |
|---------|-------------|
| XDG config file | `~/.config/rvi/config` (respects `$XDG_CONFIG_HOME`). Vi uses `~/.exrc`; vim uses `~/.vimrc`. |
| Memory safety | Written in Rust with `#![forbid(unsafe_code)]` — no unsafe memory operations |

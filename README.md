# rvi

[![CI](https://img.shields.io/github/actions/workflow/status/rogueserenity/rvi/ci.yml?branch=main)](https://github.com/rogueserenity/rvi/actions)
[![License](https://img.shields.io/badge/license-GPL--3.0-blue)](LICENSE)

A memory-safe vi clone written in Rust.

rvi aims for faithful POSIX vi compatibility with select vim extensions, enforcing `#![forbid(unsafe_code)]` throughout.

## Installation

### mise (recommended)

```toml
# .mise.toml
[tools]
"ubi:rogueserenity/rvi" = "latest"
```

Or install globally:

```bash
mise use -g "ubi:rogueserenity/rvi"
```

### Pre-built binaries

Download the latest binary for your platform from [GitHub Releases](https://github.com/rogueserenity/rvi/releases/latest):

| Platform | Architecture | File |
|----------|--------------|------|
| macOS | Apple Silicon | `rvi-aarch64-apple-darwin` |
| Linux | x86_64 | `rvi-x86_64-unknown-linux-gnu` |

### From source

```bash
git clone https://github.com/rogueserenity/rvi
cd rvi
cargo install --path .
```

## Usage

```bash
rvi [OPTIONS] [FILE]...
```

```bash
# Open a file
rvi file.txt

# Open multiple files
rvi file1.txt file2.txt file3.txt

# Open read-only
rvi -R file.txt

# Jump to line 42
rvi +42 file.txt

# Open at a tag
rvi -t my_function
```

## Features

- **POSIX vi compatible** — motions, operators, ex commands, marks, registers, macros
- **vim extensions** — visual mode, text objects, `gg`, `Ctrl-r`, jump list, `hlsearch`, `incsearch`, system clipboard
- **Unicode** — full grapheme cluster support, wide character display (CJK, emoji)
- **Memory safe** — `#![forbid(unsafe_code)]`, written in pure safe Rust
- **XDG config** — reads `~/.config/rvi/config` on startup

See [FEATURES.md](FEATURES.md) for the complete feature reference.

## Configuration

rvi loads ex commands from `~/.config/rvi/config` on startup (respects `$XDG_CONFIG_HOME`).

```vim
" ~/.config/rvi/config
set number
set autoindent
set expandtab
set tabstop=4
set shiftwidth=4
```

Settings can also be passed via the `EXINIT` environment variable:

```bash
EXINIT='set number|set ignorecase' rvi file.txt
```

## Startup Flags

| Flag | Description |
|------|-------------|
| `-R` | Read-only mode |
| `-t {tag}` | Open file at tag |
| `-r [file]` | Recover from crash |
| `-c {cmd}` | Execute ex command after load |
| `+{N}` | Go to line N after load |

## License

Licensed under [GPL-3.0](LICENSE).

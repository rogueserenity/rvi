# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

rvi is a memory-safe vi clone written in Rust. The project enforces `#![forbid(unsafe_code)]` to ensure complete memory safety.

## Build and Test Commands

```bash
cargo build              # Build the project
cargo run                # Run the editor
cargo test               # Run all tests
cargo test <test_name>   # Run a specific test
cargo clippy             # Lint with clippy
cargo fmt                # Format code
```

## Architecture

The editor uses a modular architecture separating terminal I/O, buffer management, and rendering concerns. Currently implemented modules:

### Module Structure

- **`src/lib.rs`**: Library root with `#![forbid(unsafe_code)]`, module declarations
- **`src/main.rs`**: Binary entry point that calls `rvi::run()`

### terminal/ - Terminal I/O
- `raw_mode.rs`: Raw terminal mode management
- `input.rs`: Keyboard input capture and key parsing
- `output.rs`: Terminal output operations (cursor movement, clearing)
- `size.rs`: Terminal size detection

### buffer/ - Text Buffer Management
- `mod.rs`: `Buffer` struct storing text as `Vec<String>` (line-based storage)
- `cursor.rs`: `Cursor` struct with `row: usize, col: usize` (byte offsets), grapheme-aware movement methods
- `operations.rs`: Insert/delete operations
- `selection.rs`: `Selection` struct with `SelectionType` enum
- `unicode.rs`: Grapheme iteration and display width utilities

### editor/ - Editor Core
- `mod.rs`: `Editor`, `Document`, `EditorState` structs, main event loop
- `movement.rs`: Cursor movement methods (hjkl, 0, $, etc.)
- `editing.rs`: Text editing operations (insert, delete, open line)
- `key_handlers.rs`: Key event handling per mode

### mode/ - Editor Modes
- `mod.rs`: `Mode` enum (Normal, Insert, OperatorPending, Visual, CommandLine), `Operator` enum, `VisualKind` enum

### render/ - Display Rendering
- `mod.rs`: Main `Renderer` struct coordinating all rendering
- `viewport.rs`: `Viewport` struct tracking visible portion of buffer
- `buffer_render.rs`: Renders buffer content to terminal
- `cursor_render.rs`: Cursor positioning (byte offset to display column)
- `status_line.rs`: Status line rendering

## Key Design Decisions

1. **Byte-based positions**: Cursor positions are byte offsets (like vi), with grapheme-aware operations when needed for display
2. **Line-based storage**: Text stored as `Vec<String>` for efficient line operations
3. **Unicode handling**: Uses `unicode-segmentation` for grapheme operations, `unicode-width` for display width
4. **Error handling**: Uses `thiserror` for error types, all I/O returns `Result<T, E>`. Forward-looking error types defined: `CommandError`, `FileError`, `SearchError`, `SettingsError`, `UndoError`
5. **Viewport rendering**: Only renders visible lines for performance

## Dependencies

- `termion`: Terminal I/O (raw mode, input, output)
- `fancy-regex`: Regex with backreferences for vi compatibility
- `unicode-width`: Character display width calculation
- `unicode-segmentation`: Grapheme cluster handling
- `thiserror`: Error type derivation
- `arboard`: System clipboard integration (registers `+` and `*`)
- `nix`: Unix signal handling (SIGTSTP/Ctrl-Z suspend)

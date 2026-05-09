#![forbid(unsafe_code)]

pub mod buffer;
pub mod command;
pub mod editor;
pub mod error;
pub mod file;
pub mod mode;
pub mod registers;
pub mod render;
pub mod search;
pub mod settings;
pub mod tags;
pub mod terminal;
pub mod undo;

pub use editor::Editor;
pub use error::{Error, Result};
pub use mode::{Mode, Operator, VisualKind};
pub use registers::{ContentType, RegisterContent, RegisterId, Registers};
pub use search::{SearchDirection, SearchMatch, SearchState};
pub use settings::{SetResult, Settings};
pub use undo::{EditAction, UndoEntry, UndoHistory};

/// Main entry point for the editor.
pub fn run() -> Result<()> {
    let mut editor = Editor::new()?;
    editor.run()
}

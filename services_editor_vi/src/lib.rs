//! # VI-like Editor Service
//!
//! This crate implements a modal text editor for PandaGen OS.
//!
//! ## Philosophy
//!
//! - **Modal editing**: Normal, Insert, Command modes
//! - **Capability-based**: No ambient file access; documents opened via capabilities
//! - **Versioned saves**: Creating new immutable versions instead of overwriting
//! - **Testable**: Fully testable with injected keyboard events under SimKernel
//! - **No terminal emulation**: Events are typed, not byte streams
//!
//! ## Non-Goals
//!
//! This is NOT:
//! - A POSIX terminal / TTY emulation
//! - A port of real vi/vim
//! - A full-featured editor with syntax highlighting
//! - Scriptable or pluggable
//!
//! ## Design
//!
//! - Editor operates on text buffers via capabilities
//! - Input arrives as structured KeyEvent messages
//! - Saves create new object versions
//! - Directory link updates are separate operations requiring write authority

pub mod commands;
pub mod editor;
pub mod io;
pub mod render;
pub mod state;

pub use commands::CommandParser;
pub use editor::{Editor, EditorAction, EditorError};
pub use io::{DocumentHandle, EditorIo, OpenOptions, OpenResult, SaveResult, StorageEditorIo};
pub use render::EditorView;
pub use state::{Cursor, EditorMode, EditorState, Position};

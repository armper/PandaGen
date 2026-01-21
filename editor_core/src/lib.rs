#![no_std]

//! # Editor Core
//!
//! Shared editor logic for both simulation and bare-metal environments.
//!
//! ## Philosophy
//!
//! - **No_std compatible**: Uses alloc but not std
//! - **Deterministic**: Same input trace => same editor state
//! - **Modal editing**: Normal, Insert, Command, Search modes
//! - **Mechanism over policy**: Core provides editing primitives, hosts decide rendering
//! - **No ambient authority**: IO requests are explicit, never automatic
//!
//! ## Design
//!
//! The core provides:
//! - EditorCore: State machine for modal editing
//! - CoreOutcome: Structured results from operations
//! - EditorSnapshot: Deterministic state for parity testing
//! - Key event abstraction: Platform-independent input representation

extern crate alloc;

pub mod buffer;
pub mod command;
pub mod core;
pub mod key;
pub mod mode;
pub mod snapshot;

pub use buffer::{Position, TextBuffer};
pub use command::{Command, CommandOutcome};
pub use core::{CoreIoRequest, CoreOutcome, EditorCore};
pub use key::Key;
pub use mode::EditorMode;
pub use snapshot::EditorSnapshot;

//! # PandaGen Host Runtime
//!
//! This crate provides the host runtime for PandaGen OS.
//!
//! ## Philosophy
//!
//! - **Host owns I/O**: Components never print
//! - **Output is snapshot rendering**: Not terminal state
//! - **Input is explicit events**: Not stdin streams
//! - **Deterministic mode is first-class**: For tests
//! - **No POSIX shell**: Just component orchestration
//! - **No terminal emulation**: Dumb host, smart components
//!
//! ## Responsibilities
//!
//! The host runtime:
//! - Boots the workspace with core services
//! - Runs a live event loop (input → step → render)
//! - Supports both simulation and HAL input modes
//! - Provides minimal workspace control commands
//! - Remains testable via deterministic mode
//!
//! ## Non-Responsibilities
//!
//! The host does NOT:
//! - Implement a shell with pipes or job control
//! - Provide terminal emulation (ANSI/VT codes)
//! - Give components stdout/stderr access
//! - Introduce global filesystem authority
//! - Bypass policy or budgets

pub mod commands;
pub mod input_script;
pub mod runtime;

pub use commands::{HostCommand, HostCommandError, HostCommandParser};
pub use input_script::{InputScript, InputScriptError, ScriptedInput};
pub use runtime::{HostMode, HostRuntime, HostRuntimeConfig, HostRuntimeError};

//! # Process Manager Service
//!
//! This crate manages service lifecycle.
//!
//! ## Philosophy
//!
//! Services are managed explicitly with clear lifecycle states.
//! Unlike Unix init systems (systemd, etc.), we focus on:
//! - Explicit lifecycle (not implicit fork/exec)
//! - Restart policies (not shell scripts)
//! - Capability-based dependencies (not path-based)

pub mod descriptor;
pub mod lifecycle;
pub mod manager;
pub mod process_info;

pub use descriptor::{RestartPolicy, ServiceDescriptor};
pub use lifecycle::{CrashReason, LifecycleState, ServiceHandle};
pub use manager::{ExitNotificationSource, ProcessManager, ProcessManagerError};
pub use process_info::{KillResult, KillSignal, ProcessInfo, ProcessList};

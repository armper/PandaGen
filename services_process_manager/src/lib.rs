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
//!
//! ## Not implemented
//!
//! This list used to end with "Capability-based dependencies (not
//! path-based)". `ServiceDescriptor::dependencies` and `with_dependency`
//! exist, are written, and are read by nothing: there is no ordering, no
//! wait, and no check that a dependency is running. Nothing here can express
//! "start B after A".
//!
//! `ExponentialBackoff` also implements no backoff. `should_restart` only
//! compares an attempt count against `max_attempts`, and `restart_attempts`
//! is never reset after a successful run -- so a service that fails once a
//! month eventually stops being restarted, and none of the restarts are
//! delayed.
//!
//! Said plainly rather than left in a feature list, because a feature list
//! is the first thing a caller reads and the last thing anyone checks.

pub mod descriptor;
pub mod lifecycle;
pub mod manager;
pub mod process_info;

pub use descriptor::{RestartPolicy, ServiceDescriptor};
pub use lifecycle::{CrashReason, LifecycleState, ServiceHandle};
pub use manager::{ExitNotificationSource, ProcessManager, ProcessManagerError};
pub use process_info::{KillResult, KillSignal, ProcessInfo, ProcessList};

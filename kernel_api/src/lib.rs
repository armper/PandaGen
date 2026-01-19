//! # Kernel API
//!
//! This crate defines the interface between user-space code and the kernel.
//!
//! ## Philosophy
//!
//! The kernel provides **mechanisms**, not policies:
//! - Task creation (not process forking)
//! - Message passing (not shared memory)
//! - Time management (explicit, not ambient)
//! - Capability transfer (explicit authority)
//!
//! ## Design Goals
//!
//! 1. **Testability**: The entire API can be mocked and tested
//! 2. **Explicitness**: No hidden state or ambient authority
//! 3. **Type safety**: Capabilities are strongly typed
//! 4. **Simplicity**: Minimal surface area
//!
//! ## Non-Goals
//!
//! This is NOT:
//! - POSIX (no fork, exec, signals, files)
//! - A syscall interface (though it could be implemented that way)
//! - A specific transport (trait can be implemented many ways)

pub mod error;
pub mod kernel;
pub mod syscalls;
pub mod time;

pub use error::KernelError;
pub use kernel::{KernelApi, TaskDescriptor, TaskHandle};
pub use syscalls::{
    LoopbackTransport, SyscallClient, SyscallCodec, SyscallError, SyscallErrorKind,
    SyscallRequest, SyscallRequestPayload, SyscallResponse, SyscallResponsePayload,
    SyscallServer, SyscallTransport,
};
pub use time::{Duration, Instant};

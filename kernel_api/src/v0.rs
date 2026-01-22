//! Kernel API v0: minimal task + capability + channel surface.
//!
//! This is an intentionally tiny surface that avoids POSIX primitives and
//! relies on typed IDs and explicit capability transfer. It is designed to
//! bootstrap user-task scaffolding and service IPC without ambient authority.

use alloc::string::String;
use alloc::vec::Vec;
use crate::{Duration, KernelError};
use core_types::{Cap, TaskId};
use ipc::ChannelId;

/// Minimal kernel API surface (v0).
///
/// This is a reduced subset of [`KernelApi`] meant for early bootstrapping
/// and user-task scaffolding.
pub trait KernelApiV0 {
    /// Creates a new task with the given name and initial capabilities.
    fn create_task(
        &mut self,
        name: String,
        capabilities: Vec<Cap<()>>,
    ) -> Result<TaskId, KernelError>;

    /// Creates a new bidirectional channel.
    fn create_channel(&mut self) -> Result<ChannelId, KernelError>;

    /// Sends a typed message envelope.
    fn send(
        &mut self,
        channel: ChannelId,
        message: ipc::MessageEnvelope,
    ) -> Result<(), KernelError>;

    /// Receives a typed message envelope (non-blocking semantics are defined by the kernel).
    fn recv(&mut self, channel: ChannelId) -> Result<ipc::MessageEnvelope, KernelError>;

    /// Yields execution to the scheduler.
    fn yield_now(&mut self) -> Result<(), KernelError>;

    /// Sleeps for a duration (kernel-defined semantics).
    fn sleep(&mut self, duration: Duration) -> Result<(), KernelError>;

    /// Grants a capability to another task.
    fn grant(&mut self, task: TaskId, capability: Cap<()>) -> Result<(), KernelError>;
}

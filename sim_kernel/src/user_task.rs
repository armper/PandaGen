//! User task context scaffolding (simulation).
//!
//! Provides a minimal user task context with separate user/kernel stacks
//! and a trap entry for syscalls.

use core_types::TaskId;
use ipc::{ChannelId, MessageEnvelope};
use kernel_api::{Duration, KernelApiV0, KernelError};

/// Minimal syscall set for user tasks.
#[derive(Debug, Clone)]
pub enum UserSyscall {
    Send { channel: ChannelId, message: MessageEnvelope },
    Recv { channel: ChannelId },
    Yield,
    Sleep { duration: Duration },
}

/// Syscall result for user tasks.
#[derive(Debug, Clone)]
pub enum UserSyscallResult {
    Ok,
    Message(MessageEnvelope),
}

/// Trap entry signature for user task syscalls.
pub type TrapEntry = fn(&mut dyn KernelApiV0, UserSyscall) -> Result<UserSyscallResult, KernelError>;

/// Default trap handler for user tasks.
pub fn default_trap(kernel: &mut dyn KernelApiV0, call: UserSyscall) -> Result<UserSyscallResult, KernelError> {
    match call {
        UserSyscall::Send { channel, message } => {
            kernel.send(channel, message)?;
            Ok(UserSyscallResult::Ok)
        }
        UserSyscall::Recv { channel } => {
            let message = kernel.recv(channel)?;
            Ok(UserSyscallResult::Message(message))
        }
        UserSyscall::Yield => {
            kernel.yield_now()?;
            Ok(UserSyscallResult::Ok)
        }
        UserSyscall::Sleep { duration } => {
            kernel.sleep(duration)?;
            Ok(UserSyscallResult::Ok)
        }
    }
}

/// Minimal user task context with separate stacks and a trap entry.
#[derive(Debug, Clone)]
pub struct UserTaskContext {
    pub task_id: TaskId,
    user_stack: Vec<u8>,
    kernel_stack: Vec<u8>,
    trap_entry: TrapEntry,
}

impl UserTaskContext {
    /// Creates a new user task context.
    pub fn new(
        task_id: TaskId,
        user_stack_bytes: usize,
        kernel_stack_bytes: usize,
        trap_entry: TrapEntry,
    ) -> Self {
        Self {
            task_id,
            user_stack: vec![0; user_stack_bytes],
            kernel_stack: vec![0; kernel_stack_bytes],
            trap_entry,
        }
    }

    /// Returns the user stack size.
    pub fn user_stack_size(&self) -> usize {
        self.user_stack.len()
    }

    /// Returns the kernel stack size.
    pub fn kernel_stack_size(&self) -> usize {
        self.kernel_stack.len()
    }

    /// Executes a syscall via the trap entry.
    pub fn syscall(
        &self,
        kernel: &mut dyn KernelApiV0,
        call: UserSyscall,
    ) -> Result<UserSyscallResult, KernelError> {
        (self.trap_entry)(kernel, call)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ipc::{MessagePayload, SchemaVersion};
    use crate::SimulatedKernel;

    #[test]
    fn test_user_task_context_stacks() {
        let task_id = TaskId::new();
        let ctx = UserTaskContext::new(task_id, 1024, 512, default_trap);
        assert_eq!(ctx.user_stack_size(), 1024);
        assert_eq!(ctx.kernel_stack_size(), 512);
        assert_eq!(ctx.task_id, task_id);
    }

    #[test]
    fn test_user_task_syscalls() {
        let mut kernel = SimulatedKernel::new();
        let task_id = kernel_api::KernelApi::spawn_task(
            &mut kernel,
            kernel_api::TaskDescriptor::new("user".to_string()),
        )
        .unwrap()
        .task_id;
        let channel = kernel_api::KernelApiV0::create_channel(&mut kernel).unwrap();

        let ctx = UserTaskContext::new(task_id, 256, 256, default_trap);
        let payload = MessagePayload::new(&"ping").unwrap();
        let message = MessageEnvelope::new(
            core_types::ServiceId::new(),
            "ping".to_string(),
            SchemaVersion::new(1, 0),
            payload,
        );

        ctx.syscall(&mut kernel, UserSyscall::Send { channel, message })
            .unwrap();

        let result = ctx
            .syscall(&mut kernel, UserSyscall::Recv { channel })
            .unwrap();

        match result {
            UserSyscallResult::Message(envelope) => {
                assert_eq!(envelope.action, "ping");
            }
            _ => panic!("Expected message"),
        }
    }
}

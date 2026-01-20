//! User task context scaffolding (simulation).
//!
//! Provides a minimal user task context with separate user/kernel stacks
//! and a trap entry for syscalls.

use crate::syscall_gate::{Syscall, SyscallResult};
use core_types::TaskId;
use identity::ExecutionId;
use kernel_api::{KernelApi, KernelError};

/// Minimal syscall set for user tasks (Phase 61: replaced with syscall_gate::Syscall).
/// 
/// This remains for backwards compatibility with existing code.
/// New code should use syscall_gate::Syscall directly.
#[derive(Debug, Clone)]
#[deprecated(since = "0.61.0", note = "Use syscall_gate::Syscall instead")]
pub enum UserSyscall {
    Send { channel: ipc::ChannelId, message: ipc::MessageEnvelope },
    Recv { channel: ipc::ChannelId },
    Yield,
    Sleep { duration: kernel_api::Duration },
}

/// Syscall result for user tasks (Phase 61: replaced with syscall_gate::SyscallResult).
#[derive(Debug, Clone)]
#[deprecated(since = "0.61.0", note = "Use syscall_gate::SyscallResult instead")]
pub enum UserSyscallResult {
    Ok,
    Message(ipc::MessageEnvelope),
}

/// Trap entry signature for user task syscalls.
///
/// Phase 61: This now uses the syscall gate and requires ExecutionId.
pub type TrapEntry = fn(&mut crate::SimulatedKernel, ExecutionId, Syscall) -> Result<SyscallResult, KernelError>;

/// Default trap handler for user tasks (Phase 61: uses syscall gate).
pub fn default_trap(
    kernel: &mut crate::SimulatedKernel,
    caller: ExecutionId,
    syscall: Syscall,
) -> Result<SyscallResult, KernelError> {
    let timestamp_nanos = kernel.now().as_nanos();
    
    // All syscalls must go through the gate - this enforces the isolation boundary
    match syscall {
        Syscall::CreateChannel => {
            kernel.syscall_gate_mut().record_invoked(caller, "CreateChannel".to_string(), timestamp_nanos);
            let result = kernel.create_channel().map(SyscallResult::ChannelId);
            match &result {
                Ok(_) => kernel.syscall_gate_mut().record_completed(caller, "CreateChannel".to_string(), timestamp_nanos),
                Err(err) => kernel.syscall_gate_mut().record_rejected(caller, "CreateChannel".to_string(), format!("{:?}", err), timestamp_nanos),
            }
            result
        }
        Syscall::Send { channel, message } => {
            kernel.syscall_gate_mut().record_invoked(caller, "Send".to_string(), timestamp_nanos);
            let result = kernel.send_message(channel, message).map(|_| SyscallResult::Ok);
            match &result {
                Ok(_) => kernel.syscall_gate_mut().record_completed(caller, "Send".to_string(), timestamp_nanos),
                Err(err) => kernel.syscall_gate_mut().record_rejected(caller, "Send".to_string(), format!("{:?}", err), timestamp_nanos),
            }
            result
        }
        Syscall::Recv { channel } => {
            kernel.syscall_gate_mut().record_invoked(caller, "Recv".to_string(), timestamp_nanos);
            let result = kernel.receive_message(channel, None).map(SyscallResult::Message);
            match &result {
                Ok(_) => kernel.syscall_gate_mut().record_completed(caller, "Recv".to_string(), timestamp_nanos),
                Err(err) => kernel.syscall_gate_mut().record_rejected(caller, "Recv".to_string(), format!("{:?}", err), timestamp_nanos),
            }
            result
        }
        _ => {
            // Other syscalls not yet implemented in default_trap
            kernel.syscall_gate_mut().record_rejected(caller, "Unknown".to_string(), "Not supported".to_string(), timestamp_nanos);
            Err(KernelError::InsufficientAuthority(
                "Syscall not supported in default_trap".to_string()
            ))
        }
    }
}

/// Minimal user task context with separate stacks and a trap entry.
#[derive(Debug, Clone)]
pub struct UserTaskContext {
    pub task_id: TaskId,
    pub execution_id: ExecutionId,
    user_stack: Vec<u8>,
    kernel_stack: Vec<u8>,
    trap_entry: TrapEntry,
}

impl UserTaskContext {
    /// Creates a new user task context.
    pub fn new(
        task_id: TaskId,
        execution_id: ExecutionId,
        user_stack_bytes: usize,
        kernel_stack_bytes: usize,
        trap_entry: TrapEntry,
    ) -> Self {
        Self {
            task_id,
            execution_id,
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
        kernel: &mut crate::SimulatedKernel,
        syscall: Syscall,
    ) -> Result<SyscallResult, KernelError> {
        (self.trap_entry)(kernel, self.execution_id, syscall)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ipc::{MessagePayload, SchemaVersion};
    use kernel_api::KernelApi;

    #[test]
    fn test_user_task_context_stacks() {
        let task_id = TaskId::new();
        let exec_id = ExecutionId::new();
        let ctx = UserTaskContext::new(task_id, exec_id, 1024, 512, default_trap);
        assert_eq!(ctx.user_stack_size(), 1024);
        assert_eq!(ctx.kernel_stack_size(), 512);
        assert_eq!(ctx.task_id, task_id);
        assert_eq!(ctx.execution_id, exec_id);
    }

    #[test]
    fn test_user_task_syscalls_through_gate() {
        use kernel_api::TaskDescriptor;
        
        let mut kernel = crate::SimulatedKernel::new();
        let handle = kernel.spawn_task(TaskDescriptor::new("user".to_string())).unwrap();
        let task_id = handle.task_id;
        let exec_id = kernel.get_task_identity(task_id).unwrap();
        
        let channel = kernel.create_channel().unwrap();

        let ctx = UserTaskContext::new(task_id, exec_id, 256, 256, default_trap);
        
        let payload = MessagePayload::new(&"ping").unwrap();
        let message = ipc::MessageEnvelope::new(
            core_types::ServiceId::new(),
            "ping".to_string(),
            SchemaVersion::new(1, 0),
            payload,
        );

        // Send through syscall gate
        let result = ctx.syscall(&mut kernel, Syscall::Send { channel, message: message.clone() });
        assert!(result.is_ok());

        // Recv through syscall gate
        let result = ctx.syscall(&mut kernel, Syscall::Recv { channel });
        assert!(result.is_ok());
        
        match result.unwrap() {
            SyscallResult::Message(envelope) => {
                assert_eq!(envelope.action, "ping");
            }
            _ => panic!("Expected message"),
        }
        
        // Verify syscall gate recorded events
        let audit = kernel.syscall_gate().audit_log();
        assert!(audit.events().len() >= 2); // At least Send and Recv
    }
}

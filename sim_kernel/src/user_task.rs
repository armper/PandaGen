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
#[deprecated(note = "Use syscall_gate::Syscall instead")]
pub enum UserSyscall {
    Send {
        channel: ipc::ChannelId,
        message: ipc::MessageEnvelope,
    },
    Recv {
        channel: ipc::ChannelId,
    },
    Yield,
    Sleep {
        duration: kernel_api::Duration,
    },
}

/// Syscall result for user tasks (Phase 61: replaced with syscall_gate::SyscallResult).
#[derive(Debug, Clone)]
#[deprecated(note = "Use syscall_gate::SyscallResult instead")]
pub enum UserSyscallResult {
    Ok,
    Message(ipc::MessageEnvelope),
}

/// Trap entry signature for user task syscalls.
///
/// Phase 61: This now uses the syscall gate and requires ExecutionId.
pub type TrapEntry =
    fn(&mut crate::SimulatedKernel, ExecutionId, Syscall) -> Result<SyscallResult, KernelError>;

/// The gate's name for a syscall, used for the refusal record made before
/// the syscall is dispatched.
fn syscall_name_of(syscall: &Syscall) -> &'static str {
    match syscall {
        Syscall::SpawnTask { .. } => "SpawnTask",
        Syscall::CreateChannel => "CreateChannel",
        Syscall::Send { .. } => "Send",
        Syscall::Recv { .. } => "Recv",
        Syscall::Sleep { .. } => "Sleep",
        Syscall::Now => "Now",
        Syscall::Yield => "Yield",
        Syscall::Grant { .. } => "Grant",
        Syscall::RegisterService { .. } => "RegisterService",
        Syscall::LookupService { .. } => "LookupService",
        Syscall::CreateAddressSpace => "CreateAddressSpace",
        Syscall::AllocateRegion { .. } => "AllocateRegion",
        Syscall::AccessRegion { .. } => "AccessRegion",
    }
}

/// Default trap handler for user tasks (Phase 61: uses syscall gate).
pub fn default_trap(
    kernel: &mut crate::SimulatedKernel,
    caller: ExecutionId,
    syscall: Syscall,
) -> Result<SyscallResult, KernelError> {
    let timestamp_nanos = kernel.now().as_nanos();

    // The caller is checked here, before anything is dispatched. Until this
    // existed the gate was a logger: this function recorded `Invoked`, called
    // straight into the kernel, and recorded the result -- so a cancelled
    // identity's syscalls were written down and then carried out.
    let syscall_name = syscall_name_of(&syscall);
    if let Some(err) =
        kernel
            .syscall_gate_mut()
            .refuse_if_revoked(caller, syscall_name, timestamp_nanos)
    {
        return Err(err);
    }

    match syscall {
        Syscall::CreateChannel => {
            kernel.syscall_gate_mut().record_invoked(
                caller,
                "CreateChannel".to_string(),
                timestamp_nanos,
            );
            let result = kernel.create_channel().map(SyscallResult::ChannelId);
            match &result {
                Ok(_) => kernel.syscall_gate_mut().record_completed(
                    caller,
                    "CreateChannel".to_string(),
                    timestamp_nanos,
                ),
                Err(err) => kernel.syscall_gate_mut().record_rejected(
                    caller,
                    "CreateChannel".to_string(),
                    format!("{:?}", err),
                    timestamp_nanos,
                ),
            }
            result
        }
        Syscall::Send { channel, message } => {
            kernel
                .syscall_gate_mut()
                .record_invoked(caller, "Send".to_string(), timestamp_nanos);
            let result = kernel
                .send_message(channel, message)
                .map(|_| SyscallResult::Ok);
            match &result {
                Ok(_) => kernel.syscall_gate_mut().record_completed(
                    caller,
                    "Send".to_string(),
                    timestamp_nanos,
                ),
                Err(err) => kernel.syscall_gate_mut().record_rejected(
                    caller,
                    "Send".to_string(),
                    format!("{:?}", err),
                    timestamp_nanos,
                ),
            }
            result
        }
        Syscall::Recv { channel } => {
            kernel
                .syscall_gate_mut()
                .record_invoked(caller, "Recv".to_string(), timestamp_nanos);
            let result = kernel
                .receive_message(channel, None)
                .map(SyscallResult::Message);
            match &result {
                Ok(_) => kernel.syscall_gate_mut().record_completed(
                    caller,
                    "Recv".to_string(),
                    timestamp_nanos,
                ),
                Err(err) => kernel.syscall_gate_mut().record_rejected(
                    caller,
                    "Recv".to_string(),
                    format!("{:?}", err),
                    timestamp_nanos,
                ),
            }
            result
        }
        other => {
            // Other syscalls not yet implemented in default_trap
            let syscall_name = format!("{:?}", other);
            kernel.syscall_gate_mut().record_rejected(
                caller,
                syscall_name.clone(),
                "Not supported in default_trap".to_string(),
                timestamp_nanos,
            );
            Err(KernelError::InsufficientAuthority(format!(
                "Syscall {} not supported in default_trap",
                syscall_name
            )))
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
        let handle = kernel
            .spawn_task(TaskDescriptor::new("user".to_string()))
            .unwrap();
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
        let result = ctx.syscall(
            &mut kernel,
            Syscall::Send {
                channel,
                message: message.clone(),
            },
        );
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

#[cfg(test)]
mod revoked_identity_tests {
    use super::*;
    use crate::syscall_gate::SyscallEvent;
    use kernel_api::{KernelApi, TaskDescriptor};
    use resources::{CpuTicks, ResourceBudget};

    /// The finding: the gate took `caller` on every syscall and did nothing
    /// with it but write it to the log. An identity the kernel had already
    /// cancelled for blowing its budget went on making syscalls, each one
    /// recorded as `Invoked` and then `Completed`. An audit trail that
    /// records an operation it did not prevent is a record of the breach.
    #[test]
    fn a_cancelled_identity_cannot_keep_making_syscalls() {
        let mut kernel = crate::SimulatedKernel::new();
        let handle = kernel
            .spawn_task(TaskDescriptor::new("user".to_string()))
            .unwrap();
        let task_id = handle.task_id;
        let exec_id = kernel.get_task_identity(task_id).unwrap();

        let ctx = UserTaskContext::new(task_id, exec_id, 256, 256, default_trap);

        // While it is in good standing, the syscall works.
        ctx.syscall(&mut kernel, Syscall::CreateChannel)
            .expect("a live identity must be served");

        // Blow the CPU budget, which cancels the identity.
        if let Some(identity) = kernel.get_identity_mut(exec_id) {
            identity.budget = Some(ResourceBudget::unlimited().with_cpu_ticks(CpuTicks::new(10)));
        }
        assert!(kernel.try_consume_cpu_ticks(exec_id, 11).is_err());

        kernel.syscall_gate_mut().clear_audit_log();
        let refused = ctx.syscall(&mut kernel, Syscall::CreateChannel);
        assert!(
            matches!(refused, Err(KernelError::InsufficientAuthority(_))),
            "a cancelled identity was served: {refused:?}"
        );

        // And the refusal is what the log says, not a completion.
        let audit = kernel.syscall_gate().audit_log();
        assert!(
            audit.has_event(|e| matches!(e, SyscallEvent::Rejected { .. })),
            "the refusal was not recorded"
        );
        assert!(
            !audit.has_event(|e| matches!(e, SyscallEvent::Completed { .. })),
            "a cancelled identity's syscall completed"
        );
    }

    /// The same refusal on the gate's other door, because a guard given to one
    /// call site is half a fix.
    #[test]
    fn the_gates_own_execute_refuses_a_revoked_identity_too() {
        let mut kernel = crate::SimulatedKernel::new();
        let caller = ExecutionId::new();
        let mut gate = crate::syscall_gate::SyscallGate::new();

        gate.execute(&mut kernel, caller, Syscall::CreateChannel, 0)
            .expect("an unrevoked caller must be served");

        gate.revoke(caller);
        let refused = gate.execute(&mut kernel, caller, Syscall::CreateChannel, 1);
        assert!(
            matches!(refused, Err(KernelError::InsufficientAuthority(_))),
            "execute served a revoked identity: {refused:?}"
        );
    }
}

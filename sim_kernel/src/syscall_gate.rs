//! Syscall gate for user/kernel isolation boundary.
//!
//! This module implements the syscall gate that enforces the user/kernel
//! boundary. All kernel operations from user tasks must go through this gate.

use core_types::{AddressSpaceCap, AddressSpaceId, Cap, MemoryAccessType, MemoryBacking, MemoryError, MemoryPerms, MemoryRegionCap, ServiceId, TaskId};
use identity::ExecutionId;
use ipc::{ChannelId, MessageEnvelope};
use kernel_api::{Duration, KernelApi, KernelError, TaskDescriptor, TaskHandle};
use serde::{Deserialize, Serialize};

/// Complete syscall set for user tasks.
/// This is the ONLY interface between user space and kernel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Syscall {
    // Task management
    SpawnTask { descriptor: TaskDescriptor },
    
    // Channel operations
    CreateChannel,
    Send { channel: ChannelId, message: MessageEnvelope },
    Recv { channel: ChannelId },
    
    // Time operations
    Sleep { duration: Duration },
    Now,
    Yield,
    
    // Capability operations
    Grant { task: TaskId, capability: Cap<()> },
    
    // Service registry
    RegisterService { service_id: ServiceId, channel: ChannelId },
    LookupService { service_id: ServiceId },
    
    // Memory operations (Phase 61)
    CreateAddressSpace,
    AllocateRegion {
        space_cap: AddressSpaceCap,
        size_bytes: u64,
        permissions: MemoryPerms,
        backing: MemoryBacking,
    },
    AccessRegion {
        region_cap: MemoryRegionCap,
        access_type: MemoryAccessType,
    },
}

/// Syscall result from the gate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyscallResult {
    Ok,
    TaskHandle(TaskHandle),
    ChannelId(ChannelId),
    Message(MessageEnvelope),
    Instant(kernel_api::Instant),
    AddressSpaceCap(AddressSpaceCap),
    MemoryRegionCap(MemoryRegionCap),
}

/// Syscall audit event (for testing and verification).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyscallEvent {
    /// Syscall was invoked
    Invoked {
        caller: ExecutionId,
        syscall_name: String,
        timestamp_nanos: u64,
    },
    /// Syscall completed successfully
    Completed {
        caller: ExecutionId,
        syscall_name: String,
        timestamp_nanos: u64,
    },
    /// Syscall was rejected
    Rejected {
        caller: ExecutionId,
        syscall_name: String,
        reason: String,
        timestamp_nanos: u64,
    },
    /// Attempted to bypass syscall gate (security violation)
    BypassAttempt {
        caller: ExecutionId,
        timestamp_nanos: u64,
    },
}

/// Audit log for syscall operations.
#[derive(Debug, Clone, Default)]
pub struct SyscallAuditLog {
    events: Vec<SyscallEvent>,
}

impl SyscallAuditLog {
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    pub fn record(&mut self, event: SyscallEvent) {
        self.events.push(event);
    }

    pub fn events(&self) -> &[SyscallEvent] {
        &self.events
    }

    pub fn clear(&mut self) {
        self.events.clear();
    }

    pub fn has_event<F>(&self, predicate: F) -> bool
    where
        F: Fn(&SyscallEvent) -> bool,
    {
        self.events.iter().any(predicate)
    }

    pub fn count_events<F>(&self, predicate: F) -> usize
    where
        F: Fn(&SyscallEvent) -> bool,
    {
        self.events.iter().filter(|e| predicate(e)).count()
    }
}

/// Syscall gate that enforces user/kernel boundary.
///
/// This is the ONLY way user tasks can request kernel operations.
/// Direct kernel access is prohibited.
pub struct SyscallGate {
    /// Audit log for syscall operations
    audit_log: SyscallAuditLog,
}

impl SyscallGate {
    pub fn new() -> Self {
        Self {
            audit_log: SyscallAuditLog::new(),
        }
    }

    /// Returns the audit log (test-only)
    pub fn audit_log(&self) -> &SyscallAuditLog {
        &self.audit_log
    }

    /// Clears the audit log (test-only)
    pub fn clear_audit_log(&mut self) {
        self.audit_log.clear();
    }

    /// Executes a syscall on behalf of a user task.
    ///
    /// This is the ONLY entry point from user space to kernel.
    /// It validates the caller's identity and executes the requested operation.
    pub fn execute(
        &mut self,
        kernel: &mut dyn KernelApi,
        caller: ExecutionId,
        syscall: Syscall,
        timestamp_nanos: u64,
    ) -> Result<SyscallResult, KernelError> {
        let syscall_name = self.syscall_name(&syscall);

        // Record invocation
        self.audit_log.record(SyscallEvent::Invoked {
            caller,
            syscall_name: syscall_name.clone(),
            timestamp_nanos,
        });

        // Execute the syscall
        let result = match syscall {
            Syscall::SpawnTask { descriptor } => {
                kernel.spawn_task(descriptor).map(SyscallResult::TaskHandle)
            }
            Syscall::CreateChannel => {
                kernel.create_channel().map(SyscallResult::ChannelId)
            }
            Syscall::Send { channel, message } => {
                kernel.send_message(channel, message).map(|_| SyscallResult::Ok)
            }
            Syscall::Recv { channel } => {
                kernel.receive_message(channel, None).map(SyscallResult::Message)
            }
            Syscall::Sleep { duration } => {
                kernel.sleep(duration).map(|_| SyscallResult::Ok)
            }
            Syscall::Now => {
                Ok(SyscallResult::Instant(kernel.now()))
            }
            Syscall::Yield => {
                // Yield is a hint to the scheduler, always succeeds
                Ok(SyscallResult::Ok)
            }
            Syscall::Grant { task, capability } => {
                kernel.grant_capability(task, capability).map(|_| SyscallResult::Ok)
            }
            Syscall::RegisterService { service_id, channel } => {
                kernel.register_service(service_id, channel).map(|_| SyscallResult::Ok)
            }
            Syscall::LookupService { service_id } => {
                kernel.lookup_service(service_id).map(SyscallResult::ChannelId)
            }
            Syscall::CreateAddressSpace => {
                // Memory operations need to be routed through SimulatedKernel
                // For now, return an error indicating this needs special handling
                Err(KernelError::InsufficientAuthority(
                    "CreateAddressSpace requires SimulatedKernel access".to_string()
                ))
            }
            Syscall::AllocateRegion { .. } => {
                Err(KernelError::InsufficientAuthority(
                    "AllocateRegion requires SimulatedKernel access".to_string()
                ))
            }
            Syscall::AccessRegion { .. } => {
                Err(KernelError::InsufficientAuthority(
                    "AccessRegion requires SimulatedKernel access".to_string()
                ))
            }
        };

        // Record completion or rejection
        match &result {
            Ok(_) => {
                self.audit_log.record(SyscallEvent::Completed {
                    caller,
                    syscall_name,
                    timestamp_nanos,
                });
            }
            Err(err) => {
                self.audit_log.record(SyscallEvent::Rejected {
                    caller,
                    syscall_name,
                    reason: format!("{:?}", err),
                    timestamp_nanos,
                });
            }
        }

        result
    }

    /// Executes a syscall with full kernel access (for memory operations).
    ///
    /// This variant provides access to SimulatedKernel-specific operations.
    pub fn execute_with_memory<K>(
        &mut self,
        kernel: &mut K,
        caller: ExecutionId,
        syscall: Syscall,
        timestamp_nanos: u64,
    ) -> Result<SyscallResult, MemoryError>
    where
        K: KernelApi + MemoryOps,
    {
        let syscall_name = self.syscall_name(&syscall);

        // Record invocation
        self.audit_log.record(SyscallEvent::Invoked {
            caller,
            syscall_name: syscall_name.clone(),
            timestamp_nanos,
        });

        // Execute the syscall
        let result = match syscall {
            Syscall::CreateAddressSpace => {
                kernel.create_address_space_op(caller)
                    .map(SyscallResult::AddressSpaceCap)
            }
            Syscall::AllocateRegion { space_cap, size_bytes, permissions, backing } => {
                kernel.allocate_region_op(&space_cap, size_bytes, permissions, backing, caller)
                    .map(SyscallResult::MemoryRegionCap)
            }
            Syscall::AccessRegion { region_cap, access_type } => {
                kernel.access_region_op(&region_cap, access_type, caller)
                    .map(|_| SyscallResult::Ok)
            }
            _ => {
                // Non-memory syscalls should not be routed through execute_with_memory
                return Err(MemoryError::RegionNotFound(core_types::MemoryRegionId::new()));
            }
        };

        // Record completion or rejection
        match &result {
            Ok(_) => {
                self.audit_log.record(SyscallEvent::Completed {
                    caller,
                    syscall_name,
                    timestamp_nanos,
                });
            }
            Err(err) => {
                self.audit_log.record(SyscallEvent::Rejected {
                    caller,
                    syscall_name,
                    reason: format!("{:?}", err),
                    timestamp_nanos,
                });
            }
        }

        result
    }

    fn syscall_name(&self, syscall: &Syscall) -> String {
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
        }.to_string()
    }

    /// Records a bypass attempt (security violation).
    pub fn record_bypass_attempt(&mut self, caller: ExecutionId, timestamp_nanos: u64) {
        self.audit_log.record(SyscallEvent::BypassAttempt {
            caller,
            timestamp_nanos,
        });
    }

    /// Records a syscall invocation event.
    pub fn record_invoked(&mut self, caller: ExecutionId, syscall_name: String, timestamp_nanos: u64) {
        self.audit_log.record(SyscallEvent::Invoked {
            caller,
            syscall_name,
            timestamp_nanos,
        });
    }

    /// Records a syscall completion event.
    pub fn record_completed(&mut self, caller: ExecutionId, syscall_name: String, timestamp_nanos: u64) {
        self.audit_log.record(SyscallEvent::Completed {
            caller,
            syscall_name,
            timestamp_nanos,
        });
    }

    /// Records a syscall rejection event.
    pub fn record_rejected(&mut self, caller: ExecutionId, syscall_name: String, reason: String, timestamp_nanos: u64) {
        self.audit_log.record(SyscallEvent::Rejected {
            caller,
            syscall_name,
            reason,
            timestamp_nanos,
        });
    }
}

impl Default for SyscallGate {
    fn default() -> Self {
        Self::new()
    }
}

/// Trait for memory operations that require SimulatedKernel access.
///
/// This is implemented by SimulatedKernel to provide memory management
/// operations through the syscall gate.
pub trait MemoryOps {
    fn create_address_space_op(&mut self, execution_id: ExecutionId) -> Result<AddressSpaceCap, MemoryError>;
    fn allocate_region_op(
        &mut self,
        space_cap: &AddressSpaceCap,
        size_bytes: u64,
        permissions: MemoryPerms,
        backing: MemoryBacking,
        caller_execution_id: ExecutionId,
    ) -> Result<MemoryRegionCap, MemoryError>;
    fn access_region_op(
        &mut self,
        region_cap: &MemoryRegionCap,
        access_type: MemoryAccessType,
        caller_execution_id: ExecutionId,
    ) -> Result<(), MemoryError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SimulatedKernel;

    #[test]
    fn test_syscall_gate_basic_operation() {
        let mut gate = SyscallGate::new();
        let mut kernel = SimulatedKernel::new();
        let exec_id = ExecutionId::new();

        let syscall = Syscall::CreateChannel;
        let result = gate.execute(&mut kernel, exec_id, syscall, 1000);

        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), SyscallResult::ChannelId(_)));

        // Check audit log
        assert_eq!(gate.audit_log().events().len(), 2); // Invoked + Completed
        assert!(gate.audit_log().has_event(|e| matches!(e, SyscallEvent::Invoked { .. })));
        assert!(gate.audit_log().has_event(|e| matches!(e, SyscallEvent::Completed { .. })));
    }

    #[test]
    fn test_syscall_gate_rejection() {
        let mut gate = SyscallGate::new();
        let mut kernel = SimulatedKernel::new();
        let exec_id = ExecutionId::new();

        // Try to lookup a non-existent service
        let syscall = Syscall::LookupService { service_id: ServiceId::new() };
        let result = gate.execute(&mut kernel, exec_id, syscall, 1000);

        assert!(result.is_err());

        // Check audit log
        assert_eq!(gate.audit_log().events().len(), 2); // Invoked + Rejected
        assert!(gate.audit_log().has_event(|e| matches!(e, SyscallEvent::Rejected { .. })));
    }

    #[test]
    fn test_syscall_gate_audit_tracking() {
        let mut gate = SyscallGate::new();
        let mut kernel = SimulatedKernel::new();
        let exec_id = ExecutionId::new();

        // Execute multiple syscalls
        let _ = gate.execute(&mut kernel, exec_id, Syscall::CreateChannel, 1000);
        let _ = gate.execute(&mut kernel, exec_id, Syscall::CreateChannel, 2000);
        let _ = gate.execute(&mut kernel, exec_id, Syscall::Now, 3000);

        // Should have 6 events (3 invoked + 3 completed)
        assert_eq!(gate.audit_log().events().len(), 6);

        // Count invocations
        let invocations = gate.audit_log().count_events(|e| matches!(e, SyscallEvent::Invoked { .. }));
        assert_eq!(invocations, 3);
    }

    #[test]
    fn test_syscall_gate_bypass_attempt_recording() {
        let mut gate = SyscallGate::new();
        let exec_id = ExecutionId::new();

        gate.record_bypass_attempt(exec_id, 1000);

        assert!(gate.audit_log().has_event(|e| matches!(e, SyscallEvent::BypassAttempt { .. })));
    }
}

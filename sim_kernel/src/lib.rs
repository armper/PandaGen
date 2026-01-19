//! # Simulated Kernel
//!
//! This crate provides a simulated implementation of the kernel API.
//!
//! ## Purpose
//!
//! The simulated kernel allows testing system behavior without hardware:
//! - Runs under `cargo test`
//! - Deterministic (controlled time, no real concurrency)
//! - Fast (no real I/O or context switches)
//! - Inspectable (all state is accessible)
//!
//! ## Philosophy
//!
//! **Testability is a first-class design constraint.**
//!
//! Most OS code is hard to test because it was never designed to be tested.
//! By providing a simulated kernel from day one, we ensure that services
//! and applications can be thoroughly tested in isolation.
//!
//! This is not a "toy" or "mock" - it's a full implementation of the
//! kernel API that happens to run in-process for testing.

pub mod capability_audit;
pub mod fault_injection;
pub mod test_utils;

use core_types::{
    Cap, CapabilityEvent, CapabilityInvalidReason, CapabilityMetadata, CapabilityStatus, ServiceId,
    TaskId,
};
use fault_injection::FaultInjector;
use ipc::{ChannelId, MessageEnvelope};
use kernel_api::{Duration, Instant, KernelApi, KernelError, TaskDescriptor, TaskHandle};
use std::collections::{HashMap, VecDeque};

/// Simulated kernel state
///
/// This maintains all the state needed to simulate a kernel.
/// Unlike a real kernel, this state is directly accessible for testing.
pub struct SimulatedKernel {
    /// Current simulated time
    current_time: Instant,
    /// Spawned tasks
    tasks: HashMap<TaskId, TaskInfo>,
    /// Message channels
    channels: HashMap<ChannelId, Channel>,
    /// Service registry
    services: HashMap<ServiceId, ChannelId>,
    /// Fault injector (optional, for testing)
    fault_injector: Option<FaultInjector>,
    /// Pending delayed messages
    delayed_messages: Vec<DelayedMessage>,
    /// Capability authority table: tracks which tasks own which capabilities
    capability_table: HashMap<u64, CapabilityMetadata>,
    /// Audit log for capability operations (test-only)
    capability_audit: capability_audit::CapabilityAuditLog,
}

#[derive(Debug)]
struct TaskInfo {
    #[allow(dead_code)]
    descriptor: TaskDescriptor,
}

struct Channel {
    /// Messages waiting to be received
    messages: VecDeque<MessageEnvelope>,
}

#[derive(Debug)]
struct DelayedMessage {
    channel: ChannelId,
    message: MessageEnvelope,
    deliver_at: Instant,
}

impl SimulatedKernel {
    /// Creates a new simulated kernel
    pub fn new() -> Self {
        Self {
            current_time: Instant::from_nanos(0),
            tasks: HashMap::new(),
            channels: HashMap::new(),
            services: HashMap::new(),
            fault_injector: None,
            delayed_messages: Vec::new(),
            capability_table: HashMap::new(),
            capability_audit: capability_audit::CapabilityAuditLog::new(),
        }
    }

    /// Sets the fault injector for this kernel
    ///
    /// This enables fault injection for testing. The fault injector
    /// will be applied to all message operations.
    pub fn with_fault_injector(mut self, injector: FaultInjector) -> Self {
        self.fault_injector = Some(injector);
        self
    }

    /// Sets the fault plan for this kernel
    ///
    /// Convenience method that creates a fault injector from a plan.
    pub fn with_fault_plan(self, plan: fault_injection::FaultPlan) -> Self {
        self.with_fault_injector(FaultInjector::new(plan))
    }

    /// Advances simulated time
    pub fn advance_time(&mut self, duration: Duration) {
        self.current_time = self.current_time + duration;
        self.process_delayed_messages();
    }

    /// Processes delayed messages that are ready to be delivered
    fn process_delayed_messages(&mut self) {
        let current_time = self.current_time;
        let mut ready_messages = Vec::new();

        // Find messages ready to be delivered
        self.delayed_messages.retain(|delayed| {
            if delayed.deliver_at <= current_time {
                ready_messages.push((delayed.channel, delayed.message.clone()));
                false
            } else {
                true
            }
        });

        // Deliver ready messages
        for (channel, message) in ready_messages {
            if let Some(ch) = self.channels.get_mut(&channel) {
                ch.messages.push_back(message);
            }
        }
    }

    /// Runs until no more messages are pending
    ///
    /// This advances time in small increments until all channels are empty
    /// and no delayed messages remain. Useful for test scenarios.
    pub fn run_until_idle(&mut self) {
        const MAX_ITERATIONS: usize = 1000;
        const TIME_STEP: Duration = Duration::from_millis(10);

        for _ in 0..MAX_ITERATIONS {
            if self.is_idle() {
                break;
            }
            self.advance_time(TIME_STEP);
        }
    }

    /// Checks if the kernel is idle (no messages pending)
    pub fn is_idle(&self) -> bool {
        self.channels.values().all(|ch| ch.messages.is_empty()) && self.delayed_messages.is_empty()
    }

    /// Returns the number of spawned tasks
    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }

    /// Returns the number of channels
    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    /// Returns the number of registered services
    pub fn service_count(&self) -> usize {
        self.services.len()
    }

    /// Returns the number of pending messages across all channels
    pub fn pending_message_count(&self) -> usize {
        self.channels
            .values()
            .map(|ch| ch.messages.len())
            .sum::<usize>()
            + self.delayed_messages.len()
    }

    /// Returns a reference to the capability audit log
    pub fn audit_log(&self) -> &capability_audit::CapabilityAuditLog {
        &self.capability_audit
    }

    /// Terminates a task and invalidates its capabilities
    ///
    /// This is called when a task exits or crashes. It invalidates all
    /// capabilities owned by the task to prevent use-after-free.
    pub fn terminate_task(&mut self, task_id: TaskId) {
        // Remove task
        self.tasks.remove(&task_id);

        // Invalidate all capabilities owned by this task
        let cap_ids: Vec<u64> = self
            .capability_table
            .iter()
            .filter(|(_, meta)| meta.owner == task_id)
            .map(|(id, _)| *id)
            .collect();

        for cap_id in cap_ids {
            if let Some(meta) = self.capability_table.get_mut(&cap_id) {
                meta.status = CapabilityStatus::Invalid;
                
                // Record invalidation event
                self.capability_audit.record_event(
                    self.current_time,
                    CapabilityEvent::Invalidated {
                        cap_id,
                        owner: task_id,
                        cap_type: meta.cap_type.clone(),
                    },
                );
            }
        }
    }

    /// Checks if a capability is valid for use by the specified task
    fn validate_capability(&self, cap_id: u64, task_id: TaskId) -> Result<(), CapabilityInvalidReason> {
        match self.capability_table.get(&cap_id) {
            None => Err(CapabilityInvalidReason::NeverGranted),
            Some(meta) => {
                // Check if capability is invalid
                if meta.status != CapabilityStatus::Valid {
                    return Err(CapabilityInvalidReason::TransferredAway);
                }

                // Check if owner is still alive
                if !self.tasks.contains_key(&meta.owner) {
                    return Err(CapabilityInvalidReason::OwnerDead);
                }

                // Check if the task trying to use it is the owner
                if meta.owner != task_id {
                    return Err(CapabilityInvalidReason::NeverGranted);
                }

                Ok(())
            }
        }
    }

    /// Records a capability grant in the authority table
    fn record_capability_grant(&mut self, cap_id: u64, grantee: TaskId, cap_type: String, grantor: Option<TaskId>) {
        let metadata = CapabilityMetadata {
            cap_id,
            owner: grantee,
            cap_type: cap_type.clone(),
            status: CapabilityStatus::Valid,
            grantor,
        };
        
        self.capability_table.insert(cap_id, metadata);
        
        self.capability_audit.record_event(
            self.current_time,
            CapabilityEvent::Granted {
                cap_id,
                grantor,
                grantee,
                cap_type,
            },
        );
    }

    /// Delegates a capability from one task to another (move semantics)
    ///
    /// This transfers ownership of the capability. After delegation,
    /// the original owner can no longer use the capability.
    pub fn delegate_capability(&mut self, cap_id: u64, from_task: TaskId, to_task: TaskId) -> Result<(), KernelError> {
        // Validate that the source task owns the capability
        self.validate_capability(cap_id, from_task)
            .map_err(|reason| KernelError::InvalidCapability(format!("Cannot delegate: {:?}", reason)))?;

        // Verify target task exists
        if !self.tasks.contains_key(&to_task) {
            return Err(KernelError::SendFailed("Target task not found".to_string()));
        }

        // Transfer ownership
        if let Some(meta) = self.capability_table.get_mut(&cap_id) {
            let cap_type = meta.cap_type.clone();
            meta.owner = to_task;
            // Keep status as Valid since it's being transferred to a valid owner
            
            // Record delegation event
            self.capability_audit.record_event(
                self.current_time,
                CapabilityEvent::Delegated {
                    cap_id,
                    from_task,
                    to_task,
                    cap_type,
                },
            );
        }

        Ok(())
    }

    /// Drops a capability (explicitly releases it)
    pub fn drop_capability(&mut self, cap_id: u64, owner: TaskId) -> Result<(), KernelError> {
        // Validate ownership
        self.validate_capability(cap_id, owner)
            .map_err(|reason| KernelError::InvalidCapability(format!("Cannot drop: {:?}", reason)))?;

        // Mark as invalid
        if let Some(meta) = self.capability_table.get_mut(&cap_id) {
            let cap_type = meta.cap_type.clone();
            meta.status = CapabilityStatus::Invalid;
            
            // Record drop event
            self.capability_audit.record_event(
                self.current_time,
                CapabilityEvent::Dropped {
                    cap_id,
                    owner,
                    cap_type,
                },
            );
        }

        Ok(())
    }

    /// Checks if a capability is valid for a given task (test helper)
    pub fn is_capability_valid(&self, cap_id: u64, task_id: TaskId) -> bool {
        self.validate_capability(cap_id, task_id).is_ok()
    }
}

impl Default for SimulatedKernel {
    fn default() -> Self {
        Self::new()
    }
}

impl KernelApi for SimulatedKernel {
    fn spawn_task(&mut self, descriptor: TaskDescriptor) -> Result<TaskHandle, KernelError> {
        let task_id = TaskId::new();
        let task_info = TaskInfo { descriptor };
        self.tasks.insert(task_id, task_info);
        Ok(TaskHandle::new(task_id))
    }

    fn create_channel(&mut self) -> Result<ChannelId, KernelError> {
        let channel_id = ChannelId::new();
        let channel = Channel {
            messages: VecDeque::new(),
        };
        self.channels.insert(channel_id, channel);
        Ok(channel_id)
    }

    fn send_message(
        &mut self,
        channel: ChannelId,
        message: MessageEnvelope,
    ) -> Result<(), KernelError> {
        // Check for crash-on-send fault
        if let Some(ref mut injector) = self.fault_injector {
            if injector.should_crash_on_send() {
                return Err(KernelError::SendFailed("Task crashed on send".to_string()));
            }

            // Check if message should be dropped
            if injector.should_drop_message(channel, &message) {
                // Message dropped by fault injector
                return Ok(());
            }

            // Check for delay
            if let Some(delay) = injector.get_message_delay() {
                let deliver_at = self.current_time + delay;
                self.delayed_messages.push(DelayedMessage {
                    channel,
                    message,
                    deliver_at,
                });
                return Ok(());
            }
        }

        let channel_obj = self
            .channels
            .get_mut(&channel)
            .ok_or_else(|| KernelError::ChannelError("Channel not found".to_string()))?;
        channel_obj.messages.push_back(message);

        // Apply reordering faults if present
        if let Some(ref injector) = self.fault_injector {
            injector.apply_reordering(&mut channel_obj.messages);
        }

        Ok(())
    }

    fn receive_message(
        &mut self,
        channel: ChannelId,
        _timeout: Option<Duration>,
    ) -> Result<MessageEnvelope, KernelError> {
        // Check for crash-on-recv fault
        if let Some(ref mut injector) = self.fault_injector {
            if injector.should_crash_on_recv() {
                return Err(KernelError::ReceiveFailed(
                    "Task crashed on recv".to_string(),
                ));
            }
        }

        let channel_obj = self
            .channels
            .get_mut(&channel)
            .ok_or_else(|| KernelError::ChannelError("Channel not found".to_string()))?;

        let message = channel_obj
            .messages
            .pop_front()
            .ok_or(KernelError::Timeout)?;

        // Record message processed for fault injection tracking
        if let Some(ref mut injector) = self.fault_injector {
            injector.record_message_processed();
        }

        Ok(message)
    }

    fn now(&self) -> Instant {
        self.current_time
    }

    fn sleep(&mut self, duration: Duration) -> Result<(), KernelError> {
        self.advance_time(duration);
        Ok(())
    }

    fn grant_capability(&mut self, task: TaskId, capability: Cap<()>) -> Result<(), KernelError> {
        // Verify task exists
        if !self.tasks.contains_key(&task) {
            return Err(KernelError::SendFailed("Task not found".to_string()));
        }
        
        // Record the capability grant in the authority table
        // For now, we use a generic type name since Cap<()> is type-erased
        self.record_capability_grant(capability.id(), task, "Generic".to_string(), None);
        
        Ok(())
    }

    fn register_service(
        &mut self,
        service_id: ServiceId,
        channel: ChannelId,
    ) -> Result<(), KernelError> {
        if self.services.contains_key(&service_id) {
            return Err(KernelError::ServiceAlreadyRegistered(
                service_id.to_string(),
            ));
        }
        self.services.insert(service_id, channel);
        Ok(())
    }

    fn lookup_service(&self, service_id: ServiceId) -> Result<ChannelId, KernelError> {
        self.services
            .get(&service_id)
            .copied()
            .ok_or_else(|| KernelError::ServiceNotFound(service_id.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simulated_kernel_creation() {
        let kernel = SimulatedKernel::new();
        assert_eq!(kernel.task_count(), 0);
        assert_eq!(kernel.channel_count(), 0);
    }

    #[test]
    fn test_spawn_task() {
        let mut kernel = SimulatedKernel::new();
        let descriptor = TaskDescriptor::new("test_task".to_string());
        let handle = kernel.spawn_task(descriptor).unwrap();
        assert_eq!(kernel.task_count(), 1);
        assert!(kernel.tasks.contains_key(&handle.task_id));
    }

    #[test]
    fn test_create_channel() {
        let mut kernel = SimulatedKernel::new();
        let channel = kernel.create_channel().unwrap();
        assert_eq!(kernel.channel_count(), 1);
        assert!(kernel.channels.contains_key(&channel));
    }

    #[test]
    fn test_send_receive_message() {
        let mut kernel = SimulatedKernel::new();
        let channel = kernel.create_channel().unwrap();

        let service_id = ServiceId::new();
        let payload = ipc::MessagePayload::new(&"test").unwrap();
        let message = ipc::MessageEnvelope::new(
            service_id,
            "test".to_string(),
            ipc::SchemaVersion::new(1, 0),
            payload,
        );

        kernel.send_message(channel, message.clone()).unwrap();
        let received = kernel.receive_message(channel, None).unwrap();
        assert_eq!(received.id, message.id);
    }

    #[test]
    fn test_time_advancement() {
        let mut kernel = SimulatedKernel::new();
        let initial = kernel.now();
        kernel.advance_time(Duration::from_secs(1));
        let after = kernel.now();
        assert_eq!(after.duration_since(initial), Duration::from_secs(1));
    }

    #[test]
    fn test_sleep() {
        let mut kernel = SimulatedKernel::new();
        let initial = kernel.now();
        kernel.sleep(Duration::from_millis(100)).unwrap();
        let after = kernel.now();
        assert_eq!(after.duration_since(initial), Duration::from_millis(100));
    }

    #[test]
    fn test_service_registration() {
        let mut kernel = SimulatedKernel::new();
        let service_id = ServiceId::new();
        let channel = kernel.create_channel().unwrap();

        kernel.register_service(service_id, channel).unwrap();
        assert_eq!(kernel.service_count(), 1);

        let looked_up = kernel.lookup_service(service_id).unwrap();
        assert_eq!(looked_up, channel);
    }

    #[test]
    fn test_duplicate_service_registration() {
        let mut kernel = SimulatedKernel::new();
        let service_id = ServiceId::new();
        let channel1 = kernel.create_channel().unwrap();
        let channel2 = kernel.create_channel().unwrap();

        kernel.register_service(service_id, channel1).unwrap();
        let result = kernel.register_service(service_id, channel2);
        assert!(matches!(
            result,
            Err(KernelError::ServiceAlreadyRegistered(_))
        ));
    }

    #[test]
    fn test_service_not_found() {
        let kernel = SimulatedKernel::new();
        let service_id = ServiceId::new();
        let result = kernel.lookup_service(service_id);
        assert!(matches!(result, Err(KernelError::ServiceNotFound(_))));
    }

    #[test]
    fn test_capability_grant_and_tracking() {
        let mut kernel = SimulatedKernel::new();
        let descriptor = TaskDescriptor::new("test_task".to_string());
        let handle = kernel.spawn_task(descriptor).unwrap();
        let task_id = handle.task_id;

        let cap: Cap<()> = Cap::new(42);
        kernel.grant_capability(task_id, cap).unwrap();

        // Check audit log
        let audit = kernel.audit_log();
        assert_eq!(audit.len(), 1);
        assert!(audit.has_event(|e| matches!(e, CapabilityEvent::Granted { .. })));
    }

    #[test]
    fn test_capability_delegation() {
        let mut kernel = SimulatedKernel::new();
        
        // Create two tasks
        let task1 = kernel
            .spawn_task(TaskDescriptor::new("task1".to_string()))
            .unwrap()
            .task_id;
        let task2 = kernel
            .spawn_task(TaskDescriptor::new("task2".to_string()))
            .unwrap()
            .task_id;

        // Grant capability to task1
        let cap: Cap<()> = Cap::new(42);
        kernel.grant_capability(task1, cap).unwrap();

        // Verify task1 can use it
        assert!(kernel.is_capability_valid(42, task1));

        // Delegate to task2
        kernel.delegate_capability(42, task1, task2).unwrap();

        // Now task2 owns it
        assert!(kernel.is_capability_valid(42, task2));
        // And task1 no longer owns it
        assert!(!kernel.is_capability_valid(42, task1));

        // Check audit log
        let audit = kernel.audit_log();
        assert_eq!(audit.len(), 2); // Grant + Delegate
        assert!(audit.has_event(|e| matches!(e, CapabilityEvent::Delegated { .. })));
    }

    #[test]
    fn test_capability_invalidation_on_task_death() {
        let mut kernel = SimulatedKernel::new();
        
        let task = kernel
            .spawn_task(TaskDescriptor::new("task".to_string()))
            .unwrap()
            .task_id;

        // Grant capability
        let cap: Cap<()> = Cap::new(42);
        kernel.grant_capability(task, cap).unwrap();

        // Capability is valid
        assert!(kernel.is_capability_valid(42, task));

        // Terminate task
        kernel.terminate_task(task);

        // Capability is no longer valid
        assert!(!kernel.is_capability_valid(42, task));

        // Check audit log
        let audit = kernel.audit_log();
        assert!(audit.has_event(|e| matches!(e, CapabilityEvent::Invalidated { .. })));
    }

    #[test]
    fn test_capability_drop() {
        let mut kernel = SimulatedKernel::new();
        
        let task = kernel
            .spawn_task(TaskDescriptor::new("task".to_string()))
            .unwrap()
            .task_id;

        // Grant capability
        let cap: Cap<()> = Cap::new(42);
        kernel.grant_capability(task, cap).unwrap();

        // Drop capability
        kernel.drop_capability(42, task).unwrap();

        // Capability is no longer valid
        assert!(!kernel.is_capability_valid(42, task));

        // Check audit log
        let audit = kernel.audit_log();
        assert!(audit.has_event(|e| matches!(e, CapabilityEvent::Dropped { .. })));
    }

    #[test]
    fn test_cannot_delegate_without_ownership() {
        let mut kernel = SimulatedKernel::new();
        
        let task1 = kernel
            .spawn_task(TaskDescriptor::new("task1".to_string()))
            .unwrap()
            .task_id;
        let task2 = kernel
            .spawn_task(TaskDescriptor::new("task2".to_string()))
            .unwrap()
            .task_id;

        // Try to delegate a capability task1 doesn't own
        let result = kernel.delegate_capability(999, task1, task2);
        assert!(result.is_err());
    }

    #[test]
    fn test_cannot_use_capability_after_delegation() {
        let mut kernel = SimulatedKernel::new();
        
        let task1 = kernel
            .spawn_task(TaskDescriptor::new("task1".to_string()))
            .unwrap()
            .task_id;
        let task2 = kernel
            .spawn_task(TaskDescriptor::new("task2".to_string()))
            .unwrap()
            .task_id;

        // Grant to task1
        let cap: Cap<()> = Cap::new(42);
        kernel.grant_capability(task1, cap).unwrap();

        // Delegate to task2
        kernel.delegate_capability(42, task1, task2).unwrap();

        // Task1 cannot delegate again (no longer owns it)
        let result = kernel.delegate_capability(42, task1, task2);
        assert!(result.is_err());
    }
}

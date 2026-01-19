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
pub mod policy_audit;
pub mod test_utils;

use core_types::{
    Cap, CapabilityEvent, CapabilityInvalidReason, CapabilityMetadata, CapabilityStatus, ServiceId,
    TaskId,
};
use fault_injection::FaultInjector;
use identity::{ExecutionId, ExitNotification, ExitReason, IdentityMetadata};
use ipc::{ChannelId, MessageEnvelope};
use kernel_api::{Duration, Instant, KernelApi, KernelError, TaskDescriptor, TaskHandle};
use policy::{PolicyContext, PolicyDecision, PolicyEngine, PolicyEvent};
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
    /// Identity table: ExecutionId -> IdentityMetadata
    identity_table: HashMap<ExecutionId, IdentityMetadata>,
    /// Task to identity mapping
    task_to_identity: HashMap<TaskId, ExecutionId>,
    /// Exit notifications (for supervision)
    exit_notifications: Vec<ExitNotification>,
    /// Optional policy engine for enforcement
    policy_engine: Option<Box<dyn PolicyEngine>>,
    /// Policy decision audit log (test-only)
    policy_audit: policy_audit::PolicyAuditLog,
}

#[derive(Debug)]
struct TaskInfo {
    #[allow(dead_code)]
    descriptor: TaskDescriptor,
    /// Execution identity for this task
    #[allow(dead_code)]
    execution_id: ExecutionId,
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
            identity_table: HashMap::new(),
            task_to_identity: HashMap::new(),
            exit_notifications: Vec::new(),
            policy_engine: None,
            policy_audit: policy_audit::PolicyAuditLog::new(),
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

    /// Sets the policy engine for this kernel
    ///
    /// This enables policy enforcement at enforcement points.
    /// If no policy engine is set, all operations are allowed.
    pub fn with_policy_engine(mut self, engine: Box<dyn PolicyEngine>) -> Self {
        self.policy_engine = Some(engine);
        self
    }

    /// Returns a reference to the policy audit log
    ///
    /// Used in tests to verify policy decisions were made correctly.
    pub fn policy_audit(&self) -> &policy_audit::PolicyAuditLog {
        &self.policy_audit
    }

    /// Evaluates policy for an event and context
    ///
    /// Returns Allow if no policy engine is configured.
    /// Records the decision in the policy audit log.
    fn evaluate_policy(&mut self, event: PolicyEvent, context: &PolicyContext) -> PolicyDecision {
        if let Some(engine) = &self.policy_engine {
            let decision = engine.evaluate(event.clone(), context);

            // Record decision in audit log
            let context_summary = format!(
                "actor={}, target={:?}, cap={:?}",
                context.actor_identity.name,
                context.target_identity.as_ref().map(|i| i.name.as_str()),
                context.capability_id
            );

            self.policy_audit.record_decision(
                self.current_time,
                event,
                engine.name().to_string(),
                decision.clone(),
                context_summary,
            );

            decision
        } else {
            PolicyDecision::Allow
        }
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
        self.terminate_task_with_reason(task_id, ExitReason::Normal);
    }

    /// Terminates a task with a specific exit reason
    ///
    /// Creates an exit notification with the specified reason and cleans up
    /// task resources including capabilities.
    pub fn terminate_task_with_reason(&mut self, task_id: TaskId, reason: ExitReason) {
        // Get execution ID and create exit notification
        if let Some(execution_id) = self.task_to_identity.get(&task_id).copied() {
            let notification = ExitNotification {
                execution_id,
                task_id: Some(task_id),
                reason,
                terminated_at_nanos: self.current_time.as_nanos(),
            };
            self.exit_notifications.push(notification);

            // Remove from task_to_identity mapping
            self.task_to_identity.remove(&task_id);
            // Note: we keep the identity metadata for audit purposes
        }

        // Remove task
        self.tasks.remove(&task_id);

        // Invalidate all capabilities owned by this task
        self.invalidate_task_capabilities(task_id);
    }

    /// Invalidates all capabilities owned by a task
    fn invalidate_task_capabilities(&mut self, task_id: TaskId) {
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
    fn validate_capability(
        &self,
        cap_id: u64,
        task_id: TaskId,
    ) -> Result<(), CapabilityInvalidReason> {
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
    fn record_capability_grant(
        &mut self,
        cap_id: u64,
        grantee: TaskId,
        cap_type: String,
        grantor: Option<TaskId>,
    ) {
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
    ///
    /// Trust boundary checks: Cross-domain delegation is logged for audit.
    pub fn delegate_capability(
        &mut self,
        cap_id: u64,
        from_task: TaskId,
        to_task: TaskId,
    ) -> Result<(), KernelError> {
        // Validate that the source task owns the capability
        self.validate_capability(cap_id, from_task)
            .map_err(|reason| {
                KernelError::InvalidCapability(format!("Cannot delegate: {:?}", reason))
            })?;

        // Verify target task exists
        if !self.tasks.contains_key(&to_task) {
            return Err(KernelError::SendFailed("Target task not found".to_string()));
        }

        // Check trust boundaries and policy enforcement
        let (from_identity, to_identity) = if let (Some(from_exec_id), Some(to_exec_id)) = (
            self.task_to_identity.get(&from_task),
            self.task_to_identity.get(&to_task),
        ) {
            if let (Some(from_id), Some(to_id)) = (
                self.identity_table.get(from_exec_id).cloned(),
                self.identity_table.get(to_exec_id).cloned(),
            ) {
                (Some(from_id), Some(to_id))
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        // Policy enforcement point: OnCapabilityDelegate
        if let (Some(ref from_id), Some(ref to_id)) = (from_identity, to_identity) {
            let context =
                PolicyContext::for_capability_delegation(from_id.clone(), to_id.clone(), cap_id);

            let decision = self.evaluate_policy(PolicyEvent::OnCapabilityDelegate, &context);

            match decision {
                PolicyDecision::Allow => {
                    // Continue with delegation
                }
                PolicyDecision::Deny { reason } => {
                    return Err(KernelError::InsufficientAuthority(format!(
                        "Policy denied capability delegation: {}",
                        reason
                    )));
                }
                PolicyDecision::Require { action } => {
                    return Err(KernelError::InsufficientAuthority(format!(
                        "Policy requires action before delegation: {}",
                        action
                    )));
                }
            }

            // If crossing trust domain boundary, log it
            if !from_id.same_domain(to_id) {
                // Record cross-domain delegation in audit log
                self.capability_audit.record_event(
                    self.current_time,
                    CapabilityEvent::CrossDomainDelegation {
                        cap_id,
                        from_task,
                        from_domain: from_id.trust_domain.name().to_string(),
                        to_task,
                        to_domain: to_id.trust_domain.name().to_string(),
                    },
                );
            }
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
        self.validate_capability(cap_id, owner).map_err(|reason| {
            KernelError::InvalidCapability(format!("Cannot drop: {:?}", reason))
        })?;

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

    /// Creates a new identity with the given metadata
    ///
    /// This is called internally when spawning tasks or can be used
    /// for supervisor-created identities.
    pub fn create_identity(&mut self, metadata: IdentityMetadata) -> ExecutionId {
        let execution_id = metadata.execution_id;
        self.identity_table.insert(execution_id, metadata);
        execution_id
    }

    /// Returns identity metadata for an execution
    pub fn get_identity(&self, execution_id: ExecutionId) -> Option<&IdentityMetadata> {
        self.identity_table.get(&execution_id)
    }

    /// Returns execution ID for a task
    pub fn get_task_identity(&self, task_id: TaskId) -> Option<ExecutionId> {
        self.task_to_identity.get(&task_id).copied()
    }

    /// Returns all exit notifications
    ///
    /// Used by supervisors to check for child terminations
    pub fn get_exit_notifications(&self) -> &[ExitNotification] {
        &self.exit_notifications
    }

    /// Clears exit notifications
    ///
    /// Should be called after supervisor processes notifications
    pub fn clear_exit_notifications(&mut self) {
        self.exit_notifications.clear();
    }

    /// Spawns a task with explicit identity metadata
    ///
    /// This is for supervisors who need full control over child identity
    pub fn spawn_task_with_identity(
        &mut self,
        descriptor: TaskDescriptor,
        kind: identity::IdentityKind,
        trust_domain: identity::TrustDomain,
        parent_id: Option<ExecutionId>,
        creator_id: Option<ExecutionId>,
    ) -> Result<(TaskHandle, ExecutionId), KernelError> {
        let task_id = TaskId::new();

        let mut metadata = identity::IdentityMetadata::new(
            kind,
            trust_domain,
            descriptor.name.clone(),
            self.current_time.as_nanos(),
        )
        .with_task_id(task_id);

        if let Some(parent) = parent_id {
            metadata = metadata.with_parent(parent);
        }
        if let Some(creator) = creator_id {
            metadata = metadata.with_creator(creator);
        }

        // Policy enforcement point: OnSpawn
        if let Some(creator_exec_id) = creator_id {
            if let Some(creator_identity) = self.identity_table.get(&creator_exec_id) {
                let context = PolicyContext::for_spawn(creator_identity.clone(), metadata.clone());

                let decision = self.evaluate_policy(PolicyEvent::OnSpawn, &context);

                match decision {
                    PolicyDecision::Allow => {
                        // Continue with spawn
                    }
                    PolicyDecision::Deny { reason } => {
                        return Err(KernelError::InsufficientAuthority(format!(
                            "Policy denied spawn: {}",
                            reason
                        )));
                    }
                    PolicyDecision::Require { action } => {
                        return Err(KernelError::InsufficientAuthority(format!(
                            "Policy requires action before spawn: {}",
                            action
                        )));
                    }
                }
            }
        }

        let execution_id = metadata.execution_id;

        // Store identity
        self.identity_table.insert(execution_id, metadata);
        self.task_to_identity.insert(task_id, execution_id);

        let task_info = TaskInfo {
            descriptor,
            execution_id,
        };
        self.tasks.insert(task_id, task_info);
        Ok((TaskHandle::new(task_id), execution_id))
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

        // Create execution identity for this task
        // Defaults: IdentityKind::Component, TrustDomain::user()
        // For full control over identity, use spawn_task_with_identity()
        let metadata = identity::IdentityMetadata::new(
            identity::IdentityKind::Component,
            identity::TrustDomain::user(),
            descriptor.name.clone(),
            self.current_time.as_nanos(),
        )
        .with_task_id(task_id);

        let execution_id = metadata.execution_id;

        // Store identity in kernel tables
        self.identity_table.insert(execution_id, metadata);
        self.task_to_identity.insert(task_id, execution_id);

        let task_info = TaskInfo {
            descriptor,
            execution_id,
        };
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

    #[test]
    fn test_identity_tracking() {
        let mut kernel = SimulatedKernel::new();

        let descriptor = TaskDescriptor::new("test_task".to_string());
        let handle = kernel.spawn_task(descriptor).unwrap();
        let task_id = handle.task_id;

        // Task should have an identity
        let exec_id = kernel.get_task_identity(task_id);
        assert!(exec_id.is_some());

        let identity = kernel.get_identity(exec_id.unwrap());
        assert!(identity.is_some());

        let identity = identity.unwrap();
        assert_eq!(identity.name, "test_task");
        assert_eq!(identity.task_id, Some(task_id));
    }

    #[test]
    fn test_exit_notifications() {
        let mut kernel = SimulatedKernel::new();

        let descriptor = TaskDescriptor::new("test_task".to_string());
        let handle = kernel.spawn_task(descriptor).unwrap();
        let task_id = handle.task_id;

        // No exit notifications initially
        assert_eq!(kernel.get_exit_notifications().len(), 0);

        // Terminate task
        kernel.terminate_task(task_id);

        // Should have one exit notification
        let notifications = kernel.get_exit_notifications();
        assert_eq!(notifications.len(), 1);
        assert_eq!(notifications[0].task_id, Some(task_id));
        assert_eq!(notifications[0].reason, ExitReason::Normal);
    }

    #[test]
    fn test_exit_notification_with_reason() {
        let mut kernel = SimulatedKernel::new();

        let descriptor = TaskDescriptor::new("test_task".to_string());
        let handle = kernel.spawn_task(descriptor).unwrap();
        let task_id = handle.task_id;

        // Terminate with failure reason
        kernel.terminate_task_with_reason(
            task_id,
            ExitReason::Failure {
                error: "test error".to_string(),
            },
        );

        let notifications = kernel.get_exit_notifications();
        assert_eq!(notifications.len(), 1);
        assert_eq!(
            notifications[0].reason,
            ExitReason::Failure {
                error: "test error".to_string()
            }
        );
    }

    #[test]
    fn test_spawn_with_identity() {
        let mut kernel = SimulatedKernel::new();

        // Spawn parent task
        let parent_desc = TaskDescriptor::new("parent".to_string());
        let (_parent_handle, parent_exec_id) = kernel
            .spawn_task_with_identity(
                parent_desc,
                identity::IdentityKind::Service,
                identity::TrustDomain::core(),
                None,
                None,
            )
            .unwrap();

        // Spawn child task
        let child_desc = TaskDescriptor::new("child".to_string());
        let (_child_handle, child_exec_id) = kernel
            .spawn_task_with_identity(
                child_desc,
                identity::IdentityKind::Component,
                identity::TrustDomain::core(),
                Some(parent_exec_id),
                Some(parent_exec_id),
            )
            .unwrap();

        // Verify parent-child relationship
        let child_identity = kernel.get_identity(child_exec_id).unwrap();
        assert!(child_identity.is_child_of(parent_exec_id));
        assert_eq!(child_identity.parent_id, Some(parent_exec_id));
        assert_eq!(child_identity.creator_id, Some(parent_exec_id));
    }

    #[test]
    fn test_trust_domain_delegation_same_domain() {
        let mut kernel = SimulatedKernel::new();

        // Create two tasks in the same trust domain
        let task1_desc = TaskDescriptor::new("task1".to_string());
        let (task1_handle, _) = kernel
            .spawn_task_with_identity(
                task1_desc,
                identity::IdentityKind::Component,
                identity::TrustDomain::core(),
                None,
                None,
            )
            .unwrap();

        let task2_desc = TaskDescriptor::new("task2".to_string());
        let (task2_handle, _) = kernel
            .spawn_task_with_identity(
                task2_desc,
                identity::IdentityKind::Component,
                identity::TrustDomain::core(),
                None,
                None,
            )
            .unwrap();

        // Grant capability to task1
        let cap: Cap<()> = Cap::new(42);
        kernel.grant_capability(task1_handle.task_id, cap).unwrap();

        // Delegate to task2
        kernel
            .delegate_capability(42, task1_handle.task_id, task2_handle.task_id)
            .unwrap();

        // Should succeed without cross-domain event
        let audit = kernel.audit_log();
        assert!(!audit.has_event(|e| matches!(e, CapabilityEvent::CrossDomainDelegation { .. })));
    }

    #[test]
    fn test_trust_domain_delegation_cross_domain() {
        let mut kernel = SimulatedKernel::new();

        // Create two tasks in different trust domains
        let task1_desc = TaskDescriptor::new("task1".to_string());
        let (task1_handle, _) = kernel
            .spawn_task_with_identity(
                task1_desc,
                identity::IdentityKind::Component,
                identity::TrustDomain::core(),
                None,
                None,
            )
            .unwrap();

        let task2_desc = TaskDescriptor::new("task2".to_string());
        let (task2_handle, _) = kernel
            .spawn_task_with_identity(
                task2_desc,
                identity::IdentityKind::Component,
                identity::TrustDomain::user(),
                None,
                None,
            )
            .unwrap();

        // Grant capability to task1
        let cap: Cap<()> = Cap::new(42);
        kernel.grant_capability(task1_handle.task_id, cap).unwrap();

        // Delegate to task2 (cross-domain)
        kernel
            .delegate_capability(42, task1_handle.task_id, task2_handle.task_id)
            .unwrap();

        // Should record cross-domain delegation event
        let audit = kernel.audit_log();
        assert!(audit.has_event(|e| matches!(e, CapabilityEvent::CrossDomainDelegation { .. })));

        // Verify the event details
        let events: Vec<&CapabilityEvent> = audit
            .get_events()
            .iter()
            .filter(|audit_event| {
                matches!(
                    audit_event.event,
                    CapabilityEvent::CrossDomainDelegation { .. }
                )
            })
            .map(|audit_event| &audit_event.event)
            .collect();

        assert_eq!(events.len(), 1);
        match events[0] {
            CapabilityEvent::CrossDomainDelegation {
                from_domain,
                to_domain,
                ..
            } => {
                assert_eq!(from_domain, "core");
                assert_eq!(to_domain, "user");
            }
            _ => panic!("Expected CrossDomainDelegation event"),
        }
    }

    #[test]
    fn test_policy_spawn_denied_by_trust_domain_policy() {
        use policy::TrustDomainPolicy;

        let mut kernel = SimulatedKernel::new().with_policy_engine(Box::new(TrustDomainPolicy));

        // Create a sandboxed task
        let sandbox_desc = TaskDescriptor::new("sandbox".to_string());
        let (_sandbox_handle, sandbox_exec_id) = kernel
            .spawn_task_with_identity(
                sandbox_desc,
                identity::IdentityKind::Component,
                identity::TrustDomain::sandbox(),
                None,
                None,
            )
            .unwrap();

        // Attempt to spawn a System service from sandbox (should be denied)
        let system_desc = TaskDescriptor::new("system-service".to_string());
        let result = kernel.spawn_task_with_identity(
            system_desc,
            identity::IdentityKind::System,
            identity::TrustDomain::core(),
            None,
            Some(sandbox_exec_id),
        );

        // Should be denied
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, KernelError::InsufficientAuthority(_)));

        // Check policy audit log
        let policy_audit = kernel.policy_audit();
        assert!(policy_audit.has_event(|e| {
            matches!(e.event, policy::PolicyEvent::OnSpawn) && e.decision.is_deny()
        }));
    }

    #[test]
    fn test_policy_capability_delegation_requires_approval() {
        use policy::TrustDomainPolicy;

        let mut kernel = SimulatedKernel::new().with_policy_engine(Box::new(TrustDomainPolicy));

        // Create tasks in different trust domains
        let core_desc = TaskDescriptor::new("core-service".to_string());
        let (core_handle, _) = kernel
            .spawn_task_with_identity(
                core_desc,
                identity::IdentityKind::Service,
                identity::TrustDomain::core(),
                None,
                None,
            )
            .unwrap();

        let user_desc = TaskDescriptor::new("user-component".to_string());
        let (user_handle, _) = kernel
            .spawn_task_with_identity(
                user_desc,
                identity::IdentityKind::Component,
                identity::TrustDomain::user(),
                None,
                None,
            )
            .unwrap();

        // Grant capability to core service
        let cap: Cap<()> = Cap::new(99);
        kernel.grant_capability(core_handle.task_id, cap).unwrap();

        // Attempt cross-domain delegation (should require approval)
        let result = kernel.delegate_capability(99, core_handle.task_id, user_handle.task_id);

        // Should be denied with "Require" decision
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, KernelError::InsufficientAuthority(_)));

        // Check policy audit log
        let policy_audit = kernel.policy_audit();
        assert!(policy_audit.has_event(|e| {
            matches!(e.event, policy::PolicyEvent::OnCapabilityDelegate) && e.decision.is_require()
        }));
    }

    #[test]
    fn test_policy_disabled_allows_all() {
        use policy::NoOpPolicy;

        let mut kernel = SimulatedKernel::new().with_policy_engine(Box::new(NoOpPolicy));

        // Create a sandboxed task
        let sandbox_desc = TaskDescriptor::new("sandbox".to_string());
        let (_, sandbox_exec_id) = kernel
            .spawn_task_with_identity(
                sandbox_desc,
                identity::IdentityKind::Component,
                identity::TrustDomain::sandbox(),
                None,
                None,
            )
            .unwrap();

        // Attempt to spawn a System service from sandbox (should be allowed with NoOpPolicy)
        let system_desc = TaskDescriptor::new("system-service".to_string());
        let result = kernel.spawn_task_with_identity(
            system_desc,
            identity::IdentityKind::System,
            identity::TrustDomain::core(),
            None,
            Some(sandbox_exec_id),
        );

        // Should succeed with NoOpPolicy
        assert!(result.is_ok());

        // Check policy audit log - should show Allow decisions
        let policy_audit = kernel.policy_audit();
        assert!(policy_audit.has_event(|e| {
            matches!(e.event, policy::PolicyEvent::OnSpawn) && e.decision.is_allow()
        }));
    }

    #[test]
    fn test_policy_composition_deny_wins() {
        use policy::{ComposedPolicy, NoOpPolicy, TrustDomainPolicy};

        let composed = ComposedPolicy::new()
            .add_policy(Box::new(NoOpPolicy))
            .add_policy(Box::new(TrustDomainPolicy));

        let mut kernel = SimulatedKernel::new().with_policy_engine(Box::new(composed));

        // Create a sandboxed task
        let sandbox_desc = TaskDescriptor::new("sandbox".to_string());
        let (_, sandbox_exec_id) = kernel
            .spawn_task_with_identity(
                sandbox_desc,
                identity::IdentityKind::Component,
                identity::TrustDomain::sandbox(),
                None,
                None,
            )
            .unwrap();

        // Attempt to spawn a System service from sandbox
        let system_desc = TaskDescriptor::new("system-service".to_string());
        let result = kernel.spawn_task_with_identity(
            system_desc,
            identity::IdentityKind::System,
            identity::TrustDomain::core(),
            None,
            Some(sandbox_exec_id),
        );

        // Should be denied because TrustDomainPolicy denies it (first deny wins)
        assert!(result.is_err());

        // Check policy audit log
        let policy_audit = kernel.policy_audit();
        assert!(policy_audit.has_event(|e| {
            matches!(e.event, policy::PolicyEvent::OnSpawn) && e.decision.is_deny()
        }));
    }
}

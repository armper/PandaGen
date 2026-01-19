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
pub mod resource_audit;
pub mod scheduler;
pub mod test_utils;
pub mod timer;

use core_types::{
    Cap, CapabilityEvent, CapabilityInvalidReason, CapabilityMetadata, CapabilityStatus, ServiceId,
    TaskId,
};
use fault_injection::FaultInjector;
use hal::TimerDevice;
use identity::{ExecutionId, ExitNotification, ExitReason, IdentityMetadata};
use ipc::{ChannelId, MessageEnvelope};
use kernel_api::{Duration, Instant, KernelApi, KernelError, TaskDescriptor, TaskHandle};
use policy::{PolicyContext, PolicyDecision, PolicyEngine, PolicyEvent};
use resources::CpuTicks;
use std::collections::{HashMap, VecDeque};

/// Simulated kernel state
///
/// This maintains all the state needed to simulate a kernel.
/// Unlike a real kernel, this state is directly accessible for testing.
pub struct SimulatedKernel {
    /// Timer device for tick tracking
    timer: crate::timer::SimTimerDevice,
    /// Current simulated time (derived from timer ticks)
    current_time: Instant,
    /// Nanoseconds per tick (for converting ticks to time)
    nanos_per_tick: u64,
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
    /// Resource consumption audit log (test-only, Phase 12)
    resource_audit: resource_audit::ResourceAuditLog,
    /// Cancelled execution IDs (Phase 12)
    cancelled_identities: HashMap<ExecutionId, String>,
    /// Current task context for receive operations (Phase 12)
    /// This is a workaround since KernelApi doesn't pass TaskId to receive_message
    current_receive_task: Option<TaskId>,
    /// Preemptive scheduler (Phase 23)
    scheduler: scheduler::Scheduler,
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
        Self::with_tick_resolution(Duration::from_micros(1))
    }

    /// Creates a new simulated kernel with a specific tick resolution
    ///
    /// This allows controlling the granularity of time in tests.
    /// For example, Duration::from_micros(1) means each tick = 1 microsecond.
    ///
    /// # Arguments
    ///
    /// * `tick_duration` - The duration represented by a single tick
    pub fn with_tick_resolution(tick_duration: Duration) -> Self {
        Self {
            timer: crate::timer::SimTimerDevice::new(),
            current_time: Instant::from_nanos(0),
            nanos_per_tick: tick_duration.as_nanos(),
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
            resource_audit: resource_audit::ResourceAuditLog::new(),
            cancelled_identities: HashMap::new(),
            current_receive_task: None,
            scheduler: scheduler::Scheduler::new(),
        }
    }

    /// Returns a reference to the timer device
    ///
    /// Useful for tests that need to directly manipulate time.
    pub fn timer(&self) -> &crate::timer::SimTimerDevice {
        &self.timer
    }

    /// Returns a mutable reference to the timer device
    ///
    /// Useful for tests that need to directly manipulate time.
    pub fn timer_mut(&mut self) -> &mut crate::timer::SimTimerDevice {
        &mut self.timer
    }

    /// Updates current_time based on timer ticks
    ///
    /// This is called internally after advancing the timer.
    fn sync_time_from_timer(&mut self) {
        let ticks = self.timer.poll_ticks();
        let nanos = ticks * self.nanos_per_tick;
        self.current_time = Instant::from_nanos(nanos);
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

    /// Sets the scheduler configuration for this kernel
    ///
    /// Phase 23: Configures the preemptive scheduler parameters.
    pub fn with_scheduler_config(mut self, config: scheduler::SchedulerConfig) -> Self {
        self.scheduler = scheduler::Scheduler::with_config(config);
        self
    }

    /// Returns a reference to the policy audit log
    ///
    /// Used in tests to verify policy decisions were made correctly.
    pub fn policy_audit(&self) -> &policy_audit::PolicyAuditLog {
        &self.policy_audit
    }

    /// Returns a reference to the resource audit log
    ///
    /// Phase 12: Used in tests to verify resource consumption and exhaustion.
    pub fn resource_audit(&self) -> &resource_audit::ResourceAuditLog {
        &self.resource_audit
    }

    /// Returns a reference to the scheduler audit log
    ///
    /// Phase 23: Used in tests to verify scheduling behavior.
    pub fn scheduler_audit(&self) -> &[scheduler::ScheduleEvent] {
        self.scheduler.audit_log()
    }

    /// Returns a reference to the scheduler
    ///
    /// Phase 23: Provides access to scheduler state for testing.
    pub fn scheduler(&self) -> &scheduler::Scheduler {
        &self.scheduler
    }

    /// Sets the current receive task context
    ///
    /// Phase 12: Workaround for KernelApi not passing TaskId to receive_message.
    /// Call this before receive_message to enable budget enforcement for receives.
    pub fn set_receive_context(&mut self, task_id: TaskId) {
        self.current_receive_task = Some(task_id);
    }

    /// Clears the current receive task context
    ///
    /// Phase 12: Call this after receive_message to clean up context.
    pub fn clear_receive_context(&mut self) {
        self.current_receive_task = None;
    }

    /// Attempts to consume CPU ticks for an execution identity
    ///
    /// Phase 12: External enforcement point for CPU consumption.
    /// Returns Err if budget is exhausted or identity is cancelled.
    ///
    /// Phase 22: This can be used with timer ticks for time-based CPU accounting.
    pub fn try_consume_cpu_ticks(
        &mut self,
        execution_id: ExecutionId,
        amount: u64,
    ) -> Result<(), KernelError> {
        // Check if identity is cancelled
        if self.is_identity_cancelled(execution_id) {
            return Err(KernelError::ResourceBudgetExhausted {
                resource_type: "CpuTicks (cancelled)".to_string(),
                limit: 0,
                usage: 0,
                identity: format!("{}", execution_id),
                operation: "cpu_consumption".to_string(),
            });
        }

        // Get identity metadata
        let identity = match self.identity_table.get_mut(&execution_id) {
            Some(id) => id,
            None => return Ok(()), // No identity - backward compat
        };

        // Check if identity has a budget
        let budget = match &identity.budget {
            Some(b) => b,
            None => return Ok(()), // No budget - unlimited
        };

        // Check current usage
        let current_usage = identity.usage.cpu_ticks.0;

        // Check if we would exceed the limit
        if let Some(limit) = budget.cpu_ticks {
            if current_usage + amount > limit.0 {
                // Budget exhausted - cancel identity and fail
                self.resource_audit.record_event(
                    self.current_time,
                    resource_audit::ResourceEvent::BudgetExhausted {
                        execution_id,
                        resource_type: "CpuTicks".to_string(),
                        limit: limit.0,
                        attempted_usage: current_usage + amount,
                        operation: "cpu_consumption".to_string(),
                    },
                );

                self.cancel_identity(execution_id, "CpuTicks".to_string());

                return Err(KernelError::ResourceBudgetExhausted {
                    resource_type: "CpuTicks".to_string(),
                    limit: limit.0,
                    usage: current_usage,
                    identity: format!("{}", execution_id),
                    operation: "cpu_consumption".to_string(),
                });
            }
        }

        // Consume the CPU ticks
        let before = current_usage;
        identity.usage.consume_cpu_ticks(CpuTicks::new(amount));
        let after = identity.usage.cpu_ticks.0;

        // Record audit event
        self.resource_audit.record_event(
            self.current_time,
            resource_audit::ResourceEvent::CpuConsumed {
                execution_id,
                amount,
                before,
                after,
            },
        );

        Ok(())
    }

    /// Attempts to consume a pipeline stage for an execution identity
    ///
    /// Phase 12: External enforcement point for pipeline stage consumption.
    /// Returns Err if budget is exhausted or identity is cancelled.
    pub fn try_consume_pipeline_stage(
        &mut self,
        execution_id: ExecutionId,
        stage_name: String,
    ) -> Result<(), KernelError> {
        // Check if identity is cancelled
        if self.is_identity_cancelled(execution_id) {
            return Err(KernelError::ResourceBudgetExhausted {
                resource_type: "PipelineStages (cancelled)".to_string(),
                limit: 0,
                usage: 0,
                identity: format!("{}", execution_id),
                operation: format!("pipeline_stage:{}", stage_name),
            });
        }

        // Get identity metadata
        let identity = match self.identity_table.get_mut(&execution_id) {
            Some(id) => id,
            None => return Ok(()), // No identity - backward compat
        };

        // Check if identity has a budget
        let budget = match &identity.budget {
            Some(b) => b,
            None => return Ok(()), // No budget - unlimited
        };

        // Check current usage
        let current_usage = identity.usage.pipeline_stages.0;

        // Check if we would exceed the limit
        if let Some(limit) = budget.pipeline_stages {
            if current_usage >= limit.0 {
                // Budget exhausted - cancel identity and fail
                self.resource_audit.record_event(
                    self.current_time,
                    resource_audit::ResourceEvent::BudgetExhausted {
                        execution_id,
                        resource_type: "PipelineStages".to_string(),
                        limit: limit.0,
                        attempted_usage: current_usage + 1,
                        operation: format!("pipeline_stage:{}", stage_name),
                    },
                );

                self.cancel_identity(execution_id, "PipelineStages".to_string());

                return Err(KernelError::ResourceBudgetExhausted {
                    resource_type: "PipelineStages".to_string(),
                    limit: limit.0,
                    usage: current_usage,
                    identity: format!("{}", execution_id),
                    operation: format!("pipeline_stage:{}", stage_name),
                });
            }
        }

        // Consume the pipeline stage
        let before = current_usage;
        identity.usage.consume_pipeline_stage();
        let after = identity.usage.pipeline_stages.0;

        // Record audit event
        self.resource_audit.record_event(
            self.current_time,
            resource_audit::ResourceEvent::PipelineStageConsumed {
                execution_id,
                stage_name,
                before,
                after,
            },
        );

        Ok(())
    }

    /// Cancels an execution identity due to resource exhaustion
    ///
    /// Phase 12: Marks the identity as cancelled. Further operations for this
    /// identity will be rejected.
    fn cancel_identity(&mut self, execution_id: ExecutionId, reason: String) {
        self.cancelled_identities
            .insert(execution_id, reason.clone());

        // Record in resource audit
        self.resource_audit.record_event(
            self.current_time,
            resource_audit::ResourceEvent::CancelledDueToExhaustion {
                execution_id,
                resource_type: reason,
            },
        );
        
        // Phase 23: Find and cancel the task in scheduler
        if let Some((&task_id, _)) = self.task_to_identity.iter().find(|(_, &exec_id)| exec_id == execution_id) {
            self.scheduler.cancel_task(task_id);
        }
    }

    /// Checks if an execution identity is cancelled
    ///
    /// Phase 12: Returns true if the identity has been cancelled due to
    /// resource exhaustion or other reasons.
    fn is_identity_cancelled(&self, execution_id: ExecutionId) -> bool {
        self.cancelled_identities.contains_key(&execution_id)
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
            PolicyDecision::Allow { derived: None }
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
        // Calculate how many ticks this duration represents
        let ticks_to_advance = duration.as_nanos() / self.nanos_per_tick;

        // Advance the timer
        self.timer.advance_ticks(ticks_to_advance);

        // Update current_time from timer
        self.sync_time_from_timer();

        // Process delayed messages
        self.process_delayed_messages();
        
        // Phase 23: Notify scheduler of tick advancement
        self.scheduler.on_tick_advanced(ticks_to_advance);
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

    /// Runs the scheduler for a specified number of ticks
    ///
    /// Phase 23: Executes scheduled tasks with preemption based on tick counts.
    /// This is a bounded stepping API for deterministic tests.
    ///
    /// # Arguments
    ///
    /// * `ticks` - Number of ticks to execute
    ///
    /// # Returns
    ///
    /// Returns the number of task scheduling rounds executed.
    pub fn run_for_ticks(&mut self, ticks: u64) -> usize {
        let mut scheduling_rounds = 0;
        let mut ticks_consumed = 0;

        while ticks_consumed < ticks && self.scheduler.has_runnable_tasks() {
            // Dequeue next task
            if let Some(task_id) = self.scheduler.dequeue_next() {
                scheduling_rounds += 1;

                // Run task until quantum expired or ticks exhausted
                let quantum = self.scheduler.config.quantum_ticks;
                let mut task_ticks = 0;

                while task_ticks < quantum && ticks_consumed < ticks {
                    // Check if task still exists and is runnable
                    if self.scheduler.task_state(task_id) != Some(scheduler::TaskState::Runnable) {
                        break;
                    }

                    // Execute one tick
                    let ticks_before = self.timer.current_ticks();
                    self.advance_time(Duration::from_nanos(self.nanos_per_tick));
                    let ticks_advanced = self.timer.current_ticks() - ticks_before;

                    ticks_consumed += ticks_advanced;
                    task_ticks += ticks_advanced;

                    // Try to consume CPU ticks (if task has identity)
                    if let Some(execution_id) = self.get_task_identity(task_id) {
                        if self.try_consume_cpu_ticks(execution_id, ticks_advanced).is_err() {
                            // Task cancelled due to budget exhaustion
                            break;
                        }
                    }
                }

                // Preempt if still runnable and quantum reached
                if self.scheduler.task_state(task_id) == Some(scheduler::TaskState::Runnable)
                    && task_ticks >= quantum
                {
                    self.scheduler.preempt_current();
                }
            } else {
                break;
            }
        }

        scheduling_rounds
    }

    /// Runs the scheduler for a specified number of steps
    ///
    /// Phase 23: Executes scheduled tasks for N scheduling decisions.
    /// Each step selects and runs one task for its quantum or until preemption.
    ///
    /// # Arguments
    ///
    /// * `steps` - Number of scheduling steps to execute
    ///
    /// # Returns
    ///
    /// Returns the number of steps actually executed.
    pub fn run_for_steps(&mut self, steps: usize) -> usize {
        let mut steps_executed = 0;

        for _ in 0..steps {
            if !self.scheduler.has_runnable_tasks() {
                break;
            }

            // Dequeue next task
            if let Some(task_id) = self.scheduler.dequeue_next() {
                steps_executed += 1;

                // Run task for its quantum
                let quantum = self.scheduler.config.quantum_ticks;
                for _ in 0..quantum {
                    // Check if task still exists and is runnable
                    if self.scheduler.task_state(task_id) != Some(scheduler::TaskState::Runnable) {
                        break;
                    }

                    // Execute one tick
                    let ticks_before = self.timer.current_ticks();
                    self.advance_time(Duration::from_nanos(self.nanos_per_tick));
                    let ticks_advanced = self.timer.current_ticks() - ticks_before;

                    // Try to consume CPU ticks
                    if let Some(execution_id) = self.get_task_identity(task_id) {
                        if self.try_consume_cpu_ticks(execution_id, ticks_advanced).is_err() {
                            // Task cancelled due to budget exhaustion
                            break;
                        }
                    }

                    // Check if task should be preempted
                    if self.scheduler.should_preempt(task_id) {
                        break;
                    }
                }

                // Preempt if still runnable
                if self.scheduler.task_state(task_id) == Some(scheduler::TaskState::Runnable) {
                    self.scheduler.preempt_current();
                }
            }
        }

        steps_executed
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
        
        // Phase 23: Notify scheduler that task has exited
        self.scheduler.exit_task(task_id);
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
                PolicyDecision::Allow { .. } => {
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

    /// Updates identity metadata (mutable borrow)
    ///
    /// Used by enforcement points to update usage.
    pub fn get_identity_mut(&mut self, exec_id: ExecutionId) -> Option<&mut IdentityMetadata> {
        self.identity_table.get_mut(&exec_id)
    }

    /// Phase 12: Attempts to consume a resource unit
    ///
    /// Checks budget, consumes resource, and records audit events.
    /// Returns Err if budget is exhausted or identity is cancelled.
    fn try_consume_message(
        &mut self,
        task_id: TaskId,
        operation: resource_audit::MessageOperation,
    ) -> Result<(), KernelError> {
        // Get execution ID for this task
        let execution_id = match self.task_to_identity.get(&task_id) {
            Some(id) => *id,
            None => {
                // No identity associated - allow operation (backward compat)
                return Ok(());
            }
        };

        // Check if identity is cancelled
        if self.is_identity_cancelled(execution_id) {
            return Err(KernelError::ResourceBudgetExhausted {
                resource_type: "MessageCount (cancelled)".to_string(),
                limit: 0,
                usage: 0,
                identity: format!("{}", execution_id),
                operation: format!("{:?}", operation),
            });
        }

        // Get identity metadata
        let identity = match self.identity_table.get_mut(&execution_id) {
            Some(id) => id,
            None => return Ok(()), // No identity - backward compat
        };

        // Check if identity has a budget
        let budget = match &identity.budget {
            Some(b) => b,
            None => return Ok(()), // No budget - unlimited
        };

        // Check current usage
        let current_usage = identity.usage.message_count.0;

        // Check if we would exceed the limit
        if let Some(limit) = budget.message_count {
            if current_usage >= limit.0 {
                // Budget exhausted - cancel identity and fail
                self.resource_audit.record_event(
                    self.current_time,
                    resource_audit::ResourceEvent::BudgetExhausted {
                        execution_id,
                        resource_type: "MessageCount".to_string(),
                        limit: limit.0,
                        attempted_usage: current_usage + 1,
                        operation: format!("{:?}", operation),
                    },
                );

                self.cancel_identity(execution_id, "MessageCount".to_string());

                return Err(KernelError::ResourceBudgetExhausted {
                    resource_type: "MessageCount".to_string(),
                    limit: limit.0,
                    usage: current_usage,
                    identity: format!("{}", execution_id),
                    operation: format!("{:?}", operation),
                });
            }
        }

        // Consume the message
        let before = current_usage;
        identity.usage.consume_message();
        let after = identity.usage.message_count.0;

        // Record audit event
        self.resource_audit.record_event(
            self.current_time,
            resource_audit::ResourceEvent::MessageConsumed {
                execution_id,
                operation,
                before,
                after,
            },
        );

        Ok(())
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

            // Phase 11: Validate budget inheritance
            if let Some(parent_identity) = self.identity_table.get(&parent) {
                if !metadata.budget_inherits_from(parent_identity) {
                    return Err(KernelError::InsufficientAuthority(
                        "Budget inheritance violation: child budget exceeds parent".to_string(),
                    ));
                }
            }
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
                    PolicyDecision::Allow { .. } => {
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
        
        // Phase 23: Enqueue task in scheduler
        self.scheduler.enqueue(task_id);
        
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
        
        // Phase 23: Enqueue task in scheduler
        self.scheduler.enqueue(task_id);
        
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
        // Phase 12: Try to enforce message budget if source task is known
        if let Some(source_task) = message.source {
            self.try_consume_message(source_task, resource_audit::MessageOperation::Send)?;
        }
        // else: No source - backward compat, skip enforcement

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
        // Phase 12: Try to enforce message budget if receive context is set
        if let Some(task_id) = self.current_receive_task {
            self.try_consume_message(task_id, resource_audit::MessageOperation::Receive)?;
        }
        // else: No context - backward compat, skip enforcement

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

    #[test]
    fn test_timer_integration_with_cpu_budget() {
        use resources::{CpuTicks, ResourceBudget};

        let mut kernel = SimulatedKernel::new();

        // Create an execution with a CPU tick budget
        let budget = ResourceBudget {
            cpu_ticks: Some(CpuTicks::new(1000)),
            memory_units: None,
            message_count: None,
            storage_ops: None,
            pipeline_stages: None,
        };

        let metadata = identity::IdentityMetadata::new(
            identity::IdentityKind::Component,
            identity::TrustDomain::user(),
            "budget-test".to_string(),
            kernel.now().as_nanos(),
        )
        .with_budget(budget);

        let exec_id = kernel.create_identity(metadata.clone());

        // Initial state: 0 ticks consumed
        let identity = kernel.get_identity(exec_id).unwrap();
        assert_eq!(identity.usage.cpu_ticks.0, 0);

        // Advance timer by some amount
        kernel.timer_mut().advance_ticks(100);
        kernel.sync_time_from_timer();

        // Consume CPU ticks based on timer advancement
        let result = kernel.try_consume_cpu_ticks(exec_id, 100);
        assert!(result.is_ok());

        // Check that usage was tracked
        let identity = kernel.get_identity(exec_id).unwrap();
        assert_eq!(identity.usage.cpu_ticks.0, 100);

        // Advance timer more
        kernel.timer_mut().advance_ticks(500);
        kernel.sync_time_from_timer();

        // Consume more ticks
        let result = kernel.try_consume_cpu_ticks(exec_id, 500);
        assert!(result.is_ok());

        let identity = kernel.get_identity(exec_id).unwrap();
        assert_eq!(identity.usage.cpu_ticks.0, 600);

        // Try to exceed budget
        kernel.timer_mut().advance_ticks(500);
        kernel.sync_time_from_timer();

        let result = kernel.try_consume_cpu_ticks(exec_id, 500);
        assert!(result.is_err());

        // Verify the identity was cancelled due to exhaustion
        let identity = kernel.get_identity(exec_id).unwrap();
        assert_eq!(identity.usage.cpu_ticks.0, 600); // Still at 600, consumption failed

        // Check resource audit log
        let audit = kernel.resource_audit();
        assert!(audit.has_event(|e| {
            matches!(
                e,
                resource_audit::ResourceEvent::BudgetExhausted {
                    execution_id: id,
                    resource_type,
                    ..
                } if *id == exec_id && resource_type == "CpuTicks"
            )
        }));
    }

    #[test]
    fn test_timer_deterministic_behavior() {
        // Create two kernels and run identical sequences
        let mut kernel1 = SimulatedKernel::new();
        let mut kernel2 = SimulatedKernel::new();

        // Advance both in the same way
        for i in 1..=5 {
            kernel1.advance_time(Duration::from_millis(i * 10));
            kernel2.advance_time(Duration::from_millis(i * 10));
        }

        // Both should have the same time
        assert_eq!(kernel1.now(), kernel2.now());
        assert_eq!(
            kernel1.timer().current_ticks(),
            kernel2.timer().current_ticks()
        );

        // Time should be cumulative
        assert_eq!(
            kernel1.now().as_nanos(),
            Duration::from_millis(10 + 20 + 30 + 40 + 50).as_nanos()
        );
    }

    #[test]
    fn test_timer_monotonic_with_advance_time() {
        let mut kernel = SimulatedKernel::new();

        let t1 = kernel.now();
        kernel.advance_time(Duration::from_millis(10));
        let t2 = kernel.now();
        kernel.advance_time(Duration::from_micros(500));
        let t3 = kernel.now();

        assert!(t2 > t1);
        assert!(t3 > t2);
        assert_eq!(t2.duration_since(t1), Duration::from_millis(10));
        assert_eq!(t3.duration_since(t2), Duration::from_micros(500));
    }

    #[test]
    fn test_scheduler_integration_task_enqueued() {
        let mut kernel = SimulatedKernel::new();

        let descriptor = TaskDescriptor::new("test_task".to_string());
        let handle = kernel.spawn_task(descriptor).unwrap();

        // Task should be enqueued in scheduler
        assert_eq!(kernel.scheduler().runnable_count(), 1);
        assert!(kernel.scheduler().has_runnable_tasks());

        // Terminate task - should be removed from scheduler
        kernel.terminate_task(handle.task_id);
        assert!(!kernel.scheduler().has_runnable_tasks());
    }

    #[test]
    fn test_scheduler_integration_two_tasks_interleave() {
        use resources::{CpuTicks, ResourceBudget};

        let config = scheduler::SchedulerConfig {
            quantum_ticks: 5,
            max_steps_per_tick: None,
        };
        let mut kernel = SimulatedKernel::new().with_scheduler_config(config);

        // Create two tasks with CPU budgets
        let budget = ResourceBudget {
            cpu_ticks: Some(CpuTicks::new(100)),
            memory_units: None,
            message_count: None,
            storage_ops: None,
            pipeline_stages: None,
        };

        let task1_desc = TaskDescriptor::new("task1".to_string());
        let (task1_handle, _) = kernel
            .spawn_task_with_identity(
                task1_desc,
                identity::IdentityKind::Component,
                identity::TrustDomain::user(),
                None,
                None,
            )
            .unwrap();

        // Set budget for task1
        if let Some(exec_id) = kernel.get_task_identity(task1_handle.task_id) {
            if let Some(identity) = kernel.get_identity_mut(exec_id) {
                identity.budget = Some(budget.clone());
            }
        }

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

        // Set budget for task2
        if let Some(exec_id) = kernel.get_task_identity(task2_handle.task_id) {
            if let Some(identity) = kernel.get_identity_mut(exec_id) {
                identity.budget = Some(budget);
            }
        }

        // Both tasks should be runnable
        assert_eq!(kernel.scheduler().runnable_count(), 2);

        // Run for some steps - tasks should interleave
        let steps = kernel.run_for_steps(4);
        assert_eq!(steps, 4);

        // Check scheduler audit log for interleaving
        let audit = kernel.scheduler_audit();
        assert!(audit.len() >= 4); // At least 2 selections + 2 preemptions

        // Verify both tasks were selected
        let task1_selected = audit
            .iter()
            .any(|e| matches!(e, scheduler::ScheduleEvent::TaskSelected { task_id, .. } if *task_id == task1_handle.task_id));
        let task2_selected = audit
            .iter()
            .any(|e| matches!(e, scheduler::ScheduleEvent::TaskSelected { task_id, .. } if *task_id == task2_handle.task_id));

        assert!(task1_selected, "task1 should have been selected");
        assert!(task2_selected, "task2 should have been selected");
    }

    #[test]
    fn test_scheduler_integration_preemption_events() {
        let config = scheduler::SchedulerConfig {
            quantum_ticks: 3,
            max_steps_per_tick: None,
        };
        let mut kernel = SimulatedKernel::new().with_scheduler_config(config);

        let descriptor = TaskDescriptor::new("test_task".to_string());
        let handle = kernel.spawn_task(descriptor).unwrap();

        // Run for a few ticks
        kernel.run_for_ticks(10);

        // Check that preemption events were recorded
        let audit = kernel.scheduler_audit();
        let preemption_count = audit
            .iter()
            .filter(|e| matches!(e, scheduler::ScheduleEvent::TaskPreempted { .. }))
            .count();

        // With quantum of 3 and 10 ticks, we should see at least 2 preemptions
        assert!(preemption_count >= 2, "Expected at least 2 preemptions, got {}", preemption_count);

        // Clean up
        kernel.terminate_task(handle.task_id);
    }

    #[test]
    fn test_scheduler_integration_budget_exhaustion() {
        use resources::{CpuTicks, ResourceBudget};

        let config = scheduler::SchedulerConfig {
            quantum_ticks: 5,
            max_steps_per_tick: None,
        };
        let mut kernel = SimulatedKernel::new().with_scheduler_config(config);

        // Create task with small CPU budget
        let budget = ResourceBudget {
            cpu_ticks: Some(CpuTicks::new(15)),
            memory_units: None,
            message_count: None,
            storage_ops: None,
            pipeline_stages: None,
        };

        let task_desc = TaskDescriptor::new("limited_task".to_string());
        let (task_handle, exec_id) = kernel
            .spawn_task_with_identity(
                task_desc,
                identity::IdentityKind::Component,
                identity::TrustDomain::user(),
                None,
                None,
            )
            .unwrap();

        // Set budget
        if let Some(identity) = kernel.get_identity_mut(exec_id) {
            identity.budget = Some(budget);
        }

        // Run until budget exhaustion
        kernel.run_for_ticks(20);

        // Task should be cancelled in scheduler
        assert_eq!(
            kernel.scheduler().task_state(task_handle.task_id),
            Some(scheduler::TaskState::Cancelled)
        );

        // Check scheduler audit for cancellation
        let audit = kernel.scheduler_audit();
        let cancelled = audit.iter().any(|e| {
            matches!(
                e,
                scheduler::ScheduleEvent::TaskExited {
                    task_id,
                    reason: scheduler::ExitReason::ResourceExhaustion,
                    ..
                } if *task_id == task_handle.task_id
            )
        });
        assert!(cancelled, "Task should have been cancelled due to budget exhaustion");
    }

    #[test]
    fn test_scheduler_integration_deterministic() {
        // Run same scenario twice - should get same results
        let _task1_id = TaskId::new();
        let _task2_id = TaskId::new();

        let config = scheduler::SchedulerConfig {
            quantum_ticks: 5,
            max_steps_per_tick: None,
        };

        let mut kernel1 = SimulatedKernel::new().with_scheduler_config(config.clone());
        let mut kernel2 = SimulatedKernel::new().with_scheduler_config(config);

        // Spawn same tasks
        let _ = kernel1.spawn_task(TaskDescriptor::new("task1".to_string()));
        let _ = kernel1.spawn_task(TaskDescriptor::new("task2".to_string()));
        let _ = kernel2.spawn_task(TaskDescriptor::new("task1".to_string()));
        let _ = kernel2.spawn_task(TaskDescriptor::new("task2".to_string()));

        // Run same number of steps
        kernel1.run_for_steps(10);
        kernel2.run_for_steps(10);

        // Both should have same number of audit events
        let audit1 = kernel1.scheduler_audit();
        let audit2 = kernel2.scheduler_audit();
        assert_eq!(audit1.len(), audit2.len(), "Should have same number of scheduling events");
    }
}

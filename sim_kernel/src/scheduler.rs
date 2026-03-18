//! Scheduler Foundation for Preemptive Multitasking
//!
//! Phase 23: This module provides a deterministic preemptive scheduler for SimKernel.
//!
//! ## Philosophy
//!
//! - **Mechanism, not policy**: This scheduler provides the foundation for
//!   preemption without imposing fairness or priority policies.
//! - **Determinism first**: Same inputs + same ticks => same schedule.
//! - **No hidden yields**: Preemption is explicit and testable.
//! - **Correctness over performance**: We aim for correct behavior, not optimal throughput.
//!
//! ## Design
//!
//! - **Round-robin scheduling**: Tasks are scheduled in FIFO order.
//! - **Time-sliced execution**: Each task gets a quantum of ticks before preemption.
//! - **No priorities**: All tasks are equal (for now).
//! - **No fairness guarantees**: We don't compensate for uneven execution.
//!
//! ## Future Hardware Seam
//!
//! This scheduler is designed for simulation but provides hooks for hardware:
//! - Hardware timer interrupts could trigger `on_tick_advanced()`
//! - Interrupt handler could call `should_preempt()` to decide context switches
//! - The scheduler state remains separate from interrupt handling logic

use core_types::TaskId;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Task state in the scheduler
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskState {
    /// Task is ready to run
    Runnable,
    /// Task is blocked waiting for a specific tick count
    /// The task will be unblocked when current_ticks >= wake_tick
    Blocked { wake_tick: u64 },
    /// Task has exited normally or abnormally
    Exited,
    /// Task was cancelled due to resource exhaustion
    Cancelled,
}

/// Scheduler configuration
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// Number of ticks a task can run before being preempted
    pub quantum_ticks: u64,
    /// Maximum steps per tick to guard against infinite loops (optional)
    pub max_steps_per_tick: Option<u64>,
    /// Real-time scheduling policy
    pub realtime_policy: RealTimePolicy,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            quantum_ticks: 10, // Small quantum for testing
            max_steps_per_tick: Some(1000),
            realtime_policy: RealTimePolicy::None,
        }
    }
}

/// Real-time scheduling policy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RealTimePolicy {
    None,
    EarliestDeadlineFirst,
}

/// Real-time task parameters
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RealTimeParams {
    pub period_ticks: u64,
    pub budget_ticks: u64,
}

/// Real-time task state tracked by scheduler
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RealTimeTask {
    pub params: RealTimeParams,
    pub next_deadline: u64,
    pub remaining_budget: u64,
    pub deadline_misses: u64,
}

impl RealTimeTask {
    fn new(params: RealTimeParams, current_ticks: u64) -> Self {
        Self {
            params,
            next_deadline: current_ticks.saturating_add(params.period_ticks),
            remaining_budget: params.budget_ticks,
            deadline_misses: 0,
        }
    }

    fn reset_for_next_period(&mut self) {
        self.remaining_budget = self.params.budget_ticks;
        self.next_deadline = self.next_deadline.saturating_add(self.params.period_ticks);
    }
}

/// Scheduler error for real-time admission and configuration
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchedulerError {
    TaskNotFound(TaskId),
    AdmissionControlFailed,
}

/// Scheduling event for audit trail
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScheduleEvent {
    /// Task was selected to run
    TaskSelected {
        task_id: TaskId,
        timestamp_ticks: u64,
    },
    /// Task was preempted
    TaskPreempted {
        task_id: TaskId,
        reason: PreemptionReason,
        timestamp_ticks: u64,
    },
    /// Task exited
    TaskExited {
        task_id: TaskId,
        reason: ExitReason,
        timestamp_ticks: u64,
    },
    /// Real-time deadline missed
    DeadlineMissed {
        task_id: TaskId,
        deadline_tick: u64,
        timestamp_ticks: u64,
    },
}

/// Reason for preemption
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PreemptionReason {
    /// Time quantum expired
    QuantumExpired,
    /// Task yielded voluntarily
    Yielded,
    /// Task blocked on I/O or other resource
    Blocked,
    /// Task exhausted its real-time budget
    BudgetExhausted,
}

/// Reason for task exit
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExitReason {
    /// Normal completion
    Normal,
    /// Cancelled due to resource exhaustion
    ResourceExhaustion,
    /// Crashed or failed
    Failed,
}

/// Task metadata tracked by scheduler
#[derive(Debug)]
struct TaskInfo {
    state: TaskState,
    /// Ticks consumed since last scheduling
    ticks_in_quantum: u64,
    realtime: Option<RealTimeTask>,
}

/// Run queue for tasks
///
/// This is a simple FIFO queue using VecDeque for deterministic ordering.
/// Tasks are enqueued at the back and dequeued from the front.
#[derive(Debug)]
struct RunQueue {
    queue: VecDeque<TaskId>,
}

impl RunQueue {
    fn new() -> Self {
        Self {
            queue: VecDeque::new(),
        }
    }

    fn enqueue(&mut self, task_id: TaskId) {
        self.queue.push_back(task_id);
    }

    fn dequeue(&mut self) -> Option<TaskId> {
        self.queue.pop_front()
    }

    fn dequeue_earliest_deadline<F>(&mut self, mut deadline_of: F) -> Option<TaskId>
    where
        F: FnMut(TaskId) -> Option<u64>,
    {
        let mut best_index: Option<usize> = None;
        let mut best_deadline: u64 = u64::MAX;

        for (index, task_id) in self.queue.iter().copied().enumerate() {
            if let Some(deadline) = deadline_of(task_id) {
                if deadline < best_deadline {
                    best_deadline = deadline;
                    best_index = Some(index);
                }
            }
        }

        if let Some(index) = best_index {
            return self.queue.remove(index);
        }

        self.dequeue()
    }

    fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    fn len(&self) -> usize {
        self.queue.len()
    }

    fn remove(&mut self, task_id: TaskId) {
        self.queue.retain(|&id| id != task_id);
    }
}

/// Preemptive scheduler
///
/// Manages task scheduling with time-sliced preemption.
pub struct Scheduler {
    pub(crate) config: SchedulerConfig,
    run_queue: RunQueue,
    tasks: std::collections::HashMap<TaskId, TaskInfo>,
    current_task: Option<TaskId>,
    current_ticks: u64,
    /// Audit log for scheduling events (test-only)
    audit_log: Vec<ScheduleEvent>,
}

impl Scheduler {
    /// Creates a new scheduler with default configuration
    pub fn new() -> Self {
        Self::with_config(SchedulerConfig::default())
    }

    /// Creates a new scheduler with custom configuration
    pub fn with_config(config: SchedulerConfig) -> Self {
        Self {
            config,
            run_queue: RunQueue::new(),
            tasks: std::collections::HashMap::new(),
            current_task: None,
            current_ticks: 0,
            audit_log: Vec::new(),
        }
    }

    /// Enqueues a task for execution
    ///
    /// The task is added to the run queue and marked as Runnable.
    pub fn enqueue(&mut self, task_id: TaskId) {
        let task_info = TaskInfo {
            state: TaskState::Runnable,
            ticks_in_quantum: 0,
            realtime: None,
        };
        self.tasks.insert(task_id, task_info);
        self.enqueue_runnable(task_id);
    }

    /// Dequeues the next task to run
    ///
    /// Returns None if no tasks are runnable.
    pub fn dequeue_next(&mut self) -> Option<TaskId> {
        let next = match self.config.realtime_policy {
            RealTimePolicy::None => self.run_queue.dequeue(),
            RealTimePolicy::EarliestDeadlineFirst => {
                let deadlines: std::collections::HashMap<TaskId, u64> = self
                    .tasks
                    .iter()
                    .filter_map(|(task_id, info)| {
                        info.realtime
                            .as_ref()
                            .map(|realtime| (*task_id, realtime.next_deadline))
                    })
                    .collect();

                self.run_queue
                    .dequeue_earliest_deadline(|task_id| deadlines.get(&task_id).copied())
            }
        };

        if let Some(task_id) = next {
            // Reset quantum counter for this task
            if let Some(task_info) = self.tasks.get_mut(&task_id) {
                task_info.ticks_in_quantum = 0;
            }
            self.current_task = Some(task_id);

            // Record audit event
            self.audit_log.push(ScheduleEvent::TaskSelected {
                task_id,
                timestamp_ticks: self.current_ticks,
            });

            Some(task_id)
        } else {
            self.current_task = None;
            None
        }
    }

    /// Advances the scheduler by the given number of ticks
    ///
    /// Updates tick counters for the currently running task.
    /// Also checks for blocked tasks that should be woken up.
    pub fn on_tick_advanced(&mut self, delta_ticks: u64) {
        self.current_ticks += delta_ticks;

        // Update quantum counter for current task
        if let Some(task_id) = self.current_task {
            if let Some(task_info) = self.tasks.get_mut(&task_id) {
                task_info.ticks_in_quantum += delta_ticks;
                if let Some(realtime) = task_info.realtime.as_mut() {
                    realtime.remaining_budget =
                        realtime.remaining_budget.saturating_sub(delta_ticks);
                }
            }
        }

        self.refresh_deadlines();

        // Wake up blocked tasks whose wake_tick has been reached
        self.wake_ready_tasks();
    }

    /// Sets real-time scheduling parameters for a task.
    pub fn set_real_time_params(
        &mut self,
        task_id: TaskId,
        params: RealTimeParams,
    ) -> Result<(), SchedulerError> {
        if params.period_ticks == 0
            || params.budget_ticks == 0
            || params.budget_ticks > params.period_ticks
        {
            return Err(SchedulerError::AdmissionControlFailed);
        }

        let utilization = self.utilization_ppm_with(task_id, params);
        if utilization > 1_000_000 {
            return Err(SchedulerError::AdmissionControlFailed);
        }

        let task_info = self
            .tasks
            .get_mut(&task_id)
            .ok_or(SchedulerError::TaskNotFound(task_id))?;
        task_info.realtime = Some(RealTimeTask::new(params, self.current_ticks));

        if task_info.state == TaskState::Runnable && self.current_task != Some(task_id) {
            self.run_queue.remove(task_id);
            self.enqueue_runnable(task_id);
        }

        Ok(())
    }

    /// Returns the real-time parameters for a task.
    pub fn real_time_params(&self, task_id: TaskId) -> Option<RealTimeParams> {
        self.tasks
            .get(&task_id)
            .and_then(|info| info.realtime.as_ref().map(|rt| rt.params))
    }

    /// Returns the current real-time utilization in parts-per-million.
    pub fn real_time_utilization_ppm(&self) -> u64 {
        let mut total: u128 = 0;
        for info in self.tasks.values() {
            if let Some(rt) = info.realtime.as_ref() {
                if rt.params.period_ticks > 0 {
                    total += (rt.params.budget_ticks as u128 * 1_000_000u128)
                        / rt.params.period_ticks as u128;
                }
            }
        }
        total as u64
    }

    /// Wakes up tasks that have reached their wake_tick
    ///
    /// This should be called after advancing time to unblock tasks
    /// that were sleeping and are now ready to run.
    pub fn wake_ready_tasks(&mut self) {
        let current_ticks = self.current_ticks;
        let tasks_to_wake: Vec<TaskId> = self
            .tasks
            .iter()
            .filter_map(|(task_id, info)| {
                if let TaskState::Blocked { wake_tick } = info.state {
                    if current_ticks >= wake_tick {
                        return Some(*task_id);
                    }
                }
                None
            })
            .collect();

        for task_id in tasks_to_wake {
            self.unblock_task(task_id);
        }
    }

    /// Checks if the current task should be preempted
    ///
    /// Returns true if the task has exceeded its quantum.
    pub fn should_preempt(&self, task_id: TaskId) -> bool {
        if let Some(task_info) = self.tasks.get(&task_id) {
            if let Some(realtime) = task_info.realtime.as_ref() {
                if realtime.remaining_budget == 0 {
                    return true;
                }
            }
            task_info.ticks_in_quantum >= self.config.quantum_ticks
        } else {
            false
        }
    }

    /// Preempts the current task and re-enqueues it
    ///
    /// The task is moved to the back of the run queue.
    /// Returns true if a task was preempted.
    pub fn preempt_current(&mut self) -> bool {
        if let Some(task_id) = self.current_task.take() {
            if let Some(task_info) = self.tasks.get_mut(&task_id) {
                if task_info.state == TaskState::Runnable {
                    if let Some(realtime) = task_info.realtime.as_ref() {
                        if realtime.remaining_budget == 0 {
                            let wake_tick = realtime.next_deadline;
                            task_info.state = TaskState::Blocked { wake_tick };
                            task_info.ticks_in_quantum = 0;
                            self.audit_log.push(ScheduleEvent::TaskPreempted {
                                task_id,
                                reason: PreemptionReason::BudgetExhausted,
                                timestamp_ticks: self.current_ticks,
                            });
                            return true;
                        }
                    }

                    // Reset quantum and re-enqueue
                    task_info.ticks_in_quantum = 0;
                    self.enqueue_runnable(task_id);

                    // Record audit event
                    self.audit_log.push(ScheduleEvent::TaskPreempted {
                        task_id,
                        reason: PreemptionReason::QuantumExpired,
                        timestamp_ticks: self.current_ticks,
                    });

                    return true;
                }
            }
        }
        false
    }

    /// Marks a task as blocked until a specific tick count
    ///
    /// Blocked tasks are not scheduled until they become runnable again.
    /// The task will be automatically unblocked when current_ticks >= wake_tick.
    pub fn block_task(&mut self, task_id: TaskId, wake_tick: u64) {
        if let Some(task_info) = self.tasks.get_mut(&task_id) {
            task_info.state = TaskState::Blocked { wake_tick };
            task_info.ticks_in_quantum = 0;
        }
        // Remove from run queue if present
        self.run_queue.remove(task_id);
        // Clear current task if it's being blocked
        if self.current_task == Some(task_id) {
            self.current_task = None;
        }
    }

    /// Marks a task as runnable (unblocks it)
    ///
    /// The task is added to the run queue.
    pub fn unblock_task(&mut self, task_id: TaskId) {
        if let Some(task_info) = self.tasks.get_mut(&task_id) {
            if matches!(task_info.state, TaskState::Blocked { .. }) {
                task_info.state = TaskState::Runnable;
                task_info.ticks_in_quantum = 0;
                self.enqueue_runnable(task_id);
            }
        }
    }

    /// Marks a task as exited
    ///
    /// Exited tasks are removed from scheduling.
    pub fn exit_task(&mut self, task_id: TaskId) {
        if let Some(task_info) = self.tasks.get_mut(&task_id) {
            task_info.state = TaskState::Exited;
        }
        // Remove from run queue if present
        self.run_queue.remove(task_id);
        // Clear current task if it's exiting
        if self.current_task == Some(task_id) {
            self.current_task = None;
        }

        // Record audit event
        self.audit_log.push(ScheduleEvent::TaskExited {
            task_id,
            reason: ExitReason::Normal,
            timestamp_ticks: self.current_ticks,
        });
    }

    /// Marks a task as cancelled
    ///
    /// Cancelled tasks (due to resource exhaustion) are removed from scheduling.
    pub fn cancel_task(&mut self, task_id: TaskId) {
        if let Some(task_info) = self.tasks.get_mut(&task_id) {
            task_info.state = TaskState::Cancelled;
        }
        // Remove from run queue if present
        self.run_queue.remove(task_id);
        // Clear current task if it's being cancelled
        if self.current_task == Some(task_id) {
            self.current_task = None;
        }

        // Record audit event
        self.audit_log.push(ScheduleEvent::TaskExited {
            task_id,
            reason: ExitReason::ResourceExhaustion,
            timestamp_ticks: self.current_ticks,
        });
    }

    /// Returns the currently running task
    pub fn current_task(&self) -> Option<TaskId> {
        self.current_task
    }

    /// Returns the number of runnable tasks in the queue
    pub fn runnable_count(&self) -> usize {
        self.run_queue.len()
    }

    /// Returns the task state
    pub fn task_state(&self, task_id: TaskId) -> Option<TaskState> {
        self.tasks.get(&task_id).map(|info| info.state)
    }

    /// Returns the current scheduler tick count
    pub fn current_ticks(&self) -> u64 {
        self.current_ticks
    }

    /// Returns true if there are runnable tasks
    pub fn has_runnable_tasks(&self) -> bool {
        !self.run_queue.is_empty()
    }

    /// Returns a reference to the audit log
    ///
    /// Used in tests to verify scheduling behavior.
    pub fn audit_log(&self) -> &[ScheduleEvent] {
        &self.audit_log
    }

    /// Clears the audit log
    pub fn clear_audit_log(&mut self) {
        self.audit_log.clear();
    }

    fn task_deadline(&self, task_id: TaskId) -> Option<u64> {
        self.tasks
            .get(&task_id)
            .and_then(|info| info.realtime.as_ref().map(|rt| rt.next_deadline))
    }

    fn enqueue_runnable(&mut self, task_id: TaskId) {
        match self.config.realtime_policy {
            RealTimePolicy::None => self.run_queue.enqueue(task_id),
            RealTimePolicy::EarliestDeadlineFirst => {
                if let Some(deadline) = self.task_deadline(task_id) {
                    let mut insert_pos = self.run_queue.queue.len();
                    for (index, existing) in self.run_queue.queue.iter().copied().enumerate() {
                        if let Some(existing_deadline) = self.task_deadline(existing) {
                            if deadline < existing_deadline {
                                insert_pos = index;
                                break;
                            }
                        }
                    }
                    self.run_queue.queue.insert(insert_pos, task_id);
                } else {
                    self.run_queue.enqueue(task_id);
                }
            }
        }
    }

    fn refresh_deadlines(&mut self) {
        let current_ticks = self.current_ticks;
        for (task_id, info) in self.tasks.iter_mut() {
            if let Some(realtime) = info.realtime.as_mut() {
                if realtime.params.period_ticks == 0 {
                    continue;
                }
                while current_ticks >= realtime.next_deadline {
                    if realtime.remaining_budget > 0 {
                        realtime.deadline_misses += 1;
                        self.audit_log.push(ScheduleEvent::DeadlineMissed {
                            task_id: *task_id,
                            deadline_tick: realtime.next_deadline,
                            timestamp_ticks: current_ticks,
                        });
                    }
                    realtime.reset_for_next_period();
                }
            }
        }
    }

    fn utilization_ppm_with(&self, task_id: TaskId, params: RealTimeParams) -> u64 {
        let mut total: u128 = 0;
        for (id, info) in self.tasks.iter() {
            if *id == task_id {
                if params.period_ticks > 0 {
                    total +=
                        (params.budget_ticks as u128 * 1_000_000u128) / params.period_ticks as u128;
                }
                continue;
            }
            if let Some(rt) = info.realtime.as_ref() {
                if rt.params.period_ticks > 0 {
                    total += (rt.params.budget_ticks as u128 * 1_000_000u128)
                        / rt.params.period_ticks as u128;
                }
            }
        }

        if params.period_ticks == 0 {
            return total as u64;
        }

        total as u64
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scheduler_creation() {
        let scheduler = Scheduler::new();
        assert_eq!(scheduler.runnable_count(), 0);
        assert_eq!(scheduler.current_task(), None);
        assert!(!scheduler.has_runnable_tasks());
    }

    #[test]
    fn test_enqueue_dequeue() {
        let mut scheduler = Scheduler::new();
        let task1 = TaskId::new();
        let task2 = TaskId::new();

        scheduler.enqueue(task1);
        scheduler.enqueue(task2);

        assert_eq!(scheduler.runnable_count(), 2);
        assert!(scheduler.has_runnable_tasks());

        let next = scheduler.dequeue_next();
        assert_eq!(next, Some(task1));
        assert_eq!(scheduler.current_task(), Some(task1));

        let next = scheduler.dequeue_next();
        assert_eq!(next, Some(task2));
        assert_eq!(scheduler.current_task(), Some(task2));

        let next = scheduler.dequeue_next();
        assert_eq!(next, None);
        assert_eq!(scheduler.current_task(), None);
    }

    #[test]
    fn test_round_robin_ordering() {
        let mut scheduler = Scheduler::new();
        let task1 = TaskId::new();
        let task2 = TaskId::new();
        let task3 = TaskId::new();

        // Enqueue in specific order
        scheduler.enqueue(task1);
        scheduler.enqueue(task2);
        scheduler.enqueue(task3);

        // Should dequeue in same order
        assert_eq!(scheduler.dequeue_next(), Some(task1));
        assert_eq!(scheduler.dequeue_next(), Some(task2));
        assert_eq!(scheduler.dequeue_next(), Some(task3));
    }

    #[test]
    fn test_quantum_preemption() {
        let config = SchedulerConfig {
            quantum_ticks: 10,
            max_steps_per_tick: None,
            realtime_policy: RealTimePolicy::None,
        };
        let mut scheduler = Scheduler::with_config(config);
        let task = TaskId::new();

        scheduler.enqueue(task);
        scheduler.dequeue_next();

        // Should not preempt before quantum
        assert!(!scheduler.should_preempt(task));

        // Advance ticks
        scheduler.on_tick_advanced(5);
        assert!(!scheduler.should_preempt(task));

        // Advance to quantum
        scheduler.on_tick_advanced(5);
        assert!(scheduler.should_preempt(task));

        // Advance beyond quantum
        scheduler.on_tick_advanced(1);
        assert!(scheduler.should_preempt(task));
    }

    #[test]
    fn test_preempt_and_reenqueue() {
        let config = SchedulerConfig {
            quantum_ticks: 5,
            max_steps_per_tick: None,
            realtime_policy: RealTimePolicy::None,
        };
        let mut scheduler = Scheduler::with_config(config);
        let task1 = TaskId::new();
        let task2 = TaskId::new();

        scheduler.enqueue(task1);
        scheduler.enqueue(task2);

        // Dequeue task1
        assert_eq!(scheduler.dequeue_next(), Some(task1));

        // Run for quantum
        scheduler.on_tick_advanced(5);
        assert!(scheduler.should_preempt(task1));

        // Preempt and re-enqueue
        assert!(scheduler.preempt_current());

        // task2 should be next
        assert_eq!(scheduler.dequeue_next(), Some(task2));

        // Then task1 again (round-robin)
        scheduler.on_tick_advanced(5);
        scheduler.preempt_current();
        assert_eq!(scheduler.dequeue_next(), Some(task1));
    }

    #[test]
    fn test_task_exit() {
        let mut scheduler = Scheduler::new();
        let task = TaskId::new();

        scheduler.enqueue(task);
        scheduler.dequeue_next();

        // Exit task
        scheduler.exit_task(task);

        assert_eq!(scheduler.task_state(task), Some(TaskState::Exited));
        assert_eq!(scheduler.current_task(), None);
        assert!(!scheduler.has_runnable_tasks());
    }

    #[test]
    fn test_task_cancellation() {
        let mut scheduler = Scheduler::new();
        let task = TaskId::new();

        scheduler.enqueue(task);
        scheduler.dequeue_next();

        // Cancel task
        scheduler.cancel_task(task);

        assert_eq!(scheduler.task_state(task), Some(TaskState::Cancelled));
        assert_eq!(scheduler.current_task(), None);
        assert!(!scheduler.has_runnable_tasks());
    }

    #[test]
    fn test_block_unblock() {
        let mut scheduler = Scheduler::new();
        let task = TaskId::new();

        scheduler.enqueue(task);
        scheduler.dequeue_next();

        // Block task until tick 100
        let wake_tick = 100;
        scheduler.block_task(task, wake_tick);
        assert_eq!(
            scheduler.task_state(task),
            Some(TaskState::Blocked { wake_tick })
        );
        assert!(!scheduler.has_runnable_tasks());

        // Unblock task
        scheduler.unblock_task(task);
        assert_eq!(scheduler.task_state(task), Some(TaskState::Runnable));
        assert!(scheduler.has_runnable_tasks());
    }

    #[test]
    fn test_deterministic_behavior() {
        // Two schedulers with same inputs should produce same results
        let task1 = TaskId::new();
        let task2 = TaskId::new();

        let mut sched1 = Scheduler::new();
        let mut sched2 = Scheduler::new();

        // Same enqueue order
        sched1.enqueue(task1);
        sched1.enqueue(task2);
        sched2.enqueue(task1);
        sched2.enqueue(task2);

        // Same dequeue results
        assert_eq!(sched1.dequeue_next(), sched2.dequeue_next());
        assert_eq!(sched1.dequeue_next(), sched2.dequeue_next());
    }

    #[test]
    fn test_quantum_reset_on_dequeue() {
        let config = SchedulerConfig {
            quantum_ticks: 10,
            max_steps_per_tick: None,
            realtime_policy: RealTimePolicy::None,
        };
        let mut scheduler = Scheduler::with_config(config);
        let task = TaskId::new();

        scheduler.enqueue(task);
        scheduler.dequeue_next();

        // Consume some ticks
        scheduler.on_tick_advanced(5);

        // Preempt and re-enqueue
        scheduler.preempt_current();

        // Dequeue again - quantum should be reset
        scheduler.dequeue_next();

        // Should not preempt immediately
        assert!(!scheduler.should_preempt(task));
    }

    #[test]
    fn test_audit_log_task_selected() {
        let mut scheduler = Scheduler::new();
        let task = TaskId::new();

        scheduler.enqueue(task);
        scheduler.dequeue_next();

        let log = scheduler.audit_log();
        assert_eq!(log.len(), 1);
        assert!(matches!(
            log[0],
            ScheduleEvent::TaskSelected { task_id, .. } if task_id == task
        ));
    }

    #[test]
    fn test_audit_log_task_preempted() {
        let mut scheduler = Scheduler::new();
        let task = TaskId::new();

        scheduler.enqueue(task);
        scheduler.dequeue_next();
        scheduler.on_tick_advanced(10);
        scheduler.preempt_current();

        let log = scheduler.audit_log();
        assert_eq!(log.len(), 2); // Selected + Preempted
        assert!(matches!(
            log[1],
            ScheduleEvent::TaskPreempted {
                task_id,
                reason: PreemptionReason::QuantumExpired,
                ..
            } if task_id == task
        ));
    }

    #[test]
    fn test_audit_log_task_exited() {
        let mut scheduler = Scheduler::new();
        let task = TaskId::new();

        scheduler.enqueue(task);
        scheduler.dequeue_next();
        scheduler.exit_task(task);

        let log = scheduler.audit_log();
        assert_eq!(log.len(), 2); // Selected + Exited
        assert!(matches!(
            log[1],
            ScheduleEvent::TaskExited {
                task_id,
                reason: ExitReason::Normal,
                ..
            } if task_id == task
        ));
    }

    #[test]
    fn test_audit_log_task_cancelled() {
        let mut scheduler = Scheduler::new();
        let task = TaskId::new();

        scheduler.enqueue(task);
        scheduler.dequeue_next();
        scheduler.cancel_task(task);

        let log = scheduler.audit_log();
        assert_eq!(log.len(), 2); // Selected + Exited(ResourceExhaustion)
        assert!(matches!(
            log[1],
            ScheduleEvent::TaskExited {
                task_id,
                reason: ExitReason::ResourceExhaustion,
                ..
            } if task_id == task
        ));
    }

    #[test]
    fn test_audit_log_interleaving() {
        let config = SchedulerConfig {
            quantum_ticks: 5,
            max_steps_per_tick: None,
            realtime_policy: RealTimePolicy::None,
        };
        let mut scheduler = Scheduler::with_config(config);
        let task1 = TaskId::new();
        let task2 = TaskId::new();

        scheduler.enqueue(task1);
        scheduler.enqueue(task2);

        // Run task1 for quantum
        scheduler.dequeue_next();
        scheduler.on_tick_advanced(5);
        scheduler.preempt_current();

        // Run task2 for quantum
        scheduler.dequeue_next();
        scheduler.on_tick_advanced(5);
        scheduler.preempt_current();

        let log = scheduler.audit_log();
        // Expected: Select(t1), Preempt(t1), Select(t2), Preempt(t2)
        assert_eq!(log.len(), 4);

        assert!(matches!(log[0], ScheduleEvent::TaskSelected { task_id, .. } if task_id == task1));
        assert!(matches!(log[1], ScheduleEvent::TaskPreempted { task_id, .. } if task_id == task1));
        assert!(matches!(log[2], ScheduleEvent::TaskSelected { task_id, .. } if task_id == task2));
        assert!(matches!(log[3], ScheduleEvent::TaskPreempted { task_id, .. } if task_id == task2));
    }

    #[test]
    fn test_blocked_task_automatic_wakeup() {
        let mut scheduler = Scheduler::new();
        let task = TaskId::new();

        scheduler.enqueue(task);
        scheduler.dequeue_next();

        // Block task until tick 50
        scheduler.block_task(task, 50);
        assert_eq!(
            scheduler.task_state(task),
            Some(TaskState::Blocked { wake_tick: 50 })
        );
        assert!(!scheduler.has_runnable_tasks());

        // Advance to tick 30 - should still be blocked
        scheduler.on_tick_advanced(30);
        assert_eq!(
            scheduler.task_state(task),
            Some(TaskState::Blocked { wake_tick: 50 })
        );
        assert!(!scheduler.has_runnable_tasks());

        // Advance to tick 50 - should wake up
        scheduler.on_tick_advanced(20);
        assert_eq!(scheduler.task_state(task), Some(TaskState::Runnable));
        assert!(scheduler.has_runnable_tasks());
    }

    #[test]
    fn test_multiple_blocked_tasks_wake_at_different_times() {
        let mut scheduler = Scheduler::new();
        let task1 = TaskId::new();
        let task2 = TaskId::new();
        let task3 = TaskId::new();

        scheduler.enqueue(task1);
        scheduler.enqueue(task2);
        scheduler.enqueue(task3);

        scheduler.dequeue_next();
        scheduler.block_task(task1, 10);

        scheduler.dequeue_next();
        scheduler.block_task(task2, 20);

        scheduler.dequeue_next();
        scheduler.block_task(task3, 30);

        assert!(!scheduler.has_runnable_tasks());

        // Advance to tick 10 - task1 should wake
        scheduler.on_tick_advanced(10);
        assert_eq!(scheduler.task_state(task1), Some(TaskState::Runnable));
        assert_eq!(
            scheduler.task_state(task2),
            Some(TaskState::Blocked { wake_tick: 20 })
        );
        assert_eq!(
            scheduler.task_state(task3),
            Some(TaskState::Blocked { wake_tick: 30 })
        );
        assert_eq!(scheduler.runnable_count(), 1);

        // Advance to tick 20 - task2 should wake
        scheduler.on_tick_advanced(10);
        assert_eq!(scheduler.task_state(task2), Some(TaskState::Runnable));
        assert_eq!(
            scheduler.task_state(task3),
            Some(TaskState::Blocked { wake_tick: 30 })
        );
        assert_eq!(scheduler.runnable_count(), 2);

        // Advance to tick 30 - task3 should wake
        scheduler.on_tick_advanced(10);
        assert_eq!(scheduler.task_state(task3), Some(TaskState::Runnable));
        assert_eq!(scheduler.runnable_count(), 3);
    }

    #[test]
    fn test_real_time_admission_control() {
        let mut scheduler = Scheduler::new();
        let task1 = TaskId::new();
        let task2 = TaskId::new();

        scheduler.enqueue(task1);
        scheduler.enqueue(task2);

        scheduler
            .set_real_time_params(
                task1,
                RealTimeParams {
                    period_ticks: 10,
                    budget_ticks: 6,
                },
            )
            .unwrap();

        let result = scheduler.set_real_time_params(
            task2,
            RealTimeParams {
                period_ticks: 10,
                budget_ticks: 6,
            },
        );

        assert_eq!(result, Err(SchedulerError::AdmissionControlFailed));
    }

    #[test]
    fn test_edf_orders_by_deadline() {
        let config = SchedulerConfig {
            quantum_ticks: 10,
            max_steps_per_tick: None,
            realtime_policy: RealTimePolicy::EarliestDeadlineFirst,
        };
        let mut scheduler = Scheduler::with_config(config);
        let task1 = TaskId::new();
        let task2 = TaskId::new();

        scheduler.enqueue(task1);
        scheduler.enqueue(task2);

        scheduler
            .set_real_time_params(
                task1,
                RealTimeParams {
                    period_ticks: 50,
                    budget_ticks: 10,
                },
            )
            .unwrap();
        scheduler
            .set_real_time_params(
                task2,
                RealTimeParams {
                    period_ticks: 20,
                    budget_ticks: 5,
                },
            )
            .unwrap();

        let first = scheduler.dequeue_next();
        assert_eq!(first, Some(task2));
        let second = scheduler.dequeue_next();
        assert_eq!(second, Some(task1));
    }

    #[test]
    fn test_real_time_budget_blocks_until_deadline() {
        let config = SchedulerConfig {
            quantum_ticks: 10,
            max_steps_per_tick: None,
            realtime_policy: RealTimePolicy::EarliestDeadlineFirst,
        };
        let mut scheduler = Scheduler::with_config(config);
        let task = TaskId::new();

        scheduler.enqueue(task);
        scheduler
            .set_real_time_params(
                task,
                RealTimeParams {
                    period_ticks: 20,
                    budget_ticks: 5,
                },
            )
            .unwrap();

        scheduler.dequeue_next();
        scheduler.on_tick_advanced(5);
        assert!(scheduler.should_preempt(task));
        scheduler.preempt_current();

        assert_eq!(
            scheduler.task_state(task),
            Some(TaskState::Blocked { wake_tick: 20 })
        );

        scheduler.on_tick_advanced(15);
        assert_eq!(scheduler.task_state(task), Some(TaskState::Runnable));
    }
}

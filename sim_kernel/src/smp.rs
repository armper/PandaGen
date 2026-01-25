//! SMP bring-up: multi-core scheduler and per-core time sources.

use crate::scheduler::{ExitReason, PreemptionReason, TaskState};
use core_types::TaskId;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

/// Identifier for a CPU core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CoreId(pub usize);

/// Per-core time sources (tick counters).
#[derive(Debug, Clone)]
pub struct PerCoreTimeSources {
    ticks: Vec<u64>,
}

impl PerCoreTimeSources {
    pub fn new(core_count: usize) -> Self {
        Self {
            ticks: vec![0; core_count],
        }
    }

    pub fn core_count(&self) -> usize {
        self.ticks.len()
    }

    pub fn ticks(&self, core_id: CoreId) -> u64 {
        self.ticks[core_id.0]
    }

    pub fn advance(&mut self, core_id: CoreId, delta: u64) {
        self.ticks[core_id.0] = self.ticks[core_id.0].saturating_add(delta);
    }
}

/// SMP scheduler configuration.
#[derive(Debug, Clone)]
pub struct SmpConfig {
    pub core_count: usize,
    pub quantum_ticks: u64,
}

impl Default for SmpConfig {
    fn default() -> Self {
        Self {
            core_count: 2,
            quantum_ticks: 10,
        }
    }
}

/// Scheduling event tagged with core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoreScheduleEvent {
    TaskSelected {
        core_id: CoreId,
        task_id: TaskId,
        timestamp_ticks: u64,
    },
    TaskPreempted {
        core_id: CoreId,
        task_id: TaskId,
        reason: PreemptionReason,
        timestamp_ticks: u64,
    },
    TaskExited {
        core_id: CoreId,
        task_id: TaskId,
        reason: ExitReason,
        timestamp_ticks: u64,
    },
}

#[derive(Debug)]
struct CoreState {
    run_queue: VecDeque<TaskId>,
    current_task: Option<TaskId>,
    ticks_in_quantum: u64,
}

impl CoreState {
    fn new() -> Self {
        Self {
            run_queue: VecDeque::new(),
            current_task: None,
            ticks_in_quantum: 0,
        }
    }
}

#[derive(Debug)]
struct TaskInfo {
    _state: TaskState,
}

/// Deterministic multi-core scheduler.
pub struct MultiCoreScheduler {
    config: SmpConfig,
    cores: Vec<CoreState>,
    tasks: HashMap<TaskId, TaskInfo>,
    next_core: usize,
    audit_log: Vec<CoreScheduleEvent>,
}

impl MultiCoreScheduler {
    pub fn new(config: SmpConfig) -> Self {
        let mut cores = Vec::with_capacity(config.core_count);
        for _ in 0..config.core_count {
            cores.push(CoreState::new());
        }
        Self {
            config,
            cores,
            tasks: HashMap::new(),
            next_core: 0,
            audit_log: Vec::new(),
        }
    }

    pub fn core_count(&self) -> usize {
        self.cores.len()
    }

    pub fn enqueue(&mut self, task_id: TaskId) {
        let task_info = TaskInfo {
            _state: TaskState::Runnable,
        };
        self.tasks.insert(task_id, task_info);
        let core_idx = self.next_core % self.cores.len();
        self.next_core += 1;
        self.cores[core_idx].run_queue.push_back(task_id);
    }

    pub fn dequeue_next(&mut self, core_id: CoreId, timestamp_ticks: u64) -> Option<TaskId> {
        let core = &mut self.cores[core_id.0];
        let task_id = core.run_queue.pop_front()?;
        core.current_task = Some(task_id);
        core.ticks_in_quantum = 0;
        self.audit_log.push(CoreScheduleEvent::TaskSelected {
            core_id,
            task_id,
            timestamp_ticks,
        });
        Some(task_id)
    }

    pub fn on_tick_advanced(&mut self, core_id: CoreId, delta: u64) {
        let core = &mut self.cores[core_id.0];
        if core.current_task.is_some() {
            core.ticks_in_quantum = core.ticks_in_quantum.saturating_add(delta);
        }
    }

    pub fn should_preempt(&self, core_id: CoreId) -> bool {
        let core = &self.cores[core_id.0];
        core.ticks_in_quantum >= self.config.quantum_ticks && core.current_task.is_some()
    }

    pub fn preempt_current(
        &mut self,
        core_id: CoreId,
        reason: PreemptionReason,
        timestamp_ticks: u64,
    ) {
        let core = &mut self.cores[core_id.0];
        if let Some(task_id) = core.current_task.take() {
            self.audit_log.push(CoreScheduleEvent::TaskPreempted {
                core_id,
                task_id,
                reason,
                timestamp_ticks,
            });
            core.ticks_in_quantum = 0;
            core.run_queue.push_back(task_id);
        }
    }

    pub fn exit_task(
        &mut self,
        core_id: CoreId,
        task_id: TaskId,
        reason: ExitReason,
        timestamp_ticks: u64,
    ) {
        self.tasks.remove(&task_id);
        let core = &mut self.cores[core_id.0];
        core.run_queue.retain(|id| *id != task_id);
        if core.current_task == Some(task_id) {
            core.current_task = None;
        }
        self.audit_log.push(CoreScheduleEvent::TaskExited {
            core_id,
            task_id,
            reason,
            timestamp_ticks,
        });
    }

    pub fn audit_log(&self) -> &[CoreScheduleEvent] {
        &self.audit_log
    }
}

/// SMP runtime combining scheduler + per-core time sources.
pub struct SmpRuntime {
    pub scheduler: MultiCoreScheduler,
    pub time: PerCoreTimeSources,
}

impl SmpRuntime {
    pub fn new(config: SmpConfig) -> Self {
        let time = PerCoreTimeSources::new(config.core_count);
        let scheduler = MultiCoreScheduler::new(config);
        Self { scheduler, time }
    }

    pub fn advance_core_time(&mut self, core_id: CoreId, ticks: u64) {
        self.time.advance(core_id, ticks);
        self.scheduler.on_tick_advanced(core_id, ticks);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_per_core_time_sources() {
        let mut time = PerCoreTimeSources::new(2);
        time.advance(CoreId(0), 5);
        time.advance(CoreId(1), 10);
        assert_eq!(time.ticks(CoreId(0)), 5);
        assert_eq!(time.ticks(CoreId(1)), 10);
    }

    #[test]
    fn test_multi_core_scheduler_round_robin() {
        let config = SmpConfig {
            core_count: 2,
            quantum_ticks: 3,
        };
        let mut runtime = SmpRuntime::new(config);

        let task1 = TaskId::new();
        let task2 = TaskId::new();
        runtime.scheduler.enqueue(task1);
        runtime.scheduler.enqueue(task2);

        let t1 = runtime
            .scheduler
            .dequeue_next(CoreId(0), runtime.time.ticks(CoreId(0)));
        let t2 = runtime
            .scheduler
            .dequeue_next(CoreId(1), runtime.time.ticks(CoreId(1)));
        assert_eq!(t1, Some(task1));
        assert_eq!(t2, Some(task2));

        runtime.advance_core_time(CoreId(0), 3);
        assert!(runtime.scheduler.should_preempt(CoreId(0)));
    }
}

//! Model-based checking of `sim_kernel::scheduler::Scheduler` (round-robin
//! policy). A reference model states the intended state machine; the real
//! scheduler is driven with the same operations and compared after every
//! step:
//!
//! - S1 agreement: every task's state, the current task, and the runnable
//!   count match the model;
//! - S2 order: `dequeue_next` returns exactly the task the FIFO model
//!   predicts, which also pins round-robin fairness and deterministic
//!   wake-up order;
//! - S3 dead tasks never run: exited or cancelled tasks are never selected
//!   and cannot be revived by block/unblock;
//! - S4 quantum: `should_preempt` agrees with the quantum accounting;
//! - S5 audit: selected, preempted, and exited counts match the operations
//!   that actually took effect (exit and cancel are idempotent).
//!
//! Exhaustive: every sequence of five operations over two tasks
//! (14^5 = 537,824 sequences). Random: 500 fixed-seed sequences of 60
//! operations over four tasks.

use core_types::TaskId;
use sim_kernel::scheduler::{
    ExitReason, PreemptionReason, RealTimePolicy, ScheduleEvent, Scheduler, SchedulerConfig,
    TaskState,
};
use std::collections::VecDeque;

const QUANTUM: u64 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Spawn(usize),
    Dequeue,
    Tick(u64),
    Preempt,
    Block(usize, u64),
    Unblock(usize),
    Exit(usize),
    Cancel(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Unspawned,
    Runnable,
    Blocked { wake: u64, seq: u64 },
    Exited,
    Cancelled,
}

#[derive(Debug, Clone)]
struct Model {
    states: Vec<State>,
    quantum_used: Vec<u64>,
    queue: VecDeque<usize>,
    current: Option<usize>,
    ticks: u64,
    next_seq: u64,
}

impl Model {
    fn new(tasks: usize) -> Self {
        Self {
            states: vec![State::Unspawned; tasks],
            quantum_used: vec![0; tasks],
            queue: VecDeque::new(),
            current: None,
            ticks: 0,
            next_seq: 0,
        }
    }

    fn is_dead(&self, t: usize) -> bool {
        matches!(
            self.states[t],
            State::Exited | State::Cancelled | State::Unspawned
        )
    }

    /// Returns what the operation produces: for Dequeue the selected task,
    /// for Preempt whether it preempted, and whether the op took effect.
    fn apply(&mut self, op: Op) -> (Option<usize>, bool) {
        match op {
            Op::Spawn(t) => {
                if self.states[t] != State::Unspawned {
                    return (None, false);
                }
                self.states[t] = State::Runnable;
                self.quantum_used[t] = 0;
                self.queue.push_back(t);
                (None, true)
            }
            Op::Dequeue => {
                let next = self.queue.pop_front();
                if let Some(t) = next {
                    self.quantum_used[t] = 0;
                }
                self.current = next;
                (next, next.is_some())
            }
            Op::Tick(n) => {
                self.ticks += n;
                if let Some(t) = self.current {
                    self.quantum_used[t] += n;
                }
                // Wake in (wake tick, block order) so the order is deterministic.
                let mut due: Vec<(u64, u64, usize)> = self
                    .states
                    .iter()
                    .enumerate()
                    .filter_map(|(t, s)| match s {
                        State::Blocked { wake, seq } if *wake <= self.ticks => {
                            Some((*wake, *seq, t))
                        }
                        _ => None,
                    })
                    .collect();
                due.sort();
                for (_, _, t) in due {
                    self.states[t] = State::Runnable;
                    self.quantum_used[t] = 0;
                    self.queue.push_back(t);
                }
                (None, true)
            }
            Op::Preempt => match self.current.take() {
                Some(t) => {
                    self.quantum_used[t] = 0;
                    self.queue.push_back(t);
                    (Some(t), true)
                }
                None => (None, false),
            },
            Op::Block(t, k) => {
                if self.is_dead(t) {
                    return (None, false);
                }
                let seq = self.next_seq;
                self.next_seq += 1;
                self.states[t] = State::Blocked {
                    wake: self.ticks + k,
                    seq,
                };
                self.quantum_used[t] = 0;
                self.queue.retain(|&q| q != t);
                if self.current == Some(t) {
                    self.current = None;
                }
                (None, true)
            }
            Op::Unblock(t) => {
                if !matches!(self.states[t], State::Blocked { .. }) {
                    return (None, false);
                }
                self.states[t] = State::Runnable;
                self.quantum_used[t] = 0;
                self.queue.push_back(t);
                (None, true)
            }
            Op::Exit(t) | Op::Cancel(t) => {
                if self.is_dead(t) {
                    return (None, false);
                }
                self.states[t] = if matches!(op, Op::Exit(_)) {
                    State::Exited
                } else {
                    State::Cancelled
                };
                self.queue.retain(|&q| q != t);
                if self.current == Some(t) {
                    self.current = None;
                }
                (None, true)
            }
        }
    }
}

struct Harness {
    scheduler: Scheduler,
    ids: Vec<TaskId>,
    model: Model,
    selected: usize,
    preempted: usize,
    exited: usize,
    cancelled: usize,
    trace: Vec<Op>,
}

impl Harness {
    fn new(tasks: usize) -> Self {
        let config = SchedulerConfig {
            quantum_ticks: QUANTUM,
            realtime_policy: RealTimePolicy::None,
        };
        Self {
            scheduler: Scheduler::with_config(config),
            ids: (0..tasks).map(|_| TaskId::new()).collect(),
            model: Model::new(tasks),
            selected: 0,
            preempted: 0,
            exited: 0,
            cancelled: 0,
            trace: Vec::new(),
        }
    }

    fn step(&mut self, op: Op) {
        self.trace.push(op);
        let (expected_task, effective) = self.model.apply(op);
        match op {
            Op::Spawn(t) => {
                if effective {
                    self.scheduler.enqueue(self.ids[t]);
                }
            }
            Op::Dequeue => {
                let got = self.scheduler.dequeue_next();
                let expected = expected_task.map(|t| self.ids[t]);
                assert_eq!(got, expected, "S2 dequeue order\ntrace: {:?}", self.trace);
                if got.is_some() {
                    self.selected += 1;
                }
            }
            Op::Tick(n) => self.scheduler.on_tick_advanced(n),
            Op::Preempt => {
                let got = self.scheduler.preempt_current();
                assert_eq!(got, effective, "preempt result\ntrace: {:?}", self.trace);
                if got {
                    self.preempted += 1;
                }
            }
            Op::Block(t, k) => {
                let wake = self.scheduler.current_ticks() + k;
                self.scheduler.block_task(self.ids[t], wake);
            }
            Op::Unblock(t) => self.scheduler.unblock_task(self.ids[t]),
            Op::Exit(t) => {
                self.scheduler.exit_task(self.ids[t]);
                if effective {
                    self.exited += 1;
                }
            }
            Op::Cancel(t) => {
                self.scheduler.cancel_task(self.ids[t]);
                if effective {
                    self.cancelled += 1;
                }
            }
        }
        self.check();
    }

    fn check(&self) {
        for (t, &id) in self.ids.iter().enumerate() {
            let expected = match self.model.states[t] {
                State::Unspawned => None,
                State::Runnable => Some(TaskState::Runnable),
                State::Blocked { wake, .. } => Some(TaskState::Blocked { wake_tick: wake }),
                State::Exited => Some(TaskState::Exited),
                State::Cancelled => Some(TaskState::Cancelled),
            };
            assert_eq!(
                self.scheduler.task_state(id),
                expected,
                "S1 state of task {t}\ntrace: {:?}",
                self.trace
            );
        }
        assert_eq!(
            self.scheduler.current_task(),
            self.model.current.map(|t| self.ids[t]),
            "S1 current task\ntrace: {:?}",
            self.trace
        );
        assert_eq!(
            self.scheduler.runnable_count(),
            self.model.queue.len(),
            "S1 runnable count\ntrace: {:?}",
            self.trace
        );
        if let Some(t) = self.model.current {
            assert_eq!(
                self.scheduler.should_preempt(self.ids[t]),
                self.model.quantum_used[t] >= QUANTUM,
                "S4 quantum for task {t}\ntrace: {:?}",
                self.trace
            );
        }
        let log = self.scheduler.audit_log();
        let count = |f: &dyn Fn(&ScheduleEvent) -> bool| log.iter().filter(|e| f(e)).count();
        assert_eq!(
            count(&|e| matches!(e, ScheduleEvent::TaskSelected { .. })),
            self.selected,
            "S5 selected events\ntrace: {:?}",
            self.trace
        );
        assert_eq!(
            count(&|e| matches!(
                e,
                ScheduleEvent::TaskPreempted {
                    reason: PreemptionReason::QuantumExpired,
                    ..
                }
            )),
            self.preempted,
            "S5 preempted events\ntrace: {:?}",
            self.trace
        );
        assert_eq!(
            count(&|e| matches!(
                e,
                ScheduleEvent::TaskExited {
                    reason: ExitReason::Normal,
                    ..
                }
            )),
            self.exited,
            "S5 exit events\ntrace: {:?}",
            self.trace
        );
        assert_eq!(
            count(&|e| matches!(
                e,
                ScheduleEvent::TaskExited {
                    reason: ExitReason::ResourceExhaustion,
                    ..
                }
            )),
            self.cancelled,
            "S5 cancel events\ntrace: {:?}",
            self.trace
        );
        // S3: no selection after a task's exit event.
        for &id in &self.ids {
            let mut dead = false;
            for event in log {
                match event {
                    ScheduleEvent::TaskExited { task_id, .. } if *task_id == id => dead = true,
                    ScheduleEvent::TaskSelected { task_id, .. } if *task_id == id => {
                        assert!(
                            !dead,
                            "S3 task selected after exit\ntrace: {:?}",
                            self.trace
                        );
                    }
                    _ => {}
                }
            }
        }
    }
}

fn all_ops(tasks: usize) -> Vec<Op> {
    let mut ops = vec![Op::Dequeue, Op::Tick(1), Op::Tick(3), Op::Preempt];
    for t in 0..tasks {
        ops.push(Op::Spawn(t));
        ops.push(Op::Block(t, 2));
        ops.push(Op::Unblock(t));
        ops.push(Op::Exit(t));
        ops.push(Op::Cancel(t));
    }
    ops
}

#[test]
fn exhaustive_depth_five_two_tasks() {
    let ops = all_ops(2);
    assert_eq!(ops.len(), 14);
    let mut count = 0usize;
    let mut stack: Vec<usize> = vec![0; 5];
    loop {
        let mut h = Harness::new(2);
        for &i in &stack {
            h.step(ops[i]);
        }
        count += 1;
        // Advance the odometer.
        let mut pos = stack.len();
        loop {
            if pos == 0 {
                assert_eq!(count, ops.len().pow(5));
                return;
            }
            pos -= 1;
            stack[pos] += 1;
            if stack[pos] < ops.len() {
                break;
            }
            stack[pos] = 0;
        }
    }
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

#[test]
fn random_sequences_four_tasks() {
    for seed in 1..=500u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut h = Harness::new(4);
        for _ in 0..60 {
            let t = rng.below(4) as usize;
            let op = match rng.below(12) {
                0 | 1 => Op::Spawn(t),
                2 | 3 | 4 => Op::Dequeue,
                5 | 6 => Op::Tick(1 + rng.below(4)),
                7 => Op::Preempt,
                8 => Op::Block(t, 1 + rng.below(3)),
                9 => Op::Unblock(t),
                10 => Op::Exit(t),
                _ => Op::Cancel(t),
            };
            h.step(op);
        }
    }
}

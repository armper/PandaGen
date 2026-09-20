//! Model-based checking of the earliest-deadline-first policy in
//! `sim_kernel::scheduler::Scheduler`.
//!
//! The model keeps the same run-queue list the scheduler keeps (real-time
//! tasks inserted before the first task with a later deadline, others
//! appended) and predicts:
//!
//! - E1 admission: `set_real_time_params` succeeds exactly when budget and
//!   period are sane and total utilisation stays at or below 100%;
//! - E2 selection: `dequeue_next` returns the queued task with the earliest
//!   deadline (first of equals), or the queue head when no queued task has
//!   a deadline;
//! - E3 budget: `should_preempt` fires when the budget is spent or the
//!   quantum expired, and a budget-exhausted task parks until its deadline;
//! - E4 periods: budgets and deadlines roll over on time, and a deadline
//!   miss is logged once per period in which a live task kept unspent
//!   budget (never for finished tasks);
//! - E5 agreement of task states, the current task, and the runnable count.
//!
//! Exhaustive: all 16^5 = 1,048,576 sequences of five operations over two
//! tasks. Random: 400 fixed-seed sequences of 60 operations over three tasks.

use core_types::TaskId;
use sim_kernel::scheduler::{
    RealTimeParams, RealTimePolicy, ScheduleEvent, Scheduler, SchedulerConfig, SchedulerError,
    TaskState,
};
use std::collections::BTreeMap;

const QUANTUM: u64 = 3;
const PARAMS: [(u64, u64); 4] = [(4, 1), (4, 2), (6, 3), (3, 3)];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Spawn(usize),
    SetRt(usize, usize),
    Dequeue,
    Tick(u64),
    Preempt,
    Block(usize, u64),
    Unblock(usize),
    Exit(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Unspawned,
    Runnable,
    Blocked { wake: u64 },
    Exited,
}

#[derive(Debug, Clone, Copy)]
struct Rt {
    period: u64,
    budget: u64,
    next_deadline: u64,
    remaining: u64,
}

#[derive(Debug, Clone)]
struct Model {
    states: Vec<State>,
    quantum_used: Vec<u64>,
    rt: Vec<Option<Rt>>,
    queue: Vec<usize>,
    current: Option<usize>,
    ticks: u64,
    misses: BTreeMap<usize, Vec<u64>>,
    block_seq: u64,
    block_order: Vec<u64>,
}

impl Model {
    fn new(tasks: usize) -> Self {
        Self {
            states: vec![State::Unspawned; tasks],
            quantum_used: vec![0; tasks],
            rt: vec![None; tasks],
            queue: Vec::new(),
            current: None,
            ticks: 0,
            misses: BTreeMap::new(),
            block_seq: 0,
            block_order: vec![0; tasks],
        }
    }

    fn finished(&self, t: usize) -> bool {
        matches!(self.states[t], State::Exited | State::Unspawned)
    }

    fn deadline(&self, t: usize) -> Option<u64> {
        self.rt[t].map(|r| r.next_deadline)
    }

    fn enqueue(&mut self, t: usize) {
        match self.deadline(t) {
            Some(d) => {
                let pos = self
                    .queue
                    .iter()
                    .position(|&q| matches!(self.deadline(q), Some(e) if d < e))
                    .unwrap_or(self.queue.len());
                self.queue.insert(pos, t);
            }
            None => self.queue.push(t),
        }
    }

    fn utilization_ppm(&self, replacing: Option<(usize, (u64, u64))>) -> u64 {
        let mut total = 0u64;
        for (t, rt) in self.rt.iter().enumerate() {
            if let Some((rt_task, (period, budget))) = replacing {
                if rt_task == t {
                    total += budget * 1_000_000 / period;
                    continue;
                }
            }
            if let Some(r) = rt {
                total += r.budget * 1_000_000 / r.period;
            }
        }
        total
    }

    /// Returns (accepted, selected task for Dequeue).
    fn apply(&mut self, op: Op) -> (bool, Option<usize>) {
        match op {
            Op::Spawn(t) => {
                if self.states[t] != State::Unspawned {
                    return (false, None);
                }
                self.states[t] = State::Runnable;
                self.quantum_used[t] = 0;
                self.rt[t] = None;
                self.enqueue(t);
                (true, None)
            }
            Op::SetRt(t, p) => {
                let (period, budget) = PARAMS[p];
                if self.states[t] == State::Unspawned {
                    return (false, None);
                }
                if self.utilization_ppm(Some((t, (period, budget)))) > 1_000_000 {
                    return (false, None);
                }
                self.rt[t] = Some(Rt {
                    period,
                    budget,
                    next_deadline: self.ticks + period,
                    remaining: budget,
                });
                if self.states[t] == State::Runnable && self.current != Some(t) {
                    self.queue.retain(|&q| q != t);
                    self.enqueue(t);
                }
                (true, None)
            }
            Op::Dequeue => {
                let mut best: Option<(usize, u64)> = None;
                for (i, &q) in self.queue.iter().enumerate() {
                    if let Some(d) = self.deadline(q) {
                        if best.is_none_or(|(_, bd)| d < bd) {
                            best = Some((i, d));
                        }
                    }
                }
                let next = match best {
                    Some((i, _)) => Some(self.queue.remove(i)),
                    None if !self.queue.is_empty() => Some(self.queue.remove(0)),
                    None => None,
                };
                if let Some(t) = next {
                    self.quantum_used[t] = 0;
                }
                self.current = next;
                (next.is_some(), next)
            }
            Op::Tick(n) => {
                self.ticks += n;
                if let Some(t) = self.current {
                    self.quantum_used[t] += n;
                    if let Some(r) = self.rt[t].as_mut() {
                        r.remaining = r.remaining.saturating_sub(n);
                    }
                }
                // Period rollover with deadline-miss accounting (live tasks only).
                for t in 0..self.rt.len() {
                    let finished = self.finished(t);
                    if let Some(r) = self.rt[t].as_mut() {
                        while self.ticks >= r.next_deadline {
                            if r.remaining > 0 && !finished {
                                self.misses.entry(t).or_default().push(r.next_deadline);
                            }
                            r.remaining = r.budget;
                            r.next_deadline += r.period;
                        }
                    }
                }
                // Wake-ups in (wake tick, task index) order — the checker only
                // uses one blocked wake per tick value per task so the order
                // among equal wake ticks is by block order, which here equals
                // task index order of blocking; keep a stable order.
                let mut due: Vec<(u64, usize)> = self
                    .states
                    .iter()
                    .enumerate()
                    .filter_map(|(t, s)| match s {
                        State::Blocked { wake } if *wake <= self.ticks => Some((*wake, t)),
                        _ => None,
                    })
                    .collect();
                due.sort_by_key(|&(w, t)| (w, self.block_order[t]));
                for (_, t) in due {
                    self.states[t] = State::Runnable;
                    self.quantum_used[t] = 0;
                    self.enqueue(t);
                }
                (true, None)
            }
            Op::Preempt => match self.current.take() {
                Some(t) => {
                    if let Some(r) = self.rt[t] {
                        if r.remaining == 0 {
                            self.states[t] = State::Blocked {
                                wake: r.next_deadline,
                            };
                            self.quantum_used[t] = 0;
                            self.note_block(t);
                            return (true, None);
                        }
                    }
                    self.quantum_used[t] = 0;
                    self.enqueue(t);
                    (true, None)
                }
                None => (false, None),
            },
            Op::Block(t, k) => {
                if self.finished(t) {
                    return (false, None);
                }
                self.states[t] = State::Blocked {
                    wake: self.ticks + k,
                };
                self.quantum_used[t] = 0;
                self.queue.retain(|&q| q != t);
                if self.current == Some(t) {
                    self.current = None;
                }
                self.note_block(t);
                (true, None)
            }
            Op::Unblock(t) => {
                if !matches!(self.states[t], State::Blocked { .. }) {
                    return (false, None);
                }
                self.states[t] = State::Runnable;
                self.quantum_used[t] = 0;
                self.enqueue(t);
                (true, None)
            }
            Op::Exit(t) => {
                if self.finished(t) {
                    return (false, None);
                }
                self.states[t] = State::Exited;
                // The reservation is released with the task.
                self.rt[t] = None;
                self.queue.retain(|&q| q != t);
                if self.current == Some(t) {
                    self.current = None;
                }
                (true, None)
            }
        }
    }
}

// Block ordering support lives outside `apply` to keep the match readable.
impl Model {
    fn note_block(&mut self, t: usize) {
        self.block_seq += 1;
        self.block_order[t] = self.block_seq;
    }
}

struct Harness {
    scheduler: Scheduler,
    ids: Vec<TaskId>,
    model: Model,
    trace: Vec<Op>,
}

impl Harness {
    fn new(tasks: usize) -> Self {
        let config = SchedulerConfig {
            quantum_ticks: QUANTUM,
            realtime_policy: RealTimePolicy::EarliestDeadlineFirst,
        };
        Self {
            scheduler: Scheduler::with_config(config),
            ids: (0..tasks).map(|_| TaskId::new()).collect(),
            model: Model::new(tasks),
            trace: Vec::new(),
        }
    }

    fn step(&mut self, op: Op) {
        self.trace.push(op);
        let (accepted, selected) = self.model.apply(op);
        match op {
            Op::Spawn(t) => {
                if accepted {
                    self.scheduler.enqueue(self.ids[t]);
                }
            }
            Op::SetRt(t, p) => {
                if self.model.states[t] == State::Unspawned && !accepted {
                    // The scheduler reports TaskNotFound; nothing to compare.
                    let (period_ticks, budget_ticks) = PARAMS[p];
                    let result = self.scheduler.set_real_time_params(
                        self.ids[t],
                        RealTimeParams {
                            period_ticks,
                            budget_ticks,
                        },
                    );
                    assert!(
                        matches!(
                            result,
                            Err(SchedulerError::TaskNotFound(_))
                                | Err(SchedulerError::AdmissionControlFailed)
                        ),
                        "E1 unspawned {op:?}: {result:?}\ntrace: {:?}",
                        self.trace
                    );
                } else {
                    let (period_ticks, budget_ticks) = PARAMS[p];
                    let result = self.scheduler.set_real_time_params(
                        self.ids[t],
                        RealTimeParams {
                            period_ticks,
                            budget_ticks,
                        },
                    );
                    assert_eq!(
                        result.is_ok(),
                        accepted,
                        "E1 admission {op:?}: {result:?}\ntrace: {:?}",
                        self.trace
                    );
                }
            }
            Op::Dequeue => {
                let got = self.scheduler.dequeue_next();
                assert_eq!(
                    got,
                    selected.map(|t| self.ids[t]),
                    "E2 selection\ntrace: {:?}",
                    self.trace
                );
            }
            Op::Tick(n) => self.scheduler.on_tick_advanced(n),
            Op::Preempt => {
                let got = self.scheduler.preempt_current();
                assert_eq!(got, accepted, "E3 preempt result\ntrace: {:?}", self.trace);
            }
            Op::Block(t, k) => {
                let wake = self.scheduler.current_ticks() + k;
                self.scheduler.block_task(self.ids[t], wake);
            }
            Op::Unblock(t) => self.scheduler.unblock_task(self.ids[t]),
            Op::Exit(t) => self.scheduler.exit_task(self.ids[t]),
        }
        self.check();
    }

    fn check(&self) {
        for (t, &id) in self.ids.iter().enumerate() {
            let expected = match self.model.states[t] {
                State::Unspawned => None,
                State::Runnable => Some(TaskState::Runnable),
                State::Blocked { wake } => Some(TaskState::Blocked { wake_tick: wake }),
                State::Exited => Some(TaskState::Exited),
            };
            assert_eq!(
                self.scheduler.task_state(id),
                expected,
                "E5 state of task {t}\ntrace: {:?}",
                self.trace
            );
        }
        assert_eq!(
            self.scheduler.current_task(),
            self.model.current.map(|t| self.ids[t]),
            "E5 current\ntrace: {:?}",
            self.trace
        );
        assert_eq!(
            self.scheduler.runnable_count(),
            self.model.queue.len(),
            "E5 runnable count\ntrace: {:?}",
            self.trace
        );
        assert_eq!(
            self.scheduler.real_time_utilization_ppm(),
            self.model.utilization_ppm(None),
            "E1 utilisation\ntrace: {:?}",
            self.trace
        );
        if let Some(t) = self.model.current {
            let expected = self.model.rt[t].is_some_and(|r| r.remaining == 0)
                || self.model.quantum_used[t] >= QUANTUM;
            assert_eq!(
                self.scheduler.should_preempt(self.ids[t]),
                expected,
                "E3 should_preempt for task {t}\ntrace: {:?}",
                self.trace
            );
        }
        // E4: deadline misses per task, as multisets of deadline ticks.
        for (t, &id) in self.ids.iter().enumerate() {
            let mut logged: Vec<u64> = self
                .scheduler
                .audit_log()
                .iter()
                .filter_map(|e| match e {
                    ScheduleEvent::DeadlineMissed {
                        task_id,
                        deadline_tick,
                        ..
                    } if *task_id == id => Some(*deadline_tick),
                    _ => None,
                })
                .collect();
            logged.sort_unstable();
            let mut expected = self.model.misses.get(&t).cloned().unwrap_or_default();
            expected.sort_unstable();
            assert_eq!(
                logged, expected,
                "E4 deadline misses for task {t}\ntrace: {:?}",
                self.trace
            );
        }
    }
}

fn all_ops(tasks: usize) -> Vec<Op> {
    let mut ops = vec![Op::Dequeue, Op::Tick(1), Op::Tick(2), Op::Preempt];
    for t in 0..tasks {
        ops.push(Op::Spawn(t));
        ops.push(Op::SetRt(t, 1));
        ops.push(Op::SetRt(t, 3));
        ops.push(Op::Block(t, 2));
        ops.push(Op::Unblock(t));
        ops.push(Op::Exit(t));
    }
    ops
}

#[test]
fn exhaustive_depth_five_two_tasks() {
    let ops = all_ops(2);
    assert_eq!(ops.len(), 16);
    let mut count = 0usize;
    let mut stack = vec![0usize; 5];
    loop {
        let mut h = Harness::new(2);
        for &i in &stack {
            h.step(ops[i]);
        }
        count += 1;
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
fn random_sequences_three_tasks() {
    for seed in 1..=400u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut h = Harness::new(3);
        for _ in 0..60 {
            let t = rng.below(3) as usize;
            let op = match rng.below(12) {
                0 => Op::Spawn(t),
                1 | 2 => Op::SetRt(t, rng.below(4) as usize),
                3 | 4 | 5 => Op::Dequeue,
                6 | 7 => Op::Tick(1 + rng.below(3)),
                8 => Op::Preempt,
                9 => Op::Block(t, 1 + rng.below(3)),
                10 => Op::Unblock(t),
                _ => Op::Exit(t),
            };
            h.step(op);
        }
    }
}

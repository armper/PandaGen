//! Model-based checking of the capability lifecycle in `sim_kernel`.
//!
//! A small reference model states what grant, lease, delegate, revoke,
//! drop, time, and task death must do. The real kernel is driven through
//! the same operations and compared with the model after every step:
//!
//! - I0 acceptance: the kernel accepts exactly the operations the model accepts;
//! - I1 agreement: `is_capability_valid(cap, task)` matches the model for every pair;
//! - I2 single owner: a capability is valid for at most one task;
//! - I3 no resurrection: once a capability is valid for nobody it stays that
//!   way until it is explicitly re-issued by a new grant;
//! - I4 audit: every successful revoke, delegate, drop, and every lease expiry
//!   shows up in the audit log;
//! - I5 failed operations do not change validity.
//!
//! Two explorations run: an exhaustive search of every operation sequence of
//! length three over two tasks and two capabilities (complete for that
//! bounded domain), and long random sequences from a fixed-seed xorshift
//! generator (deterministic, dependency-free, like the compositor property
//! tests).

use core_types::{Cap, CapabilityEvent, TaskId};
use kernel_api::{Duration, KernelApi, TaskDescriptor};
use sim_kernel::SimulatedKernel;
use std::collections::{BTreeMap, BTreeSet};

const NANOS_PER_MS: u64 = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Grant { task: usize, cap: u64 },
    Lease { task: usize, cap: u64, ms: u64 },
    Delegate { cap: u64, from: usize, to: usize },
    Revoke { task: usize, cap: u64 },
    Drop { task: usize, cap: u64 },
    Advance { ms: u64 },
    Kill { task: usize },
}

#[derive(Debug, Clone)]
struct CapState {
    owner: usize,
    invalid: bool,
    expires_at: Option<u64>,
}

/// Reference model of the capability table.
#[derive(Debug, Clone)]
struct Model {
    now: u64,
    alive: Vec<bool>,
    caps: BTreeMap<u64, CapState>,
}

impl Model {
    fn new(tasks: usize) -> Self {
        Self {
            now: 0,
            alive: vec![true; tasks],
            caps: BTreeMap::new(),
        }
    }

    fn valid_for(&self, cap: u64, task: usize) -> bool {
        match self.caps.get(&cap) {
            None => false,
            Some(c) => {
                !c.invalid
                    && c.owner == task
                    && self.alive[c.owner]
                    && c.expires_at.is_none_or(|e| e > self.now)
            }
        }
    }

    fn usable(&self, cap: u64, task: usize) -> bool {
        self.valid_for(cap, task)
    }

    /// An id can be (re-)issued when no task can use it.
    fn issuable(&self, cap: u64) -> bool {
        match self.caps.get(&cap) {
            None => true,
            Some(c) => !self.valid_for(cap, c.owner),
        }
    }

    /// Apply an operation; returns whether the model accepts it.
    fn apply(&mut self, op: Op) -> bool {
        match op {
            Op::Grant { task, cap } => {
                if !self.alive[task] || !self.issuable(cap) {
                    return false;
                }
                self.caps.insert(
                    cap,
                    CapState {
                        owner: task,
                        invalid: false,
                        expires_at: None,
                    },
                );
                true
            }
            Op::Lease { task, cap, ms } => {
                if !self.alive[task] || !self.issuable(cap) {
                    return false;
                }
                self.caps.insert(
                    cap,
                    CapState {
                        owner: task,
                        invalid: false,
                        expires_at: Some(self.now + ms * NANOS_PER_MS),
                    },
                );
                true
            }
            Op::Delegate { cap, from, to } => {
                if !self.usable(cap, from) || !self.alive[to] {
                    return false;
                }
                self.caps.get_mut(&cap).unwrap().owner = to;
                true
            }
            Op::Revoke { task, cap } | Op::Drop { task, cap } => {
                if !self.usable(cap, task) {
                    return false;
                }
                self.caps.get_mut(&cap).unwrap().invalid = true;
                true
            }
            Op::Advance { ms } => {
                self.now += ms * NANOS_PER_MS;
                true
            }
            Op::Kill { task } => {
                if !self.alive[task] {
                    return false;
                }
                self.alive[task] = false;
                true
            }
        }
    }
}

struct Harness {
    kernel: SimulatedKernel,
    tasks: Vec<TaskId>,
    model: Model,
    cap_ids: Vec<u64>,
    /// Capabilities that have been valid for nobody at some point.
    dead_caps: BTreeSet<u64>,
    /// Lease incarnations (cap, expiry) the kernel must have logged as expired.
    expired_leases: BTreeSet<(u64, u64)>,
    revokes: usize,
    delegations: usize,
    drops: usize,
    trace: Vec<Op>,
}

impl Harness {
    fn new(task_count: usize, cap_ids: &[u64]) -> Self {
        let mut kernel = SimulatedKernel::with_tick_resolution(Duration::from_millis(1));
        let tasks = (0..task_count)
            .map(|i| {
                kernel
                    .spawn_task(TaskDescriptor::new(format!("t{i}")))
                    .expect("spawn")
                    .task_id
            })
            .collect();
        Self {
            kernel,
            tasks,
            model: Model::new(task_count),
            cap_ids: cap_ids.to_vec(),
            dead_caps: BTreeSet::new(),
            expired_leases: BTreeSet::new(),
            revokes: 0,
            delegations: 0,
            drops: 0,
            trace: Vec::new(),
        }
    }

    fn validity(&self) -> Vec<Vec<bool>> {
        self.cap_ids
            .iter()
            .map(|&cap| {
                self.tasks
                    .iter()
                    .map(|&t| self.kernel.is_capability_valid(cap, t))
                    .collect()
            })
            .collect()
    }

    fn step(&mut self, op: Op) {
        self.trace.push(op);
        let before = self.validity();
        let expected = self.model.apply(op);
        let accepted = match op {
            Op::Grant { task, cap } => self
                .kernel
                .grant_capability(self.tasks[task], Cap::<()>::new(cap))
                .is_ok(),
            Op::Lease { task, cap, ms } => self
                .kernel
                .grant_capability_with_lease(
                    self.tasks[task],
                    Cap::<()>::new(cap),
                    Duration::from_millis(ms),
                )
                .is_ok(),
            Op::Delegate { cap, from, to } => self
                .kernel
                .delegate_capability(cap, self.tasks[from], self.tasks[to])
                .is_ok(),
            Op::Revoke { task, cap } => self
                .kernel
                .revoke_capability(cap, self.tasks[task], "model".to_string())
                .is_ok(),
            Op::Drop { task, cap } => self.kernel.drop_capability(cap, self.tasks[task]).is_ok(),
            Op::Advance { ms } => {
                self.kernel.advance_time(Duration::from_millis(ms));
                true
            }
            Op::Kill { task } => {
                let before = self.kernel.task_count();
                self.kernel.terminate_task(self.tasks[task]);
                self.kernel.task_count() < before
            }
        };
        assert_eq!(
            accepted, expected,
            "I0 acceptance mismatch on {op:?}\ntrace: {:?}",
            self.trace
        );
        if accepted {
            match op {
                Op::Grant { cap, .. } | Op::Lease { cap, .. } => {
                    // A re-issue starts a new life for the id (I3 restarts).
                    self.dead_caps.remove(&cap);
                }
                Op::Revoke { .. } => self.revokes += 1,
                Op::Delegate { .. } => self.delegations += 1,
                Op::Drop { .. } => self.drops += 1,
                _ => {}
            }
        } else {
            assert_eq!(
                self.validity(),
                before,
                "I5 failed {op:?} changed validity\ntrace: {:?}",
                self.trace
            );
        }
        self.check_invariants();
    }

    fn check_invariants(&mut self) {
        let validity = self.validity();
        for (ci, &cap) in self.cap_ids.iter().enumerate() {
            let mut holders = 0;
            for (ti, _) in self.tasks.iter().enumerate() {
                let expected = self.model.valid_for(cap, ti);
                assert_eq!(
                    validity[ci][ti], expected,
                    "I1 cap {cap} task {ti}: kernel={} model={}\ntrace: {:?}",
                    validity[ci][ti], expected, self.trace
                );
                if validity[ci][ti] {
                    holders += 1;
                }
            }
            assert!(
                holders <= 1,
                "I2 cap {cap} has {holders} holders\ntrace: {:?}",
                self.trace
            );
            let exists = self.model.caps.contains_key(&cap);
            if exists && holders == 0 {
                self.dead_caps.insert(cap);
            }
            if self.dead_caps.contains(&cap) {
                assert_eq!(
                    holders, 0,
                    "I3 cap {cap} came back to life\ntrace: {:?}",
                    self.trace
                );
            }
        }
        let audit = self.kernel.audit_log();
        let count = |pred: &dyn Fn(&CapabilityEvent) -> bool| audit.count_events(pred);
        assert_eq!(
            count(&|e| matches!(e, CapabilityEvent::Revoked { .. })),
            self.revokes,
            "I4 revoke audit\ntrace: {:?}",
            self.trace
        );
        assert_eq!(
            count(&|e| matches!(e, CapabilityEvent::Delegated { .. })),
            self.delegations,
            "I4 delegate audit\ntrace: {:?}",
            self.trace
        );
        assert_eq!(
            count(&|e| matches!(e, CapabilityEvent::Dropped { .. })),
            self.drops,
            "I4 drop audit\ntrace: {:?}",
            self.trace
        );
        // A lease that reaches its expiry while still otherwise live is
        // logged as expired exactly once per incarnation; leases already
        // revoked, dropped, or orphaned by then are not.
        for (&cap, state) in &self.model.caps {
            if let Some(expires) = state.expires_at {
                if expires <= self.model.now && !state.invalid && self.model.alive[state.owner] {
                    self.expired_leases.insert((cap, expires));
                }
            }
        }
        let logged = count(&|e| matches!(e, CapabilityEvent::LeaseExpired { .. }));
        assert_eq!(
            logged,
            self.expired_leases.len(),
            "I4 lease expiry audit\ntrace: {:?}",
            self.trace
        );
    }
}

fn all_ops(tasks: usize, caps: &[u64]) -> Vec<Op> {
    let mut ops = Vec::new();
    for task in 0..tasks {
        for &cap in caps {
            ops.push(Op::Grant { task, cap });
            ops.push(Op::Lease { task, cap, ms: 5 });
            ops.push(Op::Revoke { task, cap });
            ops.push(Op::Drop { task, cap });
            for to in 0..tasks {
                ops.push(Op::Delegate {
                    cap,
                    from: task,
                    to,
                });
            }
        }
        ops.push(Op::Kill { task });
    }
    ops.push(Op::Advance { ms: 6 });
    ops
}

#[test]
fn exhaustive_depth_three_two_tasks_two_caps() {
    let caps = [1u64, 2];
    let ops = all_ops(2, &caps);
    let mut sequences = 0usize;
    for &a in &ops {
        for &b in &ops {
            for &c in &ops {
                let mut h = Harness::new(2, &caps);
                h.step(a);
                h.step(b);
                h.step(c);
                sequences += 1;
            }
        }
    }
    assert_eq!(sequences, ops.len().pow(3));
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
fn random_sequences_three_tasks_four_caps() {
    let caps = [10u64, 20, 30, 40];
    for seed in 1..=300u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut h = Harness::new(3, &caps);
        for _ in 0..40 {
            let task = rng.below(3) as usize;
            let cap = caps[rng.below(4) as usize];
            let op = match rng.below(10) {
                0 | 1 => Op::Grant { task, cap },
                2 => Op::Lease {
                    task,
                    cap,
                    ms: [1, 3, 7][rng.below(3) as usize],
                },
                3 | 4 => Op::Delegate {
                    cap,
                    from: task,
                    to: rng.below(3) as usize,
                },
                5 => Op::Revoke { task, cap },
                6 => Op::Drop { task, cap },
                7 | 8 => Op::Advance {
                    ms: [1, 2, 4][rng.below(3) as usize],
                },
                _ => Op::Kill { task },
            };
            h.step(op);
        }
    }
}

//! Model-based checking of per-identity message budgets in `sim_kernel`.
//!
//! - B1 acceptance: a send or receive succeeds exactly when the model says so
//!   (identity not cancelled, budget not yet spent, channel not full/empty);
//! - B2 accounting: the identity's message usage equals the number of
//!   messages it actually sent or received (a rejected send to a full
//!   channel or a receive from an empty one costs nothing);
//! - B3 exhaustion: the first attempt beyond the limit cancels the identity,
//!   which is logged once, cancels the scheduler task, and refuses every
//!   later operation;
//! - B4 audit: `MessageConsumed` events match successful operations and
//!   `BudgetExhausted` events match cancellations;
//! - B5 delivery order stays FIFO per channel.
//!
//! Exhaustive: every sequence of five operations over one channel and two
//! tasks (8^5 = 32,768 sequences). Random: 300 fixed-seed sequences of 60
//! operations over two channels and three tasks.

use core_types::{ServiceId, TaskId};
use identity::ExecutionId;
use ipc::{ChannelId, MessageEnvelope, MessagePayload, SchemaVersion};
use kernel_api::{KernelApi, TaskDescriptor};
use resources::{MessageCount, ResourceBudget};
use sim_kernel::resource_audit::ResourceEvent;
use sim_kernel::scheduler::TaskState;
use sim_kernel::SimulatedKernel;
use std::collections::VecDeque;

const CAPACITY: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    SetBudget(usize, u64),
    Send(usize, usize),
    Recv(usize, usize),
}

#[derive(Debug, Clone)]
struct Model {
    limit: Vec<Option<u64>>,
    usage: Vec<u64>,
    cancelled: Vec<bool>,
    queues: Vec<VecDeque<u64>>,
    next_msg: u64,
}

impl Model {
    fn new(tasks: usize, channels: usize) -> Self {
        Self {
            limit: vec![None; tasks],
            usage: vec![0; tasks],
            cancelled: vec![false; tasks],
            queues: (0..channels).map(|_| VecDeque::new()).collect(),
            next_msg: 0,
        }
    }

    /// Charge one message to `t` if allowed; `false` means refused (and
    /// possibly cancelled).
    fn charge(&mut self, t: usize) -> bool {
        if self.cancelled[t] {
            return false;
        }
        if let Some(limit) = self.limit[t] {
            if self.usage[t] >= limit {
                self.cancelled[t] = true;
                return false;
            }
        }
        self.usage[t] += 1;
        true
    }

    fn apply(&mut self, op: Op) -> (bool, Option<u64>) {
        match op {
            Op::SetBudget(t, limit) => {
                self.limit[t] = Some(limit);
                (true, None)
            }
            Op::Send(ch, t) => {
                if self.cancelled[t] {
                    return (false, None);
                }
                if self.queues[ch].len() >= CAPACITY {
                    // Full: refused before any budget is charged.
                    return (false, None);
                }
                if !self.charge(t) {
                    return (false, None);
                }
                let id = self.next_msg;
                self.next_msg += 1;
                self.queues[ch].push_back(id);
                (true, Some(id))
            }
            Op::Recv(ch, t) => {
                if self.cancelled[t] {
                    return (false, None);
                }
                if self.queues[ch].is_empty() {
                    return (false, None);
                }
                if !self.charge(t) {
                    return (false, None);
                }
                (true, self.queues[ch].pop_front())
            }
        }
    }
}

struct Harness {
    kernel: SimulatedKernel,
    tasks: Vec<TaskId>,
    identities: Vec<ExecutionId>,
    channels: Vec<ChannelId>,
    model: Model,
    consumed: usize,
    trace: Vec<Op>,
}

fn envelope(seq: u64, source: TaskId) -> MessageEnvelope {
    let mut message = MessageEnvelope::new(
        ServiceId::new(),
        format!("m{seq}"),
        SchemaVersion::new(1, 0),
        MessagePayload::new(&seq).unwrap(),
    );
    message.source = Some(source);
    message
}

impl Harness {
    fn new(tasks: usize, channels: usize) -> Self {
        let mut kernel = SimulatedKernel::new().with_channel_capacity(CAPACITY);
        let mut ids = Vec::new();
        let mut identities = Vec::new();
        for i in 0..tasks {
            let (handle, exec) = kernel
                .spawn_task_with_identity(
                    TaskDescriptor::new(format!("t{i}")),
                    identity::IdentityKind::Component,
                    identity::TrustDomain::user(),
                    None,
                    None,
                )
                .expect("spawn");
            ids.push(handle.task_id);
            identities.push(exec);
        }
        let channels: Vec<ChannelId> = (0..channels)
            .map(|_| KernelApi::create_channel(&mut kernel).expect("channel"))
            .collect();
        let model = Model::new(tasks, channels.len());
        Self {
            kernel,
            tasks: ids,
            identities,
            channels,
            model,
            consumed: 0,
            trace: Vec::new(),
        }
    }

    fn step(&mut self, op: Op) {
        self.trace.push(op);
        let (accepted, msg) = self.model.apply(op);
        match op {
            Op::SetBudget(t, limit) => {
                let identity = self
                    .kernel
                    .get_identity_mut(self.identities[t])
                    .expect("identity");
                identity.budget = Some(ResourceBudget {
                    message_count: Some(MessageCount::new(limit)),
                    ..ResourceBudget::unlimited()
                });
            }
            Op::Send(ch, t) => {
                let seq = msg.unwrap_or(u64::MAX);
                let result = self
                    .kernel
                    .send_message(self.channels[ch], envelope(seq, self.tasks[t]));
                assert_eq!(
                    result.is_ok(),
                    accepted,
                    "B1 send {op:?}: {result:?}\ntrace: {:?}",
                    self.trace
                );
                if accepted {
                    self.consumed += 1;
                }
            }
            Op::Recv(ch, t) => {
                self.kernel.set_receive_context(self.tasks[t]);
                let result = self.kernel.receive_message(self.channels[ch], None);
                self.kernel.clear_receive_context();
                assert_eq!(
                    result.is_ok(),
                    accepted,
                    "B1 receive {op:?}: {result:?}\ntrace: {:?}",
                    self.trace
                );
                if let Ok(message) = result {
                    self.consumed += 1;
                    assert_eq!(
                        message.action,
                        format!("m{}", msg.unwrap()),
                        "B5 order\ntrace: {:?}",
                        self.trace
                    );
                }
            }
        }
        self.check();
    }

    fn check(&self) {
        for (t, &exec) in self.identities.iter().enumerate() {
            let identity = self.kernel.get_identity(exec).expect("identity");
            assert_eq!(
                identity.usage.message_count.0, self.model.usage[t],
                "B2 usage of task {t}\ntrace: {:?}",
                self.trace
            );
            let cancelled_events = self.kernel.resource_audit().count_events(|e| {
                matches!(
                    e,
                    ResourceEvent::CancelledDueToExhaustion { execution_id, .. }
                        if *execution_id == exec
                )
            });
            assert_eq!(
                cancelled_events,
                usize::from(self.model.cancelled[t]),
                "B3 cancellation of task {t}\ntrace: {:?}",
                self.trace
            );
            let exhausted_events = self.kernel.resource_audit().count_events(|e| {
                matches!(
                    e,
                    ResourceEvent::BudgetExhausted { execution_id, .. } if *execution_id == exec
                )
            });
            assert_eq!(
                exhausted_events,
                usize::from(self.model.cancelled[t]),
                "B4 exhaustion events of task {t}\ntrace: {:?}",
                self.trace
            );
            if self.model.cancelled[t] {
                assert_eq!(
                    self.kernel.scheduler().task_state(self.tasks[t]),
                    Some(TaskState::Cancelled),
                    "B3 scheduler task {t} cancelled\ntrace: {:?}",
                    self.trace
                );
            }
        }
        let consumed_events = self
            .kernel
            .resource_audit()
            .count_events(|e| matches!(e, ResourceEvent::MessageConsumed { .. }));
        assert_eq!(
            consumed_events, self.consumed,
            "B4 consumed events\ntrace: {:?}",
            self.trace
        );
        assert_eq!(
            self.kernel.pending_message_count(),
            self.model.queues.iter().map(|q| q.len()).sum::<usize>(),
            "pending count\ntrace: {:?}",
            self.trace
        );
    }
}

fn all_ops(tasks: usize) -> Vec<Op> {
    let mut ops = Vec::new();
    for t in 0..tasks {
        ops.push(Op::SetBudget(t, 1));
        ops.push(Op::SetBudget(t, 2));
        ops.push(Op::Send(0, t));
        ops.push(Op::Recv(0, t));
    }
    ops
}

#[test]
fn exhaustive_depth_five_one_channel_two_tasks() {
    let ops = all_ops(2);
    assert_eq!(ops.len(), 8);
    let mut count = 0usize;
    let mut stack = vec![0usize; 5];
    loop {
        let mut h = Harness::new(2, 1);
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
fn random_sequences_two_channels_three_tasks() {
    for seed in 1..=300u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut h = Harness::new(3, 2);
        for _ in 0..60 {
            let ch = rng.below(2) as usize;
            let t = rng.below(3) as usize;
            let op = match rng.below(10) {
                0 => Op::SetBudget(t, 1 + rng.below(6)),
                1..=5 => Op::Send(ch, t),
                _ => Op::Recv(ch, t),
            };
            h.step(op);
        }
    }
}

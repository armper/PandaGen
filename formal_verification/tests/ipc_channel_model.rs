//! Model-based checking of IPC channel access control and delivery order in
//! `sim_kernel`.
//!
//! - C1 acceptance: send and receive succeed exactly when the model says so
//!   (unrestricted channels admit everyone; a channel with an access list
//!   admits only listed tasks, anonymous senders and context-less receives
//!   included; a full channel rejects sends);
//! - C2 order: every received message is the one at the head of the model's
//!   FIFO for that channel;
//! - C3 closure: revoking the last holder leaves the channel closed, never
//!   open to everyone;
//! - C4 accounting: the kernel's pending message count matches the model.
//!
//! Exhaustive: every sequence of four operations over one channel, two tasks
//! and the anonymous sender (14^4 = 38,416 sequences). Random: 400 fixed-seed
//! sequences of 60 operations over two channels and three tasks.

use core_types::{ServiceId, TaskId};
use ipc::{ChannelId, MessageEnvelope, MessagePayload, SchemaVersion};
use kernel_api::{KernelApi, TaskDescriptor};
use sim_kernel::{ChannelAccessMode, SimulatedKernel};
use std::collections::{BTreeSet, VecDeque};

const CAPACITY: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Grant(usize, usize, ChannelAccessMode),
    Revoke(usize, usize),
    /// Send from task `Some(t)` or anonymously (`None`).
    Send(usize, Option<usize>),
    /// Receive as task `Some(t)` or without a receive context.
    Recv(usize, Option<usize>),
}

#[derive(Debug, Clone, Default)]
struct Access {
    senders: BTreeSet<usize>,
    receivers: BTreeSet<usize>,
}

#[derive(Debug, Clone)]
struct ChannelModel {
    access: Option<Access>,
    queue: VecDeque<u64>,
}

#[derive(Debug, Clone)]
struct Model {
    channels: Vec<ChannelModel>,
    next_msg: u64,
}

impl Model {
    fn new(channels: usize) -> Self {
        Self {
            channels: (0..channels)
                .map(|_| ChannelModel {
                    access: None,
                    queue: VecDeque::new(),
                })
                .collect(),
            next_msg: 0,
        }
    }

    fn may_send(&self, ch: usize, who: Option<usize>) -> bool {
        match (&self.channels[ch].access, who) {
            (None, _) => true,
            (Some(a), Some(t)) => a.senders.contains(&t),
            (Some(_), None) => false,
        }
    }

    fn may_receive(&self, ch: usize, who: Option<usize>) -> bool {
        match (&self.channels[ch].access, who) {
            (None, _) => true,
            (Some(a), Some(t)) => a.receivers.contains(&t),
            (Some(_), None) => false,
        }
    }

    /// Apply; returns (accepted, message involved).
    fn apply(&mut self, op: Op) -> (bool, Option<u64>) {
        match op {
            Op::Grant(ch, t, mode) => {
                let access = self.channels[ch].access.get_or_insert_with(Access::default);
                if matches!(mode, ChannelAccessMode::Send | ChannelAccessMode::Both) {
                    access.senders.insert(t);
                }
                if matches!(mode, ChannelAccessMode::Receive | ChannelAccessMode::Both) {
                    access.receivers.insert(t);
                }
                (true, None)
            }
            Op::Revoke(ch, t) => {
                if let Some(access) = self.channels[ch].access.as_mut() {
                    access.senders.remove(&t);
                    access.receivers.remove(&t);
                }
                (true, None)
            }
            Op::Send(ch, who) => {
                if !self.may_send(ch, who) || self.channels[ch].queue.len() >= CAPACITY {
                    return (false, None);
                }
                let id = self.next_msg;
                self.next_msg += 1;
                self.channels[ch].queue.push_back(id);
                (true, Some(id))
            }
            Op::Recv(ch, who) => {
                if !self.may_receive(ch, who) {
                    return (false, None);
                }
                match self.channels[ch].queue.pop_front() {
                    Some(id) => (true, Some(id)),
                    None => (false, None),
                }
            }
        }
    }

    fn pending(&self) -> usize {
        self.channels.iter().map(|c| c.queue.len()).sum()
    }
}

struct Harness {
    kernel: SimulatedKernel,
    tasks: Vec<TaskId>,
    channels: Vec<ChannelId>,
    model: Model,
    trace: Vec<Op>,
}

fn envelope(seq: u64, source: Option<TaskId>) -> MessageEnvelope {
    let mut message = MessageEnvelope::new(
        ServiceId::new(),
        format!("m{seq}"),
        SchemaVersion::new(1, 0),
        MessagePayload::new(&seq).unwrap(),
    );
    message.source = source;
    message
}

impl Harness {
    fn new(tasks: usize, channels: usize) -> Self {
        let mut kernel = SimulatedKernel::new().with_channel_capacity(CAPACITY);
        let tasks = (0..tasks)
            .map(|i| {
                kernel
                    .spawn_task(TaskDescriptor::new(format!("t{i}")))
                    .expect("spawn")
                    .task_id
            })
            .collect();
        let channels: Vec<ChannelId> = (0..channels)
            .map(|_| KernelApi::create_channel(&mut kernel).expect("channel"))
            .collect();
        let model = Model::new(channels.len());
        Self {
            kernel,
            tasks,
            channels,
            model,
            trace: Vec::new(),
        }
    }

    fn step(&mut self, op: Op) {
        self.trace.push(op);
        let (expected_ok, expected_msg) = self.model.apply(op);
        match op {
            Op::Grant(ch, t, mode) => {
                self.kernel
                    .grant_channel_access(self.channels[ch], self.tasks[t], mode)
                    .expect("grant");
            }
            Op::Revoke(ch, t) => {
                self.kernel
                    .revoke_channel_access(self.channels[ch], self.tasks[t])
                    .expect("revoke");
            }
            Op::Send(ch, who) => {
                // The kernel never sees the model's id for a rejected send, so
                // number outgoing messages by the model's counter regardless.
                let seq = expected_msg.unwrap_or(u64::MAX);
                let source = who.map(|t| self.tasks[t]);
                let result = self
                    .kernel
                    .send_message(self.channels[ch], envelope(seq, source));
                assert_eq!(
                    result.is_ok(),
                    expected_ok,
                    "C1 send {op:?}: {result:?}\ntrace: {:?}",
                    self.trace
                );
            }
            Op::Recv(ch, who) => {
                match who {
                    Some(t) => self.kernel.set_receive_context(self.tasks[t]),
                    None => self.kernel.clear_receive_context(),
                }
                let result = self.kernel.receive_message(self.channels[ch], None);
                self.kernel.clear_receive_context();
                assert_eq!(
                    result.is_ok(),
                    expected_ok,
                    "C1 receive {op:?}: {result:?}\ntrace: {:?}",
                    self.trace
                );
                if let Ok(message) = result {
                    let expected = format!("m{}", expected_msg.unwrap());
                    assert_eq!(
                        message.action, expected,
                        "C2 order on {op:?}\ntrace: {:?}",
                        self.trace
                    );
                }
            }
        }
        assert_eq!(
            self.kernel.pending_message_count(),
            self.model.pending(),
            "C4 pending count\ntrace: {:?}",
            self.trace
        );
    }
}

fn all_ops(tasks: usize) -> Vec<Op> {
    let mut ops = vec![Op::Send(0, None), Op::Recv(0, None)];
    for t in 0..tasks {
        for mode in [
            ChannelAccessMode::Send,
            ChannelAccessMode::Receive,
            ChannelAccessMode::Both,
        ] {
            ops.push(Op::Grant(0, t, mode));
        }
        ops.push(Op::Revoke(0, t));
        ops.push(Op::Send(0, Some(t)));
        ops.push(Op::Recv(0, Some(t)));
    }
    ops
}

#[test]
fn exhaustive_depth_four_one_channel_two_tasks() {
    let ops = all_ops(2);
    assert_eq!(ops.len(), 14);
    let mut count = 0usize;
    let mut stack = vec![0usize; 4];
    loop {
        let mut h = Harness::new(2, 1);
        for &i in &stack {
            h.step(ops[i]);
        }
        count += 1;
        let mut pos = stack.len();
        loop {
            if pos == 0 {
                assert_eq!(count, ops.len().pow(4));
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

#[test]
fn revoking_last_holder_closes_the_channel() {
    // C3 spelled out: after the only grant is revoked, nobody may use the
    // channel, unlike a channel that never had an access list.
    let mut h = Harness::new(2, 2);
    h.step(Op::Grant(0, 0, ChannelAccessMode::Both));
    h.step(Op::Revoke(0, 0));
    h.step(Op::Send(0, Some(0)));
    h.step(Op::Send(0, Some(1)));
    h.step(Op::Send(0, None));
    h.step(Op::Recv(0, Some(1)));
    assert!(!h.model.may_send(0, Some(1)));
    // Channel 1 never had an access list and stays open.
    h.step(Op::Send(1, Some(1)));
    h.step(Op::Recv(1, None));
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
    for seed in 1..=400u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut h = Harness::new(3, 2);
        for _ in 0..60 {
            let ch = rng.below(2) as usize;
            let t = rng.below(3) as usize;
            let who = if rng.below(5) == 0 { None } else { Some(t) };
            let op = match rng.below(10) {
                0 | 1 => Op::Grant(
                    ch,
                    t,
                    [
                        ChannelAccessMode::Send,
                        ChannelAccessMode::Receive,
                        ChannelAccessMode::Both,
                    ][rng.below(3) as usize],
                ),
                2 => Op::Revoke(ch, t),
                3..=6 => Op::Send(ch, who),
                _ => Op::Recv(ch, who),
            };
            h.step(op);
        }
    }
}

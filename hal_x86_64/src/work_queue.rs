//! Fixed-capacity job queue for application processors.
//!
//! The boot CPU submits jobs, wakes the other CPUs, and polls results;
//! any CPU may take and complete jobs. Results are published through
//! atomics so a waiter needs no lock to read them.

use crate::spin::SpinLock;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

/// A unit of work: an opaque kind and argument, interpreted by the runner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Job {
    pub id: u64,
    pub kind: u32,
    pub arg: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobResult {
    pub cpu: u32,
    pub value: u64,
}

struct Slot {
    /// Which job currently owns this slot. Slots are reused round-robin, so
    /// every read checks the id before trusting `done`.
    id: AtomicU64,
    done: AtomicBool,
    cpu: AtomicU32,
    value: AtomicU64,
}

/// Slot id meaning "never used"; real ids start at zero and only increase.
const NO_JOB: u64 = u64::MAX;

struct Pending<const N: usize> {
    jobs: [Job; N],
    head: usize,
    len: usize,
    next_id: u64,
}

pub struct WorkQueue<const N: usize> {
    pending: SpinLock<Pending<N>>,
    results: [Slot; N],
}

impl<const N: usize> WorkQueue<N> {
    #[allow(clippy::declare_interior_mutable_const)]
    const SLOT: Slot = Slot {
        id: AtomicU64::new(NO_JOB),
        done: AtomicBool::new(false),
        cpu: AtomicU32::new(0),
        value: AtomicU64::new(0),
    };

    pub const fn new() -> Self {
        Self {
            pending: SpinLock::new(Pending {
                jobs: [Job {
                    id: 0,
                    kind: 0,
                    arg: 0,
                }; N],
                head: 0,
                len: 0,
                next_id: 0,
            }),
            results: [Self::SLOT; N],
        }
    }

    /// Queue a job; `None` when the ring is full.
    ///
    /// Ids increase forever and a slot is claimed by id, so the queue serves
    /// any number of jobs and independent submitters never disturb each
    /// other's results. There is deliberately no global reset: one existed,
    /// and with two owners (frame bands on the boot CPU, `smp run` jobs on an
    /// application processor) each reset silently invalidated the other's
    /// in-flight ids.
    pub fn submit(&self, kind: u32, arg: u64) -> Option<u64> {
        let mut p = self.pending.lock();
        if p.len == N {
            return None;
        }
        let id = p.next_id;
        p.next_id = p.next_id.wrapping_add(1);
        let tail = (p.head + p.len) % N;
        p.jobs[tail] = Job { id, kind, arg };
        p.len += 1;
        let slot = &self.results[Self::slot_of(id)];
        // Clear the previous occupant's result before publishing the new id,
        // so a reader that sees this id can never observe a stale `done`.
        slot.done.store(false, Ordering::Relaxed);
        slot.id.store(id, Ordering::Release);
        Some(id)
    }

    const fn slot_of(id: u64) -> usize {
        (id % N as u64) as usize
    }

    /// Remove a queued job that no worker has taken yet.
    ///
    /// The submitter calls this when it gives up waiting, so an abandoned
    /// job does not occupy a ring slot for the rest of the uptime.
    pub fn cancel(&self, id: u64) -> bool {
        let mut p = self.pending.lock();
        let len = p.len;
        for offset in 0..len {
            let index = (p.head + offset) % N;
            if p.jobs[index].id != id {
                continue;
            }
            for shift in offset..len - 1 {
                let from = (p.head + shift + 1) % N;
                let to = (p.head + shift) % N;
                p.jobs[to] = p.jobs[from];
            }
            p.len -= 1;
            return true;
        }
        false
    }

    /// Take the oldest queued job, if any.
    pub fn take(&self) -> Option<Job> {
        let mut p = self.pending.lock();
        if p.len == 0 {
            return None;
        }
        let job = p.jobs[p.head];
        p.head = (p.head + 1) % N;
        p.len -= 1;
        Some(job)
    }

    pub fn pending(&self) -> usize {
        self.pending.lock().len
    }

    /// Publish a job's result. A result for a job whose slot has already
    /// been recycled is dropped rather than attributed to the new owner.
    pub fn complete(&self, id: u64, cpu: u32, value: u64) {
        let slot = &self.results[Self::slot_of(id)];
        if slot.id.load(Ordering::Acquire) != id {
            return;
        }
        slot.cpu.store(cpu, Ordering::Relaxed);
        slot.value.store(value, Ordering::Relaxed);
        slot.done.store(true, Ordering::Release);
    }

    pub fn result(&self, id: u64) -> Option<JobResult> {
        let slot = &self.results[Self::slot_of(id)];
        if slot.id.load(Ordering::Acquire) != id {
            return None;
        }
        if !slot.done.load(Ordering::Acquire) {
            return None;
        }
        Some(JobResult {
            cpu: slot.cpu.load(Ordering::Relaxed),
            value: slot.value.load(Ordering::Relaxed),
        })
    }

    /// Wait until job `id` completes or `max_spins` polls elapse.
    pub fn wait(&self, id: u64, max_spins: u64) -> Option<JobResult> {
        let mut spins = 0;
        loop {
            if let Some(r) = self.result(id) {
                return Some(r);
            }
            spins += 1;
            if spins > max_spins {
                return None;
            }
            core::hint::spin_loop();
        }
    }
}

impl<const N: usize> Default for WorkQueue<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn fifo_submit_take_complete() {
        let q = WorkQueue::<4>::new();
        assert_eq!(q.submit(1, 10), Some(0));
        assert_eq!(q.submit(2, 20), Some(1));
        assert_eq!(q.pending(), 2);
        assert_eq!(
            q.take(),
            Some(Job {
                id: 0,
                kind: 1,
                arg: 10
            })
        );
        assert_eq!(q.result(0), None);
        q.complete(0, 7, 99);
        assert_eq!(q.result(0), Some(JobResult { cpu: 7, value: 99 }));
        assert_eq!(q.take().map(|j| j.id), Some(1));
        assert_eq!(q.take(), None);
    }

    #[test]
    fn capacity_is_the_ring_not_the_lifetime() {
        let q = WorkQueue::<2>::new();
        assert!(q.submit(0, 0).is_some());
        assert!(q.submit(0, 0).is_some());
        assert_eq!(q.submit(0, 0), None, "the ring is full");
        // Draining one frees exactly one place; the id space never runs out.
        q.take();
        assert!(
            q.submit(0, 0).is_some(),
            "a freed ring slot must be usable again"
        );
    }

    #[test]
    fn the_queue_outlives_its_slot_count_without_a_global_reset() {
        // The defect this guards: `submit` used to refuse once the id
        // counter reached N, so every batch had to call a global `reset`.
        // With two owners (frame bands on the boot CPU, `smp run` jobs on an
        // application processor) each reset cleared the other's in-flight
        // ids and results.
        let q = WorkQueue::<4>::new();
        for round in 0..200u64 {
            let id = q.submit(1, round).expect("submit must keep working");
            let job = q.take().expect("the job we just queued");
            assert_eq!(job.arg, round);
            q.complete(job.id, 0, job.arg);
            assert_eq!(q.result(id).unwrap().value, round);
        }
    }

    #[test]
    fn two_owners_interleave_without_disturbing_each_other() {
        let q = WorkQueue::<8>::new();
        // Owner A queues a batch and a worker finishes it.
        let a: std::vec::Vec<u64> = (0..3).map(|i| q.submit(1, 100 + i).unwrap()).collect();
        for _ in 0..3 {
            let job = q.take().unwrap();
            q.complete(job.id, 7, job.arg);
        }
        // Owner B now starts its own batch, unaware of A.
        let b: std::vec::Vec<u64> = (0..3).map(|i| q.submit(2, 200 + i).unwrap()).collect();
        for (index, id) in a.iter().enumerate() {
            assert_eq!(
                q.result(*id).map(|r| r.value),
                Some(100 + index as u64),
                "owner A's result must survive owner B starting work"
            );
        }
        for id in &b {
            assert!(q.result(*id).is_none(), "B's jobs have not run yet");
        }
        assert!(a.iter().all(|x| !b.contains(x)), "ids never collide");
    }

    #[test]
    fn a_recycled_slot_ignores_the_previous_owners_completion() {
        let q = WorkQueue::<2>::new();
        let stale = q.submit(1, 1).unwrap();
        q.take();
        // Cycle the slot around to the same index with newer ids.
        for value in 0..2u64 {
            let id = q.submit(1, value).unwrap();
            let job = q.take().unwrap();
            q.complete(job.id, 0, job.arg);
            assert!(q.result(id).is_some());
        }
        // A very late worker reports the abandoned job. It must not be
        // attributed to whoever owns that slot now.
        let current = q.submit(1, 999).unwrap();
        q.take();
        q.complete(stale, 9, 12345);
        assert!(
            q.result(current).is_none(),
            "a late completion for a recycled id must not satisfy the new owner"
        );
        assert!(
            q.result(stale).is_none(),
            "and must not resurrect the old id"
        );
    }

    #[test]
    fn cancelling_returns_the_ring_slot() {
        let q = WorkQueue::<2>::new();
        let first = q.submit(1, 10).unwrap();
        let second = q.submit(1, 20).unwrap();
        assert_eq!(q.submit(1, 30), None, "full");
        assert!(q.cancel(first), "an untaken job can be cancelled");
        assert_eq!(q.pending(), 1);
        // The survivor is still intact and still first out.
        let job = q.take().unwrap();
        assert_eq!(job.id, second);
        assert_eq!(job.arg, 20);
        assert!(!q.cancel(second), "a taken job is no longer cancellable");
    }

    #[test]
    fn wait_times_out_without_workers() {
        let q = WorkQueue::<2>::new();
        let id = q.submit(0, 0).unwrap();
        assert_eq!(q.wait(id, 100), None);
    }

    #[test]
    fn workers_drain_queue_concurrently() {
        let q = Arc::new(WorkQueue::<32>::new());
        let ids: std::vec::Vec<u64> = (0..32).map(|i| q.submit(1, i as u64).unwrap()).collect();
        let workers: std::vec::Vec<_> = (0..4u32)
            .map(|cpu| {
                let q = Arc::clone(&q);
                thread::spawn(move || {
                    while let Some(job) = q.take() {
                        q.complete(job.id, cpu, job.arg * 2);
                    }
                })
            })
            .collect();
        for w in workers {
            w.join().unwrap();
        }
        for id in ids {
            let r = q.wait(id, 1).unwrap();
            assert_eq!(r.value, id as u64 * 2);
            assert!(r.cpu < 4);
        }
        assert_eq!(q.pending(), 0);
    }
}

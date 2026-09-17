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
    pub id: u32,
    pub kind: u32,
    pub arg: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobResult {
    pub cpu: u32,
    pub value: u64,
}

struct Slot {
    done: AtomicBool,
    cpu: AtomicU32,
    value: AtomicU64,
}

struct Pending<const N: usize> {
    jobs: [Job; N],
    head: usize,
    len: usize,
    next_id: u32,
}

pub struct WorkQueue<const N: usize> {
    pending: SpinLock<Pending<N>>,
    results: [Slot; N],
}

impl<const N: usize> WorkQueue<N> {
    #[allow(clippy::declare_interior_mutable_const)]
    const SLOT: Slot = Slot {
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

    /// Queue a job; `None` when all `N` slots are in use. Results of
    /// earlier jobs stay readable until `reset`.
    pub fn submit(&self, kind: u32, arg: u64) -> Option<u32> {
        let mut p = self.pending.lock();
        if p.len == N || p.next_id as usize == N {
            return None;
        }
        let id = p.next_id;
        p.next_id += 1;
        let tail = (p.head + p.len) % N;
        p.jobs[tail] = Job { id, kind, arg };
        p.len += 1;
        self.results[id as usize]
            .done
            .store(false, Ordering::Release);
        Some(id)
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

    /// Publish a job's result.
    pub fn complete(&self, id: u32, cpu: u32, value: u64) {
        let slot = &self.results[id as usize];
        slot.cpu.store(cpu, Ordering::Relaxed);
        slot.value.store(value, Ordering::Relaxed);
        slot.done.store(true, Ordering::Release);
    }

    pub fn result(&self, id: u32) -> Option<JobResult> {
        let slot = self.results.get(id as usize)?;
        if !slot.done.load(Ordering::Acquire) {
            return None;
        }
        Some(JobResult {
            cpu: slot.cpu.load(Ordering::Relaxed),
            value: slot.value.load(Ordering::Relaxed),
        })
    }

    /// Wait until job `id` completes or `max_spins` polls elapse.
    pub fn wait(&self, id: u32, max_spins: u64) -> Option<JobResult> {
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

    /// Drop queued jobs and forget results; ids start from 0 again.
    pub fn reset(&self) {
        let mut p = self.pending.lock();
        p.head = 0;
        p.len = 0;
        p.next_id = 0;
        for slot in &self.results {
            slot.done.store(false, Ordering::Release);
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
    fn capacity_and_reset() {
        let q = WorkQueue::<2>::new();
        assert!(q.submit(0, 0).is_some());
        assert!(q.submit(0, 0).is_some());
        assert_eq!(q.submit(0, 0), None);
        q.take();
        // ids are exhausted even though a slot freed up
        assert_eq!(q.submit(0, 0), None);
        q.reset();
        assert_eq!(q.submit(0, 0), Some(0));
        assert_eq!(q.result(1), None);
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
        let ids: std::vec::Vec<u32> = (0..32).map(|i| q.submit(1, i as u64).unwrap()).collect();
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

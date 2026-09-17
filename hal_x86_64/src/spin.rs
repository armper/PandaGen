//! Minimal test-and-set spinlock for kernel-wide shared state.
//!
//! `SpinLock<T>` is the first synchronization primitive that is actually
//! honoured across CPUs; before SMP bring-up the kernel relied on "single
//! CPU, no preemption" comments instead. It does not disable interrupts:
//! callers that share state with an interrupt handler must keep handlers
//! lock-free (as the input queues are) or mask interrupts themselves.

use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};

pub struct SpinLock<T> {
    locked: AtomicBool,
    value: UnsafeCell<T>,
}

// SAFETY: access to `value` is serialised by `locked`.
unsafe impl<T: Send> Sync for SpinLock<T> {}
unsafe impl<T: Send> Send for SpinLock<T> {}

pub struct SpinGuard<'a, T> {
    lock: &'a SpinLock<T>,
}

impl<T> SpinLock<T> {
    pub const fn new(value: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            value: UnsafeCell::new(value),
        }
    }

    /// Spin until the lock is acquired.
    pub fn lock(&self) -> SpinGuard<'_, T> {
        loop {
            if let Some(guard) = self.try_lock() {
                return guard;
            }
            while self.locked.load(Ordering::Relaxed) {
                core::hint::spin_loop();
            }
        }
    }

    /// Acquire the lock if it is free.
    pub fn try_lock(&self) -> Option<SpinGuard<'_, T>> {
        self.locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()
            .map(|_| SpinGuard { lock: self })
    }

    pub fn is_locked(&self) -> bool {
        self.locked.load(Ordering::Relaxed)
    }

    /// Access without locking.
    ///
    /// # Safety
    /// The caller must guarantee no other CPU or context touches the value
    /// concurrently (for example during single-CPU early boot).
    pub unsafe fn get_unchecked(&self) -> *mut T {
        self.value.get()
    }
}

impl<T> Deref for SpinGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        // SAFETY: the guard holds the lock.
        unsafe { &*self.lock.value.get() }
    }
}

impl<T> DerefMut for SpinGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: the guard holds the lock exclusively.
        unsafe { &mut *self.lock.value.get() }
    }
}

impl<T> Drop for SpinGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.locked.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn lock_and_release() {
        let lock = SpinLock::new(1u32);
        assert!(!lock.is_locked());
        {
            let mut g = lock.lock();
            *g += 1;
            assert!(lock.is_locked());
            assert!(lock.try_lock().is_none());
        }
        assert!(!lock.is_locked());
        assert_eq!(*lock.lock(), 2);
    }

    #[test]
    fn counter_is_consistent_across_threads() {
        let lock = Arc::new(SpinLock::new(0u64));
        let handles: std::vec::Vec<_> = (0..8)
            .map(|_| {
                let lock = Arc::clone(&lock);
                thread::spawn(move || {
                    for _ in 0..10_000 {
                        *lock.lock() += 1;
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(*lock.lock(), 80_000);
    }
}

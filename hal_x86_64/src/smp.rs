//! CPU registry for SMP bring-up.
//!
//! Application processors (APs) are parked by the bootloader and released by
//! the kernel one at a time; each one registers itself here as it comes
//! online. The registry is lock-free so it can be used before any allocator
//! or lock is trusted on the new CPU.

use core::sync::atomic::{AtomicU32, Ordering};

/// Upper bound on CPUs the kernel tracks.
pub const MAX_CPUS: usize = 64;

const EMPTY: u32 = u32::MAX;

pub struct CpuRegistry {
    online: AtomicU32,
    lapic_ids: [AtomicU32; MAX_CPUS],
}

impl CpuRegistry {
    #[allow(clippy::declare_interior_mutable_const)]
    const SLOT: AtomicU32 = AtomicU32::new(EMPTY);

    pub const fn new() -> Self {
        Self {
            online: AtomicU32::new(0),
            lapic_ids: [Self::SLOT; MAX_CPUS],
        }
    }

    /// Record a CPU as online; returns its logical index, or `None` when the
    /// registry is full.
    pub fn register(&self, lapic_id: u32) -> Option<usize> {
        let index = self.online.fetch_add(1, Ordering::AcqRel) as usize;
        if index >= MAX_CPUS {
            self.online.fetch_sub(1, Ordering::AcqRel);
            return None;
        }
        self.lapic_ids[index].store(lapic_id, Ordering::Release);
        Some(index)
    }

    pub fn online(&self) -> usize {
        (self.online.load(Ordering::Acquire) as usize).min(MAX_CPUS)
    }

    /// LAPIC id of logical CPU `index`, once it has finished registering.
    pub fn lapic_id(&self, index: usize) -> Option<u32> {
        if index >= self.online() {
            return None;
        }
        match self.lapic_ids[index].load(Ordering::Acquire) {
            EMPTY => None,
            id => Some(id),
        }
    }

    /// Spin until `expected` CPUs are online or `max_spins` polls elapse.
    pub fn wait_for(&self, expected: usize, max_spins: u64) -> bool {
        let mut spins = 0;
        while self.online() < expected {
            spins += 1;
            if spins > max_spins {
                return false;
            }
            core::hint::spin_loop();
        }
        true
    }
}

impl Default for CpuRegistry {
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
    fn registers_in_order() {
        let reg = CpuRegistry::new();
        assert_eq!(reg.online(), 0);
        assert_eq!(reg.register(0), Some(0));
        assert_eq!(reg.register(5), Some(1));
        assert_eq!(reg.online(), 2);
        assert_eq!(reg.lapic_id(0), Some(0));
        assert_eq!(reg.lapic_id(1), Some(5));
        assert_eq!(reg.lapic_id(2), None);
    }

    #[test]
    fn registry_full_is_reported() {
        let reg = CpuRegistry::new();
        for i in 0..MAX_CPUS {
            assert!(reg.register(i as u32).is_some());
        }
        assert_eq!(reg.register(99), None);
        assert_eq!(reg.online(), MAX_CPUS);
    }

    #[test]
    fn wait_for_times_out_and_succeeds() {
        let reg = CpuRegistry::new();
        assert!(!reg.wait_for(1, 100));
        reg.register(3);
        assert!(reg.wait_for(1, 0));
    }

    #[test]
    fn concurrent_registration_assigns_unique_slots() {
        let reg = Arc::new(CpuRegistry::new());
        let handles: std::vec::Vec<_> = (0..16u32)
            .map(|id| {
                let reg = Arc::clone(&reg);
                thread::spawn(move || reg.register(id).unwrap())
            })
            .collect();
        let mut slots: std::vec::Vec<usize> =
            handles.into_iter().map(|h| h.join().unwrap()).collect();
        slots.sort_unstable();
        assert_eq!(slots, (0..16).collect::<std::vec::Vec<_>>());
        assert!(reg.wait_for(16, 1_000_000));
        let mut ids: std::vec::Vec<u32> = (0..16).map(|i| reg.lapic_id(i).unwrap()).collect();
        ids.sort_unstable();
        assert_eq!(ids, (0..16).collect::<std::vec::Vec<_>>());
    }
}

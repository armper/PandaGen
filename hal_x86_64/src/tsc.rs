//! Time-stamp counter, for cycle-level timing of hot paths.

/// Current TSC value (0 on non-x86 hosts).
#[inline]
pub fn rdtsc() -> u64 {
    #[cfg(target_arch = "x86_64")]
    {
        let lo: u32;
        let hi: u32;
        // SAFETY: rdtsc has no side effects.
        unsafe {
            core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi, options(nomem, nostack, preserves_flags));
        }
        ((hi as u64) << 32) | lo as u64
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        0
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(target_arch = "x86_64")]
    fn tsc_is_monotonic_enough() {
        let a = super::rdtsc();
        let b = super::rdtsc();
        assert!(b >= a);
    }
}

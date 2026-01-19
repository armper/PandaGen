//! # Timer Device
//!
//! Hardware abstraction for monotonic time measurement.
//!
//! ## Philosophy
//!
//! **Time is a service, not a global variable.**
//!
//! This trait provides access to a monotonic tick counter. It does NOT:
//! - Provide wall-clock time (no UTC, no timezones)
//! - Block or sleep (polling only)
//! - Implement scheduling (that's for the kernel)
//! - Have implicit side effects
//!
//! ## Design Principles
//!
//! 1. **Monotonic**: Ticks never go backwards
//! 2. **Non-blocking**: Always returns immediately
//! 3. **Cumulative**: Returns total ticks since boot
//! 4. **Frequency-agnostic**: No assumptions about tick rate at this layer
//!
//! ## Use Cases
//!
//! - Delays and retries
//! - CPU budget accounting
//! - Performance measurement
//! - Timeout implementation
//!
//! ## Not For
//!
//! - Preemptive scheduling (not implemented yet)
//! - Wall-clock time (dates, times, timezones)
//! - Sleeping or blocking operations

/// Hardware timer device trait
///
/// Provides access to a monotonic tick counter. Ticks are cumulative
/// and never decrease.
///
/// # Implementation Notes
///
/// - Must be monotonic (never return a smaller value)
/// - Must not block
/// - Tick frequency is implementation-defined
/// - Overflow behavior is implementation-defined (but must remain monotonic)
///
/// # Examples
///
/// ```
/// use hal::TimerDevice;
///
/// fn measure_operation<T: TimerDevice>(timer: &mut T) {
///     let start = timer.poll_ticks();
///     // ... do work ...
///     let end = timer.poll_ticks();
///     let elapsed = end - start;
///     println!("Operation took {} ticks", elapsed);
/// }
/// ```
pub trait TimerDevice {
    /// Returns the current tick count
    ///
    /// This value is:
    /// - Monotonic (never decreases)
    /// - Cumulative (total ticks since boot or device initialization)
    /// - Non-blocking (returns immediately)
    ///
    /// # Returns
    ///
    /// The current tick count as a u64. The tick frequency is
    /// implementation-defined.
    fn poll_ticks(&mut self) -> u64;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Simple test implementation for demonstration
    struct TestTimer {
        ticks: u64,
    }

    impl TestTimer {
        fn new() -> Self {
            Self { ticks: 0 }
        }

        fn advance(&mut self, delta: u64) {
            self.ticks += delta;
        }
    }

    impl TimerDevice for TestTimer {
        fn poll_ticks(&mut self) -> u64 {
            self.ticks
        }
    }

    #[test]
    fn test_timer_monotonic() {
        let mut timer = TestTimer::new();
        let t1 = timer.poll_ticks();
        timer.advance(100);
        let t2 = timer.poll_ticks();
        timer.advance(50);
        let t3 = timer.poll_ticks();

        assert!(t2 >= t1);
        assert!(t3 >= t2);
        assert_eq!(t2 - t1, 100);
        assert_eq!(t3 - t2, 50);
    }

    #[test]
    fn test_timer_cumulative() {
        let mut timer = TestTimer::new();
        assert_eq!(timer.poll_ticks(), 0);

        timer.advance(100);
        assert_eq!(timer.poll_ticks(), 100);

        timer.advance(200);
        assert_eq!(timer.poll_ticks(), 300);
    }
}

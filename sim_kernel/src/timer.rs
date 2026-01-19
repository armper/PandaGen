//! # Simulated Timer Device
//!
//! Deterministic timer implementation for testing.
//!
//! ## Philosophy
//!
//! **Determinism enables thorough testing.**
//!
//! This timer provides controllable, deterministic time progression.
//! Unlike real hardware timers, this timer only advances when explicitly
//! told to do so.
//!
//! ## Use Cases
//!
//! - Unit tests that need predictable timing
//! - Integration tests with controlled time flow
//! - Fault injection scenarios with time-based triggers
//! - Budget enforcement testing

use hal::TimerDevice;

/// Simulated timer device with controllable time progression
///
/// This timer is deterministic and only advances when explicitly
/// instructed via `advance_ticks()`. This makes tests predictable
/// and reproducible.
///
/// # Examples
///
/// ```
/// use sim_kernel::timer::SimTimerDevice;
/// use hal::TimerDevice;
///
/// let mut timer = SimTimerDevice::new();
/// assert_eq!(timer.poll_ticks(), 0);
///
/// timer.advance_ticks(100);
/// assert_eq!(timer.poll_ticks(), 100);
///
/// timer.advance_ticks(50);
/// assert_eq!(timer.poll_ticks(), 150);
/// ```
#[derive(Debug, Clone)]
pub struct SimTimerDevice {
    /// Current tick count
    ticks: u64,
}

impl SimTimerDevice {
    /// Creates a new simulated timer starting at tick 0
    pub fn new() -> Self {
        Self { ticks: 0 }
    }

    /// Creates a new simulated timer starting at a specific tick count
    ///
    /// Useful for tests that need to start with a non-zero time.
    pub fn with_initial_ticks(ticks: u64) -> Self {
        Self { ticks }
    }

    /// Advances the timer by the specified number of ticks
    ///
    /// This is the primary way to control time in tests. The timer
    /// only advances when this method is called.
    ///
    /// # Arguments
    ///
    /// * `delta` - Number of ticks to advance
    ///
    /// # Panics
    ///
    /// Panics if advancing would overflow u64 (extremely unlikely).
    pub fn advance_ticks(&mut self, delta: u64) {
        self.ticks = self.ticks.checked_add(delta).expect("Timer tick overflow");
    }

    /// Sets the timer to a specific tick count
    ///
    /// Note: This should only be used when `new_ticks >= self.ticks`
    /// to maintain monotonicity. For normal operation, use `advance_ticks`.
    ///
    /// # Arguments
    ///
    /// * `new_ticks` - The new tick count
    ///
    /// # Panics
    ///
    /// Panics if `new_ticks < self.ticks` (would violate monotonicity).
    pub fn set_ticks(&mut self, new_ticks: u64) {
        assert!(
            new_ticks >= self.ticks,
            "Cannot set ticks backwards: {} < {}",
            new_ticks,
            self.ticks
        );
        self.ticks = new_ticks;
    }

    /// Returns the current tick count without advancing time
    ///
    /// This is equivalent to `poll_ticks()` but doesn't require
    /// mutable access, making it convenient for assertions.
    pub fn current_ticks(&self) -> u64 {
        self.ticks
    }
}

impl Default for SimTimerDevice {
    fn default() -> Self {
        Self::new()
    }
}

impl TimerDevice for SimTimerDevice {
    fn poll_ticks(&mut self) -> u64 {
        self.ticks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_timer_starts_at_zero() {
        let mut timer = SimTimerDevice::new();
        assert_eq!(timer.poll_ticks(), 0);
    }

    #[test]
    fn test_timer_with_initial_ticks() {
        let mut timer = SimTimerDevice::with_initial_ticks(1000);
        assert_eq!(timer.poll_ticks(), 1000);
    }

    #[test]
    fn test_advance_ticks() {
        let mut timer = SimTimerDevice::new();
        timer.advance_ticks(100);
        assert_eq!(timer.poll_ticks(), 100);
        timer.advance_ticks(50);
        assert_eq!(timer.poll_ticks(), 150);
    }

    #[test]
    fn test_monotonic_progression() {
        let mut timer = SimTimerDevice::new();
        let t1 = timer.poll_ticks();
        timer.advance_ticks(10);
        let t2 = timer.poll_ticks();
        timer.advance_ticks(20);
        let t3 = timer.poll_ticks();

        assert!(t2 >= t1);
        assert!(t3 >= t2);
        assert_eq!(t2 - t1, 10);
        assert_eq!(t3 - t2, 20);
    }

    #[test]
    fn test_set_ticks_forward() {
        let mut timer = SimTimerDevice::new();
        timer.set_ticks(500);
        assert_eq!(timer.poll_ticks(), 500);
        timer.set_ticks(1000);
        assert_eq!(timer.poll_ticks(), 1000);
    }

    #[test]
    #[should_panic(expected = "Cannot set ticks backwards")]
    fn test_set_ticks_backwards_panics() {
        let mut timer = SimTimerDevice::with_initial_ticks(100);
        timer.set_ticks(50); // Should panic
    }

    #[test]
    fn test_current_ticks_immutable() {
        let timer = SimTimerDevice::with_initial_ticks(42);
        assert_eq!(timer.current_ticks(), 42);
        assert_eq!(timer.current_ticks(), 42); // Can call multiple times
    }

    #[test]
    fn test_zero_advance() {
        let mut timer = SimTimerDevice::new();
        timer.advance_ticks(0);
        assert_eq!(timer.poll_ticks(), 0);
    }

    #[test]
    fn test_large_tick_values() {
        let mut timer = SimTimerDevice::with_initial_ticks(u64::MAX - 1000);
        timer.advance_ticks(500);
        assert_eq!(timer.poll_ticks(), u64::MAX - 500);
    }

    #[test]
    #[should_panic(expected = "Timer tick overflow")]
    fn test_overflow_panics() {
        let mut timer = SimTimerDevice::with_initial_ticks(u64::MAX);
        timer.advance_ticks(1); // Should panic
    }

    #[test]
    fn test_deterministic_sequence() {
        // Demonstrate deterministic behavior
        let mut timer1 = SimTimerDevice::new();
        let mut timer2 = SimTimerDevice::new();

        let sequence = vec![10, 20, 5, 100, 3];

        for &delta in &sequence {
            timer1.advance_ticks(delta);
            timer2.advance_ticks(delta);
        }

        assert_eq!(timer1.poll_ticks(), timer2.poll_ticks());
        assert_eq!(timer1.poll_ticks(), 10 + 20 + 5 + 100 + 3);
    }

    #[test]
    fn test_clone_preserves_state() {
        let mut timer1 = SimTimerDevice::with_initial_ticks(100);
        timer1.advance_ticks(50);

        let timer2 = timer1.clone();

        assert_eq!(timer1.current_ticks(), timer2.current_ticks());
        assert_eq!(timer1.current_ticks(), 150);
    }
}

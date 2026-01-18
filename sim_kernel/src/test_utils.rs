//! Test utilities for resilience testing
//!
//! This module provides helper functions and utilities for writing
//! resilience and integration tests.

use crate::fault_injection::FaultPlan;
use crate::SimulatedKernel;
use kernel_api::{Duration, KernelApi};

/// Runs a test with a fault plan applied
///
/// This is a convenience helper that creates a kernel with the given
/// fault plan and passes it to the test closure.
///
/// # Example
///
/// ```
/// use sim_kernel::test_utils::with_fault_plan;
/// use sim_kernel::fault_injection::{FaultPlan, MessageFault};
/// use kernel_api::Duration;
///
/// with_fault_plan(
///     FaultPlan::new().with_message_fault(MessageFault::DropNext { count: 1 }),
///     |kernel| {
///         // Test code here
///     }
/// );
/// ```
pub fn with_fault_plan<F>(plan: FaultPlan, f: F)
where
    F: FnOnce(&mut SimulatedKernel),
{
    let mut kernel = SimulatedKernel::new().with_fault_plan(plan);
    f(&mut kernel);
}

/// Advances time until the kernel is idle
///
/// This is useful for tests that need to wait for asynchronous operations
/// to complete.
pub fn advance_until_idle(kernel: &mut SimulatedKernel, max_duration: Duration) {
    let start_time = kernel.now();
    const TIME_STEP: Duration = Duration::from_millis(1);

    while !kernel.is_idle() {
        let elapsed = kernel.now().duration_since(start_time);
        if elapsed >= max_duration {
            break;
        }
        kernel.advance_time(TIME_STEP);
    }
}

/// Runs a kernel for a specific duration, processing delayed messages
///
/// Unlike `run_until_idle`, this runs for exactly the specified duration,
/// which is useful for timeout testing.
pub fn run_for_duration(kernel: &mut SimulatedKernel, duration: Duration) {
    let target_time = kernel.now() + duration;
    const TIME_STEP: Duration = Duration::from_millis(1);

    while kernel.now() < target_time {
        kernel.advance_time(TIME_STEP);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fault_injection::{FaultPlan, MessageFault};

    #[test]
    fn test_with_fault_plan() {
        let plan = FaultPlan::new().with_message_fault(MessageFault::DropNext { count: 1 });

        with_fault_plan(plan, |kernel| {
            assert_eq!(kernel.task_count(), 0);
        });
    }

    #[test]
    fn test_advance_until_idle() {
        let mut kernel = SimulatedKernel::new();
        advance_until_idle(&mut kernel, Duration::from_secs(1));
        assert!(kernel.is_idle());
    }

    #[test]
    fn test_run_for_duration() {
        let mut kernel = SimulatedKernel::new();
        let start = kernel.now();
        let duration = Duration::from_millis(100);

        run_for_duration(&mut kernel, duration);

        let elapsed = kernel.now().duration_since(start);
        assert!(elapsed >= duration);
    }
}

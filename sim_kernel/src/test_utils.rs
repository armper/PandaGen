//! Test utilities for resilience testing
//!
//! This module provides helper functions and utilities for writing
//! resilience and integration tests.

use crate::fault_injection::FaultPlan;
use crate::SimulatedKernel;
use kernel_api::{Duration, KernelApi};

/// Input injection utilities (simulation-only, Phase 14)
///
/// These functions allow tests to inject input events into the simulated kernel.
/// They are deliberately separated and only available in test/simulation contexts.
#[cfg(test)]
pub mod input_injection {
    use input_types::InputEvent;
    use services_input::InputSubscriptionCap;

    /// Simulated input event queue
    ///
    /// In a real system, this would be hardware events.
    /// In simulation, tests explicitly inject events.
    pub struct InputEventQueue {
        events: Vec<InputEvent>,
    }

    impl InputEventQueue {
        /// Creates a new input event queue
        pub fn new() -> Self {
            Self { events: Vec::new() }
        }

        /// Injects a key event into the queue
        pub fn inject_event(&mut self, event: InputEvent) {
            self.events.push(event);
        }

        /// Retrieves the next event, if any
        pub fn next_event(&mut self) -> Option<InputEvent> {
            if self.events.is_empty() {
                None
            } else {
                Some(self.events.remove(0))
            }
        }

        /// Returns the number of pending events
        pub fn pending_count(&self) -> usize {
            self.events.len()
        }

        /// Clears all pending events
        pub fn clear(&mut self) {
            self.events.clear();
        }
    }

    impl Default for InputEventQueue {
        fn default() -> Self {
            Self::new()
        }
    }

    /// Helper for delivering events to subscriptions
    ///
    /// In a real kernel, this would be done by hardware interrupt handlers.
    /// In simulation, tests call this explicitly.
    pub fn deliver_event_to_subscription(
        _cap: &InputSubscriptionCap,
        _event: &InputEvent,
    ) -> Result<(), String> {
        // In a real implementation, this would:
        // 1. Verify subscription is active
        // 2. Send message via kernel IPC
        // 3. Consume message budget
        //
        // For now, just return success (actual delivery via kernel API)
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use input_types::{KeyCode, KeyEvent, Modifiers};

        #[test]
        fn test_input_queue_creation() {
            let queue = InputEventQueue::new();
            assert_eq!(queue.pending_count(), 0);
        }

        #[test]
        fn test_inject_and_retrieve_event() {
            let mut queue = InputEventQueue::new();
            let event = InputEvent::key(KeyEvent::pressed(KeyCode::A, Modifiers::none()));

            queue.inject_event(event.clone());
            assert_eq!(queue.pending_count(), 1);

            let retrieved = queue.next_event();
            assert_eq!(retrieved, Some(event));
            assert_eq!(queue.pending_count(), 0);
        }

        #[test]
        fn test_multiple_events_fifo_order() {
            let mut queue = InputEventQueue::new();
            let event1 = InputEvent::key(KeyEvent::pressed(KeyCode::A, Modifiers::none()));
            let event2 = InputEvent::key(KeyEvent::pressed(KeyCode::B, Modifiers::none()));
            let event3 = InputEvent::key(KeyEvent::pressed(KeyCode::C, Modifiers::none()));

            queue.inject_event(event1.clone());
            queue.inject_event(event2.clone());
            queue.inject_event(event3.clone());

            assert_eq!(queue.next_event(), Some(event1));
            assert_eq!(queue.next_event(), Some(event2));
            assert_eq!(queue.next_event(), Some(event3));
            assert_eq!(queue.next_event(), None);
        }

        #[test]
        fn test_clear_queue() {
            let mut queue = InputEventQueue::new();
            let event = InputEvent::key(KeyEvent::pressed(KeyCode::A, Modifiers::none()));

            queue.inject_event(event);
            assert_eq!(queue.pending_count(), 1);

            queue.clear();
            assert_eq!(queue.pending_count(), 0);
        }
    }
}

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

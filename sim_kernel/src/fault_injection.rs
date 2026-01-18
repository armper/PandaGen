//! Deterministic fault injection for testing
//!
//! This module provides a composable fault injection framework that allows
//! tests to inject faults into the simulated kernel's message delivery pipeline.
//!
//! ## Design Philosophy
//!
//! - **Deterministic**: No randomness unless explicitly seeded
//! - **Composable**: Multiple fault injectors can be combined
//! - **Minimal**: Small API surface, well-documented
//! - **Test-focused**: Not intended for production use
//!
//! ## Example
//!
//! ```
//! use sim_kernel::fault_injection::{FaultPlan, MessageFault};
//! use kernel_api::Duration;
//!
//! let plan = FaultPlan::new()
//!     .with_message_fault(MessageFault::DropNext { count: 2 })
//!     .with_message_fault(MessageFault::Delay { duration: Duration::from_millis(100) });
//! ```

use ipc::{ChannelId, MessageEnvelope};
use kernel_api::Duration;
use std::collections::VecDeque;

/// A fault to inject into message delivery
#[derive(Debug, Clone)]
pub enum MessageFault {
    /// Drop the next N messages on any channel
    DropNext { count: usize },

    /// Drop the next N messages on a specific channel
    DropNextOnChannel { channel: ChannelId, count: usize },

    /// Drop messages matching a predicate (for deterministic testing)
    DropMatching { action: String },

    /// Delay the next N messages by a duration
    Delay { duration: Duration },

    /// Reorder messages within a bounded window (swap positions)
    /// Swaps message at index with the one at index + offset
    ReorderWindow { index: usize, offset: usize },
}

/// A fault to inject into task/service lifecycle
#[derive(Debug, Clone)]
pub enum LifecycleFault {
    /// Crash a task on the next send operation
    CrashOnSend,

    /// Crash a task on the next receive operation
    CrashOnRecv,

    /// Crash a task after successfully handling N messages
    CrashAfterMessages { count: usize },
}

/// A plan describing all faults to inject
///
/// This is configured per-test and provides a deterministic way to
/// inject various failure modes into the system.
#[derive(Debug, Clone, Default)]
pub struct FaultPlan {
    /// Message-level faults
    message_faults: Vec<MessageFault>,

    /// Lifecycle faults
    lifecycle_faults: Vec<LifecycleFault>,
}

impl FaultPlan {
    /// Creates a new empty fault plan
    pub fn new() -> Self {
        Self {
            message_faults: Vec::new(),
            lifecycle_faults: Vec::new(),
        }
    }

    /// Adds a message fault to the plan
    pub fn with_message_fault(mut self, fault: MessageFault) -> Self {
        self.message_faults.push(fault);
        self
    }

    /// Adds a lifecycle fault to the plan
    pub fn with_lifecycle_fault(mut self, fault: LifecycleFault) -> Self {
        self.lifecycle_faults.push(fault);
        self
    }

    /// Returns a reference to the message faults
    pub fn message_faults(&self) -> &[MessageFault] {
        &self.message_faults
    }

    /// Returns a reference to the lifecycle faults
    pub fn lifecycle_faults(&self) -> &[LifecycleFault] {
        &self.lifecycle_faults
    }
}

/// Fault injector that applies faults to message delivery
///
/// This maintains state about which faults have been applied and
/// modifies the message queue according to the fault plan.
#[derive(Debug)]
pub struct FaultInjector {
    plan: FaultPlan,

    // State tracking for stateful faults
    messages_processed: usize,
    drop_next_count: usize,
    drop_next_on_channel: std::collections::HashMap<ChannelId, usize>,
    delay_next_count: usize,
    delay_duration: Option<Duration>,
    crash_after_messages: Option<usize>,
    should_crash_on_send: bool,
    should_crash_on_recv: bool,
}

impl FaultInjector {
    /// Creates a new fault injector with the given plan
    pub fn new(plan: FaultPlan) -> Self {
        let mut injector = Self {
            plan: plan.clone(),
            messages_processed: 0,
            drop_next_count: 0,
            drop_next_on_channel: std::collections::HashMap::new(),
            delay_next_count: 0,
            delay_duration: None,
            crash_after_messages: None,
            should_crash_on_send: false,
            should_crash_on_recv: false,
        };

        // Initialize state from plan
        for fault in plan.message_faults() {
            match fault {
                MessageFault::DropNext { count } => {
                    injector.drop_next_count = *count;
                }
                MessageFault::DropNextOnChannel { channel, count } => {
                    injector.drop_next_on_channel.insert(*channel, *count);
                }
                MessageFault::Delay { duration } => {
                    injector.delay_next_count = 1;
                    injector.delay_duration = Some(*duration);
                }
                MessageFault::DropMatching { .. } => {
                    // Handled per-message
                }
                MessageFault::ReorderWindow { .. } => {
                    // Handled when applying to queue
                }
            }
        }

        for fault in plan.lifecycle_faults() {
            match fault {
                LifecycleFault::CrashOnSend => {
                    injector.should_crash_on_send = true;
                }
                LifecycleFault::CrashOnRecv => {
                    injector.should_crash_on_recv = true;
                }
                LifecycleFault::CrashAfterMessages { count } => {
                    injector.crash_after_messages = Some(*count);
                }
            }
        }

        injector
    }

    /// Checks if a message should be dropped
    ///
    /// Returns true if the fault injector determines this message should be dropped
    pub fn should_drop_message(&mut self, channel: ChannelId, message: &MessageEnvelope) -> bool {
        // Check global drop next
        if self.drop_next_count > 0 {
            self.drop_next_count -= 1;
            return true;
        }

        // Check channel-specific drop
        if let Some(count) = self.drop_next_on_channel.get_mut(&channel) {
            if *count > 0 {
                *count -= 1;
                return true;
            }
        }

        // Check action-based drop
        for fault in self.plan.message_faults() {
            if let MessageFault::DropMatching { action } = fault {
                if message.action == *action {
                    return true;
                }
            }
        }

        false
    }

    /// Returns the delay to apply to the next message, if any
    pub fn get_message_delay(&mut self) -> Option<Duration> {
        if self.delay_next_count > 0 {
            self.delay_next_count -= 1;
            self.delay_duration
        } else {
            None
        }
    }

    /// Applies reordering faults to a message queue
    pub fn apply_reordering(&self, messages: &mut VecDeque<MessageEnvelope>) {
        for fault in self.plan.message_faults() {
            if let MessageFault::ReorderWindow { index, offset } = fault {
                if *index < messages.len() && *index + offset < messages.len() {
                    messages.swap(*index, *index + offset);
                }
            }
        }
    }

    /// Checks if a crash should occur on send
    pub fn should_crash_on_send(&mut self) -> bool {
        self.should_crash_on_send
    }

    /// Checks if a crash should occur on receive
    pub fn should_crash_on_recv(&mut self) -> bool {
        self.should_crash_on_recv
    }

    /// Records that a message was processed
    pub fn record_message_processed(&mut self) {
        self.messages_processed += 1;

        // Check if we should crash after N messages
        if let Some(crash_after) = self.crash_after_messages {
            if self.messages_processed >= crash_after {
                // Mark for crash (actual crash handled by caller)
                self.should_crash_on_recv = true;
            }
        }
    }

    /// Returns the number of messages processed
    pub fn messages_processed(&self) -> usize {
        self.messages_processed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_types::ServiceId;
    use ipc::SchemaVersion;

    #[test]
    fn test_fault_plan_creation() {
        let plan = FaultPlan::new();
        assert_eq!(plan.message_faults().len(), 0);
        assert_eq!(plan.lifecycle_faults().len(), 0);
    }

    #[test]
    fn test_fault_plan_with_message_fault() {
        let plan = FaultPlan::new().with_message_fault(MessageFault::DropNext { count: 2 });
        assert_eq!(plan.message_faults().len(), 1);
    }

    #[test]
    fn test_fault_injector_drop_next() {
        let plan = FaultPlan::new().with_message_fault(MessageFault::DropNext { count: 2 });
        let mut injector = FaultInjector::new(plan);

        let channel = ChannelId::new();
        let message = create_test_message();

        // Should drop first two messages
        assert!(injector.should_drop_message(channel, &message));
        assert!(injector.should_drop_message(channel, &message));
        assert!(!injector.should_drop_message(channel, &message));
    }

    #[test]
    fn test_fault_injector_drop_on_channel() {
        let channel1 = ChannelId::new();
        let channel2 = ChannelId::new();

        let plan = FaultPlan::new().with_message_fault(MessageFault::DropNextOnChannel {
            channel: channel1,
            count: 1,
        });
        let mut injector = FaultInjector::new(plan);

        let message = create_test_message();

        // Should drop on channel1 but not channel2
        assert!(injector.should_drop_message(channel1, &message));
        assert!(!injector.should_drop_message(channel2, &message));
        assert!(!injector.should_drop_message(channel1, &message));
    }

    #[test]
    fn test_fault_injector_drop_matching() {
        let plan = FaultPlan::new().with_message_fault(MessageFault::DropMatching {
            action: "test.action".to_string(),
        });
        let mut injector = FaultInjector::new(plan);

        let channel = ChannelId::new();
        let message1 = create_test_message_with_action("test.action");
        let message2 = create_test_message_with_action("other.action");

        // Should drop matching action
        assert!(injector.should_drop_message(channel, &message1));
        assert!(!injector.should_drop_message(channel, &message2));
    }

    #[test]
    fn test_fault_injector_delay() {
        let duration = Duration::from_millis(100);
        let plan = FaultPlan::new().with_message_fault(MessageFault::Delay { duration });
        let mut injector = FaultInjector::new(plan);

        // Should return delay once
        assert_eq!(injector.get_message_delay(), Some(duration));
        assert_eq!(injector.get_message_delay(), None);
    }

    #[test]
    fn test_fault_injector_reorder() {
        let plan = FaultPlan::new().with_message_fault(MessageFault::ReorderWindow {
            index: 0,
            offset: 1,
        });
        let injector = FaultInjector::new(plan);

        let mut messages = VecDeque::new();
        messages.push_back(create_test_message_with_action("first"));
        messages.push_back(create_test_message_with_action("second"));

        injector.apply_reordering(&mut messages);

        // Messages should be swapped
        assert_eq!(messages[0].action, "second");
        assert_eq!(messages[1].action, "first");
    }

    #[test]
    fn test_fault_injector_crash_on_send() {
        let plan = FaultPlan::new().with_lifecycle_fault(LifecycleFault::CrashOnSend);
        let mut injector = FaultInjector::new(plan);

        assert!(injector.should_crash_on_send());
    }

    #[test]
    fn test_fault_injector_crash_after_messages() {
        let plan =
            FaultPlan::new().with_lifecycle_fault(LifecycleFault::CrashAfterMessages { count: 2 });
        let mut injector = FaultInjector::new(plan);

        assert!(!injector.should_crash_on_recv());
        injector.record_message_processed();
        assert!(!injector.should_crash_on_recv());
        injector.record_message_processed();
        assert!(injector.should_crash_on_recv());
    }

    // Helper functions
    fn create_test_message() -> MessageEnvelope {
        create_test_message_with_action("test.action")
    }

    fn create_test_message_with_action(action: &str) -> MessageEnvelope {
        let payload = ipc::MessagePayload::new(&"test").unwrap();
        ipc::MessageEnvelope::new(
            ServiceId::new(),
            action.to_string(),
            SchemaVersion::new(1, 0),
            payload,
        )
    }
}

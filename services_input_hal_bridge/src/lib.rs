//! # Input HAL Bridge Service
//!
//! This service bridges the Hardware Abstraction Layer (HAL) keyboard input
//! to the PandaGen input system.
//!
//! ## Philosophy
//!
//! - **Hardware is just a source**: No authority, just events
//! - **Same abstraction as simulation**: Uses existing services_input interface
//! - **Policy/budget compliant**: Subject to same rules as other components
//! - **Feature-gated**: Only enabled in "real run" mode
//! - **No breaking changes**: Simulation tests continue to work unchanged
//!
//! ## Design
//!
//! The bridge is a component with explicit identity and budget:
//! - Polls HAL KeyboardDevice for events
//! - Translates HalKeyEvent → KeyEvent via HAL translation layer
//! - Delivers events to services_input (via channel)
//! - Consumes MessageCount budget for each delivery
//! - Respects policy decisions on event routing
//!
//! ## Usage
//!
//! ```rust,ignore
//! use services_input_hal_bridge::InputHalBridge;
//! use hal_x86_64::X86Ps2Keyboard;
//!
//! // Create bridge with hardware keyboard
//! let keyboard = Box::new(X86Ps2Keyboard::new());
//! let mut bridge = InputHalBridge::new(
//!     execution_id,
//!     input_service_channel,
//!     keyboard,
//! );
//!
//! // Poll loop (run as a component/task)
//! loop {
//!     match bridge.poll(&mut kernel) {
//!         Ok(PollResult::EventDelivered) => { /* continue */ }
//!         Ok(PollResult::NoEvent) => { /* sleep */ }
//!         Err(BridgeError::BudgetExhausted) => break,
//!         Err(e) => { /* handle error */ }
//!     }
//! }
//! ```

use core_types::TaskId;
use hal::{HalKeyEvent, KeyboardDevice, KeyboardTranslator};
use identity::ExecutionId;
use input_types::InputEvent;
use services_input::InputSubscriptionCap;
use thiserror::Error;

/// Bridge error types
#[derive(Debug, Error)]
pub enum BridgeError {
    #[error("Budget exhausted: {resource}")]
    BudgetExhausted { resource: String },

    #[error("Policy denied event delivery: {reason}")]
    PolicyDenied { reason: String },

    #[error("Input service error: {0}")]
    InputServiceError(String),

    #[error("Channel error: {0}")]
    ChannelError(String),
}

/// Poll result
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollResult {
    /// An event was received from hardware and delivered
    EventDelivered,
    /// No event was available from hardware
    NoEvent,
}

/// Input HAL Bridge
///
/// Bridges hardware keyboard input to the PandaGen input system.
///
/// This component:
/// - Owns a KeyboardDevice (hardware abstraction)
/// - Owns a KeyboardTranslator (scancode → KeyCode)
/// - Delivers events via InputSubscriptionCap
/// - Tracks its own ExecutionId for budget/policy enforcement
pub struct InputHalBridge {
    /// Execution identity of this bridge component
    execution_id: ExecutionId,
    
    /// Task ID for message delivery (used in real kernel integration)
    #[allow(dead_code)]
    task_id: TaskId,
    
    /// Input subscription capability (for event delivery)
    subscription: InputSubscriptionCap,
    
    /// Hardware keyboard device
    keyboard: Box<dyn KeyboardDevice>,
    
    /// Scancode translator
    translator: KeyboardTranslator,
    
    /// Number of events delivered (for diagnostics)
    events_delivered: u64,
}

impl InputHalBridge {
    /// Creates a new input HAL bridge
    ///
    /// # Arguments
    ///
    /// * `execution_id` - Execution identity for this bridge
    /// * `task_id` - Task ID for message source
    /// * `subscription` - Input subscription capability
    /// * `keyboard` - Hardware keyboard device
    pub fn new(
        execution_id: ExecutionId,
        task_id: TaskId,
        subscription: InputSubscriptionCap,
        keyboard: Box<dyn KeyboardDevice>,
    ) -> Self {
        Self {
            execution_id,
            task_id,
            subscription,
            keyboard,
            translator: KeyboardTranslator::new(),
            events_delivered: 0,
        }
    }
    
    /// Polls for a keyboard event and delivers it if available
    ///
    /// Returns:
    /// - `Ok(PollResult::EventDelivered)` if an event was delivered
    /// - `Ok(PollResult::NoEvent)` if no hardware event was available
    /// - `Err(BridgeError)` on failure (budget exhaustion, policy denial, etc.)
    ///
    /// Note: In a real implementation, this would call kernel API to send
    /// the event as a message. For now, it's a placeholder that shows the flow.
    pub fn poll(&mut self) -> Result<PollResult, BridgeError> {
        // Poll hardware
        let hal_event = match self.keyboard.poll_event() {
            Some(event) => event,
            None => return Ok(PollResult::NoEvent),
        };
        
        // Translate to KeyEvent
        let key_event = match self.translator.translate(hal_event) {
            Some(event) => event,
            None => return Ok(PollResult::NoEvent), // Unknown key, skip
        };
        
        // Create InputEvent
        let input_event = InputEvent::key(key_event);
        
        // Deliver event
        // NOTE: In real implementation, this would:
        // 1. Check budget via kernel.try_consume_message(execution_id)
        // 2. Check policy via policy_engine.evaluate(...)
        // 3. Send message via kernel.send_message(subscription.channel, message)
        //
        // For now, we just increment counter to show the flow
        self.deliver_event(input_event)?;
        
        Ok(PollResult::EventDelivered)
    }
    
    /// Delivers an input event (internal helper)
    fn deliver_event(&mut self, _event: InputEvent) -> Result<(), BridgeError> {
        // Placeholder for message delivery
        // Real implementation would send via kernel API
        
        // Check budget (simulated)
        // if kernel.try_consume_message(self.execution_id).is_err() {
        //     return Err(BridgeError::BudgetExhausted {
        //         resource: "MessageCount".to_string(),
        //     });
        // }
        
        // Send message (simulated)
        // kernel.send_message(self.subscription.channel, message)?;
        
        self.events_delivered += 1;
        Ok(())
    }
    
    /// Returns the execution ID of this bridge
    pub fn execution_id(&self) -> ExecutionId {
        self.execution_id
    }
    
    /// Returns the subscription capability
    pub fn subscription(&self) -> &InputSubscriptionCap {
        &self.subscription
    }
    
    /// Returns the number of events delivered
    pub fn events_delivered(&self) -> u64 {
        self.events_delivered
    }
    
    /// Resets the translator state (all modifiers released)
    pub fn reset_translator(&mut self) {
        self.translator.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_types::TaskId;
    use ipc::ChannelId;
    
    /// Fake keyboard for testing
    struct FakeKeyboard {
        events: Vec<HalKeyEvent>,
        index: usize,
    }
    
    impl FakeKeyboard {
        fn new(events: Vec<HalKeyEvent>) -> Self {
            Self { events, index: 0 }
        }
    }
    
    impl KeyboardDevice for FakeKeyboard {
        fn poll_event(&mut self) -> Option<HalKeyEvent> {
            if self.index < self.events.len() {
                let event = self.events[self.index];
                self.index += 1;
                Some(event)
            } else {
                None
            }
        }
    }
    
    #[test]
    fn test_bridge_creation() {
        let exec_id = ExecutionId::new();
        let task_id = TaskId::new();
        let subscription = InputSubscriptionCap::new(1, task_id, ChannelId::new());
        let keyboard = Box::new(FakeKeyboard::new(vec![]));
        
        let bridge = InputHalBridge::new(exec_id, task_id, subscription, keyboard);
        assert_eq!(bridge.execution_id(), exec_id);
        assert_eq!(bridge.events_delivered(), 0);
    }
    
    #[test]
    fn test_bridge_poll_no_event() {
        let exec_id = ExecutionId::new();
        let task_id = TaskId::new();
        let subscription = InputSubscriptionCap::new(1, task_id, ChannelId::new());
        let keyboard = Box::new(FakeKeyboard::new(vec![]));
        
        let mut bridge = InputHalBridge::new(exec_id, task_id, subscription, keyboard);
        let result = bridge.poll().unwrap();
        
        assert_eq!(result, PollResult::NoEvent);
        assert_eq!(bridge.events_delivered(), 0);
    }
    
    #[test]
    fn test_bridge_poll_with_event() {
        let exec_id = ExecutionId::new();
        let task_id = TaskId::new();
        let subscription = InputSubscriptionCap::new(1, task_id, ChannelId::new());
        
        // Create fake keyboard with A key press
        let events = vec![HalKeyEvent::new(0x1E, true)]; // A pressed
        let keyboard = Box::new(FakeKeyboard::new(events));
        
        let mut bridge = InputHalBridge::new(exec_id, task_id, subscription, keyboard);
        let result = bridge.poll().unwrap();
        
        assert_eq!(result, PollResult::EventDelivered);
        assert_eq!(bridge.events_delivered(), 1);
    }
    
    #[test]
    fn test_bridge_poll_multiple_events() {
        let exec_id = ExecutionId::new();
        let task_id = TaskId::new();
        let subscription = InputSubscriptionCap::new(1, task_id, ChannelId::new());
        
        // A pressed, A released, B pressed
        let events = vec![
            HalKeyEvent::new(0x1E, true),
            HalKeyEvent::new(0x1E, false),
            HalKeyEvent::new(0x30, true),
        ];
        let keyboard = Box::new(FakeKeyboard::new(events));
        
        let mut bridge = InputHalBridge::new(exec_id, task_id, subscription, keyboard);
        
        assert_eq!(bridge.poll().unwrap(), PollResult::EventDelivered);
        assert_eq!(bridge.poll().unwrap(), PollResult::EventDelivered);
        assert_eq!(bridge.poll().unwrap(), PollResult::EventDelivered);
        assert_eq!(bridge.poll().unwrap(), PollResult::NoEvent);
        
        assert_eq!(bridge.events_delivered(), 3);
    }
    
    #[test]
    fn test_bridge_poll_unknown_key() {
        let exec_id = ExecutionId::new();
        let task_id = TaskId::new();
        let subscription = InputSubscriptionCap::new(1, task_id, ChannelId::new());
        
        // Unknown scancode
        let events = vec![HalKeyEvent::new(0xFF, true)];
        let keyboard = Box::new(FakeKeyboard::new(events));
        
        let mut bridge = InputHalBridge::new(exec_id, task_id, subscription, keyboard);
        let result = bridge.poll().unwrap();
        
        // Unknown keys are skipped
        assert_eq!(result, PollResult::NoEvent);
        assert_eq!(bridge.events_delivered(), 0);
    }
    
    #[test]
    fn test_bridge_reset_translator() {
        let exec_id = ExecutionId::new();
        let task_id = TaskId::new();
        let subscription = InputSubscriptionCap::new(1, task_id, ChannelId::new());
        
        // Ctrl pressed
        let events = vec![HalKeyEvent::new(0x1D, true)];
        let keyboard = Box::new(FakeKeyboard::new(events));
        
        let mut bridge = InputHalBridge::new(exec_id, task_id, subscription, keyboard);
        bridge.poll().unwrap();
        
        // Reset translator (clears modifier state)
        bridge.reset_translator();
        
        // After reset, translator should have no modifiers
        // (can't directly test internal state, but this exercises the code)
    }
}

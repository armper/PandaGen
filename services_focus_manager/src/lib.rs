//! # Focus Manager Service
//!
//! This crate implements the focus manager for PandaGen OS.
//!
//! ## Philosophy
//!
//! - **Explicit focus**: Focus is not ambient; it's explicitly requested and granted
//! - **Policy-driven**: Focus grants may be evaluated by policy engines
//! - **Stack-based**: Focus follows a stack model (push/pop)
//! - **Auditable**: All focus changes are logged for audit
//!
//! ## Non-Goals
//!
//! This is NOT:
//! - A window manager (no Z-order, no geometry)
//! - X11 focus model (no implicit focus, no focus follows mouse)
//! - A global focus singleton

use input_types::InputEvent;
use serde::{Deserialize, Serialize};
use services_input::InputSubscriptionCap;
use std::collections::VecDeque;
use thiserror::Error;

/// Focus manager error types
#[derive(Debug, Error, PartialEq, Eq)]
pub enum FocusError {
    #[error("Focus denied: {reason}")]
    FocusDenied { reason: String },

    #[error("No focused subscription")]
    NoFocus,

    #[error("Subscription not found in focus stack")]
    SubscriptionNotFound,

    #[error("Focus stack empty")]
    EmptyStack,
}

/// Focus event for audit trail
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FocusEvent {
    /// Focus was granted to a subscription
    Granted {
        subscription_id: u64,
        timestamp_ns: u64,
    },
    /// Focus was transferred from one subscription to another
    Transferred {
        from_subscription_id: u64,
        to_subscription_id: u64,
        timestamp_ns: u64,
    },
    /// Focus was released
    Released {
        subscription_id: u64,
        timestamp_ns: u64,
    },
    /// Focus request was denied
    Denied {
        subscription_id: u64,
        reason: String,
        timestamp_ns: u64,
    },
}

/// Focus manager
///
/// Manages focus policy and routes input events to the focused subscriber.
/// Uses a stack-based model where the top of the stack has focus.
pub struct FocusManager {
    /// Focus stack (top of stack has focus)
    focus_stack: VecDeque<InputSubscriptionCap>,
    /// Audit trail of focus events
    audit_trail: Vec<FocusEvent>,
    /// Next timestamp (for simulation)
    next_timestamp: u64,
}

impl FocusManager {
    /// Creates a new focus manager
    pub fn new() -> Self {
        Self {
            focus_stack: VecDeque::new(),
            audit_trail: Vec::new(),
            next_timestamp: 0,
        }
    }

    /// Requests focus for a subscription
    ///
    /// Pushes the subscription onto the focus stack and grants it focus.
    /// Policy evaluation would happen here in a full implementation.
    pub fn request_focus(&mut self, cap: InputSubscriptionCap) -> Result<(), FocusError> {
        // In a full implementation, this would consult policy engine
        // For now, we always grant focus

        let timestamp = self.next_timestamp();

        // Check if already focused
        if let Some(current) = self.focus_stack.front() {
            if current.id == cap.id {
                // Already has focus, no-op (don't push duplicate)
                return Ok(());
            }

            // Transfer focus
            self.audit_trail.push(FocusEvent::Transferred {
                from_subscription_id: current.id,
                to_subscription_id: cap.id,
                timestamp_ns: timestamp,
            });
        } else {
            // Grant initial focus
            self.audit_trail.push(FocusEvent::Granted {
                subscription_id: cap.id,
                timestamp_ns: timestamp,
            });
        }

        self.focus_stack.push_front(cap);
        Ok(())
    }

    /// Releases focus from the current subscription
    ///
    /// Pops the top of the focus stack. Focus transfers to the next item
    /// on the stack, if any.
    pub fn release_focus(&mut self) -> Result<InputSubscriptionCap, FocusError> {
        let cap = self.focus_stack.pop_front().ok_or(FocusError::EmptyStack)?;

        let timestamp = self.next_timestamp();
        self.audit_trail.push(FocusEvent::Released {
            subscription_id: cap.id,
            timestamp_ns: timestamp,
        });

        Ok(cap)
    }

    /// Removes a specific subscription from the focus stack
    ///
    /// Useful when a subscription is revoked or terminated.
    pub fn remove_subscription(&mut self, cap: &InputSubscriptionCap) -> Result<(), FocusError> {
        // Find the position of the subscription
        let pos = self
            .focus_stack
            .iter()
            .position(|c| c.id == cap.id)
            .ok_or(FocusError::SubscriptionNotFound)?;

        // Remove at that position
        self.focus_stack.remove(pos);

        let timestamp = self.next_timestamp();
        self.audit_trail.push(FocusEvent::Released {
            subscription_id: cap.id,
            timestamp_ns: timestamp,
        });

        Ok(())
    }

    /// Returns the currently focused subscription, if any
    pub fn current_focus(&self) -> Option<&InputSubscriptionCap> {
        self.focus_stack.front()
    }

    /// Checks if a subscription has focus
    pub fn has_focus(&self, cap: &InputSubscriptionCap) -> bool {
        self.focus_stack
            .front()
            .map(|c| c.id == cap.id)
            .unwrap_or(false)
    }

    /// Routes an event to the focused subscription
    ///
    /// Returns Ok(Some(cap)) if there's a focused subscription,
    /// Ok(None) if no focus, Err if routing fails.
    pub fn route_event(
        &self,
        _event: &InputEvent,
    ) -> Result<Option<InputSubscriptionCap>, FocusError> {
        Ok(self.current_focus().copied())
    }

    /// Returns the focus stack depth
    pub fn stack_depth(&self) -> usize {
        self.focus_stack.len()
    }

    /// Returns the audit trail
    pub fn audit_trail(&self) -> &[FocusEvent] {
        &self.audit_trail
    }

    /// Clears the audit trail (for testing)
    #[cfg(test)]
    pub fn clear_audit_trail(&mut self) {
        self.audit_trail.clear();
    }

    /// Gets next timestamp and increments counter
    fn next_timestamp(&mut self) -> u64 {
        let ts = self.next_timestamp;
        self.next_timestamp += 1;
        ts
    }
}

impl Default for FocusManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_types::TaskId;
    use input_types::{KeyCode, KeyEvent, Modifiers};
    use ipc::ChannelId;

    fn make_subscription(id: u64) -> InputSubscriptionCap {
        InputSubscriptionCap::new(id, TaskId::new(), ChannelId::new())
    }

    #[test]
    fn test_focus_manager_creation() {
        let manager = FocusManager::new();
        assert_eq!(manager.stack_depth(), 0);
        assert!(manager.current_focus().is_none());
    }

    #[test]
    fn test_request_focus() {
        let mut manager = FocusManager::new();
        let sub1 = make_subscription(1);

        manager.request_focus(sub1).unwrap();

        assert_eq!(manager.stack_depth(), 1);
        assert_eq!(manager.current_focus(), Some(&sub1));
        assert!(manager.has_focus(&sub1));
    }

    #[test]
    fn test_request_focus_duplicate_is_noop() {
        let mut manager = FocusManager::new();
        let sub1 = make_subscription(1);

        manager.request_focus(sub1).unwrap();
        manager.request_focus(sub1).unwrap();

        assert_eq!(manager.stack_depth(), 1); // Stack has only one entry
        assert!(manager.has_focus(&sub1));
    }

    #[test]
    fn test_focus_switching() {
        let mut manager = FocusManager::new();
        let sub1 = make_subscription(1);
        let sub2 = make_subscription(2);

        manager.request_focus(sub1).unwrap();
        assert!(manager.has_focus(&sub1));
        assert!(!manager.has_focus(&sub2));

        manager.request_focus(sub2).unwrap();
        assert!(!manager.has_focus(&sub1));
        assert!(manager.has_focus(&sub2));
    }

    #[test]
    fn test_focus_stack_behavior() {
        let mut manager = FocusManager::new();
        let sub1 = make_subscription(1);
        let sub2 = make_subscription(2);
        let sub3 = make_subscription(3);

        manager.request_focus(sub1).unwrap();
        manager.request_focus(sub2).unwrap();
        manager.request_focus(sub3).unwrap();

        assert_eq!(manager.stack_depth(), 3);
        assert!(manager.has_focus(&sub3));
    }

    #[test]
    fn test_release_focus() {
        let mut manager = FocusManager::new();
        let sub1 = make_subscription(1);
        let sub2 = make_subscription(2);

        manager.request_focus(sub1).unwrap();
        manager.request_focus(sub2).unwrap();

        assert!(manager.has_focus(&sub2));

        let released = manager.release_focus().unwrap();
        assert_eq!(released.id, sub2.id);
        assert!(manager.has_focus(&sub1));
    }

    #[test]
    fn test_release_focus_empty_stack() {
        let mut manager = FocusManager::new();
        let result = manager.release_focus();
        assert_eq!(result, Err(FocusError::EmptyStack));
    }

    #[test]
    fn test_remove_subscription() {
        let mut manager = FocusManager::new();
        let sub1 = make_subscription(1);
        let sub2 = make_subscription(2);
        let sub3 = make_subscription(3);

        manager.request_focus(sub1).unwrap();
        manager.request_focus(sub2).unwrap();
        manager.request_focus(sub3).unwrap();

        // Remove middle subscription
        manager.remove_subscription(&sub2).unwrap();
        assert_eq!(manager.stack_depth(), 2);
        assert!(manager.has_focus(&sub3));
    }

    #[test]
    fn test_remove_focused_subscription() {
        let mut manager = FocusManager::new();
        let sub1 = make_subscription(1);
        let sub2 = make_subscription(2);

        manager.request_focus(sub1).unwrap();
        manager.request_focus(sub2).unwrap();

        // Remove focused subscription
        manager.remove_subscription(&sub2).unwrap();
        assert!(manager.has_focus(&sub1));
    }

    #[test]
    fn test_remove_nonexistent_subscription() {
        let mut manager = FocusManager::new();
        let sub1 = make_subscription(1);
        let sub2 = make_subscription(2);

        manager.request_focus(sub1).unwrap();

        let result = manager.remove_subscription(&sub2);
        assert_eq!(result, Err(FocusError::SubscriptionNotFound));
    }

    #[test]
    fn test_route_event_with_focus() {
        let mut manager = FocusManager::new();
        let sub1 = make_subscription(1);

        manager.request_focus(sub1).unwrap();

        let event = InputEvent::key(KeyEvent::pressed(KeyCode::A, Modifiers::none()));
        let target = manager.route_event(&event).unwrap();

        assert!(target.is_some());
        assert_eq!(target.unwrap().id, sub1.id);
    }

    #[test]
    fn test_route_event_without_focus() {
        let manager = FocusManager::new();

        let event = InputEvent::key(KeyEvent::pressed(KeyCode::A, Modifiers::none()));
        let target = manager.route_event(&event).unwrap();

        assert!(target.is_none());
    }

    #[test]
    fn test_route_event_to_focused_only() {
        let mut manager = FocusManager::new();
        let sub1 = make_subscription(1);
        let sub2 = make_subscription(2);

        manager.request_focus(sub1).unwrap();
        manager.request_focus(sub2).unwrap();

        let event = InputEvent::key(KeyEvent::pressed(KeyCode::A, Modifiers::none()));
        let target = manager.route_event(&event).unwrap();

        // Only sub2 should receive the event
        assert!(target.is_some());
        assert_eq!(target.unwrap().id, sub2.id);
    }

    #[test]
    fn test_audit_trail_grant() {
        let mut manager = FocusManager::new();
        let sub1 = make_subscription(1);

        manager.request_focus(sub1).unwrap();

        let trail = manager.audit_trail();
        assert_eq!(trail.len(), 1);

        match &trail[0] {
            FocusEvent::Granted {
                subscription_id, ..
            } => {
                assert_eq!(*subscription_id, 1);
            }
            _ => panic!("Expected Granted event"),
        }
    }

    #[test]
    fn test_audit_trail_transfer() {
        let mut manager = FocusManager::new();
        let sub1 = make_subscription(1);
        let sub2 = make_subscription(2);

        manager.request_focus(sub1).unwrap();
        manager.clear_audit_trail();

        manager.request_focus(sub2).unwrap();

        let trail = manager.audit_trail();
        assert_eq!(trail.len(), 1);

        match &trail[0] {
            FocusEvent::Transferred {
                from_subscription_id,
                to_subscription_id,
                ..
            } => {
                assert_eq!(*from_subscription_id, 1);
                assert_eq!(*to_subscription_id, 2);
            }
            _ => panic!("Expected Transferred event"),
        }
    }

    #[test]
    fn test_audit_trail_release() {
        let mut manager = FocusManager::new();
        let sub1 = make_subscription(1);

        manager.request_focus(sub1).unwrap();
        manager.clear_audit_trail();

        manager.release_focus().unwrap();

        let trail = manager.audit_trail();
        assert_eq!(trail.len(), 1);

        match &trail[0] {
            FocusEvent::Released {
                subscription_id, ..
            } => {
                assert_eq!(*subscription_id, 1);
            }
            _ => panic!("Expected Released event"),
        }
    }

    #[test]
    fn test_focus_event_serialization() {
        let event = FocusEvent::Granted {
            subscription_id: 42,
            timestamp_ns: 1000,
        };

        let json = serde_json::to_string(&event).unwrap();
        let deserialized: FocusEvent = serde_json::from_str(&json).unwrap();

        match deserialized {
            FocusEvent::Granted {
                subscription_id,
                timestamp_ns,
            } => {
                assert_eq!(subscription_id, 42);
                assert_eq!(timestamp_ns, 1000);
            }
            _ => panic!("Wrong event type"),
        }
    }

    #[test]
    fn test_deterministic_focus_switching() {
        let mut manager = FocusManager::new();
        let sub1 = make_subscription(1);
        let sub2 = make_subscription(2);
        let sub3 = make_subscription(3);

        // Build up focus stack
        manager.request_focus(sub1).unwrap();
        manager.request_focus(sub2).unwrap();
        manager.request_focus(sub3).unwrap();

        // Verify deterministic routing
        let event = InputEvent::key(KeyEvent::pressed(KeyCode::A, Modifiers::none()));

        let target1 = manager.route_event(&event).unwrap().unwrap();
        assert_eq!(target1.id, 3);

        manager.release_focus().unwrap();
        let target2 = manager.route_event(&event).unwrap().unwrap();
        assert_eq!(target2.id, 2);

        manager.release_focus().unwrap();
        let target3 = manager.route_event(&event).unwrap().unwrap();
        assert_eq!(target3.id, 1);

        manager.release_focus().unwrap();
        let target4 = manager.route_event(&event).unwrap();
        assert!(target4.is_none());
    }
}

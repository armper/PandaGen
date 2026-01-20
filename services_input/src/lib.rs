//! # Input Service
//!
//! This crate implements the input service for PandaGen OS.
//!
//! ## Philosophy
//!
//! - **Explicit subscriptions**: Input is not ambient; must be explicitly requested
//! - **Capability-based**: Subscriptions are capabilities that can be granted/revoked
//! - **Events, not streams**: Input is structured events, not byte streams
//! - **Budget-aware**: Input delivery consumes MessageCount budget
//!
//! ## Non-Goals
//!
//! This is NOT:
//! - A hardware driver (no PS/2, USB HID)
//! - A terminal emulator (no TTY, no stdin/stdout)
//! - Global keyboard state
//! - A focus manager (that's a separate service)

use core_types::{ServiceId, TaskId};
use input_types::InputEvent;
use ipc::{ChannelId, MessageEnvelope, MessagePayload, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// Input subscription capability
///
/// Represents the right to receive input events.
/// When dropped or revoked, no more events are delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InputSubscriptionCap {
    /// Unique subscription ID
    pub id: u64,
    /// Task that owns this subscription
    pub task_id: TaskId,
    /// Channel for event delivery
    pub channel: ChannelId,
}

impl InputSubscriptionCap {
    /// Creates a new input subscription capability
    pub fn new(id: u64, task_id: TaskId, channel: ChannelId) -> Self {
        Self {
            id,
            task_id,
            channel,
        }
    }
}

/// Input service error types
#[derive(Debug, Error, PartialEq, Eq)]
pub enum InputServiceError {
    #[error("Subscription not found: {0}")]
    SubscriptionNotFound(u64),

    #[error("Subscription already exists for task: {0:?}")]
    SubscriptionAlreadyExists(TaskId),

    #[error("Invalid subscription capability")]
    InvalidCapability,

    #[error("Event delivery failed: {reason}")]
    DeliveryFailed { reason: String },
}

/// Input service schema version (v1.0).
pub const INPUT_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

/// Action identifier for input event messages.
pub const INPUT_EVENT_ACTION: &str = "input.event";

/// Returns the stable service ID for the input service.
pub fn input_service_id() -> ServiceId {
    core_types::input_service_id()
}

/// Builds an input event message envelope.
pub fn build_input_event_envelope(
    event: &InputEvent,
    source: Option<TaskId>,
) -> Result<MessageEnvelope, InputServiceError> {
    let payload = MessagePayload::new(event).map_err(|err| InputServiceError::DeliveryFailed {
        reason: err.to_string(),
    })?;
    let mut envelope = MessageEnvelope::new(
        input_service_id(),
        INPUT_EVENT_ACTION.to_string(),
        INPUT_SCHEMA_VERSION,
        payload,
    );
    if let Some(task_id) = source {
        envelope = envelope.with_source(task_id);
    }
    Ok(envelope)
}

/// Sink interface for delivering input events.
pub trait InputEventSink {
    fn send_event(
        &mut self,
        cap: &InputSubscriptionCap,
        event: &InputEvent,
    ) -> Result<(), InputServiceError>;
}

/// Input subscription record
#[derive(Debug, Clone)]
struct Subscription {
    cap: InputSubscriptionCap,
    active: bool,
}

impl Subscription {
    fn new(cap: InputSubscriptionCap) -> Self {
        Self { cap, active: true }
    }
}

/// Input service
///
/// Manages input subscriptions and event delivery.
/// Does NOT handle focus or routing policy (that's FocusManager's job).
pub struct InputService {
    /// Next subscription ID
    next_subscription_id: u64,
    /// Active subscriptions by ID
    subscriptions: HashMap<u64, Subscription>,
    /// Task to subscription mapping (for lookup)
    task_subscriptions: HashMap<TaskId, u64>,
}

impl InputService {
    /// Creates a new input service
    pub fn new() -> Self {
        Self {
            next_subscription_id: 1,
            subscriptions: HashMap::new(),
            task_subscriptions: HashMap::new(),
        }
    }

    /// Subscribes a task to keyboard input
    ///
    /// Returns a capability that represents the subscription.
    /// Only one subscription per task is allowed.
    pub fn subscribe_keyboard(
        &mut self,
        task_id: TaskId,
        channel: ChannelId,
    ) -> Result<InputSubscriptionCap, InputServiceError> {
        // Check if task already has a subscription
        if self.task_subscriptions.contains_key(&task_id) {
            return Err(InputServiceError::SubscriptionAlreadyExists(task_id));
        }

        let id = self.next_subscription_id;
        self.next_subscription_id += 1;

        let cap = InputSubscriptionCap::new(id, task_id, channel);
        let subscription = Subscription::new(cap);

        self.subscriptions.insert(id, subscription);
        self.task_subscriptions.insert(task_id, id);

        Ok(cap)
    }

    /// Revokes a subscription
    ///
    /// After revocation, no more events will be delivered.
    pub fn revoke_subscription(
        &mut self,
        cap: &InputSubscriptionCap,
    ) -> Result<(), InputServiceError> {
        let subscription = self
            .subscriptions
            .get_mut(&cap.id)
            .ok_or(InputServiceError::SubscriptionNotFound(cap.id))?;

        // Verify ownership
        if subscription.cap.task_id != cap.task_id {
            return Err(InputServiceError::InvalidCapability);
        }

        subscription.active = false;
        Ok(())
    }

    /// Unsubscribes a task completely (removes the subscription)
    pub fn unsubscribe(&mut self, cap: &InputSubscriptionCap) -> Result<(), InputServiceError> {
        let subscription = self
            .subscriptions
            .get(&cap.id)
            .ok_or(InputServiceError::SubscriptionNotFound(cap.id))?;

        // Verify ownership
        if subscription.cap.task_id != cap.task_id {
            return Err(InputServiceError::InvalidCapability);
        }

        self.subscriptions.remove(&cap.id);
        self.task_subscriptions.remove(&cap.task_id);

        Ok(())
    }

    /// Delivers an event to a specific subscription
    ///
    /// Returns Ok(true) if delivered, Ok(false) if subscription inactive,
    /// Err if subscription doesn't exist.
    ///
    /// Note: Actual message sending happens outside this service
    /// (via kernel API). This just validates the subscription.
    pub fn deliver_event(
        &self,
        cap: &InputSubscriptionCap,
        _event: &InputEvent,
    ) -> Result<bool, InputServiceError> {
        let subscription = self
            .subscriptions
            .get(&cap.id)
            .ok_or(InputServiceError::SubscriptionNotFound(cap.id))?;

        // Verify ownership
        if subscription.cap.task_id != cap.task_id {
            return Err(InputServiceError::InvalidCapability);
        }

        Ok(subscription.active)
    }

    /// Delivers an event via a sink (kernel message, queue, etc.)
    ///
    /// Returns Ok(true) if delivered, Ok(false) if subscription inactive.
    pub fn deliver_event_with<S: InputEventSink>(
        &self,
        cap: &InputSubscriptionCap,
        event: &InputEvent,
        sink: &mut S,
    ) -> Result<bool, InputServiceError> {
        let active = self.deliver_event(cap, event)?;
        if active {
            sink.send_event(cap, event)?;
        }
        Ok(active)
    }

    /// Checks if a subscription is active
    pub fn is_subscription_active(&self, cap: &InputSubscriptionCap) -> bool {
        self.subscriptions
            .get(&cap.id)
            .map(|s| s.active && s.cap.task_id == cap.task_id)
            .unwrap_or(false)
    }

    /// Returns the subscription for a task, if any
    pub fn get_task_subscription(&self, task_id: TaskId) -> Option<InputSubscriptionCap> {
        self.task_subscriptions
            .get(&task_id)
            .and_then(|sub_id| self.subscriptions.get(sub_id))
            .map(|sub| sub.cap)
    }

    /// Returns the number of active subscriptions
    pub fn active_subscription_count(&self) -> usize {
        self.subscriptions.values().filter(|s| s.active).count()
    }

    /// Returns the total number of subscriptions (active + inactive)
    pub fn total_subscription_count(&self) -> usize {
        self.subscriptions.len()
    }
}

impl Default for InputService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use input_types::{KeyCode, KeyEvent, Modifiers};

    #[test]
    fn test_input_service_creation() {
        let service = InputService::new();
        assert_eq!(service.active_subscription_count(), 0);
        assert_eq!(service.total_subscription_count(), 0);
    }

    #[test]
    fn test_subscribe_keyboard() {
        let mut service = InputService::new();
        let task_id = TaskId::new();
        let channel = ChannelId::new();

        let cap = service.subscribe_keyboard(task_id, channel).unwrap();

        assert_eq!(cap.task_id, task_id);
        assert_eq!(cap.channel, channel);
        assert_eq!(service.active_subscription_count(), 1);
        assert_eq!(service.total_subscription_count(), 1);
    }

    #[test]
    fn test_subscribe_duplicate_task_fails() {
        let mut service = InputService::new();
        let task_id = TaskId::new();
        let channel1 = ChannelId::new();
        let channel2 = ChannelId::new();

        service.subscribe_keyboard(task_id, channel1).unwrap();
        let result = service.subscribe_keyboard(task_id, channel2);

        assert_eq!(
            result,
            Err(InputServiceError::SubscriptionAlreadyExists(task_id))
        );
    }

    #[test]
    fn test_multiple_tasks_can_subscribe() {
        let mut service = InputService::new();
        let task1 = TaskId::new();
        let task2 = TaskId::new();
        let channel1 = ChannelId::new();
        let channel2 = ChannelId::new();

        let cap1 = service.subscribe_keyboard(task1, channel1).unwrap();
        let cap2 = service.subscribe_keyboard(task2, channel2).unwrap();

        assert_ne!(cap1.id, cap2.id);
        assert_eq!(service.active_subscription_count(), 2);
    }

    #[test]
    fn test_revoke_subscription() {
        let mut service = InputService::new();
        let task_id = TaskId::new();
        let channel = ChannelId::new();

        let cap = service.subscribe_keyboard(task_id, channel).unwrap();
        assert_eq!(service.active_subscription_count(), 1);

        service.revoke_subscription(&cap).unwrap();
        assert_eq!(service.active_subscription_count(), 0);
        assert_eq!(service.total_subscription_count(), 1); // Still exists, just inactive
    }

    #[test]
    fn test_revoke_nonexistent_subscription() {
        let mut service = InputService::new();
        let task_id = TaskId::new();
        let channel = ChannelId::new();
        let fake_cap = InputSubscriptionCap::new(999, task_id, channel);

        let result = service.revoke_subscription(&fake_cap);
        assert_eq!(result, Err(InputServiceError::SubscriptionNotFound(999)));
    }

    #[test]
    fn test_revoke_wrong_task() {
        let mut service = InputService::new();
        let task1 = TaskId::new();
        let task2 = TaskId::new();
        let channel = ChannelId::new();

        let cap = service.subscribe_keyboard(task1, channel).unwrap();
        let wrong_cap = InputSubscriptionCap::new(cap.id, task2, cap.channel);

        let result = service.revoke_subscription(&wrong_cap);
        assert_eq!(result, Err(InputServiceError::InvalidCapability));
    }

    #[test]
    fn test_unsubscribe() {
        let mut service = InputService::new();
        let task_id = TaskId::new();
        let channel = ChannelId::new();

        let cap = service.subscribe_keyboard(task_id, channel).unwrap();
        assert_eq!(service.total_subscription_count(), 1);

        service.unsubscribe(&cap).unwrap();
        assert_eq!(service.total_subscription_count(), 0);
        assert_eq!(service.active_subscription_count(), 0);
    }

    #[test]
    fn test_is_subscription_active() {
        let mut service = InputService::new();
        let task_id = TaskId::new();
        let channel = ChannelId::new();

        let cap = service.subscribe_keyboard(task_id, channel).unwrap();
        assert!(service.is_subscription_active(&cap));

        service.revoke_subscription(&cap).unwrap();
        assert!(!service.is_subscription_active(&cap));
    }

    #[test]
    fn test_get_task_subscription() {
        let mut service = InputService::new();
        let task_id = TaskId::new();
        let channel = ChannelId::new();

        assert_eq!(service.get_task_subscription(task_id), None);

        let cap = service.subscribe_keyboard(task_id, channel).unwrap();
        assert_eq!(service.get_task_subscription(task_id), Some(cap));
    }

    #[test]
    fn test_deliver_event_validation() {
        let mut service = InputService::new();
        let task_id = TaskId::new();
        let channel = ChannelId::new();

        let cap = service.subscribe_keyboard(task_id, channel).unwrap();
        let event = InputEvent::key(input_types::KeyEvent::pressed(
            input_types::KeyCode::A,
            input_types::Modifiers::none(),
        ));

        // Active subscription should allow delivery
        let result = service.deliver_event(&cap, &event).unwrap();
        assert!(result);

        // Revoked subscription should not allow delivery
        service.revoke_subscription(&cap).unwrap();
        let result = service.deliver_event(&cap, &event).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_deliver_event_nonexistent_subscription() {
        let service = InputService::new();
        let task_id = TaskId::new();
        let channel = ChannelId::new();
        let fake_cap = InputSubscriptionCap::new(999, task_id, channel);
        let event = InputEvent::key(input_types::KeyEvent::pressed(
            input_types::KeyCode::A,
            input_types::Modifiers::none(),
        ));

        let result = service.deliver_event(&fake_cap, &event);
        assert_eq!(result, Err(InputServiceError::SubscriptionNotFound(999)));
    }

    #[test]
    fn test_deliver_event_wrong_task() {
        let mut service = InputService::new();
        let task1 = TaskId::new();
        let task2 = TaskId::new();
        let channel = ChannelId::new();

        let cap = service.subscribe_keyboard(task1, channel).unwrap();
        let wrong_cap = InputSubscriptionCap::new(cap.id, task2, cap.channel);
        let event = InputEvent::key(input_types::KeyEvent::pressed(
            input_types::KeyCode::A,
            input_types::Modifiers::none(),
        ));

        let result = service.deliver_event(&wrong_cap, &event);
        assert_eq!(result, Err(InputServiceError::InvalidCapability));
    }

    #[test]
    fn test_subscription_lifecycle() {
        let mut service = InputService::new();
        let task_id = TaskId::new();
        let channel = ChannelId::new();

        // Subscribe
        let cap = service.subscribe_keyboard(task_id, channel).unwrap();
        assert!(service.is_subscription_active(&cap));
        assert_eq!(service.active_subscription_count(), 1);

        // Revoke (deactivate)
        service.revoke_subscription(&cap).unwrap();
        assert!(!service.is_subscription_active(&cap));
        assert_eq!(service.active_subscription_count(), 0);
        assert_eq!(service.total_subscription_count(), 1);

        // Unsubscribe (remove)
        service.unsubscribe(&cap).unwrap();
        assert_eq!(service.total_subscription_count(), 0);
    }

    #[test]
    fn test_subscription_cap_serialization() {
        let task_id = TaskId::new();
        let channel = ChannelId::new();
        let cap = InputSubscriptionCap::new(42, task_id, channel);

        let json = serde_json::to_string(&cap).unwrap();
        let deserialized: InputSubscriptionCap = serde_json::from_str(&json).unwrap();

        assert_eq!(cap, deserialized);
    }

    struct TestSink {
        delivered: usize,
    }

    impl TestSink {
        fn new() -> Self {
            Self { delivered: 0 }
        }
    }

    impl InputEventSink for TestSink {
        fn send_event(
            &mut self,
            _cap: &InputSubscriptionCap,
            _event: &InputEvent,
        ) -> Result<(), InputServiceError> {
            self.delivered += 1;
            Ok(())
        }
    }

    #[test]
    fn test_deliver_event_with_sink() {
        let mut service = InputService::new();
        let task_id = TaskId::new();
        let channel = ChannelId::new();
        let cap = service.subscribe_keyboard(task_id, channel).unwrap();
        let event = InputEvent::key(KeyEvent::pressed(KeyCode::A, Modifiers::none()));

        let mut sink = TestSink::new();
        let delivered = service.deliver_event_with(&cap, &event, &mut sink).unwrap();

        assert!(delivered);
        assert_eq!(sink.delivered, 1);
    }

    #[test]
    fn test_build_input_event_envelope() {
        let event = InputEvent::key(KeyEvent::pressed(KeyCode::B, Modifiers::none()));
        let envelope = build_input_event_envelope(&event, None).unwrap();

        assert_eq!(envelope.action, INPUT_EVENT_ACTION);
        assert_eq!(envelope.schema_version, INPUT_SCHEMA_VERSION);
        let decoded: InputEvent = envelope.payload.deserialize().unwrap();
        assert_eq!(decoded, event);
    }
}

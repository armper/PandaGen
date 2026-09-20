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
//! - **Unified delivery path**: Default polling now uses real kernel message delivery
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
//! // Create bridge with hardware keyboard + input subscription.
//! let keyboard = Box::new(X86Ps2Keyboard::new(io));
//! let mut bridge = InputHalBridge::new(
//!     execution_id,
//!     task_id,
//!     subscription_cap,
//!     keyboard,
//! );
//!
//! // Poll loop (run as a component/task)
//! loop {
//!     match bridge.poll(&input_service, &mut kernel) {
//!         Ok(PollResult::EventDelivered) => { /* continue */ }
//!         Ok(PollResult::NoEvent) => { /* sleep */ }
//!         Err(BridgeError::BudgetExhausted) => break,
//!         Err(e) => { /* handle error */ }
//!     }
//! }
//! ```

use core_types::TaskId;
use hal::{KeyboardDevice, KeyboardTranslator, PointerDevice, PointerTranslator};
use identity::ExecutionId;
use input_types::InputEvent;
use input_types::Modifiers;
use kernel_api::{KernelApiV0, KernelError};
use services_input::{
    build_input_event_envelope, InputEventSink, InputService, InputServiceError,
    InputSubscriptionCap,
};
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

/// Input sink that delivers events through a kernel API.
pub struct KernelInputSink<'a, K: KernelApiV0> {
    kernel: &'a mut K,
    source_task: Option<TaskId>,
}

impl<'a, K: KernelApiV0> KernelInputSink<'a, K> {
    pub fn new(kernel: &'a mut K, source_task: Option<TaskId>) -> Self {
        Self {
            kernel,
            source_task,
        }
    }
}

impl<'a, K: KernelApiV0> InputEventSink for KernelInputSink<'a, K> {
    fn send_event(
        &mut self,
        cap: &InputSubscriptionCap,
        event: &InputEvent,
    ) -> Result<(), InputServiceError> {
        let envelope = build_input_event_envelope(event, self.source_task)?;
        self.kernel.send(cap.channel, envelope).map_err(|err| {
            InputServiceError::DeliveryFailed {
                reason: err.to_string(),
            }
        })?;
        Ok(())
    }
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

    /// Task ID for message delivery (used for source attribution/budget checks)
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

    /// Polls for a keyboard event and delivers it through the kernel.
    ///
    /// Returns:
    /// - `Ok(PollResult::EventDelivered)` if an event was delivered
    /// - `Ok(PollResult::NoEvent)` if no hardware event was available
    /// - `Err(BridgeError)` on failure (budget exhaustion, policy denial, etc.)
    pub fn poll<K: KernelApiV0>(
        &mut self,
        input_service: &InputService,
        kernel: &mut K,
    ) -> Result<PollResult, BridgeError> {
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

        let active = input_service
            .deliver_event(&self.subscription, &input_event)
            .map_err(|err| BridgeError::InputServiceError(err.to_string()))?;

        if !active {
            return Ok(PollResult::NoEvent);
        }

        let envelope = build_input_event_envelope(&input_event, Some(self.task_id))
            .map_err(|err| BridgeError::InputServiceError(err.to_string()))?;
        kernel
            .send(self.subscription.channel, envelope)
            .map_err(Self::map_kernel_error)?;

        self.events_delivered += 1;

        Ok(PollResult::EventDelivered)
    }

    /// Polls for a keyboard event and delivers it through an arbitrary sink.
    ///
    /// This is useful for tests and alternate transports. Most runtime code should
    /// prefer [`Self::poll`] so delivery goes through the kernel API path.
    pub fn poll_with_sink<S: InputEventSink>(
        &mut self,
        input_service: &InputService,
        sink: &mut S,
    ) -> Result<PollResult, BridgeError> {
        let hal_event = match self.keyboard.poll_event() {
            Some(event) => event,
            None => return Ok(PollResult::NoEvent),
        };

        let key_event = match self.translator.translate(hal_event) {
            Some(event) => event,
            None => return Ok(PollResult::NoEvent),
        };

        let input_event = InputEvent::key(key_event);
        let delivered = input_service
            .deliver_event_with(&self.subscription, &input_event, sink)
            .map_err(|err| BridgeError::InputServiceError(err.to_string()))?;

        if delivered {
            self.events_delivered += 1;
            Ok(PollResult::EventDelivered)
        } else {
            Ok(PollResult::NoEvent)
        }
    }

    fn map_kernel_error(err: KernelError) -> BridgeError {
        match err {
            KernelError::ResourceBudgetExceeded { .. }
            | KernelError::ResourceBudgetExhausted { .. } => BridgeError::BudgetExhausted {
                resource: "MessageCount".to_string(),
            },
            KernelError::InsufficientAuthority(reason) => BridgeError::PolicyDenied { reason },
            KernelError::SendFailed(reason)
            | KernelError::ChannelError(reason)
            | KernelError::ReceiveFailed(reason) => BridgeError::ChannelError(reason),
            other => BridgeError::ChannelError(other.to_string()),
        }
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

/// Pointer HAL bridge (GFX-022)
///
/// The pointer counterpart of [`InputHalBridge`]:
/// - owns a `PointerDevice` (hardware abstraction)
/// - owns a `PointerTranslator` (relative packets → absolute typed events)
/// - delivers `InputEvent::Pointer` through the same subscription contract
///
/// One hardware packet may expand to several events (a move, button edges,
/// a wheel notch); each is delivered separately so consumers see one fact
/// per message.
pub struct PointerHalBridge {
    execution_id: ExecutionId,
    task_id: TaskId,
    subscription: InputSubscriptionCap,
    pointer: Box<dyn PointerDevice>,
    translator: PointerTranslator,
    /// Keyboard modifiers stamped onto pointer events (fed by the keyboard path).
    modifiers: Modifiers,
    events_delivered: u64,
    packets_seen: u64,
}

impl PointerHalBridge {
    /// `surface_width` x `surface_height` confines the pointer position.
    pub fn new(
        execution_id: ExecutionId,
        task_id: TaskId,
        subscription: InputSubscriptionCap,
        pointer: Box<dyn PointerDevice>,
        surface_width: u32,
        surface_height: u32,
    ) -> Self {
        Self {
            execution_id,
            task_id,
            subscription,
            pointer,
            translator: PointerTranslator::new(surface_width, surface_height),
            modifiers: Modifiers::NONE,
            events_delivered: 0,
            packets_seen: 0,
        }
    }

    /// Update the modifiers applied to subsequent pointer events.
    pub fn set_modifiers(&mut self, modifiers: Modifiers) {
        self.modifiers = modifiers;
    }

    /// Change the confinement surface (display mode or resolution change).
    pub fn resize_surface(&mut self, width: u32, height: u32) {
        self.translator.resize_surface(width, height);
    }

    pub fn translator(&self) -> &PointerTranslator {
        &self.translator
    }

    /// Drain one packet and deliver its events through the kernel.
    pub fn poll<K: KernelApiV0>(
        &mut self,
        input_service: &InputService,
        kernel: &mut K,
    ) -> Result<PollResult, BridgeError> {
        let Some(packet) = self.pointer.poll_packet() else {
            return Ok(PollResult::NoEvent);
        };
        self.packets_seen += 1;
        let batch = self.translator.translate(packet, self.modifiers);

        // Every event in the batch is attempted, whatever happens to one of
        // them.
        //
        // `poll_packet` has already consumed the packet and `translate` has
        // already advanced the translator's own button state, so a `?` here
        // dropped the rest of the batch for ever -- and this crate's own doc
        // says each event "is delivered separately so consumers see one fact
        // per message". A press whose release is lost is a stuck drag that
        // nothing will retry, because the bridge's state already says the
        // button is up; a release whose press is lost is a consumer told a
        // button came up that it never saw go down. A refusal that is also a
        // loss, and the button state makes it unrecoverable.
        let mut delivered_any = false;
        let mut failure = None;
        for event in batch.iter() {
            let input_event = InputEvent::pointer(*event);
            let active = match input_service.deliver_event(&self.subscription, &input_event) {
                Ok(active) => active,
                Err(err) => {
                    failure.get_or_insert(BridgeError::InputServiceError(err.to_string()));
                    continue;
                }
            };
            if !active {
                continue;
            }
            let envelope = match build_input_event_envelope(&input_event, Some(self.task_id)) {
                Ok(envelope) => envelope,
                Err(err) => {
                    failure.get_or_insert(BridgeError::InputServiceError(err.to_string()));
                    continue;
                }
            };
            if let Err(err) = kernel.send(self.subscription.channel, envelope) {
                failure.get_or_insert(InputHalBridge::map_kernel_error(err));
                continue;
            }
            self.events_delivered += 1;
            delivered_any = true;
        }

        if let Some(err) = failure {
            return Err(err);
        }

        Ok(if delivered_any {
            PollResult::EventDelivered
        } else {
            PollResult::NoEvent
        })
    }

    /// Drain one packet and deliver its events through an arbitrary sink.
    pub fn poll_with_sink<S: InputEventSink>(
        &mut self,
        input_service: &InputService,
        sink: &mut S,
    ) -> Result<PollResult, BridgeError> {
        let Some(packet) = self.pointer.poll_packet() else {
            return Ok(PollResult::NoEvent);
        };
        self.packets_seen += 1;
        let batch = self.translator.translate(packet, self.modifiers);

        // As in `poll`: the whole batch is attempted, and the first failure
        // is reported after it.
        let mut delivered_any = false;
        let mut failure = None;
        for event in batch.iter() {
            let input_event = InputEvent::pointer(*event);
            match input_service.deliver_event_with(&self.subscription, &input_event, sink) {
                Ok(true) => {
                    self.events_delivered += 1;
                    delivered_any = true;
                }
                Ok(false) => {}
                Err(err) => {
                    failure.get_or_insert(BridgeError::InputServiceError(err.to_string()));
                }
            }
        }

        if let Some(err) = failure {
            return Err(err);
        }

        Ok(if delivered_any {
            PollResult::EventDelivered
        } else {
            PollResult::NoEvent
        })
    }

    pub fn execution_id(&self) -> ExecutionId {
        self.execution_id
    }

    pub fn subscription(&self) -> &InputSubscriptionCap {
        &self.subscription
    }

    pub fn events_delivered(&self) -> u64 {
        self.events_delivered
    }

    pub fn packets_seen(&self) -> u64 {
        self.packets_seen
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_types::TaskId;
    use hal::HalKeyEvent;
    use ipc::ChannelId;
    use services_input::InputService;

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

    fn setup_bridge(
        events: Vec<HalKeyEvent>,
    ) -> (
        InputHalBridge,
        InputService,
        sim_kernel::SimulatedKernel,
        ChannelId,
    ) {
        let exec_id = ExecutionId::new();
        let task_id = TaskId::new();
        let mut kernel = sim_kernel::SimulatedKernel::new();
        let channel = kernel_api::KernelApiV0::create_channel(&mut kernel).unwrap();
        let mut input_service = InputService::new();
        let subscription = input_service.subscribe_keyboard(task_id, channel).unwrap();
        let keyboard = Box::new(FakeKeyboard::new(events));
        let bridge = InputHalBridge::new(exec_id, task_id, subscription, keyboard);
        (bridge, input_service, kernel, channel)
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
        let (mut bridge, input_service, mut kernel, _) = setup_bridge(vec![]);
        let result = bridge.poll(&input_service, &mut kernel).unwrap();

        assert_eq!(result, PollResult::NoEvent);
        assert_eq!(bridge.events_delivered(), 0);
    }

    #[test]
    fn test_bridge_poll_with_event() {
        let (mut bridge, input_service, mut kernel, channel) =
            setup_bridge(vec![HalKeyEvent::new(0x1E, true)]);
        let result = bridge.poll(&input_service, &mut kernel).unwrap();

        assert_eq!(result, PollResult::EventDelivered);
        assert_eq!(bridge.events_delivered(), 1);

        let envelope = kernel_api::KernelApiV0::recv(&mut kernel, channel).unwrap();
        assert_eq!(envelope.action, services_input::INPUT_EVENT_ACTION);
    }

    #[test]
    fn test_bridge_poll_multiple_events() {
        let events = vec![
            HalKeyEvent::new(0x1E, true),
            HalKeyEvent::new(0x1E, false),
            HalKeyEvent::new(0x30, true),
        ];
        let (mut bridge, input_service, mut kernel, _) = setup_bridge(events);

        assert_eq!(
            bridge.poll(&input_service, &mut kernel).unwrap(),
            PollResult::EventDelivered
        );
        assert_eq!(
            bridge.poll(&input_service, &mut kernel).unwrap(),
            PollResult::EventDelivered
        );
        assert_eq!(
            bridge.poll(&input_service, &mut kernel).unwrap(),
            PollResult::EventDelivered
        );
        assert_eq!(
            bridge.poll(&input_service, &mut kernel).unwrap(),
            PollResult::NoEvent
        );

        assert_eq!(bridge.events_delivered(), 3);
    }

    #[test]
    fn test_bridge_poll_unknown_key() {
        let (mut bridge, input_service, mut kernel, _) =
            setup_bridge(vec![HalKeyEvent::new(0xFF, true)]);
        let result = bridge.poll(&input_service, &mut kernel).unwrap();

        // Unknown keys are skipped
        assert_eq!(result, PollResult::NoEvent);
        assert_eq!(bridge.events_delivered(), 0);
    }

    #[test]
    fn test_bridge_reset_translator() {
        let (mut bridge, input_service, mut kernel, _) =
            setup_bridge(vec![HalKeyEvent::new(0x1D, true)]);
        bridge.poll(&input_service, &mut kernel).unwrap();

        // Reset translator (clears modifier state)
        bridge.reset_translator();

        // After reset, translator should have no modifiers
        // (can't directly test internal state, but this exercises the code)
    }

    #[test]
    fn test_bridge_arrow_keys_e0() {
        use hal::HalScancode;

        // Arrow keys with E0 prefix
        let events = vec![
            HalKeyEvent::with_scancode(HalScancode::e0(0x48), true), // Up pressed
            HalKeyEvent::with_scancode(HalScancode::e0(0x48), false), // Up released
            HalKeyEvent::with_scancode(HalScancode::e0(0x50), true), // Down pressed
            HalKeyEvent::with_scancode(HalScancode::e0(0x4B), true), // Left pressed
            HalKeyEvent::with_scancode(HalScancode::e0(0x4D), true), // Right pressed
        ];
        let (mut bridge, input_service, mut kernel, _) = setup_bridge(events);

        // Verify we can poll events (we can't inspect the KeyEvent directly
        // without more infrastructure, but we can verify they're delivered)
        assert_eq!(
            bridge.poll(&input_service, &mut kernel).unwrap(),
            PollResult::EventDelivered
        );
        assert_eq!(
            bridge.poll(&input_service, &mut kernel).unwrap(),
            PollResult::EventDelivered
        );
        assert_eq!(
            bridge.poll(&input_service, &mut kernel).unwrap(),
            PollResult::EventDelivered
        );
        assert_eq!(
            bridge.poll(&input_service, &mut kernel).unwrap(),
            PollResult::EventDelivered
        );
        assert_eq!(
            bridge.poll(&input_service, &mut kernel).unwrap(),
            PollResult::EventDelivered
        );
        assert_eq!(
            bridge.poll(&input_service, &mut kernel).unwrap(),
            PollResult::NoEvent
        );

        assert_eq!(bridge.events_delivered(), 5);
    }

    #[test]
    fn test_bridge_navigation_cluster_e0() {
        use hal::HalScancode;

        // Navigation cluster with E0 prefix
        let events = vec![
            HalKeyEvent::with_scancode(HalScancode::e0(0x47), true), // Home
            HalKeyEvent::with_scancode(HalScancode::e0(0x4F), true), // End
            HalKeyEvent::with_scancode(HalScancode::e0(0x49), true), // PageUp
            HalKeyEvent::with_scancode(HalScancode::e0(0x51), true), // PageDown
            HalKeyEvent::with_scancode(HalScancode::e0(0x52), true), // Insert
            HalKeyEvent::with_scancode(HalScancode::e0(0x53), true), // Delete
        ];
        let (mut bridge, input_service, mut kernel, _) = setup_bridge(events);

        for _ in 0..6 {
            assert_eq!(
                bridge.poll(&input_service, &mut kernel).unwrap(),
                PollResult::EventDelivered
            );
        }
        assert_eq!(
            bridge.poll(&input_service, &mut kernel).unwrap(),
            PollResult::NoEvent
        );
        assert_eq!(bridge.events_delivered(), 6);
    }

    #[test]
    fn test_bridge_poll_inactive_subscription_returns_no_event() {
        let (mut bridge, mut input_service, mut kernel, _) =
            setup_bridge(vec![HalKeyEvent::new(0x1E, true)]);
        let subscription = *bridge.subscription();
        input_service.revoke_subscription(&subscription).unwrap();

        let result = bridge.poll(&input_service, &mut kernel).unwrap();
        assert_eq!(result, PollResult::NoEvent);
        assert_eq!(bridge.events_delivered(), 0);
    }

    #[test]
    fn test_bridge_poll_maps_channel_error() {
        let exec_id = ExecutionId::new();
        let task_id = TaskId::new();
        let mut kernel = sim_kernel::SimulatedKernel::new();
        let invalid_channel = ChannelId::new(); // not created in kernel
        let mut input_service = InputService::new();
        let subscription = input_service
            .subscribe_keyboard(task_id, invalid_channel)
            .unwrap();
        let keyboard = Box::new(FakeKeyboard::new(vec![HalKeyEvent::new(0x1E, true)]));
        let mut bridge = InputHalBridge::new(exec_id, task_id, subscription, keyboard);

        let err = bridge.poll(&input_service, &mut kernel).unwrap_err();
        match err {
            BridgeError::ChannelError(_) => {}
            other => panic!("Expected ChannelError, got {:?}", other),
        }
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
    fn test_bridge_poll_with_sink() {
        let exec_id = ExecutionId::new();
        let task_id = TaskId::new();
        let channel = ChannelId::new();
        let mut input_service = InputService::new();
        let subscription = input_service.subscribe_keyboard(task_id, channel).unwrap();

        let events = vec![HalKeyEvent::new(0x1E, true)];
        let keyboard = Box::new(FakeKeyboard::new(events));
        let mut bridge = InputHalBridge::new(exec_id, task_id, subscription, keyboard);

        let mut sink = TestSink::new();
        let result = bridge.poll_with_sink(&input_service, &mut sink).unwrap();

        assert_eq!(result, PollResult::EventDelivered);
        assert_eq!(sink.delivered, 1);
    }

    #[test]
    fn test_bridge_kernel_sink_delivers_message() {
        let exec_id = ExecutionId::new();
        let task_id = TaskId::new();
        let mut kernel = sim_kernel::SimulatedKernel::new();
        let channel = kernel_api::KernelApiV0::create_channel(&mut kernel).unwrap();

        let mut input_service = InputService::new();
        let subscription = input_service.subscribe_keyboard(task_id, channel).unwrap();

        let events = vec![HalKeyEvent::new(0x1E, true)];
        let keyboard = Box::new(FakeKeyboard::new(events));
        let mut bridge = InputHalBridge::new(exec_id, task_id, subscription, keyboard);

        let mut sink = KernelInputSink::new(&mut kernel, None);
        let result = bridge.poll_with_sink(&input_service, &mut sink).unwrap();
        assert_eq!(result, PollResult::EventDelivered);

        let envelope = kernel_api::KernelApi::receive_message(&mut kernel, channel, None).unwrap();
        assert_eq!(envelope.action, services_input::INPUT_EVENT_ACTION);
    }

    struct FakePointer {
        packets: Vec<hal::HalPointerPacket>,
        index: usize,
    }

    impl PointerDevice for FakePointer {
        fn poll_packet(&mut self) -> Option<hal::HalPointerPacket> {
            let packet = self.packets.get(self.index).copied();
            self.index += 1;
            packet
        }
    }

    struct RecordingSink {
        events: Vec<InputEvent>,
    }

    impl InputEventSink for RecordingSink {
        fn send_event(
            &mut self,
            _cap: &InputSubscriptionCap,
            event: &InputEvent,
        ) -> Result<(), InputServiceError> {
            self.events.push(event.clone());
            Ok(())
        }
    }

    /// Refuses one event and accepts the rest.
    struct SinkThatRefusesOnce {
        refuse_at: usize,
        seen: usize,
        delivered: Vec<InputEvent>,
    }

    impl services_input::InputEventSink for SinkThatRefusesOnce {
        fn send_event(
            &mut self,
            _cap: &services_input::InputSubscriptionCap,
            event: &InputEvent,
        ) -> Result<(), services_input::InputServiceError> {
            let index = self.seen;
            self.seen += 1;
            if index == self.refuse_at {
                return Err(services_input::InputServiceError::InvalidCapability);
            }
            self.delivered.push(event.clone());
            Ok(())
        }
    }

    /// The finding: `poll_packet` has already consumed the packet and
    /// `translate` has already advanced the translator's button state, so a
    /// `?` mid-batch dropped every later event for ever. The consumer was
    /// left holding a release for a button it never saw pressed -- or a
    /// press whose release never came, which is a stuck drag that nothing
    /// retries, because the bridge's own state says the button is up.
    ///
    /// The crate's doc promises each event "is delivered separately so
    /// consumers see one fact per message".
    #[test]
    fn one_refused_event_does_not_drop_the_rest_of_the_batch() {
        // A packet that both moves and presses, so the batch has two events.
        let (mut bridge, input_service) =
            pointer_bridge(vec![hal::HalPointerPacket::new(5, 5, 1, 0)]);
        let mut sink = SinkThatRefusesOnce {
            refuse_at: 0,
            seen: 0,
            delivered: Vec::new(),
        };

        let outcome = bridge.poll_with_sink(&input_service, &mut sink);
        assert!(outcome.is_err(), "sanity: the sink refused an event");
        assert!(
            sink.seen >= 2,
            "the batch stopped at the first refusal: the sink saw {} of at \
             least 2 events",
            sink.seen
        );
    }

    fn pointer_bridge(packets: Vec<hal::HalPointerPacket>) -> (PointerHalBridge, InputService) {
        let task_id = TaskId::new();
        let mut input_service = InputService::new();
        let subscription = input_service
            .subscribe_keyboard(task_id, ChannelId::new())
            .unwrap();
        let bridge = PointerHalBridge::new(
            ExecutionId::new(),
            task_id,
            subscription,
            Box::new(FakePointer { packets, index: 0 }),
            640,
            480,
        );
        (bridge, input_service)
    }

    #[test]
    fn test_pointer_bridge_expands_packet_into_typed_events() {
        use input_types::{PointerButton, PointerEventKind, PointerPosition};

        let (mut bridge, input_service) = pointer_bridge(vec![
            hal::HalPointerPacket::new(10, -4, 0, hal::HalPointerPacket::BUTTON_PRIMARY),
            hal::HalPointerPacket::new(0, 0, 0, 0),
        ]);
        bridge.set_modifiers(Modifiers::SHIFT);
        let mut sink = RecordingSink { events: vec![] };

        assert_eq!(
            bridge.poll_with_sink(&input_service, &mut sink).unwrap(),
            PollResult::EventDelivered
        );
        assert_eq!(sink.events.len(), 2, "move then press");
        let moved = sink.events[0].as_pointer().unwrap();
        assert!(moved.is_move());
        assert_eq!(moved.position, PointerPosition::new(330, 244));
        assert!(moved.modifiers.is_shift());
        let pressed = sink.events[1].as_pointer().unwrap();
        assert!(pressed.is_press(PointerButton::Primary));
        assert_eq!(pressed.position, PointerPosition::new(330, 244));

        // Second packet releases the button at the same place.
        assert_eq!(
            bridge.poll_with_sink(&input_service, &mut sink).unwrap(),
            PollResult::EventDelivered
        );
        assert_eq!(sink.events.len(), 3);
        assert!(matches!(
            sink.events[2].as_pointer().unwrap().kind,
            PointerEventKind::Button { .. }
        ));

        // Device drained.
        assert_eq!(
            bridge.poll_with_sink(&input_service, &mut sink).unwrap(),
            PollResult::NoEvent
        );
        assert_eq!(bridge.events_delivered(), 3);
        assert_eq!(bridge.packets_seen(), 2);
    }

    #[test]
    fn test_pointer_bridge_revoked_subscription_delivers_nothing() {
        let (mut bridge, mut input_service) =
            pointer_bridge(vec![hal::HalPointerPacket::motion(1, 1)]);
        let cap = *bridge.subscription();
        input_service.revoke_subscription(&cap).unwrap();
        let mut sink = RecordingSink { events: vec![] };
        assert_eq!(
            bridge.poll_with_sink(&input_service, &mut sink).unwrap(),
            PollResult::NoEvent
        );
        assert!(sink.events.is_empty());
        assert_eq!(bridge.packets_seen(), 1);
    }

    #[test]
    fn test_pointer_bridge_delivers_through_kernel_channel() {
        let task_id = TaskId::new();
        let mut kernel = sim_kernel::SimulatedKernel::new();
        let channel = kernel_api::KernelApiV0::create_channel(&mut kernel).unwrap();
        let mut input_service = InputService::new();
        let subscription = input_service.subscribe_keyboard(task_id, channel).unwrap();
        let mut bridge = PointerHalBridge::new(
            ExecutionId::new(),
            task_id,
            subscription,
            Box::new(FakePointer {
                packets: vec![hal::HalPointerPacket::new(0, 0, 1, 0)],
                index: 0,
            }),
            100,
            100,
        );

        assert_eq!(
            bridge.poll(&input_service, &mut kernel).unwrap(),
            PollResult::EventDelivered
        );
        let envelope = kernel_api::KernelApi::receive_message(&mut kernel, channel, None).unwrap();
        assert_eq!(envelope.action, services_input::INPUT_EVENT_ACTION);
        assert_eq!(bridge.events_delivered(), 1);
    }

    #[test]
    fn test_pointer_bridge_resize_keeps_pointer_inside() {
        let (mut bridge, _service) = pointer_bridge(vec![]);
        bridge.resize_surface(10, 10);
        let position = bridge.translator().position();
        assert!(position.x < 10 && position.y < 10);
    }
}

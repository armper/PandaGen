//! Phase 59: Keyboard to Real Input Pipeline Integration Test
//!
//! This test demonstrates the end-to-end keyboard input pipeline:
//! PS/2 scancode → HalKeyEvent → KeyEvent → InputService → FocusManager → Task
//!
//! ## Pipeline Flow
//!
//! 1. **Hardware**: PS/2 keyboard generates scan codes (IRQ 1)
//! 2. **HAL Translation**: Scancode → HalKeyEvent (hal_x86_64)
//! 3. **Input Translation**: HalKeyEvent → KeyEvent (hal::keyboard_translation)
//! 4. **Service Delivery**: KeyEvent → InputService subscription
//! 5. **Focus Routing**: FocusManager routes to focused task
//! 6. **Task Consumption**: Task processes KeyEvent
//!
//! ## Philosophy
//!
//! - **No direct kernel access**: Input goes through proper service boundaries
//! - **Capability-based**: Tasks must subscribe to receive input
//! - **Focus-driven**: Only focused task receives events
//! - **Testable**: Entire pipeline works under `cargo test`

use core_types::TaskId;
use hal::keyboard::{HalKeyEvent, HalScancode};
use hal::keyboard_translation::KeyboardTranslator;
use input_types::{InputEvent, KeyCode, KeyEvent, KeyState, Modifiers};
use ipc::ChannelId;
use services_focus_manager::FocusManager;
use services_input::InputService;

/// Simulates the complete keyboard input pipeline
#[test]
fn test_keyboard_input_pipeline_end_to_end() {
    // Setup: Create input service and focus manager
    let mut input_service = InputService::new();
    let mut focus_manager = FocusManager::new();

    // Setup: Create two tasks (editor and terminal)
    let editor_task = TaskId::new();
    let terminal_task = TaskId::new();

    // Step 1: Tasks subscribe to input
    let editor_channel = ChannelId::new();
    let terminal_channel = ChannelId::new();

    let editor_sub = input_service
        .subscribe_keyboard(editor_task, editor_channel)
        .expect("Editor should subscribe");
    let terminal_sub = input_service
        .subscribe_keyboard(terminal_task, terminal_channel)
        .expect("Terminal should subscribe");

    // Step 2: Editor requests focus
    focus_manager
        .request_focus(editor_sub)
        .expect("Editor should get focus");

    // Step 3: Simulate hardware: PS/2 keyboard generates scancodes
    // User types "A" (press and release)
    let scancodes = vec![
        0x1E,        // A pressed (make code)
        0x1E | 0x80, // A released (break code)
    ];

    // Step 4: HAL layer translates scancodes to HalKeyEvents
    let hal_events: Vec<HalKeyEvent> = scancodes
        .into_iter()
        .map(|sc| {
            let pressed = (sc & 0x80) == 0;
            let code = sc & !0x80;
            HalKeyEvent::with_scancode(HalScancode::Base(code), pressed)
        })
        .collect();

    // Step 5: Translation layer converts HalKeyEvents to KeyEvents
    let mut translator = KeyboardTranslator::new();
    let key_events: Vec<KeyEvent> = hal_events
        .into_iter()
        .filter_map(|hal_ev| translator.translate(hal_ev))
        .collect();

    // Verify translation worked
    assert_eq!(key_events.len(), 2);
    assert_eq!(key_events[0].code, KeyCode::A);
    assert_eq!(key_events[0].state, KeyState::Pressed);
    assert!(key_events[0].modifiers.is_empty());
    assert_eq!(key_events[1].state, KeyState::Released);

    // Step 6: Route events through focus manager
    for key_event in &key_events {
        let input_event = InputEvent::key(key_event.clone());

        // Focus manager determines which task should receive the event
        let target_cap = focus_manager
            .route_event(&input_event)
            .expect("Should route successfully")
            .expect("Should have a focused task");

        // Verify editor got focus, not terminal
        assert_eq!(target_cap.id, editor_sub.id);
        assert_ne!(target_cap.id, terminal_sub.id);

        // Step 7: Input service validates subscription and delivers event
        let delivered = input_service
            .deliver_event(&target_cap, &input_event)
            .expect("Delivery should succeed");

        assert!(
            delivered,
            "Event should be delivered to active subscription"
        );
    }

    // Verify only the editor subscription is active in routing
    assert!(focus_manager.has_focus(&editor_sub));
    assert!(!focus_manager.has_focus(&terminal_sub));
}

/// Tests keyboard input with modifier keys (Shift, Ctrl, Alt)
#[test]
fn test_keyboard_pipeline_with_modifiers() {
    let mut input_service = InputService::new();
    let mut focus_manager = FocusManager::new();
    let mut translator = KeyboardTranslator::new();

    // Setup task and subscription
    let task = TaskId::new();
    let channel = ChannelId::new();
    let sub = input_service.subscribe_keyboard(task, channel).unwrap();
    focus_manager.request_focus(sub).unwrap();

    // Simulate: User presses Ctrl+C
    let scancodes = vec![
        0x1D,        // Left Ctrl pressed
        0x2E,        // C pressed
        0x2E | 0x80, // C released
        0x1D | 0x80, // Left Ctrl released
    ];

    // Translate through pipeline
    let hal_events: Vec<HalKeyEvent> = scancodes
        .into_iter()
        .map(|sc| {
            let pressed = (sc & 0x80) == 0;
            let code = sc & !0x80;
            HalKeyEvent::with_scancode(HalScancode::Base(code), pressed)
        })
        .collect();

    let key_events: Vec<KeyEvent> = hal_events
        .into_iter()
        .filter_map(|hal_ev| translator.translate(hal_ev))
        .collect();

    // Verify modifier state
    assert_eq!(key_events.len(), 4);

    // First event: Ctrl pressed (note: modifier includes itself when pressed)
    assert_eq!(key_events[0].code, KeyCode::LeftCtrl);
    assert!(key_events[0].is_pressed());
    assert!(key_events[0].modifiers.is_ctrl()); // Ctrl key press has Ctrl modifier

    // Second event: C pressed with Ctrl modifier
    assert_eq!(key_events[1].code, KeyCode::C);
    assert!(key_events[1].is_pressed());
    assert!(key_events[1].modifiers.is_ctrl());

    // Third event: C released with Ctrl still held
    assert_eq!(key_events[2].code, KeyCode::C);
    assert!(key_events[2].is_released());
    assert!(key_events[2].modifiers.is_ctrl());

    // Fourth event: Ctrl released
    assert_eq!(key_events[3].code, KeyCode::LeftCtrl);
    assert!(key_events[3].is_released());
    assert!(!key_events[3].modifiers.is_ctrl()); // Modifiers reflect state after release
}

/// Tests E0-prefixed extended keys (arrows, nav cluster)
#[test]
fn test_keyboard_pipeline_extended_keys() {
    let mut translator = KeyboardTranslator::new();

    // Simulate: User presses arrow keys
    // Arrow keys are E0-prefixed in PS/2
    let hal_events = vec![
        HalKeyEvent::with_scancode(HalScancode::e0(0x48), true), // Up
        HalKeyEvent::with_scancode(HalScancode::e0(0x50), true), // Down
        HalKeyEvent::with_scancode(HalScancode::e0(0x4B), true), // Left
        HalKeyEvent::with_scancode(HalScancode::e0(0x4D), true), // Right
    ];

    let key_events: Vec<KeyEvent> = hal_events
        .into_iter()
        .filter_map(|hal_ev| translator.translate(hal_ev))
        .collect();

    assert_eq!(key_events.len(), 4);
    assert_eq!(key_events[0].code, KeyCode::Up);
    assert_eq!(key_events[1].code, KeyCode::Down);
    assert_eq!(key_events[2].code, KeyCode::Left);
    assert_eq!(key_events[3].code, KeyCode::Right);
}

/// Tests focus switching between tasks
#[test]
fn test_keyboard_pipeline_focus_switching() {
    let mut input_service = InputService::new();
    let mut focus_manager = FocusManager::new();
    let mut translator = KeyboardTranslator::new();

    // Setup two tasks
    let task_a = TaskId::new();
    let task_b = TaskId::new();

    let sub_a = input_service
        .subscribe_keyboard(task_a, ChannelId::new())
        .unwrap();
    let sub_b = input_service
        .subscribe_keyboard(task_b, ChannelId::new())
        .unwrap();

    // Initially, task A has focus
    focus_manager.request_focus(sub_a).unwrap();

    // Simulate key press
    let hal_event = HalKeyEvent::with_scancode(HalScancode::base(0x1E), true); // A pressed
    let key_event = translator.translate(hal_event).unwrap();
    let input_event = InputEvent::key(key_event.clone());

    // Event should route to task A
    let target = focus_manager.route_event(&input_event).unwrap().unwrap();
    assert_eq!(target.id, sub_a.id);

    // Switch focus to task B
    focus_manager.request_focus(sub_b).unwrap();

    // Simulate another key press
    let hal_event = HalKeyEvent::with_scancode(HalScancode::base(0x30), true); // B pressed
    let key_event = translator.translate(hal_event).unwrap();
    let input_event = InputEvent::key(key_event);

    // Event should now route to task B
    let target = focus_manager.route_event(&input_event).unwrap().unwrap();
    assert_eq!(target.id, sub_b.id);
}

/// Tests that unknown scancodes are filtered out
#[test]
fn test_keyboard_pipeline_unknown_keys() {
    let mut translator = KeyboardTranslator::new();

    // Simulate unknown scancode
    let hal_event = HalKeyEvent::with_scancode(HalScancode::base(0xFF), true);
    let result = translator.translate(hal_event);

    // Should be filtered out
    assert!(result.is_none(), "Unknown keys should not produce events");
}

/// Tests complete typing sequence
#[test]
fn test_keyboard_pipeline_typing_sequence() {
    let mut translator = KeyboardTranslator::new();

    // Simulate typing "hi" (lowercase)
    let scancodes = vec![
        0x23,        // H pressed
        0x23 | 0x80, // H released
        0x17,        // I pressed
        0x17 | 0x80, // I released
    ];

    let hal_events: Vec<HalKeyEvent> = scancodes
        .into_iter()
        .map(|sc| {
            let pressed = (sc & 0x80) == 0;
            let code = sc & !0x80;
            HalKeyEvent::with_scancode(HalScancode::Base(code), pressed)
        })
        .collect();

    let key_events: Vec<KeyEvent> = hal_events
        .into_iter()
        .filter_map(|hal_ev| translator.translate(hal_ev))
        .collect();

    assert_eq!(key_events.len(), 4);
    assert_eq!(key_events[0].code, KeyCode::H);
    assert!(key_events[0].is_pressed());
    assert_eq!(key_events[1].code, KeyCode::H);
    assert!(key_events[1].is_released());
    assert_eq!(key_events[2].code, KeyCode::I);
    assert!(key_events[2].is_pressed());
    assert_eq!(key_events[3].code, KeyCode::I);
    assert!(key_events[3].is_released());
}

/// Tests Shift+letter produces correct modifier state
#[test]
fn test_keyboard_pipeline_shift_letter() {
    let mut translator = KeyboardTranslator::new();

    // Simulate typing "A" (uppercase with Shift)
    let scancodes = vec![
        0x2A,        // Left Shift pressed
        0x1E,        // A pressed
        0x1E | 0x80, // A released
        0x2A | 0x80, // Left Shift released
    ];

    let hal_events: Vec<HalKeyEvent> = scancodes
        .into_iter()
        .map(|sc| {
            let pressed = (sc & 0x80) == 0;
            let code = sc & !0x80;
            HalKeyEvent::with_scancode(HalScancode::Base(code), pressed)
        })
        .collect();

    let key_events: Vec<KeyEvent> = hal_events
        .into_iter()
        .filter_map(|hal_ev| translator.translate(hal_ev))
        .collect();

    assert_eq!(key_events.len(), 4);

    // Shift press
    assert_eq!(key_events[0].code, KeyCode::LeftShift);

    // A press with Shift modifier
    assert_eq!(key_events[1].code, KeyCode::A);
    assert!(key_events[1].is_pressed());
    assert!(key_events[1].modifiers.is_shift());
}

/// Tests that subscription lifecycle integrates with focus
#[test]
fn test_keyboard_pipeline_subscription_revocation() {
    let mut input_service = InputService::new();
    let mut focus_manager = FocusManager::new();

    let task = TaskId::new();
    let channel = ChannelId::new();
    let sub = input_service.subscribe_keyboard(task, channel).unwrap();

    focus_manager.request_focus(sub).unwrap();

    // Revoke subscription
    input_service.revoke_subscription(&sub).unwrap();

    // Try to deliver event
    let event = InputEvent::key(KeyEvent::pressed(KeyCode::A, Modifiers::none()));
    let delivered = input_service.deliver_event(&sub, &event).unwrap();

    // Should not be delivered (subscription inactive)
    assert!(!delivered);

    // Focus manager still thinks it's focused, but input service won't deliver
    // In a real system, focus manager would be notified of revocation
    assert!(focus_manager.has_focus(&sub));
}

/// Demonstrates multi-stage pipeline with error handling
#[test]
fn test_keyboard_pipeline_error_handling() {
    let mut input_service = InputService::new();
    let task = TaskId::new();
    let channel = ChannelId::new();

    // Subscribe
    let sub = input_service.subscribe_keyboard(task, channel).unwrap();

    // Attempt duplicate subscription (should fail)
    let result = input_service.subscribe_keyboard(task, ChannelId::new());
    assert!(result.is_err());

    // Unsubscribe
    input_service.unsubscribe(&sub).unwrap();

    // Try to deliver to unsubscribed task (should fail)
    let event = InputEvent::key(KeyEvent::pressed(KeyCode::A, Modifiers::none()));
    let result = input_service.deliver_event(&sub, &event);
    assert!(result.is_err());
}

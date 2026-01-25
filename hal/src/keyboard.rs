//! Keyboard device abstraction
//!
//! This module provides a hardware abstraction for keyboard input devices.
//! It defines the interface that architecture-specific implementations must provide.
//!
//! ## Philosophy
//!
//! - **Hardware is just a source**: Keyboards provide raw scan codes, not authority
//! - **Not a TTY**: This is not stdin, not a terminal emulator
//! - **Deterministic translation**: Scan codes map to logical keys predictably
//! - **Testable**: Can mock hardware via fake implementations
//!
//! ## Design
//!
//! The keyboard interface is minimal:
//! - Poll-based (no interrupts at HAL level)
//! - Returns raw hardware events
//! - Translation to PandaGen input types happens above this layer

/// Hardware scancode representation
///
/// Encodes scancodes from PS/2 keyboards (Set 1).
/// Extended keys (arrows, nav cluster) are prefixed with 0xE0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HalScancode {
    /// Base scancode (no prefix)
    Base(u8),
    /// Extended scancode (0xE0 prefix)
    E0(u8),
}

impl HalScancode {
    /// Creates a base scancode
    pub fn base(code: u8) -> Self {
        Self::Base(code)
    }

    /// Creates an E0-prefixed scancode
    pub fn e0(code: u8) -> Self {
        Self::E0(code)
    }

    /// Returns true if this is an extended (E0) scancode
    pub fn is_extended(&self) -> bool {
        matches!(self, Self::E0(_))
    }

    /// Returns the scancode value (without prefix)
    pub fn code(&self) -> u8 {
        match self {
            Self::Base(code) | Self::E0(code) => *code,
        }
    }
}

/// Hardware keyboard event
///
/// This represents a raw keyboard event from hardware before translation
/// to PandaGen's logical input types.
///
/// **NOTE**: This type should NOT leak outside the HAL boundary.
/// Only the translation layer should see `HalKeyEvent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HalKeyEvent {
    /// Raw scan code from keyboard controller
    pub scancode: HalScancode,

    /// Whether the key was pressed (true) or released (false)
    pub pressed: bool,

    /// Optional timestamp in nanoseconds (if hardware provides it)
    pub timestamp_ns: Option<u64>,
}

impl HalKeyEvent {
    /// Creates a new keyboard event with a base scancode
    pub fn new(scancode: u8, pressed: bool) -> Self {
        Self {
            scancode: HalScancode::Base(scancode),
            pressed,
            timestamp_ns: None,
        }
    }

    /// Creates a new keyboard event with explicit scancode type
    pub fn with_scancode(scancode: HalScancode, pressed: bool) -> Self {
        Self {
            scancode,
            pressed,
            timestamp_ns: None,
        }
    }

    /// Creates a keyboard event with timestamp
    pub fn with_timestamp(scancode: u8, pressed: bool, timestamp_ns: u64) -> Self {
        Self {
            scancode: HalScancode::Base(scancode),
            pressed,
            timestamp_ns: Some(timestamp_ns),
        }
    }

    /// Returns true if this is a key press event
    pub fn is_pressed(&self) -> bool {
        self.pressed
    }

    /// Returns true if this is a key release event
    pub fn is_released(&self) -> bool {
        !self.pressed
    }
}

/// Keyboard device trait
///
/// Architecture-specific implementations provide keyboard input via this trait.
///
/// ## Implementation Notes
///
/// - **Poll-based**: Call `poll_event()` to check for new events
/// - **Non-blocking**: Returns `None` if no event is available
/// - **Raw scan codes**: Events contain hardware-level scan codes
/// - **Stateless**: Device does not track modifier state or key repeat
///
/// ## Example
///
/// ```rust,ignore
/// let mut keyboard = X86Ps2Keyboard::new();
/// loop {
///     if let Some(event) = keyboard.poll_event() {
///         // Translate to logical key
///         let key_code = translate_scancode(event.scancode);
///         // Deliver to input system
///     }
/// }
/// ```
pub trait KeyboardDevice {
    /// Polls for a keyboard event
    ///
    /// Returns `Some(event)` if a key event is available, or `None` if
    /// there are no pending events.
    ///
    /// This method is non-blocking and returns immediately.
    fn poll_event(&mut self) -> Option<HalKeyEvent>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;
    use alloc::boxed::Box;

    #[test]
    fn test_hal_key_event_creation() {
        let event = HalKeyEvent::new(0x1E, true);
        assert_eq!(event.scancode, HalScancode::Base(0x1E));
        assert!(event.is_pressed());
        assert!(!event.is_released());
        assert_eq!(event.timestamp_ns, None);
    }

    #[test]
    fn test_hal_key_event_with_timestamp() {
        let event = HalKeyEvent::with_timestamp(0x1E, false, 123456789);
        assert_eq!(event.scancode, HalScancode::Base(0x1E));
        assert!(!event.is_pressed());
        assert!(event.is_released());
        assert_eq!(event.timestamp_ns, Some(123456789));
    }

    #[test]
    fn test_hal_key_event_pressed_released() {
        let press = HalKeyEvent::new(0x10, true);
        let release = HalKeyEvent::new(0x10, false);

        assert!(press.is_pressed());
        assert!(!press.is_released());
        assert!(!release.is_pressed());
        assert!(release.is_released());
    }

    #[test]
    fn test_hal_scancode_base() {
        let sc = HalScancode::base(0x1E);
        assert_eq!(sc, HalScancode::Base(0x1E));
        assert!(!sc.is_extended());
        assert_eq!(sc.code(), 0x1E);
    }

    #[test]
    fn test_hal_scancode_e0() {
        let sc = HalScancode::e0(0x48);
        assert_eq!(sc, HalScancode::E0(0x48));
        assert!(sc.is_extended());
        assert_eq!(sc.code(), 0x48);
    }

    #[test]
    fn test_hal_key_event_with_e0() {
        let event = HalKeyEvent::with_scancode(HalScancode::e0(0x48), true);
        assert_eq!(event.scancode, HalScancode::E0(0x48));
        assert!(event.scancode.is_extended());
        assert!(event.is_pressed());
    }

    /// Fake keyboard device for testing
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
    fn test_fake_keyboard_device() {
        let events = vec![
            HalKeyEvent::new(0x1E, true),  // A pressed
            HalKeyEvent::new(0x1E, false), // A released
            HalKeyEvent::new(0x30, true),  // B pressed
        ];

        let mut keyboard = FakeKeyboard::new(events.clone());

        // Poll all events
        assert_eq!(keyboard.poll_event(), Some(events[0]));
        assert_eq!(keyboard.poll_event(), Some(events[1]));
        assert_eq!(keyboard.poll_event(), Some(events[2]));
        assert_eq!(keyboard.poll_event(), None);
        assert_eq!(keyboard.poll_event(), None);
    }

    #[test]
    fn test_keyboard_device_trait() {
        let mut keyboard: Box<dyn KeyboardDevice> =
            Box::new(FakeKeyboard::new(vec![HalKeyEvent::new(0x1C, true)]));

        assert!(keyboard.poll_event().is_some());
        assert!(keyboard.poll_event().is_none());
    }
}

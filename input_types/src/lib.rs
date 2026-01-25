#![no_std]

//! # Input Types
//!
//! This crate defines the fundamental input event types for PandaGen OS.
//!
//! ## Philosophy
//!
//! - **Events, not bytes**: Input is structured events, not raw scan codes or byte streams
//! - **Explicit, not ambient**: Input must be explicitly subscribed to via capabilities
//! - **Testable**: Events are serializable and can be injected for testing
//! - **Stable**: API is versioned and designed for evolution
//!
//! ## Non-Goals
//!
//! This is NOT:
//! - Raw hardware scan codes (PS/2, USB HID)
//! - POSIX terminals or stdin/stdout
//! - Global keyboard state
//! - A complete input subsystem (just the types)

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};
use core::fmt;

/// Input event
///
/// Represents a single input event from any input device.
/// Currently supports keyboard only; pointer/touch reserved for future.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputEvent {
    /// Keyboard event
    Key(KeyEvent),
    // Reserved for future:
    // Pointer(PointerEvent),
    // Touch(TouchEvent),
}

impl InputEvent {
    /// Creates a key event
    pub fn key(event: KeyEvent) -> Self {
        Self::Key(event)
    }

    /// Returns true if this is a key event
    pub fn is_key(&self) -> bool {
        matches!(self, Self::Key(_))
    }

    /// Returns the key event if this is a key event
    pub fn as_key(&self) -> Option<&KeyEvent> {
        match self {
            Self::Key(event) => Some(event),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }
}

/// Keyboard event
///
/// Represents a single keyboard state change (key press, release, or repeat).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyEvent {
    /// The key that was pressed/released
    pub code: KeyCode,
    /// Modifier keys that were active
    pub modifiers: Modifiers,
    /// Event state (pressed, released, repeat)
    pub state: KeyState,
    /// Optional text representation (for IME support, future)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl KeyEvent {
    /// Creates a new key event
    pub fn new(code: KeyCode, modifiers: Modifiers, state: KeyState) -> Self {
        Self {
            code,
            modifiers,
            state,
            text: None,
        }
    }

    /// Creates a key pressed event
    pub fn pressed(code: KeyCode, modifiers: Modifiers) -> Self {
        Self::new(code, modifiers, KeyState::Pressed)
    }

    /// Creates a key released event
    pub fn released(code: KeyCode, modifiers: Modifiers) -> Self {
        Self::new(code, modifiers, KeyState::Released)
    }

    /// Creates a key repeat event
    pub fn repeat(code: KeyCode, modifiers: Modifiers) -> Self {
        Self::new(code, modifiers, KeyState::Repeat)
    }

    /// Adds text to this key event (for IME support)
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// Returns true if this is a press event
    pub fn is_pressed(&self) -> bool {
        self.state == KeyState::Pressed
    }

    /// Returns true if this is a release event
    pub fn is_released(&self) -> bool {
        self.state == KeyState::Released
    }

    /// Returns true if this is a repeat event
    pub fn is_repeat(&self) -> bool {
        self.state == KeyState::Repeat
    }
}

/// Key state
///
/// Represents whether a key was pressed, released, or is repeating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyState {
    /// Key was pressed down
    Pressed,
    /// Key was released
    Released,
    /// Key is auto-repeating
    Repeat,
}

impl fmt::Display for KeyState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pressed => write!(f, "pressed"),
            Self::Released => write!(f, "released"),
            Self::Repeat => write!(f, "repeat"),
        }
    }
}

/// Key code
///
/// Logical key codes, not hardware scan codes.
/// Based on common keyboard layouts, designed for extensibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KeyCode {
    // Letters
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,

    // Numbers
    Num0,
    Num1,
    Num2,
    Num3,
    Num4,
    Num5,
    Num6,
    Num7,
    Num8,
    Num9,

    // Function keys
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,

    // Special keys
    Escape,
    Tab,
    CapsLock,
    LeftShift,
    RightShift,
    LeftCtrl,
    RightCtrl,
    LeftAlt,
    RightAlt,
    LeftMeta,
    RightMeta,
    Space,
    Enter,
    Backspace,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,

    // Arrow keys
    Up,
    Down,
    Left,
    Right,

    // Punctuation and symbols
    Minus,
    Equal,
    LeftBracket,
    RightBracket,
    Backslash,
    Semicolon,
    Quote,
    Comma,
    Period,
    Slash,
    Grave,

    // Numpad
    NumpadDivide,
    NumpadMultiply,
    NumpadMinus,
    NumpadPlus,
    NumpadEnter,
    NumpadPeriod,
    Numpad0,
    Numpad1,
    Numpad2,
    Numpad3,
    Numpad4,
    Numpad5,
    Numpad6,
    Numpad7,
    Numpad8,
    Numpad9,

    // Other
    PrintScreen,
    ScrollLock,
    Pause,
    NumLock,

    // Unknown/unmapped key
    Unknown,
}

impl fmt::Display for KeyCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Modifier keys
///
/// Bitflags representing modifier key states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Modifiers {
    bits: u8,
}

impl Modifiers {
    /// No modifiers
    pub const NONE: Self = Self { bits: 0 };
    /// Control key
    pub const CTRL: Self = Self { bits: 1 << 0 };
    /// Alt key
    pub const ALT: Self = Self { bits: 1 << 1 };
    /// Shift key
    pub const SHIFT: Self = Self { bits: 1 << 2 };
    /// Meta/Super/Windows key
    pub const META: Self = Self { bits: 1 << 3 };

    /// Creates a new modifier set with no modifiers
    pub fn none() -> Self {
        Self::NONE
    }

    /// Creates a new modifier set from bits
    pub fn from_bits(bits: u8) -> Self {
        Self { bits }
    }

    /// Returns the raw bits
    pub fn bits(&self) -> u8 {
        self.bits
    }

    /// Adds a modifier
    pub fn with(mut self, other: Modifiers) -> Self {
        self.bits |= other.bits;
        self
    }

    /// Checks if a modifier is present
    pub fn contains(&self, other: Modifiers) -> bool {
        (self.bits & other.bits) == other.bits
    }

    /// Checks if Ctrl is pressed
    pub fn is_ctrl(&self) -> bool {
        self.contains(Self::CTRL)
    }

    /// Checks if Alt is pressed
    pub fn is_alt(&self) -> bool {
        self.contains(Self::ALT)
    }

    /// Checks if Shift is pressed
    pub fn is_shift(&self) -> bool {
        self.contains(Self::SHIFT)
    }

    /// Checks if Meta is pressed
    pub fn is_meta(&self) -> bool {
        self.contains(Self::META)
    }

    /// Returns true if no modifiers are pressed
    pub fn is_empty(&self) -> bool {
        self.bits == 0
    }
}

impl fmt::Display for Modifiers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_empty() {
            return write!(f, "none");
        }

        let mut parts = Vec::new();
        if self.is_ctrl() {
            parts.push("Ctrl");
        }
        if self.is_alt() {
            parts.push("Alt");
        }
        if self.is_shift() {
            parts.push("Shift");
        }
        if self.is_meta() {
            parts.push("Meta");
        }
        write!(f, "{}", parts.join("+"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use alloc::vec;

    #[test]
    fn test_input_event_key() {
        let key_event = KeyEvent::pressed(KeyCode::A, Modifiers::none());
        let event = InputEvent::key(key_event.clone());

        assert!(event.is_key());
        assert_eq!(event.as_key(), Some(&key_event));
    }

    #[test]
    fn test_key_event_pressed() {
        let event = KeyEvent::pressed(KeyCode::A, Modifiers::CTRL);

        assert!(event.is_pressed());
        assert!(!event.is_released());
        assert!(!event.is_repeat());
        assert_eq!(event.code, KeyCode::A);
        assert!(event.modifiers.is_ctrl());
    }

    #[test]
    fn test_key_event_released() {
        let event = KeyEvent::released(KeyCode::B, Modifiers::none());

        assert!(!event.is_pressed());
        assert!(event.is_released());
        assert!(!event.is_repeat());
        assert_eq!(event.code, KeyCode::B);
    }

    #[test]
    fn test_key_event_repeat() {
        let event = KeyEvent::repeat(KeyCode::C, Modifiers::SHIFT);

        assert!(!event.is_pressed());
        assert!(!event.is_released());
        assert!(event.is_repeat());
        assert!(event.modifiers.is_shift());
    }

    #[test]
    fn test_key_event_with_text() {
        let event = KeyEvent::pressed(KeyCode::A, Modifiers::none()).with_text("a");

        assert_eq!(event.text, Some("a".to_string()));
    }

    #[test]
    fn test_key_state_display() {
        assert_eq!(KeyState::Pressed.to_string(), "pressed");
        assert_eq!(KeyState::Released.to_string(), "released");
        assert_eq!(KeyState::Repeat.to_string(), "repeat");
    }

    #[test]
    fn test_modifiers_none() {
        let mods = Modifiers::none();
        assert!(mods.is_empty());
        assert!(!mods.is_ctrl());
        assert!(!mods.is_alt());
        assert!(!mods.is_shift());
        assert!(!mods.is_meta());
    }

    #[test]
    fn test_modifiers_single() {
        let mods = Modifiers::CTRL;
        assert!(!mods.is_empty());
        assert!(mods.is_ctrl());
        assert!(!mods.is_alt());
        assert!(!mods.is_shift());
        assert!(!mods.is_meta());
    }

    #[test]
    fn test_modifiers_combination() {
        let mods = Modifiers::CTRL.with(Modifiers::SHIFT);
        assert!(mods.is_ctrl());
        assert!(mods.is_shift());
        assert!(!mods.is_alt());
        assert!(!mods.is_meta());
    }

    #[test]
    fn test_modifiers_contains() {
        let mods = Modifiers::CTRL.with(Modifiers::SHIFT);
        assert!(mods.contains(Modifiers::CTRL));
        assert!(mods.contains(Modifiers::SHIFT));
        assert!(!mods.contains(Modifiers::ALT));
        assert!(mods.contains(Modifiers::CTRL.with(Modifiers::SHIFT)));
    }

    #[test]
    fn test_modifiers_display() {
        assert_eq!(Modifiers::none().to_string(), "none");
        assert_eq!(Modifiers::CTRL.to_string(), "Ctrl");
        assert_eq!(Modifiers::CTRL.with(Modifiers::ALT).to_string(), "Ctrl+Alt");
        assert_eq!(
            Modifiers::CTRL
                .with(Modifiers::SHIFT)
                .with(Modifiers::ALT)
                .to_string(),
            "Ctrl+Alt+Shift"
        );
    }

    #[test]
    fn test_key_event_serialization() {
        let event = KeyEvent::pressed(KeyCode::A, Modifiers::CTRL);
        let json = serde_json::to_string(&event).unwrap();
        let deserialized: KeyEvent = serde_json::from_str(&json).unwrap();

        assert_eq!(event, deserialized);
    }

    #[test]
    fn test_input_event_serialization() {
        let key_event = KeyEvent::pressed(KeyCode::Enter, Modifiers::none());
        let event = InputEvent::key(key_event);

        let json = serde_json::to_string(&event).unwrap();
        let deserialized: InputEvent = serde_json::from_str(&json).unwrap();

        assert_eq!(event, deserialized);
    }

    #[test]
    fn test_modifiers_serialization() {
        let mods = Modifiers::CTRL.with(Modifiers::SHIFT);
        let json = serde_json::to_string(&mods).unwrap();
        let deserialized: Modifiers = serde_json::from_str(&json).unwrap();

        assert_eq!(mods, deserialized);
    }

    #[test]
    fn test_key_event_equality() {
        let event1 = KeyEvent::pressed(KeyCode::A, Modifiers::CTRL);
        let event2 = KeyEvent::pressed(KeyCode::A, Modifiers::CTRL);
        let event3 = KeyEvent::pressed(KeyCode::B, Modifiers::CTRL);

        assert_eq!(event1, event2);
        assert_ne!(event1, event3);
    }

    #[test]
    fn test_modifiers_equality() {
        let mods1 = Modifiers::CTRL.with(Modifiers::SHIFT);
        let mods2 = Modifiers::SHIFT.with(Modifiers::CTRL);
        let mods3 = Modifiers::CTRL.with(Modifiers::ALT);

        assert_eq!(mods1, mods2);
        assert_ne!(mods1, mods3);
    }

    #[test]
    fn test_all_letter_keycodes() {
        // Verify all letter key codes are distinct
        let letters = vec![
            KeyCode::A,
            KeyCode::B,
            KeyCode::C,
            KeyCode::D,
            KeyCode::E,
            KeyCode::F,
            KeyCode::G,
            KeyCode::H,
            KeyCode::I,
            KeyCode::J,
            KeyCode::K,
            KeyCode::L,
            KeyCode::M,
            KeyCode::N,
            KeyCode::O,
            KeyCode::P,
            KeyCode::Q,
            KeyCode::R,
            KeyCode::S,
            KeyCode::T,
            KeyCode::U,
            KeyCode::V,
            KeyCode::W,
            KeyCode::X,
            KeyCode::Y,
            KeyCode::Z,
        ];

        assert_eq!(letters.len(), 26);
        for i in 0..letters.len() {
            for j in (i + 1)..letters.len() {
                assert_ne!(letters[i], letters[j]);
            }
        }
    }

    #[test]
    fn test_all_number_keycodes() {
        let numbers = vec![
            KeyCode::Num0,
            KeyCode::Num1,
            KeyCode::Num2,
            KeyCode::Num3,
            KeyCode::Num4,
            KeyCode::Num5,
            KeyCode::Num6,
            KeyCode::Num7,
            KeyCode::Num8,
            KeyCode::Num9,
        ];

        assert_eq!(numbers.len(), 10);
    }

    #[test]
    fn test_all_function_keycodes() {
        let function_keys = vec![
            KeyCode::F1,
            KeyCode::F2,
            KeyCode::F3,
            KeyCode::F4,
            KeyCode::F5,
            KeyCode::F6,
            KeyCode::F7,
            KeyCode::F8,
            KeyCode::F9,
            KeyCode::F10,
            KeyCode::F11,
            KeyCode::F12,
        ];

        assert_eq!(function_keys.len(), 12);
    }

    #[test]
    fn test_modifier_combinations_comprehensive() {
        // Test all possible single and double modifier combinations
        let all_mods = vec![
            Modifiers::CTRL,
            Modifiers::ALT,
            Modifiers::SHIFT,
            Modifiers::META,
        ];

        // Single modifiers
        for mod1 in &all_mods {
            assert!((*mod1).contains(*mod1));
            assert!(!mod1.is_empty());
        }

        // Double combinations
        for i in 0..all_mods.len() {
            for j in (i + 1)..all_mods.len() {
                let combined = all_mods[i].with(all_mods[j]);
                assert!(combined.contains(all_mods[i]));
                assert!(combined.contains(all_mods[j]));
            }
        }
    }
}

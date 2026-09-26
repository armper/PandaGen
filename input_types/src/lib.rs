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
use core::fmt;
use serde::{Deserialize, Serialize};

/// Input event
///
/// Represents a single input event from any input device.
/// Keyboard and pointer are supported; touch is reserved for future.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputEvent {
    /// Keyboard event
    Key(KeyEvent),
    /// Pointer (mouse, trackpad, tablet) event
    Pointer(PointerEvent),
    // Reserved for future:
    // Touch(TouchEvent),
}

impl InputEvent {
    /// Creates a key event
    pub fn key(event: KeyEvent) -> Self {
        Self::Key(event)
    }

    /// Creates a pointer event
    pub fn pointer(event: PointerEvent) -> Self {
        Self::Pointer(event)
    }

    /// Returns true if this is a key event
    pub fn is_key(&self) -> bool {
        matches!(self, Self::Key(_))
    }

    /// Returns true if this is a pointer event
    pub fn is_pointer(&self) -> bool {
        matches!(self, Self::Pointer(_))
    }

    /// Returns the key event if this is a key event
    pub fn as_key(&self) -> Option<&KeyEvent> {
        match self {
            Self::Key(event) => Some(event),
            _ => None,
        }
    }

    /// Returns the pointer event if this is a pointer event
    pub fn as_pointer(&self) -> Option<&PointerEvent> {
        match self {
            Self::Pointer(event) => Some(event),
            _ => None,
        }
    }
}

/// Pointer position in desktop surface pixels.
///
/// Signed so relative motion can be expressed with the same type and so a
/// pointer can be reported just outside a surface during capture without
/// wrapping. The origin is the top-left corner of the desktop surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct PointerPosition {
    pub x: i32,
    pub y: i32,
}

impl PointerPosition {
    pub const ORIGIN: Self = Self { x: 0, y: 0 };

    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    /// Position moved by `delta`, saturating at the i32 range.
    pub fn offset(self, delta: PointerDelta) -> Self {
        Self {
            x: self.x.saturating_add(delta.dx),
            y: self.y.saturating_add(delta.dy),
        }
    }

    /// Position clamped into `0..width` x `0..height`.
    pub fn clamp_to(self, width: u32, height: u32) -> Self {
        let max_x = i32::try_from(width.saturating_sub(1)).unwrap_or(i32::MAX);
        let max_y = i32::try_from(height.saturating_sub(1)).unwrap_or(i32::MAX);
        Self {
            x: self.x.clamp(0, max_x.max(0)),
            y: self.y.clamp(0, max_y.max(0)),
        }
    }
}

/// Relative pointer motion in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct PointerDelta {
    pub dx: i32,
    pub dy: i32,
}

impl PointerDelta {
    pub const ZERO: Self = Self { dx: 0, dy: 0 };

    pub const fn new(dx: i32, dy: i32) -> Self {
        Self { dx, dy }
    }

    pub const fn is_zero(self) -> bool {
        self.dx == 0 && self.dy == 0
    }
}

/// A single pointer button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PointerButton {
    /// Usually the left button.
    Primary,
    /// Usually the right button.
    Secondary,
    /// Usually the wheel button.
    Middle,
    /// Additional buttons, numbered from 0.
    Extra(u8),
}

impl PointerButton {
    /// Bit used for this button inside `PointerButtons`.
    const fn bit(self) -> u8 {
        match self {
            PointerButton::Primary => 1 << 0,
            PointerButton::Secondary => 1 << 1,
            PointerButton::Middle => 1 << 2,
            // Extra buttons share the remaining bits; beyond five they alias.
            PointerButton::Extra(index) => {
                let shift = 3 + (index % 5);
                1 << shift
            }
        }
    }
}

impl fmt::Display for PointerButton {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PointerButton::Primary => write!(f, "Primary"),
            PointerButton::Secondary => write!(f, "Secondary"),
            PointerButton::Middle => write!(f, "Middle"),
            PointerButton::Extra(index) => write!(f, "Extra{}", index),
        }
    }
}

/// Set of pointer buttons currently held.
///
/// Carried on every pointer event so consumers never have to reconstruct
/// button state from the history of press/release events they may have
/// missed while unfocused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct PointerButtons {
    bits: u8,
}

impl PointerButtons {
    pub const NONE: Self = Self { bits: 0 };
    pub const PRIMARY: Self = Self {
        bits: PointerButton::Primary.bit(),
    };
    pub const SECONDARY: Self = Self {
        bits: PointerButton::Secondary.bit(),
    };
    pub const MIDDLE: Self = Self {
        bits: PointerButton::Middle.bit(),
    };

    pub const fn none() -> Self {
        Self::NONE
    }

    pub const fn from_bits(bits: u8) -> Self {
        Self { bits }
    }

    pub const fn bits(self) -> u8 {
        self.bits
    }

    pub const fn is_empty(self) -> bool {
        self.bits == 0
    }

    pub const fn contains(self, button: PointerButton) -> bool {
        self.bits & button.bit() != 0
    }

    pub const fn with(self, button: PointerButton) -> Self {
        Self {
            bits: self.bits | button.bit(),
        }
    }

    pub const fn without(self, button: PointerButton) -> Self {
        Self {
            bits: self.bits & !button.bit(),
        }
    }

    pub const fn union(self, other: Self) -> Self {
        Self {
            bits: self.bits | other.bits,
        }
    }
}

/// Whether a button went down or up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ButtonState {
    Pressed,
    Released,
}

/// Pointer capture transitions.
///
/// Capture is explicit in PandaGen: a component that starts a drag asks to
/// capture the pointer, receives every pointer event until capture ends, and
/// is told when capture is lost so it can cancel the interaction cleanly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PointerCapture {
    /// The receiver now owns all pointer events.
    Gained,
    /// The receiver no longer owns pointer events (released or revoked).
    Lost,
}

/// What happened to the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PointerEventKind {
    /// The pointer moved. `delta` is the motion since the previous event.
    Move { delta: PointerDelta },
    /// A button changed state.
    Button {
        button: PointerButton,
        state: ButtonState,
    },
    /// Scroll wheel or trackpad scroll. Positive `dy` scrolls content up
    /// (wheel away from the user); units are device notches.
    Wheel { dx: i32, dy: i32 },
    /// Capture ownership changed for the receiver.
    Capture(PointerCapture),
}

/// Pointer event
///
/// Every variant carries the absolute position, the full held-button set,
/// and keyboard modifiers at the time of the event, so hit testing and
/// gesture logic can be written against one event without extra state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PointerEvent {
    pub kind: PointerEventKind,
    pub position: PointerPosition,
    pub buttons: PointerButtons,
    pub modifiers: Modifiers,
}

impl PointerEvent {
    pub const fn new(
        kind: PointerEventKind,
        position: PointerPosition,
        buttons: PointerButtons,
        modifiers: Modifiers,
    ) -> Self {
        Self {
            kind,
            position,
            buttons,
            modifiers,
        }
    }

    /// Pointer moved to `position` by `delta` with `buttons` held.
    pub const fn moved(
        position: PointerPosition,
        delta: PointerDelta,
        buttons: PointerButtons,
    ) -> Self {
        Self::new(
            PointerEventKind::Move { delta },
            position,
            buttons,
            Modifiers::NONE,
        )
    }

    /// `button` pressed at `position`; `buttons` is the held set including it.
    pub const fn button_pressed(
        position: PointerPosition,
        button: PointerButton,
        buttons: PointerButtons,
    ) -> Self {
        Self::new(
            PointerEventKind::Button {
                button,
                state: ButtonState::Pressed,
            },
            position,
            buttons.with(button),
            Modifiers::NONE,
        )
    }

    /// `button` released at `position`; `buttons` is the held set before release.
    pub const fn button_released(
        position: PointerPosition,
        button: PointerButton,
        buttons: PointerButtons,
    ) -> Self {
        Self::new(
            PointerEventKind::Button {
                button,
                state: ButtonState::Released,
            },
            position,
            buttons.without(button),
            Modifiers::NONE,
        )
    }

    pub const fn wheel(
        position: PointerPosition,
        dx: i32,
        dy: i32,
        buttons: PointerButtons,
    ) -> Self {
        Self::new(
            PointerEventKind::Wheel { dx, dy },
            position,
            buttons,
            Modifiers::NONE,
        )
    }

    pub const fn capture(position: PointerPosition, transition: PointerCapture) -> Self {
        Self::new(
            PointerEventKind::Capture(transition),
            position,
            PointerButtons::NONE,
            Modifiers::NONE,
        )
    }

    pub const fn with_modifiers(mut self, modifiers: Modifiers) -> Self {
        self.modifiers = modifiers;
        self
    }

    pub const fn is_move(&self) -> bool {
        matches!(self.kind, PointerEventKind::Move { .. })
    }

    /// The button and state if this is a button event.
    pub const fn button(&self) -> Option<(PointerButton, ButtonState)> {
        match self.kind {
            PointerEventKind::Button { button, state } => Some((button, state)),
            _ => None,
        }
    }

    /// True for a press of `button`.
    pub fn is_press(&self, button: PointerButton) -> bool {
        self.button() == Some((button, ButtonState::Pressed))
    }

    /// True for a release of `button`.
    pub fn is_release(&self, button: PointerButton) -> bool {
        self.button() == Some((button, ButtonState::Released))
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

impl KeyCode {
    /// The character this key types on a US keyboard, with or without
    /// Shift (HOST-004). `None` for keys that type nothing (arrows,
    /// modifiers, function keys). Letters are lowercase unless `shift`.
    ///
    /// One table for every host that turns keys into text; the host
    /// simulator had its own, lowercase only, with `;` typing `:`.
    pub const fn to_char(self, shift: bool) -> Option<char> {
        use KeyCode::*;
        let (plain, shifted) = match self {
            A => ('a', 'A'),
            B => ('b', 'B'),
            C => ('c', 'C'),
            D => ('d', 'D'),
            E => ('e', 'E'),
            F => ('f', 'F'),
            G => ('g', 'G'),
            H => ('h', 'H'),
            I => ('i', 'I'),
            J => ('j', 'J'),
            K => ('k', 'K'),
            L => ('l', 'L'),
            M => ('m', 'M'),
            N => ('n', 'N'),
            O => ('o', 'O'),
            P => ('p', 'P'),
            Q => ('q', 'Q'),
            R => ('r', 'R'),
            S => ('s', 'S'),
            T => ('t', 'T'),
            U => ('u', 'U'),
            V => ('v', 'V'),
            W => ('w', 'W'),
            X => ('x', 'X'),
            Y => ('y', 'Y'),
            Z => ('z', 'Z'),
            Num0 => ('0', ')'),
            Num1 => ('1', '!'),
            Num2 => ('2', '@'),
            Num3 => ('3', '#'),
            Num4 => ('4', '$'),
            Num5 => ('5', '%'),
            Num6 => ('6', '^'),
            Num7 => ('7', '&'),
            Num8 => ('8', '*'),
            Num9 => ('9', '('),
            Space => (' ', ' '),
            Minus => ('-', '_'),
            Equal => ('=', '+'),
            LeftBracket => ('[', '{'),
            RightBracket => (']', '}'),
            Backslash => ('\\', '|'),
            Semicolon => (';', ':'),
            Quote => ('\'', '"'),
            Comma => (',', '<'),
            Period => ('.', '>'),
            Slash => ('/', '?'),
            Grave => ('`', '~'),
            NumpadDivide => ('/', '/'),
            NumpadMultiply => ('*', '*'),
            NumpadMinus => ('-', '-'),
            NumpadPlus => ('+', '+'),
            NumpadPeriod => ('.', '.'),
            Numpad0 => ('0', '0'),
            Numpad1 => ('1', '1'),
            Numpad2 => ('2', '2'),
            Numpad3 => ('3', '3'),
            Numpad4 => ('4', '4'),
            Numpad5 => ('5', '5'),
            Numpad6 => ('6', '6'),
            Numpad7 => ('7', '7'),
            Numpad8 => ('8', '8'),
            Numpad9 => ('9', '9'),
            _ => return None,
        };
        Some(if shift { shifted } else { plain })
    }
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

    #[test]
    fn keys_type_their_characters_with_and_without_shift() {
        assert_eq!(KeyCode::A.to_char(false), Some('a'));
        assert_eq!(KeyCode::A.to_char(true), Some('A'));
        assert_eq!(KeyCode::Semicolon.to_char(false), Some(';'));
        assert_eq!(KeyCode::Semicolon.to_char(true), Some(':'));
        assert_eq!(KeyCode::Num1.to_char(true), Some('!'));
        assert_eq!(KeyCode::Quote.to_char(true), Some('"'));
        assert_eq!(KeyCode::Backslash.to_char(false), Some('\\'));
        assert_eq!(KeyCode::Numpad7.to_char(true), Some('7'));
        assert_eq!(KeyCode::Left.to_char(false), None);
        assert_eq!(KeyCode::LeftShift.to_char(true), None);
    }
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
    fn test_pointer_buttons_bitflags() {
        let held = PointerButtons::none()
            .with(PointerButton::Primary)
            .with(PointerButton::Extra(1));
        assert!(held.contains(PointerButton::Primary));
        assert!(held.contains(PointerButton::Extra(1)));
        assert!(!held.contains(PointerButton::Secondary));
        assert!(!held.contains(PointerButton::Extra(0)));
        assert!(!held.is_empty());

        let released = held.without(PointerButton::Primary);
        assert!(!released.contains(PointerButton::Primary));
        assert!(released.contains(PointerButton::Extra(1)));

        assert_eq!(
            PointerButtons::PRIMARY.union(PointerButtons::MIDDLE).bits(),
            0b101
        );
        assert_eq!(PointerButtons::from_bits(0b010), PointerButtons::SECONDARY);
    }

    #[test]
    fn test_pointer_event_constructors_track_button_state() {
        let at = PointerPosition::new(40, 12);
        let press = PointerEvent::button_pressed(at, PointerButton::Primary, PointerButtons::NONE);
        assert!(press.is_press(PointerButton::Primary));
        assert!(!press.is_press(PointerButton::Secondary));
        assert!(!press.is_release(PointerButton::Primary));
        assert!(press.buttons.contains(PointerButton::Primary));
        assert_eq!(
            press.button(),
            Some((PointerButton::Primary, ButtonState::Pressed))
        );

        let release = PointerEvent::button_released(at, PointerButton::Primary, press.buttons);
        assert!(release.is_release(PointerButton::Primary));
        assert!(release.buttons.is_empty());

        let moved = PointerEvent::moved(
            at.offset(PointerDelta::new(-5, 3)),
            PointerDelta::new(-5, 3),
            PointerButtons::PRIMARY,
        )
        .with_modifiers(Modifiers::SHIFT);
        assert!(moved.is_move());
        assert_eq!(moved.position, PointerPosition::new(35, 15));
        assert_eq!(moved.button(), None);
        assert!(moved.modifiers.is_shift());

        let wheel = PointerEvent::wheel(at, 0, -2, PointerButtons::NONE);
        assert_eq!(wheel.kind, PointerEventKind::Wheel { dx: 0, dy: -2 });

        let lost = PointerEvent::capture(at, PointerCapture::Lost);
        assert_eq!(lost.kind, PointerEventKind::Capture(PointerCapture::Lost));
        assert!(lost.buttons.is_empty());
    }

    #[test]
    fn test_pointer_position_offset_and_clamp() {
        let far = PointerPosition::new(i32::MAX, -7);
        assert_eq!(
            far.offset(PointerDelta::new(10, -10)),
            PointerPosition::new(i32::MAX, -17)
        );
        assert_eq!(
            PointerPosition::new(-3, 900).clamp_to(1280, 800),
            PointerPosition::new(0, 799)
        );
        assert_eq!(
            PointerPosition::new(5, 5).clamp_to(0, 0),
            PointerPosition::ORIGIN
        );
        assert!(PointerDelta::ZERO.is_zero());
        assert!(!PointerDelta::new(0, 1).is_zero());
    }

    #[test]
    fn test_input_event_pointer_accessors() {
        let event = InputEvent::pointer(PointerEvent::moved(
            PointerPosition::new(1, 2),
            PointerDelta::new(1, 2),
            PointerButtons::NONE,
        ));
        assert!(event.is_pointer());
        assert!(!event.is_key());
        assert!(event.as_key().is_none());
        assert_eq!(
            event.as_pointer().map(|p| p.position),
            Some(PointerPosition::new(1, 2))
        );

        let key = InputEvent::key(KeyEvent::pressed(KeyCode::A, Modifiers::NONE));
        assert!(key.as_pointer().is_none());
    }

    #[test]
    fn test_pointer_event_serialization_round_trip() {
        let events = [
            PointerEvent::moved(
                PointerPosition::new(100, -1),
                PointerDelta::new(3, -4),
                PointerButtons::PRIMARY.union(PointerButtons::MIDDLE),
            )
            .with_modifiers(Modifiers::CTRL.with(Modifiers::ALT)),
            PointerEvent::button_pressed(
                PointerPosition::new(7, 8),
                PointerButton::Extra(2),
                PointerButtons::NONE,
            ),
            PointerEvent::button_released(
                PointerPosition::new(7, 8),
                PointerButton::Secondary,
                PointerButtons::SECONDARY,
            ),
            PointerEvent::wheel(PointerPosition::ORIGIN, -1, 1, PointerButtons::NONE),
            PointerEvent::capture(PointerPosition::new(0, 0), PointerCapture::Gained),
        ];
        for event in events {
            let wrapped = InputEvent::pointer(event);
            let json = serde_json::to_string(&wrapped).unwrap();
            let back: InputEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(wrapped, back, "{json}");
        }

        // Key events keep their wire shape now that a second variant exists.
        let key = InputEvent::key(KeyEvent::pressed(KeyCode::Enter, Modifiers::none()));
        let json = serde_json::to_string(&key).unwrap();
        assert!(json.starts_with("{\"Key\":"), "{json}");
    }

    #[test]
    fn test_pointer_button_display() {
        assert_eq!(alloc::format!("{}", PointerButton::Primary), "Primary");
        assert_eq!(alloc::format!("{}", PointerButton::Extra(3)), "Extra3");
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

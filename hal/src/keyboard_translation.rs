//! Scancode to KeyCode translation
//!
//! This module translates hardware scan codes (from HAL) to logical key codes
//! (from input_types).
//!
//! ## Philosophy
//!
//! - **Deterministic mapping**: Same scan code always produces same KeyCode
//! - **Explicit "Unknown" fallback**: Unmapped keys return KeyCode::Unknown
//! - **No locale/IME complexity**: ASCII only for now, extensible later
//!
//! ## Scan Code Set
//!
//! This implementation assumes **PS/2 Scan Code Set 1** (the most common).
//! - Set 1 is the default for most PS/2 keyboards
//! - Released keys have bit 7 set (scancode | 0x80)
//! - Extended keys are prefixed with 0xE0 (not fully handled yet)

use input_types::{KeyCode, KeyEvent, KeyState, Modifiers};
use crate::keyboard::HalKeyEvent;

/// Modifier key tracking state
///
/// This tracks which modifier keys are currently pressed.
/// The translation layer maintains this state to generate proper
/// Modifiers flags for each KeyEvent.
#[derive(Debug, Clone, Default)]
pub struct ModifierState {
    left_shift: bool,
    right_shift: bool,
    left_ctrl: bool,
    right_ctrl: bool,
    left_alt: bool,
    right_alt: bool,
    left_meta: bool,
    right_meta: bool,
}

impl ModifierState {
    /// Creates a new modifier state (all released)
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Updates modifier state based on key event
    pub fn update(&mut self, code: KeyCode, pressed: bool) {
        match code {
            KeyCode::LeftShift => self.left_shift = pressed,
            KeyCode::RightShift => self.right_shift = pressed,
            KeyCode::LeftCtrl => self.left_ctrl = pressed,
            KeyCode::RightCtrl => self.right_ctrl = pressed,
            KeyCode::LeftAlt => self.left_alt = pressed,
            KeyCode::RightAlt => self.right_alt = pressed,
            KeyCode::LeftMeta => self.left_meta = pressed,
            KeyCode::RightMeta => self.right_meta = pressed,
            _ => {}
        }
    }
    
    /// Returns the current Modifiers flags
    pub fn to_modifiers(&self) -> Modifiers {
        let mut mods = Modifiers::none();
        
        if self.left_shift || self.right_shift {
            mods = mods.with(Modifiers::SHIFT);
        }
        if self.left_ctrl || self.right_ctrl {
            mods = mods.with(Modifiers::CTRL);
        }
        if self.left_alt || self.right_alt {
            mods = mods.with(Modifiers::ALT);
        }
        if self.left_meta || self.right_meta {
            mods = mods.with(Modifiers::META);
        }
        
        mods
    }
}

/// Translates a PS/2 scan code (Set 1) to a KeyCode
///
/// Returns KeyCode::Unknown for unmapped scan codes.
///
/// Note: Some scan codes overlap between navigation keys and numpad.
/// We prioritize navigation keys. Extended scan codes (0xE0 prefix)
/// would disambiguate, but that's not fully implemented yet.
pub fn scancode_to_keycode(scancode: u8) -> KeyCode {
    match scancode {
        // Row 1: ESC, function keys
        0x01 => KeyCode::Escape,
        0x3B => KeyCode::F1,
        0x3C => KeyCode::F2,
        0x3D => KeyCode::F3,
        0x3E => KeyCode::F4,
        0x3F => KeyCode::F5,
        0x40 => KeyCode::F6,
        0x41 => KeyCode::F7,
        0x42 => KeyCode::F8,
        0x43 => KeyCode::F9,
        0x44 => KeyCode::F10,
        0x57 => KeyCode::F11,
        0x58 => KeyCode::F12,
        
        // Row 2: Numbers
        0x29 => KeyCode::Grave,
        0x02 => KeyCode::Num1,
        0x03 => KeyCode::Num2,
        0x04 => KeyCode::Num3,
        0x05 => KeyCode::Num4,
        0x06 => KeyCode::Num5,
        0x07 => KeyCode::Num6,
        0x08 => KeyCode::Num7,
        0x09 => KeyCode::Num8,
        0x0A => KeyCode::Num9,
        0x0B => KeyCode::Num0,
        0x0C => KeyCode::Minus,
        0x0D => KeyCode::Equal,
        0x0E => KeyCode::Backspace,
        
        // Row 3: QWERTY row
        0x0F => KeyCode::Tab,
        0x10 => KeyCode::Q,
        0x11 => KeyCode::W,
        0x12 => KeyCode::E,
        0x13 => KeyCode::R,
        0x14 => KeyCode::T,
        0x15 => KeyCode::Y,
        0x16 => KeyCode::U,
        0x17 => KeyCode::I,
        0x18 => KeyCode::O,
        0x19 => KeyCode::P,
        0x1A => KeyCode::LeftBracket,
        0x1B => KeyCode::RightBracket,
        0x2B => KeyCode::Backslash,
        
        // Row 4: ASDF row
        0x3A => KeyCode::CapsLock,
        0x1E => KeyCode::A,
        0x1F => KeyCode::S,
        0x20 => KeyCode::D,
        0x21 => KeyCode::F,
        0x22 => KeyCode::G,
        0x23 => KeyCode::H,
        0x24 => KeyCode::J,
        0x25 => KeyCode::K,
        0x26 => KeyCode::L,
        0x27 => KeyCode::Semicolon,
        0x28 => KeyCode::Quote,
        0x1C => KeyCode::Enter,
        
        // Row 5: ZXCV row
        0x2A => KeyCode::LeftShift,
        0x2C => KeyCode::Z,
        0x2D => KeyCode::X,
        0x2E => KeyCode::C,
        0x2F => KeyCode::V,
        0x30 => KeyCode::B,
        0x31 => KeyCode::N,
        0x32 => KeyCode::M,
        0x33 => KeyCode::Comma,
        0x34 => KeyCode::Period,
        0x35 => KeyCode::Slash,
        0x36 => KeyCode::RightShift,
        
        // Bottom row
        0x1D => KeyCode::LeftCtrl,
        0x38 => KeyCode::LeftAlt,
        0x39 => KeyCode::Space,
        
        // Lock keys
        0x45 => KeyCode::NumLock,
        0x46 => KeyCode::ScrollLock,
        
        // Numpad operators
        0x4A => KeyCode::NumpadMinus,
        0x4E => KeyCode::NumpadPlus,
        0x37 => KeyCode::NumpadMultiply,
        // 0x35 => NumpadDivide (extended), conflicts with Slash
        
        // Navigation cluster (prioritized over numpad when ambiguous)
        // Note: With 0xE0 prefix these would be distinct, but we handle base codes
        0x47 => KeyCode::Home,         // Also Numpad7
        0x48 => KeyCode::Up,           // Also Numpad8
        0x49 => KeyCode::PageUp,       // Also Numpad9
        0x4B => KeyCode::Left,         // Also Numpad4
        0x4C => KeyCode::Numpad5,      // Center key (no nav equivalent)
        0x4D => KeyCode::Right,        // Also Numpad6
        0x4F => KeyCode::End,          // Also Numpad1
        0x50 => KeyCode::Down,         // Also Numpad2
        0x51 => KeyCode::PageDown,     // Also Numpad3
        0x52 => KeyCode::Insert,       // Also Numpad0
        0x53 => KeyCode::Delete,       // Also NumpadPeriod
        
        // Unmapped
        _ => KeyCode::Unknown,
    }
}

/// Keyboard translator
///
/// Maintains modifier state and translates HAL events to input_types KeyEvents.
pub struct KeyboardTranslator {
    modifiers: ModifierState,
}

impl KeyboardTranslator {
    /// Creates a new keyboard translator
    pub fn new() -> Self {
        Self {
            modifiers: ModifierState::new(),
        }
    }
    
    /// Translates a HAL keyboard event to a KeyEvent
    ///
    /// Returns None if the event should be ignored (e.g., extended scan code prefix).
    pub fn translate(&mut self, hal_event: HalKeyEvent) -> Option<KeyEvent> {
        // Determine if pressed or released
        let pressed = hal_event.is_pressed();
        let state = if pressed { KeyState::Pressed } else { KeyState::Released };
        
        // Translate scan code
        let code = scancode_to_keycode(hal_event.scancode);
        
        // Skip unknown keys
        if code == KeyCode::Unknown {
            return None;
        }
        
        // Update modifier state
        self.modifiers.update(code, pressed);
        
        // Get current modifiers
        let mods = self.modifiers.to_modifiers();
        
        // Create KeyEvent
        Some(KeyEvent::new(code, mods, state))
    }
    
    /// Resets the translator state (all modifiers released)
    pub fn reset(&mut self) {
        self.modifiers = ModifierState::new();
    }
}

impl Default for KeyboardTranslator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_scancode_to_keycode_letters() {
        assert_eq!(scancode_to_keycode(0x1E), KeyCode::A);
        assert_eq!(scancode_to_keycode(0x30), KeyCode::B);
        assert_eq!(scancode_to_keycode(0x2E), KeyCode::C);
        assert_eq!(scancode_to_keycode(0x2C), KeyCode::Z);
    }
    
    #[test]
    fn test_scancode_to_keycode_numbers() {
        assert_eq!(scancode_to_keycode(0x02), KeyCode::Num1);
        assert_eq!(scancode_to_keycode(0x0B), KeyCode::Num0);
    }
    
    #[test]
    fn test_scancode_to_keycode_special() {
        assert_eq!(scancode_to_keycode(0x01), KeyCode::Escape);
        assert_eq!(scancode_to_keycode(0x1C), KeyCode::Enter);
        assert_eq!(scancode_to_keycode(0x0E), KeyCode::Backspace);
        assert_eq!(scancode_to_keycode(0x39), KeyCode::Space);
    }
    
    #[test]
    fn test_scancode_to_keycode_arrows() {
        assert_eq!(scancode_to_keycode(0x48), KeyCode::Up);
        assert_eq!(scancode_to_keycode(0x50), KeyCode::Down);
        assert_eq!(scancode_to_keycode(0x4B), KeyCode::Left);
        assert_eq!(scancode_to_keycode(0x4D), KeyCode::Right);
    }
    
    #[test]
    fn test_scancode_to_keycode_function_keys() {
        assert_eq!(scancode_to_keycode(0x3B), KeyCode::F1);
        assert_eq!(scancode_to_keycode(0x44), KeyCode::F10);
        assert_eq!(scancode_to_keycode(0x57), KeyCode::F11);
        assert_eq!(scancode_to_keycode(0x58), KeyCode::F12);
    }
    
    #[test]
    fn test_scancode_to_keycode_modifiers() {
        assert_eq!(scancode_to_keycode(0x2A), KeyCode::LeftShift);
        assert_eq!(scancode_to_keycode(0x36), KeyCode::RightShift);
        assert_eq!(scancode_to_keycode(0x1D), KeyCode::LeftCtrl);
        assert_eq!(scancode_to_keycode(0x38), KeyCode::LeftAlt);
    }
    
    #[test]
    fn test_scancode_to_keycode_unknown() {
        assert_eq!(scancode_to_keycode(0xFF), KeyCode::Unknown);
        assert_eq!(scancode_to_keycode(0x00), KeyCode::Unknown);
    }
    
    #[test]
    fn test_modifier_state_shift() {
        let mut state = ModifierState::new();
        assert!(state.to_modifiers().is_empty());
        
        state.update(KeyCode::LeftShift, true);
        assert!(state.to_modifiers().is_shift());
        
        state.update(KeyCode::LeftShift, false);
        assert!(state.to_modifiers().is_empty());
    }
    
    #[test]
    fn test_modifier_state_ctrl() {
        let mut state = ModifierState::new();
        state.update(KeyCode::LeftCtrl, true);
        assert!(state.to_modifiers().is_ctrl());
    }
    
    #[test]
    fn test_modifier_state_multiple() {
        let mut state = ModifierState::new();
        state.update(KeyCode::LeftCtrl, true);
        state.update(KeyCode::LeftShift, true);
        
        let mods = state.to_modifiers();
        assert!(mods.is_ctrl());
        assert!(mods.is_shift());
        assert!(!mods.is_alt());
    }
    
    #[test]
    fn test_translator_basic() {
        let mut translator = KeyboardTranslator::new();
        
        let hal_event = HalKeyEvent::new(0x1E, true); // A pressed
        let key_event = translator.translate(hal_event).unwrap();
        
        assert_eq!(key_event.code, KeyCode::A);
        assert!(key_event.is_pressed());
        assert!(key_event.modifiers.is_empty());
    }
    
    #[test]
    fn test_translator_with_shift() {
        let mut translator = KeyboardTranslator::new();
        
        // Press shift
        let shift_down = HalKeyEvent::new(0x2A, true);
        translator.translate(shift_down).unwrap();
        
        // Press A (with shift held)
        let a_down = HalKeyEvent::new(0x1E, true);
        let key_event = translator.translate(a_down).unwrap();
        
        assert_eq!(key_event.code, KeyCode::A);
        assert!(key_event.modifiers.is_shift());
    }
    
    #[test]
    fn test_translator_modifier_release() {
        let mut translator = KeyboardTranslator::new();
        
        // Press Ctrl
        translator.translate(HalKeyEvent::new(0x1D, true)).unwrap();
        
        // Press C (Ctrl+C)
        let c_event = translator.translate(HalKeyEvent::new(0x2E, true)).unwrap();
        assert!(c_event.modifiers.is_ctrl());
        
        // Release Ctrl
        translator.translate(HalKeyEvent::new(0x1D, false)).unwrap();
        
        // Press C again (no Ctrl)
        let c_event2 = translator.translate(HalKeyEvent::new(0x2E, true)).unwrap();
        assert!(!c_event2.modifiers.is_ctrl());
    }
    
    #[test]
    fn test_translator_unknown_keys() {
        let mut translator = KeyboardTranslator::new();
        
        let unknown = HalKeyEvent::new(0xFF, true);
        assert_eq!(translator.translate(unknown), None);
    }
    
    #[test]
    fn test_translator_press_release_sequence() {
        let mut translator = KeyboardTranslator::new();
        
        // Press A
        let press = translator.translate(HalKeyEvent::new(0x1E, true)).unwrap();
        assert!(press.is_pressed());
        assert_eq!(press.code, KeyCode::A);
        
        // Release A
        let release = translator.translate(HalKeyEvent::new(0x1E, false)).unwrap();
        assert!(release.is_released());
        assert_eq!(release.code, KeyCode::A);
    }
    
    #[test]
    fn test_translator_reset() {
        let mut translator = KeyboardTranslator::new();
        
        // Press Ctrl
        translator.translate(HalKeyEvent::new(0x1D, true)).unwrap();
        
        // Reset
        translator.reset();
        
        // Press A (should have no modifiers)
        let event = translator.translate(HalKeyEvent::new(0x1E, true)).unwrap();
        assert!(event.modifiers.is_empty());
    }
}

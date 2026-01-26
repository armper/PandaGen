//! Bare-metal platform adapter for services_workspace_manager
//!
//! This module implements the `WorkspacePlatform` trait design for kernel integration.
//!
//! NOTE: This is currently test-only until all dependencies of services_workspace_manager
//! are no_std compatible. The bare-metal kernel binary still uses the legacy workspace
//! implementation in workspace.rs.

#[cfg(test)]
extern crate alloc;

#[cfg(test)]
use alloc::vec::Vec;
#[cfg(test)]
use core::sync::atomic::{AtomicU64, Ordering};
#[cfg(test)]
use input_types::KeyEvent;
#[cfg(test)]
use services_workspace_manager::platform::{
    WorkspaceDisplay, WorkspaceInput, WorkspacePlatform, WorkspaceTick,
};
#[cfg(test)]
use view_types::ViewFrame;

/// Bare-metal workspace platform adapter
///
/// Implements the WorkspacePlatform trait by bridging to kernel-provided
/// framebuffer, keyboard queue, and tick counter.
///
/// NOTE: This is a work-in-progress integration. Currently only available in tests
/// until dependency issues are resolved.
#[cfg(test)]
pub struct KernelWorkspacePlatform {
    display: KernelDisplay,
    input: KernelInput,
    tick: KernelTick,
}

#[cfg(test)]
impl KernelWorkspacePlatform {
    /// Creates a new kernel workspace platform for testing
    pub fn new_test() -> Self {
        Self {
            display: KernelDisplay::new(),
            input: KernelInput::new(),
            tick: KernelTick::new(),
        }
    }
}

#[cfg(test)]
impl WorkspacePlatform for KernelWorkspacePlatform {
    fn display(&mut self) -> &mut dyn WorkspaceDisplay {
        &mut self.display
    }

    fn input(&mut self) -> &mut dyn WorkspaceInput {
        &mut self.input
    }

    fn tick(&mut self) -> &mut dyn WorkspaceTick {
        &mut self.tick
    }
}

/// Kernel display implementation (test stub)
#[cfg(test)]
struct KernelDisplay {
    cleared: bool,
}

#[cfg(test)]
impl KernelDisplay {
    fn new() -> Self {
        Self { cleared: false }
    }
}

#[cfg(test)]
impl WorkspaceDisplay for KernelDisplay {
    fn render_main_view(&mut self, _frame: &ViewFrame) {
        // Test stub - would render to framebuffer in real implementation
    }

    fn render_status_view(&mut self, _frame: &ViewFrame) {
        // Test stub - would render to framebuffer in real implementation
    }

    fn render_status_strip(&mut self, _content: &str) {
        // Test stub - would render to framebuffer in real implementation
    }

    fn render_breadcrumbs(&mut self, _content: &str) {
        // Test stub - would render to framebuffer in real implementation
    }

    fn clear(&mut self) {
        self.cleared = true;
    }

    fn present(&mut self) {
        // Test stub - framebuffer writes directly to hardware in real implementation
    }
}

/// Kernel input implementation using keyboard queue
///
/// NOTE: This is a simplified test version that doesn't connect to the actual
/// keyboard IRQ queue in main.rs.
#[cfg(test)]
struct KernelInput {
    parser: Ps2ParserState,
    test_queue: Vec<u8>,
}

#[cfg(test)]
impl KernelInput {
    fn new() -> Self {
        Self {
            parser: Ps2ParserState::new(),
            test_queue: Vec::new(),
        }
    }

    /// For testing: inject scancodes directly
    #[allow(dead_code)]
    pub fn inject_scancode(&mut self, scancode: u8) {
        self.test_queue.push(scancode);
    }
}

#[cfg(test)]
impl WorkspaceInput for KernelInput {
    fn poll_event(&mut self) -> Option<KeyEvent> {
        // Poll test queue and convert scancodes to KeyEvents
        while !self.test_queue.is_empty() {
            let scancode = self.test_queue.remove(0);
            if let Some(key_event) = self.parser.process_scancode(scancode) {
                return Some(key_event);
            }
        }
        None
    }

    fn has_pending(&self) -> bool {
        !self.test_queue.is_empty()
    }
}

/// PS/2 keyboard parser state
#[cfg(test)]
struct Ps2ParserState {
    pending_e0: bool,
    shift_pressed: bool,
    ctrl_pressed: bool,
    alt_pressed: bool,
}

#[cfg(test)]
impl Ps2ParserState {
    fn new() -> Self {
        Self {
            pending_e0: false,
            shift_pressed: false,
            ctrl_pressed: false,
            alt_pressed: false,
        }
    }

    /// Process a scancode and return a KeyEvent if complete
    fn process_scancode(&mut self, scancode: u8) -> Option<KeyEvent> {
        // E0 prefix handling
        if scancode == 0xE0 {
            self.pending_e0 = true;
            return None;
        }

        let is_break = (scancode & 0x80) != 0;
        let code = scancode & 0x7F;

        // Handle modifier keys
        match code {
            0x2A | 0x36 => {
                // Left/Right Shift
                self.shift_pressed = !is_break;
                self.pending_e0 = false;
                return None;
            }
            0x1D => {
                // Left Ctrl (0x1D) or Right Ctrl (E0 0x1D)
                self.ctrl_pressed = !is_break;
                self.pending_e0 = false;
                return None;
            }
            0x38 => {
                // Left Alt (0x38) or Right Alt (E0 0x38)
                self.alt_pressed = !is_break;
                self.pending_e0 = false;
                return None;
            }
            _ => {}
        }

        // Ignore E0-prefixed keys and break codes for now
        if self.pending_e0 || is_break {
            self.pending_e0 = false;
            return None;
        }

        self.pending_e0 = false;

        // Convert scancode to KeyEvent
        self.scancode_to_key_event(code)
    }

    fn scancode_to_key_event(&self, code: u8) -> Option<KeyEvent> {
        use input_types::{KeyCode, KeyState, Modifiers};

        let mut modifiers = Modifiers::NONE;
        if self.shift_pressed {
            modifiers = modifiers.with(Modifiers::SHIFT);
        }
        if self.ctrl_pressed {
            modifiers = modifiers.with(Modifiers::CTRL);
        }
        if self.alt_pressed {
            modifiers = modifiers.with(Modifiers::ALT);
        }

        let key_code = match code {
            0x01 => KeyCode::Escape,
            0x0E => KeyCode::Backspace,
            0x0F => KeyCode::Tab,
            0x1C => KeyCode::Enter,
            0x39 => KeyCode::Space,

            // Letters
            0x1E => KeyCode::A,
            0x30 => KeyCode::B,
            0x2E => KeyCode::C,
            0x20 => KeyCode::D,
            0x12 => KeyCode::E,
            0x21 => KeyCode::F,
            0x22 => KeyCode::G,
            0x23 => KeyCode::H,
            0x17 => KeyCode::I,
            0x24 => KeyCode::J,
            0x25 => KeyCode::K,
            0x26 => KeyCode::L,
            0x32 => KeyCode::M,
            0x31 => KeyCode::N,
            0x18 => KeyCode::O,
            0x19 => KeyCode::P,
            0x10 => KeyCode::Q,
            0x13 => KeyCode::R,
            0x1F => KeyCode::S,
            0x14 => KeyCode::T,
            0x16 => KeyCode::U,
            0x2F => KeyCode::V,
            0x11 => KeyCode::W,
            0x2D => KeyCode::X,
            0x15 => KeyCode::Y,
            0x2C => KeyCode::Z,

            // Numbers
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

            // Punctuation
            0x0C => KeyCode::Minus,
            0x1A => KeyCode::LeftBracket,
            0x1B => KeyCode::RightBracket,
            0x2B => KeyCode::Backslash,
            0x27 => KeyCode::Semicolon,
            0x28 => KeyCode::Quote,
            0x33 => KeyCode::Comma,
            0x34 => KeyCode::Period,
            0x35 => KeyCode::Slash,

            // Function keys
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

            _ => return None,
        };

        Some(KeyEvent::new(key_code, modifiers, KeyState::Pressed))
    }
}

/// Kernel tick implementation
#[cfg(test)]
struct KernelTick {
    tick_count: AtomicU64,
}

#[cfg(test)]
impl KernelTick {
    fn new() -> Self {
        Self {
            tick_count: AtomicU64::new(0),
        }
    }
}

#[cfg(test)]
impl WorkspaceTick for KernelTick {
    fn advance(&mut self) -> u64 {
        let new_tick = self.tick_count.load(Ordering::Relaxed) + 1;
        self.tick_count.store(new_tick, Ordering::Relaxed);
        new_tick
    }

    fn current(&self) -> u64 {
        self.tick_count.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use input_types::{KeyCode, Modifiers};

    #[test]
    fn test_ps2_parser_basic() {
        let mut parser = Ps2ParserState::new();

        // Test 'A' key press (scancode 0x1E)
        let event = parser.process_scancode(0x1E);
        assert!(event.is_some());

        if let Some(e) = event {
            assert_eq!(e.code, KeyCode::A);
            assert!(!e.modifiers.contains(Modifiers::SHIFT));
        }
    }

    #[test]
    fn test_ps2_parser_shift() {
        let mut parser = Ps2ParserState::new();

        // Press left shift (0x2A)
        assert!(parser.process_scancode(0x2A).is_none());
        assert!(parser.shift_pressed);

        // Press 'A' with shift
        let event = parser.process_scancode(0x1E);
        assert!(event.is_some());

        if let Some(e) = event {
            assert_eq!(e.code, KeyCode::A);
            assert!(e.modifiers.contains(Modifiers::SHIFT));
        }
    }

    #[test]
    fn test_ps2_parser_ctrl() {
        let mut parser = Ps2ParserState::new();

        // Press left ctrl (0x1D)
        assert!(parser.process_scancode(0x1D).is_none());
        assert!(parser.ctrl_pressed);

        // Press 'P' with ctrl (Ctrl+P for command palette)
        let event = parser.process_scancode(0x19);
        assert!(event.is_some());

        if let Some(e) = event {
            assert_eq!(e.code, KeyCode::P);
            assert!(e.modifiers.contains(Modifiers::CTRL));
        }
    }

    #[test]
    fn test_kernel_platform_creation() {
        let platform = KernelWorkspacePlatform::new_test();
        // Just verify it can be created
        let _ = platform;
    }
}

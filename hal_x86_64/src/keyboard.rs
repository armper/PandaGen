//! x86_64 PS/2 Keyboard implementation
//!
//! This module provides a real PS/2 keyboard driver for x86_64.
//!
//! ## Implementation
//!
//! - Reads scancodes from i8042 controller (ports 0x60 and 0x64)
//! - Supports PS/2 Scan Code Set 1 (make/break codes)
//! - Handles E0 extended key sequences
//! - Non-blocking polling interface
//! - Testable via FakePortIo abstraction
//!
//! ## PS/2 Controller (i8042)
//!
//! - Status Register (0x64): Bit 0 = OBF (Output Buffer Full)
//! - Data Port (0x60): Read/write data

use crate::port_io::PortIo;
use core::prelude::v1::*;
use hal::keyboard::{HalKeyEvent, HalScancode, KeyboardDevice};

/// PS/2 controller port addresses
const PS2_DATA_PORT: u16 = 0x60;
const PS2_STATUS_PORT: u16 = 0x64;

/// PS/2 status register bits
const STATUS_OBF: u8 = 0x01; // Output Buffer Full

/// Special scancode values
const SCANCODE_E0_PREFIX: u8 = 0xE0;
const SCANCODE_BREAK_BIT: u8 = 0x80;

/// x86_64 PS/2 keyboard device
///
/// Reads keyboard events from the PS/2 controller using port I/O.
///
/// ## Usage
///
/// ```rust,ignore
/// use hal_x86_64::{X86Ps2Keyboard, RealPortIo};
///
/// let mut keyboard = X86Ps2Keyboard::new(RealPortIo::new());
/// loop {
///     if let Some(event) = keyboard.poll_event() {
///         // Process event
///     }
/// }
/// ```
pub struct X86Ps2Keyboard<P: PortIo> {
    /// Port I/O interface
    port_io: P,
    /// Parser state
    state: ParserState,
}

/// Scancode parser state machine
#[derive(Debug, Clone, Copy, Default)]
struct ParserState {
    /// Waiting for second byte of E0 sequence
    pending_e0: bool,
}

impl<P: PortIo> X86Ps2Keyboard<P> {
    /// Creates a new PS/2 keyboard device with the given port I/O implementation
    pub fn new(port_io: P) -> Self {
        Self {
            port_io,
            state: ParserState::default(),
        }
    }

    /// Checks if data is available from the keyboard controller
    fn data_available(&mut self) -> bool {
        let status = self.port_io.inb(PS2_STATUS_PORT);
        (status & STATUS_OBF) != 0
    }

    /// Reads a byte from the keyboard data port
    fn read_data(&mut self) -> u8 {
        self.port_io.inb(PS2_DATA_PORT)
    }

    /// Parses a scancode byte and updates state
    ///
    /// Returns Some(HalKeyEvent) if a complete event is ready,
    /// or None if more bytes are needed (E0 prefix) or no data available.
    fn parse_scancode(&mut self, byte: u8) -> Option<HalKeyEvent> {
        // Handle E0 prefix
        if byte == SCANCODE_E0_PREFIX {
            self.state.pending_e0 = true;
            return None; // Need next byte
        }

        // Determine if this is a break code (key release)
        let pressed = (byte & SCANCODE_BREAK_BIT) == 0;

        // Extract base scancode (remove break bit)
        let code = byte & !SCANCODE_BREAK_BIT;

        // Build scancode with E0 prefix if pending
        let scancode = if self.state.pending_e0 {
            self.state.pending_e0 = false;
            HalScancode::E0(code)
        } else {
            HalScancode::Base(code)
        };

        // Create event
        Some(HalKeyEvent::with_scancode(scancode, pressed))
    }
}

impl<P: PortIo> KeyboardDevice for X86Ps2Keyboard<P> {
    fn poll_event(&mut self) -> Option<HalKeyEvent> {
        // Non-blocking check: return immediately if no data
        if !self.data_available() {
            return None;
        }

        // Read the scancode byte
        let byte = self.read_data();

        // Parse it (may return None if E0 prefix)
        self.parse_scancode(byte)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::port_io::FakePortIo;

    #[test]
    fn test_x86_keyboard_creation() {
        let io = FakePortIo::new();
        let keyboard = X86Ps2Keyboard::new(io);
        // Should create without panicking
        drop(keyboard);
    }

    #[test]
    fn test_x86_keyboard_no_data() {
        let mut io = FakePortIo::new();
        io.script_read(PS2_STATUS_PORT, 0x00); // OBF clear

        let mut keyboard = X86Ps2Keyboard::new(io);
        assert_eq!(keyboard.poll_event(), None);
    }

    #[test]
    fn test_x86_keyboard_simple_make_code() {
        let mut io = FakePortIo::new();
        io.script_read(PS2_STATUS_PORT, STATUS_OBF); // Data available
        io.script_read(PS2_DATA_PORT, 0x1E); // Scancode: A pressed

        let mut keyboard = X86Ps2Keyboard::new(io);
        let event = keyboard.poll_event().unwrap();

        assert_eq!(event.scancode, HalScancode::Base(0x1E));
        assert!(event.pressed);
    }

    #[test]
    fn test_x86_keyboard_simple_break_code() {
        let mut io = FakePortIo::new();
        io.script_read(PS2_STATUS_PORT, STATUS_OBF);
        io.script_read(PS2_DATA_PORT, 0x1E | 0x80); // A released

        let mut keyboard = X86Ps2Keyboard::new(io);
        let event = keyboard.poll_event().unwrap();

        assert_eq!(event.scancode, HalScancode::Base(0x1E));
        assert!(!event.pressed);
    }

    #[test]
    fn test_x86_keyboard_e0_sequence() {
        let mut io = FakePortIo::new();

        // First poll: E0 prefix
        io.script_read(PS2_STATUS_PORT, STATUS_OBF);
        io.script_read(PS2_DATA_PORT, SCANCODE_E0_PREFIX);

        // Second poll: actual scancode
        io.script_read(PS2_STATUS_PORT, STATUS_OBF);
        io.script_read(PS2_DATA_PORT, 0x48); // Up arrow

        let mut keyboard = X86Ps2Keyboard::new(io);

        // First poll returns None (E0 prefix consumed)
        assert_eq!(keyboard.poll_event(), None);

        // Second poll returns the E0 event
        let event = keyboard.poll_event().unwrap();
        assert_eq!(event.scancode, HalScancode::E0(0x48));
        assert!(event.pressed);
    }

    #[test]
    fn test_x86_keyboard_e0_break_code() {
        let mut io = FakePortIo::new();

        // E0 prefix
        io.script_read(PS2_STATUS_PORT, STATUS_OBF);
        io.script_read(PS2_DATA_PORT, SCANCODE_E0_PREFIX);

        // Break code
        io.script_read(PS2_STATUS_PORT, STATUS_OBF);
        io.script_read(PS2_DATA_PORT, 0x48 | 0x80); // Up arrow released

        let mut keyboard = X86Ps2Keyboard::new(io);

        assert_eq!(keyboard.poll_event(), None);

        let event = keyboard.poll_event().unwrap();
        assert_eq!(event.scancode, HalScancode::E0(0x48));
        assert!(!event.pressed);
    }

    #[test]
    fn test_x86_keyboard_multiple_events() {
        let mut io = FakePortIo::new();

        // A pressed
        io.script_read(PS2_STATUS_PORT, STATUS_OBF);
        io.script_read(PS2_DATA_PORT, 0x1E);

        // A released
        io.script_read(PS2_STATUS_PORT, STATUS_OBF);
        io.script_read(PS2_DATA_PORT, 0x1E | 0x80);

        // No more data
        io.script_read(PS2_STATUS_PORT, 0x00);

        let mut keyboard = X86Ps2Keyboard::new(io);

        let event1 = keyboard.poll_event().unwrap();
        assert_eq!(event1.scancode, HalScancode::Base(0x1E));
        assert!(event1.pressed);

        let event2 = keyboard.poll_event().unwrap();
        assert_eq!(event2.scancode, HalScancode::Base(0x1E));
        assert!(!event2.pressed);

        assert_eq!(keyboard.poll_event(), None);
    }

    #[test]
    fn test_x86_keyboard_arrow_keys() {
        let mut io = FakePortIo::new();

        // Up arrow (E0 0x48)
        io.script_read(PS2_STATUS_PORT, STATUS_OBF);
        io.script_read(PS2_DATA_PORT, SCANCODE_E0_PREFIX);
        io.script_read(PS2_STATUS_PORT, STATUS_OBF);
        io.script_read(PS2_DATA_PORT, 0x48);

        // Down arrow (E0 0x50)
        io.script_read(PS2_STATUS_PORT, STATUS_OBF);
        io.script_read(PS2_DATA_PORT, SCANCODE_E0_PREFIX);
        io.script_read(PS2_STATUS_PORT, STATUS_OBF);
        io.script_read(PS2_DATA_PORT, 0x50);

        // Left arrow (E0 0x4B)
        io.script_read(PS2_STATUS_PORT, STATUS_OBF);
        io.script_read(PS2_DATA_PORT, SCANCODE_E0_PREFIX);
        io.script_read(PS2_STATUS_PORT, STATUS_OBF);
        io.script_read(PS2_DATA_PORT, 0x4B);

        // Right arrow (E0 0x4D)
        io.script_read(PS2_STATUS_PORT, STATUS_OBF);
        io.script_read(PS2_DATA_PORT, SCANCODE_E0_PREFIX);
        io.script_read(PS2_STATUS_PORT, STATUS_OBF);
        io.script_read(PS2_DATA_PORT, 0x4D);

        let mut keyboard = X86Ps2Keyboard::new(io);

        // Up
        assert_eq!(keyboard.poll_event(), None); // E0
        let event = keyboard.poll_event().unwrap();
        assert_eq!(event.scancode, HalScancode::E0(0x48));

        // Down
        assert_eq!(keyboard.poll_event(), None); // E0
        let event = keyboard.poll_event().unwrap();
        assert_eq!(event.scancode, HalScancode::E0(0x50));

        // Left
        assert_eq!(keyboard.poll_event(), None); // E0
        let event = keyboard.poll_event().unwrap();
        assert_eq!(event.scancode, HalScancode::E0(0x4B));

        // Right
        assert_eq!(keyboard.poll_event(), None); // E0
        let event = keyboard.poll_event().unwrap();
        assert_eq!(event.scancode, HalScancode::E0(0x4D));
    }

    #[test]
    fn test_x86_keyboard_consecutive_e0() {
        let mut io = FakePortIo::new();

        // Two E0 prefixes in a row (unusual but should handle)
        io.script_read(PS2_STATUS_PORT, STATUS_OBF);
        io.script_read(PS2_DATA_PORT, SCANCODE_E0_PREFIX);
        io.script_read(PS2_STATUS_PORT, STATUS_OBF);
        io.script_read(PS2_DATA_PORT, SCANCODE_E0_PREFIX);
        io.script_read(PS2_STATUS_PORT, STATUS_OBF);
        io.script_read(PS2_DATA_PORT, 0x48);

        let mut keyboard = X86Ps2Keyboard::new(io);

        assert_eq!(keyboard.poll_event(), None); // First E0
        assert_eq!(keyboard.poll_event(), None); // Second E0
        let event = keyboard.poll_event().unwrap();
        assert_eq!(event.scancode, HalScancode::E0(0x48));
    }

    #[test]
    fn test_x86_keyboard_trait() {
        let mut io = FakePortIo::new();
        io.script_read(PS2_STATUS_PORT, 0x00);

        let mut keyboard: Box<dyn KeyboardDevice> = Box::new(X86Ps2Keyboard::new(io));
        assert!(keyboard.poll_event().is_none());
    }
}

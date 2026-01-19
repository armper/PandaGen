//! Port I/O abstraction for x86_64
//!
//! This module provides a trait-based abstraction for x86 port I/O operations,
//! allowing for both real hardware access and fake implementations for testing.
//!
//! ## Safety
//!
//! Port I/O operations are inherently unsafe as they directly interact with hardware.
//! Care must be taken to:
//! - Only access valid hardware ports
//! - Not interfere with other system components
//! - Follow proper timing constraints
//!
//! The `RealPortIo` implementation isolates all unsafe code to small, auditable functions.

/// Port I/O trait
///
/// Abstracts x86 I/O port operations to allow test doubles.
///
/// ## Implementation Notes
///
/// Implementations must guarantee:
/// - `inb` reads a byte from the specified port
/// - `outb` writes a byte to the specified port
/// - Operations complete synchronously before returning
pub trait PortIo {
    /// Reads a byte from an I/O port
    ///
    /// # Arguments
    ///
    /// * `port` - The port address to read from
    ///
    /// # Returns
    ///
    /// The byte value read from the port
    fn inb(&mut self, port: u16) -> u8;

    /// Writes a byte to an I/O port
    ///
    /// # Arguments
    ///
    /// * `port` - The port address to write to
    /// * `value` - The byte value to write
    fn outb(&mut self, port: u16, value: u8);
}

/// Real hardware port I/O implementation
///
/// Uses x86 `in` and `out` instructions to access hardware ports.
///
/// ## Safety
///
/// This implementation is only safe when:
/// - Running on x86/x86_64 architecture with proper privilege level
/// - Accessing ports that exist and are safe to access
/// - Not interfering with other drivers or system components
///
/// ## Example
///
/// ```rust,ignore
/// let mut io = RealPortIo::new();
/// let status = io.inb(0x64); // Read PS/2 status register
/// ```
#[derive(Debug, Default)]
pub struct RealPortIo;

impl RealPortIo {
    /// Creates a new real port I/O implementation
    pub fn new() -> Self {
        Self
    }
}

impl PortIo for RealPortIo {
    #[inline]
    fn inb(&mut self, port: u16) -> u8 {
        // SAFETY: This function performs raw port I/O which is inherently unsafe.
        // Callers must ensure:
        // 1. The port address is valid for the current hardware
        // 2. Reading from this port won't cause undefined behavior
        // 3. The code has sufficient privileges (ring 0 on x86)
        //
        // The inline assembly:
        // - Uses "in al, dx" instruction to read from port
        // - Port number is in DX register (input operand)
        // - Result byte is placed in AL register (output operand)
        // - Options "nomem" and "nostack" indicate no memory/stack side effects
        unsafe {
            let value: u8;
            core::arch::asm!(
                "in al, dx",
                in("dx") port,
                out("al") value,
                options(nomem, nostack, preserves_flags)
            );
            value
        }
    }

    #[inline]
    fn outb(&mut self, port: u16, value: u8) {
        // SAFETY: This function performs raw port I/O which is inherently unsafe.
        // Callers must ensure:
        // 1. The port address is valid for the current hardware
        // 2. Writing to this port won't cause undefined behavior or damage hardware
        // 3. The code has sufficient privileges (ring 0 on x86)
        //
        // The inline assembly:
        // - Uses "out dx, al" instruction to write to port
        // - Port number is in DX register (input operand)
        // - Byte value is in AL register (input operand)
        // - Options "nomem" and "nostack" indicate no memory/stack side effects
        unsafe {
            core::arch::asm!(
                "out dx, al",
                in("dx") port,
                in("al") value,
                options(nomem, nostack, preserves_flags)
            );
        }
    }
}

/// Fake port I/O implementation for testing
///
/// Allows scripted reads and captures writes for test verification.
///
/// ## Example
///
/// ```rust
/// use hal_x86_64::port_io::{FakePortIo, PortIo};
///
/// let mut io = FakePortIo::new();
/// io.script_read(0x64, 0x01); // Status: data available
/// io.script_read(0x60, 0x1E); // Data: scancode 0x1E
///
/// assert_eq!(io.inb(0x64), 0x01);
/// assert_eq!(io.inb(0x60), 0x1E);
///
/// // Verify that reads were consumed
/// assert_eq!(io.remaining_reads(), 0);
/// ```
#[derive(Debug, Default)]
pub struct FakePortIo {
    /// Scripted read values: (port, value)
    read_script: Vec<(u16, u8)>,
    /// Current read index
    read_index: usize,
    /// Captured write operations: (port, value)
    writes: Vec<(u16, u8)>,
}

impl FakePortIo {
    /// Creates a new fake port I/O implementation
    pub fn new() -> Self {
        Self {
            read_script: Vec::new(),
            read_index: 0,
            writes: Vec::new(),
        }
    }

    /// Scripts a read operation
    ///
    /// The next call to `inb(port)` with the specified port will return `value`.
    pub fn script_read(&mut self, port: u16, value: u8) {
        self.read_script.push((port, value));
    }

    /// Scripts multiple read operations
    pub fn script_reads(&mut self, reads: &[(u16, u8)]) {
        self.read_script.extend_from_slice(reads);
    }

    /// Returns the number of scripted reads remaining
    pub fn remaining_reads(&self) -> usize {
        self.read_script.len() - self.read_index
    }

    /// Returns all captured write operations
    pub fn writes(&self) -> &[(u16, u8)] {
        &self.writes
    }

    /// Clears all captured writes
    pub fn clear_writes(&mut self) {
        self.writes.clear();
    }

    /// Resets the read script (clears all reads and resets index)
    pub fn reset_reads(&mut self) {
        self.read_script.clear();
        self.read_index = 0;
    }
}

impl PortIo for FakePortIo {
    fn inb(&mut self, port: u16) -> u8 {
        if self.read_index >= self.read_script.len() {
            panic!(
                "FakePortIo: No scripted read for port 0x{:04X} (read_index={}, script_len={})",
                port,
                self.read_index,
                self.read_script.len()
            );
        }

        let (expected_port, value) = self.read_script[self.read_index];
        if port != expected_port {
            panic!(
                "FakePortIo: Port mismatch at read_index={}: expected 0x{:04X}, got 0x{:04X}",
                self.read_index, expected_port, port
            );
        }

        self.read_index += 1;
        value
    }

    fn outb(&mut self, port: u16, value: u8) {
        self.writes.push((port, value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fake_port_io_creation() {
        let io = FakePortIo::new();
        assert_eq!(io.remaining_reads(), 0);
        assert_eq!(io.writes().len(), 0);
    }

    #[test]
    fn test_fake_port_io_script_read() {
        let mut io = FakePortIo::new();
        io.script_read(0x60, 0x1E);
        io.script_read(0x64, 0x01);

        assert_eq!(io.remaining_reads(), 2);
        assert_eq!(io.inb(0x60), 0x1E);
        assert_eq!(io.remaining_reads(), 1);
        assert_eq!(io.inb(0x64), 0x01);
        assert_eq!(io.remaining_reads(), 0);
    }

    #[test]
    fn test_fake_port_io_script_reads() {
        let mut io = FakePortIo::new();
        io.script_reads(&[(0x64, 0x01), (0x60, 0x1E), (0x64, 0x00)]);

        assert_eq!(io.remaining_reads(), 3);
        assert_eq!(io.inb(0x64), 0x01);
        assert_eq!(io.inb(0x60), 0x1E);
        assert_eq!(io.inb(0x64), 0x00);
        assert_eq!(io.remaining_reads(), 0);
    }

    #[test]
    fn test_fake_port_io_write() {
        let mut io = FakePortIo::new();
        io.outb(0x60, 0xED);
        io.outb(0x60, 0x07);

        let writes = io.writes();
        assert_eq!(writes.len(), 2);
        assert_eq!(writes[0], (0x60, 0xED));
        assert_eq!(writes[1], (0x60, 0x07));
    }

    #[test]
    fn test_fake_port_io_clear_writes() {
        let mut io = FakePortIo::new();
        io.outb(0x60, 0xFF);
        assert_eq!(io.writes().len(), 1);

        io.clear_writes();
        assert_eq!(io.writes().len(), 0);
    }

    #[test]
    fn test_fake_port_io_reset_reads() {
        let mut io = FakePortIo::new();
        io.script_read(0x60, 0x1E);
        assert_eq!(io.remaining_reads(), 1);

        io.reset_reads();
        assert_eq!(io.remaining_reads(), 0);
    }

    #[test]
    #[should_panic(expected = "No scripted read")]
    fn test_fake_port_io_panic_on_unscripted_read() {
        let mut io = FakePortIo::new();
        io.inb(0x60); // Should panic: no scripted reads
    }

    #[test]
    #[should_panic(expected = "Port mismatch")]
    fn test_fake_port_io_panic_on_wrong_port() {
        let mut io = FakePortIo::new();
        io.script_read(0x64, 0x01);
        io.inb(0x60); // Should panic: expected port 0x64, got 0x60
    }

    #[test]
    fn test_fake_port_io_multiple_operations() {
        let mut io = FakePortIo::new();

        // Script reads
        io.script_read(0x64, 0x01);
        io.script_read(0x60, 0x1E);

        // Read and write
        assert_eq!(io.inb(0x64), 0x01);
        io.outb(0x64, 0xFF);
        assert_eq!(io.inb(0x60), 0x1E);
        io.outb(0x60, 0xAA);

        // Verify writes
        let writes = io.writes();
        assert_eq!(writes.len(), 2);
        assert_eq!(writes[0], (0x64, 0xFF));
        assert_eq!(writes[1], (0x60, 0xAA));
    }

    #[test]
    fn test_real_port_io_creation() {
        let io = RealPortIo::new();
        // Should create without panicking
        drop(io);
    }

    #[test]
    fn test_real_port_io_default() {
        let io = RealPortIo::default();
        drop(io);
    }

    // Note: We can't test actual port I/O operations without hardware
    // and proper privileges. The RealPortIo implementation is tested
    // through integration tests when running on real hardware or in
    // an emulator with proper setup.
}

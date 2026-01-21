//! Display sink abstraction for test-safe output
//!
//! This module provides a trait-based abstraction that allows us to run
//! tests without touching real hardware (VGA memory at 0xB8000).
//!
//! ## Design
//!
//! - `DisplaySink` trait: Common interface for display output
//! - `VgaDisplaySink`: Real VGA hardware (production)
//! - `TestDisplaySink`: In-memory buffer (testing)

#[cfg(feature = "console_vga")]
use console_vga::VgaConsole;

/// Trait for display output (VGA or in-memory buffer)
pub trait DisplaySink {
    /// Get display dimensions (cols, rows)
    fn dims(&self) -> (usize, usize);

    /// Clear the display with the given attribute
    fn clear(&mut self, attr: u8);

    /// Write a character at the given position
    fn write_at(&mut self, col: usize, row: usize, ch: u8, attr: u8) -> bool;

    /// Write a string at the given position
    fn write_str_at(&mut self, col: usize, row: usize, text: &str, attr: u8) -> usize;

    /// Draw a cursor at the given position (usually by inverting attributes)
    fn draw_cursor(&mut self, col: usize, row: usize, attr: u8);
}

/// VGA display sink for real hardware
#[cfg(feature = "console_vga")]
pub struct VgaDisplaySink<'a> {
    console: &'a mut VgaConsole,
}

#[cfg(feature = "console_vga")]
impl<'a> VgaDisplaySink<'a> {
    pub fn new(console: &'a mut VgaConsole) -> Self {
        Self { console }
    }
}

#[cfg(feature = "console_vga")]
impl<'a> DisplaySink for VgaDisplaySink<'a> {
    fn dims(&self) -> (usize, usize) {
        (console_vga::VGA_WIDTH, console_vga::VGA_HEIGHT)
    }

    fn clear(&mut self, attr: u8) {
        self.console.clear(attr);
    }

    fn write_at(&mut self, col: usize, row: usize, ch: u8, attr: u8) -> bool {
        self.console.write_at(col, row, ch, attr)
    }

    fn write_str_at(&mut self, col: usize, row: usize, text: &str, attr: u8) -> usize {
        self.console.write_str_at(col, row, text, attr)
    }

    fn draw_cursor(&mut self, col: usize, row: usize, attr: u8) {
        self.console.draw_cursor(col, row, attr);
    }
}

/// Test display sink using in-memory buffer
#[cfg(test)]
pub struct TestDisplaySink {
    /// In-memory buffer: 80x25 cells, 2 bytes per cell (char + attr)
    buffer: Vec<u8>,
    width: usize,
    height: usize,
}

#[cfg(test)]
impl TestDisplaySink {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            buffer: vec![0; width * height * 2],
            width,
            height,
        }
    }

    /// Get the character at a position (for testing)
    #[allow(dead_code)]
    pub fn get_char(&self, col: usize, row: usize) -> Option<u8> {
        if col >= self.width || row >= self.height {
            return None;
        }
        let offset = (row * self.width + col) * 2;
        Some(self.buffer[offset])
    }

    /// Get the attribute at a position (for testing)
    #[allow(dead_code)]
    pub fn get_attr(&self, col: usize, row: usize) -> Option<u8> {
        if col >= self.width || row >= self.height {
            return None;
        }
        let offset = (row * self.width + col) * 2;
        Some(self.buffer[offset + 1])
    }

    /// Get a line of text from the buffer (for testing)
    #[allow(dead_code)]
    pub fn get_line(&self, row: usize) -> Option<String> {
        if row >= self.height {
            return None;
        }
        let mut line = String::new();
        for col in 0..self.width {
            let offset = (row * self.width + col) * 2;
            let ch = self.buffer[offset];
            if ch == 0 || ch == b' ' {
                // Skip trailing spaces
                if !line.is_empty() || ch != b' ' {
                    line.push(ch as char);
                }
            } else {
                line.push(ch as char);
            }
        }
        Some(line.trim_end().to_string())
    }
}

#[cfg(test)]
impl DisplaySink for TestDisplaySink {
    fn dims(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    fn clear(&mut self, attr: u8) {
        for row in 0..self.height {
            for col in 0..self.width {
                let offset = (row * self.width + col) * 2;
                self.buffer[offset] = b' ';
                self.buffer[offset + 1] = attr;
            }
        }
    }

    fn write_at(&mut self, col: usize, row: usize, ch: u8, attr: u8) -> bool {
        if col >= self.width || row >= self.height {
            return false;
        }

        let offset = (row * self.width + col) * 2;
        self.buffer[offset] = ch;
        self.buffer[offset + 1] = attr;
        true
    }

    fn write_str_at(&mut self, mut col: usize, mut row: usize, text: &str, attr: u8) -> usize {
        let mut written = 0;

        for byte in text.bytes() {
            if byte == b'\n' {
                row += 1;
                col = 0;
                if row >= self.height {
                    break;
                }
                continue;
            }

            if col >= self.width {
                col = 0;
                row += 1;
            }

            if row >= self.height {
                break;
            }

            if self.write_at(col, row, byte, attr) {
                written += 1;
            }

            col += 1;
        }

        written
    }

    fn draw_cursor(&mut self, _col: usize, _row: usize, _attr: u8) {
        // No-op for test sink? Or simulated?
        // For now no-op is fine for tests unless we assert cursor position.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_sink_clear() {
        let mut sink = TestDisplaySink::new(80, 25);
        sink.clear(0x07);
        assert_eq!(sink.get_char(0, 0), Some(b' '));
        assert_eq!(sink.get_attr(0, 0), Some(0x07));
    }

    #[test]
    fn test_display_sink_write_at() {
        let mut sink = TestDisplaySink::new(80, 25);
        sink.clear(0x07);
        assert!(sink.write_at(5, 3, b'A', 0x0F));
        assert_eq!(sink.get_char(5, 3), Some(b'A'));
        assert_eq!(sink.get_attr(5, 3), Some(0x0F));
    }

    #[test]
    fn test_display_sink_write_str() {
        let mut sink = TestDisplaySink::new(80, 25);
        sink.clear(0x07);
        let written = sink.write_str_at(0, 0, "Hello", 0x0F);
        assert_eq!(written, 5);
        assert_eq!(sink.get_line(0), Some("Hello".to_string()));
    }

    #[test]
    fn test_display_sink_bounds() {
        let mut sink = TestDisplaySink::new(80, 25);
        assert!(!sink.write_at(80, 0, b'X', 0x07)); // Out of bounds
        assert!(!sink.write_at(0, 25, b'X', 0x07)); // Out of bounds
    }
}
